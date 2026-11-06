//! Syzygy WDL-only tablebase support. The one sanctioned exception to CLAUDE.md rule 1: hand-writing a Syzygy parser from scratch is a binary-
//! format/compression exercise, not a chess-engine-internals one, so this depends on niklasf's own
//! pure-Rust `shakmaty-syzygy` instead. Everything else in this crate (board representation, move
//! generation, evaluation, search) stays hand-written, unaffected.
//!
//! Two uses, as Stockfish makes them. In the search, WDL only, and only right after a zeroing move
//! (`probe`); see `search.rs`'s own `TB_WIN_SCORE`/`tb_win_at`/`tb_loss_at` for how a result
//! becomes a search score. At the root, DTZ ranks the moves (`excluded_root_moves`), so that a won
//! ending is converted within the fifty-move rule. Both kinds of table are optional to the engine:
//! whatever `SyzygyPath` gives, none, WDL only or both, it plays correctly, since a tester may
//! disable them; without DTZ, the root is simply not ranked.

use shakmaty::{CastlingMode, Chess, FromSetup, Role, Setup};
use shakmaty_syzygy::{Dtz, Tablebase, Wdl};

use crate::board::Board;
use crate::board::chess_move::Move;
use crate::movegen::attacks::is_in_check;
use crate::movegen::legal::legal_moves;
use crate::board::color::Color;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;

/// A tablebase probe's own result, collapsed from `shakmaty_syzygy::Wdl`'s five values down to
/// three -- see `wdl_to_result`'s own doc comment for why `CursedWin`/`BlessedLoss` become `Draw`
/// here rather than `Win`/`Loss`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TbResult {
    Win,
    Draw,
    Loss,
}

/// `Wdl`'s two "conditional" values are properties of the position and material alone (whether the
/// shortest win takes more than 100 halfmoves without a zeroing move), never of how much of the
/// clock has already ticked by -- `CursedWin` means "a genuine win in unlimited-move chess, but the
/// 50-move rule forces a draw before conversion even from a freshly reset clock," and `BlessedLoss`
/// is its mirror. Under real tournament rules (the 50-move rule enforced), a cursed win can never
/// actually be forced and a blessed loss can never actually be lost -- so reporting either as a real
/// win/loss score would be dishonest about what the position actually delivers under the rules
/// Kalo Chess is meant to play by. Collapsed to `Draw`, the practically correct value, rather than kept
/// distinct.
fn wdl_to_result(wdl: Wdl) -> TbResult {
    match wdl {
        Wdl::Win => TbResult::Win,
        Wdl::CursedWin => TbResult::Draw,
        Wdl::Draw => TbResult::Draw,
        Wdl::BlessedLoss => TbResult::Draw,
        Wdl::Loss => TbResult::Loss,
    }
}

/// Whether `board` is even worth probing, before touching any table file: `probe_wdl_after_
/// zeroing` (the only WDL-only-compatible probe `shakmaty-syzygy` offers) is documented as correct
/// only for "a position reached directly after a capture or pawn move" -- i.e. a freshly reset
/// halfmove clock. Kalo Chess has no DTZ tables to handle the general, clock-aware case (`probe_wdl`
/// needs both WDL and DTZ), so a position reached after several quiet moves is simply not probed
/// at all, rather than risk reporting a stale classification that doesn't account for how much of
/// the 50-move count has already elapsed. `max_pieces` is `Tablebase::max_pieces()` -- a position
/// with more pieces than any loaded table covers is never probeable regardless of the clock.
fn should_probe(board: &Board, max_pieces: usize) -> bool {
    board.halfmove_clock() == 0 && board.all_occupancy().count() as usize <= max_pieces
}

/// `board` as the `shakmaty` position a probe takes, built field by field. A FEN round trip did the
/// same, formatting, parsing and allocating a string on every probe; a unit test below keeps the two
/// in agreement. Both squares number a1 = 0, b1 = 1, ..., h8 = 63, so a square converts by its
/// index. The fullmove number is irrelevant to a WDL probe and stays 1. `None` when `shakmaty`
/// refuses the position, never expected for a legal `Board`.
fn to_shakmaty(board: &Board) -> Option<Chess> {
    let mut placement = shakmaty::Board::empty();
    for square in board.all_occupancy().squares() {
        let piece = board.piece_at(square)?;
        let role = match piece.piece_type() {
            PieceType::Pawn => Role::Pawn,
            PieceType::Knight => Role::Knight,
            PieceType::Bishop => Role::Bishop,
            PieceType::Rook => Role::Rook,
            PieceType::Queen => Role::Queen,
            PieceType::King => Role::King,
        };
        placement.set_piece_at(to_shakmaty_square(square), shakmaty::Piece { color: to_shakmaty_color(piece.color()), role });
    }

    // `CastlingMode::Standard` names a right by its rook's starting square.
    let rights = board.castling_rights();
    let mut castling_rights = shakmaty::Bitboard::EMPTY;
    for (granted, rook) in [
        (rights.kingside(Color::White), Square::H1),
        (rights.queenside(Color::White), Square::A1),
        (rights.kingside(Color::Black), Square::H8),
        (rights.queenside(Color::Black), Square::A8),
    ] {
        if granted {
            castling_rights.add(to_shakmaty_square(rook));
        }
    }

    let setup = Setup {
        board: placement,
        turn: to_shakmaty_color(board.side_to_move()),
        castling_rights,
        ep_square: board.en_passant_square().map(to_shakmaty_square),
        halfmoves: u32::from(board.halfmove_clock()),
        ..Setup::empty()
    };
    Chess::from_setup(setup, CastlingMode::Standard).ok()
}

