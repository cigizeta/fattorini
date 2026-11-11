use crate::board::chess_move::Move;
use crate::board::file::File;
use crate::board::piece_type::PieceType;
use crate::board::rank::Rank;
use crate::board::square::Square;
use crate::board::Board;
use crate::movegen::legal::legal_moves;
use crate::nnue::Network;
use crate::perft::perft;
use crate::search::{
    choose_best_move_multipv_with_progress, choose_best_move_with_parameters, choose_best_move_with_progress,
    root_reported_score, select_best_result, single_move_limit, time_limit, IterationReport, PonderState,
    SearchParameters, TimeManagement, DEFAULT_MOVE_OVERHEAD_MS, MATE_SCORE, MAX_PLY,
};
use crate::tablebase::{SyzygyTablebase, TbResult};
use crate::tt::TranspositionTable;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The FEN for the standard starting position -- what `position startpos` resolves to,
/// since UCI's `position` command itself only ever carries a literal FEN or the bare
/// keyword `startpos`.
const STARTPOS_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// The transposition table's size until a GUI sends `setoption name Hash value <MB>` to ask
/// for something else -- matches Stockfish's own real default (confirmed directly against a
/// real `stockfish` binary: `echo "uci" | stockfish` reports `option name Hash type spin
/// default 16 ...`), rather than an invented number. `pub`: `arena` gives every player this
/// size unless `--hash` says otherwise.
pub const DEFAULT_HASH_MB: usize = 16;

/// Bounds advertised on the `Hash` option in the `uci` response (`option name Hash type spin
/// default <DEFAULT_HASH_MB> min <HASH_MB_MIN> max <HASH_MB_MAX>`), the standard UCI spin-type
/// shape. `HASH_MB_MAX` is a sane ceiling for this project's own single-developer-machine
/// scope, not derived from anything more precise than that -- like `DEFAULT_MOVES_LEFT`, a
/// naive placeholder to revisit only if there's ever a real reason to raise it, not a measured
/// hardware limit. Purely informational either way: `setoption` itself (see
/// `parse_setoption`) doesn't clamp to this range, since respecting it is the GUI's own
/// responsibility per the UCI spec, and `TranspositionTable::new` already tolerates any
/// `usize` gracefully (including values outside it). `pub`: `arena --hash` refuses a value
/// outside this range, since Fattorini over UCI advertises it.
pub const HASH_MB_MIN: usize = 1;
pub const HASH_MB_MAX: usize = 1024;

/// The worker-thread count until a GUI sends `setoption name Threads value <N>` to ask for
/// something else. `1` reproduces today's exact single-threaded behavior with no Lazy SMP
/// diversity in play at all, the correct default for an option a GUI might never set.
const DEFAULT_THREADS: u32 = 1;

/// Bounds advertised on the `Threads` option in the `uci` response. `THREADS_MAX = 128` is
/// deliberately not tied to the developer's own current machine (`nproc` = 12) or to
/// Stockfish's own real `max 1024` (which reflects its willingness to run on server/cluster
/// hardware this project's single-commodity-machine scope explicitly excludes) -- generous
/// enough to cover even a 64-core/128-thread single-socket HEDT chip with headroom, without
/// reaching into server/datacenter territory. Purely informational, like `HASH_MB_MAX`:
/// `setoption` itself doesn't clamp to this range, since respecting it is the GUI's own
/// responsibility per the UCI spec.
const THREADS_MIN: u32 = 1;
const THREADS_MAX: u32 = 128;

/// How many root lines `go_search` reports until a GUI sends `setoption name MultiPV value <N>`
/// to ask for more. `1` reproduces today's exact single-line behavior (`choose_best_move_with_
/// progress`, not `choose_best_move_multipv_with_progress` -- see `go_search`'s own doc comment).
const DEFAULT_MULTIPV: u32 = 1;

/// Bounds advertised on the `MultiPV` option in the `uci` response -- `500`, matching Stockfish's
/// own commonly cited default max (not independently reconfirmed against a real `stockfish`
/// binary here the way `HASH_MB_MAX`/`DEFAULT_HASH_MB` were -- worth doing before relying on this
/// number for anything beyond the advertised default). Purely informational either way, like
/// `HASH_MB_MAX`/`THREADS_MAX`: `setoption` itself doesn't clamp to this range.
const MULTIPV_MIN: u32 = 1;
const MULTIPV_MAX: u32 = 500;

/// Bounds of the `Move Overhead` option, in milliseconds (its default is `DEFAULT_MOVE_OVERHEAD_MS`):
/// 0 for a caller that keeps a margin of its own, as lichess-bot does; 5000, Stockfish's own
/// maximum, for a link slow enough to need it. Unlike the bounds above, `setoption` clamps to
/// them: an overhead past them would only empty the clock.
const MOVE_OVERHEAD_MS_MIN: u64 = 0;
const MOVE_OVERHEAD_MS_MAX: u64 = 5000;
/// A ponder search's outcome, shared between `Engine` and the search's thread: still pondering,
/// `ponderhit` (the opponent played the expected move: the same search goes on under its clock and
/// prints the `bestmove` that is played), or `stop` (another move: print the ponder search's
/// `bestmove`, which the GUI ignores).
const PONDER_PENDING: u8 = 0;
const PONDER_HIT: u8 = 1;
const PONDER_STOPPED: u8 = 2;

/// How long a ponder search that finished on its own sleeps between checks of its outcome.
const PONDER_POLL: Duration = Duration::from_millis(1);

/// One parsed line of UCI input. `Position`'s `moves` are kept as raw text, not yet
/// resolved into `Move`s -- doing that needs a `Board` to generate legal moves against,
/// one move at a time as each is applied, which `parse_command` itself has no access to
/// and shouldn't need (it stays a plain function of the input line alone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UciCommand {
    Uci,
    IsReady,
    UciNewGame,
    /// `setoption name Hash value <MB>` -- the only option Fattorini currently supports, so this
    /// is specific to it rather than a generic `{ name: String, value: String }` shape; worth
    /// generalizing once a second option actually needs one, not before.
    SetOptionHash(usize),
    /// `setoption name Threads value <N>` -- the second option Fattorini supports, following the
    /// exact same pattern `SetOptionHash` already established (a dedicated variant per option,
    /// not yet a generic `{ name, value }` shape).
    SetOptionThreads(u32),
    /// `setoption name SyzygyPath value <dirs>` -- the third option, and the first whose value
    /// doesn't fit the fixed `["name", X, "value", single_token]` shape `Hash`/`Threads` share:
    /// real engines (Stockfish included) accept a *list* of directories in one `SyzygyPath`
    /// value, delimited by the OS path-list separator, since covering both the 3-4-5-piece and
    /// 6-piece sets means pointing at two directories at once (`syzygy.sh` downloads them
    /// separately). Already split into individual directory strings by `parse_setoption`
    /// (`std::env::split_paths`, the same OS-aware splitter `PATH` itself is parsed with) --
    /// `Engine::handle` just hands each one to `SyzygyTablebase::add_directory` in turn.
    SetOptionSyzygyPath(Vec<String>),
    /// `setoption name MultiPV value <N>` -- how many root lines `go_search` reports per
    /// completed depth, following the same dedicated-variant pattern `SetOptionThreads` already
    /// established.
    SetOptionMultiPv(u32),
    /// `setoption name Ponder value true|false`: whether `bestmove` offers a ponder move. Only `go
    /// ponder` makes Fattorini ponder; with no ponder move, a GUI has nothing to ponder on.
    SetOptionPonder(bool),
    /// `setoption name Move Overhead value <ms>`: the time kept back from every move's clock
    /// (`search::time_limit`). Its name is two tokens, as Stockfish's.
    SetOptionMoveOverhead(u64),
    /// `setoption name TimeManagement value Split|Classic`: which time management plans a clock
    /// move (`search::TimeManagement`). Temporary, for the duel of the two in one binary; removed
    /// with the loser.
    SetOptionTimeManagement(TimeManagement),
    Position { fen: String, moves: Vec<String> },
    /// A plain `go`, or `go perft <depth>` when `perft_depth` is `Some`. The time-control
    /// fields mirror UCI's own tokens one-to-one (milliseconds, as the protocol sends them)
    /// with zero interpretation here -- selecting which of `wtime`/`btime` applies and
    /// computing an actual time target are all `search.rs`'s
    /// job, not this interface layer's. `depth` is carried the same way -- raw, uninterpreted
    /// -- capping iterative deepening is `go_search`'s job, same division of labor as the time
    /// controls. `nodes`, `mate`, and `searchmoves` remain silently accepted but ignored --
    /// there's no node/mate-bounded search or restricted move list yet for them to configure.
    /// `ponder` searches with no clock until `ponderhit` or `stop`; the time fields then apply to
    /// the search `ponderhit` starts.
    Go {
        perft_depth: Option<u32>,
        depth: Option<u32>,
        wtime: Option<u64>,
        btime: Option<u64>,
        winc: Option<u64>,
        binc: Option<u64>,
        movestogo: Option<u32>,
        movetime: Option<u64>,
        infinite: bool,
        ponder: bool,
    },
    /// `ponderhit`: the opponent played the move being pondered on. A no-op if no ponder search is
    /// running.
    PonderHit,
    /// Tells whatever search is currently running to stop as soon as it can and report its
    /// best move so far -- a no-op if nothing is running. Previously fell through to `Unknown`
    /// and was silently ignored, since `go` used to block the whole command loop until it
    /// finished on its own, leaving nothing here to actually stop.
    Stop,
    Quit,
    /// Anything not recognized above -- an unknown command, or a known one with malformed
    /// arguments (e.g. `position fen` with fewer than six fields). Per the UCI protocol
    /// spec's own robustness requirement ("if the engine... receives an unknown command
    /// or token it should just ignore it"), this is never an error: the caller simply
    /// does nothing for this line and moves on to the next.
    Unknown,
}

/// Parses one line of UCI input into a `UciCommand`. Whitespace-tokenized, no grammar
/// beyond that -- UCI is a flat, single-line-per-command protocol (unlike, say, PGN's
/// much larger surrounding structure of headers, move numbers, comments, and nested
/// variations), so splitting on whitespace and matching the leading token is the entire
/// job.
pub fn parse_command(line: &str) -> UciCommand {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    match tokens.first() {
        Some(&"uci") => UciCommand::Uci,
        Some(&"isready") => UciCommand::IsReady,
        Some(&"ucinewgame") => UciCommand::UciNewGame,
        Some(&"setoption") => parse_setoption(&tokens[1..]),
        Some(&"position") => parse_position(&tokens[1..]),
        Some(&"go") => parse_go(&tokens[1..]),
        Some(&"stop") => UciCommand::Stop,
        Some(&"ponderhit") => UciCommand::PonderHit,
        Some(&"quit") => UciCommand::Quit,
        _ => UciCommand::Unknown,
    }
}

/// Parses everything after `position`: either `startpos` or `fen <6 fields>`, then an
/// optional trailing `moves <m1> <m2> ...`.
fn parse_position(tokens: &[&str]) -> UciCommand {
    match tokens.first() {
        Some(&"startpos") => parse_moves_tail(STARTPOS_FEN.to_string(), &tokens[1..]),
        Some(&"fen") => {
            // A FEN is always exactly 6 whitespace-separated fields (CLAUDE.md rule 2's
            // "standard FEN strings" -- `Board::from_fen` itself enforces this same
            // count), so anything short of that is malformed rather than a shorter valid
            // FEN some other tool might accept.
            if tokens.len() < 7 {
                return UciCommand::Unknown;
            }
            parse_moves_tail(tokens[1..7].join(" "), &tokens[7..])
        }
        _ => UciCommand::Unknown,
    }
}

/// Parses an optional trailing `moves <m1> <m2> ...` after a position's already been
/// resolved to `fen`; an absent or unrecognized tail just means no moves to replay.
fn parse_moves_tail(fen: String, tokens: &[&str]) -> UciCommand {
    let moves = match tokens.first() {
        Some(&"moves") => tokens[1..].iter().map(|s| s.to_string()).collect(),
        _ => Vec::new(),
    };
    UciCommand::Position { fen, moves }
}

/// Parses everything after `setoption`. `Hash`/`Threads` still match their exact four-token
/// shape (`name <opt> value <single-token>`) directly -- simpler than a general name/value
/// splitter for just two fixed-shape options. `SyzygyPath` can't follow that same shape: its
/// value is a list of directories, so `parse_syzygy_path_value` gets everything after `value`
/// as a slice, not one token. Any other option name, a recognized name with no `value`, or a
/// non-numeric `Hash`/`Threads` value all fall through to `Unknown`, per the same "ignore
/// anything not recognized" robustness rule every other malformed command already follows.
fn parse_setoption(tokens: &[&str]) -> UciCommand {
    match tokens {
        ["name", "Hash", "value", mb] => match mb.parse::<usize>() {
            Ok(mb) => UciCommand::SetOptionHash(mb),
            Err(_) => UciCommand::Unknown,
        },
        ["name", "Threads", "value", n] => match n.parse::<u32>() {
            Ok(n) => UciCommand::SetOptionThreads(n),
            Err(_) => UciCommand::Unknown,
        },
        ["name", "SyzygyPath", "value", rest @ ..] if !rest.is_empty() => parse_syzygy_path_value(rest),
        ["name", "MultiPV", "value", n] => match n.parse::<u32>() {
            Ok(n) => UciCommand::SetOptionMultiPv(n),
            Err(_) => UciCommand::Unknown,
        },
        ["name", "Ponder", "value", "true"] => UciCommand::SetOptionPonder(true),
        ["name", "Ponder", "value", "false"] => UciCommand::SetOptionPonder(false),
        ["name", "Move", "Overhead", "value", ms] => match ms.parse::<u64>() {
            Ok(ms) => UciCommand::SetOptionMoveOverhead(ms),
            Err(_) => UciCommand::Unknown,
        },
        ["name", "TimeManagement", "value", "Split"] => UciCommand::SetOptionTimeManagement(TimeManagement::Split),
        ["name", "TimeManagement", "value", "Classic"] => UciCommand::SetOptionTimeManagement(TimeManagement::Classic),
        _ => UciCommand::Unknown,
    }
}

