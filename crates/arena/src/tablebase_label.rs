//! Labels and moves of positions the tablebases rank, without a search: self-play labels them and
//! chooses its move here.

use rand::RngExt;

use fattorini::board::chess_move::Move;
use fattorini::search::{tb_loss_at, tb_win_at};
use fattorini::tablebase::TbResult;

/// The label of a position the tables know: its result under the fifty-move rule, in the band a
/// search writes for a tablebase win one ply away, `tb_win_at(1)`, a draw 0, a loss the negative.
/// The band, not the label curve's 32-pawn ceiling: far beyond any ceiling training might choose, it
/// stays a full win if the curve changes, and it is what the search already writes for the same
/// proven fact.
pub fn tablebase_score(result: TbResult) -> i32 {
    match result {
        TbResult::Win => tb_win_at(1),
        TbResult::Draw => 0,
        TbResult::Loss => tb_loss_at(1),
    }
}

/// Self-play's move at a ranked root, among `best`, the moves of the best rank with their DTZ
/// (`SyzygyTablebase::root_result_and_best_moves`). Every win within the clock shares that rank, and
/// with no search to choose among them the first in generation order need not make progress, so the
/// tie is broken by DTZ: when winning the smallest (the fastest route to a mate or the next zeroing
/// move), when losing the largest in absolute value (the longest resistance), ties to the first in
/// generation order; when drawn, any of them at random, for variety. Never a move outside `best`, so
/// never one that spoils a win or loses a draw.
pub fn self_play_tablebase_move(best: &[(Move, i32)], result: TbResult, rng: &mut impl RngExt) -> Move {
    assert!(!best.is_empty(), "a ranked root has at least one best move");
    match result {
        TbResult::Win => best.iter().min_by_key(|&&(_, dtz)| dtz).expect("not empty").0,
        TbResult::Loss => best.iter().min_by_key(|&&(_, dtz)| -dtz.abs()).expect("not empty").0,
        TbResult::Draw => best[rng.random_range(0..best.len())].0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fattorini::board::Board;
    use fattorini::movegen::legal::legal_moves;

    /// The first `count` legal moves of the start position, distinct moves to stand for any.
    fn moves(count: usize) -> Vec<Move> {
        let (board, _) = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        legal_moves(&board, board.side_to_move()).into_iter().take(count).collect()
    }

    #[test]
    fn a_win_takes_the_smallest_dtz_and_a_loss_the_largest_ties_to_the_first() {
        let m = moves(4);
        let mut rng = rand::rng();
        let winning = [(m[0], 7), (m[1], 3), (m[2], 3), (m[3], 11)];
        assert_eq!(self_play_tablebase_move(&winning, TbResult::Win, &mut rng), m[1]);
        let losing = [(m[0], -4), (m[1], -9), (m[2], -9), (m[3], -2)];
        assert_eq!(self_play_tablebase_move(&losing, TbResult::Loss, &mut rng), m[1]);
    }

    #[test]
    fn a_draw_takes_one_of_the_given_moves() {
        let m = moves(3);
        let drawing = [(m[0], 0), (m[1], 0), (m[2], 0)];
        let mut rng = rand::rng();
        for _ in 0..50 {
            let chosen = self_play_tablebase_move(&drawing, TbResult::Draw, &mut rng);
            assert!(m.contains(&chosen));
        }
    }

    #[test]
    fn the_score_is_the_tablebase_band_or_zero() {
        assert_eq!(tablebase_score(TbResult::Win), tb_win_at(1));
        assert_eq!(tablebase_score(TbResult::Draw), 0);
        assert_eq!(tablebase_score(TbResult::Loss), tb_loss_at(1));
    }
}