fn to_shakmaty_square(square: Square) -> shakmaty::Square {
    shakmaty::Square::new(square as u32)
}

fn to_shakmaty_color(color: Color) -> shakmaty::Color {
    match color {
        Color::White => shakmaty::Color::White,
        Color::Black => shakmaty::Color::Black,
    }
}

/// The top rank of a root move that wins within the fifty-move clock (`excluded_root_moves`), and its
/// negative the bottom one, of a move that loses within it.
const MAX_DTZ: i32 = 1 << 18;

/// The last `dtz + clock` a label takes as decided, the win's limit in the ranks.
const LAST_DECIDED_PLY: i32 = 99;

/// How close `dtz + clock` may come to `LAST_DECIDED_PLY` before `label_result` stops trusting the
/// root's DTZ, which the tables may round, and ranks the moves. A wide margin costs only probes.
const LABEL_MARGIN: i32 = 4;

/// Thin wrapper around `shakmaty_syzygy::Tablebase<Chess>`, adapting Kalo Chess's own `Board` to the
/// `shakmaty::Chess` position it expects (`to_shakmaty`).
pub struct SyzygyTablebase {
    inner: Tablebase<Chess>,
}

/// Plain, so a struct holding the tables can derive `Debug`: the tables themselves print nothing
/// worth reading.
impl std::fmt::Debug for SyzygyTablebase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SyzygyTablebase")
    }
}

impl SyzygyTablebase {
    /// Reads the tables through memory maps rather than `shakmaty-syzygy`'s default of one
    /// positioned read per access, several per probe: a synchronous Windows file handle serializes
    /// the reads issued through it, so threads probing the same table queued behind each other, and
    /// 16-thread self-play gained 20% positions per second from the maps alone. No `Default`: making
    /// one carries the unsafe contract below.
    #[allow(clippy::new_without_default)]
    pub fn new() -> SyzygyTablebase {
        // SAFETY: `with_mmap_filesystem` requires that no table file is modified while mapped, and
        // turns a read error into a crash. Nothing in Kalo Chess writes a table; `bin/syzygy.sh` can
        // rewrite an incomplete one in place, and must not run while a Kalo Chess process has the
        // tables open, as its header says. The tables live on a local disk, where a read error is
        // a failing disk, not a condition to recover from.
        SyzygyTablebase { inner: unsafe { Tablebase::with_mmap_filesystem() } }
    }

    /// Loads every table in `directories`, refusing a directory that is missing or holds no tables.
    /// For the tools that load their own tables in-process, where a silent miss would change how
    /// their games are played with nothing to say so. Where those directories are is theirs to know:
    /// the engine itself takes its tables from `SyzygyPath` (`add_directory`), whose failures a GUI
    /// sees as a weaker engine rather than a stopped one, as before.
    pub fn from_directories(directories: &[String]) -> Result<SyzygyTablebase, String> {
        let mut tablebase = SyzygyTablebase::new();
        for directory in directories {
            let tables = tablebase
                .add_directory(directory)
                .map_err(|error| format!("cannot read Syzygy tables from {directory}: {error} -- run bin/syzygy.sh"))?;
            if tables == 0 {
                return Err(format!("{directory} holds no Syzygy tables -- run bin/syzygy.sh"));
            }
        }
        Ok(tablebase)
    }

    /// Adds every table file `shakmaty_syzygy` recognizes inside `path` (its own `add_directory`
    /// selects by filename). Call once per directory in a `SyzygyPath` list (`uci.rs`'s own
    /// doc comment on that option covers why it's a list, not one path). A plain `io::Error`,
    /// not `SyzygyError` -- this is a filesystem operation (a missing/unreadable directory),
    /// distinct from `probe`'s own probing-time failures below.
    pub fn add_directory(&mut self, path: &str) -> std::io::Result<usize> {
        self.inner.add_directory(path)
    }

    /// Probes `board`, or returns `None` when it isn't a position `should_probe` accepts,
    /// `shakmaty` refuses it (never expected for a real, legal `Board`, but an `Option` all the
    /// same, not a panic -- a probe miss is an ordinary outcome, not a corruption the way a bad
    /// cache magic byte would be), or the underlying probe itself errors (e.g. a material
    /// signature with no matching table file among whatever directories were actually added).
    pub fn probe(&self, board: &Board) -> Option<TbResult> {
        if !should_probe(board, self.inner.max_pieces()) {
            return None;
        }
        let position = to_shakmaty(board)?;
        let wdl = self.inner.probe_wdl_after_zeroing(&position).ok()?;
        Some(wdl_to_result(wdl))
    }

