//! Self-play games: Fattorini playing itself, in-process -- no UCI subprocess, no
//! `EngineRegistry`/`UciClient`, both of which exist specifically for driving *arbitrary external*
//! engines -- at a fixed depth. `chores` plays them for `smithy start --forge`'s self-play
//! chores and sends each to the smithy as it ends. Without a trained network, it plays with the PeSTO
//! evaluation (`eval::evaluate`) a plain `fattorini` build uses.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use rand::RngExt;

use fattorini::board::chess_move::Move;
use fattorini::board::color::Color;
use fattorini::board::Board;
use fattorini::movegen::attacks::is_in_check;
use fattorini::movegen::legal::legal_moves;
use fattorini::nnue::Network;
use fattorini::search::{
    choose_best_move_multipv_with_progress, choose_best_move_with_progress, is_insufficient_material, MultiPvLine, SearchParameters,
    MATE_SCORE, MAX_PLY,
};
use fattorini::tablebase::SyzygyTablebase;
use fattorini::tt::TranspositionTable;
use fattorini::uci::format_move;

use super::pgn::{self, GameHeaders, MoveAnnotation, Termination};
use super::tablebase_label::{self_play_tablebase_move, tablebase_score};

/// The tests' search depth, the server's: the labels are Search(9) scores, and the round robin ranks
/// the candidates under that same search. A client plays the depth its order gives.
#[cfg(test)]
const DEFAULT_SEARCH_DEPTH: u32 = fattorini::pipeline::generation::SEARCH_DEPTH;
/// Per game, both sides sharing it, fixed. The arena's default (`engines.rs`): a depth-9 search
/// needs about 1 MB of entries, so 16 MB is ample and keeps a game per core within a small machine's
/// memory.
pub(crate) const TT_SIZE_MB: usize = 16;

/// How many of the game's own first plies use the exploration mechanism below instead of always
/// playing the best move -- the tournament design's "Diversity beyond the opening book's own 2.5M
/// positions" section.
const EXPLORATION_PLIES: u32 = 6;

/// A candidate move survives the exploration filter only if it's within this many centipawns of
/// the best move found at the same depth.
const EXPLORATION_FILTER_CP: i32 = 50;

/// One completed game, played out to a real result -- decisive-or-draw covers every possible ending
/// (checkmate, stalemate, insufficient material, fifty-move, or twofold repetition -- no adjudication
/// of any other kind, see `play_one_game`'s own doc comment); self-play has no fault or time-forfeit
/// concept at all (no external process, no clock).
struct SelfPlayGame {
    result: f64,
    termination: Termination,
    moves: Vec<MoveAnnotation>,
}

/// One self-play game from `starting_fen` at `depth`, as `play_one_game` plays it, formatted as PGN
/// text, both players named `player`, with its trainable positions (`game_positions`). `None` when
/// `stop_requested` tripped mid-game. `client` sends these to its server.
#[allow(clippy::too_many_arguments)]
pub(crate) fn play_game_pgn(
    starting_fen: &str, depth: u32, tt: &TranspositionTable, tablebase: &SyzygyTablebase, network: Option<&Network>,
    parameters: Option<SearchParameters>, player: &str, rng: &mut impl RngExt, stop_requested: &AtomicBool,
) -> Option<(String, u64)> {
    let game = play_one_game(starting_fen, depth, tt, tablebase, network, parameters, rng, stop_requested)?;
    let pgn_result = match game.result {
        1.0 => "1-0",
        0.0 => "0-1",
        _ => "1/2-1/2",
    };
    let pgn_text = pgn::format_game(
        &GameHeaders {
            event: "fattorini self-play".to_string(),
            round: 0,
            white: player.to_string(),
            black: player.to_string(),
            starting_fen: starting_fen.to_string(),
        },
        &game.moves,
        pgn_result,
        game.termination,
        false,
    );
    Some((pgn_text, game_positions(&game)))
}