/// Rejoins `SyzygyPath`'s value tokens (already whitespace-split by `parse_command`) with a
/// single space each, then splits on the OS path-list separator (`;`) via `std::env::split_paths` -- the same splitter `PATH` itself is parsed with,
/// rather than a hand-rolled one. This loses any *extra* whitespace an original directory name
/// might have had (`split_whitespace` in `parse_command` already collapsed that before this
/// function ever sees it), an accepted limitation: a path containing multiple consecutive
/// spaces is exotic enough not to design around here.
fn parse_syzygy_path_value(rest: &[&str]) -> UciCommand {
    let joined = rest.join(" ");
    let dirs: Vec<String> = std::env::split_paths(&joined).map(|path| path.to_string_lossy().into_owned()).collect();
    UciCommand::SetOptionSyzygyPath(dirs)
}

/// Parses everything after `go`. `perft <depth>` is handled separately -- a distinct
/// testing command, its own fixed-position argument, never sent alongside real time
/// control. Otherwise scans the remaining tokens for whichever recognized keywords are
/// present: UCI allows `go`'s arguments in any order and any combination (unlike `perft`'s
/// fixed position), so this can't just index fixed slots the way `parse_position` does.
/// `depth` is stored (`go_search` caps iterative deepening at it); `nodes`/`mate` are
/// recognized only well enough to skip their numeric value correctly (so it doesn't get
/// misread as an unrelated keyword); `searchmoves` (and the move list after it) falls through
/// to the catch-all one-token skip -- none of those three are stored anywhere, since there's no
/// node/mate-bounded search or restricted move list yet for them to configure. `ponder` is a
/// flag with no value.
fn parse_go(tokens: &[&str]) -> UciCommand {
    if tokens.first() == Some(&"perft") {
        return match tokens.get(1).and_then(|text| text.parse::<u32>().ok()) {
            Some(depth) => UciCommand::Go {
                perft_depth: Some(depth),
                depth: None,
                wtime: None,
                btime: None,
                winc: None,
                binc: None,
                movestogo: None,
                movetime: None,
                infinite: false,
                ponder: false,
            },
            None => UciCommand::Unknown,
        };
    }

    let mut depth = None;
    let mut wtime = None;
    let mut btime = None;
    let mut winc = None;
    let mut binc = None;
    let mut movestogo = None;
    let mut movetime = None;
    let mut infinite = false;
    let mut ponder = false;

    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            "depth" => {
                depth = tokens.get(i + 1).and_then(|text| text.parse().ok());
                i += 2;
            }
            "wtime" => {
                wtime = tokens.get(i + 1).and_then(|text| text.parse().ok());
                i += 2;
            }
            "btime" => {
                btime = tokens.get(i + 1).and_then(|text| text.parse().ok());
                i += 2;
            }
            "winc" => {
                winc = tokens.get(i + 1).and_then(|text| text.parse().ok());
                i += 2;
            }
            "binc" => {
                binc = tokens.get(i + 1).and_then(|text| text.parse().ok());
                i += 2;
            }
            "movestogo" => {
                movestogo = tokens.get(i + 1).and_then(|text| text.parse().ok());
                i += 2;
            }
            "movetime" => {
                movetime = tokens.get(i + 1).and_then(|text| text.parse().ok());
                i += 2;
            }
            "infinite" => {
                infinite = true;
                i += 1;
            }
            "ponder" => {
                ponder = true;
                i += 1;
            }
            "nodes" | "mate" => i += 2,
            _ => i += 1,
        }
    }

    UciCommand::Go { perft_depth: None, depth, wtime, btime, winc, binc, movestogo, movetime, infinite, ponder }
}

/// Parses a two-character square (`"e4"`), the shape every UCI move's `from`/`to` half
/// uses. Shared with `parse_move` rather than duplicated per half.
fn parse_square(text: &str) -> Option<Square> {
    let mut chars = text.chars();
    let file = File::from_char(chars.next()?)?;
    let rank = Rank::from_char(chars.next()?)?;
    if chars.next().is_some() {
        return None;
    }
    Some(Square::new(file, rank))
}

/// The `PieceType` for a UCI promotion letter (`'q'`, `'r'`, `'b'`, `'n'`) -- always
/// lowercase regardless of color, unlike FEN's case-encodes-color convention (see
/// `fen.rs`'s `piece_from_fen_char`).
fn promotion_piece_type_from_char(c: char) -> Option<PieceType> {
    match c {
        'q' => Some(PieceType::Queen),
        'r' => Some(PieceType::Rook),
        'b' => Some(PieceType::Bishop),
        'n' => Some(PieceType::Knight),
        _ => None,
    }
}

/// The UCI promotion letter for `piece_type`, the inverse of
/// `promotion_piece_type_from_char`. Only ever called with a `Move::promotion()` value,
/// which is always `Queen`/`Rook`/`Bishop`/`Knight` when `Some` -- a pawn can't promote
/// into a pawn or a king.
fn promotion_char_from_piece_type(piece_type: PieceType) -> char {
    match piece_type {
        PieceType::Queen => 'q',
        PieceType::Rook => 'r',
        PieceType::Bishop => 'b',
        PieceType::Knight => 'n',
        PieceType::Pawn | PieceType::King => {
            unreachable!("a move's promotion field is never a pawn or a king")
        }
    }
}

/// Parses a UCI move string (`"e2e4"`, `"e7e8q"`) against `legal_moves`, returning
/// whichever candidate matches by `from`/`to`/promotion. Matching against already-
/// generated legal moves, rather than reconstructing `MoveKind` from the string by hand,
/// is what lets a plain four- or five-character string resolve into a castle or an en
/// passant capture correctly -- the string alone can't distinguish those from an ordinary
/// move (e.g. `"e1g1"` is only a castle because a legal-move candidate with that exact
/// from/to happens to carry `MoveKind::Castle`), but the position's own legal moves
/// already know.
pub fn parse_move(text: &str, legal_moves: &[Move]) -> Option<Move> {
    if text.len() != 4 && text.len() != 5 {
        return None;
    }

    let from = parse_square(&text[0..2])?;
    let to = parse_square(&text[2..4])?;
    let promotion = match text.len() {
        5 => Some(promotion_piece_type_from_char(text[4..5].chars().next().unwrap())?),
        _ => None,
    };

    legal_moves.iter().copied().find(|mv| mv.from() == from && mv.to() == to && mv.promotion() == promotion)
}

/// Formats `mv` as a UCI move string, the inverse of `parse_move`. Needed for `bestmove`
/// and for `go perft`'s per-move divide output.
pub fn format_move(mv: Move) -> String {
    let mut text = format!(
        "{}{}{}{}",
        mv.from().file().to_char(),
        mv.from().rank().to_char(),
        mv.to().file().to_char(),
        mv.to().rank().to_char()
    );
    if let Some(promotion) = mv.promotion() {
        text.push(promotion_char_from_piece_type(promotion));
    }
    text
}

/// Formats a raw search score as UCI's `score` field: `cp <x>` for an ordinary evaluation,
/// `mate <y>` for a forced mate. A genuine mate score is always exactly `MATE_SCORE` minus
/// the mating ply (`0..MAX_PLY`), so its magnitude always falls in the narrow band
/// `(MATE_SCORE - MAX_PLY, MATE_SCORE]` -- comfortably clear of anything the current
/// material-only evaluation could ever produce, so no real score can be mistaken for one.
/// `y` is in moves, not plies (UCI's own unit, not the engine's internal one): `ply_at_mate`
/// plies means `(ply_at_mate + 1) / 2` moves by whichever side actually delivers the mate,
/// an integer-division formula that happens to work for both parities (ply 1 or 2 -> 1 move,
/// ply 3 or 4 -> 2 moves, ...). Negative when the engine's own side is the one getting mated
/// -- which is exactly when `score` itself is already negative, so the sign carries over
/// unchanged.
fn format_score(score: i32) -> String {
    match split_score(score) {
        (_, Some(moves_to_mate)) => format!("mate {moves_to_mate}"),
        (Some(cp), _) => format!("cp {cp}"),
        (None, None) => unreachable!("split_score always reports one of the two"),
    }
}

/// The same classification `format_score` applies, as a pair rather than as text: `(Some(cp),
/// None)` for an ordinary evaluation, `(None, Some(moves))` for a forced mate, never both and never
/// neither. Public because a caller driving `Search` in-process needs exactly this split without
/// going through UCI's own text -- and because the mate band above is a rule that must have one
/// definition, not a second copy that could drift from this one.
pub fn split_score(score: i32) -> (Option<i32>, Option<i32>) {
    if score.abs() > MATE_SCORE - MAX_PLY as i32 {
        let ply_at_mate = MATE_SCORE - score.abs();
        let moves_to_mate = (ply_at_mate + 1) / 2;
        return (None, Some(if score > 0 { moves_to_mate } else { -moves_to_mate }));
    }
    (Some(score), None)
}

/// Formats one completed iterative-deepening depth's own `info` line. Used by `go_search`'s
/// live, per-depth callback for thread `0` -- the only place any `info` line is ever built now,
/// since only thread `0`'s own progress is ever reported live (see `go_search`'s own doc
/// comment for why every other thread stays silent). `hashfull` is read from the shared table
/// at that same call site (`TranspositionTable::hashfull`'s own doc comment), not computed here
/// -- this function only ever formats data it's handed, never reaches back into search state.
/// `root` is the root's tablebase result, `None` outside the tables: the score shown goes through
/// `root_reported_score`, the report's own score staying the search's.
fn format_info_line(r: &IterationReport, root: Option<TbResult>, hashfull: u32) -> String {
    let pv = r.pv.iter().map(|&mv| format_move(mv)).collect::<Vec<_>>().join(" ");
    // nps: multiply by 1000 before dividing, not the other way around, so a sub-second
    // time_ms doesn't get truncated to 0 first; .max(1) guards the division itself against a
    // time_ms of 0 (an extremely fast depth-1 completion).
    let nps = r.nodes * 1000 / r.time_ms.max(1);
    let score = format_score(root_reported_score(r.score, root));
    format!(
        "info depth {} seldepth {} score {score} nodes {} nps {} hashfull {hashfull} time {} pv {pv}",
        r.depth, r.seldepth, r.nodes, nps, r.time_ms
    )
}

/// `format_info_line`, extended with a `multipv <rank>` field right after `seldepth` (Stockfish's
/// own convention for where it belongs in the line), for one rank of a `MultiPV > 1` search.
/// `depth`/`seldepth`/`nodes`/`time_ms` are shared across every rank of one completed iteration
/// (`choose_best_move_multipv_with_progress`'s own doc comment covers why -- they describe the
/// whole iteration's effort, not one rank's own); only `score`/`pv` actually differ per rank, so
/// those are this function's own parameters rather than read off a single `IterationReport` the
/// way `format_info_line` does. `report` still supplies the shared fields either way -- rank `1`'s
/// own `score`/`pv` happen to equal `report.score`/`report.pv`, but are still passed explicitly
/// rather than special-cased, so every rank (including the first) goes through the same call
/// shape.
fn format_multipv_info_line(report: &IterationReport, rank: u32, score: i32, pv: &[Move], hashfull: u32) -> String {
    let pv_str = pv.iter().map(|&mv| format_move(mv)).collect::<Vec<_>>().join(" ");
    let nps = report.nodes * 1000 / report.time_ms.max(1);
    let score_str = format_score(score);
    format!(
        "info depth {} seldepth {} multipv {rank} score {score_str} nodes {} nps {nps} hashfull {hashfull} time {} pv {pv_str}",
        report.depth, report.seldepth, report.nodes, report.time_ms
    )
}

