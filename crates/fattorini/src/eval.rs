//! The PeSTO evaluation, the player `pesto`: PeSTO's tapered material and piece-square tables (`pesto.rs`).
//!
//! Every piece adds its material and its square's table entry, once for the middlegame and once
//! for the endgame. The two sums are blended by the game phase, which counts the pieces still on
//! the board: knights and bishops 1, rooks 2, queens 4, so 24 at the start of the game and 0 with
//! kings and pawns only. That blend is PeSTO's own, and its tables were tuned against it.
//!
//! Only material and tables are evaluated: PeSTO's tables were tuned with nothing beside them.

use crate::board::color::Color;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use crate::board::Board;
use crate::pesto;

/// Traditional centipawn piece values. Not part of the evaluation: move ordering (`search.rs`) and
/// SEE (`movegen/see.rs`) score captures with them, as a fixed ruler that does not move when the
/// evaluation does. The king is not valued at all: it can never be captured.
pub(crate) const fn piece_value(piece_type: PieceType) -> i32 {
    match piece_type {
        PieceType::Pawn => 100,
        PieceType::Knight => 320,
        PieceType::Bishop => 330,
        PieceType::Rook => 500,
        PieceType::Queen => 900,
        PieceType::King => 0,
    }
}

/// Where a piece of `color` on `square` reads the PeSTO tables. The tables are printed a8 first
/// and Fattorini's squares are LERF (a1 = 0), so White flips the rank (`^ 56`) and Black, whose tables
/// are White's mirrored, reads the square as it is.
fn table_index(color: Color, square: Square) -> usize {
    match color {
        Color::White => square as usize ^ 56,
        Color::Black => square as usize,
    }
}

/// The middlegame score, the endgame score, both from White's side, and the game phase.
fn tapered_terms(board: &Board) -> (i32, i32, i32) {
    let mut mg = 0;
    let mut eg = 0;
    let mut phase = 0;
    for color in [Color::White, Color::Black] {
        let sign = if color == Color::White { 1 } else { -1 };
        for piece_type in PieceType::ALL {
            let kind = piece_type as usize;
            for square in board.bitboard(Piece::new(color, piece_type)).squares() {
                let index = table_index(color, square);
                mg += sign * (pesto::MG_VALUE[kind] + pesto::MG_TABLE[kind][index]);
                eg += sign * (pesto::EG_VALUE[kind] + pesto::EG_TABLE[kind][index]);
                phase += pesto::PHASE_INC[kind];
            }
        }
    }
    (mg, eg, phase.min(pesto::PHASE_MAX))
}

/// The tapered score, from White's side.
fn unscaled(board: &Board) -> i32 {
    let (mg, eg, phase) = tapered_terms(board);
    (mg * phase + eg * (pesto::PHASE_MAX - phase)) / pesto::PHASE_MAX
}

