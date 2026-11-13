//! A `Player` that searches in this process, with no subprocess and no protocol -- the side of a
//! match that is Fattorini itself, evaluating either with the PeSTO evaluation or with a trained
//! network. This is what makes an evaluation A/B possible at all: the gate needs both sides to run
//! the identical search so that the evaluation function is the only thing that differs, and driving
//! Fattorini over UCI would instead compare two whole engine builds.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use fattorini::nnue::Network;
use fattorini::search::{
    choose_best_move_with_progress, single_move_limit, time_limit, SearchParameters, TimeManagement,
    DEFAULT_MOVE_OVERHEAD_MS, MAX_PLY,
};
use fattorini::tablebase::SyzygyTablebase;
use fattorini::tt::TranspositionTable;
use fattorini::uci::{format_move, split_score};

use super::player::{Fault, MoveLimit, MoveOutcome, MoveRequest, Player, SearchReport};


pub struct InProcessPlayer {
    /// `None` is the PeSTO evaluation. The champion of generation 0.
    evaluator: Option<Arc<Network>>,
    /// **Its own table, not a shared one, and this is a correctness requirement rather than a
    /// fairness nicety.** A transposition entry stores a score produced by one evaluation function.
    /// Two players sharing a table would read each other's scores, and the engine that resulted
    /// would be neither of the two things being compared.
    tt: TranspositionTable,
    /// Shared, unlike the table: probing is stateless and the loaded tables are never mutated, so
    /// there is no reason for each of 32 players to hold its own copy of the six-piece set.
    tablebase: Arc<SyzygyTablebase>,
    /// The search parameters it plays with: a suffix's (`@tuned`, `@untuned`), or pesto's tuning;
    /// `None` searches with the network's own tuning, or
    /// the defaults (`search::choose_best_move_with_progress`).
    parameters: Option<SearchParameters>,
    /// The final score of its previous move in this game, for the clock target's falling-score
    /// factor (`TimeLimit::with_previous_score`), as the engine keeps between `go`s. Cleared by
    /// `new_game`.
    last_score: Option<i32>,
}

impl InProcessPlayer {
    /// `hash_mb` is the table's size, the same number a UCI player is given as `Hash`.
    pub fn new(
        evaluator: Option<Arc<Network>>, tablebase: Arc<SyzygyTablebase>, hash_mb: usize,
        parameters: Option<SearchParameters>,
    ) -> InProcessPlayer {
        InProcessPlayer {
            evaluator,
            tt: TranspositionTable::new(hash_mb),
            tablebase,
            parameters,
            last_score: None,
        }
    }
}

impl Player for InProcessPlayer {
    /// Clears the hash, the same thing a real engine does on `ucinewgame` -- without it, the second
    /// game of a pair would start out reading the first game's scores, and the two games of a pair
    /// would no longer be the independent observations the test treats them as.
    fn new_game(&mut self) -> Result<(), Fault> {
        self.tt.clear();
        self.last_score = None;
        Ok(())
    }