/// Plays one game start to finish from `starting_fen`, strictly by the rules of chess -- no
/// adjudication of any kind (developer decision, 2026-09-22). A dead position
/// (`is_insufficient_material`) is a draw by the rules, not an adjudication. The one deliberate
/// deviation from the real rules: twofold repetition ends the game a draw, not threefold -- checked
/// below via `ancestor_hashes`. `None` means `stop_requested` tripped before the game finished -- the
/// caller drops it entirely. No hard ply cap: the fifty-move rule (checked unconditionally below)
/// already guarantees this loop terminates on its own.
///
/// `tt` is the game's one table, both sides' searches alike. `parameters`, when given, replace the
/// network's own tuning in every search.
#[allow(clippy::too_many_arguments)]
fn play_one_game(
    starting_fen: &str, depth: u32, tt: &TranspositionTable, tablebase: &SyzygyTablebase, network: Option<&Network>,
    parameters: Option<SearchParameters>, rng: &mut impl RngExt, stop_requested: &AtomicBool,
) -> Option<SelfPlayGame> {
    let (mut board, _fullmove_number) =
        Board::from_fen(starting_fen).unwrap_or_else(|error| panic!("opening book gave an invalid FEN {starting_fen:?}: {error:?}"));
    let mut ancestor_hashes = vec![board.hash()];
    let mut moves: Vec<MoveAnnotation> = Vec::new();
    let mut ply: u32 = 0;

    loop {
        let side_to_move = board.side_to_move();
        let legal = legal_moves(&board, side_to_move);

        if legal.is_empty() {
            let checkmate = is_in_check(&board, side_to_move);
            let result = if !checkmate {
                0.5
            } else if side_to_move == Color::White {
                0.0
            } else {
                1.0
            };
            let termination = if checkmate { Termination::Checkmate } else { Termination::Stalemate };
            return Some(SelfPlayGame { result, termination, moves });
        }
        // A dead position, neither side able to mate: drawn at once, so no row is written for the
        // moves a game would otherwise shuffle through until a repetition or the fifty-move rule.
        if is_insufficient_material(&board) {
            return Some(SelfPlayGame { result: 0.5, termination: Termination::InsufficientMaterial, moves });
        }
        if board.halfmove_clock() >= 100 {
            return Some(SelfPlayGame { result: 0.5, termination: Termination::FiftyMoveRule, moves });
        }
        if ancestor_hashes.iter().filter(|&&hash| hash == board.hash()).count() >= 2 {
            return Some(SelfPlayGame { result: 0.5, termination: Termination::Repetition, moves });
        }
        if stop_requested.load(Ordering::SeqCst) {
            return None;
        }

        let started = Instant::now();
        // The positions before `board`, as the search and its tablebase root ranking take them:
        // `ancestor_hashes` ends with `board`, kept for the repetition check above.
        let ancestors = &ancestor_hashes[..ancestor_hashes.len() - 1];
        // `best` is the position's best move, its label; `mv` the move played, the same past the
        // exploration plies.
        let (mv, best, score, reached) = if ply < EXPLORATION_PLIES {
            choose_exploration_move(&board, depth, ancestors, tt, tablebase, network, parameters, stop_requested, rng)?
        } else if let Some((result, best)) = tablebase.root_result_and_best_moves(&board, ancestors) {
            // A root the tables rank: labelled and played from them, with no search (depth 0). The
            // search would return the same proven result at its cost.
            let mv = self_play_tablebase_move(&best, result, rng);
            (mv, mv, tablebase_score(result), 0)
        } else {
            // `network` is `None` for the PeSTO evaluation's games, which is what makes their
            // labels a distillation of that evaluation's own search.
            let (reports, best_move) = choose_best_move_with_progress(
                &board, depth, None, ancestors, tt, tablebase, network, parameters, 0, stop_requested, None, |_| {},
            );
            let report = reports.last()?;
            let best_move = best_move?;
            (best_move, best_move, report.score, report.depth)
        };
        let elapsed = started.elapsed();

        // A forced mate is an exact, trustworthy distance -- the search found actual checkmate at
        // a definite ply within its own horizon -- so it gets a real `mate_in` count, computed the
        // same way `uci.rs`'s own `format_score` does for a UCI `score mate N` reply. Everything
        // else, including a tablebase-scale score (a WDL result in the search, or a ranked root's
        // label: a result, not a distance to mate) gets its raw score reported as `score_cp` as-is,
        // even when that's an enormous, self-evidently non-material number -- exactly what real
        // Stockfish's own `UCI::value` does too, and what `format_score` already does here for a
        // live UCI reply.
        let (score_cp, mate_in) = if score.abs() > MATE_SCORE - MAX_PLY as i32 {
            let ply_at_mate = MATE_SCORE - score.abs();
            let moves_to_mate = (ply_at_mate + 1) / 2;
            (None, Some(if score > 0 { moves_to_mate } else { -moves_to_mate }))
        } else {
            (Some(score), None)
        };
        moves.push(MoveAnnotation {
            uci: format_move(mv),
            score_cp,
            mate_in,
            depth: Some(reached),
            time: elapsed,
            best: Some(format_move(best)),
        });
        board.make_move(mv);
        ancestor_hashes.push(board.hash());
        ply += 1;
    }
}