/// The balance of `board`, from the side to move's perspective: positive means the side to move is
/// ahead. That is negamax's convention, which search relies on at every node.
///
/// Only material and position: checkmate and stalemate need the legal moves, which only search
/// has. Pure opposite-color bishop endings are not halved, as PeSTO does not.
pub fn evaluate(board: &Board) -> i32 {
    let white_relative = unscaled(board);
    match board.side_to_move() {
        Color::White => white_relative,
        Color::Black => -white_relative,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(fen: &str) -> Board {
        Board::from_fen(fen).unwrap().0
    }

    #[test]
    fn the_starting_position_is_balanced() {
        assert_eq!(evaluate(&board("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")), 0);
    }

    #[test]
    fn the_side_to_move_flips_the_sign() {
        let white = board("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNB1KBNR w KQkq - 0 1");
        let black = board("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNB1KBNR b KQkq - 0 1");
        assert_eq!(evaluate(&white), -evaluate(&black));
        assert!(evaluate(&white) < 0, "White is a queen down");
    }

    #[test]
    fn a_position_and_its_color_mirror_evaluate_the_same() {
        // Each pair: the board flipped vertically, colors swapped, and the other side to move.
        let pairs = [
            ("4k3/8/8/8/3N4/8/8/4K3 w - - 0 1", "4k3/8/8/3n4/8/8/8/4K3 b - - 0 1"),
            ("r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4",
             "rnbqk2r/pppp1ppp/5n2/2b1p3/4P3/2N2N2/PPPP1PPP/R1BQKB1R b KQkq - 4 4"),
            ("8/5pk1/6p1/8/2P5/1P6/P4K2/8 b - - 0 40", "8/p4k2/1p6/2p5/8/6P1/5PK1/8 w - - 0 40"),
            ("4k3/pp3p2/8/3P4/2P5/8/P4PPP/4K3 w - - 0 1", "4k3/p4ppp/8/2p5/3p4/8/PP3P2/4K3 b - - 0 1"),
        ];
        for (fen, mirror) in pairs {
            assert_eq!(evaluate(&board(fen)), evaluate(&board(mirror)), "{fen}");
        }
    }

    #[test]
    fn the_phase_counts_pieces_and_is_capped() {
        let phase = |fen: &str| tapered_terms(&board(fen)).2;
        assert_eq!(phase("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"), 24);
        assert_eq!(phase("4k3/pppppppp/8/8/8/8/PPPPPPPP/4K3 w - - 0 1"), 0);
        assert_eq!(phase("4k3/8/8/8/8/8/8/R2QK3 w - - 0 1"), 6);
        assert_eq!(phase("rnbqkbnr/pppppppp/8/8/8/8/1PPPPPPP/QNBQKBNR w Kkq - 0 1"), 24, "capped after a promotion");
    }

    #[test]
    fn a_white_piece_reads_the_printed_square_and_a_black_one_its_mirror() {
        // In the page's printed layout, d4 is row 5 (from the top), column 4: index 4 * 8 + 3.
        let printed_d4 = 4 * 8 + 3;
        assert_eq!(table_index(Color::White, Square::D4), printed_d4);
        assert_eq!(table_index(Color::Black, Square::D5), printed_d4);
        assert_eq!(pesto::MG_TABLE[PieceType::Knight as usize][printed_d4], 13);
        assert_eq!(pesto::EG_TABLE[PieceType::Knight as usize][printed_d4], 25);
        // e2 is row 7, column 5.
        assert_eq!(table_index(Color::White, Square::E2), 6 * 8 + 4);
        assert_eq!(pesto::MG_TABLE[PieceType::Pawn as usize][6 * 8 + 4], -15);
    }

    #[test]
    fn a_lone_pawn_matches_the_value_computed_by_hand() {
        // White: king e1, pawn e4. Black: king e8. The kings cancel, the phase is 0, so the value
        // is the endgame score: PeSTO 94 + (-7) for the pawn on e4.
        assert_eq!(evaluate(&board("4k3/8/8/8/4P3/8/8/4K3 w - - 0 1")), 94 - 7);
    }

    #[test]
    fn a_small_position_matches_the_value_computed_by_hand() {
        // White: king e1, knight d4. Black: king e8. The kings read mirrored squares and cancel.
        // Knight: PeSTO mg 337 + 13 = 350, eg 281 + 25 = 306, phase = 1.
        // (350 * 1 + 306 * 23) / 24 = 7388 / 24 = 307, truncated.
        assert_eq!(evaluate(&board("4k3/8/8/8/3N4/8/8/4K3 w - - 0 1")), 307);
        assert_eq!(evaluate(&board("4k3/8/8/8/3N4/8/8/4K3 b - - 0 1")), -307);
    }

    #[test]
    fn pure_opposite_bishops_do_not_halve_the_evaluation() {
        // Each side has one bishop, c8 light and c1 dark, and nothing else but pawns.
        let position = board("2b1k3/pp6/8/8/8/8/PPP5/2B1K3 w - - 0 1");
        assert_ne!(unscaled(&position), 0);
        assert_eq!(evaluate(&position), unscaled(&position));
    }
}