    /// Never returns `Err`, and that is a property of this player rather than an oversight worth
    /// leaving as a puzzle: it cannot crash the way a subprocess can, and cannot return an illegal
    /// move because it only ever generates legal ones. Under a clock, overstepping is the game
    /// loop's to judge, from the time the move took, as for a UCI player. The zero-tolerance fault
    /// machinery stays for UCI players, where it earns its keep.
    ///
    /// `Interrupted` is the one non-move outcome, and it is nearly unreachable. Iterative deepening's
    /// own loop never consults the stop signal on thread `0` at all -- it breaks only when
    /// `search_root` reports back that it stopped, and `search_root` notices the signal on a node
    /// counter, which shallow depths do not reach. So a search told to stop still runs several
    /// depths and returns the deepest move it completed; only a signal that somehow lands inside
    /// depth 1 produces no move.
    ///
    /// That is not a gap, because stopping a *game* is the game loop's job and it checks before every
    /// ply. This player's only obligation is to come back well before the ninth depth would have
    /// finished, which it does.
    ///
    /// Under a clock it plans its time exactly as `go wtime btime winc binc` does (`time_limit`), the
    /// default `Move Overhead` kept back as a release keeps it, so a timed match plays as one does, and
    /// searches against its own stop flag, fresh each move: the search raises the flag it is given
    /// when its time manager declines the next depth, and raising the match's Ctrl+C flag would end
    /// the whole run after the first timed move. Ctrl+C is still seen by the game loop before every
    /// ply, within one move's thinking time.
    fn choose_move(&mut self, request: &MoveRequest, stop_requested: &Arc<AtomicBool>) -> Result<MoveOutcome, Fault> {
        let own_stop = AtomicBool::new(false);
        let (depth, limit, stop) = match request.limit {
            MoveLimit::FixedDepth(depth) => (depth, None, stop_requested.as_ref()),
            MoveLimit::Fischer { white_remaining, black_remaining, increment, moves_to_go } => {
                let milliseconds = |duration: std::time::Duration| Some(duration.as_millis() as u64);
                let limit = time_limit(
                    request.board,
                    milliseconds(white_remaining),
                    milliseconds(black_remaining),
                    milliseconds(increment),
                    milliseconds(increment),
                    moves_to_go,
                    None,
                    false,
                    DEFAULT_MOVE_OVERHEAD_MS,
                    TimeManagement::Split,
                )
                .map(|limit| limit.with_previous_score(self.last_score));
                (MAX_PLY, limit, &own_stop)
            }
        };

        // A game move: one legal move gets the single-move cap (`single_move_limit`), alongside the
        // depth or the clock, whichever ends first.
        let (reports, best_move) = choose_best_move_with_progress(
            request.board,
            depth,
            single_move_limit(request.board, limit),
            request.ancestors,
            &self.tt,
            &self.tablebase,
            self.evaluator.as_deref(),
            self.parameters,
            0,
            stop,
            None,
            |_| {},
        );

        let Some(best_move) = best_move else {
            return Ok(MoveOutcome::Interrupted);
        };
        self.last_score = reports.last().map(|report| report.score);
        let (score_cp, mate_in) = match reports.last() {
            Some(report) => split_score(report.score),
            None => (None, None),
        };
        Ok(MoveOutcome::Move(SearchReport {
            best_move_uci: format_move(best_move),
            score_cp,
            mate_in,
            depth: reports.last().map(|report| report.depth),
        }))
    }