/// The engine's own live state: just the one `Board` currently in play. Logic lives here,
/// entirely separate from actual stdin/stdout (rule 4: assertions, not eyeballing) --
/// `handle` returns response lines as data, so every command's behavior is checkable by
/// asserting on a returned `Vec<String>`, no live process needed.
pub struct Engine {
    board: Board,
    /// Every position reached so far in the real, actually-played game, oldest first,
    /// *never* including `board` itself -- rebuilt from scratch by `set_position` every time,
    /// the same way `board` itself is. Seeds `Search::ancestors` (see its own doc comment) at
    /// `go` time, so repetition detection sees the real game's own lineage, not just whatever
    /// moves this particular search happens to make.
    ancestors: Vec<u64>,
    /// Persists across every `go` within one game -- unlike `board`/`ancestors`, which reset
    /// per position, a transposition table's whole value is remembering positions from
    /// earlier in the *same* game (or reached by transposition from the current search),
    /// so it's only cleared on `UciNewGame`, not on every `position`/`go`.
    ///
    /// `Arc`, not a plain `TranspositionTable`: `go_search`'s own real work now runs on a
    /// background thread that outlives the `handle` call spawning it (see `go_search`'s own doc
    /// comment), so it needs a handle to this table that stays valid independent of `Engine`'s
    /// own lifetime -- an `Arc` clone is exactly that, cheap (a refcount bump, never a copy of
    /// the table's own, potentially large, backing storage), and keeps `TranspositionTable`'s
    /// own already-lock-free `probe`/`store` completely unchanged.
    tt: Arc<TranspositionTable>,
    /// How many worker threads `go_search` spawns -- persists across `position`/`go` the same
    /// way `tt`'s own size does, only changed by an explicit `setoption name Threads`.
    threads: u32,
    /// How many root lines `go_search` reports per completed depth -- persists the same way
    /// `threads` does, only changed by an explicit `setoption name MultiPV`. `1` (`DEFAULT_
    /// MULTIPV`) makes `go_search` call `choose_best_move_with_progress` exactly as it always
    /// has; anything greater switches thread 0 to `choose_best_move_multipv_with_progress`
    /// instead (`go_search`'s own doc comment).
    multipv: u32,
    /// The `Ponder` option: whether `bestmove` offers a ponder move. False until a GUI sets it.
    ponder: bool,
    /// The `Move Overhead` option, in milliseconds: `DEFAULT_MOVE_OVERHEAD_MS` until a GUI sets
    /// it, kept across `ucinewgame` as `Hash` is.
    move_overhead_ms: u64,
    /// The `TimeManagement` option: `Split` until a GUI sets it, kept across `ucinewgame`.
    time_management: TimeManagement,
    /// The most recently started search's own background thread, if it (or an earlier one)
    /// hasn't been joined yet -- `None` once joined, or before any `go` has ever run. See
    /// `join_running_search`'s own doc comment for when and why this gets joined.
    running_search: Option<RunningSearch>,
    /// Shared by every writer to stdout: `go_search`'s own background thread (printing its
    /// `info`/`bestmove` lines directly, per `go_search`'s own doc comment) and `run()`'s loop
    /// (printing every other command's reply) both take this before writing, so one writer's
    /// lines can never interleave with the other's mid-line. `Arc` because the background
    /// thread needs its own handle that outlives this call, same reasoning as `tt`.
    stdout_lock: Arc<Mutex<()>>,
    /// The most recently started search's own `info`/`bestmove` lines, pushed here one at a
    /// time (each `info` line live, as thread 0's own callback produces it; the final
    /// `bestmove` line once the whole search concludes) in addition to being printed for real.
    /// Never read by production code (`run()` only cares about the lines actually reaching
    /// stdout, which happens independently of this) -- it exists purely so tests can assert on
    /// a search's real output deterministically (rule 4: assertions, not eyeballing) despite
    /// that output now being produced asynchronously, incrementally, rather than returned
    /// directly from `handle`. See `join_and_take_last_search_output`'s own doc comment for how
    /// a test actually reads it.
    last_search_output: Arc<Mutex<Vec<String>>>,
    /// Loaded Syzygy WDL tables, empty (probes nothing, `probe` always returns `None`) until a
    /// `setoption name SyzygyPath` arrives. Rebuilt from scratch on every such option, the same
    /// "replace, don't accumulate" convention `SetOptionHash` already uses for `tt` -- a second
    /// `SyzygyPath` value means "these are the directories now," not "also load these."
    ///
    /// `Arc`, for the identical reason `tt` is one: `go_search`'s background thread now probes
    /// this directly (`choose_best_move`/`choose_best_move_with_progress`'s own `tablebase`
    /// parameter), and that thread outlives the `handle` call spawning it -- see `tt`'s own doc
    /// comment for the full reasoning, which applies here unchanged.
    tablebase: Arc<SyzygyTablebase>,
    /// The network every search evaluates with, the embedded one (`embedded_evaluator`); `None` is
    /// the PeSTO evaluation. `Arc` for the same reason as `tt`: the search's background thread
    /// outlives the `handle` call that spawns it.
    evaluator: Option<Arc<Network>>,
    /// The search parameters every search uses: the embedded ones (`embedded_parameters`), then any
    /// field a `setoption` of its UCI name sets.
    parameters: SearchParameters,
    /// What the build is (`embedded_description`), said in `id name`; empty for a development build.
    description: String,
    /// Whether the next `go` pins this process to a core group (`core_pinning.rs`): set by `run`
    /// under `--lichess`, cleared by the first `go`, so the engine pins at most once. At a `go`,
    /// not at startup, because the group's size comes from `Threads`, sent before the first `go`.
    pin_at_next_go: bool,
    /// The final score of this engine's last played search in the game, from its own side: the
    /// previous move's score the clock target's falling-score factor compares against
    /// (`TimeLimit::with_previous_score`). Cleared by `ucinewgame`; never written by a ponder search
    /// the GUI stopped, whose move is not played. `Arc` for the same reason as `tt`.
    last_score: Arc<Mutex<Option<i32>>>,
}

/// One search still running (or just finished but not yet joined) in the background, spawned by
/// `go_search`. See `join_running_search`'s own doc comment for why `Engine` needs to track this
/// at all, rather than just fire-and-forget the thread.
struct RunningSearch {
    /// Shared with the background thread itself -- `UciCommand::Stop`'s entire job is flipping
    /// this to `true`; the thread notices on its own next poll (the same `CHECK_INTERVAL`
    /// mechanism built for Lazy SMP) and unwinds from wherever it currently is.
    stop_signal: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
    /// A ponder search's outcome (`PONDER_PENDING` and the two after it) and the state its thread 0
    /// reads, which `ponderhit` switches to the clock; `None` for any other search.
    ponder: Option<(Arc<AtomicU8>, Arc<PonderState>)>,
}

impl Default for Engine {
    fn default() -> Engine {
        Engine::new()
    }
}

impl Engine {
    /// A fresh engine, positioned at the standard starting position -- the same default
    /// a real UCI session begins with before any `position` command arrives.
    pub fn new() -> Engine {
        let (board, _) = Board::from_fen(STARTPOS_FEN).unwrap();
        Engine {
            board,
            ancestors: Vec::new(),
            tt: Arc::new(TranspositionTable::new(DEFAULT_HASH_MB)),
            threads: DEFAULT_THREADS,
            multipv: DEFAULT_MULTIPV,
            ponder: false,
            move_overhead_ms: DEFAULT_MOVE_OVERHEAD_MS,
            time_management: TimeManagement::Split,
            running_search: None,
            stdout_lock: Arc::new(Mutex::new(())),
            last_search_output: Arc::new(Mutex::new(Vec::new())),
            tablebase: Arc::new(SyzygyTablebase::new()),
            evaluator: None,
            parameters: SearchParameters::DEFAULT,
            description: String::new(),
            pin_at_next_go: false,
            last_score: Arc::new(Mutex::new(None)),
        }
    }

    /// `new`, playing what the build embeds: its network, its parameters, its description.
    pub fn embedded() -> Engine {
        let mut engine = Engine::new();
        engine.evaluator = embedded_evaluator();
        engine.parameters = embedded_parameters();
        engine.description = embedded_description();
        engine
    }

    /// Waits for a previous search's own background thread to fully finish, if one hasn't been
    /// joined yet -- needed before anything that requires exclusive (`&mut`) access to `self.tt`
    /// through its `Arc` (see `tt`'s own doc comment), since `Arc::get_mut` only ever succeeds
    /// once every other clone -- including the one that thread holds for the length of its own
    /// search -- has actually been dropped. Rather than lean on GUI-protocol timing (a
    /// well-behaved GUI won't send a new `go`/`ucinewgame` before receiving the previous
    /// search's `bestmove`, but that says nothing about exactly when that search's own thread
    /// finishes tearing down and drops its `Arc` clone, which is a race, not a guarantee), this
    /// makes it an explicit synchronization point instead. In the well-behaved-GUI case this
    /// always returns immediately: the previous thread has already finished and printed its own
    /// `bestmove` by the time anything here calls this.
    ///
    /// A ponder search still pondering would wait forever for a `ponderhit` or `stop` that a GUI
    /// breaking the protocol never sends, so it is stopped first, as `stop` would.
    fn join_running_search(&mut self) {
        if let Some(running) = self.running_search.take() {
            if let Some((outcome, _)) = &running.ponder
                && outcome.compare_exchange(PONDER_PENDING, PONDER_STOPPED, Ordering::SeqCst, Ordering::SeqCst).is_ok()
            {
                running.stop_signal.store(true, Ordering::Relaxed);
            }
            running.thread.join().expect("a go_search background thread panicked");
        }
    }

    /// Called right before the process actually exits on `quit` -- signals any still-running
    /// search to stop, then waits for its background thread to actually finish, so the process
    /// never exits out from under a thread still mid-write to stdout.
    fn shutdown(&mut self) {
        if let Some(running) = &self.running_search {
            running.stop_signal.store(true, Ordering::Relaxed);
        }
        self.join_running_search();
    }

    /// Test-only: waits for the most recent `go`'s background thread to fully finish, then
    /// returns what it actually printed (see `last_search_output`'s own doc comment for why
    /// this exists at all -- `handle` itself can no longer return a search's real output
    /// directly, since it's produced asynchronously now). Joining first, rather than just
    /// reading the buffer, guarantees the thread has already written its final lines: `join`
    /// only returns once the closure has fully run, which includes that write.
    #[cfg(test)]
    fn join_and_take_last_search_output(&mut self) -> Vec<String> {
        self.join_running_search();
        return std::mem::take(&mut *self.last_search_output.lock().unwrap());
    }

    /// Executes one parsed command against this engine's state, returning the lines to
    /// print in response (empty when a command produces no reply, e.g. `position`).
    pub fn handle(&mut self, command: UciCommand) -> Vec<String> {
        match command {
            UciCommand::Uci => {
                vec![
                    // Named by the workspace's version, which every crate inherits, then by what the
                    // build embeds, so a GUI and Lichess show which snapshot plays.
                    if self.description.is_empty() {
                        format!("id name Fattorini {}", env!("CARGO_PKG_VERSION"))
                    } else {
                        format!("id name Fattorini {} {}", env!("CARGO_PKG_VERSION"), self.description)
                    },
                    "id author Calogero Zarba".to_string(),
                    format!("option name Hash type spin default {DEFAULT_HASH_MB} min {HASH_MB_MIN} max {HASH_MB_MAX}"),
                    format!(
                        "option name Threads type spin default {DEFAULT_THREADS} min {THREADS_MIN} max {THREADS_MAX}"
                    ),
                    "option name SyzygyPath type string default <empty>".to_string(),
                    format!(
                        "option name MultiPV type spin default {DEFAULT_MULTIPV} min {MULTIPV_MIN} max {MULTIPV_MAX}"
                    ),
                    "option name Ponder type check default false".to_string(),
                    format!(
                        "option name Move Overhead type spin default {DEFAULT_MOVE_OVERHEAD_MS} min {MOVE_OVERHEAD_MS_MIN} max {MOVE_OVERHEAD_MS_MAX}"
                    ),
                    "option name TimeManagement type combo default Split var Split var Classic".to_string(),
                    "uciok".to_string(),
                ]
            }
            UciCommand::IsReady => vec!["readyok".to_string()],
            UciCommand::SetOptionHash(mb) => {
                // A full rebuild, not a resize in place -- real engines do the same (Stockfish
                // itself frees and reallocates on a `Hash` change), since the table's whole
                // shape (slot count, mask) depends on the size, and every existing entry was
                // indexed for the *old* size's mask. Losing whatever was already cached is an
                // accepted, unavoidable cost of resizing mid-game, not a bug.
                self.tt = Arc::new(TranspositionTable::new(mb));
                Vec::new()
            }
            UciCommand::SetOptionThreads(n) => {
                // Takes effect on the very next go_search call, which reads self.threads fresh
                // each time rather than caching a worker count anywhere else.
                self.threads = n;
                Vec::new()
            }
            UciCommand::SetOptionMultiPv(n) => {
                // Same "read fresh on every go_search call" convention as SetOptionThreads.
                self.multipv = n;
                Vec::new()
            }
            UciCommand::SetOptionPonder(ponder) => {
                self.ponder = ponder;
                Vec::new()
            }
            UciCommand::SetOptionMoveOverhead(ms) => {
                self.move_overhead_ms = ms.clamp(MOVE_OVERHEAD_MS_MIN, MOVE_OVERHEAD_MS_MAX);
                Vec::new()
            }
            UciCommand::SetOptionTimeManagement(management) => {
                self.time_management = management;
                Vec::new()
            }
            UciCommand::SetOptionSyzygyPath(dirs) => {
                // A fresh SyzygyTablebase, not a mutation of the existing one -- see this
                // field's own doc comment for why a second SyzygyPath replaces rather than
                // accumulates. A directory that doesn't exist or holds no recognized table
                // files is silently skipped (add_directory's own Result, ignored) -- stdout is
                // UCI-only (rule 8), so there is nowhere to report it, and a probe against an
                // empty or partially-loaded table set just returns None rather than misbehaving.
                let mut tablebase = SyzygyTablebase::new();
                for dir in &dirs {
                    let _ = tablebase.add_directory(dir);
                }
                self.tablebase = Arc::new(tablebase);
                Vec::new()
            }
            UciCommand::UciNewGame => {
                let (board, _) = Board::from_fen(STARTPOS_FEN).unwrap();
                self.board = board;
                self.ancestors = Vec::new();
                // A new game means every remembered position is from an unrelated game and
                // must not be trusted -- but the table's own size is a separate setting
                // (`setoption name Hash`) that a fresh game has no reason to disturb, so this
                // clears entries in place rather than rebuilding the table. `clear` needs `&mut
                // TranspositionTable`, which `Arc::get_mut` only hands out with no outstanding
                // clone anywhere -- see `join_running_search`'s own doc comment for why this is
                // called first rather than assumed.
                self.join_running_search();
                Arc::get_mut(&mut self.tt)
                    .expect("no other Arc<TranspositionTable> clone should outlive a go_search call")
                    .clear();
                *self.last_score.lock().unwrap() = None;
                Vec::new()
            }
            UciCommand::Position { fen, moves } => {
                self.set_position(&fen, &moves);
                Vec::new()
            }
            UciCommand::Go { perft_depth: Some(depth), .. } => self.go_perft(depth),
            UciCommand::Go { perft_depth: None, depth, wtime, btime, winc, binc, movestogo, movetime, infinite, ponder } => {
                let mut lines = Vec::new();
                if self.pin_at_next_go {
                    self.pin_at_next_go = false;
                    lines.push(crate::core_pinning::pin(self.threads));
                }
                lines.extend(self.go_search(depth, wtime, btime, winc, binc, movestogo, movetime, infinite, ponder));
                lines
            }
            UciCommand::PonderHit => {
                // The ponder search goes on, now under its clock, from the depth it reached; its
                // thread prints the `bestmove`. Already stopped, or not a ponder search: nothing.
                if let Some(RunningSearch { ponder: Some((outcome, state)), .. }) = &self.running_search
                    && outcome.compare_exchange(PONDER_PENDING, PONDER_HIT, Ordering::SeqCst, Ordering::SeqCst).is_ok()
                {
                    state.ponderhit();
                }
                Vec::new()
            }
            UciCommand::Stop => {
                if let Some(RunningSearch { ponder: Some((outcome, _)), .. }) = &self.running_search {
                    let _ = outcome.compare_exchange(PONDER_PENDING, PONDER_STOPPED, Ordering::SeqCst, Ordering::SeqCst);
                }
                // A silent no-op if nothing is running -- exactly Stockfish's own real
                // `threads.stop = true;`, nothing more. The background thread notices on its
                // own next CHECK_INTERVAL poll and prints its own bestmove whenever it
                // naturally unwinds; this call doesn't wait for that, or produce any output of
                // its own.
                if let Some(running) = &self.running_search {
                    running.stop_signal.store(true, Ordering::Relaxed);
                }
                Vec::new()
            }
            UciCommand::Quit | UciCommand::Unknown => Vec::new(),
        }
    }