/// The exploration move for one of the game's own first `EXPLORATION_PLIES` plies: draw `M`
/// uniformly from `{1, 2, 3}`, search with `MultiPV = M`, then play the *lowest-ranked* of those
/// `M` candidates that's still within `EXPLORATION_FILTER_CP` of the best move's score (the best
/// move is always in that same pool, and always survives its own filter, so this never falls
/// through with nothing eligible).
///
/// The score returned is the *best* move's, not the played move's, even though a deliberately
/// suboptimal move is what gets played: a position's label is its own depth-9 minimax value, a
/// property of the position rather than of whichever move exploration happened to pick next. Past
/// `EXPLORATION_PLIES` the played move *is* the best move and the two coincide, so returning the best
/// move's score here is what makes `score_cp` mean the same thing at every ply of every game.
/// Returning the played move's score instead would bias the first `EXPLORATION_PLIES` labels of
/// every game downward by up to `EXPLORATION_FILTER_CP`, always in that one direction.
///
/// The best move is returned beside the played one for the same reason: it is the position's move
/// label, written after its score at every ply.
///
/// `None` propagates a `stop_requested` interruption exactly like the non-exploration branch.
#[allow(clippy::too_many_arguments)]
fn choose_exploration_move(
    board: &Board, depth: u32, ancestors: &[u64], tt: &TranspositionTable, tablebase: &SyzygyTablebase, network: Option<&Network>,
    parameters: Option<SearchParameters>, stop_requested: &AtomicBool, rng: &mut impl RngExt,
) -> Option<(Move, Move, i32, u32)> {
    let m: u32 = rng.random_range(1..=3);
    let mut last_extra_lines: Vec<MultiPvLine> = Vec::new();
    let (reports, best_move) = choose_best_move_multipv_with_progress(
        board, depth, None, ancestors, tt, tablebase, network, parameters, stop_requested, m, None,
        |_report, extra_lines| {
            last_extra_lines = extra_lines.to_vec();
        },
    );
    let report = reports.last()?;
    let best_move = best_move?;

    let mut candidates: Vec<(i32, Move)> = vec![(report.score, best_move)];
    for line in &last_extra_lines {
        candidates.push((line.score, line.pv[0]));
    }
    let best_score = candidates[0].0;
    let eligible: Vec<(i32, Move)> =
        candidates.iter().filter(|(score, _)| *score >= best_score - EXPLORATION_FILTER_CP).copied().collect();
    let &(_, mv) = eligible.last().expect("the best move always survives its own filter");
    Some((mv, best_move, best_score, report.depth))
}

