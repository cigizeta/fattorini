use crate::board::bitboard::Bitboard;
use crate::board::color::Color;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use crate::board::Board;
use crate::movegen::bishop::bishop_attacks;
use crate::movegen::king::king_attacks;
use crate::movegen::knight::knight_attacks;
use crate::movegen::pawn::pawn_attacks;
use crate::movegen::rook::rook_attacks;

/// Whether any `by_color` piece currently attacks `square` -- the question both castling
/// (can't castle out of, through, or into check) and legality filtering (does this move
/// leave my own king in check) need answered.
///
/// The technique: place a virtual piece of each type on `square` and check whether it
/// would attack a real `by_color` piece of that same type. A knight "attacks" wherever a
/// knight would if it stood there; if a real `by_color` knight sits on one of those
/// squares, the two attack each other -- attacking is symmetric for a given piece type
/// sitting on either end, so this reuses the exact same attack tables already built for
/// move generation, no new geometry needed.
pub fn is_square_attacked(board: &Board, square: Square, by_color: Color) -> bool {
    let occupancy = board.all_occupancy();

    if (knight_attacks(square) & board.bitboard(Piece::new(by_color, PieceType::Knight))) != Bitboard::EMPTY {
        return true;
    }

    if (king_attacks(square) & board.bitboard(Piece::new(by_color, PieceType::King))) != Bitboard::EMPTY {
        return true;
    }

    let diagonal_attackers = board.bitboard(Piece::new(by_color, PieceType::Bishop))
        | board.bitboard(Piece::new(by_color, PieceType::Queen));
    if (bishop_attacks(square, occupancy) & diagonal_attackers) != Bitboard::EMPTY {
        return true;
    }

    let straight_attackers = board.bitboard(Piece::new(by_color, PieceType::Rook))
        | board.bitboard(Piece::new(by_color, PieceType::Queen));
    if (rook_attacks(square, occupancy) & straight_attackers) != Bitboard::EMPTY {
        return true;
    }

    // Pawns are the one asymmetric case: a pawn's attack pattern depends on which way it
    // points. The squares a `by_color` pawn would attack *into* `square` are exactly the
    // squares `pawn_attacks(square, by_color.opposite())` gives -- diagonal attacks are a
    // mirror image under a color flip, so this reuses the same table by flipping color
    // once, rather than needing a second, inverted one.
    let pawn_attackers = board.bitboard(Piece::new(by_color, PieceType::Pawn));
    if (pawn_attacks(square, by_color.opposite()) & pawn_attackers) != Bitboard::EMPTY {
        return true;
    }

    false
}

/// `color`'s king's current square. Every valid position has exactly one king per color
/// -- `Bitboard::squares()` only ever yields the first (and only) set bit here.
pub fn king_square(board: &Board, color: Color) -> Square {
    let king = board.bitboard(Piece::new(color, PieceType::King));
    king.squares().next().expect("a valid position always has exactly one king per color")
}

/// Whether `color`'s king is currently attacked by the opponent -- "is `color` in check,"
/// built directly from `king_square` and `is_square_attacked`.
pub fn is_in_check(board: &Board, color: Color) -> bool {
    is_square_attacked(board, king_square(board, color), color.opposite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_knight_attack() {
        let (board, _) = Board::from_fen("8/8/8/3n4/8/8/8/8 w - - 0 1").unwrap();
        assert!(is_square_attacked(&board, Square::E3, Color::Black));
        assert!(!is_square_attacked(&board, Square::E4, Color::Black));
    }

    #[test]
    fn detects_pawn_attack_and_respects_direction() {
        // Black pawn on D5 attacks C4 and E4, not C6/E6 (that would be a white pawn's
        // direction).
        let (board, _) = Board::from_fen("8/8/8/3p4/8/8/8/8 w - - 0 1").unwrap();
        assert!(is_square_attacked(&board, Square::C4, Color::Black));
        assert!(is_square_attacked(&board, Square::E4, Color::Black));
        assert!(!is_square_attacked(&board, Square::C6, Color::Black));
    }

    #[test]
    fn detects_sliding_attack_through_open_squares_but_not_through_blockers() {
        // Black rook on D8, open file down to D1: attacks D4. A white pawn on D6 blocks
        // it from reaching D4 or beyond.
        let (board, _) = Board::from_fen("3r4/8/8/8/8/8/8/8 w - - 0 1").unwrap();
        assert!(is_square_attacked(&board, Square::D4, Color::Black));

        let (board, _) = Board::from_fen("3r4/8/3P4/8/8/8/8/8 w - - 0 1").unwrap();
        assert!(!is_square_attacked(&board, Square::D4, Color::Black));
    }

    #[test]
    fn empty_board_attacks_nothing() {
        let (board, _) = Board::from_fen("8/8/8/8/8/8/8/8 w - - 0 1").unwrap();
        assert!(!is_square_attacked(&board, Square::D4, Color::White));
        assert!(!is_square_attacked(&board, Square::D4, Color::Black));
    }

    #[test]
    fn king_square_finds_the_king() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(king_square(&board, Color::White), Square::E1);
        assert_eq!(king_square(&board, Color::Black), Square::E8);
    }

    #[test]
    fn is_in_check_reflects_whether_the_king_is_attacked() {
        // Black king a8 (safely out of the way), Black rook e8 gives check to the White
        // king on e1.
        let (board, _) = Board::from_fen("k3r3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert!(is_in_check(&board, Color::White));
        assert!(!is_in_check(&board, Color::Black));
    }
}
