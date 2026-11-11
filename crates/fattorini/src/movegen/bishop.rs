use crate::board::bitboard::Bitboard;
use crate::board::chess_move::Move;
use crate::board::color::Color;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use crate::board::Board;
use crate::movegen::sliding::SlidingAttackTable;
use std::sync::LazyLock;

/// The 4 directions a bishop rays out along: the 4 diagonals.
const BISHOP_DIRECTIONS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

/// Built once and cached forever -- same reasoning as `ROOK_ATTACK_TABLE`.
static BISHOP_ATTACK_TABLE: LazyLock<SlidingAttackTable> =
    LazyLock::new(|| SlidingAttackTable::build(BISHOP_DIRECTIONS));

/// The set of squares a bishop on `square` attacks, given the actual board `occupancy`.
pub fn bishop_attacks(square: Square, occupancy: Bitboard) -> Bitboard {
    BISHOP_ATTACK_TABLE.attacks(square, occupancy)
}

/// Pseudo-legal bishop moves for `color`'s bishops on `board`, appended onto the
/// caller's shared `moves` buffer rather than returned in one of its own -- see
/// `rook_moves` for the identical shape and reasoning.
pub fn bishop_moves(board: &Board, color: Color, moves: &mut Vec<Move>) {
    bishop_moves_onto(board, color, !board.occupancy(color), moves);
}

/// `bishop_moves` restricted to destinations in `targets` -- see `knight_moves_onto`.
pub fn bishop_moves_onto(board: &Board, color: Color, targets: Bitboard, moves: &mut Vec<Move>) {
    let bishops = board.bitboard(Piece::new(color, PieceType::Bishop));
    let occupancy = board.all_occupancy();

    for from in bishops.squares() {
        let destinations = bishop_attacks(from, occupancy) & targets;
        for to in destinations.squares() {
            moves.push(Move::new(from, to, None));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bishop_moves_stops_at_blocker_and_can_capture() {
        // White bishop on D4, White pawn on F6 (blocks that diagonal), Black pawn on B2
        // (capturable, blocks beyond it). Other two diagonals (toward A7 and toward G1)
        // are open.
        let (board, _) = Board::from_fen("8/8/5P2/8/3B4/8/1p6/8 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        bishop_moves(&board, Color::White, &mut moves);

        let mut destinations: Vec<Square> = moves.iter().map(|mv| mv.to()).collect();
        destinations.sort_by_key(|square| *square as u8);

        let mut expected = vec![
            Square::E5, // toward F6, stops before it (own piece)
            Square::C5, Square::B6, Square::A7, // open diagonal
            Square::E3, Square::F2, Square::G1, // open diagonal
            Square::C3, Square::B2, // toward B2, captures it
        ];
        expected.sort_by_key(|square| *square as u8);
        assert_eq!(destinations, expected);
    }
}
