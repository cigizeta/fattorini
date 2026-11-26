//! Expanding one position: its evaluation and its outgoing edges, from searches of that position
//! alone. A plain search from a fresh table gives the best move and the best score; then the same
//! search restricted by `searchmoves` to the legal moves not yet found gives the best of the rest,
//! the table kept, again and again until a move loses the threshold or more (it is not admitted)
//! or no legal move is left. Unlike MultiPV, which fixes the number of lines beforehand and
//! searches the earlier lines again at every depth, this stops at the first bad move and pays for
//! one new line per search.

use fattorini::board::chess_move::Move;
use fattorini::board::Board;
use fattorini::movegen::attacks::is_in_check;
use fattorini::movegen::legal::legal_moves;
use fattorini::uci::{format_move, parse_move};

use super::engine::{Judge, Search};
use super::unit::{loss, Score};

/// A position's expansion. Scores are from the side to move, who plays every move here.
#[derive(Debug, Clone, PartialEq)]
pub struct Expansion {
    /// The position's evaluation: the first search's score, or the rules' when no move is legal.
    pub eval: Score,
    /// The admitted moves, best first, each with its score.
    pub moves: Vec<(Move, Score)>,
    /// Every search made, admitted or not, with the move's loss in points: for `book-probe`.
    pub trace: Vec<(Search, f64)>,
}

/// `board` expanded by `judge` at `depth`: a move is admitted while its loss stays below
/// `threshold` points.
pub fn expand(board: &Board, judge: &mut impl Judge, depth: u8, threshold: f64) -> Result<Expansion, String> {
    let mover = board.side_to_move();
    let mut remaining = legal_moves(board, mover);
    if remaining.is_empty() {
        // Checkmate: the side to move is mated, a mate for the side that gave it; stalemate: a draw.
        let eval = if is_in_check(board, mover) { Score::Mate(-1) } else { Score::Cp(0) };
        return Ok(Expansion { eval, moves: Vec::new(), trace: Vec::new() });
    }
    let fen = board.to_fen(1);
    judge.new_position()?;
    let first = judge.search(&fen, depth, None)?;
    let best = first.score;
    let mut moves = Vec::new();
    let mut trace = Vec::new();
    let mut search = first;
    loop {
        let mv = parse_move(&search.best_move, &remaining)
            .ok_or_else(|| format!("the engine played {} in {fen}, not among the moves searched", search.best_move))?;
        let lost = loss(best, search.score);
        trace.push((search.clone(), lost));
        if lost >= threshold {
            break;
        }
        moves.push((mv, search.score));
        remaining.retain(|&other| other != mv);
        if remaining.is_empty() {
            break;
        }
        let searchmoves: Vec<String> = remaining.iter().map(|&other| format_move(other)).collect();
        search = judge.search(&fen, depth, Some(&searchmoves))?;
    }
    Ok(Expansion { eval: best, moves, trace })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// An engine that answers from a script: the moves best first, each with its score. A search
    /// answers the first scripted move among those it may play.
    struct Scripted {
        script: Vec<(&'static str, Score)>,
        new_positions: usize,
        asked: Vec<Option<Vec<String>>>,
    }

    impl Scripted {
        fn new(script: Vec<(&'static str, Score)>) -> Scripted {
            Scripted { script, new_positions: 0, asked: Vec::new() }
        }
    }

    impl Judge for Scripted {
        fn new_position(&mut self) -> Result<(), String> {
            self.new_positions += 1;
            Ok(())
        }

        fn search(&mut self, _fen: &str, depth: u8, searchmoves: Option<&[String]>) -> Result<Search, String> {
            self.asked.push(searchmoves.map(<[String]>::to_vec));
            let (mv, score) = self
                .script
                .iter()
                .find(|(mv, _)| searchmoves.is_none_or(|allowed| allowed.iter().any(|a| a == mv)))
                .copied()
                .ok_or("nothing scripted among the moves")?;
            Ok(Search { best_move: mv.to_string(), score, depth, nodes: 1, time: Duration::ZERO })
        }
    }

    fn start() -> Board {
        Board::from_fen(pgn_replay::STARTING_POSITION_FEN).unwrap().0
    }

    #[test]
    fn the_trimming_stops_at_the_first_move_losing_the_threshold() {
        let mut judge = Scripted::new(vec![
            ("e2e4", Score::Cp(30)),
            ("d2d4", Score::Cp(25)),
            ("g1f3", Score::Cp(10)),
            ("b1a3", Score::Cp(-60)),
            ("c2c4", Score::Cp(20)),
        ]);
        let expansion = expand(&start(), &mut judge, 20, 10.0).unwrap();
        let admitted: Vec<String> = expansion.moves.iter().map(|&(mv, _)| format_move(mv)).collect();
        assert_eq!(admitted, ["e2e4", "d2d4", "g1f3"], "Na3 loses about 13 points and ends it");
        assert_eq!(expansion.eval, Score::Cp(30));
        assert_eq!(expansion.trace.len(), 4);
        assert_eq!(judge.new_positions, 1, "one fresh table per position");
    }

    #[test]
    fn each_search_lists_exactly_the_moves_not_yet_found() {
        let mut judge = Scripted::new(vec![("e2e4", Score::Cp(30)), ("d2d4", Score::Cp(-200))]);
        expand(&start(), &mut judge, 20, 10.0).unwrap();
        assert_eq!(judge.asked[0], None);
        let second = judge.asked[1].as_ref().unwrap();
        assert_eq!(second.len(), 19);
        assert!(!second.contains(&"e2e4".to_string()));
    }

    #[test]
    fn the_trimming_stops_when_the_legal_moves_run_out() {
        // White's king on a1 has two moves; both are as good.
        let (board, _) = Board::from_fen("7k/8/8/8/8/8/8/K7 w - - 0 1").unwrap();
        let mut judge = Scripted::new(vec![("a1a2", Score::Cp(0)), ("a1b1", Score::Cp(0)), ("a1b2", Score::Cp(0))]);
        let expansion = expand(&board, &mut judge, 20, 10.0).unwrap();
        assert_eq!(expansion.moves.len(), 3);
        assert_eq!(judge.asked.len(), 3);
    }

    #[test]
    fn mate_and_stalemate_need_no_search() {
        let (mated, _) = Board::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3").unwrap();
        let mut judge = Scripted::new(Vec::new());
        let expansion = expand(&mated, &mut judge, 20, 10.0).unwrap();
        assert_eq!((expansion.eval, expansion.moves.len()), (Score::Mate(-1), 0));
        let (stalemate, _) = Board::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert_eq!(expand(&stalemate, &mut judge, 20, 10.0).unwrap().eval, Score::Cp(0));
        assert!(judge.asked.is_empty());
    }
}