    /// The root moves a search of `board` must not consider, from the DTZ tables: every legal move
    /// outside the best rank. `ancestors` is the game's positions before `board`, oldest first, as
    /// the search takes them. `None` when the root is not ranked, which leaves the search as it was
    /// without DTZ: more pieces than the tables, castling rights left, or any probe failing (no DTZ
    /// tables given, or one missing for a material reached).
    ///
    /// Stockfish's `Tablebases::root_probe` (fetched from `syzygy/tbprobe.cpp`, 2026-10-07) with
    /// its fifty-move rule on and its `rankDTZ` off. Each move's DTZ is that of the position after
    /// it, seen from the mover: from WDL when the move zeroes the clock, 0 when it draws by
    /// repetition or the fifty-move rule, otherwise the child's DTZ negated and moved one ply away
    /// from zero; a mate in one is 1. Ranks, `cnt50` being the root's clock:
    ///
    /// - a win within the clock (`dtz + cnt50 <= 99`, no repetition since the last zeroing move):
    ///   `MAX_DTZ`, every such move alike, so the search chooses among them by its own evaluation;
    /// - a win the clock frustrates: `MAX_DTZ / 2 - (dtz + cnt50)`;
    /// - a loss within the clock (`-2 * dtz + cnt50 < 100`): `-MAX_DTZ`;
    /// - a loss the clock saves: `-MAX_DTZ / 2 + (-dtz + cnt50)`;
    /// - a draw: 0.
    ///
    /// As the clock runs, fewer wins stay within it, so only moves that make progress keep the
    /// top rank: a won ending is converted under the fifty-move rule.
    pub fn excluded_root_moves(&self, board: &Board, ancestors: &[u64]) -> Option<Vec<Move>> {
        let ranked = self.root_ranks(board, ancestors)?;
        let best = ranked.iter().map(|ranked| ranked.rank).max()?;
        Some(ranked.iter().filter(|ranked| ranked.rank < best).map(|ranked| ranked.mv).collect())
    }

    /// The result at `board` under the fifty-move rule, from its best root rank (`root_ranks`): a
    /// win if some move wins within the clock, a loss if every move loses within it, a draw
    /// otherwise (a win the clock frustrates, a loss it saves, or a draw; also a win after a
    /// repetition since the last zeroing move, which the ranking cannot tell from a frustrated one,
    /// and which is rare). `None` where the root is
    /// not ranked, or has no legal move. For labelling the replay without a search; `ancestors` as
    /// `excluded_root_moves` takes them, the positions before `board` only.
    pub fn root_result(&self, board: &Board, ancestors: &[u64]) -> Option<TbResult> {
        let best = self.root_ranks(board, ancestors)?.iter().map(|ranked| ranked.rank).max()?;
        Some(result_of_rank(best))
    }

    /// The result at `board` as a training label: a win or a loss when it is decided within the
    /// fifty-move clock (`dtz + clock <= LAST_DECIDED_PLY`, the same limit for both sides, so that a
    /// line of best play keeps its value from one ply to the next), a draw otherwise; also a draw
    /// after a repetition since the last zeroing move, for both sides. `root_result` differs at a
    /// loss, by the search's move ordering: it takes a loss of 50 plies or more for a draw, while
    /// the same ending seen from the winner's side is a win. `None` as `root_ranks` gives it, or
    /// where the root has no legal move.
    ///
    /// One DTZ probe of the root rather than `root_ranks`' one per legal move, which cost a scatter
    /// a table read for each: `probe_dtz` answers a drawn position from the WDL table alone. Only
    /// when a rule can matter move by move, or the DTZ is close enough to the limit for its rounding
    /// to matter (`LABEL_MARGIN`), does it rank the moves (`label_by_ranks`), whose answer it always
    /// equals.
    pub fn label_result(&self, board: &Board, ancestors: &[u64]) -> Option<TbResult> {
        if !self.root_rankable(board) || legal_moves(board, board.side_to_move()).is_empty() {
            return None;
        }
        let cnt50 = board.halfmove_clock() as i32;
        // From clock 98 a child can reach the fifty-move draw, and a repetition can make a child a
        // threefold: neither shows in the root's DTZ.
        if cnt50 >= 98 || repeated_since_zeroing(board, ancestors) {
            return self.label_by_ranks(board, ancestors);
        }
        let dtz = self.inner.probe_dtz(&to_shakmaty(board)?).ok()?.ignore_rounding().0;
        if dtz == 0 {
            return Some(TbResult::Draw);
        }
        let last = dtz.abs() + cnt50;
        if (last - LAST_DECIDED_PLY).abs() <= LABEL_MARGIN {
            return self.label_by_ranks(board, ancestors);
        }
        Some(if last > LAST_DECIDED_PLY {
            TbResult::Draw
        } else if dtz > 0 {
            TbResult::Win
        } else {
            TbResult::Loss
        })
    }

    /// `label_result` from every root move's DTZ: the moves' ranks for a win, and for a loss, every
    /// move losing, the longest defence against the limit.
    fn label_by_ranks(&self, board: &Board, ancestors: &[u64]) -> Option<TbResult> {
        let ranked = self.root_ranks(board, ancestors)?;
        let best_dtz = ranked.iter().map(|ranked| ranked.dtz).max()?;
        if best_dtz > 0 {
            let best = ranked.iter().map(|ranked| ranked.rank).max()?;
            return Some(result_of_rank(best));
        }
        if best_dtz == 0 {
            return Some(TbResult::Draw);
        }
        let longest = ranked.iter().map(|ranked| ranked.dtz).min()?;
        let decided = -longest + board.halfmove_clock() as i32 <= LAST_DECIDED_PLY && !repeated_since_zeroing(board, ancestors);
        Some(if decided { TbResult::Loss } else { TbResult::Draw })
    }

