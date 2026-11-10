use crate::board::bitboard::Bitboard;
use crate::board::chess_move::{Move, MoveKind};
use crate::board::color::Color;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use crate::board::Board;
use crate::eval::{evaluate, piece_value};
use crate::movegen::attacks::{is_in_check, king_square};
use crate::movegen::bishop::bishop_attacks;
use crate::movegen::knight::knight_attacks;
use crate::movegen::legal::Legality;
use crate::movegen::pawn::pawn_attacks;
use crate::movegen::{noisy_moves, pseudo_legal_moves};
use crate::movegen::rook::rook_attacks;
use crate::movegen::see::{attackers_to, see_ge};
use crate::nnue::Network;
use crate::tablebase::{SyzygyTablebase, TbResult};
use crate::tt::{Bound, TranspositionTable};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

mod move_picker;
use move_picker::MovePicker;

/// Sentinel magnitude for a forced checkmate -- comfortably above any possible material
/// score (the current evaluation is material-only, so nowhere near this large), meaning a
/// mate score is always distinguishable from "merely up a lot of material." The actual
/// returned value is this constant minus `ply` -- how many plies from *this search's own
/// root* the mate was actually found at (see `alpha_beta_with_buffer`) -- so a faster mate
/// scores more decisively than a slower one. Deliberately not tied to `target_depth`: since
/// negamax's bubbling back to the root only ever negates a score (never changes its
/// magnitude), the root's own returned magnitude ends up being exactly `MATE_SCORE -
/// ply_at_mate` regardless of how deep that particular search was asked to go -- which is
/// also what makes recovering "mate in N" later (`ply_at_mate = MATE_SCORE - abs(score)`)
/// a plain subtraction, with nothing else to account for.
pub const MATE_SCORE: i32 = 32_000;

/// The "infinity" bound alpha-beta's window starts at (root call: `alpha_beta(board, depth,
/// -INFINITY, INFINITY)`), and every recursive call negates and passes down as the child's
/// own window. `i32::MAX` specifically, because that's safe by definition of the type
/// itself -- no score `evaluate`/`negamax`/`alpha_beta` can ever produce needs checking
/// against it, since nothing can exceed `i32::MAX` to begin with.
///
/// Never `i32::MIN`, even though it looks like the more textbook-symmetric choice: negating
/// `i32::MIN` overflows (two's complement has no positive counterpart for it), where
/// negating `-i32::MAX` cleanly yields `i32::MIN + 1`. This isn't a one-off edge case to
/// guard once at the root -- every recursive call computes `-beta`/`-alpha` to pass down as
/// the child's window, so it's on the hot path of every single node.
pub(crate) const INFINITY: i32 = i32::MAX;

/// Absolute ceiling on distance from the root (`ply`), spanning main search and quiescence
/// combined -- real, load-bearing insurance against runaway recursion, not merely symbolic the
/// way a comparable cap is in an engine with proper repetition detection behind it: Fattorini has
/// none yet, so nothing else would otherwise stop a perpetual-check-shaped sequence (a capture
/// that incidentally gives check, chaining into mandatory evasion search, repeating) from
/// recursing indefinitely.
///
/// The *only* depth-related ceiling in the engine now -- there is no separate cap on iterative
/// deepening's own *intent* (`uci.rs`'s `go_search` hands `choose_best_move` whatever depth an
/// explicit `go depth N` asks for, or an effectively unbounded one otherwise, trusting time
/// management -- the headroom-margin/deadline checks in `choose_best_move` -- to be what
/// actually stops an ordinary search, not an arbitrary depth number). But it does directly bound
/// the outer iterative-deepening loop's own requested depth (`choose_best_move_with_progress`'s
/// `for depth in 1..=max_depth.min(MAX_PLY)`), not just recursion distance inside any one search
/// -- a real bug, not a hypothetical one, showed why that second role is load-bearing: with
/// `limit: None` (no time control at all) over a fast-resolving position, every iteration past
/// `MAX_PLY` returns almost instantly (recursion is already clipped at `ply >= MAX_PLY` before it
/// does real work), so nothing but this clamp stops the loop from requesting thousands of ever-
/// larger depths in a fraction of a second, each one cheap enough that time management never
/// gets a chance to react -- and `(target_depth - ply) * (target_depth - ply)` in the history
/// bonus overflowed `i32` once depth climbed far enough. An explicit `go depth N` for some very
/// large `N` in an unusually forced, low-branching line is the other scenario this same clamp
/// guards, silently capping the request at `MAX_PLY` rather than honoring it literally. In
/// ordinary time-bounded play this remains what it always was -- quiescence's own runaway-
/// recursion guard against a pathological forced-capture chain, essentially never reached by the
/// *main* search given real chess's exponential branching (our own measured ~10x node-count
/// growth per depth means reaching anywhere near this ceiling through ordinary play would take
/// longer than is practically possible). Raised from `30` to a deliberately generous `100`
/// specifically because it now carries this dual role alone, with nothing shallower backing it
/// up.
pub const MAX_PLY: u32 = 100;

/// Whether `score` falls in the range that can only mean a detected forced mate for the side
/// to move -- not a graded "how bad does this look" scale, a narrow binary test. Reused
/// directly from Stockfish's own `is_loss(Value)` (fetched from `types.h`, not recalled --
/// CLAUDE.md rule 2), collapsed to one comparison since Fattorini has no tablebase layer to
/// distinguish "search proved a mate" from "a tablebase says this is lost" the way Stockfish's
/// own three-constant version does (see `search.md`'s "Quiescence move-count pruning" design
/// section for the full derivation). Gates the move-count cap inside `quiescence`: while the
/// best score found so far at a node is still in this range, every other pruning decision at
/// that node is skipped too -- finding the one saving resource in a desperate position matters
/// more than pruning cost there.
fn is_loss(score: i32) -> bool {
    score <= -(MATE_SCORE - MAX_PLY as i32)
}

/// The mirror of `is_loss`: whether `score` falls in the range that can only mean a detected
/// forced mate *for* the side to move, rather than against it -- same collapsed-to-one-
/// comparison reasoning (no tablebase layer to distinguish "search proved it" from "a
/// tablebase says so" the way Stockfish's own three-constant version does). Gates razoring
/// (`search.md`'s design section): a mate-scale `alpha` is entirely reachable mid-search, once
/// some sibling branch has already found a forced mate and it bubbled up as the current best
/// score -- and against a bound that large, any ordinary evaluation looks hopeless by
/// comparison, which would otherwise make razoring fire and skip the one kind of search
/// (quiet mating nets, not the captures-and-checks-only `quiescence` razoring falls back to)
/// that could actually deliver on that mate.
fn is_win(score: i32) -> bool {
    score >= MATE_SCORE - MAX_PLY as i32
}

/// The best score achievable from a node at `ply` if the side to move there delivers
/// checkmate on its own very next move -- Stockfish's own `mate_in(ply)` (fetched, not
/// recalled -- rule 2), the standard chess-programming term. Used by mate distance pruning to
/// tighten `beta`: no line starting from that node could ever legitimately score higher than
/// mating on the spot, so a `beta` above this is already unreachable.
fn mate_in(ply: u32) -> i32 {
    MATE_SCORE - ply as i32
}

/// The mirror of `mate_in`: the worst score a node at `ply` could ever report -- being mated
/// immediately, before a single move is even made. Stockfish's own `mated_in(ply)`. Used by
/// mate distance pruning to tighten `alpha`: the true value there can never legitimately be
/// lower than this.
fn mated_in(ply: u32) -> i32 {
    -(MATE_SCORE - ply as i32)
}

/// Sentinel magnitude for a tablebase-proven win with no known distance to conversion --
/// Fattorini's tablebases (`tablebase.rs`) are WDL-only (no DTZ), so unlike a search-proven mate
/// there is no ply count to report here, only "this position is a win." Sits strictly below the
/// real mate band (`MATE_SCORE - MAX_PLY`) so it can never be confused with one, and strictly
/// above any ordinary material-only evaluation. Mirrors Stockfish's own real, fetched constant
/// (`types.h`): `VALUE_TB = VALUE_MATE_IN_MAX_PLY - 1`, translated onto Fattorini's own `MATE_SCORE`/
/// `MAX_PLY` rather than Stockfish's (`246`, chosen for unrelated reasons -- see `MAX_PLY`'s own
/// doc comment).
pub const TB_WIN_SCORE: i32 = (MATE_SCORE - MAX_PLY as i32) - 1;

/// The best score achievable from a node at `ply` given a tablebase-proven win there, faded by
/// search ply exactly like `mate_in` -- `ply` here is purely "how deep into this recursion," not
/// anything looked up from the table itself (WDL carries no distance at all), so this only ever
/// expresses "prefer forcing entry into the known win sooner rather than shuffling," never a real
/// proven mate distance. Never confused with `mate_in`'s own band: `tb_win_at(0) = TB_WIN_SCORE`
/// sits strictly below `mate_in(MAX_PLY) = MATE_SCORE - MAX_PLY`, and stays below it for every
/// `ply` up to `MAX_PLY`, matching the `MAX_PLY`-wide margin Stockfish's own `VALUE_TB_WIN_IN_
/// MAX_PLY = VALUE_TB - MAX_PLY` keeps between the two bands.
pub fn tb_win_at(ply: u32) -> i32 {
    TB_WIN_SCORE - ply as i32
}

/// The mirror of `tb_win_at`: the score for a tablebase-proven loss at `ply`.
pub fn tb_loss_at(ply: u32) -> i32 {
    -(TB_WIN_SCORE - ply as i32)
}

/// Whether `score` falls in the tablebase-win band (`[TB_WIN_SCORE - MAX_PLY, TB_WIN_SCORE]`) --
/// mirrors `is_win`'s own shape, one band lower. Never overlaps `is_win`'s own real-mate band: the
/// two are separated by construction (`TB_WIN_SCORE`'s own doc comment).
pub fn is_tb_win(score: i32) -> bool {
    score >= TB_WIN_SCORE - MAX_PLY as i32 && score <= TB_WIN_SCORE
}

/// The mirror of `is_tb_win`.
pub fn is_tb_loss(score: i32) -> bool {
    score <= -(TB_WIN_SCORE - MAX_PLY as i32) && score >= -TB_WIN_SCORE
}

/// The score to report for a search whose root the tables rank (`SyzygyTablebase::root_result`):
/// search probes only after a zeroing move, so in an ending reached long ago its leaves are the
/// network's, cautious late in a game, and a won KQ vs KN once showed +1.59. A win reports
/// `tb_win_at(0)` and a loss `tb_loss_at(0)`, Stockfish's way, unless the search already found a
/// decisive score of the same sign (a mate, or a capture into the tables), which says at least as
/// much; a draw, cursed wins and blessed losses included, reports 0. `None`, a root the tables do
/// not rank, leaves the score alone. Reporting only: the search, its move and its clock keep the
/// search's own score.
pub fn root_reported_score(search_score: i32, root: Option<TbResult>) -> i32 {
    let decisive = search_score.abs() >= TB_WIN_SCORE - MAX_PLY as i32;
    match root {
        None => search_score,
        Some(TbResult::Win) if decisive && search_score > 0 => search_score,
        Some(TbResult::Win) => tb_win_at(0),
        Some(TbResult::Loss) if decisive && search_score < 0 => search_score,
        Some(TbResult::Loss) => tb_loss_at(0),
        Some(TbResult::Draw) => 0,
    }
}

/// `score`, relative to the root, made relative to the node at `ply` for the transposition table:
/// a winning band (mate or tablebase) gets `ply` added, a losing band `ply` subtracted, anything
/// else is unchanged. Stockfish's `value_to_tt`. The table outlives one search and a position recurs
/// at other plies, so a stored distance must not depend on where it was found: unconverted, a mate
/// reached again deeper looks closer than it is, and a slower line scores as well as a faster one.
/// A converted value stays inside its own band: `mate_in(p)` becomes `MATE_SCORE` and `tb_win_at(p)`
/// becomes `TB_WIN_SCORE`, and the `MAX_PLY`-wide gap between the bands is never crossed.
fn score_to_tt(score: i32, ply: u32) -> i32 {
    if is_win(score) || is_tb_win(score) {
        return score + ply as i32;
    }
    if is_loss(score) || is_tb_loss(score) {
        return score - ply as i32;
    }
    score
}

/// The reverse of `score_to_tt`: a stored, node-relative `score` made relative to the root again
/// from the node at `ply`. Stockfish's `value_from_tt`, without its fifty-move guard (Fattorini scores a
/// fifty-move draw as `0` when the search reaches it, so leaving the guard out costs only a rare
/// stored mate that the rule turns into a draw).
fn score_from_tt(score: i32, ply: u32) -> i32 {
    if is_win(score) || is_tb_win(score) {
        return score - ply as i32;
    }
    if is_loss(score) || is_tb_loss(score) {
        return score + ply as i32;
    }
    score
}

/// The placeholder `best_move` a tablebase-hit entry is stored to the TT with -- `TranspositionTable::store`
/// requires a real `Move`, but a WDL classification is a fact about the *position*, not a move
/// recommendation, so there is no real move to store (Stockfish's own real `tbprobe.cpp` passes
/// `Move::none()` to its own TT write for the identical reason -- confirmed via `search.cpp`,
/// not recalled). `A1` to `A1`: `from == to` can never be a real generated move (every move
/// actually changes square), so this can never accidentally match and get "tried" by the
/// TT-move priority stage in `alpha_beta_with_buffer`'s own move loop -- it just fails to match
/// anything in `moves[i..end]` and falls through, the same already-established, harmless
/// fate a stale TT move from a completely different context already has (see that loop's own
/// doc comment on `tt_move_is_noisy`).
fn tablebase_tt_move() -> Move {
    Move::new(Square::A1, Square::A1, None)
}

/// The shallowest depth searched in an aspiration window; shallower depths take the full window.
/// Five, Stockfish 10's value: below it the previous depth's score is too noisy to centre a narrow
/// window on, and those depths cost so little that a full window loses nothing.
pub(crate) const ASPIRATION_MIN_DEPTH: u32 = 5;

/// How much wider each further Lazy SMP thread's first window is: five centipawns. Threads that
/// fail high and low at different points search different nodes and fill the shared table with
/// different entries, which is the whole benefit of more than one thread; one centipawn could
/// vanish in the evaluation's own granularity. Not a search parameter: tuning games run one thread.
pub(crate) const ASPIRATION_THREAD_STEP: i32 = 5;

/// The half-width of the very first aspiration window tried at a given depth by worker thread
/// `thread_idx` -- see `SearchParameters::aspiration_delta` for why depth-to-depth width exists at
/// all, and `ASPIRATION_THREAD_STEP` for why it also varies by thread. Thread `0` gets exactly
/// `aspiration_delta`, and each further thread `ASPIRATION_THREAD_STEP` more, with no repeat: at the
/// `Threads` ceiling of 128 and the default 20 that is 655 centipawns, still a bounded window.
fn initial_aspiration_delta(aspiration_delta: i32, thread_idx: u32) -> i32 {
    aspiration_delta + ASPIRATION_THREAD_STEP * thread_idx as i32
}

/// The fifty-move rule's own threshold, in the unit `Board::halfmove_clock` actually counts:
/// plies, not moves -- fifty moves by *each* side, with nobody's pawn moving or a capture
/// occurring, is a hundred plies. Treated as an automatic draw once reached, the same
/// simplification `is_repetition` already makes for its own rule (a single repeat, not the
/// literal FIDE threefold): real engines don't wait for an actual claim either, since nothing
/// about search benefits from distinguishing "automatically drawn" from "drawable on request."
const FIFTY_MOVE_HALFMOVES: u8 = 100;

/// How many nodes pass between each read of the clock, once a real `deadline` exists to check
/// against. Throttled rather than read on every node for the same reason Stockfish's own
/// `check_time` throttles it (a countdown counter there; a plain node-count modulo here, since
/// nothing here needs Stockfish's extra nodes-as-time-control mode) -- reading a wall clock on
/// every single node would be needless overhead when only the periodic reads spaced this far
/// apart are actually needed to keep the deadline honest. `1024`: at the roughly 99,000
/// nodes/second measured in the iterative-deepening timing investigation, that's about a 10ms
/// window between checks -- small next to the half of the clock `MAX_CLOCK_FRACTION` always leaves
/// unspent, and a negligible number of actual clock reads per second either way. A
/// placeholder like every other constant here, not a tuned value.
const CHECK_INTERVAL: u64 = 1024;

/// Two pawns, per ply of remaining depth: the default of three of the six margins in
/// `SearchParameters` (classical futility, null move's eval term, delta pruning in quiescence). The
/// six shared it from 2026-10-03, never measured for strength, until a tune and four duels
/// (2026-11-08) moved razoring, reverse futility and SEE pruning to values of their own, each argued
/// in its field's doc comment. In `piece_value`'s pawn; where a margin meets a static evaluation it
/// is scaled to the evaluator's own pawn (`EvalMargins`), so a default holds for any evaluator, and
/// SEE pruning, which compares material with material, reads it as it is.
pub const MARGIN_PER_PLY: i32 = 2 * piece_value(PieceType::Pawn);

/// The values a search prunes, orders and windows by, as run-time parameters rather than constants,
/// so that one process can play two sets against each other. `DEFAULT` is the untuned search; a
/// tune moves every value (`parameters.txt`). The margins are in `piece_value`'s pawn.
///
/// Each default is argued from what its mechanism means in chess, not from one evaluator, since the
/// margins are scaled to each evaluator's own pawn; what suits one evaluator better is its own tune's
/// to find (the developer, 2026-11-08). Razoring, reverse futility, SEE pruning, the aspiration
/// window and the good-quiet threshold moved on 2026-11-08, the lc0 network's first tune showing the
/// way: new against old, PeSTO gained about 29 Elo and the lc0 network about 54 at 10+0.1, both even
/// at depth 9.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchParameters {
    /// Razoring, per ply: a node whose evaluation is this far below alpha goes to quiescence. A pawn
    /// and a half: how far a quiet move could still lift the node per ply, a pawn, and half a pawn
    /// more for quiescence's blindness to anything but captures.
    pub razoring_margin: i32,
    /// Reverse futility, per ply: a node whose static evaluation is this far above beta needs no
    /// search. A pawn: how much the opponent can win back per ply without a tactic, a positional
    /// swing.
    pub reverse_futility_margin: i32,
    /// Classical futility, per ply: a quiet move that cannot plausibly lift the static evaluation
    /// this far, up to alpha, is skipped. Two pawns (`MARGIN_PER_PLY`), wider than reverse
    /// futility's pawn although it asks the mirror question: skipping a quiet wrongly can lose the
    /// best move outright, while a wrong reverse-futility cut only misjudges a node's score.
    pub futility_margin: i32,
    /// Null move, per extra ply the null search loses: one ply for each of these by which the static
    /// evaluation clears beta (`null_move_plies`). Not a gate: a step of reduction. Two pawns
    /// (`MARGIN_PER_PLY`) of surplus a ply.
    pub null_move_margin: i32,
    /// Delta pruning in quiescence: a capture whose victim plus this cannot lift the static
    /// evaluation to alpha is skipped. Two pawns (`MARGIN_PER_PLY`): what a capture can bring beyond
    /// its victim, a promotion threat included.
    pub delta_margin: i32,
    /// SEE pruning in the main search, per ply: a move losing more material than this by static
    /// exchange is skipped, the margin growing with how much the remaining search could find. A pawn
    /// and a half: how much material a move may give up per ply for compensation the search could
    /// still find. Must stay positive: the threshold is `-(margin * remaining)`.
    pub see_prune_margin: i32,
    /// How much material a capture may lose by static exchange and still be searched in quiescence:
    /// half a pawn. SEE counts material only, so a slightly losing capture can still be the right
    /// one for a reason it cannot see; Stockfish lets through `see_ge(move, -74)` for the same
    /// reason. Half a pawn is a little stricter than that.
    pub see_margin: i32,
    /// The first aspiration window's half-width, around the previous depth's score: ten
    /// centipawns, a window of 20. Narrow enough to cut most of the tree when the score holds, wide
    /// enough that it usually does; a failure only widens it and searches again. How far a score
    /// moves from one depth to the next is the evaluator's own, so no value is right for every
    /// evaluator: ten is a middle one, half Stockfish 10's twenty, the lc0 network's tune having
    /// ended at seven.
    pub aspiration_delta: i32,
    /// History pruning's threshold, in units of `remaining^2`, the size of one history bonus at that
    /// depth: a quiet move is skipped once its history is worse than 35 such failures. Stockfish's
    /// `-4136 * depth` threshold, divided by its own bonus at depths 4 and 10, comes to 35 to 37
    /// failures; the same order here keeps the technique as conservative as Stockfish's.
    pub history_pruning_multiplier: i32,
    /// The move picker's good-quiet threshold (`move_picker`): a quiet whose combined history score
    /// is above it is searched before the losing captures, one at or below it after them. In history
    /// points, a weighted net cutoff rate (`apply_history_bonus`), so it holds only for today's
    /// history: the `d * d` bonus, the two tables' ceilings, and tables fresh per search. At amanda's
    /// generation 1 about 6% of quiets score below it, Stockfish's "small tail" of clearly bad
    /// quiets. `-128` was chosen by duels against `0`, the other measured candidate: drawn at depth 9
    /// and at 10s+0.1, both leaning to `-128` by about 20 Elo; `-140` since 2026-11-08, where the lc0
    /// network's tune settled, a little stricter still.
    pub good_quiet_threshold: i32,
}

impl SearchParameters {
    /// Every value its reasoned default, each argued in its field's doc comment.
    pub const DEFAULT: SearchParameters = SearchParameters {
        razoring_margin: 3 * piece_value(PieceType::Pawn) / 2,
        reverse_futility_margin: piece_value(PieceType::Pawn),
        futility_margin: MARGIN_PER_PLY,
        null_move_margin: MARGIN_PER_PLY,
        delta_margin: MARGIN_PER_PLY,
        see_prune_margin: 3 * piece_value(PieceType::Pawn) / 2,
        see_margin: piece_value(PieceType::Pawn) / 2,
        aspiration_delta: 10,
        history_pruning_multiplier: -35,
        good_quiet_threshold: -140,
    };

    /// The fields' names, in declaration order: the keys of a parameters file, and the order of
    /// `values` and `from_values`.
    pub const NAMES: [&'static str; 10] = [
        "razoring_margin",
        "reverse_futility_margin",
        "futility_margin",
        "null_move_margin",
        "delta_margin",
        "see_prune_margin",
        "see_margin",
        "aspiration_delta",
        "history_pruning_multiplier",
        "good_quiet_threshold",
    ];

    /// The fields as an array, in `NAMES`' order.
    pub fn values(&self) -> [i32; 10] {
        [
            self.razoring_margin,
            self.reverse_futility_margin,
            self.futility_margin,
            self.null_move_margin,
            self.delta_margin,
            self.see_prune_margin,
            self.see_margin,
            self.aspiration_delta,
            self.history_pruning_multiplier,
            self.good_quiet_threshold,
        ]
    }

    /// The inverse of `values`.
    pub fn from_values(values: [i32; 10]) -> SearchParameters {
        let [razoring_margin, reverse_futility_margin, futility_margin, null_move_margin, delta_margin, see_prune_margin, see_margin, aspiration_delta, history_pruning_multiplier, good_quiet_threshold] =
            values;
        SearchParameters {
            razoring_margin,
            reverse_futility_margin,
            futility_margin,
            null_move_margin,
            delta_margin,
            see_prune_margin,
            see_margin,
            aspiration_delta,
            history_pruning_multiplier,
            good_quiet_threshold,
        }
    }

    /// The text of a parameters file: one `name<TAB>value` line per field, in `NAMES`' order.
    pub fn to_file_text(&self) -> String {
        let mut text = String::new();
        for (name, value) in SearchParameters::NAMES.iter().zip(self.values()) {
            text.push_str(&format!("{name}\t{value}\n"));
        }
        text
    }

    /// The parameters in a parameters file's `text`, read from `path` (named in errors). Every field
    /// exactly once, in any order, and nothing else: a tuned player silently searching with a
    /// default for a misspelled field would be found only by its play.
    pub fn parse_file_text(path: &str, text: &str) -> Result<SearchParameters, String> {
        let mut values: [Option<i32>; 10] = [None; 10];
        for line in text.lines() {
            let Some((name, value)) = line.split_once('\t') else {
                return Err(format!("{path}: expected \"name<TAB>value\", got {line:?}"));
            };
            let Some(index) = SearchParameters::NAMES.iter().position(|&known| known == name) else {
                return Err(format!("{path}: unknown parameter {name:?}"));
            };
            let Ok(value) = value.trim().parse::<i32>() else {
                return Err(format!("{path}: {name} is not a whole number: {value:?}"));
            };
            if values[index].replace(value).is_some() {
                return Err(format!("{path}: {name} appears twice"));
            }
        }
        let mut complete = [0; 10];
        for (index, value) in values.iter().enumerate() {
            let Some(value) = value else {
                return Err(format!("{path}: {} is missing", SearchParameters::NAMES[index]));
            };
            complete[index] = *value;
        }
        Ok(SearchParameters::from_values(complete))
    }

    /// The parameters file at `path`: `None` when there is none, an `Err` when it cannot be read or
    /// parsed.
    pub fn read_file(path: &str) -> Result<Option<SearchParameters>, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("{path}: {error}")),
        };
        SearchParameters::parse_file_text(path, &text).map(Some)
    }

    /// Writes these parameters to `path`, through a `.tmp` file renamed into place, so a reader never
    /// sees half a file.
    pub fn write_file(&self, path: &str) -> Result<(), String> {
        let tmp_path = format!("{path}.tmp");
        std::fs::write(&tmp_path, self.to_file_text()).map_err(|error| format!("{tmp_path}: {error}"))?;
        std::fs::rename(&tmp_path, path).map_err(|error| format!("{path}: {error}"))?;
        Ok(())
    }
}

/// The four margins of `SearchParameters` compared with a static evaluation at every node, in the
/// evaluator's own pawn (`Search::in_evaluator_pawns`): kept rather than recomputed at every node.
/// Delta pruning's margin is converted together with its victim's value, where it is used.
#[derive(Clone, Copy, Debug)]
struct EvalMargins {
    razoring: i32,
    reverse_futility: i32,
    futility: i32,
    null_move: i32,
    /// One pawn: the correction history's bound on a correction, and half its bound on an error.
    pawn: i32,
}

/// The deepest remaining depth at which classical and reverse futility prune: three plies. Both
/// trust a static evaluation to stand in for a search; beyond three plies of search left, that
/// search can find too much for a static guess to be trusted, however wide the margin.
const FUTILITY_MAX_DEPTH: u32 = 3;

/// The plies null-move pruning's search loses at the shallowest depth it applies at, the null move's
/// own ply included: three, the classic fixed R of 2 plus that ply.
const NULL_MOVE_BASE_PLIES: u32 = 3;

/// The null search loses one more ply per this much remaining depth: about 5 plies at depth 9 and 6
/// at depth 13. Stockfish's form around its versions 9 and 10; its later `7 + depth / 3` prunes far
/// harder, tuned together with features Fattorini lacks, and Fattorini does not prune harder.
const NULL_MOVE_DEPTH_DIVISOR: u32 = 4;

/// The most extra plies the evaluation's margin over beta adds, one per null move margin: a position
/// already far above beta needs less depth to confirm the cutoff.
const NULL_MOVE_MAX_EVAL_PLIES: i32 = 3;

/// The least remaining depth null-move pruning applies at: four, so that with the floor of
/// `null_move_plies` the null search keeps at least one full ply of its own instead of falling
/// straight into quiescence.
const NULL_MOVE_MIN_REMAINING: u32 = NULL_MOVE_BASE_PLIES + 1;

/// How many plies shallower than this node the null-move search runs, the null move's own ply
/// included: `NULL_MOVE_BASE_PLIES`, plus one per `NULL_MOVE_DEPTH_DIVISOR` of `remaining`, plus one
/// per `margin_per_ply` (the evaluator's scaled null move margin) that the static evaluation clears
/// beta by (`eval_margin`, never negative at the call site), at most `NULL_MOVE_MAX_EVAL_PLIES` of
/// those. At most `remaining - 1`, so the null search keeps a ply; at `NULL_MOVE_MIN_REMAINING`
/// that floor gives exactly the base three.
fn null_move_plies(remaining: u32, eval_margin: i32, margin_per_ply: i32) -> u32 {
    let eval_plies = (eval_margin / margin_per_ply).clamp(0, NULL_MOVE_MAX_EVAL_PLIES) as u32;
    let plies = NULL_MOVE_BASE_PLIES + remaining / NULL_MOVE_DEPTH_DIVISOR + eval_plies;
    plies.min(remaining - 1)
}

/// Internal Iterative Reduction: a node with no TT move to order by is searched one ply shallower.
/// One ply at every node type; Stockfish's larger reductions on cut nodes rest on node types, which
/// Fattorini does not track.
const IIR_REDUCTION: u32 = 1;

/// The least remaining depth IIR applies at: four, Stockfish's `depth >= 4`. Shallower, a missing TT
/// move costs too little to be worth a reduction.
const IIR_MIN_REMAINING: u32 = 4;

/// How much Internal Iterative Reduction shaves off this node's own `target_depth`: `IIR_REDUCTION`
/// with no TT move to order by at `remaining >= IIR_MIN_REMAINING` and `ply > 0`, `0` otherwise
/// (the ordinary, unreduced case, the overwhelming majority of calls). A free function rather than
/// inlined at the one call site, matching `is_lmr_eligible`/`lmr_reduction`'s own split -- lets the
/// trigger condition and its effect be unit tested directly against plain inputs, not only
/// observed as an emergent property of a full search (this project's own killer/move-ordering
/// node-count tests were deleted for exactly that fragility -- unrelated techniques changing
/// overall node counts broke them for reasons that had nothing to do with the thing they were
/// meant to check).
///
/// `ply > 0`, matching `is_null_move_pruning_applicable`'s own root exclusion: at the root, and
/// only at the root, `target_depth` is also what gets *reported* -- `choose_best_move_with_progress`
/// pushes `IterationReport { depth, .. }` using its own outer loop variable, unaffected by whatever
/// this function shadows internally, so an unreduced report next to a silently-reduced actual
/// search would misrepresent what was really checked. Worse than a reporting nuance, discussed
/// directly with the developer: it would also mean an iteration whose root lacks a TT move doesn't
/// actually advance iterative deepening's own real depth at all, defeating the point of that
/// iteration outright, not just mislabeling it. Interior nodes have no such externally-visible
/// depth promise to break, so this exclusion is root-only, not a blanket low-`ply` one.
fn iir_reduction(ply: u32, tt_move: Option<Move>, remaining: u32) -> u32 {
    if ply > 0 && tt_move.is_none() && remaining >= IIR_MIN_REMAINING {
        return IIR_REDUCTION;
    }
    0
}

/// Every structural precondition for null-move pruning except the `eval >= beta` cost/benefit
/// pre-filter itself (checked separately at the call site, against the already-computed `eval`
/// value, since recomputing it here would need `board`/`evaluate` this function has no reason to
/// otherwise depend on) and except the recursive probe's own outcome. A pure function, directly
/// unit-tested, the same pattern `is_lmr_eligible`/`is_history_pruned` already use:
/// - `ply > 0`: the same root-exclusion every other bare-score cutoff in this function already
///   needs (see the TT probe's own doc comment for the real game that caught this once) -- a
///   cutoff here would report a score with no move behind it, exactly the `bestmove 0000` bug
///   class.
/// - `!in_check`: passing while in check isn't a legal concept to begin with.
/// - `!null_move_in_progress`: refuses to stack a second null move directly on the first, which
///   would mean neither side actually moved and would tell this position nothing real about
///   itself.
/// - `remaining >= NULL_MOVE_MIN_REMAINING`: see that constant's own doc comment for the
///   floor reasoning.
/// - `!is_win(beta) && !is_tb_win(beta)`: skipped while hunting a proven mate or tablebase win,
///   since letting the opponent's king "escape" for a free tempo can hide a real forced result
///   a genuine reply would not have allowed. Currently unreachable in practice given
///   `evaluate`'s own bounded range (nowhere close to either band), since the `eval >= beta`
///   pre-filter checked separately already rules out a mate-or-TB-scale `beta` on its own --
///   kept anyway as a defensive, standard guard against a future evaluation function (rule 7's
///   eventual learned one) whose range might not stay so comfortably bounded. `is_tb_win`
///   added alongside the tablebase work: `TB_WIN_SCORE` sits deliberately below `is_win`'s own
///   threshold (so a TB score prints as `cp`, not `mate`), so without this addition a TB-scale
///   `beta` would have sailed straight past a guard whose whole point is "don't trust a
///   heuristic shortcut against a proven, bounded-below-only-by-search-ply result."
/// - `has_non_pawn_material`: refuses to trust "a free tempo never hurts" in king-and-pawn
///   endings, where passing can genuinely be the best move (zugzwang) -- the caller passes
///   `has_non_pawn_material(board, color)` straight through, reusing late-move pruning's own
///   identical zugzwang guard rather than a second, separately-named copy of the same check.
fn is_null_move_pruning_applicable(
    ply: u32, in_check: bool, null_move_in_progress: bool, remaining: u32, beta: i32, has_non_pawn_material: bool,
) -> bool {
    ply > 0
        && !in_check
        && !null_move_in_progress
        && remaining >= NULL_MOVE_MIN_REMAINING
        && !is_win(beta)
        && !is_tb_win(beta)
        && has_non_pawn_material
}

/// Quiescence's move-count cap: once two ordinary captures (not a check, a recapture or a
/// promotion) have been tried at a node without a cutoff, no further one is. Stockfish's
/// `moveCount > 2` in `qsearch`: captures come best first, so the third and later rarely matter.
const QUIESCENCE_MOVE_LIMIT: u32 = 2;

/// Finds the highest-scoring move in `moves[from..until]` and swaps it into `moves[from]`,
/// but only when it's an actual improvement over the quiet baseline -- `score` returns `0`
/// for every quiet move, so any strictly positive score means a capture or promotion. Reports
/// whether it did anything, rather than unconditionally acting: this is one stage in a
/// priority chain (see its call site), and a stage that found nothing worth doing has to say
/// so, so the next-priority stage (killer, then whatever's left) gets a chance instead of
/// this one silently settling for an arbitrary tied quiet move.
///
/// One incremental step of a lazy selection sort, not a full sort up front: each call only
/// scores the not-yet-examined remainder, unlike sorting the whole slice unconditionally
/// before the loop even starts. Plain linear scan, not `sort_unstable_by_key` or similar --
/// quicksort/mergesort need the whole input to produce *any* ordering at all, so neither can
/// answer "just the next-best one" without doing the full sort regardless, the wrong shape
/// for what this loop needs (chess's branching factor is small enough that the difference
/// wouldn't matter much either way, but that's not the real reason a batch sort doesn't fit
/// here).
///
/// Quiet moves are not scored at all: only a strictly positive score is ever picked, and a quiet
/// move never scores above 0 under either scorer (`move_score` gives it 0; under `see` its own
/// gain is 0 and recaptures only subtract), so skipping it changes no pick. With `see` as the scorer that is most of the scan's cost saved.
///
/// Equal scores are broken by `tie_key`, the smallest first, never by position in the buffer:
/// the swaps here and in `try_killer` move other entries around, quiets included, so a
/// position-based tie-break would make the pick depend on which quiets the buffer holds. With the
/// key, the pick depends only on the set of moves in `moves[from..until]`, so a buffer holding the
/// noisy moves alone searches the same tree.
fn try_best_noisy_move(
    moves: &mut [Move],
    from: usize,
    until: usize,
    board: &Board,
    score: fn(&Board, Move) -> i32,
) -> bool {
    let noisy_score = |mv: Move| if is_quiet(board, mv) { i32::MIN } else { score(board, mv) };
    let mut best = from;
    let mut best_score = noisy_score(moves[best]);
    for i in (from + 1)..until {
        let s = noisy_score(moves[i]);
        if s > best_score || (s == best_score && tie_key(moves[i]) < tie_key(moves[best])) {
            best_score = s;
            best = i;
        }
    }
    if best_score <= 0 {
        return false;
    }
    moves.swap(from, best);
    true
}

/// A total order on the moves of one position, for `try_best_noisy_move`'s ties: `from`, then
/// `to`, then the promotion piece (`None` first). `from` and `to` alone tie only between one pawn's
/// promotions, which the last field separates.
fn tie_key(mv: Move) -> (usize, usize, usize) {
    let promotion = mv.promotion().map_or(0, |piece_type| piece_type as usize + 1);
    (mv.from() as usize, mv.to() as usize, promotion)
}

/// If `killer` is `Some` and present anywhere in `moves[from..until]`, swaps it into
/// `moves[from]` and reports success. Reports failure (no-op) otherwise -- either no killer
/// has been recorded for this depth yet (`None`), or it has but isn't a candidate here at
/// all (it might not even be pseudo-legal at this node, since it was recorded at a *sibling*
/// node that isn't guaranteed to share every candidate with this one). Taking `Option<Move>`
/// directly, rather than making the caller unwrap it first, keeps the call site a plain function
/// call. Used by `quiescence` to put its TT move first; the main search uses the move picker.
fn try_killer(moves: &mut [Move], from: usize, until: usize, killer: Option<Move>) -> bool {
    let killer = match killer {
        Some(killer) => killer,
        None => return false,
    };
    if let Some(position) = (from..until).find(|&j| moves[j] == killer) {
        moves.swap(from, position);
        return true;
    }
    false
}

/// Looks up `history`'s recorded bonus for `mv`, indexed by the moving piece and destination
/// square -- `[piece][to]`, not `[from][to]` (see the design doc for why this shape fits
/// Fattorini's own node-count scale better than the other standard one). The piece is read straight
/// off `board` at `mv`'s own origin square: `board` here is always the position *before* `mv`
/// is made, so `mv.from()` is guaranteed to hold whichever piece is about to move.
fn history_score(history: &[[i32; 64]; 12], board: &Board, mv: Move) -> i32 {
    let piece = board.piece_at(mv.from()).expect("history_score: no piece on the from square");
    history[piece as usize][mv.to() as usize]
}

/// `history_score` plus continuation history's own bonus for the same move, given whatever move
/// was actually played one ply before it (`prev_move`) -- a plain, unweighted sum, matching
/// Stockfish's own real combination (fetched, not assumed; see `continuation-history.md`). Used
/// only for move-ordering (the move picker), deliberately *not* by `is_history_pruned`:
/// that pruning decision stays on `history_score` alone, on purpose
/// (`continuation-history.md`'s own "decided" section) -- folding a second, much sparser signal
/// into a decision that skips searching a move entirely is a bigger soundness commitment than
/// just reordering an already-going-to-be-searched move. `prev_move: None` (the root, or right
/// after a null move) makes the continuation term `0`, so this is exactly `history_score`'s own
/// unmodified value there.
fn combined_history_score(
    history: &[[i32; 64]; 12], continuation_history: &[i32], prev_move: Option<(Piece, Square)>, board: &Board,
    mv: Move,
) -> i32 {
    let piece = board.piece_at(mv.from()).expect("combined_history_score: no piece on the from square");
    let flat = history[piece as usize][mv.to() as usize];
    let continuation = match prev_move {
        Some((prev_piece, prev_to)) => {
            continuation_history[continuation_history_index(prev_piece, prev_to, piece, mv.to())]
        }
        None => 0,
    };
    flat + continuation
}

/// A move's ordering priority -- higher searches first. Captures are scored MVV-LVA (Most
/// Valuable Victim, Least Valuable Attacker): `10 *` the victim's value so even the
/// cheapest capture of a queen outranks the priciest capture of a pawn, minus the
/// attacker's own value so that, between two captures of equally valuable victims, the one
/// giving up less material if the capture turns out to be unsound is tried first. A
/// promotion adds the promoted piece's own value on top, win or not -- reaching the last
/// rank is forcing enough to search early regardless. A quiet move (no capture, no
/// promotion) scores `0`, searched last, in whatever order it was generated.
///
/// The main search tree does not use this for its own ordering (the move picker scores captures
/// by victim and checks SEE) -- kept here, unchanged, purely because `quiescence` still calls it
/// directly. That's
/// deliberate, not an oversight: `quiescence`'s own move loop treats `try_best_noisy_move`
/// reporting failure (`best_score <= 0`) as proof that *no capture at all* remains among
/// `moves[i..end]`, safe to `break` on rather than merely `continue` past -- a guarantee this
/// function provides structurally (`10 *` any real victim's value always exceeds any single
/// attacker's own, so a genuine capture can never score `<= 0` here). Real SEE values can
/// legitimately be zero (an exactly even trade) or negative (a losing one), which would silently
/// break that guarantee and start returning `false` while real captures -- ones `quiescence`'s
/// own separate `see_margin` gate deliberately still wants a look at -- are still sitting
/// unexamined in the buffer.
fn move_score(board: &Board, mv: Move) -> i32 {
    let mut score = 0;

    if let Some(victim) = captured_piece(board, mv) {
        let attacker = board.piece_at(mv.from()).expect("move_score: no piece on the from square");
        score += 10 * piece_value(victim.piece_type()) - piece_value(attacker.piece_type());
    }

    if let Some(promotion) = mv.promotion() {
        score += piece_value(promotion);
    }

    score
}

/// Whether `mv` is quiet -- no capture, no promotion. Gates killer *recording*: only a quiet
/// move can ever become a killer, since captures/promotions already have MVV-LVA's own
/// reliable ranking and don't need this heuristic's help. Kept as its own predicate rather
/// than checking `move_score(board, mv) == 0`, so recording behaves identically regardless of
/// whichever scoring strategy happens to be plugged in -- notably including a hypothetical
/// scoring strategy that ties every move at `0` (captures included), so it can't be used to
/// answer "is this quiet" the way `move_score` itself reliably can.
fn is_quiet(board: &Board, mv: Move) -> bool {
    captured_piece(board, mv).is_none() && mv.promotion().is_none()
}

/// Whether `color` has at least one knight, bishop, rook, or queen -- Stockfish's own
/// `non_pawn_material(us)` (fetched, not recalled -- rule 2), used to guard late-move pruning
/// against the one scenario where skipping a quiet move is genuinely dangerous: a pure king-
/// and-pawn ending for the side to move, where zugzwang means almost any move can be forced and
/// "just try a different quiet move" isn't a safe substitute -- the identical reasoning
/// null-move pruning's own zugzwang guard needs this same check for (see
/// `is_null_move_pruning_applicable`'s own call site in `alpha_beta_with_buffer`), not a
/// hypothetical technique Fattorini doesn't have -- both consumers share this one function rather
/// than each rolling their own copy of the same material check.
fn has_non_pawn_material(board: &Board, color: Color) -> bool {
    board.bitboard(Piece::new(color, PieceType::Knight)) != Bitboard::EMPTY
        || board.bitboard(Piece::new(color, PieceType::Bishop)) != Bitboard::EMPTY
        || board.bitboard(Piece::new(color, PieceType::Rook)) != Bitboard::EMPTY
        || board.bitboard(Piece::new(color, PieceType::Queen)) != Bitboard::EMPTY
}

/// How many moves may be tried at a node before late-move pruning skips any further quiet one:
/// `(3 + remaining^2)`, halved when not improving -- Stockfish's own formula, plain integers with no
/// evaluation scale in them. Quadratic in depth, so it hardly prunes where much search is left.
/// Always at least 1, so a node's first move is never pruned.
fn late_move_pruning_threshold(remaining: u32, improving: bool) -> u32 {
    (3 + remaining * remaining) / if improving { 1 } else { 2 }
}

/// Whether `mv` should be skipped outright by late-move pruning, at a node where
/// `applicable` (the node-level gate: `ply > 0 && !in_check && has_non_pawn_material`,
/// precomputed once per node, not per move) already holds. A pure function of its own
/// arguments, not a `Search` method, so it is tested against hand-picked inputs rather than only
/// through a full search, whose node counts every other technique also moves.
fn is_late_move_pruned(
    applicable: bool, best_score: i32, board: &Board, mv: Move, moves_tried: u32, remaining: u32, improving: bool,
) -> bool {
    // `!is_loss(best_score) && !is_tb_loss(best_score)`: skipped while already facing a proven
    // mate or tablebase loss, since one of these still-untried quiet moves could be exactly the
    // one line that avoids or delays it -- `is_tb_loss` added alongside the tablebase work, for
    // the identical reason `is_null_move_pruning_applicable`'s own `!is_tb_win` addition was:
    // `TB_WIN_SCORE`'s band sits below `is_loss`'s own threshold on purpose (so it prints as
    // `cp`), so without this a TB-scale `best_score` would have skipped this guard entirely.
    applicable
        && !is_loss(best_score)
        && !is_tb_loss(best_score)
        && is_quiet(board, mv)
        && moves_tried >= late_move_pruning_threshold(remaining, improving)
}

/// Whether `mv`, already known not to be this node's first searched move (the caller only asks
/// once `best_move.is_some()`, the same signal PVS itself keys off of), is worth a cheap
/// reduced-depth probe before the normal null-window scout. `remaining >= 3`, not Stockfish's own
/// literal entry gate (`depth >= 2`, fetched, not recalled -- rule 2): Stockfish's real formula
/// pairs that entry gate with its own floor (`std::max(1, ...)`) guaranteeing the reduced child
/// never drops below 1 ply of real search of its own. Fattorini's reduction is expressed as `lmr_
/// reduction(remaining, moves_tried)` extra plies taken from `target_depth` on top of the move's
/// own normal decrement (see the call site and `lmr_reduction`'s own doc comment for why `remaining
/// >= 3` is still exactly the right floor even now that the reduction itself grows with `remaining`
/// and `moves_tried`, rather than always being a flat `1`).
///
/// `!gives_check(board, mv)` is a real change from this function's own original reasoning, not an
/// oversight: an earlier version of this doc comment argued deliberately *against* a check-giving
/// exemption here, for symmetry with `is_late_move_pruned` (which still has none, a separate,
/// deferred decision -- pruning a move outright and reducing its depth are different enough
/// techniques that they don't have to move in lockstep). That reasoning held right up until search
/// extensions gave checking moves their own, opposite treatment: reducing *and* extending the same
/// move would be incoherent, not just redundant, so extension has to win outright, not merely be
/// favored. `is_quiet` still matters on its own besides that -- a capture that also happens to give
/// check is still exempted via the ordinary `is_quiet` route, not because of the check itself.
///
/// A pure function, not a `Search` method, for the same reason `is_late_move_pruned` is one.
fn is_lmr_eligible(remaining: u32, board: &Board, mv: Move) -> bool {
    remaining >= 3 && is_quiet(board, mv) && !gives_check(board, mv)
}

/// How many extra plies (beyond the one every recursive call already loses via `ply + 1`) an
/// `is_lmr_eligible` move's reduced probe skips -- grown along two independent axes, both on a
/// logarithmic curve, not Stockfish's own real, heavily-tuned formula (`reductions[depth] *
/// reductions[moveCount] / 1024`, built from `log()`-table floating-point constants fitted through
/// self-play -- fetched, not recalled, rule 2), which is exactly the kind of unexplainable-magic-
/// number shape this session already ruled out once for reverse futility pruning's own `improving`
/// extension. The same *structural* idea Stockfish's own formula uses -- both `remaining` and
/// `moves_tried` should earn extra reduction only by *multiplying*, not merely increasing, since
/// the cost of searching a subtree grows exponentially with depth and the number of siblings still
/// to try -- without adopting Stockfish's own tuned arithmetic to get there: this adds the two
/// log-shaped terms rather than multiplying and rescaling them, and uses Rust's exact integer
/// `ilog` rather than continuous floating-point `log()` tables fitted to self-play data.
///
/// `u32::ilog(3)` on both terms introduces no new tuned constant at all: the base `3` is the same
/// `3` `is_lmr_eligible` already fixes as the minimum eligible `remaining`, reused for
/// `moves_tried` too rather than inventing an independently-reasoned second base. `moves_tried` at
/// the call site already includes the current move itself (the loop increments it before reaching
/// here), so the earliest possible eligible call has `remaining == 3, moves_tried == 2` --
/// `3.ilog(3) + 2.ilog(3) == 1 + 0 == 1`, identical to this technique's original flat-`1` behavior
/// at that exact boundary, so this is still a strict generalization of what already existed, not a
/// departure from it. The move-count term only starts contributing once `moves_tried` reaches `3`,
/// the depth term only regrows once `remaining` reaches `9`.
///
/// Floor safety (the reduced probe must always keep at least 1 real ply: `remaining - 1 -
/// lmr_reduction(...) >= 1`) is no longer provable from relative growth rates alone the way the
/// single-axis version was -- two independently-safe terms don't automatically stay safe combined.
/// At `remaining == 3` the depth term alone already saturates the floor (`3.ilog(3) == 1` is the
/// most `remaining - 2 == 1` allows), so an unbounded `moves_tried` term (real chess allows up to
/// ~218 legal moves in a position, `218.ilog(3) == 4`) could otherwise send the reduced probe's
/// own `remaining` negative -- not just unsound, an actual `u32` underflow panic at the call site's
/// `target_depth - lmr_reduction(...)`. `.min(remaining - 2)` fixes this by construction instead:
/// `remaining - 1 - lmr_reduction(...) >= remaining - 1 - (remaining - 2) == 1` holds unconditionally,
/// a direct guarantee rather than an asymptotic argument. The real consequence: the move-count term
/// only ever has room to matter at nodes with `remaining` comfortably above the minimum, where the
/// depth term alone hasn't already used up all the available slack.
fn lmr_reduction(remaining: u32, moves_tried: u32) -> u32 {
    let unclamped = remaining.ilog(3) + moves_tried.ilog(3);
    unclamped.min(remaining - 2)
}

/// Whether `mv` should be skipped outright by history-based pruning: a second, independent gate
/// alongside `is_late_move_pruned` (a move can fail either, both, or neither), keyed on the
/// move's own accumulated track record across the whole search rather than on how many moves
/// have already been tried at this one node. `applicable` mirrors futility pruning's own gate
/// (`best_move.is_some()`), for the identical reason: a node must always find at least one real
/// move, so the mandatory first candidate is never a pruning target. `is_quiet` matters here
/// specifically because `history` is only ever written for quiet moves (both the bonus and malus
/// sites gate on it) -- a noisy move's `[piece][to]` slot could hold a stale score left behind by
/// an unrelated quiet move sharing the same destination, so reading it for a capture would be
/// noise, not signal. A pure function, not a `Search` method, for the same reason
/// `is_late_move_pruned` is one.
fn is_history_pruned(
    applicable: bool, history: &[[i32; 64]; 12], board: &Board, mv: Move, remaining: u32, multiplier: i32,
) -> bool {
    applicable && is_quiet(board, mv) && history_score(history, board, mv) < multiplier * (remaining * remaining) as i32
}

/// Whether `mv` should be skipped outright on the strength of its own static exchange evaluation
/// alone -- the first pruning technique in this loop that isn't limited to quiet moves: unlike
/// `is_late_move_pruned`/`is_history_pruned`/classical futility pruning above (all three exempt
/// captures via their own `is_quiet` guard), this one applies to any move type, since `see_ge`'s
/// own accounting already puts a hanging quiet move and a bad capture on the same numeric footing,
/// so one margin serves both. A pure function, directly unit-tested, as every other pruning
/// predicate here.
/// - `applicable` folds in `best_move.is_some()` -- the same "never prunes the mandatory first
///   move at a node" guarantee `is_history_pruned`/futility pruning above already preserve -- and
///   `!gives_check(board, mv)`, mirroring `is_lmr_eligible`'s own exemption: a move earning a
///   check extension shouldn't be discardable by the same static snapshot in the same breath.
/// - `!see_ge(board, mv, -(margin * remaining as i32))`: prunes once the move's own
///   forced-exchange result nets worse than the margin allows, the margin growing linearly with
///   how much `remaining` search could still find compensation. Unbounded in depth: SEE is an
///   exact material count for one move, not a whole-board guess, so it needs no depth cap the way
///   futility's static evaluation does.
fn is_see_pruned(applicable: bool, board: &Board, mv: Move, remaining: u32, margin: i32) -> bool {
    applicable && !see_ge(board, mv, -(margin * remaining as i32))
}

/// Whether `mv` recaptures on the same square the immediately preceding move (if any) landed
/// on -- Stockfish's own `move.to_sq() != prevSq` exemption from the move-count cap, inverted
/// to a positive predicate. `prev_to_sq` is `None` at the very first move `quiescence`'s own
/// recursion searches (see that parameter's own doc comment), so nothing is ever exempted as a
/// recapture there -- only a move made *within* an already-running quiescence chain can
/// recapture anything, by definition.
fn is_recapture(mv: Move, prev_to_sq: Option<Square>) -> bool {
    prev_to_sq == Some(mv.to())
}

/// Whether `mv`, played on `board` (before it's actually made -- no `make_move`/`unmake_move`
/// anywhere in here), gives check to the opponent's king. Two independent mechanisms, checked
/// separately since they're genuinely different: **direct check**, the moving piece itself
/// attacking the enemy king from its new square, and **discovered check**, moving off `mv.from()`
/// exposing an already-aimed friendly slider that piece was blocking. Mirrors what current
/// Stockfish's own `Position::gives_check` computes (`checkSquares`/`blockersForKing`, fetched --
/// not recalled -- CLAUDE.md rule 2), but built by reusing `attackers_to` (`see.rs`'s own
/// reverse-ray-casting primitive) for the harder, sliding-piece half of both computations,
/// rather than Stockfish's own per-node-cached bitboards -- this recomputes a full ray-cast on
/// every call instead, the simpler, provably-correct version to land first (this project's own
/// land-the-naive-version-first methodology, same as delta pruning before SEE), revisited only
/// if profiling ever shows the recomputation cost matters (rule 5).
///
/// **Known, deliberate gap: castling is not handled.** `Move` doesn't even carry the rook's own
/// from/to for a castling move (see `Move`'s own doc comment -- they're implied by the king's
/// destination, a make/unmake concern), and the rook's own jump could independently give check
/// on top of (or instead of) the king's. Unreached by `quiescence`'s own call site today (castling
/// is quiet, never a candidate in that not-in-check noisy-move loop), so not built until a real
/// caller needs it -- exactly the kind of documented, reachability-scoped gap this file's own
/// `quiescence` doc comment already keeps for stalemate detection.
fn gives_check(board: &Board, mv: Move) -> bool {
    let color = board.side_to_move();
    let enemy_king = king_square(board, color.opposite());

    // Occupancy after `mv`, built the same way SEE's own x-ray computation already does
    // (`from` empties; en passant additionally empties the captured pawn's own square,
    // distinct from `to`) -- plus explicitly setting `to`, which SEE never needed (its own
    // ray-casts are anchored *at* `to`, where a square's own occupancy bit doesn't affect
    // what its own attack table returns). Here the ray-casts are anchored at the enemy king
    // instead, so whether `to` blocks some other slider's path genuinely matters.
    let mut occupancy = board.all_occupancy() ^ Bitboard::from_square(mv.from());
    if mv.kind() == MoveKind::EnPassantCapture {
        let captured_pawn_square = Square::new(mv.to().file(), mv.from().rank());
        occupancy = occupancy ^ Bitboard::from_square(captured_pawn_square);
    }
    occupancy = occupancy | Bitboard::from_square(mv.to());

    // Direct check: same "place a virtual piece on this square and see what it would attack"
    // technique `is_square_attacked` already uses, just for one specific piece type and square
    // (the mover, now on `to`) against one specific target (the enemy king) instead of scanning
    // every type against every real attacker. `mv.promotion().unwrap_or(...)` reads the
    // promoted piece's own type when `mv` promotes -- correct for free, even though today's one
    // caller (the move-count cap's exemption gate) already excludes promotions before this is
    // ever reached.
    let moving_piece_type =
        mv.promotion().unwrap_or(board.piece_at(mv.from()).expect("gives_check: no piece on the from square").piece_type());
    let direct_attacks = match moving_piece_type {
        PieceType::Pawn => pawn_attacks(mv.to(), color),
        PieceType::Knight => knight_attacks(mv.to()),
        PieceType::Bishop => bishop_attacks(mv.to(), occupancy),
        PieceType::Rook => rook_attacks(mv.to(), occupancy),
        PieceType::Queen => bishop_attacks(mv.to(), occupancy) | rook_attacks(mv.to(), occupancy),
        // A king can never legally deliver check this way: landing adjacent to the enemy king
        // is exactly the condition that makes a king move illegal in the first place (symmetric
        // attack -- see `is_square_attacked`'s own king_attacks check), so this case is already
        // filtered out by legality before `gives_check` could ever be asked about it.
        PieceType::King => Bitboard::EMPTY,
    };
    if (direct_attacks & Bitboard::from_square(enemy_king)) != Bitboard::EMPTY {
        return true;
    }

    // Discovered check: a friendly bishop/rook/queen that only reaches the enemy king now that
    // `mv.from()` reads as empty in `occupancy`. `attackers_to`, anchored at the enemy king with
    // this same post-move occupancy, finds exactly this: the mover's own former square no longer
    // stops a ray passing through it, so any line that was blocked only by `mv`'s own piece now
    // reaches whatever real slider actually sits behind it. Never finds the mover itself this
    // way (its real board square, `from`, is exactly what's now marked passable, so a ray
    // through `from` never stops -- and hence never "attacks" -- there); restricted to `color`'s
    // own sliders, since `attackers_to` returns both colors combined and an opponent piece being
    // revealed here isn't a check on their own king.
    let sliders = board.bitboard(Piece::new(color, PieceType::Bishop))
        | board.bitboard(Piece::new(color, PieceType::Rook))
        | board.bitboard(Piece::new(color, PieceType::Queen));
    (attackers_to(board, enemy_king, occupancy) & sliders) != Bitboard::EMPTY
}

/// Whether `board` is already an automatic draw under the fifty-move rule. Checked only once
/// a node is already known not to be checkmate/stalemate (never as an early bypass ahead of
/// move generation the way `is_repetition` is): checkmate has to win if the two ever coincide
/// -- a mate delivered on the very move that also happens to reach the fiftieth move is still
/// a decisive result, not a draw -- and unlike a runaway repetition chain, there's no
/// unbounded-recursion risk here to justify checking any earlier than that.
fn is_fifty_move_draw(board: &Board) -> bool {
    board.halfmove_clock() >= FIFTY_MOVE_HALFMOVES
}

/// Whether `board`'s material is provably insufficient for *either* side to ever force
/// checkmate, no matter how the position plays out from here on -- the classical,
/// FIDE-recognized subset of "dead position" (Article 5.2.2: drawn once neither side can
/// checkmate the other "with any series of legal moves") that's decidable from piece counts
/// alone, unlike the fully general rule (which can in principle cover a completely frozen
/// pawn structure too, but isn't practically decidable):
/// - King vs King
/// - King and Knight vs King (either side)
/// - King and Bishop vs King (either side)
/// - King and Bishop vs King and Bishop, only when both bishops sit on the same color square
///
/// King and two Knights vs King is deliberately *not* included, despite being unforceable
/// against correct defense: checkmate there isn't actually impossible, only impossible to
/// force -- it needs the defending side's own cooperation, a real distinction from the four
/// cases above, where no sequence of legal moves, played by either side, could ever produce
/// checkmate.
///
/// Checked unconditionally, ahead of move generation, the same way `is_repetition` is (and
/// unlike `is_fifty_move_draw`): unlike the fifty-move rule, there's no checkmate-priority
/// conflict to guard against here -- a position actually matching one of the four patterns
/// above is *provably* incapable of ever being checkmate, that's precisely what "insufficient
/// mating material" means, so nothing this could ever wrongly override.
///
/// Lives here, not in `eval.rs`'s `evaluate`, for the same reason checkmate/stalemate
/// detection already does: this is a draw-detection fact belonging to search, not a material-
/// counting one -- and checking it only inside `evaluate` (called solely at leaves) would
/// still be correct (every position reachable from an already-insufficient one is also
/// insufficient, since captures only ever remove material and there are no pawns left to
/// promote new material) but would waste a full subtree's worth of search effort learning
/// what's already knowable at this node, exactly the inefficiency checking `is_repetition`
/// early already exists to avoid.
pub fn is_insufficient_material(board: &Board) -> bool {
    // A pawn, rook, or queen for either side rules this out immediately, regardless of what
    // else is on the board -- a pawn can promote into new material, and a lone rook or queen
    // can always force mate on its own.
    for piece_type in [PieceType::Pawn, PieceType::Rook, PieceType::Queen] {
        let white_count = board.bitboard(Piece::new(Color::White, piece_type)).count();
        let black_count = board.bitboard(Piece::new(Color::Black, piece_type)).count();
        if white_count > 0 || black_count > 0 {
            return false;
        }
    }

    let white_bishops = board.bitboard(Piece::new(Color::White, PieceType::Bishop));
    let black_bishops = board.bitboard(Piece::new(Color::Black, PieceType::Bishop));
    let white_minors =
        board.bitboard(Piece::new(Color::White, PieceType::Knight)).count() + white_bishops.count();
    let black_minors =
        board.bitboard(Piece::new(Color::Black, PieceType::Knight)).count() + black_bishops.count();

    if white_minors == 0 && black_minors == 0 {
        return true; // King vs King.
    }
    if white_minors + black_minors == 1 {
        // A single knight or bishop, either side, against a bare king -- which one it is,
        // and which side has it, doesn't matter: either way it's alone against a lone king.
        return true;
    }
    if white_bishops.count() == 1 && black_bishops.count() == 1 && white_minors == 1 && black_minors == 1 {
        // Exactly one bishop each, nothing else -- insufficient only if they share a color.
        return same_square_color(white_bishops, black_bishops);
    }
    false
}

/// Whether `white_bishop`/`black_bishop` -- each expected to have exactly one bit set --
/// sit on the same color of square. `Square::is_light_square` carries the actual parity
/// reasoning; this is just naming its result for the one bishop that happens to be here.
fn same_square_color(white_bishop: Bitboard, black_bishop: Bitboard) -> bool {
    let white_square = white_bishop.squares().next().unwrap();
    let black_square = black_bishop.squares().next().unwrap();
    white_square.is_light_square() == black_square.is_light_square()
}

/// The piece `mv` captures, or `None` for a quiet move. An ordinary move is a capture
/// exactly when its destination square isn't empty, read straight off the mailbox via
/// `piece_at`. En passant needs its own rule instead: the captured pawn sits on the square
/// passed over, not `to` itself -- but it's always a pawn belonging to whoever isn't
/// moving, by the rule's own definition, so no board lookup is even needed for that case.
fn captured_piece(board: &Board, mv: Move) -> Option<Piece> {
    if mv.kind() == MoveKind::EnPassantCapture {
        return Some(Piece::new(board.side_to_move().opposite(), PieceType::Pawn));
    }
    board.piece_at(mv.to())
}

/// Classifies a fail-soft alpha-beta result against the window it was searched with, for
/// storing in the transposition table -- the standard alpha-beta-with-TT convention: a value
/// the search never even raised `alpha` past (`<= original_alpha`, "fail low") only proves an
/// upper bound on the true value; a value that caused a beta cutoff (`>= beta`, "fail high")
/// only proves a lower bound; anything strictly between the two is the exact value. Takes
/// `original_alpha` -- the window's lower edge *before* the caller's own loop started
/// narrowing it -- rather than reading `alpha` itself, since by the time a caller has a
/// `score` to classify, its own `alpha` has usually already moved past its original value.
fn classify_bound(score: i32, original_alpha: i32, beta: i32) -> Bound {
    if score <= original_alpha {
        return Bound::Upper;
    }
    if score >= beta {
        return Bound::Lower;
    }
    Bound::Exact
}

/// `Search::continuation_history`'s own flattened length: `[prev_piece][prev_to][piece][to]`,
/// `12 * 64 * 12 * 64`. See `continuation-history.md` for why this is one flat `Vec<i32>` rather
/// than a nested `[[[[i32; 64]; 12]; 64]; 12]` array embedded directly in `Search` -- at ~2.25 MB,
/// a nested array would be copied by value every iterative-deepening iteration the same way
/// `history` already is threaded through `search_root`/`choose_best_move_with_progress`, fine for
/// `history`'s own 3 KB but a real, avoidable cost at 750x that size. A `Vec` moves (a pointer/
/// length/capacity triple), never copying the backing buffer, so this costs the same to thread
/// through that fold as moving `history` already does.
const CONTINUATION_HISTORY_LEN: usize = 12 * 64 * 12 * 64;

/// Flattened index into `Search::continuation_history` for one `(prev_piece, prev_to, piece, to)`
/// combination -- the same `[piece as usize][to as usize]` indexing convention `history` already
/// uses, just nested one level deeper for the move played `1` ply before `mv`.
fn continuation_history_index(prev_piece: Piece, prev_to: Square, piece: Piece, to: Square) -> usize {
    ((prev_piece as usize * 64 + prev_to as usize) * 12 + piece as usize) * 64 + to as usize
}

/// History gravity's ceiling for the flat table (`Search::history`), the largest magnitude a bucket
/// reaches: 8192, Stockfish's butterfly-history cap (7183) rounded to a power of two. Gravity
/// pulls each bucket back as it nears the cap, so old cutoffs fade instead of piling up.
const FLAT_HISTORY_MAX: i32 = 8192;

/// `Search::continuation_history`'s own ceiling -- `4x FLAT_HISTORY_MAX`, the same rough ratio
/// Stockfish's own two real caps sit in (`30000 / 7183 ~= 4.2`), for the sparsity reason
/// `FLAT_HISTORY_MAX`'s own doc comment gives.
const CONTINUATION_HISTORY_MAX: i32 = FLAT_HISTORY_MAX * 4;

/// Applies one history bonus (or malus, via a negative `bonus`) to `entry` with gravity: the
/// update shrinks as `entry` approaches `+-max`, so repeated bonuses self-limit instead of growing
/// without bound, and a malus pulls a saturated entry back down just as effectively as a fresh one
/// -- matching Stockfish's own real update formula (fetched, not assumed). `bonus` is clamped to
/// `+-max` first so one outsized event can't overshoot `max` in a single step (the flat-history
/// bonus alone already can, at `MAX_PLY`: `100 * 100 = 10000 > FLAT_HISTORY_MAX`); the `i64`
/// intermediate avoids any risk of `entry * bonus` overflowing `i32` before the division shrinks it
/// back down.
fn apply_history_bonus(entry: &mut i32, bonus: i32, max: i32) {
    let bonus = bonus.clamp(-max, max);
    let decay = (*entry as i64 * bonus.abs() as i64 / max as i64) as i32;
    *entry += bonus - decay;
}

/// The state threaded through every recursive call of a single search, bundled into one
/// struct instead of passing `moves`/`nodes`/`killers` and the rest as separate parameters at
/// every call site (including the recursive one). None of them are the actual substance of what
/// alpha-beta recurses over the way `board`/`alpha`/`beta`/`depth` are -- they don't change
/// *identity* across the whole recursive tree (only their contents mutate in place). Those four
/// stay explicit method arguments;
/// everything here becomes `self`, reached via `&mut self` instead of four-plus reference
/// parameters. This also means quiescence (once it exists) can share this exact bundle -- a
/// second method on the same struct, rather than a second function repeating the same
/// parameter list.
struct Search<'a> {
    /// Shared buffer for pseudo-legal moves at every node in the recursion, reused and
    /// truncated rather than freshly allocated per node -- unchanged in behavior from when
    /// this was a `&mut Vec<Move>` parameter.
    moves: Vec<Move>,
    /// How many times `alpha_beta_with_buffer` has actually been called this search -- one
    /// increment per call, the same convention `perft` already uses, so a caller can compare
    /// it against `negamax`'s own count on the same position and confirm pruning is actually
    /// happening, not just assumed from the algorithm being textbook.
    nodes: u64,
    /// The deepest `ply` actually visited anywhere in this search so far -- UCI's `seldepth`
    /// ("selective search depth"), distinct from `target_depth` ("depth") itself. Updated via
    /// `self.seldepth = self.seldepth.max(ply)` at the top of both `alpha_beta_with_buffer` and
    /// `quiescence`, alongside the existing node-counting -- needs no special initial value,
    /// since even a search that never enters `quiescence` at all still reaches `target_depth`
    /// on its own (every leaf calls `alpha_beta_with_buffer` once at `ply == target_depth`
    /// before handing off). Always `>= target_depth`, since `quiescence` only ever extends
    /// `ply` further, never stops short of it.
    seldepth: u32,
    /// Two stored quiet moves per `ply`, tried right after captures/promotions are exhausted
    /// and before any other quiet move, on the theory that whatever quiet move caused a cutoff
    /// at a sibling node is likely to do so again here. Indexed by `ply` itself, not
    /// `target_depth - ply` ("remaining depth-to-go") the way this table used to be: `ply` is
    /// the same absolute distance from the root regardless of `target_depth`, so a killer
    /// recorded here means the same thing -- applies to the same actual position -- at every
    /// iterative-deepening iteration, where `remaining`-indexing did not (the same slot number
    /// pointed at a different `ply` in every iteration, since `target_depth` itself changes).
    /// That's exactly what makes carrying this table across iterations safe now: sized by
    /// `MAX_PLY`, not `target_depth + 1`, and owned and threaded through by `choose_best_move`
    /// (via `search_root`/`new_search`) -- moved in on construction, handed back out after the
    /// call (see `search_root`'s own doc comment), rather than rebuilt fresh here every time.
    ///
    /// Two slots, not one, to avoid thrashing: if two different quiet moves alternate as the
    /// cutoff cause across nearby siblings, a single slot would have each overwrite the other,
    /// losing whichever one isn't the *most* recent every time. Slot `0` is the most recently
    /// recorded cutoff at this `ply`, slot `1` the one before it -- classic MRU replacement
    /// (see the recording site for the exact shift rule).
    killers: Vec<[Option<Move>; 2]>,
    /// One running score per `(piece, destination square)` -- `[piece as usize][to as usize]`,
    /// the same indexing shape `board::zobrist::PIECE_SQUARE` uses. Not indexed by node or by
    /// `ply` the way `killers` is: the whole point is generalizing across the *entire* tree, so
    /// "how good has a knight-to-f5-shaped move been anywhere in this search" is one shared
    /// number, not one per sibling group. Bumped by `(target_depth - ply)^2` (Schaeffer's
    /// original 1989 formula -- a published convention, not invented here) at the same
    /// recording site `killers` already uses, whenever a quiet move causes a cutoff. Read by the
    /// move picker, through `combined_history_score`, to order the quiets.
    ///
    /// Persisted across iterative-deepening iterations exactly like `killers` (threaded through
    /// by `choose_best_move`/`search_root`/`new_search`, not rebuilt fresh here), for an even
    /// stronger reason: unlike `killers`, this table was never indexed by anything
    /// ply-dependent to begin with, so there's no cross-iteration meaning-mismatch to guard
    /// against at all -- a `[piece][to]` bucket means the same thing regardless of `ply` or
    /// `target_depth`.
    history: [[i32; 64]; 12],
    /// Continuation history (1-ply / countermove history): `history`'s own idea, refined by one
    /// extra dimension of context -- indexed by both the current move and whichever move was
    /// played `1` ply before it (`continuation_history_index`'s own doc comment has the exact
    /// flattened layout). See `continuation-history.md` for the full design; in short, every
    /// event that already writes a bonus/malus into `history` also writes the identical amount
    /// into this table at `[prev_piece][prev_to][piece][to]`, gated on a previous move actually
    /// existing to condition on (`self.prev_move[ply].is_some()` -- always true except at the
    /// root or right after a null move). Read by the move picker, summed with `history`'s
    /// own score, plain and unweighted (matching Stockfish's own real combination). Persisted
    /// across iterative-deepening iterations exactly like `history` (threaded through by
    /// `choose_best_move`/`search_root`/`new_search`), for the identical reason: never indexed by
    /// anything ply-dependent, so a bucket means the same thing regardless of `ply`/`target_depth`.
    /// A flat `Vec<i32>`, not a nested array -- see `CONTINUATION_HISTORY_LEN`'s own doc comment
    /// for why.
    continuation_history: Vec<i32>,
    /// What was played to reach ply `p`, stored at `prev_move[p]` -- `None` at the root (`p ==
    /// 0`) or right after a null move, where there's no real previous move to condition
    /// continuation history on. Set unconditionally, once per move actually searched (not once
    /// per recursive call variant -- PVS/LMR/re-search all recurse into the same child for the
    /// same move), right before whichever of those recursive calls the move loop takes. Unlike
    /// `pv`/`static_evals`, this needs no explicit per-node clearing on entry: every slot is
    /// always overwritten by its own parent immediately before the child it belongs to is ever
    /// called, so there's no window where a stale value from an unrelated branch could be read.
    /// Fresh per `Search` (like `pv`/`static_evals`), not threaded across iterative-deepening
    /// iterations -- a ply-to-position mapping only means anything within one fixed-depth
    /// search's own traversal, the same reasoning `static_evals`'s own doc comment already gives.
    prev_move: Vec<Option<(Piece, Square)>>,
    /// One static evaluation per `ply`, `None` meaning "no eval available here to compare
    /// against" -- either nothing has visited this `ply` yet this search, or whatever did was
    /// in check (`evaluate` is never called there). Read by `is_improving`, comparing the
    /// current node's own eval against whichever node last occupied `ply - 2` -- the same side
    /// to move, since plies alternate color (`search.md`'s design section has the real
    /// Stockfish source this mirrors and the reasoning behind the `None` shape).
    ///
    /// Unlike `killers`/`history`, this does *not* persist across iterative-deepening
    /// iterations: a two-plies-back lookup only means anything within one fixed-depth search's
    /// own ply-to-position mapping, which is unrelated from one iteration to the next -- fresh
    /// per `Search`, the same lifetime as `nodes`/`pv`.
    ///
    /// Written unconditionally to `None` at the very top of `alpha_beta_with_buffer`, before
    /// anything else, for the identical reason `pv[ply]` already is: without that, a node that
    /// skips writing a real value here (in check) would otherwise leave behind whatever an
    /// unrelated, already-backtracked sibling last wrote at this same `ply`, and a descendant
    /// two plies on reading it would splice in contaminated data from a completely different
    /// branch -- the same class of bug this file's own manual UCI testing already caught once
    /// for `pv`.
    static_evals: Vec<Option<i32>>,
    /// One row per `ply`, `0..=target_depth` -- row `ply` holds the best continuation found
    /// so far starting from a node at that distance from the root, main search only (never
    /// written by `quiescence`: its own choice among captures isn't a genuine best-move
    /// decision the way full-width search is, so it isn't reported as part of the PV). Same
    /// shared-state pattern `killers` already uses (indexed, mutated in place as the
    /// recursion proceeds) rather than threaded through return values -- updated at the exact
    /// same moment `best_score` itself improves: `self.pv[ply] = [mv] ++ self.pv[ply + 1]`.
    /// Row `0` (the root's own line) is what `search_root` reads back out -- its first move is
    /// what `best_root_move` used to be, a field retired now that this exists (exactly the
    /// retirement its own doc comment anticipated: "this field's whole job becomes 'the first
    /// move of the root's own PV' ... this field can likely be retired in favor of that").
    ///
    /// A row is genuinely shared across the whole tree, not owned by one node: countless
    /// unrelated calls at the same `ply` (different branches entirely, visited at different
    /// points in the traversal) each read and write this same index over the search's lifetime.
    /// `alpha_beta_with_buffer` clears its own row unconditionally on entry (see its own top-
    /// of-function comment) specifically so this sharing is safe -- without that, a node that
    /// returns early without ever reaching the improvement write below (a repetition, a TT
    /// cutoff, insufficient material, a fifty-move draw) would leave whatever an entirely
    /// unrelated earlier node last wrote at that same row sitting there, and a parent that later
    /// reads it (because *this* early-returning node's own score happened to improve *its*
    /// parent's best score) would silently splice a stale, unrelated continuation onto its own
    /// line -- exactly the class of bug this project's own manual UCI testing once caught (an
    /// impossible move appearing in a real reported PV, traced back to precisely this).
    ///
    /// Stored as one triangular table (`PvTable`), not a `Vec` per row: the update copies in place
    /// and allocates nothing.
    pv: PvTable,
    /// The absolute point in time this search must stop by, if any. `None` means either no
    /// time limit exists for this `go` at all, or -- once more than one worker thread exists --
    /// that this `Search` belongs to a thread other than thread `0`. Only thread `0` ever
    /// computes a real deadline from the actual time budget; every other thread relies entirely
    /// on `stop_signal` instead (see that field's own doc comment for why). Checked only every
    /// `CHECK_INTERVAL` nodes, not on every call -- see `CHECK_INTERVAL`'s own doc comment for
    /// why, and `stopped` below for the other half of this mechanism.
    deadline: Option<Instant>,
    /// Set once a throttled check finds either this search's own `deadline` has passed (thread
    /// `0` only) or `stop_signal` has been set by someone else, and checked unconditionally at
    /// the very top of both `alpha_beta_with_buffer` and `quiescence`, before either even
    /// increments `nodes`. This is what makes the abort propagate on the very next call anywhere
    /// in the tree once it trips, rather than only at the next `CHECK_INTERVAL`-node checkpoint
    /// -- without it, every node in between a tripped deadline and the next throttled check
    /// would still do real work, recursing further, for up to `CHECK_INTERVAL - 1` more calls.
    /// Once `true`, stays `true` for the rest of this `Search`'s life -- there's no reason to
    /// ever clear it, since a `Search` is fresh per iterative-deepening iteration and never
    /// reused.
    stopped: bool,
    /// Whether the immediately preceding recursive call, one ply up, made a null move to get
    /// here -- read by null-move pruning's own gate to refuse a second null move stacked
    /// directly on the first, which would mean neither side actually moved and would tell this
    /// position nothing real about itself. Saved and restored around the recursive call the
    /// same way `self.ancestors`'s own push/pop already brackets every other recursive call in
    /// `alpha_beta_with_buffer` -- a single flag, not a `Vec` indexed by `ply` the way
    /// `static_evals`/`pv` are, because save-restore-around-the-call is exactly what "only the
    /// direct parent matters" needs, with no requirement to remember anything about ancestors
    /// further back than that.
    null_move_in_progress: bool,
    /// One shared flag, scoped to a single `go` (created fresh per `choose_best_move` call, not
    /// the process-lifetime kind a real thread pool's own stop flag would be), that every
    /// worker thread's `Search` holds a reference to. Thread `0` is the only one that ever
    /// writes `true` to it -- exactly when its own `deadline` check (above) decides time is up,
    /// whether between iterations or mid-search -- mirroring the same real-engine model checked
    /// directly against Stockfish's own `thread.cpp`/`search.cpp`: one designated thread reads
    /// the clock and decides, every thread (that one included) just polls a shared flag and
    /// bails the moment it sees it set. Every other thread never computes a deadline of its own
    /// at all; its own periodic check reads this flag instead. With only one thread running
    /// today, this is still always present (never `None`) but never actually observed as `true`
    /// from anywhere but the same thread that set it.
    stop_signal: &'a AtomicBool,
    /// A ponder search's state, thread 0 only: while pondering, `deadline` is not enforced. `None`
    /// for every other search.
    ponder: Option<&'a PonderState>,
    /// Every ancestor of whichever position the current recursive call is actually looking
    /// at -- never that position's own hash, only what came before it. Seeded once, at
    /// construction, with the real game's own move history (from UCI's `position ... moves
    /// ...`, via `choose_best_move`); extended during recursion by pushing the *current*
    /// call's own `board.hash()` right before recursing into a child (making it that child's
    /// own newest ancestor), popped again once that recursive call returns -- so at any point,
    /// this holds exactly the sequence of positions that led to wherever the search is right
    /// now, whether that lineage came from the real game or from moves this very search made.
    /// Checked by `is_repetition`, from both `alpha_beta_with_buffer` and `quiescence`. Named
    /// `ancestors`, not `history` -- that plain name is reserved for the move-ordering history
    /// heuristic table, the canonical chess-programming term for it.
    ancestors: Vec<u64>,
    /// A `&` reference, not an owned value like every other field here: unlike
    /// `killers`/`history`/`ancestors` (each fresh-built or cloned per call site and handed in
    /// by value), the transposition table persists across the *whole game*, well beyond any
    /// single `Search`'s own lifetime -- `Engine` is what actually owns it. Shared, not `&mut`,
    /// specifically because multiple worker threads (each running their own `Search`) need to
    /// hold and use the same table at once -- `TranspositionTable::probe`/`store` both already
    /// work through shared access for exactly this reason. Shared by both
    /// `alpha_beta_with_buffer` and `quiescence`, the same way `nodes`/`ancestors` already are.
    tt: &'a TranspositionTable,
    /// Loaded Syzygy WDL tables to probe in-tree, or `None` when none are loaded (`Engine`'s own
    /// default, and every existing test's default via `new_search` below) -- in which case
    /// `alpha_beta_with_buffer`'s own probe attempt is skipped outright, identical to today's
    /// behavior before this field existed. A shared reference, not owned, for the same reason
    /// `tt` is: multiple worker threads probe the same loaded tables concurrently, and
    /// `SyzygyTablebase::probe` only ever needs `&self` to do it.
    tablebase: Option<&'a SyzygyTablebase>,
    /// A trained network to evaluate leaves with, or `None` for the PeSTO evaluation --
    /// which is what every existing caller and every test still gets, so this field's arrival
    /// changes nothing until something passes a network.
    ///
    /// On `Search` rather than in a process-global, and the reason is the second gate rather than
    /// the first: one generation playing the previous one means two different networks alive in one
    /// process at the same time, which no global can express. A shared reference for the same reason
    /// `tt` and `tablebase` are -- every Lazy SMP worker thread evaluates against the same loaded
    /// weights, and evaluating only ever needs `&self`, so there is nothing to synchronize.
    evaluator: Option<&'a Network>,
    /// psq's quantized accumulators, one pair per ply (indexed by color, see
    /// `QuantizedPsqNetwork::update`):
    /// `accumulators[ply]` always describes the board being searched at `ply`. Entry 0 is computed
    /// from scratch at the root, and every child's is updated from its parent's right after its
    /// move is found legal, so evaluating a node costs only the head. Empty unless the evaluator
    /// is a psq network. One set per `Search`, so each Lazy SMP thread has its own.
    accumulators: Vec<[Vec<i16>; 2]>,
    /// The evaluator's pawn against `piece_value`'s, in percent: 100 for PeSTO, a network's
    /// as measured when it loaded (`nnue::measure_label_percent`). Set with the evaluator.
    pawn_percent: i32,
    /// What this search prunes, orders and windows by: `SearchParameters::DEFAULT` unless the
    /// caller gave others (`set_parameters`).
    parameters: SearchParameters,
    /// `parameters`' margins compared with a static evaluation, in the evaluator's pawn.
    eval_margins: EvalMargins,
    /// MultiPV's own root move exclusion: a repeated root search, at the same
    /// depth, treats every move in here as if it didn't exist at all -- letting the *next*-best
    /// root move win instead, with its own real, exact score (a genuine full-window search, not a
    /// scout-search bound) rather than just "worse than whichever move this excludes." Empty by
    /// default (`new_search`), meaning `MultiPV == 1`'s exact existing behavior is unaffected --
    /// only `search_root` ever sets this to something non-empty, once per additional rank beyond
    /// the first. Checked only at `ply == 0` (`alpha_beta_with_buffer`'s own root move loop): a
    /// move at any deeper ply is a different move in a different position, and coincidentally
    /// sharing a from/to pair with an excluded root move means nothing there.
    excluded_root_moves: Vec<Move>,
    /// The picker's move scores, index for index with `moves`: a node's entries sit at the same
    /// indices as its moves, and a child only writes past them. Grown as needed, never shrunk.
    scores: Vec<i32>,
    /// Every quiet move fully searched so far at each node on the current line, a stack shared the
    /// way `moves` is: a node's own quiets sit past its parent's, from the length it found on entry,
    /// and it truncates back to that length before returning. Its capacity grows once and is
    /// reused, where a `Vec` per node allocated at every node that searched a quiet move.
    quiets: Vec<Move>,
    /// The correction history (`corr`): per key and side to move, a running average of the
    /// search's error against the static evaluation, in the evaluator's units. Indexed by
    /// `correction_index`; carried from one iteration to the next as the histories are.
    correction_history: Vec<i32>,
}

/// Entries per correction table and side to move: tiny beside the TT, and a pawn structure or a
/// set of pieces repeats often enough within one search for an entry to learn (search-vs-stockfish's
/// defaults, the developer, 2026-10-31).
const CORRECTION_ENTRIES: usize = 16_384;

/// The correction history's length: three keys (`Board::pawn_key`, and `Board::non_pawn_key` for
/// each colour), each with a table per side to move.
const CORRECTION_HISTORY_LEN: usize = 3 * 2 * CORRECTION_ENTRIES;

/// The sample depth past which a sample counts no more: at `min(depth, CORRECTION_DEPTH_CAP)` of
/// `CORRECTION_WEIGHT_DIVISOR`, a sample from depth 8 or deeper moves an entry halfway to itself, as
/// much as everything learnt before, one from depth 1 a sixteenth. A deeper search is a better
/// witness; 8 is a round cap, and a depth 9 search's nodes rarely sit deeper than that below them.
const CORRECTION_DEPTH_CAP: u32 = 8;
const CORRECTION_WEIGHT_DIVISOR: i32 = 16;

/// Moves per row of `PvTable`, and its number of rows. The row at `MAX_PLY` is only ever cleared,
/// never written, and a row holds at most one move more than the row after it, so the row at
/// `ply` never holds more than `MAX_PLY - ply` moves.
const PV_ROW: usize = MAX_PLY as usize + 1;

/// `Search::pv`'s storage: a triangular principal-variation table, every row in one allocation
/// made once per `Search`. Rows were once a `Vec` each, and every improvement allocated a fresh one
/// and cloned the child's row into it, at least once per node that searched a move. With
/// self-play's 16 threads in one process, those allocations contended on Windows' shared process
/// heap: removing them was measured.
struct PvTable {
    /// Row `ply` is `moves[ply * PV_ROW..ply * PV_ROW + lengths[ply]]`.
    moves: Vec<Move>,
    lengths: Vec<usize>,
}

impl PvTable {
    fn new() -> PvTable {
        // Any move fills the slots: only `lengths` says what a row holds.
        let filler = Move::new(Square::A1, Square::A1, None);
        PvTable { moves: vec![filler; PV_ROW * PV_ROW], lengths: vec![0; PV_ROW] }
    }

    /// The best line found so far from the node at `ply`.
    fn line(&self, ply: u32) -> &[Move] {
        let start = ply as usize * PV_ROW;
        &self.moves[start..start + self.lengths[ply as usize]]
    }

    fn clear(&mut self, ply: u32) {
        self.lengths[ply as usize] = 0;
    }

    /// Row `ply` becomes `mv` followed by row `ply + 1`, copied in place.
    fn update(&mut self, ply: u32, mv: Move) {
        let row = ply as usize * PV_ROW;
        let child_length = self.lengths[ply as usize + 1];
        self.moves[row] = mv;
        self.moves.copy_within(row + PV_ROW..row + PV_ROW + child_length, row + 1);
        self.lengths[ply as usize] = child_length + 1;
    }

    /// Writes `line` into row `ply` directly, for tests that plant a stale row.
    #[cfg(test)]
    fn set(&mut self, ply: u32, line: &[Move]) {
        let row = ply as usize * PV_ROW;
        self.moves[row..row + line.len()].copy_from_slice(line);
        self.lengths[ply as usize] = line.len();
    }
}

/// A fresh `Search` for a single top-level call -- the one piece of construction `alpha_beta`
/// and `search_root` both need identically, extracted so a fresh call site of either one doesn't
/// have to restate every field. `deadline` is `Search`'s own hard, in-tree ceiling for this one
/// call -- `None` means no time limit applies at all. `ancestors` seeds `Search::ancestors` with
/// the real game's own ancestor positions -- empty when there's no real game behind this call
/// (`alpha_beta`'s own tests) or none played yet. `killers` and `history` are both moved straight
/// into their matching `Search` fields unchanged -- unlike `ancestors` (cloned fresh per depth so
/// each iteration's own push/pop stays independent), both are meant to keep accumulating across
/// the whole `go`, so the same tables are threaded in and handed back out rather than rebuilt here
/// (see their own doc comments on `Search`). `tt` and `stop_signal` are both shared borrows, not
/// moved -- see their own doc comments on `Search` for why they're the two fields here that are
/// references rather than owned values.
///
/// `depth` itself is accepted but no longer used to size `static_evals`/`pv` -- both are now
/// always sized to `MAX_PLY + 1` (the same absolute ceiling `killers` is already sized to,
/// matching `MAX_PLY` itself), a fix made necessary by search extensions: `self.pv[ply]`/
/// `self.static_evals[ply]` are both indexed unconditionally near the very top of
/// `alpha_beta_with_buffer`, *before* the `ply >= MAX_PLY` check further down ever runs, so the
/// largest `ply` either one is ever indexed at is `MAX_PLY` itself (the call that finally trips
/// that check), regardless of what nominal `depth` this iteration actually requested. Before
/// extensions existed, `ply` could never exceed the nominal `depth` it was given (every recursive
/// call cost exactly one ply, always), so sizing to `depth + 1` happened to be safe in practice --
/// extensions break that invariant outright, since a checking move can now push `ply` arbitrarily
/// far past the nominal `depth` along its own line, with no cap. `depth`'s own parameter is kept
/// (not removed) since every real call site still has a natural depth to pass, and changing this
/// function's signature for what's now purely an unused-in-body reason isn't worth the churn at
/// every call site, including every test's own.
#[allow(clippy::too_many_arguments)]
fn new_search<'a>(
    depth: u32, deadline: Option<Instant>, ancestors: Vec<u64>, killers: Vec<[Option<Move>; 2]>,
    history: [[i32; 64]; 12], continuation_history: Vec<i32>, tt: &'a TranspositionTable,
    stop_signal: &'a AtomicBool,
) -> Search<'a> {
    let _ = depth;
    Search {
        moves: Vec::new(),
        nodes: 0,
        seldepth: 0,
        killers,
        history,
        continuation_history,
        prev_move: vec![None; MAX_PLY as usize + 1],
        static_evals: vec![None; MAX_PLY as usize + 1],
        pv: PvTable::new(),
        deadline,
        stopped: false,
        null_move_in_progress: false,
        // Set by `search_root_pondering`, as `tablebase` below is.
        ponder: None,
        ancestors,
        tt,
        // Always None here -- no tablebase parameter added to this constructor itself, since
        // that would force every one of this function's many direct test call sites to supply
        // one. search_root sets this field directly on the Search this returns, once threading
        // a real tablebase through search_root/choose_best_move actually happens.
        tablebase: None,
        // Defaulted here for the same reason `tablebase` just above is: adding a parameter would
        // force every one of this constructor's many direct test call sites to supply one, and
        // `None` is the inert value -- the PeSTO evaluation, exactly today's behavior.
        // `search_root` sets it directly on the `Search` this returns.
        evaluator: None,
        accumulators: Vec::new(),
        pawn_percent: 100,
        parameters: SearchParameters::DEFAULT,
        eval_margins: eval_margins(&SearchParameters::DEFAULT, 100),
        stop_signal,
        excluded_root_moves: Vec::new(),
        scores: Vec::new(),
        quiets: Vec::new(),
        // Fresh, as `new_search`'s other defaults; `search_root_pondering` hands in the one the
        // iterations carry.
        correction_history: vec![0; CORRECTION_HISTORY_LEN],
    }
}

/// `parameters`' margins compared with a static evaluation, in an evaluator's pawn of
/// `pawn_percent` of `piece_value`'s.
fn eval_margins(parameters: &SearchParameters, pawn_percent: i32) -> EvalMargins {
    let scaled = |centipawns: i32| -> i32 { centipawns * pawn_percent / 100 };
    EvalMargins {
        razoring: scaled(parameters.razoring_margin),
        reverse_futility: scaled(parameters.reverse_futility_margin),
        futility: scaled(parameters.futility_margin),
        null_move: scaled(parameters.null_move_margin),
        pawn: scaled(piece_value(PieceType::Pawn)),
    }
}

impl<'a> Search<'a> {
    /// Whether `deadline` has passed, read at `CHECK_INTERVAL`. A ponder search ignores its deadline
    /// while pondering, and after `ponderhit` stops at once if the time ran out while it pondered
    /// (Stockfish's `check_time`: `if (ponder) return;`, then `stopOnPonderhit` stops).
    fn is_out_of_time(&self) -> bool {
        let Some(deadline) = self.deadline else {
            return false;
        };
        if let Some(ponder) = self.ponder {
            if ponder.is_pondering() {
                return false;
            }
            if ponder.stops_now() {
                return true;
            }
        }
        Instant::now() >= deadline
    }

    /// Whether `color` has a legal move on `board`, stopping at the first one found: the leaf's
    /// mate and stalemate test. Generates into `self.moves` past whatever the callers above hold,
    /// and truncates back, so it allocates nothing.
    fn has_legal_move(&mut self, board: &Board, color: Color) -> bool {
        let start = self.moves.len();
        pseudo_legal_moves(board, color, &mut self.moves);
        let legality = Legality::new(board, color);
        let found = self.moves[start..].iter().any(|&mv| legality.is_legal(board, mv));
        self.moves.truncate(start);
        found
    }

    /// This search's own leaf evaluation, in negamax's convention: positive means the side to move
    /// is ahead.
    ///
    /// The two paths agree on that convention without any conversion, which is why this is a plain
    /// dispatch rather than a place where signs get fixed up. `eval::evaluate` negates a
    /// White-relative balance for Black; a trained network's output is *already* side-to-move
    /// relative, because every feature it reads is oriented for the mover and every piece identity
    /// is own/opponent rather than White/Black.
    ///
    /// `ply` is where `board` sits in this search: a psq network reads `accumulators[ply]` instead
    /// of recomputing them, so the caller must pass the ply `board` was reached at.
    fn evaluate(&self, board: &Board, ply: u32) -> i32 {
        #[cfg(feature = "node-stats")]
        crate::node_stats::evaluated(
            ply as usize,
            self.tt.probe(board.hash()).is_some(),
            matches!(self.evaluator, Some(Network::Psq(..))) && (ply as usize) < self.accumulators.len(),
        );
        match self.evaluator {
            Some(Network::Psq(_, network)) if (ply as usize) < self.accumulators.len() => {
                network.evaluate_by_color_cp(&self.accumulators[ply as usize], board)
            }
            Some(network) => network.evaluate(board),
            None => evaluate(board),
        }
    }

    /// `board`'s entries in the correction history, one per key: the pawns, White's other
    /// pieces, Black's, each in its side to move's table.
    fn correction_indices(board: &Board) -> [usize; 3] {
        let side = board.side_to_move() as usize;
        let keys = [board.pawn_key(), board.non_pawn_key(Color::White), board.non_pawn_key(Color::Black)];
        std::array::from_fn(|table| (table * 2 + side) * CORRECTION_ENTRIES + (keys[table] % CORRECTION_ENTRIES as u64) as usize)
    }

    /// What the correction history adds to `board`'s static evaluation: the plain average of its
    /// three entries (no evidence for weighting the keys), clamped to one pawn, enough to fix a pawn
    /// structure or a placement the evaluation misjudges, never to outweigh a piece. Kept on
    /// 2026-10-31 after its duels: drawn, but about +17 Elo on the clock with both networks.
    fn correction(&self, board: &Board) -> i32 {
        let sum: i32 = Self::correction_indices(board).iter().map(|&index| self.correction_history[index]).sum();
        (sum / 3).clamp(-self.eval_margins.pawn, self.eval_margins.pawn)
    }

    /// Teaches the correction history that a search of `depth` found `score` where the static
    /// evaluation said `static_eval` (uncorrected), `bound` telling which side of the truth `score`
    /// is. Only a sign the bound proves: a fail high only shows the score at least `score`, so it
    /// may raise an entry, never lower it, a fail low the reverse. The error is clamped to two pawns:
    /// a larger one is a tactic, not the evaluation's bias. Each entry moves toward the error by
    /// `min(depth, CORRECTION_DEPTH_CAP) / CORRECTION_WEIGHT_DIVISOR` of the gap.
    fn update_correction(&mut self, board: &Board, depth: u32, static_eval: i32, score: i32, bound: Bound) {
        let error = score - static_eval;
        let proven = match bound {
            Bound::Exact => true,
            Bound::Lower => error > 0,
            Bound::Upper => error < 0,
        };
        if !proven {
            return;
        }
        let error = error.clamp(-2 * self.eval_margins.pawn, 2 * self.eval_margins.pawn);
        let weight = depth.min(CORRECTION_DEPTH_CAP) as i32;
        for index in Self::correction_indices(board) {
            let entry = &mut self.correction_history[index];
            *entry += (error - *entry) * weight / CORRECTION_WEIGHT_DIVISOR;
        }
    }

    /// What a cutoff on a static `value >= beta` returns: the midpoint of `value` and `beta`. The
    /// cutoff proves only `>= beta`; the excess is the evaluation's say alone, trusted half. Never
    /// below `beta`, the division truncating toward zero from a sum of at least `2 * beta`. A mate or
    /// tablebase `beta` is left alone, `value` returned: halfway to it is no score at all. Kept on
    /// 2026-10-31 after its duels: won at depth 9, flat on the clock.
    fn blended(value: i32, beta: i32) -> i32 {
        if is_win(beta) || is_loss(beta) || is_tb_win(beta) || is_tb_loss(beta) {
            return value;
        }
        (value + beta) / 2
    }

    /// Sets the evaluator and its pawn scale, and for a psq network allocates one accumulator pair
    /// per ply and computes the root's from scratch.
    fn set_evaluator(&mut self, evaluator: Option<&'a Network>, root: &Board) {
        self.evaluator = evaluator;
        self.set_pawn_percent(evaluator.map_or(100, Network::pawn_percent));
        self.accumulators = match evaluator {
            // Every entry starts as the root's: entry 0 is then correct, and the rest are only
            // buffers of the right size, each overwritten before it is read.
            Some(Network::Psq(_, network)) => vec![network.refresh_by_color(root); MAX_PLY as usize + 2],
            _ => Vec::new(),
        };
    }

    /// Sets the evaluator's pawn scale, in percent, and the margins derived from it.
    fn set_pawn_percent(&mut self, pawn_percent: i32) {
        self.pawn_percent = pawn_percent;
        self.eval_margins = eval_margins(&self.parameters, pawn_percent);
    }

    /// Sets the search's parameters, and their margins in the evaluator's pawn.
    fn set_parameters(&mut self, parameters: SearchParameters) {
        self.parameters = parameters;
        self.eval_margins = eval_margins(&parameters, self.pawn_percent);
    }

    /// Razoring's test: `eval` so far below `alpha` that only quiescence is worth trusting.
    fn is_razored(&self, eval: i32, alpha: i32, remaining: u32) -> bool {
        eval < alpha.saturating_sub(self.eval_margins.razoring * remaining as i32)
    }

    /// Reverse futility's test: `static_eval` so far above `beta` that the node needs no search.
    fn is_reverse_futile(&self, static_eval: i32, beta: i32, remaining: u32) -> bool {
        static_eval - self.eval_margins.reverse_futility * remaining as i32 >= beta
    }

    /// Classical futility's test: no quiet move can plausibly lift `static_eval` to `alpha`.
    fn is_futile(&self, static_eval: i32, alpha: i32, remaining: u32) -> bool {
        static_eval + self.eval_margins.futility * remaining as i32 <= alpha
    }

    /// `centipawns` of `piece_value`'s pawn in the evaluator's own pawn, for comparing with a
    /// static evaluation. Exactly `centipawns` for PeSTO.
    fn in_evaluator_pawns(&self, centipawns: i32) -> i32 {
        centipawns * self.pawn_percent / 100
    }

    /// `accumulators[ply + 1]` from `accumulators[ply]`, for `child` reached from `parent` at
    /// `ply`. A no-op unless the evaluator is psq.
    fn update_accumulators(&mut self, ply: u32, parent: &Board, child: &Board) {
        let Some(Network::Psq(_, network)) = self.evaluator else {
            return;
        };
        let ply = ply as usize;
        if ply + 1 >= self.accumulators.len() {
            return;
        }
        let (before, after) = self.accumulators.split_at_mut(ply + 1);
        network.update(&before[ply], &mut after[0], parent, child);
        #[cfg(feature = "node-stats")]
        crate::node_stats::updated(ply + 1);
    }

    /// The recursive search behind `alpha_beta`. Identical to `negamax_with_buffer` except
    /// for the things alpha-beta actually adds: the `alpha`/`beta` window itself, the node
    /// counter, the early-exit once a move proves this node can't affect its parent's
    /// decision, and the move ordering (the picker, `self.killers`, `self.history`).
    ///
    /// `ply` is distance already traveled from this search's own root (`0` there, `+1` per
    /// recursive call); `target_depth` is how deep this search was asked to go, constant
    /// across the whole recursion. `target_depth - ply` -- "remaining depth-to-go" -- is what
    /// a single `depth` parameter used to mean before this pair replaced it, and is recomputed
    /// from the two below wherever the old code needed it (recursion continue/leaf, mate
    /// scoring, killer indexing). Counting up rather than down is deliberate: it puts this
    /// already in the dual-counter shape selective search (extensions, reductions, null-move
    /// pruning) will eventually need, and it's what lets a second, independent threshold
    /// (`MAX_PLY`, checked below) exist without a second parameter competing with this one for
    /// what "depth" means.
    ///
    /// The moves come from the staged picker (`move_picker`), each scored once per node: the TT
    /// move, good captures, the killers, good quiets, losing captures, bad quiets.
    fn alpha_beta_with_buffer(
        &mut self,
        board: &Board,
        ply: u32,
        target_depth: u32,
        mut alpha: i32,
        mut beta: i32,
    ) -> i32 {
        // Checked first, unconditionally, before even counting this call as a node -- once a
        // deadline has actually tripped (below), every remaining call anywhere in the tree
        // should bail immediately rather than do any further real work. See `Search::stopped`'s
        // own doc comment for why this can't just be folded into the throttled check below.
        if self.stopped {
            return alpha;
        }

        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply);

        // Unconditional, before any of the early returns below -- see `Search::pv`'s own doc
        // comment for why. This row is shared across the whole tree at this same `ply`; without
        // clearing it here, a node that bails out through one of those early returns (never
        // reaching the improvement write further down) would leave behind whatever an entirely
        // unrelated earlier node last wrote there, for a parent to later splice into its own line
        // as if it were this node's real continuation. Clearing here makes every early return
        // correct for free: each one now hands back an already-empty row, meaning exactly "no
        // further main-search continuation is known from here" -- which is the truth for all of
        // them (a repetition, a TT cutoff, insufficient material, a fifty-move draw, or the
        // horizon itself all have no real line to report), not just for the terminal-node case
        // this used to be handled for alone.
        self.pv.clear(ply);

        // Unconditional, for the identical reason `pv[ply]` just above is: `static_evals` is
        // shared across the whole tree at this same `ply`, so without clearing it here, a node
        // that skips writing a real value (in check, no eval computed) would otherwise leave
        // behind whatever an unrelated, already-backtracked sibling last wrote there -- and
        // `is_improving`, reading two plies back, would silently splice in contaminated data
        // from a completely different branch (see that field's own doc comment).
        self.static_evals[ply as usize] = None;

        // The throttled half of the same mechanism: actually reading the clock (or the shared
        // flag), but only once every `CHECK_INTERVAL` nodes rather than on every call -- see
        // its own doc comment for why. Trips `self.stopped` rather than returning some special
        // value the caller has to recognize; every frame above this one on the call stack,
        // waiting on this one's return, will hit the unconditional check above on its own very
        // next call and bail the same way.
        if self.nodes.is_multiple_of(CHECK_INTERVAL) {
            // Checked first, regardless of thread: this is how every thread other than 0 ever
            // learns time is up at all, since only thread 0's `deadline` is ever `Some` (see
            // that field's own doc comment). For thread 0 this is almost always a no-op (it's
            // the only writer), but checking it unconditionally costs nothing and needs no
            // special-casing by thread identity here.
            if self.stop_signal.load(Ordering::Relaxed) {
                self.stopped = true;
                return alpha;
            }
            if self.is_out_of_time() {
                self.stopped = true;
                // Thread 0 alone reaches this: broadcasts the decision to every other
                // thread sharing this same `go`'s stop_signal, not just this one's own
                // `self.stopped`.
                self.stop_signal.store(true, Ordering::Relaxed);
                return alpha;
            }
        }

        // Checked next, ahead of the `MAX_PLY` fallback below: a repetition is a genuine,
        // definitive fact about this position (it's a real draw), where `MAX_PLY` is only an
        // approximation (a plain evaluation standing in for search that didn't happen) -- the
        // real fact should win whenever both would otherwise apply to the same node.
        //
        // `ply > 0`, a real bug fix, not a design choice: without this guard, a root position
        // that happens to repeat an earlier position from the real game's own move history (a
        // completely ordinary thing for `position fen ... moves ...` to hand the engine, e.g. a
        // shuffle that returns to a position already played once) would return this bare `0`
        // before the move loop ever runs, leaving `pv[0]` empty and no move recorded at all --
        // the identical illegal-`bestmove` bug class the TT probe, razoring, reverse futility
        // pruning, and mate distance pruning already guard against for the exact same reason.
        // Found via a real bench position (`search.md`'s progress log has the full diagnosis),
        // not a unit test -- the same way `root-tt-cutoff.md`'s own bug was originally found.
        if ply > 0 && self.is_repetition(board) {
            return 0;
        }

        // Same reasoning and the same fix as `is_repetition` just above -- see
        // `is_insufficient_material`'s own doc comment for why this can be checked this early
        // otherwise, unconditionally, unlike `is_fifty_move_draw`. Not yet observed to actually
        // misfire on any real bench position the way `is_repetition` was, but the exact same gap
        // exists in principle, so it's fixed alongside it rather than left for later.
        if ply > 0 && is_insufficient_material(board) {
            return 0;
        }

        // A tablebase hit is the same kind of genuine, definitive fact about this exact
        // position as the repetition/insufficient-material checks just above -- checked here,
        // ahead of everything else below, for the same reason. `ply > 0`, the same guard those
        // two use: a hit at the real root (ply 0) would return an exact score with no move
        // behind it, the identical bestmove-0000 bug class. `self.tablebase` is `None` (skipped
        // outright, no probe attempted) until a real one is threaded through search_root --
        // see `Search::tablebase`'s own doc comment. `SyzygyTablebase::probe` already declines
        // (returns `None`) whenever the position isn't one its own preconditions cover (too
        // many pieces, or reached by a non-zeroing move) -- see its own doc comment -- so this
        // is a no-op past that first `Option` check for the overwhelming majority of nodes,
        // not a real file probe every time. `tb_win_at(ply)`/`tb_loss_at(ply)` fade by search
        // ply exactly like `mate_in`/`mated_in` do, since there is no tablebase-reported
        // distance to fade by instead (WDL only) -- see their own doc comments.
        if ply > 0
            && let Some(tablebase) = self.tablebase
            && let Some(result) = tablebase.probe(board)
        {
            let score = match result {
                TbResult::Win => tb_win_at(ply),
                TbResult::Draw => 0,
                TbResult::Loss => tb_loss_at(ply),
            };
            // Cached, unlike every other early-return cutoff in this function
            // (repetition/insufficient-material/mate-distance/MAX_PLY all return
            // directly with no store) -- a genuine exception, not an oversight: this is
            // the one early return whose value is a completely resolved fact about the
            // position (Win/Draw/Loss, not a mere alpha-beta bound), so a transposition
            // into this same position later -- via a different move order elsewhere in
            // the tree, or a deeper iterative-deepening depth -- gets a free, still-
            // correct hit instead of re-probing the table from scratch. `Bound::Exact`
            // (already true, not a choice -- see `wdl_to_result`'s own doc comment for
            // why Fattorini's collapsed three-way TbResult carries no cursed-value ambiguity
            // left to represent as a Lower/Upper bound the way Stockfish's own five-way
            // Wdl still needs to). `MAX_PLY` as the stored depth, not this node's own
            // `remaining`: unlike an ordinary search score (only as trustworthy as how
            // much subtree backed it up), this fact's reliability never depends on depth
            // at all, so storing it at the absolute ceiling means any future probe at
            // this position trusts it immediately, regardless of that query's own
            // remaining depth -- `store`'s own `depth > existing_depth` replacement
            // check, and `probe`'s own callers' `entry.depth() >= remaining` cutoff
            // check, both already treat a larger stored depth as more authoritative, so
            // this needs no special-casing on either end.
            self.tt.store(board.hash(), tablebase_tt_move(), MAX_PLY, score_to_tt(score, ply), Bound::Exact);
            return score;
        }

        // Mate distance pruning: genuinely different from every other pruning technique in
        // this function -- a provable tightening of the window itself, not a heuristic guess
        // that could ever discard a real line. `alpha` can never legitimately be lower than
        // "already mated here" (`mated_in(ply)`); `beta` can never legitimately be higher than
        // "mate delivered on the very next move" (`mate_in(ply + 1)`). If tightening both
        // collapses the window (`alpha >= beta`), a shorter mate was already found elsewhere in
        // the tree (or this branch is already worse than a mate found elsewhere), and no real
        // search could ever change that -- checked against Stockfish's own real source (fetched,
        // not recalled -- rule 2), which does exactly this. `ply > 0`, a deliberate divergence
        // from Stockfish's own version (it has no such gate): a bare cutoff here reports a score
        // with no legal move and no `pv[0]` behind it, the identical `bestmove 0000` bug class
        // the TT probe, razoring, and reverse futility pruning below already guard against --
        // Fattorini's own `pv` mechanism is why this needs the gate, not a disagreement with
        // Stockfish's own reasoning.
        alpha = alpha.max(mated_in(ply));
        beta = beta.min(mate_in(ply + 1));
        if ply > 0 && alpha >= beta {
            return alpha;
        }

        // The absolute ply safety ceiling, checked next -- essentially never actually binds
        // *here* specifically (real chess's exponential branching means `target_depth` itself
        // essentially never reaches anywhere near `MAX_PLY` in practice, and time management is
        // what stops an ordinary search long before then regardless), but is no longer provably
        // inert the way it was when a separate, always-shallower `MAX_ITERATIVE_DEPTH` backed
        // it up: there's nothing else now stopping `target_depth` itself (an explicit `go depth
        // N` for unusually large `N`) from reaching this far in an unusually forced,
        // low-branching line. `quiescence` also extends `ply` well past `target_depth` with its
        // own separate copy of this same check, not by recursing back through here.
        if ply >= MAX_PLY {
            return self.evaluate(board, ply);
        }

        // Transposition-table lookup. A single `probe` (not two) into an owned `Option<Entry>`
        // (`Entry` is `Copy`) so the borrow of `self.tt` ends immediately, rather than being
        // held open across the loop below by a returned `&Entry`. `remaining` sufficient depth
        // is what separates a genuine cutoff from a mere move-ordering hint: an entry searched
        // shallower than this node's own remaining work hasn't actually proven anything about
        // it, so it falls through to `tt_move` below instead of resolving the node outright.
        // Deliberately two-outcome, not three: a `Lower`/`Upper` hit that doesn't clear this
        // node's own `alpha`/`beta` window outright is simply not used to narrow that window
        // either (unlike some textbook presentations of this same lookup, which tighten
        // `alpha`/`beta` from a non-cutoff bound before continuing the search) -- it only ever
        // falls through to `tt_move` below. Narrower everywhere else in this function stays
        // fully correct either way (a wider window than strictly necessary costs pruning
        // opportunity, never correctness), just with less of it than the more thorough version
        // would extract from the same entry.
        //
        // `ply > 0`, not unconditional -- found the hard way, via a real game (illustrated by
        // this project's own testing tools finding it, not a unit test): `search_root` calls
        // this function at `ply == 0` and then reads back `self.pv[0]` to decide which move to
        // actually report as `bestmove`. A bare cutoff resolves the node with a score alone,
        // never touching `pv[0]` (that only gets written by the move loop below), so if the
        // root's own position already has a sufficiently-deep TT entry -- entirely realistic
        // after several real moves, since the same position can easily have been reached as a
        // transposition somewhere inside an earlier move's own deeper search tree, and the
        // table persists for the whole game -- the root can resolve with an empty `pv`, which
        // `choose_best_move` cannot tell apart from a genuinely legal-move-free position
        // (checkmate/stalemate). It then reports no move at all, an actual illegal `bestmove
        // 0000` sent to a real GUI even though the position plainly has legal replies. Checked
        // directly against Stockfish's own `search.cpp`: its own cutoff is gated on `!PvNode`,
        // and the root is always a PV node, so it never takes this shortcut there either --
        // only ever using a root TT hit as the `tt_move` ordering hint below, exactly as this
        // does now. Every non-root node is completely unaffected: interior nodes never need to
        // report their own best move upward, only their score, so a bare cutoff there was
        // always correct and still is.
        let remaining = target_depth - ply;
        let color = board.side_to_move();

        // Computed once, reused by `static_eval` below, razoring, and late-move pruning's own
        // node-level gate -- three separate consumers now, no longer worth three separate
        // inline `is_in_check` calls at the same node.
        let in_check = is_in_check(board, color);

        // The fifty-move draw at once when not in check, as Stockfish and `quiescence` do: no mate
        // is possible without check, so nothing below needs searching. In check, a mate must still
        // win over the rule, so that case waits for move generation (the late check below), and so
        // does the root, for the same bestmove-0000 reason as the repetition check above.
        if ply > 0 && !in_check && is_fifty_move_draw(board) {
            return 0;
        }

        let tt_entry = self.tt.probe(board.hash());
        if let Some(entry) = tt_entry
            && ply > 0
            && entry.depth() >= remaining
        {
            let score = score_from_tt(entry.score(), ply);
            match entry.bound() {
                Bound::Exact => return score,
                Bound::Lower if score >= beta => return score,
                Bound::Upper if score <= alpha => return score,
                _ => {}
            }
        }
        let tt_move = tt_entry.map(|entry| entry.best_move());

        // This node's one static evaluation, after the TT cutoffs above so a cutoff never pays for
        // it. Every reader below takes it from here, and so does `quiescence` when this node is
        // handed to it whole (the leaf, a razored node): the same board at the same ply always
        // evaluates the same, so a second call would only cost time, and with a trained network
        // that time is most of a leaf. `Some` wherever a reader can use it: off check, either
        // below the root (razoring, null-move pruning, `static_evals`) or within futility's depth
        // (both futility techniques, which the root also uses).
        // `raw_eval` is what the correction history learns against; `node_eval`, corrected, is what
        // every reader below and `quiescence` see.
        let raw_eval = if !in_check && (ply > 0 || remaining <= FUTILITY_MAX_DEPTH) {
            Some(self.evaluate(board, ply))
        } else {
            None
        };
        let node_eval = raw_eval.map(|raw| raw + self.correction(board));
        // Both futility pruning techniques' view of it (classical, inside the move loop, and
        // reverse, below): `Some` only within their depth ceiling, read against `remaining`
        // before the IIR reduction below.
        let static_eval = if remaining <= FUTILITY_MAX_DEPTH { node_eval } else { None };

        // Internal Iterative Reduction: no TT move means no ordering hint for this node's own
        // move loop below -- reduce its own depth rather than spend a full, ordering-blind
        // search on it (see `iir_reduction`'s own doc comment for the trigger condition and why
        // it's a separate, unit-tested function). Shadows both `target_depth` (what every
        // recursive call below this point hands its children) and `remaining` (what razoring/
        // reverse futility/LMR eligibility already gate on, further down this same function)
        // together, so the reduction is real everywhere downstream in this call, not just to
        // recursive children -- matching where Stockfish's own real implementation applies this,
        // before razoring/futility/the move loop, not after. `iir_reduction`'s own doc comment
        // covers why this is gated on `ply > 0`, unlike the rest of this technique's condition.
        let reduction = iir_reduction(ply, tt_move, remaining);
        let target_depth = target_depth - reduction;
        let remaining = remaining - reduction;

        // Razoring: unbounded in remaining depth, unlike both futility techniques around it --
        // Stockfish's own real shape (fetched, not recalled -- rule 2) has no separate depth
        // ceiling either, letting the margin's own growth with `remaining` do all the gating.
        // Where reverse futility below asks "is this position already so good it can't need
        // searching," razoring asks the mirror question against `alpha` instead of `beta`: is
        // this position so far below `alpha` that no continuation from here could plausibly
        // recover, making the full move loop pointless -- only `quiescence`'s own
        // captures-and-checks-only search still worth trusting. Reads `eval`, this node's
        // evaluation below the root, rather than `static_eval`: that one is gated to shallow
        // remaining depth only, where razoring's own unbounded shape needs to fire regardless
        // of depth. `!is_win(alpha) && !is_tb_win(alpha)`: without it, a mate-or-
        // tablebase-scale `alpha` would make any ordinary evaluation look hopeless by
        // comparison, silently skipping the one kind of search (quiet mating nets, or a real
        // path into tablebase-covered territory) that could actually deliver on it. `is_tb_win`
        // added alongside the tablebase work, for the same reason every other pruning guard in
        // this function got the identical addition: `TB_WIN_SCORE` sits below `is_win`'s own
        // threshold on purpose (so it prints as `cp`, not `mate`), so it would otherwise sail
        // straight past a guard meant to catch exactly this class of score. `ply > 0`,
        // the same root-exclusion every other bare-score cutoff in this function already
        // needs (see the TT probe's own doc comment for the real game that caught this once).
        // Null-move pruning below reads the same `eval`, which is `None` whenever `ply == 0 ||
        // in_check`, exactly the condition both consumers already need to check first anyway.
        let eval = if ply > 0 { node_eval } else { None };
        if let Some(e) = eval {
            // Recorded regardless of whether razoring itself actually fires below -- the one
            // reading of the evaluation not gated to shallow `remaining` the way `static_eval`
            // is, so the natural place to populate `static_evals` for `is_improving`'s own
            // later use (`search.md`'s design section has the full reasoning).
            self.static_evals[ply as usize] = Some(e);

            // `!is_win(alpha)` now gates only the pruning decision, not the eval computation
            // above -- a node where `alpha` happens to be mate-scale still gets its eval
            // recorded for `is_improving`'s benefit, even though razoring itself won't fire
            // there. `saturating_sub`, not a bare `-`: `alpha` legitimately equals the
            // `-INFINITY` sentinel (`i32::MIN + 1`) at plenty of ordinary nodes -- the very
            // first move of any search inherits it directly from its parent. Subtracting any
            // positive margin from that underflows `i32` outright. Saturating is also the
            // semantically correct choice, not just the safe one: if `alpha` is already at the
            // very bottom of the representable range, no real evaluation could ever be lower
            // than that either, so this correctly can never fire in that case, rather than
            // needing a separate special-cased guard for it.
            if !is_win(alpha) && !is_tb_win(alpha) && self.is_razored(e, alpha, remaining) {
                return self.quiescence_from(board, ply, alpha, beta, None, eval);
            }
        }

        // Reverse futility pruning (static null-move pruning): the mirror image of classical
        // futility pruning below, reusing the identical `static_eval`/margin/depth-ceiling
        // rather than a fresh, unmeasured pair -- see `search.md`'s design section. Where
        // classical futility asks "can any one quiet move plausibly raise the score to alpha,"
        // this asks the opposite up front, before a single move is even generated: is this
        // node's own static evaluation already so far above `beta`, even after subtracting the
        // same generous margin, that no line starting here plausibly needs searching at all.
        // `ply > 0`, not unconditional -- the exact same root-vs-non-root distinction the TT
        // cutoff above already has to make, and for the identical reason: a bare cutoff here
        // reports a score with no legal move and no `pv[0]` behind it, which `search_root`
        // can't tell apart from a genuinely move-free position (see that probe's own doc
        // comment for the real game that already caught this exact bug once).
        if ply > 0 && let Some(static_eval) = static_eval && self.is_reverse_futile(static_eval, beta, remaining) {
            return Self::blended(static_eval, beta);
        }

        // Declared here, ahead of null-move pruning below, rather than immediately before the
        // move loop the way it used to be -- both now need a scratch board to make and unmake a
        // move on before the loop itself has generated anything. Reused unchanged by the loop
        // afterward: `make_null_move`/`unmake_null_move` restore it to exactly `*board` again
        // before either of them ever runs.
        let mut scratch = *board;

        // Null-move pruning: give the opponent a free tempo (skip this side's own move
        // entirely) and search the result at reduced depth with a null window around `beta`. If
        // even that best-case-for-the-opponent search still fails high, a real move here should
        // do at least as well, so the whole subtree is pruned without generating a single move.
        // Not the same technique as reverse futility pruning just above, despite that
        // technique's own alternate textbook name ("static null-move pruning") -- that one never
        // actually makes a move; this one does. `search.md`'s design section has the full
        // soundness-guard reasoning for each condition `is_null_move_pruning_applicable` checks.
        if is_null_move_pruning_applicable(
                ply,
                in_check,
                self.null_move_in_progress,
                remaining,
                beta,
                has_non_pawn_material(board, color),
            )
            && let Some(e) = eval
            // Cheap pre-filter before paying for the recursive probe at all: if this node's
            // own evaluation is already below `beta`, giving the opponent a free move can
            // only make things look worse still, never better -- Stockfish's own real gate
            // for the identical reason.
            && e >= beta
        {
            let undo = scratch.make_null_move();
            self.update_accumulators(ply, board, &scratch);
            self.null_move_in_progress = true;
            self.ancestors.push(board.hash());
            // A null move is a pass, not a real move -- nothing for continuation
            // history to condition on, so the child sees `None` here explicitly
            // (not whatever this same slot happened to hold from an unrelated,
            // already-backtracked branch), the same "no previous move" meaning the
            // root itself already has.
            self.prev_move[ply as usize + 1] = None;
            // The child sits one ply deeper already, so it is handed `plies - 1` less
            // target depth to end up `plies` shallower than this node.
            let plies = null_move_plies(remaining, e - beta, self.eval_margins.null_move);
            let null_score = -self.alpha_beta_with_buffer(
                &scratch,
                ply + 1,
                target_depth - (plies - 1),
                -beta,
                -beta + 1,
            );
            self.ancestors.pop();
            self.null_move_in_progress = false;
            scratch.unmake_null_move(undo);

            if null_score >= beta {
                // A "mate" (or, since the tablebase work, a tablebase win) found only
                // because the opponent got two moves in a row isn't real -- clamped to
                // `beta` rather than left to propagate upward and corrupt real mate- or
                // tablebase-distance reasoning elsewhere in the tree. `is_tb_win` added
                // for the identical reason: `TB_WIN_SCORE` sits below `is_win`'s own
                // threshold on purpose, so a TB-scale `null_score` would otherwise slip
                // through this clamp unrecognized.
                return if is_win(null_score) || is_tb_win(null_score) { beta } else { null_score };
            }
        }

        // The leaf: everything past this point is the move loop, and at the leaf the loop would
        // only learn whether a legal move exists (its alpha update, recursion, pruning, and table
        // updates are all for interior nodes), so the leaf goes to `quiescence` here instead,
        // with its evaluation as the stand pat. Mate, stalemate, and the fifty-move rule are
        // decided first, in the order the interior nodes decide them after their loop, by a
        // legal-move test that stops at the first legal move: quiescence never finds stalemate,
        // and deciding the rest inside it would count it as one more node.
        if ply >= target_depth {
            if !self.has_legal_move(board, color) {
                return if in_check { -(MATE_SCORE - ply as i32) } else { 0 };
            }
            if is_fifty_move_draw(board) {
                return 0;
            }
            // `alpha`/`beta` as this call was given them, so quiescence continues the same
            // window. No store here: `quiescence` probes and stores this same position itself.
            return self.quiescence_from(board, ply, alpha, beta, None, node_eval);
        }

        // Saved before the loop below ever touches `alpha` -- `classify_bound` (used at this
        // node's own store, near the end) needs the window's original lower edge, not whatever
        // `alpha` has narrowed to by the time this node finishes.
        let original_alpha = alpha;

        // Late-move pruning's own node-level gate and `improving` state, both constant across
        // every iteration of the loop below so computed once here rather than per-move.
        // `has_non_pawn_material`: guards against the zugzwang risk of pruning a quiet move in
        // a pure king-and-pawn ending (`search.md`'s design section has the full reasoning).
        let late_move_pruning_applicable = ply > 0 && !in_check && has_non_pawn_material(board, color);
        let improving = self.is_improving(ply, beta);

        // Where this node's moves begin in the shared buffer: the picker appends them stage by stage.
        let start = self.moves.len();
        // The `move-stats` measurement scores every move before the loop, so it generates them all.
        #[cfg(feature = "move-stats")]
        let all_moves: Vec<Move> = {
            let mut all = Vec::new();
            pseudo_legal_moves(board, color, &mut all);
            all
        };

        // Same provably-safe-placeholder reasoning as negamax's own best_score (see above).
        let mut best_score = i32::MIN;
        // Always `Some` by the time this node reaches its own store below, for `ply <
        // target_depth` with at least one legal move: `best_score` starts at `i32::MIN`, so the
        // first legal move that's actually recursed into -- whatever its `child_score` -- already
        // satisfies `child_score > best_score` and sets this the moment it returns. Futility
        // pruning below is deliberately kept from ever skipping *every* legal move at a node
        // (it never fires while this is still `None`), which is exactly what keeps that "always
        // `Some`" guarantee true even though not every legal move gets recursed into anymore.
        let mut best_move: Option<Move> = None;
        let mut found_legal_move = false;
        // How many moves have already been confirmed legal at *this* node -- read by
        // late-move pruning's own threshold check below (against the count *before* the
        // current move, incremented after), matching the quiescence move-count cap's own
        // already-reasoned-through convention. Incremented unconditionally once legality is
        // known, regardless of whether the current move itself then also gets futility- or
        // late-move-pruned -- Stockfish's own `moveCount` counts every move looked at, not just
        // the ones a given technique cares about.
        let mut moves_tried: u32 = 0;

        // Every quiet move fully searched at this node so far, in the order tried -- collected
        // so that if a *later* move goes on to cause a cutoff, all of these can be penalized as
        // "tried and failed" (the history malus below), not left indistinguishable from a move
        // nobody has ever looked at. A move only ever lands here on the non-cutoff path (see the
        // `else if searched_quiet_move` branch below), so by construction it never includes
        // whichever move eventually does cause the cutoff -- that one gets the bonus instead.
        // Discarded unused if this node's loop never reaches a cutoff at all, matching malus's
        // own "only on a cutoff" trigger, the same condition the existing bonus already uses.
        // `self.quiets[quiets_start..]`: this node's own, behind its ancestors' (`Search::quiets`).
        let quiets_start = self.quiets.len();

        // The `move-stats` measurement: every legal quiet move but the TT move, scored before the
        // loop, and what the loop then did with each.
        #[cfg(feature = "move-stats")]
        let quiet_scores: Vec<(Move, i32)> = if in_check {
            Vec::new()
        } else {
            all_moves
                .iter()
                .copied()
                .filter(|&mv| {
                    if Some(mv) == tt_move || !is_quiet(board, mv) {
                        return false;
                    }
                    let mut after = *board;
                    after.make_move(mv);
                    return !is_in_check(&after, color);
                })
                .map(|mv| {
                    let score = combined_history_score(
                        &self.history, &self.continuation_history, self.prev_move[ply as usize], board, mv,
                    );
                    return (mv, score);
                })
                .collect()
        };
        #[cfg(feature = "move-stats")]
        let losing_captures: Vec<Move> = if in_check {
            Vec::new()
        } else {
            all_moves
                .iter()
                .copied()
                .filter(|&mv| {
                    if Some(mv) == tt_move || is_quiet(board, mv) || see_ge(board, mv, 0) {
                        return false;
                    }
                    let mut after = *board;
                    after.make_move(mv);
                    return !is_in_check(&after, color);
                })
                .collect()
        };
        #[cfg(feature = "move-stats")]
        let mut reached: Vec<Move> = Vec::new();
        #[cfg(feature = "move-stats")]
        let mut searched: Vec<Move> = Vec::new();
        #[cfg(feature = "move-stats")]
        let mut cutoff_move: Option<Move> = None;

        // The staged picker (`move_picker`) hands out every move once, in its order: the TT move,
        // good captures, the two killers, good quiets, losing captures, bad quiets; in check,
        // every evasion sorted.
        let mut picker =
            MovePicker::new(start, tt_move, self.killers[ply as usize], self.parameters.good_quiet_threshold, in_check);
        // Legality judged on `board`, before any move is made: only a move actually searched
        // below is made, so a pruned one costs no make, check test or unmake.
        let legality = Legality::new(board, color);
        // The legal moves reached so far other than the TT move: the cutoff move's place in the
        // order a move head would improve.
        #[cfg(feature = "node-stats")]
        let mut legal_after_tt: u64 = 0;
        // `(nodes, wasted nodes)` when the first legal move after the TT move started, and when the
        // latest one did.
        #[cfg(feature = "node-stats")]
        let (mut span_start, mut move_start): ((u64, u64), (u64, u64)) = ((0, 0), (0, 0));
        loop {
            let next = picker.next(
                &mut self.moves, &mut self.scores, board, &self.history, &self.continuation_history,
                self.prev_move[ply as usize],
            );
            let Some(mv) = next else {
                break;
            };

            // MultiPV's own root move exclusion (`Search::excluded_root_moves`'s own doc comment)
            // -- only ever non-empty at ply == 0, and only ever populated by search_root for a
            // repeated root search finding the next-best move with better-ranked ones out of the
            // way. Skipped before make_move/legality/every pruning decision below: an excluded
            // move contributes nothing to this search at all, exactly as if it weren't legal here.
            if ply == 0 && self.excluded_root_moves.contains(&mv) {
                continue;
            }

            #[cfg(feature = "move-stats")]
            reached.push(mv);

            // Read after the `if legality.is_legal` block below (whether this move triggers
            // late-move pruning decides whether the rest of the quiets are skipped) -- declared
            // out here so it's still in scope there, defaulting to `false` for an illegal
            // pseudo-legal move (which was never really a candidate to begin with).
            let mut late_move_pruned = false;
            // Same declare-outside-the-`if`-block reasoning as `late_move_pruned` just above:
            // read after that block, to decide whether `mv` joins this node's quiets on the
            // non-cutoff path below. Stays `false` for a noisy move, an illegal pseudo-legal
            // move, or one skipped outright by futility/late-move pruning -- none of those were
            // ever actually given a real chance to fail, so none of them belong in the malus
            // pool.
            let mut searched_quiet_move = false;
            if legality.is_legal(board, mv) {
                found_legal_move = true;
                #[cfg(feature = "node-stats")]
                if Some(mv) != tt_move {
                    legal_after_tt += 1;
                    move_start = (self.nodes, crate::node_stats::wasted_nodes());
                    if legal_after_tt == 1 {
                        span_start = move_start;
                    }
                }
                // Always true since the leaf hands off to quiescence before the loop; kept as a
                // block rather than re-indenting everything inside it.
                if ply < target_depth {
                    // Futility pruning: at shallow enough remaining depth, a quiet move that
                    // can't plausibly raise the score to `alpha` even with a generous margin
                    // isn't worth recursing into at all. Never while `best_move` is still
                    // `None` -- deliberately keyed on that, not on `i == start`: a pseudo-legal
                    // candidate earlier in `moves[start..end]` can turn out illegal (a pinned
                    // piece, say), which would leave the true first *legal* move sitting past
                    // index `start` with nothing yet recorded. Gating on `best_move` instead
                    // guarantees the actual first move that would otherwise leave it unset is
                    // always fully searched, which is what keeps `best_move` guaranteed `Some`
                    // below without needing a synthetic fallback score the way Stockfish's own
                    // version does (see `MARGIN_PER_PLY`'s doc comment). `static_eval` is
                    // only `Some` when the node-level gate (`!in_check`, `remaining <=
                    // FUTILITY_MAX_DEPTH`) already holds, so this is otherwise a plain
                    // `is_quiet` check plus one comparison, not a second full re-check of the
                    // node-level conditions.
                    //
                    // `!is_win(alpha)`: a real, pre-existing gap, found and fixed alongside the
                    // tablebase work but not caused by it -- razoring's own identical margin
                    // check (`!is_win(alpha) && e < alpha.saturating_sub(...)`, above) already
                    // has this guard, for exactly the reason explained on its own doc comment: a
                    // mate-scale `alpha` makes any ordinary evaluation look hopeless by
                    // comparison, and without the guard this would wrongly prune a quiet move
                    // that could actually deliver on that mate. This check had no such guard at
                    // all -- unlike razoring's `e < ...` (strictly weaker the higher `alpha`
                    // is, so a huge `alpha` there just never fires), the direction here
                    // (`... <= alpha`) makes a huge `alpha` fire *more* easily, not less, so the
                    // gap was real, not just theoretical. `!is_tb_win(alpha)` alongside it, for
                    // the same reason every other pruning guard in this function needed the
                    // identical addition: `TB_WIN_SCORE` sits below `is_win`'s own threshold on
                    // purpose (so it prints as `cp`, not `mate`), so a TB-scale `alpha` would
                    // otherwise fire this exact same bug all over again.
                    let futility_pruned = match static_eval {
                        Some(static_eval)
                            if best_move.is_some() && is_quiet(board, mv) && !is_win(alpha) && !is_tb_win(alpha) =>
                        {
                            self.is_futile(static_eval, alpha, remaining)
                        }
                        _ => false,
                    };

                    // Late-move pruning: independent of futility pruning above (a move can
                    // satisfy either, both, or neither) -- once several moves have already been
                    // tried at this node without a cutoff, a further quiet one isn't worth
                    // trying either, regardless of `alpha`/`static_eval`. Checked against
                    // `moves_tried` *before* it's incremented just below, matching the
                    // quiescence move-count cap's own convention (`search.md`'s design section
                    // has the full "never skips the first move" reasoning this guarantees).
                    late_move_pruned =
                        is_late_move_pruned(late_move_pruning_applicable, best_score, board, mv, moves_tried, remaining, improving);
                    moves_tried += 1;

                    // History-based pruning: independent of both checks above (a move can
                    // satisfy any combination of the three) -- this specific move's own
                    // accumulated track record across the whole search, not this node's move
                    // count or this node's static evaluation, says it isn't worth trying here
                    // either.
                    let history_pruned = is_history_pruned(
                        best_move.is_some(), &self.history, board, mv, remaining, self.parameters.history_pruning_multiplier,
                    );

                    // Computed once, ahead of both `is_see_pruned`'s own exemption and the
                    // extension mechanism below, rather than calling `gives_check` twice for the
                    // same move -- `board`/`mv` (before the move), not `scratch` (after it), the
                    // convention `gives_check`'s own signature already expects, the same one its
                    // `quiescence` call site uses.
                    let mv_gives_check = gives_check(board, mv);

                    // SEE pruning: independent of the three checks above (a move can satisfy any
                    // combination of the four) -- skips a move outright once its own forced-
                    // exchange result already nets worse than a depth-scaled margin allows,
                    // regardless of whether it's a capture or a quiet move (`search.md`'s design
                    // section has the full reasoning for why this one isn't limited to quiet
                    // moves the way the other three are).
                    let see_pruned = is_see_pruned(
                        best_move.is_some() && !mv_gives_check, board, mv, remaining, self.parameters.see_prune_margin,
                    );

                    if !futility_pruned && !late_move_pruned && !history_pruned && !see_pruned {
                        let undo = scratch.make_move(mv);
                        // The child's slot loads while the accumulators update below.
                        self.tt.prefetch(scratch.hash());

                        // The child's accumulators only now, once the move is sure to be searched:
                        // none of the four pruning tests above reads them, and a pruned move would
                        // pay a whole update (about 100 ns on caroline, 2026-10-07) for nothing.
                        self.update_accumulators(ply, board, &scratch);

                        // Recorded here, unconditionally, the moment this move is confirmed to
                        // actually be searched -- not gated on how the search below turns out,
                        // since even a move that raises `alpha` without reaching `beta` still
                        // counts as "tried" for the next move's own possible malus.
                        searched_quiet_move = is_quiet(board, mv);
                        #[cfg(feature = "move-stats")]
                        searched.push(mv);

                        // Search extension: a move that gives check gets an extra ply of real
                        // depth rather than losing one, added to every genuine (non-LMR-probe)
                        // recursive call below for this move -- `target_depth + extension`, the
                        // mirror image of how LMR/null-move pruning already express a reduction as
                        // `target_depth - k`. No cap on how many times this can stack along one
                        // line (`search.md`'s design section has the full reasoning): a genuinely
                        // unbounded checking sequence can't exist in real chess in the first place
                        // (a finite board must eventually repeat or hit the fifty-move mark), and
                        // `is_repetition`/`is_fifty_move_draw`/the absolute `MAX_PLY` ceiling all
                        // already terminate a line regardless of how many times it's been extended
                        // -- termination was never actually at risk, only worst-case node count
                        // was, and that's a "measure, don't assume" question (rule 5), not a
                        // reason to invent a cap up front.
                        let extension: u32 = if mv_gives_check { 1 } else { 0 };

                        // Continuation history's own "what led here" for whichever child this
                        // move's real recursive search(es) below descend into -- set once per
                        // move, not once per recursive-call variant, since every one of them
                        // (first-move full window, LMR scout, non-LMR scout, re-search) descends
                        // into the exact same child ply for the exact same `mv`. A plain
                        // overwrite, no push/pop needed the way `self.ancestors` needs (unlike a
                        // repetition-detection lineage, this only ever needs the single most
                        // recent entry per ply): the next move tried at this same node simply
                        // overwrites this same slot before its own recursive call.
                        let piece =
                            board.piece_at(mv.from()).expect("alpha_beta_with_buffer: no piece on the from square");
                        self.prev_move[ply as usize + 1] = Some((piece, mv.to()));

                        // `board` (this call's own position, not `scratch`) is what becomes the
                        // child's newest ancestor -- pushed right before recursing into it, popped
                        // right after, so `self.ancestors` always holds exactly the lineage leading
                        // to wherever the search currently is.
                        //
                        // Principal Variation Search: the first move searched at this node (the
                        // still-`None` case, the same signal futility pruning above already keys
                        // off of) gets the full window, same as always. Every move after that
                        // first one gets a cheap null-window scout first -- betting it won't beat
                        // `alpha` -- and only pays for a full-width re-search if that bet actually
                        // fails (`alpha < scout < beta`, meaning this move might really be the new
                        // best line and the one-point-wide scout can't be trusted for its exact
                        // value). A scout that confirms the bet (`<= alpha`) or already proves a
                        // cutoff (`>= beta`) is trustworthy as-is under fail-soft, no re-search
                        // needed either way. See `search.md`'s design section for why this is
                        // main-search only, not also inside `quiescence`.
                        let child_score = if best_move.is_none() {
                            self.ancestors.push(board.hash());
                            let score = -self.alpha_beta_with_buffer(
                                &scratch,
                                ply + 1,
                                target_depth + extension,
                                -beta,
                                -alpha,
                            );
                            self.ancestors.pop();
                            score
                        } else {
                            // Late move reductions: a cheap reduced-depth null-window probe,
                            // tried first for a move eligible under `is_lmr_eligible`, betting
                            // the missing depth won't matter because this move wasn't going to
                            // beat `alpha` anyway. `target_depth - lmr_reduction(remaining,
                            // moves_tried)` is how "that many plies less remaining" is expressed
                            // here -- Fattorini has no separate depth-budget parameter to shrink the
                            // way Stockfish does, but `remaining = target_depth - ply` everywhere
                            // else in this function means handing the child a smaller
                            // `target_depth` already *is* handing it less remaining depth, by that
                            // same definition (search.md's design section). `moves_tried` here is
                            // already post-increment (this move itself included), matching
                            // `lmr_reduction`'s own doc comment. `is_lmr_eligible`'s own `remaining
                            // >= 3` guard keeps this subtraction safe and, more importantly,
                            // `lmr_reduction`'s own `.min(remaining - 2)` clamp guarantees the
                            // reduced child still gets at least 1 real ply of its own before its
                            // own horizon regardless of how large `moves_tried` gets -- see that
                            // function's own doc comment for the full floor argument. Only a
                            // fail-low (`<= alpha`) is trusted directly; landing inside the window,
                            // or even looking like it beats `beta`, falls through to the unreduced
                            // ladder below instead of being believed -- a reduced search's fail-
                            // high can be a horizon artifact (a refutation the missing depth didn't
                            // get to see), not a real result.
                            let reduced_fail_low = if is_lmr_eligible(remaining, board, mv) {
                                self.ancestors.push(board.hash());
                                let scout = -self.alpha_beta_with_buffer(
                                    &scratch,
                                    ply + 1,
                                    target_depth - lmr_reduction(remaining, moves_tried),
                                    -(alpha + 1),
                                    -alpha,
                                );
                                self.ancestors.pop();
                                if scout <= alpha { Some(scout) } else { None }
                            } else {
                                None
                            };

                            if let Some(scout) = reduced_fail_low {
                                scout
                            } else {
                                self.ancestors.push(board.hash());
                                let scout = -self.alpha_beta_with_buffer(
                                    &scratch,
                                    ply + 1,
                                    target_depth + extension,
                                    -(alpha + 1),
                                    -alpha,
                                );
                                self.ancestors.pop();
                                if scout > alpha && scout < beta {
                                    self.ancestors.push(board.hash());
                                    let score = -self.alpha_beta_with_buffer(
                                        &scratch,
                                        ply + 1,
                                        target_depth + extension,
                                        -beta,
                                        -alpha,
                                    );
                                    self.ancestors.pop();
                                    score
                                } else {
                                    scout
                                }
                            }
                        };
                        // Strictly-greater, not `.max()`: identical resulting `best_score` either
                        // way (a tie never changed it under `.max()` either), but this gives an
                        // explicit moment -- "best_score just actually improved" -- to hook the
                        // PV bookkeeping onto below.
                        if child_score > best_score {
                            best_score = child_score;
                            best_move = Some(mv);
                            // This node's own best line: this move, followed by whatever the
                            // child just reported as *its* best line.
                            self.pv.update(ply, mv);
                        }
                        alpha = alpha.max(best_score);
                        scratch.unmake_move(mv, undo);
                    }
                }
            }

            // Late-move pruning skips every remaining quiet, not just this one: the threshold only
            // grows, so nothing later at this node could un-trigger it. It does not end the loop,
            // as Stockfish's `skipQuiets` does not: the losing captures come after the good quiets
            // and are still searched.
            if late_move_pruned {
                picker.skip_quiets = true;
                continue;
            }

            // Fail-soft: `best_score` (whatever it actually reached) is what eventually gets
            // returned even after this break, not `beta` itself -- the cutoff only stops
            // *searching further*, it doesn't change what value this node reports. Placed
            // after `unmake_move` so the board is always left consistent regardless of which
            // iteration this fires on.
            if alpha >= beta {
                #[cfg(feature = "move-stats")]
                {
                    cutoff_move = Some(mv);
                }
                #[cfg(feature = "node-stats")]
                {
                    let quiet = is_quiet(board, mv);
                    crate::node_stats::cutoff(Some(mv) == tt_move, legal_after_tt, quiet);
                    // The siblings searched in vain between the TT move and this one, less the
                    // waste already recorded inside them, so no node is counted twice.
                    if Some(mv) != tt_move && legal_after_tt > 1 {
                        let span = move_start.0 - span_start.0;
                        let inner = move_start.1 - span_start.1;
                        crate::node_stats::waste(remaining, span - inner, quiet);
                    }
                }
                if is_quiet(board, mv) {
                    // Same event, three independent tables now: the history bonus below,
                    // continuation history's own identical bonus just after it, and the killer
                    // MRU update after that -- recorded separately since they answer different
                    // questions (a per-(piece, square) running score across the whole search, the
                    // same score further conditioned on the move played one ply before, and "the"
                    // one move to try first at this exact ply).
                    let piece =
                        board.piece_at(mv.from()).expect("alpha_beta_with_buffer: no piece on the from square");
                    let bonus = ((target_depth - ply) * (target_depth - ply)) as i32;
                    apply_history_bonus(&mut self.history[piece as usize][mv.to() as usize], bonus, FLAT_HISTORY_MAX);

                    // Continuation history's own identical bonus, written alongside the flat
                    // one above whenever a previous move actually exists to condition on --
                    // absent at the root or right after a null move (`self.prev_move[ply as
                    // usize]` is `None` there), where there's nothing to index by
                    // (`continuation-history.md`'s own "decided" section).
                    if let Some((prev_piece, prev_to)) = self.prev_move[ply as usize] {
                        let index = continuation_history_index(prev_piece, prev_to, piece, mv.to());
                        apply_history_bonus(&mut self.continuation_history[index], bonus, CONTINUATION_HISTORY_MAX);
                    }

                    // History malus: every quiet move already fully searched at this node,
                    // before this one, gets the exact same bonus formula subtracted instead of
                    // added -- the mirror image of the bonus just above, not a separately-tuned
                    // penalty (`search.md`'s design section has the reasoning for staying this
                    // simple rather than reproducing Stockfish's own, differently-shaped, more
                    // heavily-tuned malus). Without this, a move tried and beaten by a later
                    // cutoff stayed indistinguishable from one nobody had ever tried at all --
                    // both read `0`. Continuation history's own malus mirrors this identically,
                    // same gate as its bonus above.
                    // By index: the body updates `self.history`, which a borrow of `self.quiets`
                    // would forbid. Every child has already truncated its own quiets away.
                    for index in quiets_start..self.quiets.len() {
                        let quiet_mv = self.quiets[index];
                        let quiet_piece = board
                            .piece_at(quiet_mv.from())
                            .expect("alpha_beta_with_buffer: no piece on the from square");
                        apply_history_bonus(
                            &mut self.history[quiet_piece as usize][quiet_mv.to() as usize], -bonus, FLAT_HISTORY_MAX,
                        );
                        if let Some((prev_piece, prev_to)) = self.prev_move[ply as usize] {
                            let index = continuation_history_index(prev_piece, prev_to, quiet_piece, quiet_mv.to());
                            apply_history_bonus(&mut self.continuation_history[index], -bonus, CONTINUATION_HISTORY_MAX);
                        }
                    }

                    // MRU replacement: shift the current slot 0 into slot 1 and put this move
                    // in slot 0 -- unless it's already slot 0, in which case a repeat cutoff
                    // shouldn't duplicate the same move into both slots.
                    let existing = self.killers[ply as usize];
                    if existing[0] != Some(mv) {
                        self.killers[ply as usize] = [Some(mv), existing[0]];
                    }
                }
                break;
            } else if searched_quiet_move {
                // This move didn't cause a cutoff (the `if` above didn't fire) -- if some later
                // move at this same node goes on to cause one, this one becomes eligible for the
                // malus applied above.
                self.quiets.push(mv);
            }
        }

        #[cfg(feature = "move-stats")]
        crate::move_stats::record(remaining, &quiet_scores, &losing_captures, cutoff_move, &searched, &reached);
        #[cfg(feature = "node-stats")]
        if picker.went_past_tt_move() {
            crate::node_stats::ordered_node(remaining);
        }

        self.moves.truncate(start);
        self.quiets.truncate(quiets_start);

        if !found_legal_move {
            // No explicit clear needed here -- this row was already emptied on entry (see the
            // top-of-function comment on that), and a terminal node never reaches the
            // improvement write above, so it's guaranteed to still be empty here: correctly
            // reporting "the game ends here," not some unrelated subtree's stale leftovers.
            // `ply` directly, not `remaining` -- see `MATE_SCORE`'s own doc comment for why
            // the mate-distance sentinel is deliberately independent of `target_depth`.
            return if is_in_check(board, color) { -(MATE_SCORE - ply as i32) } else { 0 };
        }
        if is_fifty_move_draw(board) {
            // Cleared, unlike the terminal case above: this node has searched its children, and
            // an improving one wrote its line into this row. Left there, the parent would splice
            // it into its own line, a reported PV running past the draw. Not at the root, whose
            // row is where `search_root` reads the move it plays.
            if ply > 0 {
                self.pv.clear(ply);
            }
            return 0;
        }
        let best = best_move.expect("an interior node with a legal move always sets best_move");
        let bound = classify_bound(best_score, original_alpha, beta);
        self.tt.store(board.hash(), best, remaining, score_to_tt(best_score, ply), bound);
        // The correction history learns where a quiet best move's search corrects the static
        // evaluation (Stockfish's conditions): never in check (no evaluation), never from a
        // capture (its score is the exchange's, not the evaluation's error), never from a mate or
        // tablebase score, nor from a search cut short.
        if !self.stopped
            && let Some(raw) = raw_eval
            && captured_piece(board, best).is_none()
            && !is_win(best_score.abs())
            && !is_tb_win(best_score.abs())
        {
            self.update_correction(board, remaining, raw, best_score, bound);
        }
        best_score
    }

    /// Whether `board` has already occurred earlier in this search's own lineage --
    /// `self.ancestors`, which by construction never includes `board`'s own hash, only its
    /// ancestors (see `ancestors`'s own doc comment). Bounded to the last
    /// `board.halfmove_clock()` entries: an irreversible move (a pawn move or capture) makes
    /// every position from before it permanently unreachable again, so nothing further back
    /// could ever be a real match -- scanning past that point would only be wasted comparison
    /// work, never find one. A single earlier occurrence is treated as a repetition outright,
    /// not the literal threefold FIDE requires: without a transposition table, reaching an
    /// identical position a second time means its own subtree was already fully explored once
    /// before, at *more* remaining depth than is left now -- continuing would only redo that
    /// same work to a shallower, strictly less reliable result, at real extra cost, for no
    /// actual gain.
    fn is_repetition(&self, board: &Board) -> bool {
        let len = self.ancestors.len();
        let earliest = len.saturating_sub(board.halfmove_clock() as usize);
        self.ancestors[earliest..len].contains(&board.hash())
    }

    /// Whether the position at `ply` looks better than it did two plies ago, for the same side
    /// to move -- Stockfish's own real definition (`search.md`'s design section has the fetched
    /// source), used to scale how aggressively late-move pruning skips quiet moves: more
    /// leniently when things are trending better, since a position getting worse is exactly
    /// when a full move loop is more likely to be needed. `false` whenever `static_evals[ply]`
    /// is `None` (this node was in check, so there's nothing here to compare at all) or `ply <
    /// 2` (no two-plies-back position exists yet this search) -- the safe default, matching
    /// Stockfish's own in-check zero-out. `eval >= beta` short-circuits to `true` regardless of
    /// history, the second half of Stockfish's real definition: a position already clearing
    /// `beta` outright is "improving" on its own terms, no comparison needed.
    fn is_improving(&self, ply: u32, beta: i32) -> bool {
        let Some(eval) = self.static_evals[ply as usize] else { return false };
        if eval >= beta {
            return true;
        }
        if ply < 2 {
            return false;
        }
        match self.static_evals[ply as usize - 2] {
            Some(previous) => eval > previous,
            None => false,
        }
    }

    /// Extends search past the main search's own horizon through noisy moves (captures,
    /// promotions), so a position isn't judged mid-exchange -- e.g. right after a queen takes
    /// a pawn but before the pawn's own defender recaptures (the horizon effect). Shares
    /// `self.moves`/`self.nodes` with the main search; there's no `target_depth` here, only
    /// `ply` checked against the shared `MAX_PLY` ceiling.
    ///
    /// In check, there's no stand pat and no quiet-move filter: every legal move is a
    /// mandatory candidate evasion, exactly as demanding as the main search's own loop, since
    /// declining to respond to check isn't a legal choice the way declining a capture is.
    ///
    /// One known, deliberate gap: a genuine stalemate (not in check, but zero legal moves of
    /// any kind) isn't specially detected here the way the main search detects it -- it would
    /// silently fall through to stand pat's material evaluation instead of the correct `0`.
    /// Catching it would mean generating and legality-checking every move even when none of
    /// them are noisy, defeating quiescence's whole point of only paying for captures. Real
    /// engines' own quiescence search accepts the same gap for the same reason -- checkmate
    /// (only reachable via the mandatory in-check branch, which already legality-checks
    /// everything) is handled because it's unavoidable there, stalemate is not.
    ///
    /// Probes and stores `self.tt`, the same shared table the main search uses -- not a
    /// separate one -- but always with a fixed sentinel `depth` of `0` rather than a computed
    /// remaining-depth-to-go, since quiescence has no `target_depth` to subtract `ply` from.
    ///
    /// `prev_to_sq` is the destination square of the move that led to *this* node, if any --
    /// used only to recognize a recapture (see `is_recapture`), one of the move-count cap's own
    /// exemptions below. `None` at the leaf call from `alpha_beta_with_buffer` (the main search
    /// doesn't track this at all, so there's nothing real to hand down at that boundary -- a
    /// deliberate scope limit, not an oversight: see `search.md`'s design section),
    /// `Some(mv.to())` at every recursive call quiescence makes into itself, since that move's
    /// own destination *is* "the previous move" for whatever node it leads to.
    fn quiescence(&mut self, board: &Board, ply: u32, alpha: i32, beta: i32, prev_to_sq: Option<Square>) -> i32 {
        self.quiescence_from(board, ply, alpha, beta, prev_to_sq, None)
    }

    /// `quiescence`, given this node's own evaluation when the caller already has it: the main
    /// search hands its leaf (and a razored node) to quiescence as the same board at the same ply,
    /// so its evaluation is exactly the stand pat, and recomputing it would only cost time (a
    /// trained network's evaluation is far dearer than the PeSTO one). `None` evaluates.
    /// Read only when not in check, where the stand pat exists, as the caller's own value does.
    fn quiescence_from(
        &mut self, board: &Board, ply: u32, mut alpha: i32, beta: i32, prev_to_sq: Option<Square>,
        evaluation: Option<i32>,
    ) -> i32 {
        // Same two-tier stop mechanism as `alpha_beta_with_buffer`, and for the same reason:
        // quiescence shares `self.nodes`/`self.deadline`/`self.stopped`/`self.stop_signal` with
        // the main search, since a pathological forced-capture chain here needs stopping just
        // as much as a runaway main-search iteration does. See that function's own comments for
        // why this is two checks, not one, and for why `stop_signal` is checked first.
        if self.stopped {
            return alpha;
        }

        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply);
        #[cfg(feature = "node-stats")]
        crate::node_stats::quiescence_node();

        if self.nodes.is_multiple_of(CHECK_INTERVAL) {
            if self.stop_signal.load(Ordering::Relaxed) {
                self.stopped = true;
                return alpha;
            }
            if self.is_out_of_time() {
                self.stopped = true;
                self.stop_signal.store(true, Ordering::Relaxed);
                return alpha;
            }
        }

        // Same repetition check as the main search, checked ahead of `MAX_PLY` for the same
        // reason -- see `is_repetition`'s own doc comment. Real here too, not just for
        // consistency: quiescence searches quiet check-evasions when in check (see below), so
        // a genuine repeating cycle -- a perpetual check, most plausibly -- can occur entirely
        // within quiescence's own horizon, made of moves the main search never even reaches.
        if self.is_repetition(board) {
            return 0;
        }

        // Same reasoning as `is_repetition` just above -- see `is_insufficient_material`'s
        // own doc comment for why this can be checked this early, unconditionally, unlike
        // `is_fifty_move_draw`.
        if is_insufficient_material(board) {
            return 0;
        }

        // Same tablebase check as `alpha_beta_with_buffer`, and genuinely useful here, not just
        // for consistency: quiescence exists specifically to chase forced capture sequences,
        // which are exactly the kind of position most likely to have just zeroed the halfmove
        // clock (`should_probe`'s own precondition, inside `SyzygyTablebase::probe`) and dropped
        // to a low enough piece count to be covered by a loaded table. No `ply > 0` guard here,
        // unlike that function's own version of this check -- quiescence is never the literal
        // search root (see this function's own doc comment: it's only ever entered from inside
        // `alpha_beta_with_buffer`, already at `ply >= 1`), so there's no equivalent bestmove-
        // 0000 risk to guard against.
        if let Some(tablebase) = self.tablebase
            && let Some(result) = tablebase.probe(board)
        {
            let score = match result {
                TbResult::Win => tb_win_at(ply),
                TbResult::Draw => 0,
                TbResult::Loss => tb_loss_at(ply),
            };
            // Cached at MAX_PLY, not this function's own usual sentinel depth of 0 (see
            // this function's own doc comment) -- same reasoning as
            // alpha_beta_with_buffer's identical store: a resolved WDL fact never becomes
            // stale as remaining depth changes, unlike an ordinary quiescence score, so it
            // is stored as trustworthy at any remaining depth, not just "at least as deep
            // as a fixed-0 quiescence call always is anyway."
            self.tt.store(board.hash(), tablebase_tt_move(), MAX_PLY, score_to_tt(score, ply), Bound::Exact);
            return score;
        }

        // Checked here too, ahead of `MAX_PLY` -- unlike `alpha_beta_with_buffer`, `ply` here
        // is genuinely reachable past `MAX_PLY` (that's the whole point of the ceiling), so
        // without this, a position that's both past `MAX_PLY` and a fifty-move draw would
        // wrongly fall through to `evaluate`'s plain material score instead. Safe to check
        // unconditionally-but-for-`!in_check` this early, before move generation, only because
        // checkmate is impossible without being in check -- there's no decisive result this
        // could ever wrongly override here. The in-check case still needs its own check at the
        // very end, once checkmate has actually been ruled out by move generation.
        let color = board.side_to_move();
        let in_check = is_in_check(board, color);
        if !in_check && is_fifty_move_draw(board) {
            return 0;
        }

        if ply >= MAX_PLY {
            return self.evaluate(board, ply);
        }

        // Transposition-table lookup, same shared table `alpha_beta_with_buffer` uses. No
        // depth-sufficiency check here the way that function has one: quiescence's own
        // "remaining depth" is always the fixed sentinel `0` (see the `store` call below), and
        // every entry's `depth` is a `u32`, so "at least as deep as 0" already holds for
        // literally every entry -- not worth spelling out as a runtime comparison that could
        // only ever be true. Kept as a separate `tt_entry`/`tt_move` pair, same two-step style
        // `alpha_beta_with_buffer` already uses, rather than discarding the entry once it fails
        // to produce an outright cutoff: even a non-cutoff entry's own stored best move is still
        // real ordering evidence below, worth keeping around for that.
        let tt_entry = self.tt.probe(board.hash());
        if let Some(entry) = tt_entry {
            let score = score_from_tt(entry.score(), ply);
            match entry.bound() {
                Bound::Exact => return score,
                Bound::Lower if score >= beta => return score,
                Bound::Upper if score <= alpha => return score,
                _ => {}
            }
        }
        let tt_move = tt_entry.map(|entry| entry.best_move());
        // Saved before `alpha` gets its first possible mutation below (the stand-pat update) --
        // same reasoning as `alpha_beta_with_buffer`'s own `original_alpha`.
        let original_alpha = alpha;

        // Stand pat only applies outside of check: the side to move can always decline every
        // capture, so it's a real, already-computed candidate score there, not a placeholder
        // needing to be proven safe. In check, there's no such fallback -- `i32::MIN` is
        // reinstated for the same reason the main search needs it, since a legal evasion might
        // not exist at all. Kept as its own `Option<i32>` (not folded straight into
        // `best_score`) so delta pruning below can keep reading the position's own *static*
        // value even after `best_score` itself has since improved past it.
        // Corrected (`corr`) as the main search's evaluation is; one handed down already is.
        let stand_pat = if in_check { None } else { Some(evaluation.unwrap_or_else(|| self.evaluate(board, ply) + self.correction(board))) };
        let mut best_score = match stand_pat {
            Some(stand_pat) => {
                if stand_pat >= beta {
                    // Fail-soft cutoff before a single move is even generated: the position is
                    // already good enough on its own that no capture needs trying.
                    return Self::blended(stand_pat, beta);
                }
                alpha = alpha.max(stand_pat);
                stand_pat
            }
            None => i32::MIN,
        };

        // Outside of check only captures and promotions are ever searched, so only they are
        // generated; in check every evasion is a candidate, quiet or not. The pick below breaks
        // ties by the move itself (`try_best_noisy_move`), so leaving the quiets out of the buffer
        // changes no pick and the tree is the same as with full generation.
        let start = self.moves.len();
        if in_check {
            pseudo_legal_moves(board, color, &mut self.moves);
        } else {
            noisy_moves(board, color, &mut self.moves);
        }
        let end = self.moves.len();

        let mut scratch = *board;
        let legality = Legality::new(board, color);
        let mut found_legal_move = false;
        // `None` whenever `best_score` never actually came from a searched move -- the in-check
        // placeholder never overwritten (no legal evasion), or the stand-pat value outside of
        // check never beaten by any capture. Both are real, valid results with nothing to
        // record as a "best move," which is exactly why the store below only fires when this
        // is `Some`.
        let mut best_move: Option<Move> = None;
        // How many moves have actually been made (legal, past every earlier pruning check) at
        // *this* node so far -- read by the move-count cap above, incremented alongside
        // `found_legal_move` below for *every* move that reaches that point, promotions and
        // recaptures included. Matches Stockfish's own `moveCount`: it's a plain count of moves
        // tried at this node, not filtered down to just the ones the cap could apply to -- an
        // earlier promotion still counts toward making the cap trip sooner for a later ordinary
        // capture, even though the promotion itself was exempt from the cap.
        let mut noisy_moves_tried: u32 = 0;
        for i in start..end {
            // Outside of check, only noisy moves are worth searching, best-scoring first: the
            // same lazy incremental `try_best_noisy_move` stage the main search's own priority
            // chain uses (MVV-LVA + promotions), reused as-is rather than reimplemented. A real
            // capture or promotion always scores strictly positive under `move_score` (the
            // victim's `10x` weight alone always exceeds any attacker's own value), so "no
            // noisy move found among `moves[i..end]`" and "everything left is quiet" are the
            // same fact -- and once that's true it stays true for every later `i` too, so the
            // loop stops outright instead of continuing to scan for something no longer there.
            // In check, every legal move is a mandatory candidate evasion, quiet or not.
            //
            // The TT-suggested move, if any, gets first priority ahead of even MVV-LVA -- same
            // reasoning as `alpha_beta_with_buffer`'s own identical priority, and the same reuse
            // of `try_killer` itself (see that function's own call site there): "find this one
            // candidate in the remaining buffer and swap it to the front" doesn't care which
            // table supplied the candidate. Gated to `i == start` for the same reason as there
            // -- once swapped into place, no later iteration's scan over `moves[i..end]` can
            // find it again. Outside of check, only attempted when the TT move is itself noisy
            // (`!is_quiet`): `self.tt` is shared with the main search, so an entry probed here
            // can easily carry a *quiet* best move stored from a completely different context --
            // searching that would break the one invariant `try_best_noisy_move`'s own
            // stop-the-loop behavior depends on (every candidate outside of check is noisy), so
            // a quiet TT move is simply ignored here rather than tried. In check, every move is
            // already a mandatory candidate regardless of type, so no such gating is needed.
            if in_check {
                if i == start {
                    try_killer(&mut self.moves, i, end, tt_move);
                }
            } else {
                let tt_move_is_noisy = tt_move.is_some_and(|mv| !is_quiet(board, mv));
                if !(i == start && tt_move_is_noisy && try_killer(&mut self.moves, i, end, tt_move))
                    && !try_best_noisy_move(&mut self.moves, i, end, board, move_score)
                {
                    break;
                }
            }
            let mv = self.moves[i];

            // Move-count cap: once `QUIESCENCE_MOVE_LIMIT` ordinary captures have already been
            // tried at this node without a cutoff, no further one is searched -- regardless of
            // how many more individually-SEE-safe captures remain in `moves[i..end]`. Mirrors
            // Stockfish's own nesting rather than one flat `&&` chain: the outer `if` is the
            // exemption gate, all four of Stockfish's own (`is_loss`/promotion/recapture/
            // `givesCheck`) now real, the cap comparison itself a plain one-liner once inside it.
            // `!is_tb_loss(best_score)` added alongside `is_loss` for the same reason every
            // other pruning guard in this file needed it: `TB_WIN_SCORE`'s band sits below
            // `is_loss`'s own threshold on purpose, so a TB-scale `best_score` -- already facing
            // a proven tablebase loss, exactly when a further capture might still be the one
            // that helps -- would otherwise slip past this exemption gate unrecognized.
            #[allow(clippy::collapsible_if)]
            if !in_check
                && !is_loss(best_score)
                && !is_tb_loss(best_score)
                && mv.promotion().is_none()
                && !is_recapture(mv, prev_to_sq)
                && !gives_check(board, mv)
            {
                if noisy_moves_tried >= QUIESCENCE_MOVE_LIMIT {
                    // `break`, not `continue`: `try_best_noisy_move`'s own best-first ordering
                    // means every move from here on is at least as low-priority as the one that
                    // just tripped this, so there's nothing left worth trying.
                    break;
                }
            }

            // Delta pruning: checked before SEE below since it's the cheaper test (no attacker
            // enumeration, just a couple of additions against the already-computed `stand_pat`)
            // -- even winning the captured piece outright, plus a safety margin, still couldn't
            // raise the score to `alpha`, so this capture isn't worth making at all. Never for a
            // promotion: the margin below only estimates an ordinary capture's own victim, and a
            // promotion's own material swing is large enough that the same estimate would risk
            // wrongly discarding a move that's actually good (the same exclusion SEE's own
            // `gain[0]` accounting makes, for the same reason). `captured_piece` is guaranteed
            // `Some` here -- `mv` already passed `try_best_noisy_move` and isn't a promotion, so
            // it must be an ordinary capture.
            if !in_check && mv.promotion().is_none() {
                let victim = captured_piece(board, mv).expect("quiescence: noisy non-promotion move has no victim");
                let static_eval = stand_pat.expect("quiescence: stand pat exists whenever not in check");
                let best_case =
                    static_eval + self.in_evaluator_pawns(piece_value(victim.piece_type()) + self.parameters.delta_margin);
                if best_case <= alpha {
                    continue;
                }
            }

            // Static exchange evaluation: a capture that loses more than `see_margin` is never
            // worth searching here, regardless of how it ranks by MVV-LVA above -- ordering
            // alone only changes *when* a bad capture gets tried, this is what stops it from
            // being tried at all. `-see_margin`, not `0` -- see that field's own doc comment
            // for why a small negative allowance, not a break-even requirement, is the real
            // Stockfish-motivated choice here. A pure promotion (no victim) always scores `>= 0`
            // under `see_ge` (any promoted piece is worth more than the pawn it replaces), so
            // this never filters one out regardless of the threshold's exact value. Only outside
            // of check, same reasoning as the noisy-move filter above: every legal response to
            // check stays mandatory regardless of material cost.
            if !in_check && !see_ge(board, mv, -self.parameters.see_margin) {
                continue;
            }

            if legality.is_legal(board, mv) {
                let undo = scratch.make_move(mv);
                self.tt.prefetch(scratch.hash());
                found_legal_move = true;
                noisy_moves_tried += 1;
                self.update_accumulators(ply, board, &scratch);
                // Same push-before-recurse, pop-after pattern as the main search's own loop.
                self.ancestors.push(board.hash());
                // `Some(mv.to())`: whichever child node this leads to, `mv` is now *its* own
                // immediately preceding move, exactly the fact `is_recapture` needs at that
                // next level down.
                let child_score = -self.quiescence(&scratch, ply + 1, -beta, -alpha, Some(mv.to()));
                self.ancestors.pop();
                // Strictly-greater, not `.max()`, for the same reason `alpha_beta_with_buffer`
                // uses it: an explicit moment where `best_score` just improved, to hook
                // `best_move`'s own update onto.
                if child_score > best_score {
                    best_score = child_score;
                    best_move = Some(mv);
                }
                alpha = alpha.max(best_score);
                scratch.unmake_move(mv, undo);
            }

            if alpha >= beta {
                break;
            }
        }

        self.moves.truncate(start);

        if in_check && !found_legal_move {
            // Same formula as the main search's own mate scoring -- see `MATE_SCORE`'s doc
            // comment for why it's `ply`-based rather than tied to any particular search's
            // requested depth.
            return -(MATE_SCORE - ply as i32);
        }
        // Only ever reached via the `in_check` path now -- `!in_check` already returned
        // earlier above if this were going to be true, so this is what makes the fifty-move
        // rule apply to check-evasion lines too, once checkmate has actually been ruled out.
        if is_fifty_move_draw(board) {
            return 0;
        }
        // Sentinel depth `0`, per this function's own doc comment -- and only when a move
        // actually produced `best_score` (see `best_move`'s own doc comment for the two cases
        // where it's `None`): `Entry` has no slot for "no move," so a value reached by
        // declining every capture goes uncached here rather than stretching the schema to fit
        // it. A missed caching opportunity, not a correctness gap -- the position is just
        // searched again in full next time it's reached.
        if let Some(mv) = best_move {
            self.tt.store(board.hash(), mv, 0, score_to_tt(best_score, ply), classify_bound(best_score, original_alpha, beta));
        }
        best_score
    }
}

/// The time limit of one `go` command, as read from its arguments by `time_limit`.
///
/// No safety margin is ever subtracted from either variant. When competing, the GUI's own move
/// overhead (lichess-bot's) already hands over a reduced clock; locally, `MAX_CLOCK_FRACTION`
/// leaves half the clock unspent after every move, far more than the stop-to-`bestmove` latency.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimeLimit {
    /// `go movetime`: a fixed target, used as given. With no cap, it overruns by the stop latency
    /// (a few milliseconds), on the assumption that nothing enforces `movetime` strictly.
    Fixed { target_ms: u64 },
    /// The side to move's clock and increment (`0` when absent), and `movestogo`: `None` under
    /// Fischer, `Some` under a repeating control such as CCRL's 40/15, counting down to `1` before
    /// each refill. `single_move`: the position has one legal move, so the target is capped at
    /// `SINGLE_MOVE_CAP_MS` (`single_move_limit`). `previous_score`: the final score of this
    /// engine's previous search in the game, from its own side, for the falling-score factor
    /// (`falling_score_factor`); `None` on its first move.
    Clock { time_ms: u64, inc_ms: u64, moves_to_go: Option<u32>, single_move: bool, previous_score: Option<i32> },
}

impl TimeLimit {
    /// This limit with `previous_score` as the previous move's score, if it is a clock.
    pub fn with_previous_score(self, previous_score: Option<i32>) -> TimeLimit {
        match self {
            TimeLimit::Clock { time_ms, inc_ms, moves_to_go, single_move, .. } => {
                TimeLimit::Clock { time_ms, inc_ms, moves_to_go, single_move, previous_score }
            }
            fixed => fixed,
        }
    }
}

/// The most time a move with a single legal move gets, from Stockfish's `search.cpp` (fetched
/// 2026-10-03): `if (rootMoves.size() == 1) totalTime = std::min(500.0, totalTime);`, "for a better
/// viewer experience". It still searches, briefly, to report a score and a ponder move.
pub const SINGLE_MOVE_CAP_MS: u64 = 500;

/// `limit` for a move played in a game, capped at `SINGLE_MOVE_CAP_MS` when `board` has exactly
/// one legal move: in a game only the move matters, and a forced move has one. Under a clock the
/// target is capped (`single_move`); `movetime` becomes at most the cap; no time limit (`go depth`,
/// the arena's fixed depth) becomes the cap, alongside the depth, whichever ends first.
///
/// Only for game moves: never for self-play or replay, whose depth-9 score is a training label and
/// must not be cut short (they call `choose_best_move` directly, so they never reach this), and never
/// for `go infinite`, which runs until told to stop. A ponder search is capped too, but ignores its
/// deadline until `ponderhit` (`PonderState`).
pub fn single_move_limit(board: &Board, limit: Option<TimeLimit>) -> Option<TimeLimit> {
    if crate::movegen::legal::legal_moves(board, board.side_to_move()).len() != 1 {
        return limit;
    }
    Some(match limit {
        Some(TimeLimit::Clock { time_ms, inc_ms, moves_to_go, previous_score, .. }) => {
            TimeLimit::Clock { time_ms, inc_ms, moves_to_go, single_move: true, previous_score }
        }
        Some(TimeLimit::Fixed { target_ms }) => TimeLimit::Fixed { target_ms: target_ms.min(SINGLE_MOVE_CAP_MS) },
        None => TimeLimit::Fixed { target_ms: SINGLE_MOVE_CAP_MS },
    })
}

/// The parameters of the clock target:
/// `target = min(((alpha + 1/M) * T + beta * I) * (1 + r * C) * F, MAX_CLOCK_FRACTION * T)`,
/// with `T` the clock, `I` the increment, `M` the moves to go (`1/M = 0` under Fischer), `C` the
/// search instability in `[0, 1]` (see `TimeManager`) and `F` the falling-score factor
/// (`falling_score_factor`). Iterative deepening starts no new depth once `s * target` has elapsed,
/// and aborts the running one at `target`.
///
/// Settled by reasonable magnitudes, not tuned, as every search parameter (decided 2026-10-12): a
/// change of a few Elo is below what a duel can resolve, and Lichess games showed sensible use of
/// the clock. One set covers Fischer and repeating controls alike.
pub(crate) struct TimeParameters {
    /// 0.05: a twentieth of the clock per stable move, so a long game still has most of its clock
    /// after its first moves; at 3+2, 9 s of a 10 s target.
    alpha: f64,
    /// 0.5: half the increment. Below 1, so a stable move spends less than the increment and a
    /// Fischer clock settles above zero instead of draining; instability, through `r`, is what
    /// spends beyond it.
    beta: f64,
    /// 2.0: a best move changing at every depth (`C = 1`) triples the target, the moves where more
    /// depth most often changes the choice.
    r: f64,
    /// 0.5: a depth costs about as much as all the shallower ones together, so one started past
    /// half the target would likely be aborted at `target` and its time wasted.
    s: f64,
}

pub(crate) const TIME_PARAMETERS: TimeParameters = TimeParameters { alpha: 0.05, beta: 0.5, r: 2.0, s: 0.5 };

/// The cap on any one move, as a fraction of the remaining clock. Under Fischer it binds only in
/// deep time trouble (even at full instability, only when `T < 4.3 * I` with `F = 1`), and a clock hitting it every move still settles at `2 * I` instead of
/// draining. Near a refill (`M` small) it is what decides, letting the last move before the control
/// spend half the clock, the rest carrying over.
const MAX_CLOCK_FRACTION: f64 = 0.5;

/// The `Move Overhead` option's default, in milliseconds: the time a move is assumed to lose outside
/// the search's own timing, kept back from every clock and `movetime` (`time_limit`). It covers the
/// search noticing its deadline (every `CHECK_INTERVAL` nodes, about a millisecond with a network at
/// the 841,150 nodes/s per thread benched 2026-11-09), unwinding and writing `bestmove`, the GUI's
/// own handling, and a core briefly taken away when a tester plays several games on one machine.
/// Stockfish's default is 10; 20 leaves room above it at no measurable cost (about 0.8 s set aside at
/// the start of a 40/15 control, 40 ms under Fischer). A fixed amount, not a fraction of the clock,
/// since lag does not shrink with the clock. It matters at default settings: rating-list testers set
/// no option. lichess-bot keeps a margin of its own on top.
pub const DEFAULT_MOVE_OVERHEAD_MS: u64 = 20;

/// `movetime`'s fraction of the target past which no new depth starts: its own `s`, at the same
/// value for the same reason.
const FIXED_STOP_FRACTION: f64 = 0.5;

/// The clock target of one move, in milliseconds -- the one place the formula of `TimeParameters`
/// lives. `falling` is `falling_score_factor`'s `F`, multiplying the target before the cap. A
/// `movestogo 0` is not valid UCI; it is read as `M = 1` rather than dividing by zero.
fn clock_target_ms(
    time_ms: u64, inc_ms: u64, moves_to_go: Option<u32>, instability: f64, falling: f64, p: &TimeParameters,
) -> u64 {
    let time = time_ms as f64;
    let inc = inc_ms as f64;
    let per_move = match moves_to_go {
        Some(moves) => 1.0 / f64::from(moves.max(1)),
        None => 0.0,
    };
    let target = ((p.alpha + per_move) * time + p.beta * inc) * (1.0 + p.r * instability) * falling;
    target.min(MAX_CLOCK_FRACTION * time).round() as u64
}

/// The score drop, in `piece_value`'s centipawns, that moves the falling-score factor by 1: a drop
/// of a third of it is about 17% more time, a rise of 40% of it the floor. A round default, scaled
/// to the evaluator's own pawn (`falling_score_factor`), so a real pawn buys the same time under
/// every evaluation; judged with the factor as a whole by one duel.
const FALLING_SCORE_SCALE_CP: i32 = 200;

/// The bounds of the falling-score factor: Stockfish's `std::clamp(fallingEval, 0.576, 1.728)`
/// (`search.cpp`, fetched 2026-10-08), rounded.
const FALLING_SCORE_MIN: f64 = 0.6;
const FALLING_SCORE_MAX: f64 = 1.7;

/// `F`, the falling-score factor of the clock target: above 1 when `current`, the score of the
/// depth just completed, is below `previous`, the engine's previous move's final score (the search
/// is finding the position worse than the game thought), below 1 when above it. Stockfish's
/// `fallingEval`, with one reference instead of two: the previous move's score, without its ring of
/// the last moves' scores. 1 with no previous score, or when either score is a mate or tablebase
/// score, whose bands are not evaluations. `pawn_percent` is the evaluator's pawn against
/// `piece_value`'s.
fn falling_score_factor(previous: Option<i32>, current: i32, pawn_percent: i32) -> f64 {
    let Some(previous) = previous else {
        return 1.0;
    };
    let decisive = TB_WIN_SCORE - MAX_PLY as i32;
    if previous.abs() >= decisive || current.abs() >= decisive {
        return 1.0;
    }
    let scale = f64::from(FALLING_SCORE_SCALE_CP * pawn_percent) / 100.0;
    (1.0 + f64::from(previous - current) / scale).clamp(FALLING_SCORE_MIN, FALLING_SCORE_MAX)
}

/// A ponder search's state, shared between `uci.rs` and thread 0. A ponder search is the game
/// move's own search from `go ponder` on, with the real clock, only kept from stopping while
/// pondering; on `ponderhit` it goes on from the depth it reached, the time already spent counting
/// as spent. Stockfish's handling (fetched 2026-10-03): `ponderhit` only clears
/// `main_manager()->ponder`; when the time is up while pondering, `stopOnPonderhit` is set instead
/// of stopping, and the search stops on `ponderhit`.
pub struct PonderState {
    pondering: AtomicBool,
    stop_on_ponderhit: AtomicBool,
}

impl PonderState {
    /// A ponder search's state as `go ponder` starts it: pondering, the time not yet up. No
    /// `Default`: a state that is already pondering is no neutral default.
    #[allow(clippy::new_without_default)]
    pub fn new() -> PonderState {
        PonderState { pondering: AtomicBool::new(true), stop_on_ponderhit: AtomicBool::new(false) }
    }

    /// `ponderhit`: from now on, the search keeps to its clock.
    pub fn ponderhit(&self) {
        self.pondering.store(false, Ordering::SeqCst);
    }

    fn is_pondering(&self) -> bool {
        self.pondering.load(Ordering::SeqCst)
    }

    /// Called when the time manager would stop: while pondering, records that the search must stop
    /// on `ponderhit` and reports `true`, to go on searching.
    fn defer_stop(&self) -> bool {
        if !self.is_pondering() {
            return false;
        }
        self.stop_on_ponderhit.store(true, Ordering::SeqCst);
        true
    }

    /// Whether the time ran out while pondering and `ponderhit` has arrived since.
    fn stops_now(&self) -> bool {
        !self.is_pondering() && self.stop_on_ponderhit.load(Ordering::SeqCst)
    }
}

/// Whether thread 0 may start another depth: the time manager's `should_start_next`, except that a
/// ponder search goes on while pondering (`defer_stop`), and stops at once on `ponderhit` if the
/// time ran out meanwhile.
fn may_start_next_depth(time_manager: &TimeManager, ponder: Option<&PonderState>) -> bool {
    if ponder.is_some_and(|ponder| ponder.stops_now()) {
        return false;
    }
    if time_manager.should_start_next() {
        return true;
    }
    ponder.is_some_and(|ponder| ponder.defer_stop())
}

/// Thread 0's time decisions across one iterative deepening run: when to start a new depth, and the
/// deadline of the one it starts.
///
/// The target is recomputed after every completed depth, from the search instability `C`: the
/// discounted number of best move changes, `C[0] = C[1] = 1` (nothing is known yet), then
/// `C[k + 1] = 0.5 * C[k] + 0.5 * (move[k] != move[k + 1])`, which stays in `[0, 1]`. A depth runs
/// against the target fixed when it started, so the deadline inside the tree never moves. A stable
/// depth can shrink the target below the time already elapsed, which just stops at once.
///
/// Under a clock it is also recomputed from the falling-score factor `F` (`falling_score_factor`),
/// from the score of the depth just completed against the previous move's; 1 before any depth.
struct TimeManager {
    started: Instant,
    limit: Option<TimeLimit>,
    instability: f64,
    last_move: Option<Move>,
    /// The evaluator's pawn against `piece_value`'s, in percent, which scales `F`.
    pawn_percent: i32,
    falling: f64,
}

impl TimeManager {
    fn new(limit: Option<TimeLimit>, started: Instant, pawn_percent: i32) -> TimeManager {
        TimeManager { started, limit, instability: 1.0, last_move: None, pawn_percent, falling: 1.0 }
    }

    /// Updates the instability with the best move of a depth that just completed, and `F` with its
    /// score.
    fn record_iteration(&mut self, best_move: Move, score: i32) {
        self.instability = match self.last_move {
            None => 1.0,
            Some(last_move) => 0.5 * self.instability + 0.5 * f64::from(u8::from(last_move != best_move)),
        };
        self.last_move = Some(best_move);
        if let Some(TimeLimit::Clock { previous_score, .. }) = self.limit {
            self.falling = falling_score_factor(previous_score, score, self.pawn_percent);
        }
    }

    /// `None` when there is no time limit at all.
    fn target(&self) -> Option<Duration> {
        match self.limit? {
            TimeLimit::Fixed { target_ms } => Some(Duration::from_millis(target_ms)),
            TimeLimit::Clock { time_ms, inc_ms, moves_to_go, single_move, .. } => {
                let target =
                    clock_target_ms(time_ms, inc_ms, moves_to_go, self.instability, self.falling, &TIME_PARAMETERS);
                // After the formula and `MAX_CLOCK_FRACTION`, so the cap only ever lowers it.
                let target = if single_move { target.min(SINGLE_MOVE_CAP_MS) } else { target };
                Some(Duration::from_millis(target))
            }
        }
    }

    /// Whether a new depth may start: the elapsed time is still under `s * target`.
    fn should_start_next(&self) -> bool {
        let Some(target) = self.target() else {
            return true;
        };
        let stop_fraction = match self.limit {
            Some(TimeLimit::Clock { .. }) => TIME_PARAMETERS.s,
            _ => FIXED_STOP_FRACTION,
        };
        self.started.elapsed() < target.mul_f64(stop_fraction)
    }

    /// The instant the depth about to start must abort at.
    fn deadline(&self) -> Option<Instant> {
        self.target().map(|target| self.started + target)
    }
}

/// Reads the time limit of a `go` command. `None` means no time limit applies at all -- a bare `go`
/// with none of these fields set, or `infinite`. Real `infinite` support (search until `stop`, never
/// on a self-imposed clock) needs `go` to run on its own thread that stays responsive to an async
/// `stop` while searching -- `uci.rs`'s `go_search` does exactly that, so `None` here genuinely
/// means "no deadline, run until `stop` or `max_depth`": `alpha_beta_with_buffer`'s own
/// `CHECK_INTERVAL` poll checks the shared stop flag unconditionally, regardless of whether a
/// deadline was ever set at all. `movetime`, when present, takes precedence over `wtime`/`btime` --
/// not something the UCI spec actually defines (checked directly against its text: it says nothing
/// about which parameter wins when several are sent together in the same `go`), just our own
/// resolution of a genuine gap in it.
///
/// Otherwise the clock of `board.side_to_move()` applies, with its own increment. If that side's
/// own time field is missing entirely, there's nothing to plan from, so this falls back to `None`
/// too -- the same "no limit" behavior as a bare `go`.
///
/// `move_overhead_ms` (the `Move Overhead` option, `DEFAULT_MOVE_OVERHEAD_MS`) is kept back before
/// anything plans with the time, so a move never plans to spend it. When it takes all the time, the
/// target is 0: depth 1, which has no deadline, still runs and answers, and nothing deeper starts --
/// what `movetime 0` has always meant. No floor of 1 ms, which would let a fast depth 2 start. From the clock,
/// once per move it must still cover: `M + 2` times under `movestogo` (Stockfish's `2 + mtg`), one
/// for each move left before the control and two spare; twice under Fischer, where the increment
/// refills the clock and a stable move spends half of it. From `movetime` once, unlike Stockfish:
/// the search stops at exactly its target, so without it every move would run past `movetime` by
/// its unwinding and its `bestmove`.
#[allow(clippy::too_many_arguments)]
pub fn time_limit(
    board: &Board,
    wtime: Option<u64>,
    btime: Option<u64>,
    winc: Option<u64>,
    binc: Option<u64>,
    movestogo: Option<u32>,
    movetime: Option<u64>,
    infinite: bool,
    move_overhead_ms: u64,
) -> Option<TimeLimit> {
    if infinite {
        return None;
    }
    if let Some(movetime) = movetime {
        return Some(TimeLimit::Fixed { target_ms: movetime.saturating_sub(move_overhead_ms) });
    }

    let (my_time, my_inc) = match board.side_to_move() {
        Color::White => (wtime, winc.unwrap_or(0)),
        Color::Black => (btime, binc.unwrap_or(0)),
    };
    let my_time = my_time?;
    let reserves = movestogo.map_or(2, |moves| u64::from(moves.max(1)) + 2);
    Some(TimeLimit::Clock {
        time_ms: my_time.saturating_sub(move_overhead_ms.saturating_mul(reserves)),
        inc_ms: my_inc,
        moves_to_go: movestogo,
        single_move: false,
        previous_score: None,
    })
}

/// One depth's worth of root search: constructs a fresh `Search` and makes exactly one
/// top-level call to `alpha_beta_with_buffer`, then reads back which move produced the best
/// score at its own root (`ply == 0`) alongside the score and node count -- no separate
/// move-enumeration loop of its own. Root moves are generated and searched exactly once, via
/// the same fused pseudo-legal-generation-plus-legality-check pattern every other node already
/// uses, rather than filtering through `legal_moves` first (which would make/unmake every
/// candidate once there, then again here to actually search it -- exactly what `legal_moves`'s
/// own doc comment warns isn't fit for a search's hot path).
///
/// Returns the empty line specifically when the root has no legal moves at all
/// (checkmate/stalemate) -- `search.pv[0]` simply never gets written in that case, since no
/// candidate ever passes the legality check.
///
/// The fourth value, `stopped`, is `search.stopped` itself: whether this call actually ran to
/// completion or got cut short by `deadline`. It can't be inferred from `pv`/`score` alone --
/// an aborted search can still have recorded a nonempty `pv` and a plausible-looking `score`
/// from partway through, neither of which means anything once the search that produced them
/// didn't finish.
///
/// `ancestors` seeds the fresh `Search`'s own repetition-detection lineage with the real
/// game's ancestor positions (see `Search::ancestors`'s own doc comment) -- taken by value
/// since each depth of iterative deepening needs its own independent copy to push/pop
/// during its own recursion, not one shared across iterations.
///
/// The fifth value, `seldepth`, is `search.seldepth` -- see its own doc comment.
///
/// `killers` and `history` are both the opposite of `ancestors`: not cloned away, but moved in
/// and handed straight back out (as the sixth and seventh return values) so `choose_best_move`
/// can carry the *same*, still-accumulating tables into the next depth rather than starting
/// over. Handed back regardless of whether this call actually completed (`stopped`) -- unlike
/// `pv`/`score`, which summarize an entire subtree and are worthless if that subtree wasn't
/// finished, each individual killer/history entry records one cutoff that genuinely already
/// happened wherever `alpha_beta_with_buffer` reached it, so both stay real, local evidence
/// even inside an aborted iteration.
///
/// `tt` is threaded straight through to `new_search`, same as `killers`/`history` -- one real
/// table shared across every depth of iterative deepening, and (see `Search::tt`'s own doc
/// comment) across every worker thread too.
///
/// `tablebase`, unlike every parameter above, isn't handed to `new_search` at all -- that
/// constructor always defaults `Search::tablebase` to `None` (see its own doc comment for why:
/// dozens of tests build a `Search` directly through it and shouldn't all need to supply one).
/// This function sets the field directly on the `Search` `new_search` hands back, right after
/// construction, unconditionally -- `Engine` always holds a real `SyzygyTablebase` (empty,
/// probing nothing, until a `SyzygyPath` option loads real tables), so there is no `None` case
/// to thread through at this layer, unlike `Search::tablebase`'s own `Option`.
///
/// `previous_score` is the previous depth's own final score, `None` before one exists (depth
/// `1`) -- what centers this depth's aspiration window, once `depth >= ASPIRATION_MIN_DEPTH`
/// (see that constant's own doc comment for why shallower depths always use the full window
/// instead). The retry loop below reuses the *same* `Search` (and so the same `killers`/
/// `history`, still being built up) across every attempt at this one depth -- it's genuinely
/// the same position and depth being re-searched, not a new one, so nothing learned by a
/// failed attempt is stale for the very next one.
///
/// `thread_idx` identifies which worker thread this call belongs to -- `0` for the only thread
/// that exists today -- and only ever affects the very first window's width, via
/// `initial_aspiration_delta`; see that function's own doc comment for why threads need
/// different widths at all.
///
/// `stop_signal` is threaded straight through to `new_search`, same as `tt` -- see
/// `Search::stop_signal`'s own doc comment for the mechanism it's part of. `deadline` itself
/// only ever being `Some` for thread `0`'s own calls is what `choose_best_move` (this
/// function's only caller) enforces, not something checked here.
///
/// Test-only: the real callers search through `search_root_pondering`, which takes the ponder state.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn search_root(
    board: &Board, depth: u32, deadline: Option<Instant>, ancestors: Vec<u64>,
    killers: Vec<[Option<Move>; 2]>, history: [[i32; 64]; 12], continuation_history: Vec<i32>,
    tt: &TranspositionTable, tablebase: &SyzygyTablebase, evaluator: Option<&Network>,
    previous_score: Option<i32>, thread_idx: u32, stop_signal: &AtomicBool, excluded_root_moves: &[Move],
) -> RootResult {
    search_root_pondering(
        board, depth, deadline, ancestors, killers, history, continuation_history, vec![0; CORRECTION_HISTORY_LEN], tt,
        tablebase, evaluator, None, previous_score, thread_idx, stop_signal, excluded_root_moves, None,
    )
}

/// The parameters a search uses when its caller gives none: `evaluator`'s tuning
/// (`Network::tuning`, from the files beside its own) if there is one, else
/// `SearchParameters::DEFAULT`.
pub fn default_parameters_for(evaluator: Option<&Network>) -> SearchParameters {
    evaluator.map_or(SearchParameters::DEFAULT, |network| network.tuning().parameters())
}

/// The PeSTO evaluation of `board` once the pending captures are resolved: the search's own
/// quiescence with the PeSTO evaluation, a full window and no table, from the side to move.
/// What a network is compared with when its scale against PeSTO is measured: the static evaluation
/// of a position taken mid-exchange counts material about to be won back, which a network rightly
/// does not (2026-10-20: on positions of human games, the side to move 3 to 10 pawns behind by the
/// static count was mostly about to recapture, and every network read those positions as about
/// equal). `board` must not be in check, where quiescence has no stand pat.
pub fn pesto_quiescence(board: &Board) -> i32 {
    let tt = TranspositionTable::new(0);
    let stop_signal = AtomicBool::new(false);
    let mut search = new_search(
        0, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
    );
    search.set_evaluator(None, board);
    search.set_parameters(SearchParameters::DEFAULT);
    search.quiescence(board, 0, -INFINITY, INFINITY, None)
}

/// What one root search hands back: the principal variation, its score, the nodes, whether it was
/// stopped, the selective depth, then the killers, history, continuation history and correction
/// history the next iteration starts from.
type RootResult = (Vec<Move>, i32, u64, bool, u32, Vec<[Option<Move>; 2]>, [[i32; 64]; 12], Vec<i32>, Vec<i32>);

/// `search_root` for a search that may be a ponder search (`ponder`: see `PonderState`).
/// `parameters` replaces the evaluator's own (`default_parameters_for`) when given.
#[allow(clippy::too_many_arguments)]
fn search_root_pondering(
    board: &Board, depth: u32, deadline: Option<Instant>, ancestors: Vec<u64>,
    killers: Vec<[Option<Move>; 2]>, history: [[i32; 64]; 12], continuation_history: Vec<i32>,
    correction_history: Vec<i32>, tt: &TranspositionTable, tablebase: &SyzygyTablebase, evaluator: Option<&Network>,
    parameters: Option<SearchParameters>, previous_score: Option<i32>, thread_idx: u32, stop_signal: &AtomicBool,
    excluded_root_moves: &[Move], ponder: Option<&PonderState>,
) -> RootResult {
    let mut search = new_search(depth, deadline, ancestors, killers, history, continuation_history, tt, stop_signal);
    search.correction_history = correction_history;
    search.ponder = ponder;
    // Set directly on the field new_search itself always defaults to None -- see that
    // function's own doc comment for why a tablebase parameter isn't added there instead (it
    // would force every one of its many direct test call sites to supply one).
    search.tablebase = Some(tablebase);
    // Same reasoning, same mechanism -- `None` here is the PeSTO evaluation.
    search.set_evaluator(evaluator, board);
    search.set_parameters(parameters.unwrap_or_else(|| default_parameters_for(evaluator)));
    // Same reasoning as `tablebase` just above -- `new_search` always defaults this to empty
    // (`MultiPV == 1`'s exact behavior), set directly here instead of threading it through that
    // constructor, which would force every one of its own direct test call sites to supply one.
    search.excluded_root_moves = excluded_root_moves.to_vec();

    // The very first window tried at this depth. Anything shallower than
    // `ASPIRATION_MIN_DEPTH`, or with no previous score to center on yet, starts at the full
    // sentinel range instead -- since no real score can ever reach `INFINITY`, the retry loop
    // below can never actually fail against it, so it always succeeds on its first attempt.
    // That's not special-cased separately; it falls straight out of the same loop.
    let initial_delta = initial_aspiration_delta(search.parameters.aspiration_delta, thread_idx);
    let (mut alpha, mut beta, mut delta) = match previous_score {
        Some(score) if depth >= ASPIRATION_MIN_DEPTH => {
            ((score - initial_delta).max(-INFINITY), score.saturating_add(initial_delta), initial_delta)
        }
        _ => (-INFINITY, INFINITY, initial_delta),
    };

    // Widens and retries the same depth on a fail-low/fail-high, exactly as long as it takes
    // to land a score strictly inside its own window (proof that it's exact, not just a
    // bound). Guaranteed to terminate without an explicit retry cap: doubling `delta` from
    // the first window's half-width exceeds any real achievable score (bounded by `MATE_SCORE`)
    // within a couple dozen retries at most, at which point `alpha`/`beta` are already
    // clamped to the full sentinel range and no further failure is possible.
    let score = loop {
        let attempt = search.alpha_beta_with_buffer(board, 0, depth, alpha, beta);
        // A deadline that trips mid-retry needs its own exit, checked before the fail-low/
        // fail-high comparisons below: once `search.stopped` is set, every subsequent call
        // hits `alpha_beta_with_buffer`'s own unconditional top-of-function check and returns
        // its `alpha` parameter exactly, unconditionally -- which satisfies `attempt <= alpha`
        // forever (it's equality, not a real fail-low), so without this check the loop would
        // keep "retrying" indefinitely, doubling `delta` without bound until it overflows.
        // The exact value broken here is never actually used regardless: `choose_best_move`
        // already discards this depth's score/pv entirely once it sees `stopped`.
        if search.stopped {
            break attempt;
        }
        if attempt <= alpha {
            // Fail low: the true score is at most `attempt`, so the window's own floor was
            // set too high. `beta` shrinks to the old `alpha` (the true score is now known to
            // be below where `beta` used to be too), and `alpha` widens downward from the
            // actual failed value, not just from its own old position.
            beta = alpha;
            alpha = (attempt - delta).max(-INFINITY);
        } else if attempt >= beta {
            // Fail high, the mirror image: `alpha` rises to (at most) the old `beta`, and
            // `beta` widens upward from the actual failed value.
            alpha = (beta - delta).max(alpha);
            // Saturating, as `INFINITY` is `i32::MAX`: a plain `+` would overflow rather than clamp.
            beta = attempt.saturating_add(delta);
        } else {
            // Landed strictly inside the window -- an exact score, not just a bound. Done.
            break attempt;
        }
        delta *= 2;
    };

    (
        search.pv.line(0).to_vec(), score, search.nodes, search.stopped, search.seldepth, search.killers,
        search.history, search.continuation_history, search.correction_history,
    )
}

/// One completed iterative-deepening iteration's worth of reporting -- what UCI's `info depth
/// <N> score cp <X> nodes <total> pv <moves>` line needs (see `choose_best_move`'s own doc
/// comment for what `nodes` actually accumulates). `pv` stops at the main search's own
/// horizon -- it never includes `quiescence`'s own continuation (see `Search::pv`'s doc
/// comment for why) -- and is always nonempty here: `choose_best_move` only ever pushes a
/// report for a depth that actually found a real move.
pub struct IterationReport {
    pub depth: u32,
    /// The deepest ply actually visited anywhere in this depth's own search tree -- UCI's
    /// `seldepth`. See `Search::seldepth`'s own doc comment; always `>= depth`, since
    /// `quiescence` only ever extends further, never stops short of it.
    pub seldepth: u32,
    pub score: i32,
    pub nodes: u64,
    pub pv: Vec<Move>,
    /// Milliseconds elapsed since `choose_best_move` itself started, not just this one
    /// depth's own search -- cumulative, the same way `nodes` already is, so the two stay
    /// consistent with each other (both "totals for the search so far", not "just this
    /// iteration").
    pub time_ms: u64,
}

/// Iterative deepening: search depth `1`, then `2`, then `3`, ..., up to `max_depth`, each a
/// complete, independent call to `search_root` (never resumed or continued from the last).
/// Returns one `IterationReport` per completed depth plus whichever move the *last completed*
/// one chose -- never a result from a depth that was interrupted partway through.
///
/// `limit` is the time limit of this whole call -- `None` means no time limit at all (every depth
/// up to `max_depth` runs unconditionally). When `Some`, a `TimeManager` gates every depth after
/// the first, twice: before starting a new depth, the elapsed time must still be under
/// `s * target` (a headroom margin, not a plain "not yet over target" check -- since the next depth
/// is typically far more expensive than the current one, starting one that then has to be aborted
/// wastes everything it computed). Each depth actually searched beyond the first is given a hard
/// deadline of `target` from this function's own start -- if it trips, `search_root`'s `stopped`
/// comes back `true` and that whole iteration is discarded outright (never pushed as an
/// `IterationReport`, never allowed to overwrite `best_move`): an interrupted search's score and
/// move are exactly as unreliable as each other, for the same reason, so neither is trusted alone.
/// The target itself is recomputed after every completed depth, from how often the best move has
/// changed (see `TimeManager`).
///
/// Depth `1` is the one exception to both checks -- it always runs with no deadline at all,
/// regardless of `limit`. It's cheap enough that it should never actually need interrupting,
/// and guaranteeing it completes is what keeps a real legal move available even under a
/// limit too small for anything else: without this, an exhausted limit could leave
/// `best_move` at `None` even though the root has legal moves, which `uci.rs` would then
/// misreport as the UCI null move (`bestmove 0000`) -- a value specifically meant for "no
/// legal moves exist" (checkmate/stalemate), not "never got the chance to look."
///
/// `IterationReport::nodes` is a running *cumulative* total across every depth completed so
/// far, not just that one depth's own count -- the standard convention for what a reported
/// node count means ("total search effort this command"), and cheap to maintain since it's
/// just an accumulator this loop already has everything it needs for.
///
/// Stops early, rather than looping all the way to `max_depth`, the moment a depth reports an
/// empty `pv` -- that only happens when the root itself has no legal moves (checkmate/
/// stalemate), a depth-independent fact no further iteration would ever change.
///
/// `ancestors` is the real game's own move history so far (from UCI's `position ... moves
/// ...`), oldest first, never including `board` itself -- see `Search::ancestors`'s own doc
/// comment. Cloned fresh into each depth's own `search_root` call, since every iteration
/// needs an independent copy to push/pop during its own recursion.
///
/// `killers`, `history`, and `continuation_history`, by contrast, all start empty here
/// (`killers`: `MAX_PLY` slots, none recorded yet; `history`: an all-zero `[piece][to]` table;
/// `continuation_history`: an all-zero, much larger flattened table, see
/// `CONTINUATION_HISTORY_LEN`) and are then threaded through every depth's own `search_root` call
/// in a fold -- carried out of one iteration and straight into the next, on purpose. For
/// `killers`, that's safe specifically because a ply-indexed entry means the same thing (the same
/// actual position) at that `ply` regardless of which `target_depth` is currently running -- see
/// `Search::killers`'s own doc comment for why that's true now, where it wasn't when this table
/// was indexed by remaining depth-to-go instead. For `history`/`continuation_history`, it's safe
/// even more directly: neither was ever indexed by anything ply-dependent to begin with.
///
/// `tt`, unlike `killers`/`history`, isn't built fresh here at all -- it's `Engine`'s own
/// table, already carrying whatever earlier moves of the same game wrote into it, passed in
/// and reborrowed into each depth's own `search_root` call (Rust reborrows a `&mut T` argument
/// automatically at each call site, rather than moving it, so the same loop can pass it again
/// on the next iteration).
///
/// `previous_score` starts `None` (depth `1` has no earlier depth to center a window on) and
/// becomes `Some` after every depth that actually completes -- never from an aborted one,
/// since `stopped` always breaks this loop immediately (see below), so there is never a next
/// iteration that would need a score from one that didn't finish.
///
/// `thread_idx` identifies which worker thread this entire search belongs to -- `0` for the
/// single thread that exists today, and passed straight through to every depth's own
/// `search_root` call unchanged (it names *which thread this is*, not something that varies
/// depth to depth the way `previous_score` does). See `initial_aspiration_delta`'s own doc
/// comment for what it actually changes.
///
/// `tt` is a shared reference, not `&mut` -- this function represents *one worker thread's own*
/// full iterative-deepening search, and multiple threads need to call it concurrently against
/// the same table. That means the caller, not this function, is responsible for calling
/// `tt.new_search()` exactly once before invoking this at all (whether once, single-threaded,
/// or once per spawned thread against the same table) -- see `TranspositionTable::new_search`'s
/// own doc comment for why it must never happen per thread or per depth. This function used to
/// call it itself, back when it was still the single, only caller of `search_root`; now that
/// multiple concurrent callers are possible, only the code that knows "this is the start of a
/// new `go`" -- not this function, which has no way to tell whether it's thread 0 of 1 or
/// thread 7 of 12 -- can call it correctly.
///
/// `tablebase` is threaded straight through to `search_root`, same shared-reference shape as
/// `tt` and for the same reason -- every worker thread probes the same loaded tables
/// concurrently. Unlike `tt`, there's no matching `new_search()`-style per-`go` reset to call:
/// `SyzygyTablebase::probe` is stateless across calls, nothing here needs to change between one
/// `go` and the next.
///
/// The between-depths headroom gate below is itself split by `thread_idx`, mirroring the same
/// real-engine asymmetry `Search::stop_signal`'s own doc comment describes: only thread `0`
/// ever computes real elapsed time against `limit` and decides whether to keep going, and it
/// alone broadcasts that decision to `stop_signal` when it fires. Every other thread never
/// reads a clock here at all -- its only "should I start the next depth" check is the shared
/// flag thread `0` might have already set.
/// `evaluator` is the leaf evaluation every thread of this search uses: `None` for the PeSTO
/// one, `Some` for a trained network. Positional alongside `tt`/`tablebase` because it is the same
/// kind of thing -- a shared, read-only resource the search consults at every node -- and passed
/// explicitly rather than read from a global so two different networks can play each other inside
/// one process, which is what a generational gate is.
#[allow(clippy::too_many_arguments)]
pub fn choose_best_move(
    board: &Board, max_depth: u32, limit: Option<TimeLimit>, ancestors: &[u64], tt: &TranspositionTable,
    tablebase: &SyzygyTablebase, evaluator: Option<&Network>, thread_idx: u32, stop_signal: &AtomicBool,
) -> (Vec<IterationReport>, Option<Move>) {
    choose_best_move_with_progress(
        board, max_depth, limit, ancestors, tt, tablebase, evaluator, None, thread_idx, stop_signal, None, |_| {},
    )
}

/// `choose_best_move` with every search parameter given (`SearchParameters`), in place of the
/// evaluator's own: one process playing two parameter sets against each other.
#[allow(clippy::too_many_arguments)]
pub fn choose_best_move_with_parameters(
    board: &Board, max_depth: u32, limit: Option<TimeLimit>, ancestors: &[u64], tt: &TranspositionTable,
    tablebase: &SyzygyTablebase, evaluator: Option<&Network>, parameters: SearchParameters, thread_idx: u32,
    stop_signal: &AtomicBool,
) -> (Vec<IterationReport>, Option<Move>) {
    choose_best_move_with_progress(
        board, max_depth, limit, ancestors, tt, tablebase, evaluator, Some(parameters), thread_idx, stop_signal, None,
        |_| {},
    )
}

/// Identical to `choose_best_move` in every way except one: `on_iteration` is called once per
/// completed depth, immediately after that depth's own `IterationReport` is pushed -- letting a
/// caller observe (and, in practice, print) each depth's progress live, as it happens, rather
/// than only once this function fully returns. `choose_best_move` itself is a thin wrapper
/// around this with a no-op callback, kept as the plain entry point every existing caller
/// (dozens of tests, and every worker thread but one in `go_search`) already uses unchanged --
/// only `go_search`'s own thread `0` needs to call this directly, with a real, printing callback.
/// Every other thread still calls plain `choose_best_move`: a live progress stream from every
/// Lazy-SMP worker at once would just be overlapping, conflicting noise (several threads
/// reporting different scores for the same nominal depth at different times), the same reason
/// real engines (confirmed directly against Stockfish's own `search.cpp`) only ever report their
/// main thread's own progress, never every worker's.
///
/// `ponder` is `Some` for thread 0 of a ponder search (`PonderState`), `None` otherwise.
/// `parameters` replaces the evaluator's own (its tuned margin, or the defaults) when given.
#[allow(clippy::too_many_arguments)]
pub fn choose_best_move_with_progress(
    board: &Board, max_depth: u32, limit: Option<TimeLimit>, ancestors: &[u64], tt: &TranspositionTable,
    tablebase: &SyzygyTablebase, evaluator: Option<&Network>, parameters: Option<SearchParameters>, thread_idx: u32,
    stop_signal: &AtomicBool,
    ponder: Option<&PonderState>, mut on_iteration: impl FnMut(&IterationReport),
) -> (Vec<IterationReport>, Option<Move>) {
    let started = Instant::now();
    let mut time_manager = TimeManager::new(limit, started, evaluator.map_or(100, Network::pawn_percent));
    let mut reports = Vec::new();
    let mut total_nodes = 0;
    let mut best_move = None;
    let mut killers = vec![[None, None]; MAX_PLY as usize];
    let mut history = [[0; 64]; 12];
    let mut continuation_history = vec![0; CONTINUATION_HISTORY_LEN];
    let mut correction_history = vec![0; CORRECTION_HISTORY_LEN];
    let mut previous_score = None;
    // Root moves the DTZ tables rank below the best: never searched, so the move played keeps the
    // tablebase result within the fifty-move rule. Empty without DTZ tables or outside them.
    let excluded = tablebase.excluded_root_moves(board, ancestors).unwrap_or_default();

    // `.min(MAX_PLY)`: past `MAX_PLY`, `alpha_beta_with_buffer` clips every recursion at
    // `ply >= MAX_PLY` before it does any real work, so an iteration requesting a depth beyond
    // that returns almost instantly with nothing gained -- and, left unclamped, a limitless
    // search (`limit: None`) over a fast-resolving position (e.g. a forced mate) can run
    // thousands of these free iterations before anything else stops it, `depth` climbing far
    // past what `(target_depth - ply) * (target_depth - ply)` was ever sized for and overflowing
    // `i32` in the history bonus (a real crash hit in testing, not a hypothetical). `MAX_PLY` is
    // the one ceiling this engine has left; it has to actually bound the outer loop's own
    // request, not just internal recursion.
    for depth in 1..=max_depth.min(MAX_PLY) {
        let deadline = if thread_idx != 0 {
            // Never a clock of its own -- only ever reacts to thread 0's decision.
            if stop_signal.load(Ordering::Relaxed) {
                break;
            }
            None
        } else if depth == 1 {
            None
        } else {
            if !may_start_next_depth(&time_manager, ponder) {
                // Broadcast this decision: every other thread needs to see it too, not just
                // stop this loop -- see Search::stop_signal's own doc comment.
                stop_signal.store(true, Ordering::Relaxed);
                break;
            }
            time_manager.deadline()
        };

        let (
            pv,
            score,
            nodes,
            stopped,
            seldepth,
            updated_killers,
            updated_history,
            updated_continuation_history,
            updated_correction_history,
        ) = search_root_pondering(
            board, depth, deadline, ancestors.to_vec(), killers, history, continuation_history, correction_history, tt,
            tablebase, evaluator, parameters, previous_score, thread_idx, stop_signal, &excluded, ponder,
        );
        // Kept even when `stopped` -- see `search_root`'s own doc comment for why a killer or
        // history entry stays trustworthy evidence regardless of whether the iteration that
        // recorded it finished. The correction history learns only from nodes that finished.
        killers = updated_killers;
        history = updated_history;
        continuation_history = updated_continuation_history;
        correction_history = updated_correction_history;
        if stopped {
            break;
        }
        if pv.is_empty() {
            break;
        }
        previous_score = Some(score);
        total_nodes += nodes;
        best_move = Some(pv[0]);
        time_manager.record_iteration(pv[0], score);
        let time_ms = started.elapsed().as_millis() as u64;
        reports.push(IterationReport { depth, seldepth, score, nodes: total_nodes, pv, time_ms });
        // Right after the push, not before: on_iteration's whole point is observing a depth
        // that's now actually part of reports, not one about to be discarded by some later
        // check this function doesn't have (there is none after this point today, but the
        // ordering itself is the real guarantee, not an accident of where nothing else happens
        // to intervene yet).
        on_iteration(reports.last().unwrap());
    }

    (reports, best_move)
}

/// One additional candidate line beyond the best one, for a single completed iteration --
/// `IterationReport`'s own `score`/`pv` cover rank 1 (the best move); this covers one rank from `2`
/// to `MultiPV`. Real and exact, not a scout-search bound: each one comes from its own independent,
/// full-window `search_root` call with every better-ranked move excluded entirely
/// (`Search::excluded_root_moves`'s own doc comment covers why that distinction
/// matters -- an ordinary single-line search only ever proves a non-best root move "worse than X",
/// never its own real value).
#[derive(Clone)]
pub struct MultiPvLine {
    pub score: i32,
    pub pv: Vec<Move>,
}

/// `choose_best_move_with_progress`, generalized to report the top `multipv` root moves per
/// completed depth instead of just the best one. Always behaves as
/// thread 0 -- no `thread_idx` parameter -- since MultiPV search doesn't participate in Lazy-SMP at
/// all yet: every worker thread but 0
/// keeps calling plain `choose_best_move` regardless of the engine-wide MultiPV setting, so this is
/// only ever the one search `go_search` spawns in place of `choose_best_move_with_progress` when
/// `MultiPV > 1`.
///
/// `on_iteration`'s first argument is rank 1's own `IterationReport`, exactly what
/// `choose_best_move_with_progress`'s own callback already receives. Its second argument is every
/// additional rank found this same iteration, `2..=multipv`, in rank order -- empty whenever
/// `multipv <= 1` (`2..=multipv` is then an empty range, so no extra `search_root` call ever runs
/// that depth at all).
///
/// Every rank at a given depth is searched from the *same* starting `killers`/`history`/
/// `continuation_history` (this depth's own tables as they stood before rank 1 ran, not chained
/// from an earlier rank's own updates -- these are independent, parallel candidate searches, not a
/// sequential refinement of each other). Only rank 1's own returned, updated tables carry forward
/// into the *next* depth, exactly as they already do in `choose_best_move_with_progress` -- an
/// extra rank's own updates are reporting-only, discarded once that one `search_root` call
/// returns. Returns the same shape as `choose_best_move`/`choose_best_move_with_progress`
/// (rank-1-only `IterationReport`s, `Option<Move>` best move).
///
/// `parameters` replaces the evaluator's own (`default_parameters_for`) at every rank when given.
#[allow(clippy::too_many_arguments)]
pub fn choose_best_move_multipv_with_progress(
    board: &Board, max_depth: u32, limit: Option<TimeLimit>, ancestors: &[u64], tt: &TranspositionTable,
    tablebase: &SyzygyTablebase, evaluator: Option<&Network>, parameters: Option<SearchParameters>, stop_signal: &AtomicBool, multipv: u32,
    ponder: Option<&PonderState>, mut on_iteration: impl FnMut(&IterationReport, &[MultiPvLine]),
) -> (Vec<IterationReport>, Option<Move>) {
    let started = Instant::now();
    let mut time_manager = TimeManager::new(limit, started, evaluator.map_or(100, Network::pawn_percent));
    let mut reports = Vec::new();
    let mut total_nodes = 0;
    let mut best_move = None;
    let mut killers = vec![[None, None]; MAX_PLY as usize];
    let mut history = [[0; 64]; 12];
    let mut continuation_history = vec![0; CONTINUATION_HISTORY_LEN];
    let mut correction_history = vec![0; CORRECTION_HISTORY_LEN];
    let mut previous_score = None;
    // As in `choose_best_move_with_progress`: the root moves the DTZ tables rank below the best.
    // Every rank's search excludes them too, so lines past the best group are not shown.
    let tablebase_excluded = tablebase.excluded_root_moves(board, ancestors).unwrap_or_default();

    for depth in 1..=max_depth.min(MAX_PLY) {
        let deadline = if depth == 1 {
            None
        } else {
            if !may_start_next_depth(&time_manager, ponder) {
                stop_signal.store(true, Ordering::Relaxed);
                break;
            }
            time_manager.deadline()
        };

        let (
            pv,
            score,
            nodes,
            stopped,
            seldepth,
            updated_killers,
            updated_history,
            updated_continuation_history,
            updated_correction_history,
        ) = search_root_pondering(
            board, depth, deadline, ancestors.to_vec(), killers.clone(), history, continuation_history.clone(),
            correction_history.clone(), tt, tablebase, evaluator, parameters, previous_score, 0, stop_signal, &tablebase_excluded,
            ponder,
        );
        if stopped {
            break;
        }
        if pv.is_empty() {
            break;
        }
        previous_score = Some(score);
        total_nodes += nodes;
        best_move = Some(pv[0]);
        time_manager.record_iteration(pv[0], score);

        // Extra ranks 2..=multipv: one independent, full-window search_root call per rank, at
        // this same depth, excluding every better-ranked move found so far this iteration.
        let mut excluded = tablebase_excluded.clone();
        excluded.push(pv[0]);
        let mut extra_lines = Vec::new();
        for _ in 2..=multipv {
            let (extra_pv, extra_score, extra_nodes, extra_stopped, ..) = search_root_pondering(
                board, depth, deadline, ancestors.to_vec(), killers.clone(), history, continuation_history.clone(),
                correction_history.clone(), tt, tablebase, evaluator, parameters, previous_score, 0, stop_signal, &excluded, ponder,
            );
            total_nodes += extra_nodes;
            if extra_stopped || extra_pv.is_empty() {
                // Either this extra search itself got interrupted, or multipv exceeds the number
                // of distinct legal root moves left to rank -- stop adding further ranks this
                // iteration rather than reporting a bogus or incomplete one.
                break;
            }
            excluded.push(extra_pv[0]);
            extra_lines.push(MultiPvLine { score: extra_score, pv: extra_pv });
        }

        // Only rank 1's own updated tables carry forward into the next depth -- this function's
        // own doc comment.
        killers = updated_killers;
        history = updated_history;
        continuation_history = updated_continuation_history;
        correction_history = updated_correction_history;

        let time_ms = started.elapsed().as_millis() as u64;
        reports.push(IterationReport { depth, seldepth, score, nodes: total_nodes, pv, time_ms });
        on_iteration(reports.last().unwrap(), &extra_lines);
    }

    (reports, best_move)
}

/// Selects one worker thread's own `choose_best_move` result to actually report, out of several
/// run concurrently against the same position (Lazy SMP): the thread that completed the greatest
/// iterative-deepening depth wins; ties at the same depth are broken by comparing raw score
/// directly, no adjustment of any kind. That's a valid comparison with no sign-flipping needed
/// only because every thread searches the same root, from the same side's perspective, in the
/// same `go` -- there is no cross-thread perspective mismatch to reconcile, unlike the sign flip
/// that happens ply to ply inside any one thread's own tree. A nearer forced mate already scores
/// strictly higher than a farther one (`MATE_SCORE - ply`, from that same one shared
/// perspective), so this needs no separate mate-distance special case either.
///
/// Checked against Stockfish's own real `ThreadPool::get_best_thread()` rather than invented:
/// its actual scheme is a vote weighted by each move's score margin over the pool's *worst*
/// thread, plus a special case preferring the shorter mate. Deliberately not adopted here -- that
/// vote is a hedge against threads genuinely disagreeing, which only pays for itself statistically
/// at Stockfish's own much larger thread counts, and needs a `RootMove` list with vote-tracking
/// infrastructure Fattorini doesn't have. A directionally-reasoned simplification, not a measured one,
/// to revisit only if real multi-threaded self-play data ever shows threads disagreeing often
/// enough for it to matter.
///
/// A thread whose `reports` is empty (the root had no legal moves, or it was stopped before
/// depth `1` even finished) never wins over one with at least one completed depth -- and if every
/// thread's `reports` is empty, the result is the same empty-reports, no-move value regardless of
/// which thread's empty result happens to be picked.
pub fn select_best_result(
    results: Vec<(Vec<IterationReport>, Option<Move>)>,
) -> (Vec<IterationReport>, Option<Move>) {
    let mut best: (Vec<IterationReport>, Option<Move>) = (Vec::new(), None);

    for candidate in results {
        let candidate_wins = match (candidate.0.last(), best.0.last()) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some(c), Some(b)) => c.depth > b.depth || (c.depth == b.depth && c.score > b.score),
        };
        if candidate_wins {
            best = candidate;
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::square::Square;
    use crate::movegen::see::see;

    /// Fail-soft alpha-beta search: same tree, same final score as `negamax`, just skipping
    /// subtrees that provably can't affect the result once a move scoring `>= beta` is found.
    /// "Fail-soft" means the returned score is whatever `best_score` actually reached, not
    /// clamped into `[alpha, beta]` -- more informative than a clamped ("fail-hard") return, at
    /// no extra cost.
    ///
    /// Returns `(score, nodes)`: `nodes` counts how many times the recursive worker was actually
    /// called -- one increment per call, the same convention `perft` already uses -- so a caller
    /// can compare it against `negamax`'s own count on the same position and confirm pruning is
    /// actually happening, not just assumed from the algorithm being textbook.
    ///
    /// Lives here, not in the main module, because every caller is a test in this same file --
    /// this is the raw, untimed comparison API the test suite uses to check the real, optimized
    /// search (`alpha_beta_with_buffer`, with pruning/ordering/TT) against a plain textbook
    /// implementation, never something production code calls (that goes through
    /// `choose_best_move` instead).
    fn alpha_beta(board: &Board, depth: u32, alpha: i32, beta: i32) -> (i32, u64) {
        // No deadline -- time management only applies to real `go` searches, driven through
        // `choose_best_move` instead. No ancestors either, for the same reason: this is a
        // from-scratch comparison with no real game behind it to seed one from. Killers and
        // history start empty too -- there's no prior iterative-deepening iteration here to have
        // accumulated either. Same reasoning gives this its own throwaway, one-slot transposition
        // table rather than a real one threaded in: nothing about a from-scratch comparison call
        // should persist anywhere past this one call. Same reasoning for the stop signal: a fresh
        // one, never set, since there's no other thread here that could ever set it.
        let tt = TranspositionTable::new(0);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(
            depth, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
        );
        // `depth` here is the search's requested depth -- it becomes `target_depth` internally,
        // constant across the whole recursion, while `ply` (distance actually traveled from this
        // root) starts at `0` and counts up from there. See `alpha_beta_with_buffer`'s own doc
        // comment for why counting up (paired with a separate, absolute `MAX_PLY`) was chosen
        // over the countdown this replaces.
        let score = search.alpha_beta_with_buffer(board, 0, depth, alpha, beta);
        (score, search.nodes)
    }

    /// Fixed-depth negamax search, kept only as `alpha_beta`'s reference implementation
    /// (see the comparison tests below) -- no longer used by production code, which now
    /// calls `choose_best_move` instead. Otherwise unchanged from the version this replaced:
    /// same buffer-reuse wrapper, same fused-legality recursive worker, now also carrying a
    /// node counter (one increment per call, `perft`'s own convention) so its count is directly
    /// comparable to `alpha_beta`'s.
    fn negamax(board: &Board, depth: u32) -> (i32, u64) {
        let mut moves = Vec::new();
        let mut nodes = 0;
        let score = negamax_with_buffer(board, depth, &mut moves, &mut nodes);
        (score, nodes)
    }

    fn negamax_with_buffer(board: &Board, depth: u32, moves: &mut Vec<Move>, nodes: &mut u64) -> i32 {
        *nodes += 1;

        let start = moves.len();
        let color = board.side_to_move();
        pseudo_legal_moves(board, color, moves);
        let end = moves.len();

        let mut scratch = *board;
        // Provably never actually returned: whenever `found_legal_move` ends up true (with
        // `depth > 0`), at least one loop iteration below computes a real `score` and folds
        // it in via `.max`, so this placeholder is always superseded before `best_score` is
        // read at the bottom. It only needs to be lower than every real achievable score, not
        // some bound tied to `depth` or `MATE_SCORE` specifically, so `i32::MIN` is the
        // simplest choice that's always safely true.
        let mut best_score = i32::MIN;
        let mut found_legal_move = false;

        for i in start..end {
            let mv = moves[i];
            let undo = scratch.make_move(mv);
            if !is_in_check(&scratch, color) {
                found_legal_move = true;
                if depth > 0 {
                    let score = -negamax_with_buffer(&scratch, depth - 1, moves, nodes);
                    best_score = best_score.max(score);
                }
            }
            scratch.unmake_move(mv, undo);
        }

        moves.truncate(start);

        if !found_legal_move {
            return if is_in_check(board, color) { -(MATE_SCORE + depth as i32) } else { 0 };
        }
        if depth == 0 { evaluate(board) } else { best_score }
    }

    /// Other tests search in parallel and record into the same global table, so this checks what
    /// holds whatever they add: some nodes and some quiets recorded.
    #[cfg(feature = "move-stats")]
    #[test]
    fn move_stats_records_nodes_and_their_quiets() {
        let (board, _) = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let tablebase = SyzygyTablebase::new();
        let stop = AtomicBool::new(false);
        choose_best_move(&board, 4, None, &[], &tt, &tablebase, None, 0, &stop);
        let stats = crate::move_stats::take();
        assert!(stats.nodes.iter().sum::<u64>() > 0);
        let quiets: u64 = (0..crate::move_stats::DEPTHS).map(|depth| stats.depth_totals(depth).iter().sum::<u64>()).sum();
        assert!(quiets > 0);
    }

    #[test]
    fn depth_zero_returns_plain_material_evaluation() {
        // No terminal condition here (plenty of legal moves both sides), so depth 0 must
        // fall through to a direct evaluate() call -- confirmed by comparing against it
        // directly rather than hand-computing the expected number twice.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNB1KBNR w KQkq - 0 1").unwrap();
        assert_eq!(negamax(&board, 0).0, evaluate(&board));
    }

    #[test]
    fn tablebase_tt_move_can_never_match_a_real_generated_move() {
        // from == to can never be a real move -- every actual move changes square -- so this
        // sentinel is guaranteed to fail to match anything in a real move list and simply be
        // ignored by the TT-move priority stage, the same fate an already-stale TT move from a
        // completely different context already has. The one piece of the TT-caching change
        // actually testable without real .rtbw files (see the test just below for why).
        let mv = tablebase_tt_move();
        assert_eq!(mv.from(), mv.to());
    }

    #[test]
    fn a_tablebase_with_no_loaded_tables_leaves_alpha_beta_with_buffer_unaffected() {
        // No .rtbw files are checked into this repo (tablebase.rs's own progress notes cover
        // why -- they're downloaded locally by the developer, never committed), so there is
        // nothing here to exercise a real tablebase *hit* against. This confirms the weaker but
        // still real thing that can be checked without one: Search::tablebase being genuinely
        // Some(&tablebase) rather than the default None doesn't change this function's result
        // at all when that tablebase has nothing loaded (SyzygyTablebase::probe's own
        // max_pieces() gate rejects every position outright first) -- the exact behavior every
        // other test in this module already exercises implicitly via new_search's own default.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNB1KBNR w KQkq - 0 1").unwrap();
        let depth = 2;
        let (without_tablebase, _) = alpha_beta(&board, depth, -INFINITY, INFINITY);

        let tablebase = SyzygyTablebase::new();
        let tt = TranspositionTable::new(0);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(
            depth, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12],
            vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
        );
        search.tablebase = Some(&tablebase);
        let with_tablebase = search.alpha_beta_with_buffer(&board, 0, depth, -INFINITY, INFINITY);

        assert_eq!(with_tablebase, without_tablebase);
    }

    #[test]
    fn futility_pruning_reduces_nodes_when_alpha_is_high() {
        // White has 9 legal quiet moves here (two pawn pushes each for a2/h2, five king
        // moves), none captures or promotions, none giving check -- a clean, minimal position
        // for isolating futility pruning's own effect at `target_depth = 1` (`remaining == 1`,
        // well within `FUTILITY_MAX_DEPTH`). `alpha` set to exactly the pruning threshold
        // (`static_eval + futility_margin`) should skip every move but the mandatory
        // first one; `-INFINITY` should skip none -- the same "identical algorithm, one
        // engineered value changed" methodology already used for delta pruning's own tests.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/P6P/4K3 w - - 0 1").unwrap();
        let static_eval = evaluate(&board);
        let high_alpha = static_eval + SearchParameters::DEFAULT.futility_margin;

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.set_parameters(SearchParameters::DEFAULT);
        search.alpha_beta_with_buffer(&board, 0, 1, high_alpha, INFINITY);
        let high_alpha_nodes = search.nodes;

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.set_parameters(SearchParameters::DEFAULT);
        search.alpha_beta_with_buffer(&board, 0, 1, -INFINITY, INFINITY);
        let low_alpha_nodes = search.nodes;

        assert!(high_alpha_nodes < low_alpha_nodes);
    }

    #[test]
    fn futility_pruning_never_skips_the_first_move_at_a_node() {
        // Same position as above, `alpha` pushed absurdly high -- every quiet move's own best
        // case fails the futility test. If the first-move exemption didn't exist, `best_move`
        // would stay `None` despite a legal move existing, and this node's own store would
        // panic on its `best_move.expect(...)`.
        //
        // Asserting `pv[0]` is non-empty, not a node-count floor: an absurdly high `alpha`
        // here also means the mandatory first move's own child call inherits an extremely low
        // `beta` (`-alpha`, negated) -- low enough that reverse futility pruning now correctly
        // prunes that child before it ever reaches `quiescence`, which is completely
        // legitimate alpha-beta behavior, not a bug (see `search.md`'s reverse futility
        // pruning design section). That once made a plain `nodes >= 3` floor here go stale for
        // reasons having nothing to do with the actual thing this test checks -- `pv[0]` being
        // non-empty is the real invariant (a move was found and recorded as best), and stays
        // true regardless of how much or how little gets searched beneath that first move.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/P6P/4K3 w - - 0 1").unwrap();
        let static_eval = evaluate(&board);
        let alpha = static_eval + SearchParameters::DEFAULT.futility_margin * MAX_PLY as i32;

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.set_parameters(SearchParameters::DEFAULT);
        search.alpha_beta_with_buffer(&board, 0, 1, alpha, INFINITY);
        assert!(!search.pv.line(0).is_empty(), "expected the mandatory first move to still be found and recorded");
    }

    #[test]
    fn futility_pruning_does_not_apply_when_alpha_is_already_mate_scale() {
        // Same position as futility_pruning_reduces_nodes_when_alpha_is_high, but alpha pushed
        // to a genuine mate-for-us bound instead of merely past the ordinary pruning
        // threshold: without the `!is_win(alpha)` guard, every quiet move past the mandatory
        // first one would still look hopeless against a mate-scale alpha and get skipped --
        // the real, pre-existing gap found and fixed alongside the tablebase work (razoring's
        // own identical margin check already had this guard; this one didn't). Compared
        // against that same test's own `high_alpha` (an ordinary, non-mate value already past
        // the pruning margin) rather than against its `-INFINITY` case: a mate-scale alpha
        // still isn't identical to `-INFINITY` overall (it still narrows every child call's
        // own `beta`, which affects other pruning deeper in the tree, so exact equality with
        // the fully unpruned count isn't the right invariant) -- but disabling *this* guard
        // specifically must still search strictly more than `high_alpha`'s own pruned count,
        // since `high_alpha` demonstrably triggers futility pruning at every move past the
        // first (that test's own point) and `mate_scale_alpha` is even larger still, so an
        // unfixed guard would prune at least as aggressively, never more loosely.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/P6P/4K3 w - - 0 1").unwrap();
        let static_eval = evaluate(&board);
        let high_alpha = static_eval + SearchParameters::DEFAULT.futility_margin;
        let mate_scale_alpha = MATE_SCORE - MAX_PLY as i32;

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.set_parameters(SearchParameters::DEFAULT);
        search.alpha_beta_with_buffer(&board, 0, 1, high_alpha, INFINITY);
        let high_alpha_nodes = search.nodes;

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.set_parameters(SearchParameters::DEFAULT);
        search.alpha_beta_with_buffer(&board, 0, 1, mate_scale_alpha, INFINITY);
        let mate_scale_alpha_nodes = search.nodes;

        assert!(
            mate_scale_alpha_nodes > high_alpha_nodes,
            "expected a mate-scale alpha ({mate_scale_alpha_nodes} nodes) to disable futility pruning \
             entirely, searching strictly more than an ordinary pruning-triggering alpha does \
             ({high_alpha_nodes} nodes)"
        );
    }

    #[test]
    fn futility_pruning_does_not_apply_when_alpha_is_already_tablebase_scale() {
        // Same shape as the mate-scale version just above, but with a TB-scale alpha instead --
        // added alongside the tablebase work, for the identical reason.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/P6P/4K3 w - - 0 1").unwrap();
        let static_eval = evaluate(&board);
        let high_alpha = static_eval + SearchParameters::DEFAULT.futility_margin;

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.set_parameters(SearchParameters::DEFAULT);
        search.alpha_beta_with_buffer(&board, 0, 1, high_alpha, INFINITY);
        let high_alpha_nodes = search.nodes;

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.set_parameters(SearchParameters::DEFAULT);
        search.alpha_beta_with_buffer(&board, 0, 1, TB_WIN_SCORE, INFINITY);
        let tb_scale_alpha_nodes = search.nodes;

        assert!(
            tb_scale_alpha_nodes > high_alpha_nodes,
            "expected a TB-scale alpha ({tb_scale_alpha_nodes} nodes) to disable futility pruning \
             entirely, searching strictly more than an ordinary pruning-triggering alpha does \
             ({high_alpha_nodes} nodes)"
        );
    }

    #[test]
    fn reverse_futility_pruning_skips_the_whole_node_when_static_eval_beats_beta_by_a_wide_margin() {
        // White (a bare king plus a queen against a bare king) is up comfortably more than
        // any plausible `reverse_futility_margin * remaining` margin could ever eat through --
        // `evaluate`'s own material term alone clears `beta` here. `ply = 1`, not `0`:
        // reverse futility pruning is gated to `ply > 0` (see the implementation's own
        // comment for why), so this calls `alpha_beta_with_buffer` directly at a non-root
        // ply, the same way other single-node tests in this file already do.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        let static_eval = evaluate(&board);

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        let score = search.alpha_beta_with_buffer(&board, 1, 3, -1, 0);
        // The midpoint of the evaluation and beta (0), `blended`.
        assert_eq!(score, static_eval / 2);
        assert_eq!(search.nodes, 1);

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.alpha_beta_with_buffer(&board, 1, 3, -INFINITY, INFINITY);
        assert!(search.nodes > 1, "expected the unpruned arm to actually search moves");
    }

    #[test]
    fn reverse_futility_pruning_does_not_apply_at_the_root() {
        // Same position and triggering condition as above, but at `ply = 0`: without the
        // `ply > 0` gate, this would return a bare score with no move behind it -- silently
        // reproducing the exact `bestmove 0000` bug class the TT cutoff above already had to
        // guard against once, for real (see that probe's own doc comment).
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.alpha_beta_with_buffer(&board, 0, 3, -INFINITY, 0);
        assert!(!search.pv.line(0).is_empty(), "expected the root to still find and report a real move");
    }

    #[test]
    fn razoring_delegates_to_quiescence_when_static_eval_is_far_below_alpha() {
        // Black is up a queen and a rook against a bare White king, well clear of any of the
        // three pieces giving check -- `evaluate` (White to move) comes out far enough below
        // `alpha` here that razoring's own unbounded margin comfortably clears it. `ply = 1`,
        // not `0`: razoring is gated to `ply > 0` (see the implementation's own comment for
        // why), so this calls `alpha_beta_with_buffer` directly at a non-root ply.
        //
        // Compared directly against an independent `quiescence` call on the same
        // position/window rather than just checking "some plausible score": if razoring
        // actually delegates, the two must agree exactly, and the node count must be exactly
        // `1` (this node itself) plus whatever `quiescence` alone visits -- not a full move
        // loop's worth of additional search.
        let (board, _) = Board::from_fen("4k3/8/8/r7/q7/8/8/4K3 w - - 0 1").unwrap();

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut quiescence_only =
            new_search(3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        let expected = quiescence_only.quiescence(&board, 1, -500, -499, None);

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        let score = search.alpha_beta_with_buffer(&board, 1, 3, -500, -499);
        assert_eq!(score, expected);
        assert_eq!(search.nodes, 1 + quiescence_only.nodes);
    }

    #[test]
    fn razoring_does_not_apply_at_the_root() {
        // Same position and triggering condition as above, but at `ply = 0`: without the
        // `ply > 0` gate, this would return a bare `quiescence` result with no move behind
        // it -- the exact `bestmove 0000` bug class the TT cutoff above already had to guard
        // against once, for real (see that probe's own doc comment).
        let (board, _) = Board::from_fen("4k3/8/8/r7/q7/8/8/4K3 w - - 0 1").unwrap();

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.alpha_beta_with_buffer(&board, 0, 3, -500, INFINITY);
        assert!(!search.pv.line(0).is_empty(), "expected the root to still find and report a real move");
    }

    #[test]
    fn razoring_does_not_apply_when_alpha_is_already_mate_scale() {
        // Same hopeless-looking position, but `alpha` set to a mate-for-us bound: without the
        // `!is_win(alpha)` guard, any ordinary evaluation would look hopeless against it,
        // wrongly delegating to quiescence's captures-and-checks-only search instead of
        // running the full move loop that could actually deliver a real forced mate elsewhere
        // in the tree. `pv[1]` (not `pv[0]`, since this calls at `ply = 1`) non-empty proves
        // the full loop ran and recorded a real move -- impossible via razoring's own bypass,
        // which returns before ever touching `pv`.
        let (board, _) = Board::from_fen("4k3/8/8/r7/q7/8/8/4K3 w - - 0 1").unwrap();
        let alpha = MATE_SCORE - MAX_PLY as i32;

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.alpha_beta_with_buffer(&board, 1, 3, alpha, INFINITY);
        assert!(!search.pv.line(1).is_empty(), "expected the full move loop to run and record a real move");
    }

    #[test]
    fn razoring_does_not_apply_when_alpha_is_already_tablebase_scale() {
        // Same shape as the mate-scale version just above, but with a TB-scale alpha instead --
        // added alongside the tablebase work, for the identical reason.
        let (board, _) = Board::from_fen("4k3/8/8/r7/q7/8/8/4K3 w - - 0 1").unwrap();

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.alpha_beta_with_buffer(&board, 1, 3, TB_WIN_SCORE, INFINITY);
        assert!(!search.pv.line(1).is_empty(), "expected the full move loop to run and record a real move");
    }

    #[test]
    fn null_move_pruning_prunes_the_whole_node_when_a_reduced_probe_still_fails_high() {
        // White (a bare king plus a queen against a bare king) is so far ahead that even
        // letting Black move twice in a row -- the null move itself, then Black's own real
        // reply -- still fails high against a modest `beta`, exactly the case this technique
        // exists to skip. `ply = 1`, `target_depth = 5` (`remaining = 4`): the minimum this
        // technique ever applies at, and comfortably past `FUTILITY_MAX_DEPTH` (`3`), so
        // reverse futility pruning's own `static_eval` is `None` here and can't be the thing
        // actually doing the cutting.
        //
        // Verified two ways: the returned score must exactly match an independent recursive
        // probe of the null-moved position computed the same way the real code computes it, and
        // the node count must be exactly `1` (this node itself) plus whatever that recursive
        // probe alone visits -- not a full move loop's worth of additional search.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        let beta = 0;

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut expected_probe =
            new_search(3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        let mut null_moved_board = board;
        null_moved_board.make_null_move();
        let expected_score = -expected_probe.alpha_beta_with_buffer(&null_moved_board, 2, 3, -beta, -beta + 1);
        assert!(expected_score >= beta, "expected even a reduced probe against Black to still fail high");
        assert!(!is_win(expected_score), "expected an ordinary score here, not a mate -- see the clamp test below");

        let tt = TranspositionTable::new(1);
        // A harmless pre-existing TT move (depth `0`, well under `remaining`, so it can never
        // trigger the early-return cutoff above) keeps Internal Iterative Reduction from firing
        // here -- without it, `remaining == 4` is *also* exactly `IIR_MIN_REMAINING`, and IIR
        // would silently shave this node down to `remaining == 3`, below
        // `NULL_MOVE_MIN_REMAINING`, defeating the very scenario this test means to isolate. Real
        // play has both techniques active and stacking at once; this test isolates one of them.
        tt.store(board.hash(), Move::new(Square::D1, Square::D2, None), 0, 0, Bound::Upper);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(5, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        let score = search.alpha_beta_with_buffer(&board, 1, 5, beta - 1, beta);
        assert_eq!(score, expected_score);
        assert_eq!(search.nodes, 1 + expected_probe.nodes);
    }

    #[test]
    fn null_move_pruning_clamps_a_mate_range_result_from_the_reduced_probe_to_beta() {
        // Constructed so that passing -- the null move itself -- hands Black a position where
        // Black is already checkmated: White queen g7, defended by White's own king on f7,
        // Black's bare king boxed into h8 with no legal reply and no escape square. The reduced
        // probe genuinely finds a "mate," but only because Black got a free pass into an
        // already-lost position, not a real mate distance -- without the clamp below, this
        // would propagate mate-scale magnitude upward as if it meant something real.
        let (board, _) = Board::from_fen("7k/5KQ1/8/8/8/8/8/8 w - - 0 1").unwrap();

        let tt = TranspositionTable::new(1);
        // Same reasoning as `null_move_pruning_prunes_the_whole_node_when_a_reduced_probe_still_
        // fails_high`'s own harmless pre-existing TT move: keeps Internal Iterative Reduction
        // from claiming this node's own `remaining == 4` before null-move pruning's `remaining >=
        // NULL_MOVE_MIN_REMAINING` check gets to see it.
        tt.store(board.hash(), Move::new(Square::G7, Square::G6, None), 0, 0, Bound::Upper);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(5, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        let score = search.alpha_beta_with_buffer(&board, 1, 5, -INFINITY, 0);
        assert_eq!(score, 0, "expected the cutoff to clamp to beta, not leak a bogus mate-range score");
    }

    #[test]
    fn a_checking_move_recurses_at_one_more_ply_than_an_ordinary_move_would() {
        // White's king on a1 is in check from Black's rook on a5, along the otherwise-empty
        // a-file. White's queen, sitting on a8, resolves this by capturing the rook (Qxa5) --
        // a capture, so `is_quiet` is already false for it, which alone keeps futility, late-
        // move, and history pruning from ever touching it (all three require a quiet move to
        // prune at all). That capture happens to land the queen on the same rank as Black's own
        // king on e5, with nothing between them on that rank -- so recapturing the rook also
        // delivers check, the move this test actually means to isolate. Seeded into the TT as
        // this node's own best move (the same `depth == 0`/`Bound::Upper` "ordering hint only"
        // convention already used elsewhere in this file) so it's guaranteed to be tried first.
        //
        // The outer node itself being in check (not just the move under test giving check) is
        // the deliberate design choice here, not incidental: `in_check` gates reverse futility
        // pruning's own `static_eval` computation, null-move pruning's own `eval` computation
        // and its own `is_null_move_pruning_applicable` precondition, and late-move pruning's
        // own node-level gate -- all three read `!in_check`. With the outer node itself in
        // check, every one of those early returns is structurally impossible here regardless of
        // `remaining` or `beta`, which is what lets this test go back to the simplest possible
        // window (a tight `beta = 0`, this position's overwhelming material edge guaranteeing an
        // actual fail-high on the very first move) instead of hunting for a depth/window
        // combination that dodges each heuristic's own trigger range one at a time.
        let (board, _) = Board::from_fen("Q7/8/8/r3k3/8/8/8/K7 w - - 0 1").unwrap();
        assert!(is_in_check(&board, Color::White));
        let checking_move = Move::new(Square::A8, Square::A5, None);
        assert!(gives_check(&board, checking_move));

        let tt = TranspositionTable::new(1);
        tt.store(board.hash(), checking_move, 0, 0, Bound::Upper);
        let stop_signal = AtomicBool::new(false);
        let beta = 0;

        // Independently computed expectation: exactly what the production code should do once a
        // move is confirmed to give check -- recurse at `target_depth + 1`, the extension itself
        // -- via a completely separate `Search`/`TranspositionTable`, so nothing here shares any
        // state with the real run below.
        let expected_tt = TranspositionTable::new(1);
        let expected_stop_signal = AtomicBool::new(false);
        let mut expected_probe = new_search(
            4, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12],
            vec![0; CONTINUATION_HISTORY_LEN], &expected_tt, &expected_stop_signal,
        );
        let mut scratch = board;
        scratch.make_move(checking_move);
        let expected_score = -expected_probe.alpha_beta_with_buffer(&scratch, 2, 4 + 1, -beta, INFINITY);
        assert!(expected_score >= beta, "expected recapturing the rook to still fail high here");

        let mut search =
            new_search(4, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        let score = search.alpha_beta_with_buffer(&board, 1, 4, -INFINITY, beta);

        // Confirms the TT hint actually did put `checking_move` first, and that it really did
        // fail high on the strength of that one move alone -- if this ever fails, the score/
        // node-count mismatches below would be a red herring about ordering, not about the
        // extension itself.
        assert_eq!(search.pv.line(1).first(), Some(&checking_move));

        // The real proof is both the score *and* the node count: if the recursive call had used
        // the un-extended `target_depth` alone (`4`), it would still search *something*, but a
        // different amount of it -- an exact match on both together is only possible if the
        // extension really did add the extra ply, not merely architecturally exist unused.
        assert_eq!(score, expected_score);
        assert_eq!(search.nodes, 1 + expected_probe.nodes);
    }

    #[test]
    fn checkmate_is_detected_regardless_of_requested_depth() {
        // Classic back-rank mate: White king g1 boxed in by its own f2/g2/h2 pawns, Black
        // rook a1 checking along the completely open first rank -- no block, no capture,
        // no escape square (f1/h1 are still on the attacked rank; g2 is occupied). The
        // root position itself has zero legal moves, so this must return the same mate
        // score at every requested depth: the root's own `depth` parameter is what gets
        // used, since no recursion into children ever happens.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/5PPP/r5K1 w - - 0 1").unwrap();
        assert_eq!(negamax(&board, 0).0, -MATE_SCORE);
        assert_eq!(negamax(&board, 3).0, -(MATE_SCORE + 3));
    }

    #[test]
    fn stalemate_is_a_draw_regardless_of_requested_depth() {
        // The textbook King+Queen-vs-King stalemate trap: White king a1 has no legal
        // move (a2 and b1 are both attacked by the queen on b3 via diagonal/file, b2 is
        // attacked by both the queen and the black king) and isn't in check itself (a1
        // isn't reachable by the queen in one move from b3). Same "same result at every
        // depth" logic as the checkmate test above.
        let (board, _) = Board::from_fen("8/8/8/8/8/1q6/2k5/K7 w - - 0 1").unwrap();
        assert_eq!(negamax(&board, 0).0, 0);
        assert_eq!(negamax(&board, 2).0, 0);
    }

    #[test]
    fn depth_one_prefers_the_move_that_wins_more_material() {
        // White king e1, knight e4; Black king e8, pawn c5 (a legal knight capture:
        // e4-c5 is a valid knight move). Every other legal move is quiet, so the capturing
        // move must be the one negamax actually settles on. Rather than hand-deriving
        // evaluate()'s own exact number for the resulting position (which would need
        // re-deriving by hand every time TDLeaf(lambda) retunes any weight -- see
        // eval-tuning.md), compare against evaluate() computed directly on the actual
        // resulting board: negamax at depth 1 can only return the negated evaluate() of
        // the position after Nxc5 specifically if it actually chose that move over every
        // quiet alternative.
        let (board, _) = Board::from_fen("4k3/8/8/2p5/4N3/8/8/4K3 w - - 0 1").unwrap();
        let mut after_capture = board;
        after_capture.make_move(Move::new(Square::E4, Square::C5, None));
        assert_eq!(negamax(&board, 1).0, -evaluate(&after_capture));
    }

    // The four tests above, repeated directly against `alpha_beta` with a full window
    // (`-INFINITY, INFINITY` -- the same window a fresh search always starts with, see its
    // own doc comment): a full-window search can't prune anything at its own top level (see
    // `matches_negamax_and_visits_fewer_nodes_once_there_is_something_to_prune` below for
    // why), so these confirm `alpha_beta`'s basic correctness independently of `negamax`,
    // not merely by comparison to it.

    #[test]
    fn alpha_beta_depth_zero_returns_plain_material_evaluation() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNB1KBNR w KQkq - 0 1").unwrap();
        assert_eq!(alpha_beta(&board, 0, -INFINITY, INFINITY).0, evaluate(&board));
    }

    #[test]
    fn alpha_beta_checkmate_is_detected_regardless_of_requested_depth() {
        // Unlike `negamax`'s own reference version of this test (above), both assertions here
        // expect the *same* score: mate distance is scored from `ply` (this checkmate sits at
        // the root, `ply == 0`, regardless of how deep the search was asked to go), not from
        // `target_depth` -- requesting a deeper search doesn't change the real distance to a
        // mate that's already sitting at the root.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/5PPP/r5K1 w - - 0 1").unwrap();
        assert_eq!(alpha_beta(&board, 0, -INFINITY, INFINITY).0, -MATE_SCORE);
        assert_eq!(alpha_beta(&board, 3, -INFINITY, INFINITY).0, -MATE_SCORE);
    }

    #[test]
    fn alpha_beta_stalemate_is_a_draw_regardless_of_requested_depth() {
        let (board, _) = Board::from_fen("8/8/8/8/8/1q6/2k5/K7 w - - 0 1").unwrap();
        assert_eq!(alpha_beta(&board, 0, -INFINITY, INFINITY).0, 0);
        assert_eq!(alpha_beta(&board, 2, -INFINITY, INFINITY).0, 0);
    }

    #[test]
    fn alpha_beta_depth_one_prefers_the_move_that_wins_more_material() {
        // A second Black pawn (a7, untouched by anything below) keeps Nxc5 from landing on
        // King+Knight vs bare King -- one of the four insufficient-material patterns, which
        // would otherwise score the capture as a dead draw (0) instead of the material gain
        // this test means to confirm. With a7 still on the board afterwards, the position
        // stays a real, ongoing King+Knight vs King+Pawn -- not insufficient material -- so
        // the capture's actual value shows through. Same evaluate()-of-the-actual-resulting-
        // position pattern as `depth_one_prefers_the_move_that_wins_more_material` above,
        // for the same reason (no hand-derived magic number to re-derive after every future
        // reweighting).
        let (board, _) = Board::from_fen("4k3/p7/8/2p5/4N3/8/8/4K3 w - - 0 1").unwrap();
        let mut after_capture = board;
        after_capture.make_move(Move::new(Square::E4, Square::C5, None));
        assert_eq!(alpha_beta(&board, 1, -INFINITY, INFINITY).0, -evaluate(&after_capture));
    }

    #[test]
    fn visits_fewer_nodes_than_negamax_once_there_is_something_to_prune() {
        // The starting position, three plies deep: enough branching (20 first moves, ~20-30
        // replies to each, ~20-30 replies to those) that real material swings -- a hanging
        // piece somewhere among the many lines a fixed, untactical move order walks through
        // -- are essentially certain, which is exactly what a cutoff needs (a tie can never
        // trigger `alpha >= beta`, only a genuine improvement can). A single top-level call
        // like this one can't prune anything at its own root (the very first move explored
        // always gets the full incoming window, and nothing above this call has narrowed it
        // going in) -- any savings has to come from cutoffs *inside* the tree beneath it, at
        // nodes that inherited an already-tightened bound from a sibling explored earlier in
        // their own parent's loop.
        //
        // Deliberately not comparing `alpha_beta`'s score against `negamax`'s own here (this
        // test used to, until bishop mobility broke it): `alpha_beta`'s leaf always extends
        // through `quiescence` once it reaches the horizon, while `negamax`'s leaf always
        // stops flat at a plain `evaluate` call -- confirmed (via a temporary diagnostic,
        // since deleted) that quiescence genuinely fires hundreds of times across this tree,
        // so the two searches are answering different questions whenever a capture sits near
        // the horizon of whichever line each one settles on as best. They were never actually
        // guaranteed to agree on the *score* in that case -- only on relative node count,
        // which is what this test is actually named for and still checks.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();

        let (_, negamax_nodes) = negamax(&board, 3);
        let (_, alpha_beta_nodes) = alpha_beta(&board, 3, -INFINITY, INFINITY);

        assert!(
            alpha_beta_nodes < negamax_nodes,
            "expected alpha-beta to visit fewer nodes than negamax ({alpha_beta_nodes} vs {negamax_nodes})"
        );
    }

    #[test]
    fn mvv_lva_ranks_bigger_victims_first() {
        // Two hypothetical captures by the same rook, scored without regard to whether
        // either is actually reachable in one legal move (`move_score` only reads what's
        // on `from`/`to`, never move geometry or path-clearance -- the pawn on d2 sitting
        // between the rook and the queen doesn't matter here). MVV-LVA's whole point is
        // that what a capture wins dominates the ranking, so the queen capture must sort
        // first regardless of the two attackers being identical.
        let (board, _) = Board::from_fen("4k3/8/8/3q4/8/8/3p4/3RK3 w - - 0 1").unwrap();
        let capture_pawn = Move::new(Square::D1, Square::D2, None);
        let capture_queen = Move::new(Square::D1, Square::D5, None);

        assert!(move_score(&board, capture_queen) > move_score(&board, capture_pawn));
    }

    #[test]
    fn mvv_lva_prefers_the_cheaper_attacker_for_equal_victims() {
        // Same victim (the queen on d4), two hypothetical attackers of different value --
        // the pawn capture must sort first: between two captures that win the same thing,
        // giving up less material if the capture turns out to be unsound is preferred.
        let (board, _) = Board::from_fen("4k3/8/8/8/1P1q4/8/8/1R2K3 w - - 0 1").unwrap();
        let pawn_takes_queen = Move::new(Square::B4, Square::D4, None);
        let rook_takes_queen = Move::new(Square::B1, Square::D4, None);

        assert!(move_score(&board, pawn_takes_queen) > move_score(&board, rook_takes_queen));
    }

    #[test]
    fn captures_and_promotions_are_ordered_ahead_of_quiet_moves() {
        let (board, _) = Board::from_fen("r3k3/1P6/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let quiet_king_move = Move::new(Square::E1, Square::D1, None);
        // A quiet promotion (b8 is empty) -- no capture involved, but still scored well
        // above an ordinary quiet move via the promoted piece's own value.
        let promotion = Move::new(Square::B7, Square::B8, Some(PieceType::Queen));

        assert!(move_score(&board, promotion) > move_score(&board, quiet_king_move));
    }

    #[test]
    fn is_quiet_is_true_only_for_moves_with_no_capture_and_no_promotion() {
        let (board, _) = Board::from_fen("r3k3/1P6/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let quiet_king_move = Move::new(Square::E1, Square::D1, None);
        let promotion = Move::new(Square::B7, Square::B8, Some(PieceType::Queen));
        assert!(is_quiet(&board, quiet_king_move));
        assert!(!is_quiet(&board, promotion));

        let (capture_board, _) = Board::from_fen("4k3/8/8/3q4/8/8/3p4/3RK3 w - - 0 1").unwrap();
        let capture_pawn = Move::new(Square::D1, Square::D2, None);
        assert!(!is_quiet(&capture_board, capture_pawn));
    }

    #[test]
    fn is_recapture_is_true_only_when_the_move_lands_on_the_previous_moves_destination() {
        let mv = Move::new(Square::D5, Square::E6, None);
        assert!(is_recapture(mv, Some(Square::E6)));
        assert!(!is_recapture(mv, Some(Square::D5)));
        assert!(!is_recapture(mv, None));
    }

    #[test]
    fn gives_check_detects_a_direct_check_from_the_moved_pieces_new_square() {
        // White queen captures a pawn on d5, landing on the same file as the Black king with
        // nothing between them (d6/d7 empty) -- a direct check delivered by the queen itself,
        // not by anything it uncovers.
        let (board, _) = Board::from_fen("3k4/8/8/3p4/8/8/8/K2Q4 w - - 0 1").unwrap();
        let mv = Move::new(Square::D1, Square::D5, None);
        assert!(gives_check(&board, mv));
    }

    #[test]
    fn gives_check_is_false_for_a_capture_that_checks_nothing() {
        // Same queen, capturing a pawn on h5 instead -- reachable in one move (the same
        // diagonal), but not aligned with the Black king on d8 by rank, file, or diagonal.
        let (board, _) = Board::from_fen("3k4/8/8/7p/8/8/8/K2Q4 w - - 0 1").unwrap();
        let mv = Move::new(Square::D1, Square::H5, None);
        assert!(!gives_check(&board, mv));
    }

    #[test]
    fn gives_check_detects_a_discovered_check_from_a_friendly_slider() {
        // White rook a1, White knight a4 blocking its own file view of the Black king on a8.
        // The knight captures a pawn on c3 -- a destination that does *not* itself attack a8
        // (isolating this from a direct check), but vacating a4 opens the a-file, giving check
        // via the rook instead.
        let (board, _) = Board::from_fen("k7/8/8/8/N7/2p5/4K3/R7 w - - 0 1").unwrap();
        let mv = Move::new(Square::A4, Square::C3, None);
        assert!(gives_check(&board, mv));
    }

    #[test]
    fn gives_check_detects_a_discovered_check_revealed_by_an_en_passant_capture() {
        // The classic "horizontal discovered check" case: White rook a5, Black king h5, White
        // pawn e5 capturing Black's just-double-pushed pawn en passant (to d6). The captured
        // pawn's own square (d5, not the move's destination d6) is the one blocker between the
        // rook and the king on this rank -- only correct if `gives_check` clears *that* square,
        // not just `mv.from()`/`mv.to()`.
        let (board, _) = Board::from_fen("8/8/8/R2pP2k/8/8/8/1K6 w - - 0 1").unwrap();
        let mv = Move::en_passant_capture(Square::E5, Square::D6);
        assert!(gives_check(&board, mv));
    }

    #[test]
    fn classify_bound_reports_upper_when_the_score_never_raised_alpha() {
        assert_eq!(classify_bound(10, 10, 100), Bound::Upper);
        assert_eq!(classify_bound(5, 10, 100), Bound::Upper);
    }

    #[test]
    fn classify_bound_reports_lower_when_the_score_reached_beta() {
        assert_eq!(classify_bound(100, 10, 100), Bound::Lower);
        assert_eq!(classify_bound(150, 10, 100), Bound::Lower);
    }

    #[test]
    fn classify_bound_reports_exact_when_the_score_landed_strictly_inside_the_window() {
        assert_eq!(classify_bound(50, 10, 100), Bound::Exact);
    }

    #[test]
    fn try_killer_swaps_the_killer_to_front_when_present() {
        let mut moves = vec![
            Move::new(Square::E1, Square::D1, None),
            Move::new(Square::E1, Square::F1, None),
            Move::new(Square::E1, Square::E2, None),
        ];
        let until = moves.len();
        assert!(try_killer(&mut moves, 0, until, Some(Move::new(Square::E1, Square::E2, None))));
        assert_eq!(moves[0], Move::new(Square::E1, Square::E2, None));
    }

    #[test]
    fn try_killer_reports_failure_and_leaves_moves_unchanged_when_the_killer_is_absent() {
        let original = vec![Move::new(Square::E1, Square::D1, None), Move::new(Square::E1, Square::F1, None)];
        let mut moves = original.clone();
        // A move that never appears in the slice at all -- e.g. recorded by a sibling node
        // whose own position doesn't share this candidate.
        let until = moves.len();
        assert!(!try_killer(&mut moves, 0, until, Some(Move::new(Square::A1, Square::A8, None))));
        assert_eq!(moves, original);
    }

    #[test]
    fn try_killer_reports_failure_when_no_killer_has_been_recorded_yet() {
        let original = vec![Move::new(Square::E1, Square::D1, None)];
        let mut moves = original.clone();
        let until = moves.len();
        assert!(!try_killer(&mut moves, 0, until, None));
        assert_eq!(moves, original);
    }

    #[test]
    fn try_best_noisy_move_reports_failure_and_leaves_moves_unchanged_when_everything_is_quiet() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let original = vec![Move::new(Square::E1, Square::D1, None), Move::new(Square::E1, Square::F1, None)];
        let mut moves = original.clone();
        let until = moves.len();
        assert!(!try_best_noisy_move(&mut moves, 0, until, &board, move_score));
        assert_eq!(moves, original);
    }

    #[test]
    fn try_best_noisy_move_reports_success_and_swaps_in_an_actual_capture() {
        let (board, _) = Board::from_fen("4k3/8/8/3q4/8/8/8/3RK3 w - - 0 1").unwrap();
        let quiet_king_move = Move::new(Square::E1, Square::F1, None);
        let capture_queen = Move::new(Square::D1, Square::D5, None);
        let mut moves = vec![quiet_king_move, capture_queen];
        let until = moves.len();
        assert!(try_best_noisy_move(&mut moves, 0, until, &board, move_score));
        assert_eq!(moves[0], capture_queen);
    }

    /// Two rooks each take a pawn: equal `move_score`. The pick is the same in either buffer order,
    /// and a quiet between them changes nothing, so the pick depends only on the set of moves.
    #[test]
    fn try_best_noisy_move_breaks_ties_by_the_move_not_by_its_place_in_the_buffer() {
        let (board, _) = Board::from_fen("4k3/8/8/p6p/8/8/8/R3K2R w - - 0 1").unwrap();
        let a_rook_takes = Move::new(Square::A1, Square::A5, None);
        let h_rook_takes = Move::new(Square::H1, Square::H5, None);
        let quiet_king_move = Move::new(Square::E1, Square::E2, None);
        assert_eq!(move_score(&board, a_rook_takes), move_score(&board, h_rook_takes));
        for buffer in [
            vec![a_rook_takes, h_rook_takes],
            vec![h_rook_takes, a_rook_takes],
            vec![quiet_king_move, h_rook_takes, a_rook_takes],
            vec![h_rook_takes, quiet_king_move, a_rook_takes],
        ] {
            let mut moves = buffer.clone();
            let until = moves.len();
            assert!(try_best_noisy_move(&mut moves, 0, until, &board, move_score));
            assert_eq!(moves[0], a_rook_takes, "buffer {buffer:?}");
        }
    }

    #[test]
    fn tie_key_separates_the_promotions_of_one_pawn() {
        let knight = Move::new(Square::B7, Square::B8, Some(PieceType::Knight));
        let queen = Move::new(Square::B7, Square::B8, Some(PieceType::Queen));
        let push = Move::new(Square::B6, Square::B7, None);
        assert_ne!(tie_key(knight), tie_key(queen));
        assert!(tie_key(push) < tie_key(knight), "an earlier from square comes first");
    }

    #[test]
    fn see_correctly_demotes_a_losing_capture_that_mvv_lva_wrongly_treats_as_good() {
        // White queen takes a pawn defended by another pawn: a clean net loss (queen for a
        // pawn once recaptured), but MVV-LVA's crude `10 * victim - attacker` formula still
        // scores it strictly positive (`10*100 - 900 = 100`) -- exactly the gap real SEE closes.
        // No other capture is available, so this isolates the one move's own scoring difference
        // rather than a ranking-among-several comparison.
        let (board, _) = Board::from_fen("4k3/8/2p5/3p4/8/8/8/3QK3 w - - 0 1").unwrap();
        let queen_takes_defended_pawn = Move::new(Square::D1, Square::D5, None);

        assert!(move_score(&board, queen_takes_defended_pawn) > 0);
        assert!(!see_ge(&board, queen_takes_defended_pawn, 0));
    }

    #[test]
    fn try_best_noisy_move_with_see_demotes_the_same_losing_capture_move_score_would_have_promoted() {
        let (board, _) = Board::from_fen("4k3/8/2p5/3p4/8/8/8/3QK3 w - - 0 1").unwrap();
        let queen_takes_defended_pawn = Move::new(Square::D1, Square::D5, None);
        let quiet_king_move = Move::new(Square::E1, Square::F1, None);

        let mut with_move_score = vec![quiet_king_move, queen_takes_defended_pawn];
        let until = with_move_score.len();
        assert!(try_best_noisy_move(&mut with_move_score, 0, until, &board, move_score));

        let mut with_see = vec![quiet_king_move, queen_takes_defended_pawn];
        assert!(!try_best_noisy_move(&mut with_see, 0, until, &board, see));
        assert_eq!(with_see, vec![quiet_king_move, queen_takes_defended_pawn], "expected no reordering");
    }

    #[test]
    fn history_score_reads_the_recorded_bonus_for_the_moving_piece_and_destination() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mut history = [[0; 64]; 12];
        let king = Piece::new(Color::White, PieceType::King);
        history[king as usize][Square::F1 as usize] = 42;
        assert_eq!(history_score(&history, &board, Move::new(Square::E1, Square::F1, None)), 42);
        assert_eq!(history_score(&history, &board, Move::new(Square::E1, Square::D1, None)), 0);
    }

    #[test]
    fn continuation_history_index_stays_within_bounds_and_distinguishes_every_axis() {
        let a = Piece::new(Color::White, PieceType::King);
        let b = Piece::new(Color::Black, PieceType::Queen);
        let indices = [
            continuation_history_index(a, Square::A1, a, Square::A1),
            continuation_history_index(b, Square::A1, a, Square::A1),
            continuation_history_index(a, Square::H8, a, Square::A1),
            continuation_history_index(a, Square::A1, b, Square::A1),
            continuation_history_index(a, Square::A1, a, Square::H8),
        ];
        for &index in &indices {
            assert!(index < CONTINUATION_HISTORY_LEN);
        }
        // Every one of the five differs from every other -- changing any single one of the four
        // axes (prev_piece, prev_to, piece, to) has to land on a genuinely different bucket, or
        // two unrelated (prev_move, move) pairs would silently share the same score.
        for i in 0..indices.len() {
            for j in (i + 1)..indices.len() {
                assert_ne!(indices[i], indices[j], "indices {i} and {j} collided");
            }
        }
    }

    #[test]
    fn apply_history_bonus_applies_the_bonus_unshrunk_from_a_fresh_entry() {
        // At `entry == 0`, gravity's decay term (`entry * bonus.abs() / max`) is itself `0`, so a
        // bonus to a never-touched bucket lands exactly as-is -- gravity only ever shrinks later
        // updates, once there's something in the bucket to pull against.
        let mut entry = 0;
        apply_history_bonus(&mut entry, 100, 1000);
        assert_eq!(entry, 100);
    }

    #[test]
    fn apply_history_bonus_converges_to_the_ceiling_without_ever_overshooting_it() {
        // Repeatedly applying the maximum possible bonus should still leave `entry` sitting at
        // exactly `max`, never past it -- the whole point of gravity over a bare `+=`.
        let mut entry = 900;
        apply_history_bonus(&mut entry, 1000, 1000);
        assert_eq!(entry, 1000);
        apply_history_bonus(&mut entry, 1000, 1000);
        assert_eq!(entry, 1000, "a saturated entry must not climb past max on a repeat bonus");
    }

    #[test]
    fn apply_history_bonus_clamps_an_oversized_bonus_to_max_before_applying_it() {
        // A single event's raw bonus can exceed `max` outright (Fattorini's own flat-history bonus
        // does, at deep plies: see `FLAT_HISTORY_MAX`'s own doc comment) -- it must still behave
        // exactly as if `max` itself had been passed, not overshoot in one step.
        let mut clamped = 0;
        apply_history_bonus(&mut clamped, 5000, 1000);
        let mut at_max = 0;
        apply_history_bonus(&mut at_max, 1000, 1000);
        assert_eq!(clamped, at_max);
    }

    #[test]
    fn apply_history_bonus_malus_pulls_a_populated_entry_down_faster_than_a_bare_subtraction_would() {
        // Gravity's decay term also applies to a malus: the further `entry` already sits from
        // `0`, the more a same-sized malus pulls it down, past what a plain `-=` would have done --
        // exactly the "a saturated entry is easier to correct" property gravity is meant to give.
        let mut entry = 500;
        apply_history_bonus(&mut entry, -200, 1000);
        assert_eq!(entry, 200, "expected the malus to overshoot a bare -200, not just apply it");
    }

    #[test]
    fn combined_history_score_is_flat_history_alone_with_no_previous_move() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mut history = [[0; 64]; 12];
        let king = Piece::new(Color::White, PieceType::King);
        history[king as usize][Square::F1 as usize] = 42;
        let continuation_history = vec![0; CONTINUATION_HISTORY_LEN];
        assert_eq!(
            combined_history_score(&history, &continuation_history, None, &board, Move::new(Square::E1, Square::F1, None)),
            42
        );
    }

    #[test]
    fn combined_history_score_adds_the_continuation_bonus_for_the_given_previous_move() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mut history = [[0; 64]; 12];
        let king = Piece::new(Color::White, PieceType::King);
        history[king as usize][Square::F1 as usize] = 42;
        let mut continuation_history = vec![0; CONTINUATION_HISTORY_LEN];
        let black_queen = Piece::new(Color::Black, PieceType::Queen);
        let index = continuation_history_index(black_queen, Square::D8, king, Square::F1);
        continuation_history[index] = 100;
        let prev_move = Some((black_queen, Square::D8));
        assert_eq!(
            combined_history_score(&history, &continuation_history, prev_move, &board, Move::new(Square::E1, Square::F1, None)),
            142
        );
    }

    #[test]
    fn cutoffs_record_a_quiet_move_as_that_depths_killer() {
        // The same starting position the pruning-benefit test above already established
        // produces real internal cutoffs at depth 3 (fewer nodes than negamax) -- reused
        // here rather than hand-crafting a new position, since what matters is just "did
        // some node actually record a killer," not which specific move. Note the root call
        // itself can never be the one that cuts off (full window, see that test's own
        // comment), so any recorded killer has to come from a node *inside* the tree --
        // exactly the mechanism this test is checking for.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = Search {
            moves: Vec::new(),
            nodes: 0,
            seldepth: 0,
            killers: vec![[None, None]; MAX_PLY as usize],
            history: [[0; 64]; 12],
            continuation_history: vec![0; CONTINUATION_HISTORY_LEN],
            prev_move: vec![None; MAX_PLY as usize + 1],
            static_evals: vec![None; MAX_PLY as usize + 1],
            pv: PvTable::new(),
            deadline: None,
            stopped: false,
            null_move_in_progress: false,
            stop_signal: &stop_signal,
            ponder: None,
            ancestors: Vec::new(),
            tt: &tt,
            tablebase: None,
            evaluator: None,
            accumulators: Vec::new(),
            pawn_percent: 100,
            parameters: SearchParameters::DEFAULT,
            eval_margins: eval_margins(&SearchParameters::DEFAULT, 100),
            excluded_root_moves: Vec::new(),
            scores: Vec::new(),
            quiets: Vec::new(),
            correction_history: vec![0; CORRECTION_HISTORY_LEN],
        };
        search.alpha_beta_with_buffer(&board, 0, 3, -INFINITY, INFINITY);
        assert!(
            search.killers.iter().any(|slots| slots.iter().any(Option::is_some)),
            "expected at least one depth to record a killer"
        );
    }

    #[test]
    fn cutoffs_record_a_history_bonus_for_the_cutoff_causing_move() {
        // Same position and depth as the killer version above, which already confirmed a real
        // internal cutoff happens here -- the history bonus is recorded at the exact same site,
        // off the exact same event, so this just confirms the second table actually gets
        // written too, not a different mechanism.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = Search {
            moves: Vec::new(),
            nodes: 0,
            seldepth: 0,
            killers: vec![[None, None]; MAX_PLY as usize],
            history: [[0; 64]; 12],
            continuation_history: vec![0; CONTINUATION_HISTORY_LEN],
            prev_move: vec![None; MAX_PLY as usize + 1],
            static_evals: vec![None; MAX_PLY as usize + 1],
            pv: PvTable::new(),
            deadline: None,
            stopped: false,
            null_move_in_progress: false,
            stop_signal: &stop_signal,
            ponder: None,
            ancestors: Vec::new(),
            tt: &tt,
            tablebase: None,
            evaluator: None,
            accumulators: Vec::new(),
            pawn_percent: 100,
            parameters: SearchParameters::DEFAULT,
            eval_margins: eval_margins(&SearchParameters::DEFAULT, 100),
            excluded_root_moves: Vec::new(),
            scores: Vec::new(),
            quiets: Vec::new(),
            correction_history: vec![0; CORRECTION_HISTORY_LEN],
        };
        search.alpha_beta_with_buffer(&board, 0, 3, -INFINITY, INFINITY);
        assert!(
            search.history.iter().any(|row| row.iter().any(|&score| score > 0)),
            "expected at least one (piece, square) to record a history bonus"
        );
    }

    #[test]
    fn cutoffs_record_a_continuation_history_bonus_for_the_cutoff_causing_move() {
        // Identical setup to the flat history bonus test above -- the same real internal cutoff
        // (at some ply > 0, never the root itself) writes into both tables at once, off the same
        // event, so this just confirms the second table genuinely gets written too.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = Search {
            moves: Vec::new(),
            nodes: 0,
            seldepth: 0,
            killers: vec![[None, None]; MAX_PLY as usize],
            history: [[0; 64]; 12],
            continuation_history: vec![0; CONTINUATION_HISTORY_LEN],
            prev_move: vec![None; MAX_PLY as usize + 1],
            static_evals: vec![None; MAX_PLY as usize + 1],
            pv: PvTable::new(),
            deadline: None,
            stopped: false,
            null_move_in_progress: false,
            stop_signal: &stop_signal,
            ponder: None,
            ancestors: Vec::new(),
            tt: &tt,
            tablebase: None,
            evaluator: None,
            accumulators: Vec::new(),
            pawn_percent: 100,
            parameters: SearchParameters::DEFAULT,
            eval_margins: eval_margins(&SearchParameters::DEFAULT, 100),
            excluded_root_moves: Vec::new(),
            scores: Vec::new(),
            quiets: Vec::new(),
            correction_history: vec![0; CORRECTION_HISTORY_LEN],
        };
        search.alpha_beta_with_buffer(&board, 0, 3, -INFINITY, INFINITY);
        assert!(
            search.continuation_history.iter().any(|&score| score > 0),
            "expected at least one (prev_piece, prev_to, piece, to) to record a continuation history bonus"
        );
    }

    #[test]
    fn the_root_never_gets_a_previous_move_to_condition_continuation_history_on() {
        // `self.prev_move[0]` has no parent call to ever set it -- only a recursive call sets
        // `prev_move[ply + 1]`, and the root is never anyone's child within this search. Checked
        // directly after a real search, rather than trying to engineer a root-level cutoff
        // specifically (the root's own full-window search structurally can't produce one, per
        // `cutoffs_record_a_history_bonus_for_the_cutoff_causing_move`'s own comment) -- this is
        // the actual invariant that guarantees the root can never contaminate continuation
        // history, not an indirect proxy for it.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.alpha_beta_with_buffer(&board, 0, 3, -INFINITY, INFINITY);
        assert_eq!(search.prev_move[0], None);
    }

    #[test]
    fn cutoffs_record_a_history_malus_for_a_quiet_move_tried_and_beaten_by_a_later_cutoff() {
        // Same position as the bonus test above, but deliberately deeper (6, not 3): malus needs
        // a strictly rarer event than bonus does -- some node where at least two quiet moves are
        // *fully searched* (not skipped outright by futility/late-move pruning), the first fails
        // to cut off, and a later one does. At depth 3, killers/history already promote the
        // eventual cutoff-causer to the front often enough, and futility pruning/reverse futility
        // pruning/razoring/late move reductions all thin out how many quiet moves ever get a real
        // look at such shallow remaining depths, that this specific scenario doesn't reliably
        // arise anywhere in so small a tree -- confirmed by this test failing at depth 3 even
        // though the bonus/killer tests both pass there. A larger tree gives it real room to
        // occur somewhere.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = Search {
            moves: Vec::new(),
            nodes: 0,
            seldepth: 0,
            killers: vec![[None, None]; MAX_PLY as usize],
            history: [[0; 64]; 12],
            continuation_history: vec![0; CONTINUATION_HISTORY_LEN],
            prev_move: vec![None; MAX_PLY as usize + 1],
            static_evals: vec![None; MAX_PLY as usize + 1],
            pv: PvTable::new(),
            deadline: None,
            stopped: false,
            null_move_in_progress: false,
            stop_signal: &stop_signal,
            ponder: None,
            ancestors: Vec::new(),
            tt: &tt,
            tablebase: None,
            evaluator: None,
            accumulators: Vec::new(),
            pawn_percent: 100,
            parameters: SearchParameters::DEFAULT,
            eval_margins: eval_margins(&SearchParameters::DEFAULT, 100),
            excluded_root_moves: Vec::new(),
            scores: Vec::new(),
            quiets: Vec::new(),
            correction_history: vec![0; CORRECTION_HISTORY_LEN],
        };
        search.alpha_beta_with_buffer(&board, 0, 6, -INFINITY, INFINITY);
        assert!(
            search.history.iter().any(|row| row.iter().any(|&score| score < 0)),
            "expected at least one (piece, square) to record a history malus"
        );
    }

    /// A `Search` with nothing killer/ordering/PV-related actually needed -- `quiescence`
    /// never touches `killers`/`pv`, so these fields are just present to
    /// satisfy the struct, not exercised by the tests below.
    fn quiescence_search<'a>(tt: &'a TranspositionTable, stop_signal: &'a AtomicBool) -> Search<'a> {
        Search {
            moves: Vec::new(),
            nodes: 0,
            seldepth: 0,
            killers: Vec::new(),
            history: [[0; 64]; 12],
            continuation_history: vec![0; CONTINUATION_HISTORY_LEN],
            prev_move: vec![None; MAX_PLY as usize + 1],
            static_evals: Vec::new(),
            pv: PvTable::new(),
            deadline: None,
            stopped: false,
            null_move_in_progress: false,
            stop_signal,
            ponder: None,
            ancestors: Vec::new(),
            tt,
            tablebase: None,
            evaluator: None,
            accumulators: Vec::new(),
            pawn_percent: 100,
            parameters: SearchParameters::DEFAULT,
            eval_margins: eval_margins(&SearchParameters::DEFAULT, 100),
            excluded_root_moves: Vec::new(),
            scores: Vec::new(),
            quiets: Vec::new(),
            correction_history: vec![0; CORRECTION_HISTORY_LEN],
        }
    }

    #[test]
    fn quiescence_returns_stand_pat_when_nothing_noisy_is_available() {
        // Bare kings -- every legal move is a quiet king move, so the loop skips all of them
        // and stand pat (plain material eval, `0` here) is all there is to return. `nodes ==
        // 1` confirms no recursion happened at all, not just that the final score matches.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        assert_eq!(search.quiescence(&board, 0, -INFINITY, INFINITY, None), 0);
        assert_eq!(search.nodes, 1);
    }

    #[test]
    fn quiescence_recurses_through_a_favorable_capture_that_plain_evaluation_misses() {
        // White queen takes a free, undefended rook (d4-d8, clear file, no recapture -- the
        // black king on a6 is too far away to matter). Plain evaluation only sees what's on
        // the board *before* the capture; quiescence should find that actually taking the
        // free rook scores strictly better than standing pat on the pre-capture position --
        // true regardless of exactly what the material/mobility/king-safety weights are
        // worth, unlike a hand-derived exact score (which would need re-deriving by hand
        // every time TDLeaf(lambda) retunes any of them -- see eval-tuning.md).
        let (board, _) = Board::from_fen("3r4/8/k7/8/3Q4/8/8/4K3 w - - 0 1").unwrap();
        let stand_pat = evaluate(&board);

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        assert!(search.quiescence(&board, 0, -INFINITY, INFINITY, None) > stand_pat);
    }

    #[test]
    fn quiescence_stand_pat_cutoff_skips_searching_any_move_at_all() {
        // Same position as above -- a hugely favorable capture exists -- but `beta` is set
        // to stand pat's own value (computed directly via `evaluate`, not a hand-derived
        // magic number), which already triggers the cutoff before a single move is
        // generated. `nodes == 1` confirms the capture was never even looked at, not merely
        // that it lost a comparison after being tried.
        let (board, _) = Board::from_fen("3r4/8/k7/8/3Q4/8/8/4K3 w - - 0 1").unwrap();
        let stand_pat = evaluate(&board);
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        assert_eq!(search.quiescence(&board, 0, -INFINITY, stand_pat, None), stand_pat);
        assert_eq!(search.nodes, 1);
    }

    #[test]
    fn quiescence_delta_prunes_a_hopeless_small_capture_without_recursing() {
        // A single pawn capture (e2xd3), otherwise undefended: SEE would happily pass it
        // (wins a clean pawn, no recapture), but `alpha` is set to exactly the best case this
        // capture could ever produce (stand pat plus the captured pawn plus the margin) --
        // simulating "already winning big elsewhere in the tree." It must be skipped outright:
        // `nodes == 1` confirms it was never even made, not merely that recursing into it lost
        // a comparison.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/3p4/4P3/4K3 w - - 0 1").unwrap();
        let stand_pat = evaluate(&board);
        let alpha = stand_pat + piece_value(PieceType::Pawn) + SearchParameters::DEFAULT.delta_margin;

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        assert_eq!(search.quiescence(&board, 0, alpha, INFINITY, None), stand_pat);
        assert_eq!(search.nodes, 1);
    }

    #[test]
    fn quiescence_does_not_delta_prune_a_capture_that_could_still_reach_alpha() {
        // Same position, `alpha` one point below the exact threshold above -- the capture's
        // best case now clears it, so it must actually be searched: `nodes` grows past the
        // parent call alone, confirming the child recursion really happened.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/3p4/4P3/4K3 w - - 0 1").unwrap();
        let stand_pat = evaluate(&board);
        let alpha = stand_pat + piece_value(PieceType::Pawn) + SearchParameters::DEFAULT.delta_margin - 1;

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        search.quiescence(&board, 0, alpha, INFINITY, None);
        assert!(search.nodes > 1);
    }

    #[test]
    fn quiescence_move_count_cap_stops_searching_further_ordinary_captures() {
        // White king b1, rooks on c1/e1/g1, each able to capture an undefended Black pawn on
        // an otherwise open file (c7/e7/g7) -- three simultaneously available, individually
        // SEE-safe, non-promotion, non-check-giving captures, one more than
        // `QUIESCENCE_MOVE_LIMIT` (`2`). Black's own reply (a bare king on a8, too far to
        // capture anything) has no noisy move of its own, so every capture actually made
        // contributes exactly one further node -- turning "only `QUIESCENCE_MOVE_LIMIT` of the
        // three are ever tried" into a plain node-count assertion, the same technique the
        // killer-moves investigation already established: `nodes == 1` (the root call) `+
        // QUIESCENCE_MOVE_LIMIT` (one per capture actually made), not `+ 3`.
        let (board, _) = Board::from_fen("k7/2p1p1p1/8/8/8/8/8/1KR1R1R1 w - - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        search.quiescence(&board, 0, -INFINITY, INFINITY, None);
        assert_eq!(search.nodes, 1 + QUIESCENCE_MOVE_LIMIT as u64);
    }

    #[test]
    fn quiescence_move_count_cap_does_not_apply_when_in_check() {
        // Same position `quiescence_searches_quiet_evasions_when_in_check` uses -- White king
        // e1, in check from the Black rook on e8, four legal quiet evasions (d1/d2/f1/f2) --
        // one more than that test needed to confirm, but exactly enough to also confirm the
        // move-count cap itself never applies here: `!in_check` gates the cap's own exemption
        // check, so every evasion stays mandatory regardless of count. Same node-count
        // reasoning as the capture-cap test above (each evasion leaves Black, a bare king too
        // far to capture anything, with nothing noisy to search, so each contributes exactly
        // one further node): `nodes == 1 + 4`, not capped at `1 + QUIESCENCE_MOVE_LIMIT`.
        let (board, _) = Board::from_fen("k3r3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        search.quiescence(&board, 0, -INFINITY, INFINITY, None);
        assert_eq!(search.nodes, 1 + 4);
    }

    #[test]
    fn quiescence_tries_the_tt_suggested_move_before_mvv_lva_ordering() {
        // White has two noisy candidates: c3xd4 (pawn takes queen, MVV-LVA score `10*900-100
        // = 8900`, the overwhelming natural first pick) and d6xf7+ (knight takes a mere pawn,
        // MVV-LVA score only `10*100-320 = 680`) -- but the knight capture also delivers
        // checkmate (Black's king on h8 is boxed in by its own rook on g8 and pawns g7/h7, and
        // a knight check can never be blocked). Neither MVV-LVA nor SEE can see that: both only
        // evaluate the immediate exchange on the capture square itself, never a consequence on
        // a different square entirely -- exactly the situation a stored TT best move is meant
        // to help with, standing in here for whatever prior search actually found it.
        let (board, _) = Board::from_fen("6rk/5ppp/3N4/8/3q4/2P5/8/K7 w - - 0 1").unwrap();

        // Strictly between "wins a queen" (the pawn capture's own ordinary outcome, nowhere
        // near this magnitude even accounting for positional swings) and "delivers mate" (near
        // `MATE_SCORE`) -- a generous `5x` multiple of a bare queen's value, not a tightly
        // hand-tuned number, precisely so this doesn't need to know the exact positional
        // contribution of `evaluate`'s own PST/mobility/king-safety terms.
        let beta = piece_value(PieceType::Queen) * 5;

        let tt = TranspositionTable::new(1);
        // Manually seeds the TT with the knight move as this position's own already-known best
        // move -- standing in for whatever earlier, deeper search (or prior iterative-deepening
        // iteration) would have stored this for real; `Bound::Upper`/score `0` deliberately
        // doesn't itself trigger a cutoff (`0 <= alpha` is false against `-INFINITY`), so this
        // only ever supplies the ordering hint, not an outright resolved score.
        let mv_knight = Move::new(Square::D6, Square::F7, None);
        tt.store(board.hash(), mv_knight, 0, 0, Bound::Upper);

        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        let score = search.quiescence(&board, 0, -INFINITY, beta, None);

        // The real proof is the node count, not just the final score: if the pawn-takes-queen
        // move were tried first instead (natural MVV-LVA order, ignoring the TT hint), it would
        // score nowhere near `beta` and not cut off, forcing the knight's own mate-detecting
        // branch to be tried next regardless -- `nodes` would be `1 + 1 (queen branch) + 1
        // (knight branch) = 3`. With the TT hint honored, the knight move is tried *first*,
        // immediately clears `beta`, and the pawn-takes-queen move is never tried at all:
        // `nodes == 1 + 1`, not `1 + 2`.
        assert!(score > beta, "expected the mate score to clear beta");
        assert_eq!(search.nodes, 1 + 1);
    }

    #[test]
    fn quiescence_ignores_a_quiet_tt_move_outside_of_check() {
        // A stored TT best move can be quiet even though quiescence's own not-in-check branch
        // only ever wants noisy candidates -- `self.tt` is shared with the main search, which
        // routinely stores quiet best moves. If the TT-move stage's own `!is_quiet` gate were
        // missing or wrong, this quiet move would get swapped to the front and then reach delta
        // pruning's own `captured_piece(...).expect(...)`, which panics on a non-capture -- this
        // test's real job is confirming that never happens, not just that the final score looks
        // right, the same way `quiescence_recurses_through_a_favorable_capture_that_plain_
        // evaluation_misses` confirms a real capture still gets found.
        let (board, _) = Board::from_fen("4k3/p7/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
        let stand_pat = evaluate(&board);

        let tt = TranspositionTable::new(1);
        let quiet_king_move = Move::new(Square::E1, Square::E2, None);
        tt.store(board.hash(), quiet_king_move, 0, 0, Bound::Upper);

        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        let score = search.quiescence(&board, 0, -INFINITY, INFINITY, None);
        // Rxa7 still gets found and searched despite the quiet TT hint -- if it had been
        // silently skipped instead (the failure mode a missing gate would produce, short of an
        // outright panic), this would fall back to a bare stand pat instead of beating it.
        assert!(score > stand_pat);
    }

    #[test]
    fn quiescence_checkmate_is_scored_by_ply() {
        // The same back-rank mate the main search's own mate tests use. In check, with no
        // legal evasion at all -- `found_legal_move` stays false, so this must hit the
        // dedicated mate branch rather than falling through to `best_score` (which would
        // still be sitting at its `i32::MIN` placeholder here, never having been overwritten).
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/5PPP/r5K1 w - - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        assert_eq!(search.quiescence(&board, 0, -INFINITY, INFINITY, None), -MATE_SCORE);
    }

    #[test]
    fn quiescence_searches_quiet_evasions_when_in_check() {
        // White king e1, in check along the open e-file from the black rook on e8. Every
        // legal response is a quiet king move (d1/d2/f1/f2) -- no captures exist at all. If
        // the in-check branch wrongly applied the not-in-check quiet-move filter, none of
        // these would ever be tried, `found_legal_move` would wrongly stay false, and this
        // would be misreported as checkmate (`-MATE_SCORE`, a magnitude no real material/
        // positional evaluation could ever come close to producing here). Asserting the
        // real score is nowhere near that sentinel confirms quiet evasions are actually
        // searched, without needing to know exactly what any of the four evasions' own
        // weights-dependent scores are.
        let (board, _) = Board::from_fen("k3r3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        assert!(search.quiescence(&board, 0, -INFINITY, INFINITY, None) > -MATE_SCORE);
        assert!(search.nodes > 1, "expected at least one evasion to actually be searched");
    }

    #[test]
    fn a_search_already_marked_stopped_returns_immediately_without_counting_a_node() {
        // Confirms the unconditional check really does come before `self.nodes += 1`, not
        // just that a stopped search eventually stops -- `nodes` staying at `0` proves this
        // call did no work at all, rather than doing one node's worth before bailing.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(4, None, Vec::new(), Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.stopped = true;

        search.alpha_beta_with_buffer(&board, 0, 4, -INFINITY, INFINITY);
        assert_eq!(search.nodes, 0);

        let qsearch_tt = TranspositionTable::new(1);
        let mut qsearch = quiescence_search(&qsearch_tt, &stop_signal);
        qsearch.stopped = true;
        qsearch.quiescence(&board, 0, -INFINITY, INFINITY, None);
        assert_eq!(qsearch.nodes, 0);
    }

    #[test]
    fn a_past_deadline_aborts_the_search_before_it_completes() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        // Depth 8, not the original 4 -- the deadline is only ever checked every
        // `CHECK_INTERVAL` (1024) nodes, so this test only means anything if the full,
        // uninterrupted search would visit comfortably more than that. Depth 4's own one-shot
        // (non-iterative, cold `killers`/`history`) node count from this exact position used to
        // safely clear it (2,507, the historical baseline this test once asserted directly) but
        // has since dropped under 1,024 as PVS, razoring, and late-move pruning all landed --
        // exactly the reason this asserted a hard node-count ceiling was itself the wrong thing
        // to lean on (same lesson `futility_pruning_never_skips_the_first_move_at_a_node`
        // already ran into). Depth 8 rather than a more marginal bump, for headroom against
        // this same problem recurring as pruning keeps improving.
        let mut search = new_search(
            8, Some(Instant::now() - Duration::from_secs(1)), Vec::new(), vec![[None, None]; MAX_PLY as usize],
            [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
        );

        search.alpha_beta_with_buffer(&board, 0, 8, -INFINITY, INFINITY);

        // Only `stopped` is checked now, not a node-count ceiling -- see above for why a hard
        // number here is exactly the fragile thing this test just got bitten by. `stopped`
        // being `true` already proves the periodic deadline check fired and caught the past
        // deadline; a full depth-8 search from the starting position finishing on its own
        // despite an already-expired deadline would be the real bug this test exists to catch.
        assert!(search.stopped);
    }

    #[test]
    fn time_limit_reads_the_side_to_moves_own_clock_under_fischer() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        // No increment sent means an increment of 0, and no movestogo means Fischer.
        let limit = time_limit(&board, Some(60_000), Some(999_999), None, Some(500), None, None, false, 0);
        assert_eq!(limit, Some(clock(60_000, 0, None, false)));
    }

    /// A clock limit with no previous score.
    fn clock(time_ms: u64, inc_ms: u64, moves_to_go: Option<u32>, single_move: bool) -> TimeLimit {
        TimeLimit::Clock { time_ms, inc_ms, moves_to_go, single_move, previous_score: None }
    }

    #[test]
    fn time_limit_selects_black_time_when_black_is_to_move() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1").unwrap();
        let limit = time_limit(&board, Some(999_999), Some(90_000), Some(7), Some(100), None, None, false, 0);
        assert_eq!(limit, Some(clock(90_000, 100, None, false)));
    }

    #[test]
    fn time_limit_passes_movestogo_through_under_a_repeating_control() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let limit = time_limit(&board, Some(60_000), None, None, None, Some(10), None, false, 0);
        assert_eq!(limit, Some(clock(60_000, 0, Some(10), false)));
    }

    /// White to move, in check from the queen on g2, with one legal move: `Kxg2`. The pawn keeps
    /// the game going after it.
    const ONE_LEGAL_MOVE: &str = "7k/8/8/8/8/8/1p4q1/7K w - - 0 1";

    #[test]
    fn a_single_legal_move_caps_every_kind_of_game_limit_and_nothing_else() {
        let (forced, _) = Board::from_fen(ONE_LEGAL_MOVE).unwrap();
        assert_eq!(crate::movegen::legal::legal_moves(&forced, forced.side_to_move()).len(), 1);
        let clock_limit = Some(clock(600_000, 0, None, false).with_previous_score(Some(30)));
        assert_eq!(
            single_move_limit(&forced, clock_limit),
            Some(clock(600_000, 0, None, true).with_previous_score(Some(30))),
            "the cap keeps the previous score"
        );
        assert_eq!(single_move_limit(&forced, Some(TimeLimit::Fixed { target_ms: 2_000 })), Some(TimeLimit::Fixed { target_ms: 500 }));
        assert_eq!(single_move_limit(&forced, Some(TimeLimit::Fixed { target_ms: 100 })), Some(TimeLimit::Fixed { target_ms: 100 }));
        assert_eq!(single_move_limit(&forced, None), Some(TimeLimit::Fixed { target_ms: 500 }), "go depth, no clock");

        let (start, _) = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        for limit in [clock_limit, Some(TimeLimit::Fixed { target_ms: 2_000 }), None] {
            assert_eq!(single_move_limit(&start, limit), limit, "more than one legal move: unchanged");
        }
    }

    #[test]
    fn the_single_move_cap_only_ever_lowers_the_clock_target() {
        let started = Instant::now();
        let long = TimeManager::new(Some(clock(600_000, 0, None, true)), started, 100);
        assert_eq!(long.target(), Some(Duration::from_millis(SINGLE_MOVE_CAP_MS)));
        // 400 ms left: `MAX_CLOCK_FRACTION` already gives at most 200, under the cap.
        let short = TimeManager::new(Some(clock(400, 0, None, true)), started, 100);
        let uncapped = TimeManager::new(Some(clock(400, 0, None, false)), started, 100);
        assert_eq!(short.target(), uncapped.target());
        assert!(short.target().unwrap() <= Duration::from_millis(200));
    }

    /// Self-play and replay call `choose_best_move` with no limit at all, never through
    /// `single_move_limit`: a forced position's label is still its full-depth score.
    #[test]
    fn a_search_with_no_limit_reaches_its_depth_on_a_single_legal_move() {
        let (board, _) = Board::from_fen(ONE_LEGAL_MOVE).unwrap();
        let (reports, best_move) = choose_best_move(
            &board, 6, None, &[], &TranspositionTable::new(1), &SyzygyTablebase::new(), None, 0, &AtomicBool::new(false),
        );
        assert_eq!(reports.last().map(|report| report.depth), Some(6));
        assert_eq!(format!("{best_move:?}"), "Some(Move { from: H1, to: G2, promotion: None, kind: Quiet })");
    }

    #[test]
    fn time_limit_movetime_takes_precedence_over_wtime_and_btime_and_is_used_as_given() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let limit = time_limit(&board, Some(60_000), Some(60_000), None, None, None, Some(2_000), false, 0);
        assert_eq!(limit, Some(TimeLimit::Fixed { target_ms: 2_000 }));
    }

    #[test]
    fn time_limit_is_none_when_infinite_is_set() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        // Even with a perfectly good wtime present, infinite wins -- meaning "no deadline, run
        // until stop or max_depth," genuinely honored now that go_search runs asynchronously
        // and polls a shared stop flag regardless of whether any deadline was ever set.
        assert_eq!(time_limit(&board, Some(60_000), None, None, None, None, None, true, 0), None);
    }

    #[test]
    fn time_limit_is_none_with_no_time_control_at_all() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        assert_eq!(time_limit(&board, None, None, None, None, None, None, false, 0), None);
    }

    #[test]
    fn time_limit_is_none_when_only_the_opponents_clock_is_given() {
        // White to move, but only Black's remaining time was sent -- there's nothing to
        // plan White's own move from, so this degrades to "no limit," the same as a
        // bare `go`, rather than guessing from the wrong side's clock.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        assert_eq!(time_limit(&board, None, Some(60_000), None, None, None, None, false, 0), None);
    }

    #[test]
    fn the_move_overhead_is_kept_back_twice_from_a_fischer_clock() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let limit = time_limit(&board, Some(60_000), None, Some(1_000), None, None, None, false, 20);
        assert_eq!(limit, Some(clock(59_960, 1_000, None, false)), "the increment is left alone");
    }

    #[test]
    fn the_move_overhead_is_kept_back_once_per_move_to_go_and_twice_more() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let limit = time_limit(&board, Some(60_000), None, None, None, Some(10), None, false, 20);
        assert_eq!(limit, Some(clock(60_000 - 12 * 20, 0, Some(10), false)));
        // `movestogo 0` is not valid UCI; read as 1, as `clock_target_ms` reads it.
        let limit = time_limit(&board, Some(60_000), None, None, None, Some(0), None, false, 20);
        assert_eq!(limit, Some(clock(60_000 - 3 * 20, 0, Some(0), false)));
    }

    #[test]
    fn the_move_overhead_is_kept_back_from_movetime() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let limit = time_limit(&board, None, None, None, None, None, Some(2_000), false, 20);
        assert_eq!(limit, Some(TimeLimit::Fixed { target_ms: 1_980 }));
        let limit = time_limit(&board, None, None, None, None, None, Some(10), false, 20);
        assert_eq!(limit, Some(TimeLimit::Fixed { target_ms: 0 }), "depth 1 only, as movetime 0");
    }

    #[test]
    fn a_clock_below_the_reserve_still_plays_a_legal_move() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let limit = time_limit(&board, Some(30), None, None, None, None, None, false, 20);
        assert_eq!(limit, Some(clock(0, 0, None, false)));
        let (reports, best_move) = choose_best_move(
            &board, MAX_PLY as u32, limit, &[], &TranspositionTable::new(1), &SyzygyTablebase::new(), None, 0,
            &AtomicBool::new(false),
        );
        assert!(best_move.is_some(), "depth 1 still answers when the reserve takes the whole clock");
        assert_eq!(reports.last().map(|report| report.depth), Some(1), "and nothing deeper starts");
    }

    #[test]
    fn clock_target_at_three_plus_two_spans_one_to_three_times_the_stable_target() {
        // (0.05 * 180,000 + 0.5 * 2,000) * (1 + 2 * C).
        assert_eq!(clock_target_ms(180_000, 2_000, None, 0.0, 1.0, &TIME_PARAMETERS), 10_000);
        assert_eq!(clock_target_ms(180_000, 2_000, None, 1.0, 1.0, &TIME_PARAMETERS), 30_000);
    }

    #[test]
    fn clock_target_is_capped_at_half_the_clock_in_time_trouble() {
        // Uncapped: (0.05 * 1,000 + 0.5 * 2,000) * 3 = 3,150.
        assert_eq!(clock_target_ms(1_000, 2_000, None, 1.0, 1.0, &TIME_PARAMETERS), 500);
    }

    #[test]
    fn clock_target_under_a_repeating_control_grows_as_the_refill_nears() {
        // 40/15: (0.05 + 1/40) * 900,000 = 67,500; on the last move before the refill the uncapped
        // (0.05 + 1) * 900,000 exceeds the whole clock, so the cap decides.
        assert_eq!(clock_target_ms(900_000, 0, Some(40), 0.0, 1.0, &TIME_PARAMETERS), 67_500);
        assert_eq!(clock_target_ms(900_000, 0, Some(1), 0.0, 1.0, &TIME_PARAMETERS), 450_000);
    }

    #[test]
    fn clock_target_reads_movestogo_zero_as_one() {
        assert_eq!(
            clock_target_ms(900_000, 0, Some(0), 0.0, 1.0, &TIME_PARAMETERS),
            clock_target_ms(900_000, 0, Some(1), 0.0, 1.0, &TIME_PARAMETERS),
        );
    }

    #[test]
    fn the_falling_score_factor_multiplies_the_target_before_the_cap() {
        // 10,000 at 3+2 and C = 0 (above), times F.
        assert_eq!(clock_target_ms(180_000, 2_000, None, 0.0, 1.7, &TIME_PARAMETERS), 17_000);
        assert_eq!(clock_target_ms(180_000, 2_000, None, 0.0, 0.6, &TIME_PARAMETERS), 6_000);
        assert_eq!(clock_target_ms(1_000, 2_000, None, 1.0, 1.7, &TIME_PARAMETERS), 500, "still capped");
    }

    #[test]
    fn a_falling_score_gives_more_time_and_a_rising_one_less() {
        assert_eq!(falling_score_factor(None, -300, 100), 1.0, "no previous move");
        assert_eq!(falling_score_factor(Some(40), 40, 100), 1.0);
        assert!((falling_score_factor(Some(40), -27, 100) - 1.335).abs() < 1e-9, "a third of a pawn and more");
        assert!((falling_score_factor(Some(0), -66, 100) - 1.33).abs() < 1e-9);
        assert_eq!(falling_score_factor(Some(0), -400, 100), FALLING_SCORE_MAX);
        assert_eq!(falling_score_factor(Some(0), 200, 100), FALLING_SCORE_MIN);
        assert!((falling_score_factor(Some(0), 50, 100) - 0.75).abs() < 1e-9);
    }

    #[test]
    fn a_mate_or_tablebase_score_on_either_side_leaves_the_target_alone() {
        for decisive in [mate_in(5), -mate_in(5), tb_win_at(3), tb_loss_at(3)] {
            assert_eq!(falling_score_factor(Some(decisive), 0, 100), 1.0);
            assert_eq!(falling_score_factor(Some(0), decisive, 100), 1.0);
        }
    }

    #[test]
    fn a_root_the_tables_rank_reports_their_result_over_the_networks_score() {
        // The KQ vs KN that showed +1.59: the win replaces an ordinary score, and its mirror.
        assert_eq!(root_reported_score(159, Some(TbResult::Win)), tb_win_at(0));
        assert_eq!(root_reported_score(-159, Some(TbResult::Win)), tb_win_at(0));
        assert_eq!(root_reported_score(-159, Some(TbResult::Loss)), tb_loss_at(0));
        // A draw, cursed wins and blessed losses included, is 0 whatever the search thought.
        for score in [80, -80, 0, tb_win_at(2), -mate_in(9)] {
            assert_eq!(root_reported_score(score, Some(TbResult::Draw)), 0, "{score}");
        }
    }

    #[test]
    fn a_decisive_search_score_agreeing_with_the_tables_is_kept_and_a_disagreeing_one_replaced() {
        // A mate or a capture into the tables says at least as much as the root's result.
        for kept in [mate_in(7), tb_win_at(4)] {
            assert_eq!(root_reported_score(kept, Some(TbResult::Win)), kept, "{kept}");
            assert_eq!(root_reported_score(-kept, Some(TbResult::Loss)), -kept, "{kept}");
        }
        // The wrong sign is the search's mistake, not news: the tables win.
        assert_eq!(root_reported_score(-mate_in(7), Some(TbResult::Win)), tb_win_at(0));
        assert_eq!(root_reported_score(tb_win_at(4), Some(TbResult::Loss)), tb_loss_at(0));
    }

    #[test]
    fn outside_the_tables_the_search_score_is_reported_as_it_is() {
        for score in [0, 159, -320, mate_in(3), -mate_in(3), tb_win_at(2), tb_loss_at(2)] {
            assert_eq!(root_reported_score(score, None), score, "{score}");
        }
    }

    #[test]
    fn the_same_real_drop_gives_the_same_factor_under_every_evaluator_pawn() {
        // A pawn of 70 PeSTO centipawns' worth: a drop of 70 there is a drop of 100 here.
        assert_eq!(falling_score_factor(Some(0), -70, 70), falling_score_factor(Some(0), -100, 100));
    }

    #[test]
    fn the_time_manager_recomputes_the_falling_factor_after_every_depth_under_a_clock_only() {
        let a = Move::new(Square::E2, Square::E4, None);
        let limit = clock(180_000, 2_000, None, false).with_previous_score(Some(0));
        let mut time_manager = TimeManager::new(Some(limit), Instant::now(), 100);
        assert_eq!(time_manager.falling, 1.0, "nothing searched yet");
        time_manager.record_iteration(a, -100);
        assert_eq!(time_manager.falling, 1.5);
        time_manager.record_iteration(a, 0);
        assert_eq!(time_manager.falling, 1.0);

        let mut fixed = TimeManager::new(Some(TimeLimit::Fixed { target_ms: 1_000 }), Instant::now(), 100);
        fixed.record_iteration(a, -1_000);
        assert_eq!(fixed.target(), Some(Duration::from_millis(1_000)), "movetime is used as given");
    }

    #[test]
    fn time_manager_instability_discounts_best_move_changes() {
        let a = Move::new(Square::E2, Square::E4, None);
        let b = Move::new(Square::D2, Square::D4, None);
        let mut time_manager = TimeManager::new(None, Instant::now(), 100);
        assert_eq!(time_manager.instability, 1.0);
        let mut observed = Vec::new();
        for best_move in [a, a, b, b, b] {
            time_manager.record_iteration(best_move, 0);
            observed.push(time_manager.instability);
        }
        assert_eq!(observed, vec![1.0, 0.5, 0.75, 0.375, 0.1875]);
    }

    #[test]
    fn search_root_picks_the_move_that_wins_more_material() {
        // Same position as `alpha_beta_depth_one_prefers_the_move_that_wins_more_material`
        // (including the second, a7 pawn -- see that test's comment for why it's there):
        // the only legal move that wins material is the knight capturing the c5 pawn.
        let (board, _) = Board::from_fen("4k3/p7/8/2p5/4N3/8/8/4K3 w - - 0 1").unwrap();
        let (pv, score, nodes, stopped, _seldepth, ..) =
            search_root(
                &board, 1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN],
                &TranspositionTable::new(1), &SyzygyTablebase::new(), None, None, 0, &AtomicBool::new(false), &[],
            );
        // Depth 1 -- the PV can only ever be one move long, no continuation possible.
        assert_eq!(pv, vec![Move::new(Square::E4, Square::C5, None)]);
        // Same evaluate()-of-the-actual-resulting-position pattern as
        // `alpha_beta_depth_one_prefers_the_move_that_wins_more_material`, not a hand-
        // derived magic number -- see that test's comment for why.
        let mut after_capture = board;
        after_capture.make_move(Move::new(Square::E4, Square::C5, None));
        assert_eq!(score, -evaluate(&after_capture));
        assert!(nodes > 0);
        assert!(!stopped, "a search with no deadline at all should never report stopped");
    }

    #[test]
    fn search_root_excludes_a_root_move_entirely_letting_the_next_best_one_win_instead() {
        // Same fixture as search_root_picks_the_move_that_wins_more_material: the only legal move
        // that wins material is the knight capturing the c5 pawn, so it's the unique best move at
        // depth 1. Excluding it must make search_root return some other, strictly worse move --
        // proving excluded_root_moves really does remove a move from consideration entirely,
        // rather than merely deprioritizing it or leaving it reachable some other way.
        let (board, _) = Board::from_fen("4k3/p7/8/2p5/4N3/8/8/4K3 w - - 0 1").unwrap();
        let winning_capture = Move::new(Square::E4, Square::C5, None);

        let (best_pv, best_score, ..) = search_root(
            &board, 1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN],
            &TranspositionTable::new(1), &SyzygyTablebase::new(), None, None, 0, &AtomicBool::new(false), &[],
        );
        assert_eq!(best_pv, vec![winning_capture], "sanity check: same result the unexcluded test above expects");

        let (excluded_pv, excluded_score, ..) = search_root(
            &board, 1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN],
            &TranspositionTable::new(1), &SyzygyTablebase::new(), None, None, 0, &AtomicBool::new(false), &[winning_capture],
        );
        assert!(!excluded_pv.is_empty(), "the root has other legal moves and must still report one");
        assert_ne!(excluded_pv[0], winning_capture, "the excluded move must never be chosen");
        assert!(
            excluded_score < best_score,
            "excluding the unique best move must yield a strictly worse score: excluded_score = {excluded_score}, best_score = {best_score}"
        );
    }

    #[test]
    fn a_deep_tt_hit_at_the_root_still_produces_a_real_move_not_an_empty_pv() {
        // A real position from an actual Fattorini against Stockfish game (cutechess-cli, 5+3):
        // 1.d4 d5 2.Qd3 c5 3.dxc5 Nf6 4.e4 Nxe4 5.Nc3 Nxc3, White to move. Fattorini reported an
        // illegal `bestmove 0000` here even though White has plenty of legal replies (bxc3,
        // Qxc3, and everything else -- White isn't even in check) -- root-caused to the TT
        // cutoff above firing at `ply == 0` itself, whenever this exact position already had
        // a sufficiently-deep entry (realistic mid-game: the same position can easily have
        // been reached as a transposition inside an earlier move's own deeper search tree).
        // Reproduced directly here rather than trusting a real game to hit it again: prime
        // the table with an artificially deep, Exact-bound entry for this exact hash first.
        let (board, _) =
            Board::from_fen("rnbqkb1r/pp2pppp/8/2Pp4/8/2nQ4/PPP2PPP/R1B1KBNR w KQkq - 0 6")
                .unwrap();
        let tt = TranspositionTable::new(1);
        tt.store(board.hash(), Move::new(Square::B2, Square::C3, None), 5, 0, Bound::Exact);

        let (pv, _score, _nodes, stopped, _seldepth, ..) = search_root(
            &board, 1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12],
            vec![0; CONTINUATION_HISTORY_LEN], &tt, &SyzygyTablebase::new(), None, None, 0, &AtomicBool::new(false), &[],
        );

        assert!(!stopped);
        assert!(!pv.is_empty(), "the root has legal replies (bxc3, Qxc3, ...) and must report one");
    }

    #[test]
    fn search_root_reports_an_empty_pv_at_a_terminal_root() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/5PPP/r5K1 w - - 0 1").unwrap();
        let (pv, _score, _nodes, stopped, _seldepth, ..) =
            search_root(
                &board, 3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN],
                &TranspositionTable::new(1), &SyzygyTablebase::new(), None, None, 0, &AtomicBool::new(false), &[],
            );
        assert!(pv.is_empty());
        assert!(!stopped);
    }

    #[test]
    fn search_root_pv_threads_a_real_continuation_up_from_deeper_plies() {
        // Same position and best first move as above, one ply deeper -- confirming the PV
        // isn't just the root's own move (that's already covered above) but actually carries
        // a real continuation built up from a deeper node's own line, not just truncated to
        // length 1. Which exact Black reply comes second isn't pinned down (every legal Black
        // king move ties under material-only evaluation), only that a real second move exists.
        // Same second, a7 pawn as the tests above, for the same reason: without it, Nxc5
        // leaves King+Knight vs bare King, which the insufficient-material check would score
        // as a dead draw rather than the material-winning line this test relies on.
        let (board, _) = Board::from_fen("4k3/p7/8/2p5/4N3/8/8/4K3 w - - 0 1").unwrap();
        let (pv, _score, _nodes, _stopped, _seldepth, ..) =
            search_root(
                &board, 2, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN],
                &TranspositionTable::new(1), &SyzygyTablebase::new(), None, None, 0, &AtomicBool::new(false), &[],
            );
        assert_eq!(pv.len(), 2, "expected a two-move PV, got {pv:?}");
        assert_eq!(pv[0], Move::new(Square::E4, Square::C5, None));
    }

    #[test]
    fn search_root_seldepth_exceeds_target_depth_when_quiescence_extends_further() {
        // White pawn e4 can capture on d5; Black pawn c6 can then recapture -- a genuine
        // multi-ply capture sequence reachable only through quiescence's own extension past
        // the main search's nominal horizon. Depth 1 hands off to quiescence right after
        // White's first move, so any further recursion visible in `seldepth` has to have come
        // from quiescence itself, not the main search.
        let (board, _) = Board::from_fen("4k3/8/2p5/3p4/4P3/8/8/4K3 w - - 0 1").unwrap();
        let (_pv, _score, _nodes, _stopped, seldepth, ..) =
            search_root(
                &board, 1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN],
                &TranspositionTable::new(1), &SyzygyTablebase::new(), None, None, 0, &AtomicBool::new(false), &[],
            );
        assert!(seldepth > 1, "expected quiescence to extend past target_depth 1, got seldepth {seldepth}");
    }

    #[test]
    fn search_root_seldepth_equals_target_depth_when_quiescence_never_extends() {
        // Pawns locked head to head on the e-file (no diagonal captures available to either
        // side) -- nothing for quiescence to chase, so every leaf stands pat immediately and
        // `seldepth` never exceeds the nominal `target_depth`. Deliberately not bare kings:
        // King vs King is itself one of the four insufficient-material patterns, which would
        // make the search stop at `ply == 0` before ever reaching `target_depth`, defeating
        // what this test means to check.
        let (board, _) = Board::from_fen("4k3/8/8/4p3/4P3/8/8/4K3 w - - 0 1").unwrap();
        let (_pv, _score, _nodes, _stopped, seldepth, ..) =
            search_root(
                &board, 2, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN],
                &TranspositionTable::new(1), &SyzygyTablebase::new(), None, None, 0, &AtomicBool::new(false), &[],
            );
        assert_eq!(seldepth, 2);
    }

    #[test]
    fn search_root_hands_back_a_populated_killers_table_after_a_real_search() {
        // Same starting position and depth the direct-construction test above
        // (`each_stage_of_the_priority_chain_records_a_killer`) already confirmed records at
        // least one killer -- `search_root` is a different call path to the same
        // `alpha_beta_with_buffer`, so this confirms the *plumbing* (the table really is
        // threaded through and handed back, not silently dropped), not the algorithm again.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let (.., killers, _history, _continuation_history, _correction_history) =
            search_root(
                &board, 3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN],
                &TranspositionTable::new(1), &SyzygyTablebase::new(), None, None, 0, &AtomicBool::new(false), &[],
            );
        assert!(
            killers.iter().any(|slots| slots.iter().any(Option::is_some)),
            "expected at least one ply to have recorded a killer"
        );
    }

    #[test]
    fn initial_aspiration_delta_keeps_thread_zero_at_the_plain_default() {
        // Thread 0 -- the only thread that exists today -- must reproduce exactly today's
        // behavior: no per-thread widening at all.
        let delta = SearchParameters::DEFAULT.aspiration_delta;
        assert_eq!(initial_aspiration_delta(delta, 0), delta);
    }

    #[test]
    fn initial_aspiration_delta_widens_by_one_step_per_thread_index() {
        let delta = SearchParameters::DEFAULT.aspiration_delta;
        for thread_idx in 0..128 {
            assert_eq!(initial_aspiration_delta(delta, thread_idx), delta + ASPIRATION_THREAD_STEP * thread_idx as i32);
        }
    }

    #[test]
    fn search_root_ignores_previous_score_below_the_aspiration_minimum_depth() {
        // A wildly wrong "previous" score -- if the gate below `ASPIRATION_MIN_DEPTH` didn't
        // hold, this would narrow the window enough to fail and change the outcome. The full
        // window is used regardless at this depth, so the result has to match `alpha_beta`'s
        // own untimed, always-full-window reference exactly.
        let (board, _) = Board::from_fen("4k3/p7/8/2p5/4N3/8/8/4K3 w - - 0 1").unwrap();
        let depth = ASPIRATION_MIN_DEPTH - 1;
        let (reference_score, _) = alpha_beta(&board, depth, -INFINITY, INFINITY);

        let (_pv, score, _nodes, stopped, ..) = search_root(
            &board, depth, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12],
            vec![0; CONTINUATION_HISTORY_LEN], &TranspositionTable::new(1), &SyzygyTablebase::new(), None, Some(0), 0,
            &AtomicBool::new(false), &[],
        );

        assert!(!stopped);
        assert_eq!(score, reference_score);
    }

    #[test]
    fn search_root_aspiration_retry_terminates_cleanly_once_stopped() {
        // Regression test for a real bug caught during implementation: once `search.stopped`
        // is set, `alpha_beta_with_buffer` returns its own `alpha` parameter unconditionally,
        // which satisfies the retry loop's `attempt <= alpha` fail-low check by equality --
        // without an explicit `stopped` check of its own, the loop would keep "retrying"
        // forever, doubling `delta` without bound until it overflows `i32` (a panic in this
        // debug/test build). An already-past deadline forces `stopped` well before this
        // depth could ever complete on its own; the real assertion is simply that this call
        // returns at all, without panicking or hanging.
        // Depth deliberately pushed above `ASPIRATION_MIN_DEPTH` itself (still comfortably
        // satisfying its own `depth >= ASPIRATION_MIN_DEPTH` gate, so aspiration retries still
        // apply): late move reductions have since dropped the starting position's own depth-5
        // node count below `CHECK_INTERVAL` (1024), the same class of staleness the deadline
        // test elsewhere in this file already hit once before (that one fixed by raising its own
        // depth from 4 to 8) -- the deadline is only ever sampled every `CHECK_INTERVAL` nodes,
        // so a search finishing before the first sample point never has a chance to see it.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let depth = ASPIRATION_MIN_DEPTH + 3;
        let (_pv, _score, _nodes, stopped, ..) = search_root(
            &board, depth, Some(Instant::now() - Duration::from_secs(1)), Vec::new(),
            vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN],
            &TranspositionTable::new(1), &SyzygyTablebase::new(), None, Some(0), 0, &AtomicBool::new(false), &[],
        );
        assert!(stopped);
    }

    #[test]
    fn a_terminal_node_clears_its_own_stale_pv_row_rather_than_leaking_it_to_a_parent() {
        // `Search::pv` is shared, reused state -- countless unrelated nodes across a real
        // search sit at the same `ply` at different points in time. Deliberately seeding
        // `pv[0]` with unrelated garbage before the call stands in for a stale leftover write
        // from some earlier, unrelated node that happened to share this row -- without
        // `alpha_beta_with_buffer`'s own entry-point clear, a parent reading this row after this
        // call returned improved its own score would misread that garbage as a real continuation
        // instead of the correct "the game ends here."
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/5PPP/r5K1 w - - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(
            3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
        );
        search.pv.set(0, &[Move::new(Square::A1, Square::A8, None)]);
        search.alpha_beta_with_buffer(&board, 0, 3, -INFINITY, INFINITY);
        assert!(search.pv.line(0).is_empty());
    }

    #[test]
    fn a_tt_cutoff_clears_its_own_stale_pv_row_rather_than_leaking_it_to_a_parent() {
        // The bug this and the terminal-node test above actually guard against in practice --
        // caught via real manual UCI testing (an impossible move turning up in a real reported
        // PV), not found by the node-count/score assertions this project otherwise relies on: a
        // TT hit at sufficient depth returns immediately (see `alpha_beta_with_buffer`'s own TT
        // probe), skipping the improvement write below it just as completely as a terminal node
        // does. Seeding `pv[1]` with unrelated garbage stands in for a stale write left behind
        // by some earlier, unrelated node that happened to share this same row during a real
        // search's traversal.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/5PPP/r5K1 w - - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        // depth 2, matching `remaining = target_depth(3) - ply(1)` below exactly, so the probe's
        // `entry.depth() >= remaining` check passes and this resolves via the Exact-bound cutoff
        // unconditionally, regardless of score.
        tt.store(board.hash(), Move::new(Square::A1, Square::A8, None), 2, 0, Bound::Exact);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(
            3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
        );
        search.pv.set(1, &[Move::new(Square::B1, Square::B8, None)]);
        search.alpha_beta_with_buffer(&board, 1, 3, -INFINITY, INFINITY);
        assert!(search.pv.line(1).is_empty());
    }

    #[test]
    fn is_repetition_finds_an_earlier_matching_hash_within_the_halfmove_clock_window() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 1 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let search = new_search(1, None, vec![board.hash()], Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        assert!(search.is_repetition(&board));
    }

    #[test]
    fn is_repetition_ignores_a_match_when_halfmove_clock_is_zero() {
        // halfmove_clock 0 means this position was just reached via an irreversible move (a
        // pawn move or capture) -- nothing before it could possibly be this same position
        // again, no matter what's sitting in `ancestors`.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let search = new_search(1, None, vec![board.hash()], Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        assert!(!search.is_repetition(&board));
    }

    #[test]
    fn is_repetition_is_false_with_no_matching_hash_at_all() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 5 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let search = new_search(1, None, vec![1, 2, 3], Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        assert!(!search.is_repetition(&board));
    }

    #[test]
    fn is_repetition_ignores_a_match_further_back_than_the_halfmove_clock_allows() {
        // The match sits two entries back, but `halfmove_clock` only allows looking back one
        // -- an irreversible move happened in between, making this specific match impossible
        // in reality even though the raw hash value coincides here (deliberately, to isolate
        // the windowing logic itself from needing a real position to produce the collision).
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 1 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let search = new_search(1, None, vec![board.hash(), 999], Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        assert!(!search.is_repetition(&board));
    }

    #[test]
    fn is_improving_is_false_when_the_current_ply_has_no_recorded_eval() {
        // `static_evals[ply]` still `None` -- nothing has ever visited this ply this search
        // (or it did, in check, and never recorded one). No eval to compare at all.
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let search = new_search(5, None, Vec::new(), Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        assert!(!search.is_improving(2, INFINITY));
    }

    #[test]
    fn is_improving_is_true_when_current_eval_already_beats_beta() {
        // The `eval >= beta` override fires regardless of `ply` or any two-plies-back history
        // -- `ply = 0` here specifically to confirm it doesn't even attempt (and panic on) the
        // `ply - 2` lookup once this short-circuit already applies.
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(5, None, Vec::new(), Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.static_evals[0] = Some(100);
        assert!(search.is_improving(0, 50));
    }

    #[test]
    fn is_improving_is_false_when_there_is_no_two_plies_back_position_yet() {
        // `ply = 1`: a real eval is recorded here, but no `ply - 2` position exists yet this
        // search -- the safe default, not a `ply - 2` underflow panic.
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(5, None, Vec::new(), Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.static_evals[1] = Some(100);
        assert!(!search.is_improving(1, INFINITY));
    }

    #[test]
    fn is_improving_is_true_when_current_eval_beats_the_one_from_two_plies_back() {
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(5, None, Vec::new(), Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.static_evals[0] = Some(0);
        search.static_evals[2] = Some(50);
        assert!(search.is_improving(2, INFINITY));
    }

    #[test]
    fn is_improving_is_false_when_current_eval_is_worse_than_two_plies_back() {
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(5, None, Vec::new(), Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.static_evals[0] = Some(50);
        search.static_evals[2] = Some(0);
        assert!(!search.is_improving(2, INFINITY));
    }

    #[test]
    fn is_improving_treats_a_missing_two_plies_back_eval_as_not_improving() {
        // `static_evals[0]` never set (that ancestor was in check) -- no comparison possible,
        // not a fallback to "improving by default."
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(5, None, Vec::new(), Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.static_evals[2] = Some(50);
        assert!(!search.is_improving(2, INFINITY));
    }

    #[test]
    fn has_non_pawn_material_is_false_for_a_pure_king_and_pawn_side() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/P7/4K3 w - - 0 1").unwrap();
        assert!(!has_non_pawn_material(&board, Color::White));
    }

    #[test]
    fn has_non_pawn_material_is_true_with_a_single_knight() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/3NK3 w - - 0 1").unwrap();
        assert!(has_non_pawn_material(&board, Color::White));
    }

    #[test]
    fn late_move_pruning_threshold_is_always_at_least_one_even_at_zero_remaining_depth() {
        // What guarantees the very first move at any node is never late-move-pruned, without a
        // separate special case -- see `is_late_move_pruned`'s own tests below.
        assert_eq!(late_move_pruning_threshold(0, false), 1);
        assert_eq!(late_move_pruning_threshold(0, true), 3);
    }

    #[test]
    fn late_move_pruning_threshold_grows_with_remaining_depth() {
        assert_eq!(late_move_pruning_threshold(1, false), 2);
        assert_eq!(late_move_pruning_threshold(1, true), 4);
        assert_eq!(late_move_pruning_threshold(3, false), 6);
    }

    #[test]
    fn is_late_move_pruned_is_false_when_not_applicable_at_this_node() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mv = Move::new(Square::E1, Square::D1, None);
        assert!(!is_late_move_pruned(false, 0, &board, mv, 100, 1, false));
    }

    #[test]
    fn is_late_move_pruned_is_false_while_best_score_is_in_the_loss_range() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mv = Move::new(Square::E1, Square::D1, None);
        let best_score = -(MATE_SCORE - MAX_PLY as i32);
        assert!(!is_late_move_pruned(true, best_score, &board, mv, 100, 1, false));
    }

    #[test]
    fn is_late_move_pruned_is_false_while_best_score_is_in_the_tablebase_loss_range() {
        // Same shape as the mate-scale version just above, but with a TB-scale best_score
        // instead -- added alongside the tablebase work, since TB_WIN_SCORE sits below
        // is_loss's own threshold on purpose and would otherwise slip past this guard.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mv = Move::new(Square::E1, Square::D1, None);
        assert!(!is_late_move_pruned(true, -TB_WIN_SCORE, &board, mv, 100, 1, false));
    }

    #[test]
    fn is_late_move_pruned_is_false_for_a_noisy_move() {
        // A queen capturing a rook -- not quiet, so late-move pruning never applies to it
        // regardless of how many moves have already been tried.
        let (board, _) = Board::from_fen("3r4/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        let mv = Move::new(Square::D1, Square::D8, None);
        assert!(!is_late_move_pruned(true, 0, &board, mv, 100, 1, false));
    }

    #[test]
    fn is_late_move_pruned_is_false_before_the_threshold_is_reached() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mv = Move::new(Square::E1, Square::D1, None);
        // remaining = 1, not improving: threshold = (3 + 1) / 2 = 2.
        assert!(!is_late_move_pruned(true, 0, &board, mv, 1, 1, false));
    }

    #[test]
    fn is_late_move_pruned_is_true_once_the_threshold_is_reached() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mv = Move::new(Square::E1, Square::D1, None);
        assert!(is_late_move_pruned(true, 0, &board, mv, 2, 1, false));
    }

    #[test]
    fn is_lmr_eligible_is_false_below_the_remaining_depth_threshold() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mv = Move::new(Square::E1, Square::D1, None);
        assert!(!is_lmr_eligible(2, &board, mv));
    }

    #[test]
    fn is_lmr_eligible_is_false_for_a_noisy_move() {
        let (board, _) = Board::from_fen("3r4/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        let mv = Move::new(Square::D1, Square::D8, None);
        assert!(!is_lmr_eligible(3, &board, mv));
    }

    #[test]
    fn is_lmr_eligible_is_false_for_a_check_giving_quiet_move() {
        // Queen d1 to d8: the destination is empty (quiet, not a capture -- `is_quiet` alone
        // would not exclude this), but it lands on the same rank as Black's king on e8, giving
        // check. Extension has to win outright over reduction for the same move, not just be
        // favored -- see `is_lmr_eligible`'s own doc comment.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        let mv = Move::new(Square::D1, Square::D8, None);
        assert!(is_quiet(&board, mv));
        assert!(gives_check(&board, mv));
        assert!(!is_lmr_eligible(3, &board, mv));
    }

    #[test]
    fn is_lmr_eligible_is_true_for_a_quiet_move_at_sufficient_remaining_depth() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mv = Move::new(Square::E1, Square::D1, None);
        assert!(is_lmr_eligible(3, &board, mv));
    }

    #[test]
    fn lmr_reduction_matches_the_original_flat_reduction_at_the_minimum_eligible_depth() {
        // `is_lmr_eligible`'s own floor, paired with the earliest possible `moves_tried` an
        // eligible call can ever have (the loop's own post-increment convention, see
        // `lmr_reduction`'s own doc comment) -- this technique's original, pre-log-shaped
        // behavior at exactly this boundary, preserved deliberately.
        assert_eq!(lmr_reduction(3, 2), 1);
    }

    #[test]
    fn lmr_reduction_does_not_grow_again_until_remaining_actually_triples() {
        // `moves_tried` held fixed at `2` (its own zero-contribution point) throughout, isolating
        // the depth axis alone.
        assert_eq!(lmr_reduction(8, 2), 1);
        assert_eq!(lmr_reduction(9, 2), 2);
        assert_eq!(lmr_reduction(26, 2), 2);
        assert_eq!(lmr_reduction(27, 2), 3);
    }

    #[test]
    fn lmr_reduction_does_not_grow_again_until_moves_tried_actually_triples() {
        // `remaining` held fixed at `27` throughout -- comfortably above the minimum, so the
        // `.min(remaining - 2)` clamp has plenty of slack and can't be the thing masking the
        // move-count axis here.
        assert_eq!(lmr_reduction(27, 2), 3);
        assert_eq!(lmr_reduction(27, 3), 4);
        assert_eq!(lmr_reduction(27, 8), 4);
        assert_eq!(lmr_reduction(27, 9), 5);
    }

    #[test]
    fn lmr_reduction_clamps_an_unbounded_moves_tried_at_the_minimum_remaining() {
        // At `remaining == 3`, the depth term alone already saturates the floor
        // (`remaining - 2 == 1`) -- the move-count term must contribute nothing further here,
        // regardless of how large `moves_tried` gets, exactly the scenario the clamp exists for
        // (see `lmr_reduction`'s own doc comment for the `u32` underflow this prevents).
        assert_eq!(lmr_reduction(3, 218), 1);
    }

    #[test]
    fn lmr_reduction_never_violates_the_at_least_one_real_ply_floor() {
        // The property `lmr_reduction`'s own doc comment argues holds unconditionally, by
        // construction, not just at the boundary case checked above -- checked directly here
        // across a wide range of both axes rather than trusted from the argument alone, the same
        // "assertions, not eyeballing" discipline every other pure predicate in this file already
        // gets. `moves_tried` swept past chess's own real maximum legal move count (218) for good
        // measure, even though a real call site never actually reaches that high.
        for remaining in 3..300 {
            for moves_tried in 1..250 {
                assert!(
                    remaining >= lmr_reduction(remaining, moves_tried) + 2,
                    "remaining = {remaining}, moves_tried = {moves_tried} left less than 1 real ply for the reduced probe"
                );
            }
        }
    }

    #[test]
    fn iir_reduction_is_zero_with_a_tt_move_present() {
        let mv = Move::new(Square::E1, Square::D1, None);
        assert_eq!(iir_reduction(1, Some(mv), IIR_MIN_REMAINING), 0);
    }

    #[test]
    fn iir_reduction_is_zero_below_the_minimum_remaining() {
        assert_eq!(iir_reduction(1, None, IIR_MIN_REMAINING - 1), 0);
    }

    #[test]
    fn iir_reduction_is_zero_at_the_root_even_with_no_tt_move_and_sufficient_remaining() {
        // The root exclusion this technique's own doc comment argues for: `target_depth` at
        // `ply == 0` is also what gets reported, so a reduction here would silently stop
        // iterative deepening from actually advancing on that iteration, not just mislabel it.
        assert_eq!(iir_reduction(0, None, IIR_MIN_REMAINING), 0);
    }

    #[test]
    fn iir_reduction_is_the_full_reduction_with_no_tt_move_at_the_minimum_remaining() {
        assert_eq!(iir_reduction(1, None, IIR_MIN_REMAINING), IIR_REDUCTION);
    }

    #[test]
    fn iir_reduction_is_the_full_reduction_comfortably_above_the_minimum_remaining() {
        assert_eq!(iir_reduction(5, None, IIR_MIN_REMAINING + 20), IIR_REDUCTION);
    }

    #[test]
    fn is_null_move_pruning_applicable_is_false_at_the_root() {
        assert!(!is_null_move_pruning_applicable(0, false, false, NULL_MOVE_MIN_REMAINING, 0, true));
    }

    #[test]
    fn is_null_move_pruning_applicable_is_false_while_in_check() {
        assert!(!is_null_move_pruning_applicable(1, true, false, NULL_MOVE_MIN_REMAINING, 0, true));
    }

    #[test]
    fn is_null_move_pruning_applicable_is_false_immediately_after_another_null_move() {
        assert!(!is_null_move_pruning_applicable(1, false, true, NULL_MOVE_MIN_REMAINING, 0, true));
    }

    #[test]
    fn is_null_move_pruning_applicable_is_false_below_the_minimum_remaining_depth() {
        assert!(!is_null_move_pruning_applicable(1, false, false, NULL_MOVE_MIN_REMAINING - 1, 0, true));
    }

    #[test]
    fn is_null_move_pruning_applicable_is_false_when_beta_is_already_mate_scale() {
        let mate_scale_beta = MATE_SCORE - MAX_PLY as i32;
        assert!(!is_null_move_pruning_applicable(1, false, false, NULL_MOVE_MIN_REMAINING, mate_scale_beta, true));
    }

    #[test]
    fn is_null_move_pruning_applicable_is_false_when_beta_is_already_tablebase_scale() {
        // Same shape as the mate-scale version just above, but with a TB-scale beta instead --
        // added alongside the tablebase work, for the identical reason.
        assert!(!is_null_move_pruning_applicable(1, false, false, NULL_MOVE_MIN_REMAINING, TB_WIN_SCORE, true));
    }

    #[test]
    fn is_null_move_pruning_applicable_is_false_without_non_pawn_material() {
        assert!(!is_null_move_pruning_applicable(1, false, false, NULL_MOVE_MIN_REMAINING, 0, false));
    }

    #[test]
    fn is_null_move_pruning_applicable_is_true_when_every_precondition_holds() {
        assert!(is_null_move_pruning_applicable(1, false, false, NULL_MOVE_MIN_REMAINING, 0, true));
    }

    #[test]
    fn null_move_plies_grow_with_depth_and_margin_and_always_leave_a_ply() {
        // The shallowest null move loses today's three plies, whatever the margin: the floor.
        let per_ply = SearchParameters::DEFAULT.null_move_margin;
        assert_eq!(null_move_plies(NULL_MOVE_MIN_REMAINING, 0, per_ply), 3);
        assert_eq!(null_move_plies(NULL_MOVE_MIN_REMAINING, 10 * per_ply, per_ply), 3);
        // The depth term: one more ply per four of remaining depth.
        assert_eq!(null_move_plies(8, 0, per_ply), 5);
        assert_eq!(null_move_plies(9, 0, per_ply), 5);
        assert_eq!(null_move_plies(13, 0, per_ply), 6);
        // The eval term: one per margin over beta, rounded down, capped at three.
        assert_eq!(null_move_plies(20, per_ply - 1, per_ply), 8);
        assert_eq!(null_move_plies(20, per_ply, per_ply), 9);
        assert_eq!(null_move_plies(20, 3 * per_ply, per_ply), 11);
        assert_eq!(null_move_plies(20, 100 * per_ply, per_ply), 11);
        // In a smaller evaluator pawn, the same evaluation margin is worth more plies.
        assert_eq!(null_move_plies(20, per_ply, per_ply / 2), 10);
        // Never more than remaining - 1.
        for remaining in NULL_MOVE_MIN_REMAINING..40 {
            for margin in [0, per_ply, 5 * per_ply] {
                assert!(null_move_plies(remaining, margin, per_ply) < remaining, "remaining {remaining}, margin {margin}");
            }
        }
    }

    #[test]
    fn is_history_pruned_is_false_when_not_applicable_at_this_node() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mv = Move::new(Square::E1, Square::D1, None);
        let mut history = [[0; 64]; 12];
        let king = Piece::new(Color::White, PieceType::King);
        history[king as usize][Square::D1 as usize] = -1_000_000;
        assert!(!is_history_pruned(false, &history, &board, mv, 2, SearchParameters::DEFAULT.history_pruning_multiplier));
    }

    #[test]
    fn is_history_pruned_is_false_for_a_noisy_move() {
        let (board, _) = Board::from_fen("3r4/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        let mv = Move::new(Square::D1, Square::D8, None);
        let mut history = [[0; 64]; 12];
        let queen = Piece::new(Color::White, PieceType::Queen);
        history[queen as usize][Square::D8 as usize] = -1_000_000;
        assert!(!is_history_pruned(true, &history, &board, mv, 2, SearchParameters::DEFAULT.history_pruning_multiplier));
    }

    #[test]
    fn is_history_pruned_is_false_above_the_threshold() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mv = Move::new(Square::E1, Square::D1, None);
        let mut history = [[0; 64]; 12];
        let king = Piece::new(Color::White, PieceType::King);
        // remaining = 2: threshold is the multiplier * 4 = -140. One point above it.
        history[king as usize][Square::D1 as usize] = SearchParameters::DEFAULT.history_pruning_multiplier * 4 + 1;
        assert!(!is_history_pruned(true, &history, &board, mv, 2, SearchParameters::DEFAULT.history_pruning_multiplier));
    }

    #[test]
    fn is_history_pruned_is_true_below_the_threshold() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mv = Move::new(Square::E1, Square::D1, None);
        let mut history = [[0; 64]; 12];
        let king = Piece::new(Color::White, PieceType::King);
        history[king as usize][Square::D1 as usize] = SearchParameters::DEFAULT.history_pruning_multiplier * 4 - 1;
        assert!(is_history_pruned(true, &history, &board, mv, 2, SearchParameters::DEFAULT.history_pruning_multiplier));
    }

    #[test]
    fn is_see_pruned_is_false_when_not_applicable_at_this_node() {
        // White knight a1 to b3: walks into a square only Black's own pawn on a4 attacks, with
        // nothing of White's own left nearby to recapture -- hangs the whole knight for nothing,
        // the same "quiet move into an undefended, attacked square" shape SEE's own occupancy
        // math already handles for `gives_check`. Used by every test below, `applicable` alone
        // is what varies.
        let (board, _) = Board::from_fen("4k3/8/8/8/p7/8/8/N3K3 w - - 0 1").unwrap();
        let mv = Move::new(Square::A1, Square::B3, None);
        assert!(!is_see_pruned(false, &board, mv, 1, SearchParameters::DEFAULT.see_prune_margin));
    }

    #[test]
    fn is_see_pruned_is_false_for_a_move_that_does_not_lose_material() {
        let (board, _) = Board::from_fen("4k3/8/8/8/p7/8/8/N3K3 w - - 0 1").unwrap();
        // The king stepping to an entirely unattacked square -- no capture happens at all, so
        // its own SEE value is exactly `0`, clearing even the smallest possible margin
        // (`remaining == 1`).
        let mv = Move::new(Square::E1, Square::E2, None);
        assert!(!is_see_pruned(true, &board, mv, 1, SearchParameters::DEFAULT.see_prune_margin));
    }

    #[test]
    fn is_see_pruned_is_true_for_a_move_that_hangs_a_piece_for_nothing_at_shallow_remaining() {
        let (board, _) = Board::from_fen("4k3/8/8/8/p7/8/8/N3K3 w - - 0 1").unwrap();
        let mv = Move::new(Square::A1, Square::B3, None);
        // remaining = 1: whatever `see_prune_margin` is set to, one ply's worth of it is still far
        // short of a whole knight's value, so the knight's own SEE value (-320) fails the margin
        // outright whichever value is live.
        assert!(is_see_pruned(true, &board, mv, 1, SearchParameters::DEFAULT.see_prune_margin));
    }

    #[test]
    fn is_see_pruned_stops_pruning_once_remaining_grows_the_margin_past_the_loss() {
        let (board, _) = Board::from_fen("4k3/8/8/8/p7/8/8/N3K3 w - - 0 1").unwrap();
        let mv = Move::new(Square::A1, Square::B3, None);
        let knight_value = piece_value(PieceType::Knight);
        // Derived from `see_prune_margin` itself, not a hardcoded remaining -- a tune or a new
        // default moves it, and a literal like `4` would silently stop meaning what its own
        // comment claimed the moment the margin changed underneath it.
        // `(knight_value - 1) / see_prune_margin` is the largest `remaining` whose margin still
        // strictly undershoots the knight's own loss (integer division floors, and the `- 1`
        // guards against an exact multiple counting as "still pruned" when it would actually
        // already clear the threshold).
        let margin = SearchParameters::DEFAULT.see_prune_margin;
        let remaining_still_pruned = ((knight_value - 1) / margin) as u32;
        assert!(is_see_pruned(true, &board, mv, remaining_still_pruned, margin));
        // One ply more of margin is guaranteed to clear the knight's own -320: the identical move
        // that was pruned just above is fully trusted again here, purely because the margin
        // itself grew, not because anything about the move did.
        assert!(!is_see_pruned(true, &board, mv, remaining_still_pruned + 1, margin));
    }

    #[test]
    fn mate_in_and_mated_in_are_mirror_images() {
        assert_eq!(mate_in(0), MATE_SCORE);
        assert_eq!(mated_in(0), -MATE_SCORE);
        assert_eq!(mate_in(5), MATE_SCORE - 5);
        assert_eq!(mated_in(5), -(MATE_SCORE - 5));
    }

    #[test]
    fn a_score_survives_the_round_trip_through_the_table_at_any_ply() {
        for ply in [0, 1, 7, MAX_PLY] {
            for score in [mate_in(3), mated_in(3), tb_win_at(3), tb_loss_at(3), 0, 137, -137] {
                assert_eq!(score_from_tt(score_to_tt(score, ply), ply), score, "score {score} at ply {ply}");
            }
            for score in [0, 137, -137] {
                assert_eq!(score_to_tt(score, ply), score);
                assert_eq!(score_from_tt(score, ply), score);
            }
        }
    }

    #[test]
    fn a_stored_mate_or_tablebase_score_is_relative_to_its_node() {
        for ply in [0, 1, 7, 40, MAX_PLY] {
            assert_eq!(score_to_tt(mate_in(ply), ply), MATE_SCORE);
            assert_eq!(score_to_tt(mated_in(ply), ply), -MATE_SCORE);
            assert_eq!(score_to_tt(tb_win_at(ply), ply), TB_WIN_SCORE);
            assert_eq!(score_to_tt(tb_loss_at(ply), ply), -TB_WIN_SCORE);
        }
    }

    #[test]
    fn a_score_stored_at_one_ply_and_probed_at_another_keeps_its_distance_from_the_node() {
        let tt = TranspositionTable::new(1);
        let mv = Move::new(Square::A1, Square::A2, None);
        let cases = [(mate_in(5), mate_in(1)), (mated_in(5), mated_in(1)), (tb_win_at(5), tb_win_at(1)), (tb_loss_at(5), tb_loss_at(1))];
        for (index, (stored_at_ply_5, expected_at_ply_1)) in cases.into_iter().enumerate() {
            let hash = 0x1234_5678_9ABC_0000 + index as u64;
            tt.store(hash, mv, 3, score_to_tt(stored_at_ply_5, 5), Bound::Exact);
            let entry = tt.probe(hash).expect("the entry was just stored");
            assert_eq!(score_from_tt(entry.score(), 1), expected_at_ply_1);
        }
    }

    /// The case the unconverted table got wrong: a mate found in one search, and the table kept
    /// for the next search of the same game, two plies later along the mating line. The second
    /// search must report the mate two plies nearer, the same as a fresh table does.
    #[test]
    fn a_mate_found_in_one_search_is_two_plies_nearer_after_both_sides_play_its_line() {
        let (board, _) = Board::from_fen("k7/8/2K5/8/8/8/8/7Q w - - 0 1").unwrap();
        let tablebase = SyzygyTablebase::new();
        let stop = AtomicBool::new(false);
        let tt = TranspositionTable::new(16);
        let (reports, _) = choose_best_move(&board, 10, None, &[], &tt, &tablebase, None, 0, &stop);
        let first = reports.last().expect("a search to depth 10 reports");
        assert!(is_win(first.score), "KQ vs K with the kings close is a forced mate, got {}", first.score);
        let plies = MATE_SCORE - first.score;
        assert!(plies >= 3, "the test needs a mate at least two moves away, got {plies} plies");

        let mut later = board;
        later.make_move(first.pv[0]);
        later.make_move(first.pv[1]);
        let (reports, _) = choose_best_move(&later, 8, None, &[], &tt, &tablebase, None, 0, &stop);
        assert_eq!(reports.last().unwrap().score, mate_in(plies as u32 - 2), "same table");

        let fresh = TranspositionTable::new(16);
        let (reports, _) = choose_best_move(&later, 8, None, &[], &fresh, &tablebase, None, 0, &stop);
        assert_eq!(reports.last().unwrap().score, mate_in(plies as u32 - 2), "fresh table");
    }

    #[test]
    fn mate_distance_pruning_returns_early_once_a_shorter_mate_makes_this_node_unreachable() {
        // `alpha` set just above `mate_in(2)` -- one more than the best this node could ever
        // achieve (mating on its own very next move, from `ply = 1`) -- models "a shorter mate
        // was already found upward in the tree," Stockfish's own comment for this exact
        // scenario. `nodes == 1` confirms an outright early return, no move generation at all.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        let alpha = mate_in(2) + 1;

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        let score = search.alpha_beta_with_buffer(&board, 1, 3, alpha, INFINITY);
        assert_eq!(score, alpha);
        assert_eq!(search.nodes, 1);
    }

    #[test]
    fn mate_distance_pruning_does_not_apply_at_the_root() {
        // Same triggering condition as above, but at `ply = 0`: without the `ply > 0` gate,
        // this would return a bare score with no move behind it -- the exact `bestmove 0000`
        // bug class the TT probe, razoring, and reverse futility pruning already guard against
        // (see this technique's own implementation comment).
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        let alpha = mate_in(1) + 1;

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search =
            new_search(3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        search.alpha_beta_with_buffer(&board, 0, 3, alpha, INFINITY);
        assert!(!search.pv.line(0).is_empty(), "expected the root to still find and report a real move");
    }

    #[test]
    fn a_psq_search_with_incremental_accumulators_matches_one_that_refreshes_every_node() {
        // The same fixed-depth search twice with the same psq network: once through
        // `set_evaluator` (accumulators updated along the tree), once with the evaluator set but
        // no accumulators, which makes `evaluate` fall back to a from-scratch evaluation at every
        // node. Same score and same best move is what shows that every child's accumulators are
        // updated before the child is evaluated, including after null moves and in quiescence.
        //
        // Float rounding differs between the two paths by about 1e-7, far below the half
        // centipawn it would take to change a rounded score, so the two searches are expected to
        // agree exactly.
        let network = Network::from_psq(crate::nnue::tests::sample_network(16, &[8])).unwrap();
        let fens = [
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
            "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 11",
        ];
        for fen in fens {
            let (board, _) = Board::from_fen(fen).unwrap();
            let mut results = Vec::new();
            for incremental in [true, false] {
                let tt = TranspositionTable::new(1);
                let stop_signal = AtomicBool::new(false);
                let mut search = new_search(
                    5, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12],
                    vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
                );
                if incremental {
                    search.set_evaluator(Some(&network), &board);
                    assert!(!search.accumulators.is_empty(), "test's own premise: the incremental path is on");
                } else {
                    search.evaluator = Some(&network);
                    search.set_pawn_percent(network.pawn_percent());
                }
                let score = search.alpha_beta_with_buffer(&board, 0, 5, -INFINITY, INFINITY);
                results.push((score, search.pv.line(0).first().copied(), search.nodes));
            }
            assert_eq!(results[0], results[1], "{fen}: incremental (score, best move, nodes) against from scratch");
        }
    }

    #[test]
    fn alpha_beta_with_buffer_returns_zero_immediately_for_a_repeated_position() {
        // White is up a whole rook -- if the repetition check weren't actually wired in, this
        // would evaluate hugely in White's favor, not 0. Confirms it really does override what
        // would otherwise be a very different score, not just that the check exists somewhere.
        // `ply = 1`, not `0`: see `repetition_check_does_not_apply_at_the_root` just below for
        // why the root itself is deliberately exempt.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 1 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(3, None, vec![board.hash()], Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        let score = search.alpha_beta_with_buffer(&board, 1, 3, -INFINITY, INFINITY);
        assert_eq!(score, 0);
        assert_eq!(search.nodes, 1, "expected the repetition to be caught before any move was even generated");
    }

    #[test]
    fn repetition_check_does_not_apply_at_the_root() {
        // Regression test for a real bug found via a bench position (not a unit test --
        // `search.md`'s progress log has the full diagnosis): a real `position fen ... moves
        // ...` sequence can perfectly ordinarily land back on a position already sitting in
        // `ancestors` (a shuffle that returns to where it started), and without the `ply > 0`
        // guard, the root would resolve via a bare `return 0` before the move loop ever ran --
        // leaving `pv[0]` empty and no move reported at all, the identical illegal-`bestmove`
        // bug class the TT probe/razoring/reverse futility pruning/mate distance pruning already
        // guard against for the same reason. Same board and ancestors as the test just above
        // (a real repetition by every other measure), but called at `ply = 0` this time.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 1 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(
            3, None, vec![board.hash()], vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
        );
        search.alpha_beta_with_buffer(&board, 0, 3, -INFINITY, INFINITY);
        assert!(!search.pv.line(0).is_empty(), "expected the root to still find and report a real move");
    }

    #[test]
    fn quiescence_returns_zero_immediately_for_a_repeated_position() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 1 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(0, None, vec![board.hash()], Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        let score = search.quiescence(&board, 0, -INFINITY, INFINITY, None);
        assert_eq!(score, 0);
        assert_eq!(search.nodes, 1, "expected the repetition to be caught before any move was even generated");
    }

    #[test]
    fn a_full_search_leaves_ancestors_exactly_as_it_found_it() {
        // The push before, and pop after, each recursive call (in both `alpha_beta_with_buffer`
        // and `quiescence`) has to stay perfectly balanced across the *whole* recursion tree,
        // however deep or wide it actually goes -- otherwise `ancestors` would leak entries (or
        // lose real ones) that would then wrongly carry over into whatever the next depth's own
        // `search_root` call does with its own fresh copy of the same seed. Depth 3 from the
        // starting position exercises plenty of real recursion, both main-search and
        // quiescence, without needing to hand-construct a specific repeating line.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let seed = vec![111, 222];
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(
            3, None, seed.clone(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
        );
        search.alpha_beta_with_buffer(&board, 0, 3, -INFINITY, INFINITY);
        assert_eq!(search.ancestors, seed);
    }

    #[test]
    fn is_fifty_move_draw_is_true_at_the_hundred_ply_threshold() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 100 1").unwrap();
        assert!(is_fifty_move_draw(&board));
    }

    #[test]
    fn is_fifty_move_draw_is_false_just_below_the_threshold() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 99 1").unwrap();
        assert!(!is_fifty_move_draw(&board));
    }

    #[test]
    fn alpha_beta_with_buffer_returns_zero_at_the_fifty_move_threshold() {
        // Same position `alpha_beta_depth_one_prefers_the_move_that_wins_more_material` uses
        // -- Ne4xc5 would normally score +320 -- but `halfmove_clock` is already at the
        // fifty-move threshold, so every line from here is an automatic draw regardless of
        // material.
        let (board, _) = Board::from_fen("4k3/8/8/2p5/4N3/8/8/4K3 w - - 100 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(
            1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
        );
        let score = search.alpha_beta_with_buffer(&board, 0, 1, -INFINITY, INFINITY);
        assert_eq!(score, 0);
    }

    #[test]
    fn a_fifty_move_node_not_in_check_returns_the_draw_without_searching() {
        // The position above, below the root: no child is searched, the draw is known at entry.
        let (board, _) = Board::from_fen("4k3/8/8/2p5/4N3/8/8/4K3 w - - 100 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(
            3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
        );
        assert_eq!(search.alpha_beta_with_buffer(&board, 1, 3, -INFINITY, INFINITY), 0);
        assert_eq!(search.nodes, 1, "expected the fifty-move check to fire before any child was searched");
    }

    #[test]
    fn a_fifty_move_node_in_check_reports_no_line_past_the_draw() {
        // White in check from the rook, with evasions to search: the node searches them, an
        // improving one writes its line into this row, and the draw must clear it, else the
        // parent's reported PV would run past the draw.
        let (board, _) = Board::from_fen("4r1k1/8/8/8/8/8/8/R3K3 w - - 100 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(
            3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
        );
        assert_eq!(search.alpha_beta_with_buffer(&board, 1, 3, -INFINITY, INFINITY), 0);
        assert!(search.nodes > 1, "in check, the evasions are searched");
        assert!(search.pv.line(1).is_empty(), "{:?}", search.pv.line(1));
    }

    #[test]
    fn alpha_beta_with_buffer_prioritizes_checkmate_over_the_fifty_move_draw() {
        // Same back-rank mate `search_root_reports_an_empty_pv_at_a_terminal_root` uses, but
        // with `halfmove_clock` already at the fifty-move threshold -- confirms checkmate wins
        // when the two coincide, rather than silently turning a decisive loss into a draw.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/5PPP/r5K1 w - - 100 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(
            1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
        );
        let score = search.alpha_beta_with_buffer(&board, 0, 1, -INFINITY, INFINITY);
        assert_eq!(score, -MATE_SCORE);
    }

    #[test]
    fn quiescence_returns_zero_at_the_fifty_move_threshold_when_not_in_check() {
        // Same position `quiescence_recurses_through_a_favorable_capture_that_plain_evaluation_
        // misses` uses -- Qxd8 would normally return +900 -- but `halfmove_clock` is already at
        // the fifty-move threshold, checked here before stand pat or any move is even
        // considered.
        let (board, _) = Board::from_fen("3r4/8/k7/8/3Q4/8/8/4K3 w - - 100 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        assert_eq!(search.quiescence(&board, 0, -INFINITY, INFINITY, None), 0);
        assert_eq!(search.nodes, 1, "expected the fifty-move check to fire before any move was even generated");
    }

    #[test]
    fn quiescence_returns_zero_at_the_fifty_move_threshold_when_in_check_but_not_mated() {
        // Same position `quiescence_searches_quiet_evasions_when_in_check` uses -- every legal
        // evasion is quiet, material is otherwise unbalanced, but `halfmove_clock` is already
        // at the fifty-move threshold. Exercises the in-check path's own check, reached only
        // once move generation has actually ruled out checkmate.
        let (board, _) = Board::from_fen("k3r3/8/8/8/8/8/8/4K3 w - - 100 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        assert_eq!(search.quiescence(&board, 0, -INFINITY, INFINITY, None), 0);
    }

    #[test]
    fn quiescence_prioritizes_checkmate_over_the_fifty_move_draw() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/5PPP/r5K1 w - - 100 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        assert_eq!(search.quiescence(&board, 0, -INFINITY, INFINITY, None), -MATE_SCORE);
    }

    #[test]
    fn king_vs_king_is_insufficient_material() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert!(is_insufficient_material(&board));
    }

    #[test]
    fn king_and_knight_vs_king_is_insufficient_material() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4KN2 w - - 0 1").unwrap();
        assert!(is_insufficient_material(&board));
    }

    #[test]
    fn king_and_bishop_vs_king_is_insufficient_material() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4KB2 w - - 0 1").unwrap();
        assert!(is_insufficient_material(&board));
    }

    #[test]
    fn same_colored_bishops_are_insufficient_material() {
        // White's bishop on c1 and Black's on f8 both sit on dark squares.
        let (board, _) = Board::from_fen("4kb2/8/8/8/8/8/8/2B1K3 w - - 0 1").unwrap();
        assert!(is_insufficient_material(&board));
    }

    #[test]
    fn opposite_colored_bishops_are_not_insufficient_material() {
        // Same as above, but Black's bishop moved from f8 (dark) to g8 (light).
        let (board, _) = Board::from_fen("4k1b1/8/8/8/8/8/8/2B1K3 w - - 0 1").unwrap();
        assert!(!is_insufficient_material(&board));
    }

    #[test]
    fn two_knights_one_each_side_is_not_insufficient_material() {
        // Deliberately excluded from the classical FIDE list -- see
        // `is_insufficient_material`'s own doc comment for why.
        let (board, _) = Board::from_fen("4kn2/8/8/8/8/8/8/4KN2 w - - 0 1").unwrap();
        assert!(!is_insufficient_material(&board));
    }

    #[test]
    fn a_pawn_rules_out_insufficient_material_regardless_of_everything_else() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
        assert!(!is_insufficient_material(&board));
    }

    #[test]
    fn two_minor_pieces_on_one_side_is_not_insufficient_material() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/2B1KB2 w - - 0 1").unwrap();
        assert!(!is_insufficient_material(&board));
    }

    #[test]
    fn alpha_beta_with_buffer_returns_zero_for_insufficient_material() {
        // King and knight vs king would normally score +320 for White -- confirms the check
        // actually overrides material, not just exists in isolation, and fires before any
        // move is even generated (nodes == 1). `ply = 1`, not `0`: see
        // `insufficient_material_check_does_not_apply_at_the_root` just below.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4KN2 w - - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(3, None, Vec::new(), Vec::new(), [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal);
        let score = search.alpha_beta_with_buffer(&board, 1, 3, -INFINITY, INFINITY);
        assert_eq!(score, 0);
        assert_eq!(search.nodes, 1, "expected the check to fire before any move was even generated");
    }

    #[test]
    fn insufficient_material_check_does_not_apply_at_the_root() {
        // Same fix, same reasoning as `repetition_check_does_not_apply_at_the_root` just above --
        // not observed to actually misfire on any real bench position (unlike the repetition
        // one), but the identical root-corruption gap existed in principle, so it was fixed and
        // tested alongside it.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4KN2 w - - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(
            3, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN], &tt, &stop_signal,
        );
        search.alpha_beta_with_buffer(&board, 0, 3, -INFINITY, INFINITY);
        assert!(!search.pv.line(0).is_empty(), "expected the root to still find and report a real move");
    }

    #[test]
    fn quiescence_returns_zero_for_insufficient_material() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4KN2 w - - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        assert_eq!(search.quiescence(&board, 0, -INFINITY, INFINITY, None), 0);
        assert_eq!(search.nodes, 1, "expected the check to fire before any move was even generated");
    }

    #[test]
    fn a_tablebase_with_no_loaded_tables_leaves_quiescence_unaffected() {
        // Same limitation as alpha_beta_with_buffer's own version of this test (see
        // a_tablebase_with_no_loaded_tables_leaves_alpha_beta_with_buffer_unaffected): no
        // .rtbw files are checked into this repo, so this only confirms Some(&tablebase) with
        // nothing loaded behaves identically to the None every other quiescence test already
        // exercises via quiescence_search's own default. A real capture sequence available
        // (not bare kings) so quiescence actually recurses past the first stand-pat check,
        // rather than returning early for a reason that would mask this field's own effect.
        let (board, _) = Board::from_fen("4k3/8/2p5/3p4/4P3/8/8/4K3 w - - 0 1").unwrap();

        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut without_tablebase = quiescence_search(&tt, &stop_signal);
        let score_without = without_tablebase.quiescence(&board, 1, -INFINITY, INFINITY, None);

        let tablebase = SyzygyTablebase::new();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut with_tablebase = quiescence_search(&tt, &stop_signal);
        with_tablebase.tablebase = Some(&tablebase);
        let score_with = with_tablebase.quiescence(&board, 1, -INFINITY, INFINITY, None);

        assert_eq!(score_with, score_without);
    }

    #[test]
    fn choose_best_move_runs_every_depth_up_to_max_and_accumulates_nodes() {
        // Second, a7 pawn again for the same reason as the tests above: keeps Nxc5 from
        // landing on King+Knight vs bare King, which the insufficient-material check would
        // otherwise flatten to a dead-draw score of 0.
        let (board, _) = Board::from_fen("4k3/p7/8/2p5/4N3/8/8/4K3 w - - 0 1").unwrap();
        let (reports, best_move) = choose_best_move(
            &board, 3, None, &[], &TranspositionTable::new(1), &SyzygyTablebase::new(), None, 0,
            &AtomicBool::new(false),
        );

        assert_eq!(reports.len(), 3);
        for (i, report) in reports.iter().enumerate() {
            assert_eq!(report.depth, i as u32 + 1);
        }
        // Cumulative, not per-depth: each report's own node count can only grow (or, in a
        // position this small, at least never shrink) as deeper iterations add their own
        // nodes on top of every shallower one's.
        assert!(reports[1].nodes >= reports[0].nodes);
        assert!(reports[2].nodes >= reports[1].nodes);
        // time_ms is elapsed wall-clock time since choose_best_move's own start, read via
        // Instant::elapsed() at a strictly later point for each successive report -- unlike
        // the node-count check above, this isn't just "true for this position," it's
        // guaranteed by the monotonic clock itself.
        assert!(reports[1].time_ms >= reports[0].time_ms);
        assert!(reports[2].time_ms >= reports[1].time_ms);
        assert_eq!(best_move, Some(Move::new(Square::E4, Square::C5, None)));
    }

    #[test]
    fn choose_best_move_calls_on_iteration_once_per_completed_depth_with_matching_content() {
        // Same position as the test just above, reusing its own already-established shape.
        let (board, _) = Board::from_fen("4k3/p7/8/2p5/4N3/8/8/4K3 w - - 0 1").unwrap();
        let mut observed_depths = Vec::new();
        let (reports, _best_move) = choose_best_move_with_progress(
            &board, 3, None, &[], &TranspositionTable::new(1), &SyzygyTablebase::new(), None, None, 0,
            &AtomicBool::new(false), None, |report| observed_depths.push(report.depth),
        );

        // Exactly one callback per depth actually completed, in the same order choose_best_move
        // itself reports them -- on_iteration observes the same sequence, not a separate one.
        assert_eq!(observed_depths, reports.iter().map(|r| r.depth).collect::<Vec<_>>());
    }

    #[test]
    fn the_pesto_evaluation_searches_untuned_by_default() {
        assert_eq!(default_parameters_for(None), SearchParameters::DEFAULT);
    }

    /// `choose_best_move_with_parameters` at the defaults is `choose_best_move` exactly, and each of
    /// the ten parameters, moved alone, changes the tree: every one is read where it belongs. Depth 9,
    /// for null move's eval term, which needs 6 plies left to have room.
    #[test]
    fn every_search_parameter_reaches_the_search() {
        let (board, _) =
            Board::from_fen("r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4").unwrap();
        let nodes = |parameters: SearchParameters| {
            let (reports, _) = choose_best_move_with_parameters(
                &board, 9, None, &[], &TranspositionTable::new(16), &SyzygyTablebase::new(), None, parameters, 0,
                &AtomicBool::new(false),
            );
            reports.last().unwrap().nodes
        };
        let (default_reports, _) = choose_best_move(
            &board, 9, None, &[], &TranspositionTable::new(16), &SyzygyTablebase::new(), None, 0,
            &AtomicBool::new(false),
        );
        let default = SearchParameters::DEFAULT;
        let default_nodes = nodes(default);
        assert_eq!(default_nodes, default_reports.last().unwrap().nodes);

        let moved: [(&str, SearchParameters); 10] = [
            ("razoring", SearchParameters { razoring_margin: 50, ..default }),
            ("reverse futility", SearchParameters { reverse_futility_margin: 50, ..default }),
            ("futility", SearchParameters { futility_margin: 50, ..default }),
            // Its term only adds plies where the null search still has room under `remaining - 1`:
            // 6 or more plies left, so depth 9 for plies 1 to 3. A margin of 1 gives every
            // evaluation above beta the most.
            ("null move", SearchParameters { null_move_margin: 1, ..default }),
            ("delta", SearchParameters { delta_margin: 0, ..default }),
            ("SEE pruning", SearchParameters { see_prune_margin: 50, ..default }),
            ("SEE in quiescence", SearchParameters { see_margin: 200, ..default }),
            ("aspiration", SearchParameters { aspiration_delta: 5, ..default }),
            ("history pruning", SearchParameters { history_pruning_multiplier: -5, ..default }),
            ("good quiet", SearchParameters { good_quiet_threshold: 0, ..default }),
        ];
        for (name, parameters) in moved {
            assert_ne!(nodes(parameters), default_nodes, "{name}: moving it alone should change the tree");
        }
    }

    /// A cutoff on a static value returns the midpoint of it and beta, never below beta, and its
    /// value alone against a mate or tablebase beta.
    #[test]
    fn a_static_cutoff_returns_the_midpoint_of_its_value_and_beta() {
        assert_eq!(Search::blended(300, 100), 200);
        assert_eq!(Search::blended(-99, -100), -99, "-199 / 2 truncates toward zero, above beta");
        assert_eq!(Search::blended(101, 100), 100, "never below beta");
        assert_eq!(Search::blended(-101, -100 - 2), -101);
        assert_eq!(Search::blended(MATE_SCORE - 3, mate_in(5)), MATE_SCORE - 3, "a mate beta");
        assert_eq!(Search::blended(tb_win_at(2), tb_win_at(4)), tb_win_at(2), "a tablebase beta");
        assert_eq!(Search::blended(-300, mated_in(5)), -300, "a mated beta");
    }

    /// An entry learns only the sign its bound proves, moves by `min(depth, 8) / 16` of the gap,
    /// takes errors clamped to two pawns; the correction is the three entries' average, clamped to
    /// one pawn.
    #[test]
    fn the_correction_history_learns_what_the_bound_proves_and_corrects_within_a_pawn() {
        let (board, _) = Board::from_fen("4k3/pp6/8/8/8/8/PP6/4K3 w - - 0 1").unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = quiescence_search(&tt, &stop_signal);
        let pawn = search.eval_margins.pawn;

        search.update_correction(&board, 8, 0, -40, Bound::Lower);
        search.update_correction(&board, 8, 0, 40, Bound::Upper);
        assert_eq!(search.correction(&board), 0, "a fail high below the evaluation, or a fail low above it, proves nothing");

        search.update_correction(&board, 8, 0, 40, Bound::Lower);
        assert_eq!(search.correction(&board), 20, "depth 8: halfway to the error");
        search.update_correction(&board, 1, 0, 36, Bound::Exact);
        assert_eq!(search.correction(&board), 21, "depth 1: a sixteenth of the gap");
        search.update_correction(&board, 20, 0, -40, Bound::Upper);
        // 21 + (-40 - 21) * 8 / 16 = 21 - 30, the division truncating toward zero.
        assert_eq!(search.correction(&board), -9, "depth 20 counts as 8");

        for _ in 0..20 {
            search.update_correction(&board, 8, 0, 10 * pawn, Bound::Exact);
        }
        assert_eq!(search.correction(&board), pawn, "the error clamped to two pawns, the correction to one");
    }

    #[test]
    fn choose_best_move_with_an_exhausted_budget_still_completes_depth_one() {
        // A `0`ms target means the headroom gate would reject every depth after the first --
        // by the time it's checked, `started.elapsed()` is already nonzero (the
        // `Instant::now()` call itself, and everything since, takes measurably longer than
        // the clock's own resolution). Depth `1` skips that gate entirely and always runs, so
        // a real move is still found rather than misreporting the root as having none.
        // Second, a7 pawn again -- see `alpha_beta_depth_one_prefers_the_move_that_wins_more_material`.
        // The exact depth passed here doesn't matter -- the `0`ms target below stops the loop
        // after depth 1 regardless of how high this is set.
        let (board, _) = Board::from_fen("4k3/p7/8/2p5/4N3/8/8/4K3 w - - 0 1").unwrap();
        let (reports, best_move) = choose_best_move(
            &board, 9, Some(TimeLimit::Fixed { target_ms: 0 }), &[], &TranspositionTable::new(1),
            &SyzygyTablebase::new(), None, 0, &AtomicBool::new(false),
        );
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].depth, 1);
        assert_eq!(best_move, Some(Move::new(Square::E4, Square::C5, None)));
    }

    #[test]
    fn choose_best_move_stops_immediately_at_a_terminal_root() {
        // No legal moves at all -- every one of `max_depth`'s iterations would rediscover the
        // exact same fact, so this must stop after the first and report nothing.
        // The exact depth passed here doesn't matter -- a terminal root (no legal moves) stops
        // the loop after the very first iteration regardless of how high this is set.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/5PPP/r5K1 w - - 0 1").unwrap();
        let (reports, best_move) = choose_best_move(
            &board, 9, None, &[], &TranspositionTable::new(1), &SyzygyTablebase::new(), None, 0,
            &AtomicBool::new(false),
        );
        assert!(reports.is_empty());
        assert_eq!(best_move, None);
    }

    #[test]
    fn choose_best_move_multipv_with_progress_reports_no_extra_lines_when_multipv_is_one() {
        // multipv <= 1 must behave exactly like plain choose_best_move_with_progress -- no extra
        // search_root calls, no extra reported lines, at every depth.
        let (board, _) = Board::from_fen("4k3/p7/8/2p5/4N3/8/8/4K3 w - - 0 1").unwrap();
        let mut every_extra_lines_len = Vec::new();
        let (reports, best_move) = choose_best_move_multipv_with_progress(
            &board, 3, None, &[], &TranspositionTable::new(1), &SyzygyTablebase::new(), None, None,
            &AtomicBool::new(false), 1, None, |_report, extra_lines| every_extra_lines_len.push(extra_lines.len()),
        );

        assert_eq!(reports.len(), 3);
        assert_eq!(every_extra_lines_len, vec![0, 0, 0]);
        assert_eq!(best_move, Some(Move::new(Square::E4, Square::C5, None)));
    }

    #[test]
    fn choose_best_move_multipv_with_progress_reports_the_second_best_root_move_as_an_extra_line() {
        // Same fixture as search_root_excludes_a_root_move_entirely_letting_the_next_best_one_win_
        // instead: the knight's capture on c5 is the unique best move, so multipv = 2 must report
        // exactly one extra line per depth, whose own move is never that capture and whose own
        // score never beats rank 1's.
        let (board, _) = Board::from_fen("4k3/p7/8/2p5/4N3/8/8/4K3 w - - 0 1").unwrap();
        let winning_capture = Move::new(Square::E4, Square::C5, None);
        let mut observed = Vec::new();
        let (reports, best_move) = choose_best_move_multipv_with_progress(
            &board, 3, None, &[], &TranspositionTable::new(1), &SyzygyTablebase::new(), None, None,
            &AtomicBool::new(false), 2, None, |report, extra_lines| observed.push((report.score, extra_lines.to_vec())),
        );

        assert_eq!(reports.len(), 3);
        assert_eq!(best_move, Some(winning_capture));
        for (best_score, extra_lines) in observed {
            assert_eq!(extra_lines.len(), 1, "multipv = 2 must report exactly one extra line per depth");
            assert_ne!(extra_lines[0].pv[0], winning_capture, "the second line must never repeat the best move");
            assert!(
                extra_lines[0].score < best_score,
                "the second line's own score must be strictly worse than rank 1's: {} vs {}",
                extra_lines[0].score, best_score
            );
        }
    }

    /// A minimal `IterationReport` for `select_best_result`'s own tests -- only `depth` and
    /// `score` matter to that function, so the rest are filled with whatever value is cheapest
    /// to write.
    fn report(depth: u32, score: i32) -> IterationReport {
        IterationReport { depth, seldepth: depth, score, nodes: 0, pv: Vec::new(), time_ms: 0 }
    }

    #[test]
    fn select_best_result_prefers_the_greater_completed_depth() {
        let shallow_but_high_score = || (vec![report(1, 500), report(2, 500)], Some(Move::new(Square::A2, Square::A3, None)));
        let deep_but_low_score = || (vec![report(1, 0), report(2, 0), report(3, 0)], Some(Move::new(Square::B2, Square::B3, None)));

        let (reports, best_move) = select_best_result(vec![shallow_but_high_score(), deep_but_low_score()]);
        assert_eq!(reports.last().unwrap().depth, 3);
        assert_eq!(best_move, Some(Move::new(Square::B2, Square::B3, None)));

        // Order shouldn't matter -- the same winner either way.
        let (reports, best_move) = select_best_result(vec![deep_but_low_score(), shallow_but_high_score()]);
        assert_eq!(reports.last().unwrap().depth, 3);
        assert_eq!(best_move, Some(Move::new(Square::B2, Square::B3, None)));
    }

    #[test]
    fn select_best_result_breaks_a_depth_tie_by_raw_score() {
        let lower_score = (vec![report(4, 100)], Some(Move::new(Square::A2, Square::A3, None)));
        let higher_score = (vec![report(4, 300)], Some(Move::new(Square::B2, Square::B3, None)));

        let (reports, best_move) = select_best_result(vec![lower_score, higher_score]);
        assert_eq!(reports.last().unwrap().score, 300);
        assert_eq!(best_move, Some(Move::new(Square::B2, Square::B3, None)));
    }

    #[test]
    fn select_best_result_prefers_any_completed_result_over_an_empty_one() {
        let empty = (Vec::new(), None);
        let completed = (vec![report(1, 0)], Some(Move::new(Square::A2, Square::A3, None)));

        let (reports, best_move) = select_best_result(vec![empty, completed]);
        assert_eq!(reports.last().unwrap().depth, 1);
        assert_eq!(best_move, Some(Move::new(Square::A2, Square::A3, None)));
    }

    #[test]
    fn select_best_result_is_empty_when_every_thread_reported_nothing() {
        let (reports, best_move) = select_best_result(vec![(Vec::new(), None), (Vec::new(), None)]);
        assert!(reports.is_empty());
        assert_eq!(best_move, None);
    }

    /// Searches `fen` as a leaf: `ply == target_depth == 1`, an open window.
    fn leaf_score(fen: &str) -> i32 {
        let (board, _) = Board::from_fen(fen).unwrap();
        let tt = TranspositionTable::new(1);
        let stop_signal = AtomicBool::new(false);
        let mut search = new_search(
            1, None, Vec::new(), vec![[None, None]; MAX_PLY as usize], [[0; 64]; 12], vec![0; CONTINUATION_HISTORY_LEN],
            &tt, &stop_signal,
        );
        search.alpha_beta_with_buffer(&board, 1, 1, -INFINITY, INFINITY)
    }

    #[test]
    fn a_stalemated_leaf_scores_a_draw_not_its_material() {
        // Black king a8, stalemated by the queen on c7 and the king on b6, a queen down.
        assert_eq!(leaf_score("k7/2Q5/1K6/8/8/8/8/8 b - - 0 1"), 0);
    }

    #[test]
    fn a_mated_leaf_scores_mate_at_its_ply() {
        // Black king a8, mated by the queen on b7, which the king on b6 defends.
        assert_eq!(leaf_score("k7/1Q6/1K6/8/8/8/8/8 b - - 0 1"), -(MATE_SCORE - 1));
    }

    /// Positions of every phase, from the opening to a rook ending.
    const FIXED_SEARCH_POSITIONS: &[&str] = &[
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
        "4rrk1/pp1n3p/3q2pQ/2p1pb2/2PP4/2P3N1/P2B2PP/4RRK1 b - - 7 19",
        "r1bq1rk1/ppp1nppp/4n3/3p3Q/3P4/1BP1B3/PP1N2PP/R4RK1 w - - 1 16",
        "6k1/6p1/6Pp/ppp5/3pn2P/1P3K2/1PP2P2/3N4 b - - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 11",
    ];

    /// `(nodes, score, best move)` of a depth-6 search of each `FIXED_SEARCH_POSITIONS` entry, with
    /// the PeSTO evaluation, one thread, and a fresh table. Recorded from the search as it
    /// stood, so a change meant to make the search faster without changing it (such as evaluating
    /// each node once) must reproduce them exactly. Re-recorded when the staged move picker replaced
    /// the old order, which changes the tree on purpose, and again when quiescence's pick began
    /// breaking ties by the move instead of its place in the buffer (`try_best_noisy_move`), and
    /// again when razoring moved to the shared `MARGIN_PER_PLY` (two pawns a ply, from one and a half),
    /// and again when the null-move reduction began growing with depth and margin (`null_move_plies`),
    /// and again when generation became staged (captures generated apart from the quiets, so equal
    /// captures meet in another order), and when pawn moves came to be generated by bitboard shifts
    /// (equal pawn moves meet in another order), and when the `pv` switch, kept, stopped razoring,
    /// reverse futility and null move at PV nodes, and when `root` and `ttquiet` were kept too (no
    /// classical futility at the root, no reverse futility with a quiet TT move), and when `corr`
    /// was kept (every static evaluation corrected by the correction history), and when `blend`
    /// was kept (reverse futility and stand pat returning the midpoint of their value and beta), and
    /// when `pv`, `root` and `ttquiet` were reverted, leaving `corr` and `blend`. Unchanged when the
    /// switches were removed, `corr` and `blend` becoming the search's own code. Re-recorded when the
    /// defaults moved (2026-11-08: razoring and SEE pruning a pawn and a half, reverse futility a
    /// pawn, the aspiration window 10, the good-quiet threshold -140): 16,896 nodes in all against
    /// 24,154, the same move in four positions of six.
    const FIXED_SEARCH_EXPECTED: &[(u64, i32, &str)] = &[
        (807, 22, "Some(Move { from: G1, to: F3, promotion: None, kind: Quiet })"),
        (4703, -37, "Some(Move { from: E2, to: A6, promotion: None, kind: Quiet })"),
        (4616, 101, "Some(Move { from: F5, to: D3, promotion: None, kind: Quiet })"),
        (4711, -51, "Some(Move { from: H2, to: H3, promotion: None, kind: Quiet })"),
        (830, -80, "Some(Move { from: E4, to: D6, promotion: None, kind: Quiet })"),
        (1229, 76, "Some(Move { from: B4, to: F4, promotion: None, kind: Quiet })"),
    ];

    #[test]
    fn a_fixed_depth_search_reproduces_its_recorded_nodes_scores_and_moves() {
        let observed: Vec<(u64, i32, String)> = FIXED_SEARCH_POSITIONS
            .iter()
            .map(|fen| {
                let (board, _) = Board::from_fen(fen).unwrap();
                let (reports, best_move) = choose_best_move(
                    &board, 6, None, &[], &TranspositionTable::new(16), &SyzygyTablebase::new(), None, 0,
                    &AtomicBool::new(false),
                );
                let last = reports.last().expect("a depth-6 search completes");
                (last.nodes, last.score, format!("{best_move:?}"))
            })
            .collect();
        let expected: Vec<(u64, i32, String)> =
            FIXED_SEARCH_EXPECTED.iter().map(|&(nodes, score, mv)| (nodes, score, mv.to_string())).collect();
        assert_eq!(observed, expected, "observed {observed:#?}");
    }
}