    /// Nothing to tear down: no subprocess, and the table goes with the player.
    fn shutdown(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use fattorini::board::Board;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    /// Under a clock it answers with a legal move well within its own time, and leaves the match's
    /// stop flag alone: the search raises the flag it searches against when it declines the next
    /// depth, which on the match's flag would stop the whole run.
    #[test]
    fn under_a_clock_it_answers_in_time_and_leaves_the_matchs_stop_flag_alone() {
        let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        let (board, _) = Board::from_fen(fen).unwrap();
        let mut player =
            InProcessPlayer::new(None, Arc::new(SyzygyTablebase::new()), 16, Some(SearchParameters { razoring_margin: 150, ..SearchParameters::DEFAULT }));
        player.new_game().expect("an in-process player never faults");
        let ancestors = vec![board.hash()];
        let remaining = Duration::from_secs(2);
        let request = MoveRequest {
            starting_fen: fen,
            moves_uci: &[],
            board: &board,
            ancestors: &ancestors,
            limit: MoveLimit::Fischer {
                white_remaining: remaining,
                black_remaining: remaining,
                increment: Duration::from_millis(100),
                moves_to_go: None,
            },
        };
        let stop = Arc::new(AtomicBool::new(false));

        let started = Instant::now();
        match player.choose_move(&request, &stop).expect("never faults") {
            MoveOutcome::Move(report) => {
                let legal = fattorini::movegen::legal::legal_moves(&board, board.side_to_move());
                assert!(fattorini::uci::parse_move(&report.best_move_uci, &legal).is_some(), "{:?}", report.best_move_uci);
            }
            MoveOutcome::TimedOut => panic!("an in-process player never reports a timeout itself"),
            MoveOutcome::Interrupted => panic!("nothing asked it to stop"),
        }
        assert!(started.elapsed() < remaining, "took {:?} of a {remaining:?} clock", started.elapsed());
        assert!(!stop.load(Ordering::Relaxed), "the match's stop flag was raised");
    }

    /// The whole reason this player exists, checked at its narrowest: asked for a move at a fixed
    /// depth it returns a legal one, with a score and the depth it actually reached. Depth 4, so the
    /// test stays fast -- the property under test is that the wiring holds, not how well it plays.
    #[test]
    fn choosing_a_move_at_a_fixed_depth_reports_a_legal_move_with_its_score() {
        let (board, _) = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let mut player = InProcessPlayer::new(None, Arc::new(SyzygyTablebase::new()), 16, None);
        player.new_game().expect("an in-process player never faults");

        let ancestors = vec![board.hash()];
        let request = MoveRequest {
            starting_fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            moves_uci: &[],
            board: &board,
            ancestors: &ancestors,
            limit: MoveLimit::FixedDepth(4),
        };
        let stop = Arc::new(AtomicBool::new(false));

        match player.choose_move(&request, &stop).expect("never faults") {
            MoveOutcome::Move(report) => {
                let legal = fattorini::movegen::legal::legal_moves(&board, board.side_to_move());
                assert!(
                    fattorini::uci::parse_move(&report.best_move_uci, &legal).is_some(),
                    "{:?} should be legal in the starting position",
                    report.best_move_uci
                );
                assert_eq!(report.depth, Some(4));
                assert!(report.score_cp.is_some(), "the starting position is not a mate score");
            }
            MoveOutcome::TimedOut => panic!("a fixed-depth search has no clock to exceed"),
            MoveOutcome::Interrupted => panic!("nothing asked it to stop"),
        }
    }

    /// A player asked to stop still answers with a move, and that is the search's real behaviour
    /// rather than a shortcoming: depth 1 runs unconditionally, with no deadline and no look at the
    /// stop signal, so there is always a completed depth to report. Pinned by a test because the
    /// obvious guess is the opposite one, and because it is what makes the division of labour
    /// correct -- dropping an interrupted *game* belongs to `play_one_game`, which checks before
    /// every ply, not to the player, whose only obligation is to return promptly.
    #[test]
    fn a_player_whose_stop_signal_is_already_set_still_answers_with_the_move_depth_one_found() {
        let (board, _) = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let mut player = InProcessPlayer::new(None, Arc::new(SyzygyTablebase::new()), 16, None);
        let ancestors = vec![board.hash()];
        let request = MoveRequest {
            starting_fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            moves_uci: &[],
            board: &board,
            ancestors: &ancestors,
            limit: MoveLimit::FixedDepth(9),
        };
        let stop = Arc::new(AtomicBool::new(true));

        match player.choose_move(&request, &stop).expect("never faults") {
            MoveOutcome::Move(report) => {
                let legal = fattorini::movegen::legal::legal_moves(&board, board.side_to_move());
                assert!(
                    fattorini::uci::parse_move(&report.best_move_uci, &legal).is_some(),
                    "{:?} should be legal in the starting position",
                    report.best_move_uci
                );
                // Short of the 9 that was asked for, but deliberately not pinned to an exact depth:
                // where the deepening actually stops depends on when `search_root`'s node-counted
                // check first observes the signal, which is a search-tuning detail and not this
                // player's contract. What matters here is that it stops early at all.
                let depth = report.depth.expect("a completed depth reports its own number");
                assert!(depth < 9, "an already-stopped search should stop short of the requested depth, got {depth}");
            }
            MoveOutcome::TimedOut => panic!("a fixed-depth search has no clock to exceed"),
            MoveOutcome::Interrupted => panic!("depth 1 completes regardless of the stop signal"),
        }
    }
}