    /// `root_result` and a best move by the same ranking, ranked once, for the replay's label of a
    /// position the tables know: one of the moves of the best rank, the moves the search itself
    /// chooses among in play (`excluded_root_moves`), so the label never prefers what play does not.
    /// `preferred`, the game's own move, when it is one of them, the choice of a strong engine;
    /// otherwise the first in generation order. No preference by DTZ, as in play: every win within
    /// the clock is alike. Never a move that spoils a win or loses a draw.
    pub fn root_result_and_move(&self, board: &Board, ancestors: &[u64], preferred: Option<Move>) -> Option<(TbResult, Move)> {
        let (result, best) = self.root_result_and_best_moves(board, ancestors)?;
        let chosen = preferred.filter(|mv| best.iter().any(|&(candidate, _)| candidate == *mv)).unwrap_or(best[0].0);
        Some((result, chosen))
    }

    /// `root_result` and every move of the best rank with its DTZ, from the mover's side (positive
    /// winning, 1 a mate in one, negative losing, 0 drawing), in generation order. For self-play,
    /// which plays a ranked root without a search: every win within the clock shares the best rank,
    /// so it must break the tie by progress itself (Stockfish's `rankDTZ`), which play leaves to the
    /// search. `None` where the root is not ranked, or has no legal move.
    pub fn root_result_and_best_moves(&self, board: &Board, ancestors: &[u64]) -> Option<(TbResult, Vec<(Move, i32)>)> {
        let ranked = self.root_ranks(board, ancestors)?;
        let best = ranked.iter().map(|ranked| ranked.rank).max()?;
        let moves = ranked.iter().filter(|ranked| ranked.rank == best).map(|ranked| (ranked.mv, ranked.dtz)).collect();
        Some((result_of_rank(best), moves))
    }

    /// Whether the tables can rank `board`: no more pieces than they cover, and no castling right.
    fn root_rankable(&self, board: &Board) -> bool {
        let rights = board.castling_rights();
        let any_castling = [Color::White, Color::Black].iter().any(|&color| rights.kingside(color) || rights.queenside(color));
        board.all_occupancy().count() as usize <= self.inner.max_pieces() && !any_castling
    }

    /// Every legal move at `board` with its rank (`excluded_root_moves`' comment), or `None` where
    /// the root is not ranked.
    fn root_ranks(&self, board: &Board, ancestors: &[u64]) -> Option<Vec<RankedMove>> {
        if !self.root_rankable(board) {
            return None;
        }

        let cnt50 = board.halfmove_clock() as i32;
        let window = repetition_window(board, ancestors);
        let repeated = repeated_since_zeroing(board, ancestors);

        let mover = board.side_to_move();
        let moves = legal_moves(board, mover);
        let mut ranks = Vec::with_capacity(moves.len());
        for &mv in &moves {
            let mut child = *board;
            child.make_move(mv);
            let child_moves = legal_moves(&child, child.side_to_move());
            let in_check = is_in_check(&child, child.side_to_move());
            // A threefold repetition: the child seen twice already since the last zeroing move.
            let threefold = child.halfmove_clock() != 0 && window.iter().filter(|&&hash| hash == child.hash()).count() >= 2;
            let fifty = child.halfmove_clock() >= 100;

            // A mate or stalemate is settled here, before any probe: Stockfish relies on its prober's
            // DTZ for a mated position (then corrects it to 1), which `shakmaty-syzygy` need not
            // give the same way.
            let dtz = if child_moves.is_empty() {
                if in_check { 1 } else { 0 }
            } else if child.halfmove_clock() == 0 {
                let wdl = -self.inner.probe_wdl_after_zeroing(&to_shakmaty(&child)?).ok()?;
                Dtz::before_zeroing(wdl).0
            } else if fifty || threefold {
                0
            } else {
                let child_dtz = -self.inner.probe_dtz(&to_shakmaty(&child)?).ok()?.ignore_rounding().0;
                if child_dtz > 0 { child_dtz + 1 } else if child_dtz < 0 { child_dtz - 1 } else { 0 }
            };

            let rank = if dtz > 0 {
                if dtz + cnt50 <= 99 && !repeated { MAX_DTZ } else { MAX_DTZ / 2 - (dtz + cnt50) }
            } else if dtz < 0 {
                if -dtz * 2 + cnt50 < 100 { -MAX_DTZ } else { -MAX_DTZ / 2 + (-dtz + cnt50) }
            } else {
                0
            };
            ranks.push(RankedMove { mv, rank, dtz });
        }
        Some(ranks)
    }
}

/// The positions since the last zeroing move, root included: what a repetition can match.
fn repetition_window(board: &Board, ancestors: &[u64]) -> Vec<u64> {
    let cnt50 = board.halfmove_clock() as usize;
    let mut window = ancestors[ancestors.len().saturating_sub(cnt50)..].to_vec();
    window.push(board.hash());
    window
}

/// Whether a position has repeated since the last zeroing move.
fn repeated_since_zeroing(board: &Board, ancestors: &[u64]) -> bool {
    let window = repetition_window(board, ancestors);
    window.iter().enumerate().any(|(index, hash)| window[..index].contains(hash))
}

