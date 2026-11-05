use crate::board::bitboard::Bitboard;
use crate::board::chess_move::Move;
use crate::board::color::Color;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use crate::board::Board;
use crate::movegen::bishop::bishop_attacks;
use crate::movegen::rook::rook_attacks;

/// The set of squares a queen on `square` attacks, given the actual board `occupancy`
/// -- exactly the union of what a bishop and a rook would each attack from there, since
/// a queen simply combines both pieces' movement.
pub fn queen_attacks(square: Square, occupancy: Bitboard) -> Bitboard {
    bishop_attacks(square, occupancy) | rook_attacks(square, occupancy)
}

/// Pseudo-legal queen moves for `color`'s queens on `board`, appended onto the caller's
/// shared `moves` buffer rather than returned in one of its own -- see `rook_moves` for
/// the identical shape and reasoning.
pub fn queen_moves(board: &Board, color: Color, moves: &mut Vec<Move>) {
    queen_moves_onto(board, color, !board.occupancy(color), moves);
}

/// `queen_moves` restricted to destinations in `targets` -- see `knight_moves_onto`.
pub fn queen_moves_onto(board: &Board, color: Color, targets: Bitboard, moves: &mut Vec<Move>) {
    let queens = board.bitboard(Piece::new(color, PieceType::Queen));
    let occupancy = board.all_occupancy();

    for from in queens.squares() {
        let destinations = queen_attacks(from, occupancy) & targets;
        for to in destinations.squares() {
            moves.push(Move::new(from, to, None));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queen_attacks_combines_bishop_and_rook_attacks() {
        let occupancy = Bitboard::from_square(Square::D6) | Bitboard::from_square(Square::F6);
        let expected = bishop_attacks(Square::D4, occupancy) | rook_attacks(Square::D4, occupancy);
        assert_eq!(queen_attacks(Square::D4, occupancy), expected);
    }

    #[test]
    fn queen_moves_stops_at_blockers_in_both_move_shapes() {
        // White queen on D4, White pawn on D6 (blocks the file), Black pawn on F6
        // (capturable, blocks that diagonal).
        let (board, _) = Board::from_fen("8/8/3P1p2/8/3Q4/8/8/8 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        queen_moves(&board, Color::White, &mut moves);

        let straight_and_diagonal_blocked: Vec<Square> =
            moves.iter().map(|mv| mv.to()).filter(|square| *square == Square::D6 || *square == Square::F6).collect();

        // D6 (own pawn) must never appear; F6 (enemy pawn) must appear as a capture.
        assert!(!straight_and_diagonal_blocked.contains(&Square::D6));
        assert!(straight_and_diagonal_blocked.contains(&Square::F6));
    }
}