    /// Replaces the live board with `fen`, then replays `moves` one at a time. Malformed
    /// input at any step (a bad FEN, a move that doesn't resolve against the position as
    /// it stands so far) stops right there rather than panicking -- the UCI spec's own
    /// robustness rule ("ignore and continue") applied to a multi-step command: whatever
    /// state was reached before the bad step is what's kept, no attempt to guess past it.
    fn set_position(&mut self, fen: &str, moves: &[String]) {
        let board = match Board::from_fen(fen) {
            Ok((board, _)) => board,
            Err(_) => return,
        };
        self.board = board;
        self.ancestors = Vec::new();

        for text in moves {
            let color = self.board.side_to_move();
            let candidates = legal_moves(&self.board, color);
            let mv = match parse_move(text, &candidates) {
                Some(mv) => mv,
                None => return,
            };
            // Pushed *before* making the move: this is `self.board`'s hash as it stands right
            // now, about to become an ancestor of whatever position `mv` leads to -- not the
            // position `mv` produces, which is `self.board`'s own new value after this call
            // and thus never itself an entry in `self.ancestors` (see that field's own doc
            // comment for why `ancestors` never includes the current position).
            self.ancestors.push(self.board.hash());
            self.board.make_move(mv);
        }
    }

    /// `go perft <depth>`'s divide output: each root move with its own subtree count,
    /// then the grand total -- Stockfish's own familiar shape. Depth 0 is a special case
    /// handled separately: perft's own definition of depth 0 is "the current position is
    /// itself the one leaf," so no root move is actually made, and there is nothing to
    /// divide by.
    fn go_perft(&self, depth: u32) -> Vec<String> {
        if depth == 0 {
            return vec!["Nodes searched: 1".to_string()];
        }

        let color = self.board.side_to_move();
        let mut lines = Vec::new();
        let mut total = 0u64;

        for mv in legal_moves(&self.board, color) {
            let mut scratch = self.board;
            scratch.make_move(mv);
            let count = perft(&scratch, depth - 1);
            lines.push(format!("{} : {}", format_move(mv), count));
            total += count;
        }

        lines.push(format!("Nodes searched: {total}"));
        lines
    }

    /// Plain `go`'s real work now runs on its own background thread, spawned here and handed
    /// off to immediately -- this call itself always returns an empty `Vec` right away, not the
    /// eventual `info`/`bestmove` lines, so `run()`'s stdin loop can keep reading (and, in
    /// particular, can actually deliver a `stop`) while the search is still in progress.
    ///
    /// Every `info depth ...` line comes from thread `0` alone, printed live the instant that
    /// depth actually completes (`choose_best_move_with_progress`'s own `on_iteration`
    /// callback) -- not batched up and printed only once the whole search is over, which is
    /// what this used to do (and what silently broke a real GUI's live analysis display,
    /// caught only by watching one, never by a test that just reads the eventual return value).
    /// Every other thread still calls plain `choose_best_move`, silently, exactly as before:
    /// only one thread's progress is ever reported live, matching Stockfish's own real
    /// behavior (only its main thread ever prints an `info` line, confirmed directly against
    /// its `search.cpp`) -- reporting every Lazy-SMP worker's own progress at once would just
    /// be overlapping, conflicting noise. `select_best_result`'s own final choice of which
    /// thread's move to report is completely unaffected by this: only the concluding
    /// `bestmove` line ever reflects the true overall winner, which can still be a different
    /// thread than whichever one's `info` lines were streaming live throughout -- the same
    /// asymmetry Stockfish itself has.
    ///
    /// The seven time-control arguments are passed straight through to `time_limit`
    /// unexamined -- selecting which of them applies, filling in defaults, and turning them
    /// into an actual target are all `search.rs`'s job, not this interface layer's. `depth`
    /// is separate from all of that: it caps iterative deepening itself, orthogonal to
    /// whether the search is also time-bounded, so it's applied directly below rather than
    /// folded into `time_limit`'s own result.
    ///
    /// A `position`/`go`/`ucinewgame` arriving while this search is still running is not
    /// specially guarded against, matching the UCI spec's own assumption that a compliant GUI
    /// never sends one before receiving this search's own `bestmove`. Nothing unsafe happens
    /// even if that's violated: `board`/`ancestors` are cloned below rather than borrowed from
    /// `Engine`'s own live fields, so a later `Position` mutating those doesn't affect a search
    /// already in flight, and `Arc`'s reference counting means a `setoption name Hash` racing an
    /// in-flight search just leaves that search finishing against the table it started with, not
    /// a crash -- just not a scenario worth writing extra code for on purpose.
    #[allow(clippy::too_many_arguments)]
    fn go_search(
        &mut self,
        depth: Option<u32>,
        wtime: Option<u64>,
        btime: Option<u64>,
        winc: Option<u64>,
        binc: Option<u64>,
        movestogo: Option<u32>,
        movetime: Option<u64>,
        infinite: bool,
        ponder: bool,
    ) -> Vec<String> {
        let limit = time_limit(
            &self.board,
            wtime,
            btime,
            winc,
            binc,
            movestogo,
            movetime,
            infinite,
            self.move_overhead_ms,
            self.time_management,
        );
        // A game move with one legal move is capped (`single_move_limit`); `infinite` is not. A
        // ponder search is the game move's search, so it is capped too, from `ponderhit` on.
        let limit = if infinite { limit } else { single_move_limit(&self.board, limit) };
        // No depth ceiling of its own here -- an explicit `go depth N` is passed through exactly
        // as requested, and a `go` with no `depth` at all asks for as far as it can go
        // (`u32::MAX` is never actually reached; it's just "no separate cap," not a real target).
        // Time management (the `TimeManager` gate and deadline inside `choose_best_move`) is what
        // actually bounds an ordinary, timed search, the same way it always did even back when a
        // separate depth cap also existed underneath it. `choose_best_move_with_progress` itself
        // silently clamps whatever `target_depth` it's handed to `MAX_PLY` before its own outer
        // loop runs (`search.rs`'s own `MAX_PLY` doc comment) -- genuinely a crash-safety
        // requirement, not just a quality one: a limitless `go` (`limit: None`, e.g. `go
        // infinite` or a real UCI client that never sends time controls) over a fast-resolving
        // position has nothing else to stop it, and an unclamped loop there is exactly what
        // overflowed `i32` in the history bonus during real testing.
        let target_depth = depth.unwrap_or(u32::MAX);

        // A previous search's own background thread must be fully joined before this touches
        // self.tt through Arc::get_mut below -- see join_running_search's own doc comment for
        // why this can't just be assumed from GUI-protocol timing. Joined before `last_score` is
        // read too, so it holds the previous search's score.
        self.join_running_search();
        let limit = limit.map(|limit| limit.with_previous_score(*self.last_score.lock().unwrap()));
        let last_score = Arc::clone(&self.last_score);

        // Exactly once per real search, before any worker thread starts -- choose_best_move
        // itself can no longer call this (it takes tt by shared reference now, since multiple
        // worker threads call it concurrently against the same table), so this is the one place
        // that both knows "this is the start of a new go" and still has exclusive access to bump
        // the generation counter.
        Arc::get_mut(&mut self.tt)
            .expect("no other Arc<TranspositionTable> clone should outlive a go_search call")
            .new_search();

        // Owned by the background thread below, not borrowed -- unlike before, that thread now
        // outlives this call entirely, so nothing here can lend it a reference the way
        // std::thread::scope alone used to let it borrow self.board/self.ancestors/self.tt
        // directly. board is Copy (a plain snapshot; see this function's own doc comment for why
        // a position changing mid-search isn't a concern), ancestors is cloned (small, a handful
        // of u64s), tt is an Arc clone (cheap, still the same shared table), and stop_signal is a
        // fresh Arc<AtomicBool> that this Engine and the background thread each need their own
        // handle to -- one clone goes to Engine's own running_search (so a later Stop can reach
        // it), the other is moved into the thread itself.
        let board = self.board;
        let ancestors = self.ancestors.clone();
        let tt = Arc::clone(&self.tt);
        let tablebase = Arc::clone(&self.tablebase);
        let evaluator = self.evaluator.clone();
        let parameters = self.parameters;
        let threads = self.threads;
        let multipv = self.multipv;
        let stop_signal = Arc::new(AtomicBool::new(false));
        let running_stop_signal = Arc::clone(&stop_signal);
        let stdout_lock = Arc::clone(&self.stdout_lock);
        let last_search_output = Arc::clone(&self.last_search_output);
        let offers_ponder_move = self.ponder;
        let ponder = ponder.then(|| (Arc::new(AtomicU8::new(PONDER_PENDING)), Arc::new(PonderState::new())));
        let ponder_outcome = ponder.as_ref().map(|(outcome, _)| Arc::clone(outcome));
        let ponder_state = ponder.as_ref().map(|(_, state)| Arc::clone(state));

        let thread = std::thread::spawn(move || {
            // Same borrow-before-.map() pattern this function's inner thread::scope has always
            // used, now over owned local data instead of Engine's own fields: a move closure
            // inside .map() runs once per thread, and can only move a non-Copy value like
            // AtomicBool once, not repeatedly, so everything captured below is a reference
            // (Copy) rather than the owned value itself.
            let board = &board;
            let ancestors = &ancestors;
            let tt = &tt;
            let tablebase = &tablebase;
            let evaluator = evaluator.as_deref();
            let stop_signal = &*stop_signal;
            let ponder_state = ponder_state.as_deref();
            // The root's tablebase result, once per search, for the scores shown
            // (`root_reported_score`); `None` outside the tables.
            let root_tablebase = tablebase.root_result(board, ancestors);

            // std::thread::scope guarantees every spawned handle is joined before it returns,
            // exactly the already-settled shape ("go spawns N worker threads, joins all of
            // them, then replies") -- just nested one level deeper now, inside the outer
            // background thread rather than inside go_search's own call frame directly.
            let results: Vec<(Vec<IterationReport>, Option<Move>)> = std::thread::scope(|scope| {
                let handles: Vec<_> = (0..threads)
                    .map(|thread_idx| {
                        // Only thread 0 gets a real callback -- see go_search's own doc comment
                        // for why every other thread stays silent. Its own clones of
                        // stdout_lock/last_search_output, separate from the ones this function
                        // keeps for the final bestmove line below: this closure is moved into
                        // its own spawned thread, which needs to own whatever it captures.
                        if thread_idx == 0 {
                            let stdout_lock = Arc::clone(&stdout_lock);
                            let last_search_output = Arc::clone(&last_search_output);
                            scope.spawn(move || run_main_thread(stop_signal, || {
                                // multipv <= 1 keeps calling choose_best_move_with_progress
                                // exactly as before this option existed -- MultiPV search adds a
                                // real, extra cost (one full-window search per additional rank),
                                // so the common, default case pays nothing for a feature it isn't
                                // using.
                                if multipv > 1 {
                                    choose_best_move_multipv_with_progress(
                                        board, target_depth, limit, ancestors, tt, tablebase, evaluator, None, stop_signal,
                                        multipv, ponder_state,
                                        move |report, extra_lines| {
                                            // Every rank shown is of the tables' best rank (the
                                            // rest are excluded), so every one takes the root's
                                            // result.
                                            let shown = |score| root_reported_score(score, root_tablebase);
                                            let mut lines = vec![format_multipv_info_line(
                                                report, 1, shown(report.score), &report.pv, tt.hashfull(),
                                            )];
                                            for (i, extra) in extra_lines.iter().enumerate() {
                                                lines.push(format_multipv_info_line(
                                                    report, i as u32 + 2, shown(extra.score), &extra.pv, tt.hashfull(),
                                                ));
                                            }
                                            // Same push-before-print, explicit-flush reasoning as
                                            // the single-line branch below -- one rank per line,
                                            // all pushed/printed together so a reader never sees
                                            // one iteration's lines interleaved with another's.
                                            let mut output = last_search_output.lock().unwrap();
                                            output.extend(lines.iter().cloned());
                                            drop(output);
                                            let _guard = stdout_lock.lock().unwrap();
                                            for line in &lines {
                                                println!("{line}");
                                            }
                                            std::io::stdout().flush().unwrap();
                                        },
                                    )
                                } else {
                                    choose_best_move_with_progress(
                                        board, target_depth, limit, ancestors, tt, tablebase, evaluator,
                                        Some(parameters), thread_idx, stop_signal, ponder_state,
                                        move |report| {
                                            let line = format_info_line(report, root_tablebase, tt.hashfull());
                                            // Pushed before the print, not after -- see
                                            // last_search_output's own doc comment for what this is
                                            // for; a test reading it back shouldn't depend on the
                                            // print itself having gone smoothly.
                                            last_search_output.lock().unwrap().push(line.clone());
                                            let _guard = stdout_lock.lock().unwrap();
                                            println!("{line}");
                                            // Explicit, not left to Stdout's own LineWriter: verified
                                            // by hand (temporary eprintln! instrumentation, removed
                                            // once answered) that relying on implicit flush-on-newline
                                            // alone was not reliable on this machine's actual pipe/tty
                                            // stack -- info lines sat unflushed for seconds at
                                            // a time, then all appeared together, the exact live-
                                            // reporting bug this change exists to fix in the first
                                            // place. A GUI reading this engine's output needs each
                                            // line the instant it's produced, not whenever the OS
                                            // happens to flush a buffer on its own.
                                            std::io::stdout().flush().unwrap();
                                        },
                                    )
                                }
                            }))
                        } else {
                            scope.spawn(move || {
                                choose_best_move_with_parameters(
                                    board, target_depth, limit, ancestors, tt, tablebase, evaluator, parameters,
                                    thread_idx, stop_signal,
                                )
                            })
                        }
                    })
                    .collect();
                handles.into_iter().map(|handle| handle.join().unwrap()).collect()
            });

            // Only best_move matters here -- every info line any thread's own reports could
            // have produced was already handled above (thread 0 printed its own live; every
            // other thread's own reports are simply never reported, matching go_search's own
            // doc comment on why only one thread's progress is ever shown).
            let (reports, best_move) = select_best_result(results);

            // A ponder search answers only after `ponderhit` or `stop`: one that finished on its
            // own (a forced mate, `MAX_PLY`, a `depth`) waits for either, then answers at once.
            if let Some(outcome) = &ponder_outcome {
                while outcome.load(Ordering::SeqCst) == PONDER_PENDING {
                    std::thread::sleep(PONDER_POLL);
                }
            }
            // The previous move's score for the next search's clock target, unless this was a ponder
            // search the GUI stopped: its move is not the one played.
            let played = ponder_outcome.as_ref().is_none_or(|outcome| outcome.load(Ordering::SeqCst) == PONDER_HIT);
            if played && let Some(report) = reports.last() {
                *last_score.lock().unwrap() = Some(report.score);
            }

            // The reply the search expects, when `Ponder` is on.
            let ponder_move = match reports.last() {
                Some(report) if offers_ponder_move => ponder_move(board, &report.pv, tt),
                _ => None,
            };
            let bestmove_line = match (best_move, ponder_move) {
                (Some(mv), Some(reply)) => format!("bestmove {} ponder {}", format_move(mv), format_move(reply)),
                (Some(mv), None) => format!("bestmove {}", format_move(mv)),
                (None, _) => "bestmove 0000".to_string(),
            };
            // The last completed iteration once more, the winning thread's, so its PV starts with the
            // move played: a GUI that collects a move's `info` only after `ponderhit` (lichess-bot,
            // through python-chess) otherwise records no PV and no score whenever no iteration
            // completes between the `ponderhit` and the `bestmove`. Not under MultiPV, an analysis
            // setting, where one rank out of several would mislead.
            let final_info_line = match reports.last() {
                Some(report) if multipv <= 1 => Some(format_info_line(report, root_tablebase, tt.hashfull())),
                _ => None,
            };
            let mut output = last_search_output.lock().unwrap();
            output.extend(final_info_line.iter().cloned());
            output.push(bestmove_line.clone());
            drop(output);
            let _guard = stdout_lock.lock().unwrap();
            if let Some(line) = &final_info_line {
                println!("{line}");
            }
            println!("{bestmove_line}");
            std::io::stdout().flush().unwrap();
        });

        self.running_search = Some(RunningSearch { stop_signal: running_stop_signal, thread, ponder });
        Vec::new()
    }
}