/// A root move, its rank by the DTZ tables (`excluded_root_moves`' comment), and the DTZ the rank
/// was computed from.
struct RankedMove {
    mv: Move,
    rank: i32,
    dtz: i32,
}

/// The result of a root whose best move has `rank`.
fn result_of_rank(rank: i32) -> TbResult {
    if rank == MAX_DTZ {
        TbResult::Win
    } else if rank == -MAX_DTZ {
        TbResult::Loss
    } else {
        TbResult::Draw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A missing directory and an empty one are both refused, naming the directory: the caller loads
    /// tables in-process and must not play on without them unnoticed.
    #[test]
    fn from_directories_refuses_a_missing_or_empty_directory() {
        let empty = std::env::temp_dir().join(format!("kalo-chess-tablebase-empty-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let empty = empty.to_str().unwrap().to_string();
        let missing = format!("{empty}/missing");

        let refused = SyzygyTablebase::from_directories(std::slice::from_ref(&missing)).expect_err("a missing directory");
        assert!(refused.contains(&missing), "{refused}");
        let refused = SyzygyTablebase::from_directories(std::slice::from_ref(&empty)).expect_err("an empty directory");
        assert!(refused.contains("holds no Syzygy tables"), "{refused}");
        std::fs::remove_dir_all(&empty).ok();
    }

    /// The data root's tables, WDL and DTZ, or `None` (and a note) where `bin/syzygy.sh` has not run:
    /// the tests that need real tables are skipped rather than failed on such a machine.
    fn local_tables() -> Option<SyzygyTablebase> {
        let root = crate::pipeline::data_root::data_root();
        let directories: Vec<String> =
            ["3-4-5-wdl", "6-wdl", "3-4-5-dtz", "6-dtz"].iter().map(|set| format!("{root}/chess/syzygy/{set}")).collect();
        let tables = SyzygyTablebase::from_directories(&directories).ok();
        if tables.is_none() {
            println!("skipped: no Syzygy WDL and DTZ tables under {root}/chess/syzygy");
        }
        tables
    }

    /// `excluded_root_moves`' moves as UCI strings, sorted, for comparing with a list.
    fn names(moves: &[Move]) -> Vec<String> {
        let mut names: Vec<String> = moves.iter().map(|&mv| crate::uci::format_move(mv)).collect();
        names.sort();
        names
    }

    #[test]
    fn without_tables_or_with_castling_rights_the_root_is_not_ranked() {
        let (board, _) = Board::from_fen("k7/7Q/1K6/8/8/8/8/8 w - - 0 1").unwrap();
        assert_eq!(SyzygyTablebase::new().excluded_root_moves(&board, &[]), None, "no tables at all");
        let Some(tables) = local_tables() else { return };
        let (castling, _) = Board::from_fen("4k3/8/8/8/8/8/8/R3K3 w Q - 0 1").unwrap();
        assert_eq!(tables.excluded_root_moves(&castling, &[]), None, "castling rights left");
    }

    #[test]
    fn a_move_that_throws_the_win_away_is_excluded_and_the_winning_ones_kept() {
        // Black's king on a8, White's on b6: the queen mates on a7, b7, g8 or h8, and Qc7
        // stalemates. Every winning move shares the top rank, the stalemate is excluded.
        let Some(tables) = local_tables() else { return };
        let (board, _) = Board::from_fen("k7/7Q/1K6/8/8/8/8/8 w - - 0 1").unwrap();
        let excluded = names(&tables.excluded_root_moves(&board, &[]).expect("KQ vs K is in the tables"));
        assert!(excluded.contains(&"h7c7".to_string()), "the stalemate must be excluded: {excluded:?}");
        assert!(!excluded.contains(&"h7h8".to_string()), "a mate must be kept: {excluded:?}");
        let kept = legal_moves(&board, Color::White).len() - excluded.len();
        assert!(kept > 1, "more than the mate wins within the clock at 0: {excluded:?}");
    }

    #[test]
    fn late_in_the_fifty_move_count_only_the_moves_that_still_win_in_time_are_kept() {
        // At clock 98, one ply is left before the fifty-move draw: only the mates in one still win.
        let Some(tables) = local_tables() else { return };
        let (board, _) = Board::from_fen("k7/7Q/1K6/8/8/8/8/8 w - - 98 80").unwrap();
        let excluded = tables.excluded_root_moves(&board, &[]).expect("KQ vs K is in the tables");
        let kept: Vec<Move> = legal_moves(&board, Color::White).into_iter().filter(|mv| !excluded.contains(mv)).collect();
        assert_eq!(names(&kept), ["h7a7", "h7b7", "h7g8", "h7h8"].map(String::from).to_vec());
    }

    /// `label_result`'s one probe answers as ranking every move does: random walks through endings,
    /// long ones among them (KBN vs K, KRB vs KR), from clocks on both sides of the limits. A fixed
    /// generator, so a failure repeats.
    #[test]
    fn the_label_from_one_probe_agrees_with_the_label_from_every_move() {
        let Some(tables) = local_tables() else { return };
        for (placement, side) in LABEL_FENS {
            for clock in [0, 30, 45, 60, 75, 85, 92, 96] {
                for seed in 1..=8u64 {
                    let (mut board, _) = Board::from_fen(&format!("{placement} {side} - - {clock} 1")).unwrap();
                    let mut random = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
                    let mut ancestors = Vec::new();
                    for _ in 0..24 {
                        let fen = board.to_fen(1);
                        assert_eq!(tables.label_result(&board, &ancestors), tables.label_by_ranks(&board, &ancestors), "{fen}");
                        let moves = legal_moves(&board, board.side_to_move());
                        if moves.is_empty() {
                            break;
                        }
                        random ^= random << 13;
                        random ^= random >> 7;
                        random ^= random << 17;
                        ancestors.push(board.hash());
                        board.make_move(moves[(random % moves.len() as u64) as usize]);
                    }
                }
            }
        }
    }

    const LABEL_FENS: [(&str, &str); 7] = [
        ("8/8/8/4k3/8/8/8/1Q2K3", "b"),
        ("8/8/8/4k3/8/8/8/BN2K3", "b"),
        ("8/8/2r5/4k3/8/8/8/RB2K3", "w"),
        ("8/4p3/8/4k3/8/8/4P3/4K3", "w"),
        ("8/2p5/8/4k3/8/8/1N6/1N2K3", "w"),
        ("7q/8/8/4k3/8/8/1P6/1Q2K3", "b"),
        ("8/6r1/8/4k3/8/8/1P6/1R2K3", "w"),
    ];

    /// A loss the clock leaves time for is a loss, whatever the DTZ against half the clock, and every
    /// move of the loser leaves the winner a win: `root_result` would read the same position as a
    /// draw from this clock on, the case the label exists for. KQ vs K wins in at most 20 plies.
    #[test]
    fn a_loss_within_the_clock_is_labelled_as_its_win_is() {
        let Some(tables) = local_tables() else { return };
        let (board, _) = Board::from_fen("8/8/8/4k3/8/8/8/1Q2K3 b - - 75 80").unwrap();
        assert_eq!(tables.label_result(&board, &[]), Some(TbResult::Loss));
        for mv in legal_moves(&board, board.side_to_move()) {
            let mut child = board;
            child.make_move(mv);
            assert_eq!(tables.label_result(&child, &[]), Some(TbResult::Win));
        }
    }

    /// A repetition reads as a draw for both sides, and a label has `root_ranks`' `None`s.
    #[test]
    fn the_label_of_a_repetition_or_an_unranked_root() {
        let Some(tables) = local_tables() else { return };
        for fen in ["8/8/8/4k3/8/8/8/1Q2K3 w - - 3 10", "8/8/8/4k3/8/8/8/1Q2K3 b - - 3 10"] {
            let (board, _) = Board::from_fen(fen).unwrap();
            assert_eq!(tables.label_result(&board, &[board.hash()]), Some(TbResult::Draw), "{fen}");
        }
        let (mated, _) = Board::from_fen("k7/1Q6/1K6/8/8/8/8/8 b - - 0 1").unwrap();
        assert_eq!(tables.label_result(&mated, &[]), None);
        let (castling, _) = Board::from_fen("8/8/8/4k3/8/8/8/R3K3 w Q - 0 1").unwrap();
        assert_eq!(tables.label_result(&castling, &[]), None);
    }

    /// The speed of one probe against ranking every move, printed. Not a pass or a fail: run it by
    /// hand, on the machine holding the tables, idle:
    /// `cargo test -p kalo-chess --release label_speed -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn label_speed() {
        let Some(tables) = local_tables() else { return };
        let mut boards = Vec::new();
        for (placement, side) in LABEL_FENS {
            for clock in 0..40 {
                let (board, _) = Board::from_fen(&format!("{placement} {side} - - {clock} 1")).unwrap();
                for &mv in &legal_moves(&board, board.side_to_move()) {
                    let mut child = board;
                    child.make_move(mv);
                    boards.push(child);
                }
            }
        }
        let start = std::time::Instant::now();
        boards.iter().for_each(|board| {
            std::hint::black_box(tables.label_by_ranks(board, &[]));
        });
        let by_ranks = start.elapsed();
        let start = std::time::Instant::now();
        boards.iter().for_each(|board| {
            std::hint::black_box(tables.label_result(board, &[]));
        });
        println!("{} positions: every move {by_ranks:?}, one probe {:?}", boards.len(), start.elapsed());
    }

    /// The root's result follows the clock: KQ vs K is won at clock 0, still won at 98 (the mates in
    /// one), and drawn at 99, when no move can mate before the fifty-move draw; the bare king's side
    /// loses. And the history must hold only the positions before the root: with the root itself at
    /// its end, as the arena's games once passed it, the win reads as a repetition.
    #[test]
    fn the_root_result_follows_the_fifty_move_clock_and_needs_the_history_before_the_root() {
        let Some(tables) = local_tables() else { return };
        let result = |fen: &str, ancestors: &[u64]| {
            let (board, _) = Board::from_fen(fen).unwrap();
            tables.root_result(&board, ancestors)
        };
        assert_eq!(result("k7/7Q/1K6/8/8/8/8/8 w - - 0 1", &[]), Some(TbResult::Win));
        assert_eq!(result("k7/7Q/1K6/8/8/8/8/8 w - - 98 80", &[]), Some(TbResult::Win));
        assert_eq!(result("k7/7Q/1K6/8/8/8/8/8 w - - 99 80", &[]), Some(TbResult::Draw));
        assert_eq!(result("8/8/8/4k3/8/8/8/1Q2K3 b - - 0 1", &[]), Some(TbResult::Loss));

        let (board, _) = Board::from_fen("8/8/8/4k3/8/8/8/1Q2K3 w - - 3 10").unwrap();
        assert_eq!(tables.root_result(&board, &[]), Some(TbResult::Win));
        assert_eq!(tables.root_result(&board, &[board.hash()]), Some(TbResult::Draw), "the root read as repeated");
        assert_eq!(SyzygyTablebase::new().root_result(&board, &[]), None, "no tables at all");
    }

    /// The replay's tablebase move: one of the moves play chooses among, the game's when it is one,
    /// and never one that spoils a win or loses a draw.
    #[test]
    fn the_root_move_is_one_play_would_choose_and_never_spoils_the_result() {
        let Some(tables) = local_tables() else { return };
        let uci = |board: &Board, text: &str| crate::uci::parse_move(text, &legal_moves(board, board.side_to_move())).unwrap();
        let chosen = |board: &Board, preferred: Option<Move>| {
            let (result, mv) = tables.root_result_and_move(board, &[], preferred).expect("in the tables");
            (result, crate::uci::format_move(mv))
        };

        // Every win within the clock is alike, as in play: the game's winning move is kept, slower
        // than a mate in one or not; its stalemate never is.
        let (won, _) = Board::from_fen("k7/7Q/1K6/8/8/8/8/8 w - - 0 1").unwrap();
        let excluded = tables.excluded_root_moves(&won, &[]).unwrap();
        let (result, mv) = chosen(&won, None);
        assert_eq!(result, TbResult::Win);
        assert!(!excluded.contains(&uci(&won, &mv)), "{mv} is not one of play's moves");
        assert_eq!(chosen(&won, Some(uci(&won, "h7h8"))).1, "h7h8", "the game's mate is kept");
        assert_eq!(chosen(&won, Some(uci(&won, "h7h6"))).1, "h7h6", "the game's slower win is kept too");
        assert_ne!(chosen(&won, Some(uci(&won, "h7c7"))).1, "h7c7", "the game's stalemate is not the label");

        // KR vs KR, the rooks on different ranks: Ra5 hangs the rook to Rxa5, Ra3 holds the draw.
        let (drawn, _) = Board::from_fen("2k5/8/8/7r/R7/8/8/2K5 w - - 0 1").unwrap();
        let (result, mv) = chosen(&drawn, Some(uci(&drawn, "a4a5")));
        assert_eq!(result, TbResult::Draw);
        assert_ne!(mv, "a4a5", "the game's losing move is not the label");
        assert_eq!(chosen(&drawn, Some(uci(&drawn, "a4a3"))).1, "a4a3", "the game's drawing move is kept");
    }

    /// Self-play's tablebase moves: every move of the best rank, with the DTZ it was ranked by.
    #[test]
    fn the_best_moves_carry_their_dtz_and_share_the_best_rank() {
        let Some(tables) = local_tables() else { return };
        let uci = |moves: &[(Move, i32)]| -> Vec<(String, i32)> {
            moves.iter().map(|&(mv, dtz)| (crate::uci::format_move(mv), dtz)).collect()
        };

        // KQ vs K: every listed move wins, the four mates in one at DTZ 1, the stalemate absent.
        let (won, _) = Board::from_fen("k7/7Q/1K6/8/8/8/8/8 w - - 0 1").unwrap();
        let (result, best) = tables.root_result_and_best_moves(&won, &[]).expect("in the tables");
        let best = uci(&best);
        assert_eq!(result, TbResult::Win);
        assert!(best.iter().all(|&(_, dtz)| dtz > 0), "every best move wins: {best:?}");
        for mate in ["h7a7", "h7b7", "h7g8", "h7h8"] {
            assert!(best.contains(&(mate.to_string(), 1)), "{mate} at DTZ 1: {best:?}");
        }
        assert!(!best.iter().any(|(mv, _)| mv == "h7c7"), "the stalemate is not a best move: {best:?}");

        // KR vs KR: a draw, every listed move drawing, the rook-hanging Ra5 absent.
        let (drawn, _) = Board::from_fen("2k5/8/8/7r/R7/8/8/2K5 w - - 0 1").unwrap();
        let (result, best) = tables.root_result_and_best_moves(&drawn, &[]).expect("in the tables");
        let best = uci(&best);
        assert_eq!(result, TbResult::Draw);
        assert!(best.iter().all(|&(_, dtz)| dtz == 0), "every best move draws: {best:?}");
        assert!(!best.iter().any(|(mv, _)| mv == "a4a5"), "the losing move is not a best move: {best:?}");

        // The bare king's side, losing: every move loses, so every move is listed.
        let (lost, _) = Board::from_fen("8/8/8/4k3/8/8/8/1Q2K3 b - - 0 1").unwrap();
        let (result, best) = tables.root_result_and_best_moves(&lost, &[]).expect("in the tables");
        assert_eq!(result, TbResult::Loss);
        assert_eq!(best.len(), legal_moves(&lost, Color::Black).len());
        assert!(best.iter().all(|&(_, dtz)| dtz < 0), "every move loses");
    }

    /// Plays `fen` out with both sides searching to `depth`, the game history passed as a real game
    /// passes it, and returns the ply on which the defender was mated, or `None` if the game reached
    /// the fifty-move draw (or 200 plies) first.
    fn plies_to_mate(tables: &SyzygyTablebase, fen: &str, depth: u32) -> Option<u32> {
        use crate::search::choose_best_move;
        use crate::tt::TranspositionTable;
        use std::sync::atomic::AtomicBool;

        let (mut board, _) = Board::from_fen(fen).unwrap();
        let mut ancestors: Vec<u64> = Vec::new();
        let mut tt = TranspositionTable::new(16);
        let stop = AtomicBool::new(false);
        for ply in 0..200 {
            let mover = board.side_to_move();
            if legal_moves(&board, mover).is_empty() {
                return if is_in_check(&board, mover) { Some(ply) } else { None };
            }
            if board.halfmove_clock() >= 100 {
                return None;
            }
            tt.new_search();
            let (_, best) = choose_best_move(&board, depth, None, &ancestors, &tt, tables, None, 0, &stop);
            ancestors.push(board.hash());
            board.make_move(best.expect("a position with legal moves has a best move"));
        }
        None
    }

    #[test]
    fn the_engine_mates_with_queen_and_with_rook_within_the_fifty_move_rule() {
        // Far starts, kings apart, so no shallow search sees the mate from the first move: only the
        // ranking keeps the conversion moving.
        let Some(tables) = local_tables() else { return };
        for fen in ["8/8/8/4k3/8/8/8/1Q2K3 w - - 0 1", "8/8/8/4k3/8/8/8/R3K3 w - - 0 1"] {
            let mate = plies_to_mate(&tables, fen, 6);
            assert!(mate.is_some(), "{fen}: not mated within the fifty-move rule");
        }
    }

    #[test]
    fn wdl_to_result_maps_unconditional_values_directly() {
        assert_eq!(wdl_to_result(Wdl::Win), TbResult::Win);
        assert_eq!(wdl_to_result(Wdl::Draw), TbResult::Draw);
        assert_eq!(wdl_to_result(Wdl::Loss), TbResult::Loss);
    }

    #[test]
    fn wdl_to_result_collapses_50_move_conditional_values_to_draw() {
        // Neither can actually be forced under real tournament rules (the 50-move rule
        // enforced) even from a freshly reset clock -- reporting either as a real win/loss would
        // claim a result the rules don't actually deliver.
        assert_eq!(wdl_to_result(Wdl::CursedWin), TbResult::Draw);
        assert_eq!(wdl_to_result(Wdl::BlessedLoss), TbResult::Draw);
    }

    #[test]
    fn should_probe_rejects_a_position_with_more_pieces_than_any_loaded_table_covers() {
        let (board, _) = Board::from_fen("8/8/8/4k3/8/8/4P3/4K3 w - - 0 1").unwrap();
        assert!(should_probe(&board, 3));
        assert!(!should_probe(&board, 2));
    }

    #[test]
    fn should_probe_rejects_a_position_reached_by_a_quiet_move() {
        // `probe_wdl_after_zeroing` is only documented as correct directly after a capture or
        // pawn move -- a nonzero halfmove clock means neither happened most recently.
        let (mut board, _) = Board::from_fen("8/8/8/4k3/8/8/4P3/4K3 w - - 3 5").unwrap();
        assert!(!should_probe(&board, 6));
        board = Board::from_fen("8/8/8/4k3/8/8/4P3/4K3 w - - 0 1").unwrap().0;
        assert!(should_probe(&board, 6));
    }

    /// The direct conversion gives the very position the FEN round trip it replaced gave: piece
    /// placement, both colours to move, every castling right, en passant squares with and without
    /// a capture available, and a nonzero clock.
    #[test]
    fn to_shakmaty_agrees_with_the_fen_round_trip() {
        use std::str::FromStr;

        use shakmaty::EnPassantMode;
        use shakmaty::fen::Fen;

        for fen in [
            "8/8/8/4k3/8/8/4P3/4K3 w - - 0 1",
            "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
            "r3k2r/8/8/8/8/8/8/R3K2R b Kq - 4 9",
            "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
            "4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1",
            "4k3/8/8/8/4P3/8/8/4K3 b - e3 0 1",
            "8/8/8/8/8/2k5/8/K1q5 w - - 0 1",
            "8/2n5/8/3QK3/8/8/8/1b3k2 b - - 0 1",
        ] {
            let (board, _) = Board::from_fen(fen).unwrap();
            let direct = to_shakmaty(&board).unwrap_or_else(|| panic!("refused {fen}"));
            let round_trip: Chess = Fen::from_str(&board.to_fen(1)).unwrap().into_position(CastlingMode::Standard).unwrap();
            assert_eq!(
                Fen::from_position(&direct, EnPassantMode::Always).to_string(),
                Fen::from_position(&round_trip, EnPassantMode::Always).to_string(),
                "{fen}"
            );
        }
    }

    #[test]
    fn probe_returns_none_when_no_tables_are_loaded() {
        let tablebase = SyzygyTablebase::new();
        let (board, _) = Board::from_fen("8/8/8/4k3/8/8/4P3/4K3 w - - 0 1").unwrap();
        assert_eq!(tablebase.probe(&board), None);
    }
}
