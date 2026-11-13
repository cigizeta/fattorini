//! A `score` chore's one task: an opening book's move judged by the search, which plays no game.
//! The position after the move is searched to the chore's depth on one thread, with a fresh table
//! and no tablebases, so the same move gets the same score on every arena whose build and network
//! are the same; whether they are is the bench's node count (`bench_nodes`), which the smithy
//! compares with its own. The score is turned to the side that played the move: the search's is the
//! side to move's, its opponent.

use std::sync::atomic::AtomicBool;

use fattorini::bench::{bench_with, BENCH_DEPTH};
use fattorini::board::Board;
use fattorini::movegen::attacks::is_in_check;
use fattorini::movegen::legal::legal_moves;
use fattorini::nnue::Network;
use fattorini::pipeline::protocol::{BookScore, SCORE_HASH_MB};
use fattorini::search::{choose_best_move_with_parameters, default_parameters_for, SearchParameters};
use fattorini::tablebase::SyzygyTablebase;
use fattorini::tt::TranspositionTable;
use fattorini::uci::split_score;

/// What one score search found: the score from the side that played the book move, the depth
/// completed, and the nodes searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Scored {
    pub score: BookScore,
    pub depth: u32,
    pub nodes: u64,
}

/// The parameters a score search plays with: the player's, or the network's own tune.
pub(crate) fn parameters_of(network: Option<&Network>, given: Option<SearchParameters>) -> SearchParameters {
    given.unwrap_or_else(|| default_parameters_for(network))
}

/// Searches `fen`, the position after a book move, to `depth`, as the module says; `Ok(None)` when
/// stopped first. A position with no legal move (the book move mated or stalemated) is scored
/// without a search: a mate in one, or a draw.
pub(crate) fn score_position(
    fen: &str, depth: u32, network: Option<&Network>, parameters: SearchParameters, stop: &AtomicBool,
) -> Result<Option<Scored>, String> {
    let (board, _) = Board::from_fen(fen).map_err(|error| format!("score: {fen:?} is not a FEN: {error:?}"))?;
    if legal_moves(&board, board.side_to_move()).is_empty() {
        let score = if is_in_check(&board, board.side_to_move()) { BookScore::Mate(1) } else { BookScore::Cp(0) };
        return Ok(Some(Scored { score, depth, nodes: 0 }));
    }
    let tt = TranspositionTable::new(SCORE_HASH_MB);
    // Empty: no tables are read, so a machine holding Syzygy files scores as one without.
    let tablebase = SyzygyTablebase::new();
    let (reports, _) = choose_best_move_with_parameters(&board, depth, None, &[], &tt, &tablebase, network, parameters, 0, stop);
    if stop.load(std::sync::atomic::Ordering::SeqCst) {
        return Ok(None);
    }
    let Some(last) = reports.last() else {
        return Err(format!("score: the search of {fen:?} reported no depth"));
    };
    if last.depth < depth {
        return Err(format!("score: the search of {fen:?} reached depth {} of {depth}", last.depth));
    }
    let score = match split_score(-last.score) {
        (_, Some(moves)) => BookScore::Mate(moves),
        (Some(cp), _) => BookScore::Cp(cp),
        (None, None) => unreachable!("split_score always reports one of the two"),
    };
    Ok(Some(Scored { score, depth: last.depth, nodes: last.nodes }))
}

/// This machine's `bench` node count with `network` and `parameters`: one thread, a fresh table each,
/// the same on every run of one build and network, so a different count means a different search.
pub(crate) fn bench_nodes(network: Option<&Network>, parameters: SearchParameters) -> u64 {
    bench_with(network, parameters, BENCH_DEPTH).0
}

#[cfg(test)]
mod tests {
    use super::*;

    const AFTER_E4: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";

    #[test]
    fn a_score_search_is_the_same_twice_and_from_the_mover_s_side() {
        let stop = AtomicBool::new(false);
        let parameters = parameters_of(None, None);
        let first = score_position(AFTER_E4, 6, None, parameters, &stop).unwrap().unwrap();
        let second = score_position(AFTER_E4, 6, None, parameters, &stop).unwrap().unwrap();
        assert_eq!(first, second, "one thread and a fresh table: the same search");
        assert_eq!(first.depth, 6);
        assert!(first.nodes > 0);
        // White has just dropped its queen on d5 for nothing: from White's side, far below even.
        let blunder = "rnbqkbnr/ppp1pppp/8/3Q4/8/8/PPPP1PPP/RNB1KBNR b KQkq - 0 3";
        let Scored { score, .. } = score_position(blunder, 4, None, parameters, &stop).unwrap().unwrap();
        assert!(matches!(score, BookScore::Cp(cp) if cp < -500), "the queen hangs to Qxd5: {score:?}");
    }

    #[test]
    fn a_mating_book_move_is_a_mate_in_one_without_a_search() {
        let stop = AtomicBool::new(false);
        // Fool's mate: Black has just played Qh4#.
        let mated = "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3";
        let scored = score_position(mated, 20, None, parameters_of(None, None), &stop).unwrap().unwrap();
        assert_eq!((scored.score, scored.nodes), (BookScore::Mate(1), 0));
    }

    #[test]
    fn a_stopped_search_scores_nothing() {
        let stop = AtomicBool::new(true);
        assert_eq!(score_position(AFTER_E4, 6, None, parameters_of(None, None), &stop), Ok(None));
    }
}