/// Runs thread 0's search, then sets `stop_signal` however it ended. Helpers have no clock and stop
/// only on that signal, which thread 0 sets itself only when time runs out: ending at the depth cap
/// (`MAX_PLY`, reached fast on mate and tablebase scores) or on an empty PV left them searching, and
/// `go_search`'s join, and so its `bestmove`, waited on them (a Lichess game hung for minutes).
/// Stockfish's main thread likewise sets `threads.stop` when its iterative deepening ends. Here, not
/// in `search.rs`: arena games, self-play and replay pass `choose_best_move` a stop flag that outlives
/// one move, which this would set for good.
fn run_main_thread<T>(stop_signal: &AtomicBool, search: impl FnOnce() -> T) -> T {
    let result = search();
    stop_signal.store(true, Ordering::Relaxed);
    result
}

/// The reply to offer as the ponder move after the PV's first move: the PV's second move, or, when
/// a transposition-table cutoff cut the PV to one move (common when the position was searched
/// before), the move the table holds for the position after it, if that move is legal there, as
/// Stockfish's `extract_ponder_from_tt` does. Read once the search is over, so the search itself is
/// unchanged.
fn ponder_move(board: &Board, pv: &[Move], tt: &TranspositionTable) -> Option<Move> {
    if let Some(&reply) = pv.get(1) {
        return Some(reply);
    }
    let best = *pv.first()?;
    let mut after = *board;
    after.make_move(best);
    let stored = tt.probe(after.hash())?.best_move();
    // Matched by squares and promotion, and the generator's own move returned: a move's kind
    // (a double push, a castle) is the generator's to say.
    legal_moves(&after, after.side_to_move()).into_iter().find(|legal| {
        legal.from() == stored.from() && legal.to() == stored.to() && legal.promotion() == stored.promotion()
    })
}

/// The `.nnue` a build embeds when `FATTORINI_EVALFILE` names one at build time (`build.rs`), empty
/// otherwise: a snapshot and a release are built this way. The engine plays nothing else: no
/// option nor file beside the binary chooses a network.
const EMBEDDED_NETWORK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded.nnue"));

/// The `parameters.txt` a build embeds (`FATTORINI_PARAMETERS`), empty for the untuned defaults.
const EMBEDDED_PARAMETERS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded-parameters.txt"));

/// What the build is (`FATTORINI_DESCRIPTION`), one line, empty for a development build.
const EMBEDDED_DESCRIPTION: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded-description.txt"));

/// The embedded network, `None` (the PeSTO evaluation) when the build has none. One that fails to
/// load ends the process: playing on with the PeSTO evaluation would silently be another engine
/// than the one built. stdout is UCI-only, so the reason goes to stderr.
pub fn embedded_evaluator() -> Option<Arc<Network>> {
    if EMBEDDED_NETWORK.is_empty() {
        return None;
    }
    match Network::from_bytes(EMBEDDED_NETWORK, "the embedded network") {
        Ok(network) => Some(Arc::new(network)),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

/// The embedded parameters, `SearchParameters::DEFAULT` when the build has none; malformed ones end
/// the process, as a bad network does.
pub fn embedded_parameters() -> SearchParameters {
    if EMBEDDED_PARAMETERS.is_empty() {
        return SearchParameters::DEFAULT;
    }
    let text = String::from_utf8_lossy(EMBEDDED_PARAMETERS);
    SearchParameters::parse_file_text("the embedded parameters", &text).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    })
}

/// What the build is, empty for a development build.
pub fn embedded_description() -> String {
    String::from_utf8_lossy(EMBEDDED_DESCRIPTION).trim().to_string()
}

/// The third-party notices a release embeds (`FATTORINI_NOTICES`, written by `cargo-about`), empty
/// for every other build. Their licenses (MIT, Apache and others) ask that a distributed binary
/// carry them, and the ODbL asks the training data's attribution: one file, so they live inside it.
const EMBEDDED_NOTICES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded-notices.txt"));

/// The embedded notices, empty when the build has none.
pub fn embedded_notices() -> String {
    String::from_utf8_lossy(EMBEDDED_NOTICES).into_owned()
}