/// The trainable positions of a self-play game: every searched position, one per move, and the
/// final one, which no move leaves and whose label is the game's result. Self-play writes no book
/// plies (its opening is in the starting FEN), so a game of p plies has p + 1, as `smithy`'s
/// stream counts them.
fn game_positions(game: &SelfPlayGame) -> u64 {
    game.moves.len() as u64 + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use fattorini::tablebase::TbResult;

    use super::super::data_root;

    #[test]
    fn a_dead_position_ends_the_game_a_draw_before_any_move() {
        let tt = TranspositionTable::new(1);
        let tablebase = SyzygyTablebase::new();
        let stop_requested = AtomicBool::new(false);
        let mut rng = rand::rng();
        // King against king, and one bishop each on light squares (c8 and f1).
        for fen in ["8/8/4k3/8/8/4K3/8/8 w - - 0 1", "2b1k3/8/8/8/8/8/8/4KB2 b - - 0 1"] {
            let game = play_one_game(fen, DEFAULT_SEARCH_DEPTH, &tt, &tablebase, None, None, &mut rng, &stop_requested).expect("not stopped");
            assert_eq!(game.result, 0.5, "{fen}");
            assert_eq!(game.termination, Termination::InsufficientMaterial, "{fen}");
            assert!(game.moves.is_empty(), "{fen}: no move played");
        }
    }

    /// KQ vs K from a far start: past the exploration plies every position is played from the tables
    /// without a search (depth 0), labelled with the tablebase band, and the win is converted, since
    /// the winner takes the smallest DTZ and the loser the largest, so the DTZ falls every ply and no
    /// position repeats. Skipped where `bin/syzygy.sh` has not run.
    #[test]
    fn a_ranked_root_is_played_from_the_tables_without_a_search_and_converted() {
        let Ok(tablebase) = SyzygyTablebase::from_directories(&data_root::syzygy_directories()) else {
            println!("skipped: no Syzygy WDL and DTZ tables");
            return;
        };
        let tt = TranspositionTable::new(16);
        let stop_requested = AtomicBool::new(false);
        let mut rng = rand::rng();
        let game = play_one_game("8/8/8/4k3/8/8/8/1Q2K3 w - - 0 1", DEFAULT_SEARCH_DEPTH, &tt, &tablebase, None, None, &mut rng, &stop_requested)
            .expect("not stopped");

        assert_eq!(game.termination, Termination::Checkmate);
        assert_eq!(game.result, 1.0);
        let past_exploration = &game.moves[EXPLORATION_PLIES as usize..];
        assert!(!past_exploration.is_empty());
        for (index, annotation) in past_exploration.iter().enumerate() {
            let white = index % 2 == 0;
            let expected = if white { tablebase_score(TbResult::Win) } else { tablebase_score(TbResult::Loss) };
            assert_eq!(annotation.depth, Some(0), "ply {}: no search", EXPLORATION_PLIES as usize + index);
            assert_eq!(annotation.score_cp, Some(expected), "ply {}", EXPLORATION_PLIES as usize + index);
            assert_eq!(annotation.best, Some(annotation.uci.clone()), "the move played is the label's");
        }
    }

    /// Self-play's speed on this machine without a smithy: `FATTORINI_SELF_PLAYER` (a network's name, or
    /// `pesto`) playing itself as a chore's worker plays it (`play_game_pgn`, a 16 MB table each,
    /// cleared between games, the data root's book and tables), a game per logical core, for
    /// `FATTORINI_SELF_SECONDS` (default 600). Prints every minute the positions a second since the start,
    /// each game's final one included, and the games. Run with the machine otherwise idle.
    #[test]
    #[ignore = "a timing on this machine: FATTORINI_SELF_PLAYER names the player"]
    fn self_play_speed() {
        use std::sync::atomic::AtomicU64;
        use std::sync::Arc;

        let player = std::env::var("FATTORINI_SELF_PLAYER").expect("FATTORINI_SELF_PLAYER: a network's name, or pesto");
        let seconds: u64 = std::env::var("FATTORINI_SELF_SECONDS").map_or(600, |text| text.parse().expect("FATTORINI_SELF_SECONDS: seconds"));
        std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).expect("the repository root");
        let network = (player != "pesto").then(|| {
            let path = fattorini::pipeline::players::resolve_network_file(&player).unwrap_or_else(|problem| panic!("{problem}"));
            Arc::new(Network::load(&path).unwrap_or_else(|problem| panic!("{problem}")))
        });
        let tablebase = Arc::new(data_root::load_tablebase());
        let book = Arc::new(super::super::random_book::RandomBook::open(&data_root::book_path()));
        let workers = std::thread::available_parallelism().map_or(1, |cores| cores.get());
        let (positions, games, stop) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)));
        println!("\n{player}: {workers} workers at depth {DEFAULT_SEARCH_DEPTH}, for {seconds} s");
        let started = Instant::now();
        std::thread::scope(|scope| {
            for _ in 0..workers {
                let (network, tablebase, book) = (network.clone(), Arc::clone(&tablebase), Arc::clone(&book));
                let (positions, games, stop, player) = (Arc::clone(&positions), Arc::clone(&games), Arc::clone(&stop), player.clone());
                scope.spawn(move || {
                    let mut tt = TranspositionTable::new(TT_SIZE_MB);
                    let mut rng = rand::rng();
                    let mut reader = book.reader();
                    while !stop.load(Ordering::SeqCst) {
                        let fen = book.random_fen(&mut reader, &mut rng);
                        let Some((_, played)) =
                            play_game_pgn(&fen, DEFAULT_SEARCH_DEPTH, &tt, &tablebase, network.as_deref(), Some(SearchParameters::DEFAULT), &player, &mut rng, &stop)
                        else {
                            break;
                        };
                        tt.clear();
                        positions.fetch_add(played, Ordering::SeqCst);
                        games.fetch_add(1, Ordering::SeqCst);
                    }
                });
            }
            for minute in 1..=seconds.div_ceil(60) {
                std::thread::sleep(Duration::from_secs(60.min(seconds - (minute - 1) * 60)));
                let elapsed = started.elapsed().as_secs_f64();
                println!(
                    "{:>4.0} s: {:>10} positions/s, {} games",
                    elapsed,
                    progress::grouped((positions.load(Ordering::SeqCst) as f64 / elapsed).round() as u64),
                    progress::grouped(games.load(Ordering::SeqCst))
                );
            }
            stop.store(true, Ordering::SeqCst);
        });
    }

    #[test]
    fn game_positions_counts_every_searched_position_and_the_final_one() {
        let annotation = |uci: &str, score_cp: Option<i32>, mate_in: Option<i32>| MoveAnnotation {
            uci: uci.to_string(),
            score_cp,
            mate_in,
            depth: Some(9),
            time: Duration::from_millis(10),
            best: Some(uci.to_string()),
        };
        let game = SelfPlayGame {
            result: 0.0,
            termination: Termination::Checkmate,
            moves: vec![
                annotation("f2f3", Some(-40), None),
                annotation("e7e5", Some(35), None),
                annotation("g2g4", Some(-900), None),
                annotation("d8h4", None, Some(1)),
            ],
        };
        assert_eq!(game_positions(&game), 5, "four searched positions, and the final mated one");
    }
}