/// The only place that actually touches stdin/stdout -- everything it does (parsing,
/// state, replies) is the same `parse_command`/`Engine::handle` machinery already tested
/// above without any I/O involved. Reads one line at a time, stops at `Quit` (checked
/// before calling `handle`, since quitting produces no reply of its own to print).
///
/// `Quit` calls `Engine::shutdown` before breaking out of the loop -- signaling any still-
/// running search to stop and waiting for its background thread to actually finish, so the
/// process never exits out from under a thread still mid-write to stdout. Every other
/// command's reply is printed here under `engine`'s own `stdout_lock`, the same lock
/// `go_search`'s background thread takes around its own `info`/`bestmove` lines -- without it,
/// a reply printed here while a search is still running in the background could interleave
/// mid-line with that thread's own output.
///
/// `lichess` is the command line's `--lichess`: the first `go` pins the process to a free core
/// group (`core_pinning.rs`).
pub fn run(lichess: bool) {
    let mut engine = Engine::embedded();
    engine.pin_at_next_go = lichess;
    if !engine.description.is_empty() {
        println!("info string {}", engine.description);
    }
    let stdin = std::io::stdin();

    for line in stdin.lines() {
        let command = parse_command(&line.expect("failed to read from stdin"));
        if command == UciCommand::Quit {
            engine.shutdown();
            break;
        }
        let responses = engine.handle(command);
        if !responses.is_empty() {
            let _guard = engine.stdout_lock.lock().unwrap();
            for response in responses {
                println!("{response}");
                // Same reasoning as go_search's own explicit flush: don't rely on Stdout's
                // implicit flush-on-newline alone, since it was observed to not be reliable on
                // this machine's actual pipe/tty stack.
                std::io::stdout().flush().unwrap();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::evaluate;

    /// A plain `go`: no perft, no time control, no depth cap at all. What most tests below
    /// actually want, since they're exercising something other than `go`'s own argument
    /// parsing -- spelled out here once rather than all eight fields at every call site.
    fn plain_go() -> UciCommand {
        UciCommand::Go {
            perft_depth: None,
            depth: None,
            wtime: None,
            btime: None,
            winc: None,
            binc: None,
            movestogo: None,
            movetime: None,
            infinite: false,
            ponder: false,
        }
    }

    /// A real `go movetime <ms>` -- what a test wanting an actual, bounded search should send
    /// instead of `plain_go()`. There's no depth cap at all any more to keep a bare, untimed
    /// `go` fast on its own (a real timed game should let the time budget be the binding
    /// constraint, not an artificially low depth cap -- see `go_search`'s own `target_depth`
    /// comment), so a test that needs a real search to finish quickly drives it via an explicit
    /// deadline instead, the same way an actual timed game would.
    fn movetime_go(ms: u64) -> UciCommand {
        UciCommand::Go {
            perft_depth: None,
            depth: None,
            wtime: None,
            btime: None,
            winc: None,
            binc: None,
            movestogo: None,
            movetime: Some(ms),
            infinite: false,
            ponder: false,
        }
    }

    /// A real `go depth <n>`, with no time control at all -- an untimed but depth-capped
    /// search, the shape `go_search`'s own `target_depth` (`depth.unwrap_or(u32::MAX)`) exists
    /// to serve: an explicit `depth` is what bounds the search here, not a time budget.
    fn depth_go(depth: u32) -> UciCommand {
        UciCommand::Go {
            perft_depth: None,
            depth: Some(depth),
            wtime: None,
            btime: None,
            winc: None,
            binc: None,
            movestogo: None,
            movetime: None,
            infinite: false,
            ponder: false,
        }
    }

    /// `go perft <depth>`, with no time control -- the other shape most tests below want.
    /// Named `perft_go`, not `go_perft`, specifically to not collide in spirit with
    /// `Engine::go_perft` (a different thing: that's the method under test here, this just
    /// builds the command that drives it).
    fn perft_go(depth: u32) -> UciCommand {
        UciCommand::Go {
            perft_depth: Some(depth),
            depth: None,
            wtime: None,
            btime: None,
            winc: None,
            binc: None,
            movestogo: None,
            movetime: None,
            infinite: false,
            ponder: false,
        }
    }

    #[test]
    fn format_move_prints_plain_coordinates() {
        assert_eq!(format_move(Move::new(Square::E2, Square::E4, None)), "e2e4");
    }

    #[test]
    fn format_move_appends_lowercase_promotion_letter() {
        assert_eq!(format_move(Move::new(Square::E7, Square::E8, Some(PieceType::Queen))), "e7e8q");
        assert_eq!(format_move(Move::new(Square::E7, Square::E8, Some(PieceType::Knight))), "e7e8n");
    }

    #[test]
    fn format_score_reports_an_ordinary_score_as_cp() {
        assert_eq!(format_score(320), "cp 320");
        assert_eq!(format_score(-150), "cp -150");
        assert_eq!(format_score(0), "cp 0");
    }

    #[test]
    fn format_score_reports_a_score_just_below_the_mate_band_as_cp() {
        // MATE_SCORE - MAX_PLY itself sits exactly on the boundary and stays cp -- it would
        // correspond to a ply_at_mate of MAX_PLY, one past what the search can ever actually
        // produce (a mate at ply MAX_PLY is never detected: the MAX_PLY safety net already
        // returns a plain evaluation for that node before checkmate is ever checked for).
        assert_eq!(format_score(MATE_SCORE - MAX_PLY as i32), format!("cp {}", MATE_SCORE - MAX_PLY as i32));
    }

    #[test]
    fn format_score_reports_a_winning_mate_in_moves_not_plies() {
        // ply_at_mate 1 or 2 both collapse to "mate 1" -- delivered on the mating side's
        // very next move either way (the opponent's own reply, when there is one, doesn't
        // add a second move to the count).
        assert_eq!(format_score(MATE_SCORE - 1), "mate 1");
        assert_eq!(format_score(MATE_SCORE - 2), "mate 1");
        assert_eq!(format_score(MATE_SCORE - 3), "mate 2");
        assert_eq!(format_score(MATE_SCORE - 4), "mate 2");
    }

    #[test]
    fn format_score_reports_a_losing_mate_as_negative() {
        assert_eq!(format_score(-(MATE_SCORE - 1)), "mate -1");
        assert_eq!(format_score(-(MATE_SCORE - 2)), "mate -1");
        assert_eq!(format_score(-(MATE_SCORE - 3)), "mate -2");
    }

    #[test]
    fn an_info_line_shows_the_roots_tablebase_result_in_the_tablebase_band_not_as_a_mate() {
        // The KQ vs KN that showed +1.59: the line carries the tables' win, as `cp` in the band.
        let report = IterationReport {
            depth: 12, seldepth: 18, score: 159, nodes: 1000, pv: vec![Move::new(Square::D1, Square::D5, None)], time_ms: 10,
        };
        let shown = crate::search::tb_win_at(0);
        assert!(format_info_line(&report, Some(TbResult::Win), 0).contains(&format!(" score cp {shown} ")));
        assert!(format_info_line(&report, Some(TbResult::Draw), 0).contains(" score cp 0 "));
        assert!(format_info_line(&report, None, 0).contains(" score cp 159 "));
        assert_eq!(report.score, 159, "the report itself keeps the search's score");
    }

    #[test]
    fn parse_move_finds_the_matching_legal_move() {
        let legal_moves = vec![Move::new(Square::E2, Square::E4, None), Move::new(Square::D2, Square::D4, None)];
        assert_eq!(parse_move("e2e4", &legal_moves), Some(legal_moves[0]));
    }

    #[test]
    fn parse_move_disambiguates_by_promotion_letter() {
        let legal_moves = vec![
            Move::new(Square::E7, Square::E8, Some(PieceType::Queen)),
            Move::new(Square::E7, Square::E8, Some(PieceType::Knight)),
        ];
        assert_eq!(parse_move("e7e8n", &legal_moves), Some(legal_moves[1]));
    }

    #[test]
    fn parse_move_resolves_castle_from_plain_coordinates() {
        // Nothing in the text "e1g1" says "castle" -- only the fact that the legal move
        // with that exact from/to happens to carry MoveKind::Castle does.
        let legal_moves = vec![Move::castle(Square::E1, Square::G1)];
        assert_eq!(parse_move("e1g1", &legal_moves), Some(legal_moves[0]));
    }

    #[test]
    fn parse_move_resolves_en_passant_from_plain_coordinates() {
        let legal_moves = vec![Move::en_passant_capture(Square::D5, Square::E6)];
        assert_eq!(parse_move("d5e6", &legal_moves), Some(legal_moves[0]));
    }

    #[test]
    fn parse_move_rejects_a_move_not_in_the_legal_list() {
        let legal_moves = vec![Move::new(Square::E2, Square::E4, None)];
        assert_eq!(parse_move("a2a4", &legal_moves), None);
    }

    #[test]
    fn parse_move_rejects_malformed_text() {
        let legal_moves = vec![Move::new(Square::E2, Square::E4, None)];
        assert_eq!(parse_move("", &legal_moves), None);
        assert_eq!(parse_move("e2", &legal_moves), None);
        assert_eq!(parse_move("e2e4qq", &legal_moves), None);
        assert_eq!(parse_move("z9z8", &legal_moves), None);
        assert_eq!(parse_move("e2e4x", &legal_moves), None);
    }

    #[test]
    fn format_and_parse_round_trip() {
        let mv = Move::new(Square::G1, Square::F3, None);
        let legal_moves = vec![mv];
        assert_eq!(parse_move(&format_move(mv), &legal_moves), Some(mv));
    }

    #[test]
    fn parse_command_recognizes_the_fixed_reply_commands() {
        assert_eq!(parse_command("uci"), UciCommand::Uci);
        assert_eq!(parse_command("isready"), UciCommand::IsReady);
        assert_eq!(parse_command("ucinewgame"), UciCommand::UciNewGame);
        assert_eq!(parse_command("stop"), UciCommand::Stop);
        assert_eq!(parse_command("quit"), UciCommand::Quit);
    }

    #[test]
    fn parse_command_reads_setoption_hash() {
        assert_eq!(parse_command("setoption name Hash value 64"), UciCommand::SetOptionHash(64));
    }

    #[test]
    fn a_search_parameter_is_no_uci_option() {
        // Set only by the `parameters.txt` beside the network, so no GUI can disagree with it.
        assert_eq!(parse_command("setoption name RazoringMargin value 150"), UciCommand::Unknown);
    }

    #[test]
    fn parse_command_rejects_an_unrecognized_option_name() {
        assert_eq!(parse_command("setoption name Contempt value 20"), UciCommand::Unknown);
    }

    #[test]
    fn parse_command_rejects_hash_with_a_non_numeric_value() {
        assert_eq!(parse_command("setoption name Hash value banana"), UciCommand::Unknown);
    }

    #[test]
    fn parse_command_rejects_setoption_with_no_value() {
        assert_eq!(parse_command("setoption name Hash"), UciCommand::Unknown);
    }

    #[test]
    fn parse_command_reads_setoption_threads() {
        assert_eq!(parse_command("setoption name Threads value 4"), UciCommand::SetOptionThreads(4));
    }

    #[test]
    fn parse_command_rejects_threads_with_a_non_numeric_value() {
        assert_eq!(parse_command("setoption name Threads value banana"), UciCommand::Unknown);
    }

    #[test]
    fn parse_command_rejects_threads_with_no_value() {
        assert_eq!(parse_command("setoption name Threads"), UciCommand::Unknown);
    }

    #[test]
    fn parse_command_reads_setoption_multipv() {
        assert_eq!(parse_command("setoption name MultiPV value 3"), UciCommand::SetOptionMultiPv(3));
    }

    #[test]
    fn parse_command_rejects_multipv_with_a_non_numeric_value() {
        assert_eq!(parse_command("setoption name MultiPV value banana"), UciCommand::Unknown);
    }

    #[test]
    fn parse_command_rejects_multipv_with_no_value() {
        assert_eq!(parse_command("setoption name MultiPV"), UciCommand::Unknown);
    }

    #[test]
    fn parse_command_reads_a_single_syzygy_path() {
        assert_eq!(
            parse_command("setoption name SyzygyPath value /data/syzygy-6-wdl"),
            UciCommand::SetOptionSyzygyPath(vec!["/data/syzygy-6-wdl".to_string()])
        );
    }

    #[test]
    fn parse_command_splits_multiple_syzygy_paths_on_the_os_list_separator() {
        // std::env::join_paths is the exact inverse of the std::env::split_paths
        // parse_syzygy_path_value uses -- OS-correct either way, rather than hardcoding ':'.
        let joined = std::env::join_paths(["/data/syzygy-3-4-5-wdl", "/data/syzygy-6-wdl"]).unwrap();
        assert_eq!(
            parse_command(&format!("setoption name SyzygyPath value {}", joined.to_string_lossy())),
            UciCommand::SetOptionSyzygyPath(vec![
                "/data/syzygy-3-4-5-wdl".to_string(),
                "/data/syzygy-6-wdl".to_string()
            ])
        );
    }

    #[test]
    fn parse_command_rejects_syzygy_path_with_no_value() {
        assert_eq!(parse_command("setoption name SyzygyPath"), UciCommand::Unknown);
    }

    #[test]
    fn parse_command_resolves_startpos_to_its_fen() {
        assert_eq!(
            parse_command("position startpos"),
            UciCommand::Position { fen: STARTPOS_FEN.to_string(), moves: Vec::new() }
        );
    }

    #[test]
    fn parse_command_reads_a_literal_fen() {
        let fen = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
        assert_eq!(
            parse_command(&format!("position fen {fen}")),
            UciCommand::Position { fen: fen.to_string(), moves: Vec::new() }
        );
    }

    #[test]
    fn parse_command_collects_trailing_moves() {
        assert_eq!(
            parse_command("position startpos moves e2e4 e7e5"),
            UciCommand::Position {
                fen: STARTPOS_FEN.to_string(),
                moves: vec!["e2e4".to_string(), "e7e5".to_string()],
            }
        );
    }

    #[test]
    fn parse_command_rejects_a_short_fen() {
        assert_eq!(parse_command("position fen rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -"), UciCommand::Unknown);
    }

    #[test]
    fn parse_command_reads_plain_and_perft_go() {
        assert_eq!(parse_command("go"), plain_go());
        assert_eq!(parse_command("go perft 5"), perft_go(5));
    }

    #[test]
    fn parse_command_reads_go_depth() {
        assert_eq!(parse_command("go depth 6"), depth_go(6));
    }

    #[test]
    fn parse_go_reads_every_time_control_field_regardless_of_order() {
        assert_eq!(
            parse_command("go winc 5 wtime 60000 movetime 2000 btime 55000 movestogo 20 binc 6 infinite"),
            UciCommand::Go {
                perft_depth: None,
                depth: None,
                wtime: Some(60000),
                btime: Some(55000),
                winc: Some(5),
                binc: Some(6),
                movestogo: Some(20),
                movetime: Some(2000),
                infinite: true,
                ponder: false,
            }
        );
    }

    #[test]
    fn parse_go_skips_unsupported_arguments_without_misreading_their_values() {
        // `depth` is read and stored; `nodes`/`mate` each take a numeric value that must
        // still be skipped as a pair -- getting this wrong (skipping only the keyword) would
        // misread the value as an unrelated keyword and corrupt everything parsed after it.
        // `ponder`/`searchmoves` take no value of their own here (searchmoves' move list is
        // just further tokens this parser doesn't recognize, harmlessly skipped one at a
        // time), so `wtime` parsing right after either of them confirms the skip logic
        // didn't overshoot.
        assert_eq!(
            parse_command("go depth 10 nodes 500000 mate 3 ponder wtime 1000"),
            UciCommand::Go {
                perft_depth: None,
                depth: Some(10),
                wtime: Some(1000),
                btime: None,
                winc: None,
                binc: None,
                movestogo: None,
                movetime: None,
                infinite: false,
                ponder: true,
            }
        );
        assert_eq!(
            parse_command("go searchmoves e2e4 d2d4 wtime 1000"),
            UciCommand::Go {
                perft_depth: None,
                depth: None,
                wtime: Some(1000),
                btime: None,
                winc: None,
                binc: None,
                movestogo: None,
                movetime: None,
                infinite: false,
                ponder: false,
            }
        );
    }

    #[test]
    fn parse_go_treats_a_non_numeric_time_value_as_absent_rather_than_unknown() {
        // A malformed value doesn't reject the whole command (unlike `perft`'s own strict
        // `parse_command_rejects_a_non_numeric_perft_depth` below) -- `go` degrades one
        // field at a time, per the UCI spec's own robustness convention.
        assert_eq!(parse_command("go wtime five"), plain_go());
    }

    #[test]
    fn parse_command_rejects_a_non_numeric_perft_depth() {
        assert_eq!(parse_command("go perft five"), UciCommand::Unknown);
    }

    #[test]
    fn parse_command_treats_unknown_input_as_unknown() {
        assert_eq!(parse_command("setoption name Style value Risky"), UciCommand::Unknown);
        assert_eq!(parse_command(""), UciCommand::Unknown);
        assert_eq!(parse_command("position"), UciCommand::Unknown);
    }

    #[test]
    fn uci_command_replies_with_identity_and_uciok() {
        let mut engine = Engine::new();
        assert_eq!(
            engine.handle(UciCommand::Uci),
            vec![
                format!("id name Fattorini {}", env!("CARGO_PKG_VERSION")),
                "id author Calogero Zarba".to_string(),
                "option name Hash type spin default 16 min 1 max 1024".to_string(),
                "option name Threads type spin default 1 min 1 max 128".to_string(),
                "option name SyzygyPath type string default <empty>".to_string(),
                "option name MultiPV type spin default 1 min 1 max 500".to_string(),
                "option name Ponder type check default false".to_string(),
                "option name Move Overhead type spin default 20 min 0 max 5000".to_string(),
                "option name TimeManagement type combo default Split var Split var Classic".to_string(),
                "uciok".to_string(),
            ]
        );
    }

    #[test]
    fn a_described_build_says_what_it_is_in_its_name() {
        let mut engine = Engine::new();
        engine.description = "pesto@untuned, commit abc".to_string();
        let replies = engine.handle(UciCommand::Uci);
        assert_eq!(replies[0], format!("id name Fattorini {} pesto@untuned, commit abc", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn a_search_with_a_network_plays_a_legal_move() {
        // The network's values do not matter here, only that the search runs with it and answers:
        // nnue.rs's own small fixture, given to the engine as a build would embed it.
        let path = std::env::temp_dir().join(format!("fattorini-uci-network-test-{}.nnue", std::process::id()));
        let path = path.to_str().unwrap().to_string();
        crate::nnue::tests::sample_network(8, &[4]).write(&path).expect("failed to write the test network");

        let mut engine = Engine::new();
        engine.evaluator = Some(Arc::new(Network::load(&path).expect("the test network loads")));
        engine.handle(parse_command("go depth 3"));
        let output = engine.join_and_take_last_search_output();
        let bestmove = output.last().expect("a search prints its bestmove");
        assert!(bestmove.starts_with("bestmove ") && bestmove != "bestmove 0000", "{bestmove}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_development_build_embeds_nothing() {
        // `cargo test` builds with no `FATTORINI_*` set.
        let engine = Engine::embedded();
        assert!(engine.evaluator.is_none());
        assert_eq!(engine.parameters, SearchParameters::DEFAULT);
        assert!(engine.description.is_empty());
        assert!(embedded_notices().is_empty());
    }

    #[test]
    fn isready_replies_readyok() {
        let mut engine = Engine::new();
        assert_eq!(engine.handle(UciCommand::IsReady), vec!["readyok".to_string()]);
    }

    #[test]
    fn stop_with_nothing_running_is_a_silent_no_op() {
        let mut engine = Engine::new();
        assert_eq!(engine.handle(UciCommand::Stop), Vec::<String>::new());
    }

    /// Thread 0's search ending, however it ends, stops the helpers: their only stop is this signal.
    #[test]
    fn run_main_thread_returns_the_search_result_and_then_stops_the_helpers() {
        let stop_signal = AtomicBool::new(false);
        let result = run_main_thread(&stop_signal, || {
            assert!(!stop_signal.load(Ordering::Relaxed), "not stopped while thread 0 still searches");
            7
        });
        assert_eq!(result, 7);
        assert!(stop_signal.load(Ordering::Relaxed));
    }

    #[test]
    fn parse_command_reads_ponderhit_and_the_ponder_option() {
        assert_eq!(parse_command("ponderhit"), UciCommand::PonderHit);
        assert_eq!(parse_command("setoption name Ponder value true"), UciCommand::SetOptionPonder(true));
        assert_eq!(parse_command("setoption name Ponder value false"), UciCommand::SetOptionPonder(false));
        assert_eq!(parse_command("setoption name Ponder value maybe"), UciCommand::Unknown);
    }

    /// Every `bestmove` line in `output`.
    fn bestmoves(output: &[String]) -> Vec<String> {
        output.iter().filter(|line| line.starts_with("bestmove")).cloned().collect()
    }

    #[test]
    fn bestmove_offers_no_ponder_move_while_the_option_is_off() {
        let mut engine = Engine::new();
        engine.handle(depth_go(4));
        let output = engine.join_and_take_last_search_output();
        let bestmove = output.last().unwrap();
        assert_eq!(bestmove.split_whitespace().count(), 2, "{bestmove}");
    }

    #[test]
    fn bestmove_offers_the_pv_reply_as_its_ponder_move_once_the_option_is_on() {
        let mut engine = Engine::new();
        engine.handle(parse_command("setoption name Ponder value true"));
        engine.handle(depth_go(4));
        let output = engine.join_and_take_last_search_output();
        let tokens: Vec<&str> = output.last().unwrap().split_whitespace().collect();
        assert_eq!(tokens.len(), 4, "{tokens:?}");
        assert_eq!((tokens[0], tokens[2]), ("bestmove", "ponder"));

        let mut board = engine.board;
        let best = parse_move(tokens[1], &legal_moves(&board, board.side_to_move())).expect("a legal best move");
        board.make_move(best);
        assert!(parse_move(tokens[3], &legal_moves(&board, board.side_to_move())).is_some(), "{tokens:?}");
    }

    #[test]
    fn a_one_move_pv_takes_its_ponder_move_from_the_transposition_table() {
        let (board, _) = Board::from_fen(STARTPOS_FEN).unwrap();
        let e2e4 = parse_move("e2e4", &legal_moves(&board, board.side_to_move())).unwrap();
        let mut after = board;
        after.make_move(e2e4);
        let tt = TranspositionTable::new(1);
        let offered = |pv: &[Move]| ponder_move(&board, pv, &tt).map(format_move);
        assert_eq!(offered(&[e2e4]), None, "nothing stored, nothing offered");

        tt.store(after.hash(), Move::new(Square::E7, Square::E5, None), 1, 0, crate::tt::Bound::Exact);
        assert_eq!(offered(&[e2e4]), Some("e7e5".to_string()));
        let d7d5 = parse_move("d7d5", &legal_moves(&after, after.side_to_move())).unwrap();
        assert_eq!(offered(&[e2e4, d7d5]), Some("d7d5".to_string()), "a longer PV's second move comes first");

        tt.store(after.hash(), Move::new(Square::E7, Square::E4, None), 2, 0, crate::tt::Bound::Exact);
        assert_eq!(offered(&[e2e4]), None, "a stored move illegal there is not offered");
    }

    #[test]
    fn a_search_answered_by_the_table_still_offers_a_ponder_move() {
        let mut engine = Engine::new();
        engine.handle(parse_command("setoption name Ponder value true"));
        for _ in 0..2 {
            engine.handle(depth_go(6));
            let output = engine.join_and_take_last_search_output();
            let tokens: Vec<&str> = output.last().unwrap().split_whitespace().collect();
            assert_eq!(tokens.len(), 4, "the second search's PV is cut short by the table: {tokens:?}");
        }
    }

    #[test]
    fn a_finished_ponder_search_answers_only_stop() {
        let mut engine = Engine::new();
        // Mate in one (Ra8#): depth 1 finishes at once, then has to wait.
        engine.handle(parse_command("position fen 6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1"));
        engine.handle(parse_command("go ponder depth 1 wtime 1000 btime 1000"));
        std::thread::sleep(Duration::from_millis(200));
        assert!(bestmoves(&engine.last_search_output.lock().unwrap()).is_empty(), "no bestmove before stop");

        engine.handle(UciCommand::Stop);
        let output = engine.join_and_take_last_search_output();
        let answers = bestmoves(&output);
        assert_eq!(answers.len(), 1, "{answers:?}");
        assert_ne!(answers[0], "bestmove 0000");
    }

    #[test]
    fn ponderhit_answers_once_from_the_timed_search() {
        let mut engine = Engine::new();
        engine.handle(parse_command("go ponder movetime 100"));
        std::thread::sleep(Duration::from_millis(50));
        engine.handle(UciCommand::PonderHit);
        let output = engine.join_and_take_last_search_output();
        let answers = bestmoves(&output);
        assert_eq!(answers.len(), 1, "{answers:?}");
        assert_ne!(answers[0], "bestmove 0000");
    }

    /// The depth of every `info depth` line in `output`, in order.
    fn info_depths(output: &[String]) -> Vec<u32> {
        output
            .iter()
            .filter_map(|line| line.strip_prefix("info depth "))
            .map(|rest| rest.split_whitespace().next().unwrap().parse().unwrap())
            .collect()
    }

    #[test]
    fn ponderhit_after_a_ponder_past_the_target_answers_at_once_from_the_same_search() {
        let mut engine = Engine::new();
        // Target about 300 ms (`clock_target_ms` at full instability), well under the second spent
        // pondering.
        engine.handle(parse_command("go ponder wtime 2000 btime 2000"));
        std::thread::sleep(Duration::from_millis(1_000));
        assert!(bestmoves(&engine.last_search_output.lock().unwrap()).is_empty(), "no bestmove while pondering");

        let hit = std::time::Instant::now();
        engine.handle(UciCommand::PonderHit);
        let output = engine.join_and_take_last_search_output();
        let elapsed = hit.elapsed();
        assert!(elapsed < Duration::from_millis(200), "took {elapsed:?} after ponderhit");
        let answers = bestmoves(&output);
        assert_eq!(answers.len(), 1, "{answers:?}");
        assert_ne!(answers[0], "bestmove 0000");
        let depths = info_depths(&output);
        let (last, iterations) = depths.split_last().expect("at least the final info line");
        assert!(iterations.windows(2).all(|pair| pair[1] == pair[0] + 1), "one search, never restarted: {depths:?}");
        assert_eq!(Some(last), iterations.last(), "the final line repeats the last iteration: {depths:?}");
    }

    /// A ponder search that already finished on its own answers the `ponderhit` with no new
    /// iteration: the final `info` line is then the one record of the move's PV and score.
    #[test]
    fn a_ponderhit_with_no_new_iteration_still_ends_with_a_pv_for_the_move_played() {
        let mut engine = Engine::new();
        engine.handle(parse_command("go ponder depth 2"));
        std::thread::sleep(Duration::from_millis(300));
        let before_hit = engine.last_search_output.lock().unwrap().len();
        engine.handle(UciCommand::PonderHit);
        let output = engine.join_and_take_last_search_output();
        let after_hit = &output[before_hit..];
        assert_eq!(after_hit.len(), 2, "{after_hit:?}");
        let played = after_hit[1].split_whitespace().nth(1).unwrap();
        let pv = after_hit[0].split(" pv ").nth(1).unwrap_or_else(|| panic!("no pv: {:?}", after_hit[0]));
        assert!(after_hit[0].starts_with("info depth 2 "), "{:?}", after_hit[0]);
        assert_eq!(pv.split(' ').next(), Some(played), "{after_hit:?}");
    }

    #[test]
    fn ponderhit_on_a_single_legal_move_answers_within_the_cap() {
        let mut engine = Engine::new();
        engine.handle(parse_command("position fen 7k/8/8/8/8/8/1p4q1/7K w - - 0 1"));
        let started = std::time::Instant::now();
        engine.handle(parse_command("go ponder wtime 600000 btime 600000"));
        std::thread::sleep(Duration::from_millis(100));
        engine.handle(UciCommand::PonderHit);
        let output = engine.join_and_take_last_search_output();
        let elapsed = started.elapsed();
        assert!(output.last().unwrap().starts_with("bestmove h1g2"), "{output:?}");
        assert!(elapsed < Duration::from_millis(1_500), "took {elapsed:?}");
    }

    #[test]
    fn ponderhit_with_nothing_running_is_a_silent_no_op() {
        let mut engine = Engine::new();
        assert_eq!(engine.handle(UciCommand::PonderHit), Vec::<String>::new());
        engine.handle(depth_go(1));
        engine.join_and_take_last_search_output();
        assert_eq!(engine.handle(UciCommand::PonderHit), Vec::<String>::new());
    }

    #[test]
    fn position_and_go_perft_agree_with_the_known_startpos_values() {
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position { fen: STARTPOS_FEN.to_string(), moves: Vec::new() });

        // The textbook fact (also used in perft.rs's own tests): 20 legal first moves,
        // each a single-node subtree at depth 1.
        let lines = engine.handle(perft_go(1));
        assert_eq!(lines.len(), 21);
        assert_eq!(lines.last(), Some(&"Nodes searched: 20".to_string()));
        assert!(lines.contains(&"e2e4 : 1".to_string()));
    }

    #[test]
    fn go_perft_depth_zero_is_one_node_with_no_divide() {
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position { fen: STARTPOS_FEN.to_string(), moves: Vec::new() });
        assert_eq!(engine.handle(perft_go(0)), vec!["Nodes searched: 1".to_string()]);
    }

    #[test]
    fn position_moves_are_replayed_in_order() {
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position {
            fen: STARTPOS_FEN.to_string(),
            moves: vec!["e2e4".to_string()],
        });

        // Black to move now -- e7e5 only appears among the root moves divide lists if the
        // position actually advanced past White's first move.
        let lines = engine.handle(perft_go(1));
        assert!(lines.contains(&"e7e5 : 1".to_string()));
    }

    #[test]
    fn set_position_builds_ancestors_from_every_move_except_the_last() {
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position {
            fen: "4k3/8/8/8/8/8/8/4K3 w - - 0 1".to_string(),
            moves: vec!["e1d1".to_string(), "e8d8".to_string()],
        });

        let (start, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mut after_first_move = start;
        after_first_move.make_move(Move::new(Square::E1, Square::D1, None));
        // Two entries for two moves: the position each move started from -- never the
        // position the last move actually produced, which is `engine.board` itself, not one
        // of its own ancestors.
        assert_eq!(engine.ancestors, vec![start.hash(), after_first_move.hash()]);
    }

    #[test]
    fn set_position_ancestors_stays_in_sync_when_a_move_is_malformed() {
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position {
            fen: "4k3/8/8/8/8/8/8/4K3 w - - 0 1".to_string(),
            moves: vec!["e1d1".to_string(), "not-a-move".to_string()],
        });

        // Only the first move actually happened -- ancestors has exactly the one entry that
        // move started from, matching exactly how far `engine.board` itself actually advanced.
        let (start, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(engine.ancestors, vec![start.hash()]);
    }

    #[test]
    fn an_unresolvable_move_stops_replay_without_panicking() {
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position {
            fen: STARTPOS_FEN.to_string(),
            moves: vec!["e2e4".to_string(), "not-a-move".to_string(), "e7e5".to_string()],
        });

        // Stopped right after e2e4 -- still Black to move, e7e5 never got applied (it's
        // still available as a root move from here, not consumed).
        let lines = engine.handle(perft_go(1));
        assert!(lines.contains(&"e7e5 : 1".to_string()));
    }

    #[test]
    fn a_malformed_fen_leaves_the_position_unchanged() {
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position { fen: "not a real fen".to_string(), moves: Vec::new() });

        // Still the starting position: 20 legal first moves.
        let lines = engine.handle(perft_go(1));
        assert_eq!(lines.last(), Some(&"Nodes searched: 20".to_string()));
    }

    /// A long clock and a deep `go depth` on a position with one legal move both answer within the
    /// single-move cap, plus generous room for a debug build's stop latency.
    #[test]
    fn a_single_legal_move_is_played_within_the_cap_under_a_clock_and_a_depth() {
        let forced = "7k/8/8/8/8/8/1p4q1/7K w - - 0 1";
        for go in ["go wtime 600000 btime 600000", "go depth 40"] {
            let mut engine = Engine::new();
            engine.handle(UciCommand::Position { fen: forced.to_string(), moves: Vec::new() });
            let started = std::time::Instant::now();
            engine.handle(parse_command(go));
            let lines = engine.join_and_take_last_search_output();
            let elapsed = started.elapsed();
            assert!(lines.last().unwrap().starts_with("bestmove h1g2"), "{go}: {lines:?}");
            assert!(elapsed < std::time::Duration::from_millis(1_500), "{go}: took {elapsed:?}");
        }
    }

    #[test]
    fn plain_go_returns_a_legal_move() {
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position { fen: STARTPOS_FEN.to_string(), moves: Vec::new() });

        // A real, bounded search -- driven by `movetime`, not a bare `go`, now that there's no
        // depth cap at all to keep an untimed search fast (that's the time budget's job in a
        // real game; see `go_search`'s own `target_depth` comment). How many depths actually
        // complete in `500`ms is machine-speed-dependent, so this checks the shape of whatever
        // came back rather than an exact count -- the one guarantee
        // that's actually deterministic is that depth `1` always completes.
        engine.handle(movetime_go(500));
        let lines = engine.join_and_take_last_search_output();
        assert!(lines.len() >= 3, "expected at least one info line, its repeat, plus bestmove, got {lines:?}");
        // The final line before `bestmove` repeats the last iteration's.
        let (iterations, last) = lines[..lines.len() - 1].split_at(lines.len() - 2);
        let depth_of = |line: &str| line.split_whitespace().nth(2).unwrap().to_string();
        assert_eq!(depth_of(&last[0]), depth_of(iterations.last().unwrap()), "{lines:?}");
        for (i, line) in lines[..lines.len() - 1].iter().enumerate() {
            let expected_depth = if i < iterations.len() { i as u32 + 1 } else { iterations.len() as u32 };
            assert!(
                line.starts_with(&format!("info depth {expected_depth} seldepth "))
                    && line.contains(" score cp ")
                    && line.contains(" nodes ")
                    && line.contains(" nps ")
                    && line.contains(" hashfull ")
                    && line.contains(" time "),
                "line {i} ({line:?}) doesn't match the expected info-line shape"
            );
            let pv = line.split(" pv ").nth(1).unwrap_or_else(|| panic!("line {i} ({line:?}) has no pv section"));
            assert!(!pv.is_empty(), "line {i} ({line:?}) has an empty pv");
        }
        let bestmove = lines.last().unwrap().strip_prefix("bestmove ").expect("go should reply with bestmove");

        let (board, _) = Board::from_fen(STARTPOS_FEN).unwrap();
        let legal = legal_moves(&board, board.side_to_move());
        assert!(parse_move(bestmove, &legal).is_some());
    }

    #[test]
    fn go_depth_caps_iterative_deepening_at_the_requested_depth() {
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position { fen: STARTPOS_FEN.to_string(), moves: Vec::new() });

        // Untimed (`limit` stays `None`, same as a bare `go`), but capped by `depth` -- this is
        // the regression `go_search`'s original `target_depth` fix guarded against: before it, a
        // `go depth N` ran to a fixed depth cap regardless of what depth was actually requested,
        // since the `depth` token was parsed and then silently discarded. `target_depth` now
        // honors an explicit `depth` exactly, only ever silently clamped against `MAX_PLY` (`100`)
        // deep inside `choose_best_move_with_progress` -- irrelevant here since `2` is nowhere
        // near that ceiling, so this is still the one real guarantee worth a dedicated test.
        engine.handle(depth_go(2));
        let lines = engine.join_and_take_last_search_output();

        assert_eq!(lines.len(), 4, "expected exactly 2 info lines, the last repeated, plus bestmove, got {lines:?}");
        assert!(lines[0].starts_with("info depth 1 "), "line 0 ({:?}) should be depth 1", lines[0]);
        assert!(lines[1].starts_with("info depth 2 "), "line 1 ({:?}) should be depth 2", lines[1]);
        assert!(lines[2].starts_with("info depth 2 "), "line 2 ({:?}) should repeat depth 2", lines[2]);
        assert!(lines[3].starts_with("bestmove "), "line 3 ({:?}) should be bestmove", lines[3]);
    }

    #[test]
    fn setoption_multipv_makes_go_search_report_extra_ranked_lines() {
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position { fen: STARTPOS_FEN.to_string(), moves: Vec::new() });
        engine.handle(UciCommand::SetOptionMultiPv(2));

        engine.handle(depth_go(2));
        let lines = engine.join_and_take_last_search_output();

        // 2 depths * 2 ranks each, plus one bestmove line.
        assert_eq!(lines.len(), 5, "expected 2 ranks at each of 2 depths plus bestmove, got {lines:?}");
        assert!(lines[0].starts_with("info depth 1 ") && lines[0].contains(" multipv 1 "), "{:?}", lines[0]);
        assert!(lines[1].starts_with("info depth 1 ") && lines[1].contains(" multipv 2 "), "{:?}", lines[1]);
        assert!(lines[2].starts_with("info depth 2 ") && lines[2].contains(" multipv 1 "), "{:?}", lines[2]);
        assert!(lines[3].starts_with("info depth 2 ") && lines[3].contains(" multipv 2 "), "{:?}", lines[3]);
        assert!(lines[4].starts_with("bestmove "), "{:?}", lines[4]);

        // The two ranks at the same depth must report two different moves -- not the same line
        // twice under a different multipv tag.
        let pv_of = |line: &str| line.split(" pv ").nth(1).unwrap().split(' ').next().unwrap().to_string();
        assert_ne!(pv_of(&lines[0]), pv_of(&lines[1]), "rank 1 and rank 2 must report different first moves");
        assert_ne!(pv_of(&lines[2]), pv_of(&lines[3]), "rank 1 and rank 2 must report different first moves");
    }

    #[test]
    fn multipv_of_one_reports_no_multipv_field_at_all() {
        // The default (MultiPV == 1) must keep producing exactly today's info-line shape --
        // no "multipv" token anywhere, matching plain_go_returns_a_legal_move's own assertion
        // shape, just confirmed explicitly here as the direct regression guard for this option.
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position { fen: STARTPOS_FEN.to_string(), moves: Vec::new() });

        engine.handle(depth_go(2));
        let lines = engine.join_and_take_last_search_output();

        assert_eq!(lines.len(), 4, "expected 2 info lines, the last repeated, plus bestmove, got {lines:?}");
        for line in &lines[..3] {
            assert!(!line.contains("multipv"), "{line:?}");
        }
    }

    #[test]
    fn plain_go_actually_picks_the_best_move_not_just_any_legal_one() {
        // White king e1, knight e4; Black king e8, rook c5 and pawn a7 -- Ne4xc5 is the only
        // legal move that wins material (every other legal move is quiet). The a7 pawn is
        // there so that capturing c5 still leaves Black with material: without it, Nxc5
        // would land on King+Knight vs bare King, one of the four insufficient-material
        // patterns, and the engine would (correctly) score that as a dead draw instead of a
        // material gain, defeating the point of this test. This exercises the full pipeline
        // end to end -- `choose_best_move`'s own root-move selection plus `go_search`'s
        // formatting of its result into UCI text -- not just `search.rs`'s own tests in
        // isolation: a regression anywhere along that path (an off-by-one sign error, a
        // formatting mistake) would only be caught by asking the engine, through UCI, to
        // actually choose among real alternatives.
        //
        // A rook, not a pawn: this position used to have Black's c5 piece be a pawn, which
        // made Nxc5's own material edge only ~100cp -- small enough for ordinary iterative-
        // deepening score noise among quiet moves (PST, then pawn structure, then mobility,
        // each adding their own small differentiation) to transiently exceed it at a few
        // specific depths, changing the reported bestmove there before later depths
        // self-corrected. Each time a new evaluation term landed, the depth where this
        // happened moved, requiring the movetime budget needed to search past it to keep
        // growing (500ms, then 2000ms, then -- measured directly, not assumed -- beyond
        // 4800ms once all four mobility pieces existed). Rather than keep chasing that same
        // problem after every future evaluation term, this position now has the knight
        // capture a rook instead (~500cp) -- confirmed directly (rule 5) to report `e4c5` as
        // the best move at every depth from 1 through 10, not just eventually, since ordinary
        // quiet-move noise (still only tens of centipawns) can never approach a 500cp gap.
        // Movetime dropped back to 500ms accordingly, exactly the forward note left when this
        // whole instability first appeared anticipated ("revisit ... once evaluation
        // stabilizes ... measured, not assumed").
        let fen = "4k3/p7/8/2r5/4N3/8/8/4K3 w - - 0 1";
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position { fen: fen.to_string(), moves: Vec::new() });

        engine.handle(movetime_go(500));
        let lines = engine.join_and_take_last_search_output();
        assert_eq!(lines.last(), Some(&"bestmove e4c5".to_string()));
    }

    #[test]
    fn go_reports_a_forced_mate_as_score_mate_not_a_huge_cp_number() {
        // Black king g8, pawns f7/g7/h7 still on their own squares (boxing their own king
        // in); White rook a1, king h1. Ra1-a8 is checkmate on the spot: the rook then
        // commands the whole back rank, the king has nowhere to go and nothing to capture.
        // Exercises the full stack end to end -- `format_score`'s own unit tests already
        // cover the arithmetic in isolation, but only a real search reaching a real
        // checkmate confirms it's actually wired up to what `go_search` prints.
        let fen = "6k1/5ppp/8/8/8/8/8/R6K w - - 0 1";
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position { fen: fen.to_string(), moves: Vec::new() });

        engine.handle(movetime_go(500));
        let lines = engine.join_and_take_last_search_output();
        assert_eq!(lines.last(), Some(&"bestmove a1a8".to_string()));
        // Found at depth 1 already, since the mate itself sits one ply below the root
        // regardless of how deep the search is asked to go -- true of every reported depth,
        // not just the first, so checking just the first line is representative.
        assert!(lines[0].contains(" score mate 1 "), "unexpected line: {}", lines[0]);
    }

    #[test]
    fn go_movetime_still_returns_a_real_move_even_when_the_budget_is_tiny() {
        // `movetime 0` is a `0`ms target -- exhausted before the loop even starts. Exercises the
        // full stack end to end (UCI parsing, `time_limit`, `choose_best_move`'s depth-1 guarantee, `go_search`'s
        // formatting), confirming a real move comes back rather than the null-move fallback
        // that would wrongly claim this position has no legal moves at all.
        // Second, a7 pawn again -- see `plain_go_actually_picks_the_best_move_not_just_any_legal_one`.
        let fen = "4k3/p7/8/2p5/4N3/8/8/4K3 w - - 0 1";
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position { fen: fen.to_string(), moves: Vec::new() });

        engine.handle(movetime_go(0));
        let lines = engine.join_and_take_last_search_output();
        // Exactly one completed depth (the guaranteed depth 1), then bestmove. Not asserting
        // an exact node count here -- that's `search.rs`'s own tests' job (rule 5: measured,
        // not guessed at) -- just the shape and the score/move this specific position implies.
        assert_eq!(lines.len(), 3, "depth 1, its repeat, bestmove: {lines:?}");
        assert!(lines[1].starts_with("info depth 1 ") && lines[1].ends_with(" pv e4c5"), "{lines:?}");
        // seldepth 2, not 1: Ne4-d6+ (a different root move from the reported best one, but
        // still explored) checks the Black king, and quiescence must search every legal
        // evasion when in check -- not just captures -- so that one extra real ply gets
        // visited even though no capture is involved. Measured, not predicted (rule 5): an
        // initial guess of `1` here turned out wrong for exactly this reason.
        // score cp: computed directly via evaluate() on the actual resulting position, not
        // a hand-derived magic number -- see `search.rs`'s own
        // `alpha_beta_depth_one_prefers_the_move_that_wins_more_material` for why (no
        // number to re-derive by hand every time TDLeaf(lambda) retunes a weight).
        let (root_board, _) = Board::from_fen(fen).unwrap();
        let mut after_capture = root_board;
        after_capture.make_move(Move::new(Square::E4, Square::C5, None));
        let expected_score = -evaluate(&after_capture);
        assert!(
            lines[0].starts_with(&format!("info depth 1 seldepth 2 score cp {expected_score} nodes ")),
            "unexpected line: {}",
            lines[0]
        );
        // nps/time are timing-dependent (machine speed, scheduling jitter), so only their
        // presence is checked here, not a specific value -- same reasoning as not asserting
        // an exact node count above. hashfull is deterministic in principle (a fresh table,
        // one depth-1 search), but pinning an exact expected count here would duplicate
        // TranspositionTable::hashfull's own dedicated tests above for no real benefit --
        // presence, plus staying within the documented 0..=1000 permille range, is what this
        // integration test actually needs to check.
        assert!(lines[0].contains(" nps "), "unexpected line: {}", lines[0]);
        assert!(lines[0].contains(" time "), "unexpected line: {}", lines[0]);
        let hashfull: u32 = lines[0]
            .split(" hashfull ")
            .nth(1)
            .unwrap_or_else(|| panic!("unexpected line: {}", lines[0]))
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap_or_else(|_| panic!("hashfull wasn't a valid integer: {}", lines[0]));
        assert!(hashfull <= 1000, "hashfull {hashfull} exceeds the documented 0..=1000 permille range");
        // Depth 1's PV can only be the one move itself -- known exactly here, unlike the node
        // count above.
        assert!(lines[0].ends_with(" pv e4c5"), "unexpected line: {}", lines[0]);
        assert_eq!(lines[2], "bestmove e4c5");
    }

    #[test]
    fn plain_go_replies_null_move_with_no_legal_moves() {
        // A standard stalemate position (Black king h8, boxed in by White's queen and
        // king, not itself in check) -- confirmed below, not just assumed, before relying
        // on it: zero legal moves for Black.
        let fen = "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1";
        let (board, _) = Board::from_fen(fen).unwrap();
        assert!(legal_moves(&board, board.side_to_move()).is_empty());

        let mut engine = Engine::new();
        engine.handle(UciCommand::Position { fen: fen.to_string(), moves: Vec::new() });
        engine.handle(plain_go());
        assert_eq!(engine.join_and_take_last_search_output(), vec!["bestmove 0000".to_string()]);
    }

    #[test]
    fn setoption_hash_rebuilds_the_table_at_the_requested_size() {
        let mut engine = Engine::new();
        let default_slots = engine.tt.slots();

        engine.handle(UciCommand::SetOptionHash(1));
        // 1 MB is far smaller than the 16 MB default, so this can only pass if the table was
        // actually rebuilt at the new size, not merely accepted and ignored.
        assert!(engine.tt.slots() < default_slots);
        assert_eq!(engine.tt.slots(), TranspositionTable::new(1).slots());
    }

    #[test]
    fn setoption_threads_stores_the_requested_count() {
        let mut engine = Engine::new();
        assert_eq!(engine.threads, DEFAULT_THREADS);

        engine.handle(UciCommand::SetOptionThreads(4));
        assert_eq!(engine.threads, 4);
    }

    #[test]
    fn setoption_multipv_stores_the_requested_count() {
        let mut engine = Engine::new();
        assert_eq!(engine.multipv, DEFAULT_MULTIPV);

        engine.handle(UciCommand::SetOptionMultiPv(3));
        assert_eq!(engine.multipv, 3);
    }

    #[test]
    fn parse_command_reads_the_move_overhead_option() {
        assert_eq!(parse_command("setoption name Move Overhead value 100"), UciCommand::SetOptionMoveOverhead(100));
        assert_eq!(parse_command("setoption name Move Overhead value banana"), UciCommand::Unknown);
        assert_eq!(parse_command("setoption name Move Overhead"), UciCommand::Unknown);
    }

    #[test]
    fn setoption_move_overhead_stores_the_value_within_its_bounds() {
        let mut engine = Engine::new();
        assert_eq!(engine.move_overhead_ms, DEFAULT_MOVE_OVERHEAD_MS);

        engine.handle(UciCommand::SetOptionMoveOverhead(0));
        assert_eq!(engine.move_overhead_ms, 0);
        engine.handle(UciCommand::SetOptionMoveOverhead(9_999));
        assert_eq!(engine.move_overhead_ms, MOVE_OVERHEAD_MS_MAX);
        engine.handle(UciCommand::UciNewGame);
        assert_eq!(engine.move_overhead_ms, MOVE_OVERHEAD_MS_MAX, "kept across ucinewgame");
    }

    #[test]
    fn parse_command_reads_the_time_management_option() {
        assert_eq!(
            parse_command("setoption name TimeManagement value Classic"),
            UciCommand::SetOptionTimeManagement(TimeManagement::Classic)
        );
        assert_eq!(
            parse_command("setoption name TimeManagement value Split"),
            UciCommand::SetOptionTimeManagement(TimeManagement::Split)
        );
        assert_eq!(parse_command("setoption name TimeManagement value Other"), UciCommand::Unknown);
    }

    #[test]
    fn setoption_time_management_is_split_by_default_and_kept_across_ucinewgame() {
        let mut engine = Engine::new();
        assert_eq!(engine.time_management, TimeManagement::Split);
        engine.handle(UciCommand::SetOptionTimeManagement(TimeManagement::Classic));
        engine.handle(UciCommand::UciNewGame);
        assert_eq!(engine.time_management, TimeManagement::Classic);
    }

    #[test]
    fn setoption_syzygy_path_swallows_a_missing_directory_without_panicking() {
        let mut engine = Engine::new();
        let lines = engine.handle(UciCommand::SetOptionSyzygyPath(vec!["/nonexistent/syzygy/path".to_string()]));
        assert_eq!(lines, Vec::<String>::new());
    }

    #[test]
    fn ucinewgame_resets_to_the_starting_position() {
        let mut engine = Engine::new();
        engine.handle(UciCommand::Position { fen: "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1".to_string(), moves: Vec::new() });
        engine.handle(UciCommand::UciNewGame);

        let lines = engine.handle(perft_go(1));
        assert_eq!(lines.last(), Some(&"Nodes searched: 20".to_string()));
    }

    #[test]
    fn a_played_search_leaves_its_score_for_the_next_and_ucinewgame_clears_it() {
        let mut engine = Engine::new();
        assert_eq!(*engine.last_score.lock().unwrap(), None);
        engine.handle(parse_command("go depth 2"));
        engine.join_and_take_last_search_output();
        assert!(engine.last_score.lock().unwrap().is_some(), "the search's final score is kept");

        engine.handle(UciCommand::UciNewGame);
        assert_eq!(*engine.last_score.lock().unwrap(), None);
    }

    #[test]
    fn quit_and_unknown_produce_no_output() {
        let mut engine = Engine::new();
        assert_eq!(engine.handle(UciCommand::Quit), Vec::<String>::new());
        assert_eq!(engine.handle(UciCommand::Unknown), Vec::<String>::new());
    }
}
