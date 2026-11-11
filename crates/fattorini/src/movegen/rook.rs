use crate::board::bitboard::Bitboard;
use crate::board::chess_move::Move;
use crate::board::color::Color;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use crate::board::Board;
use crate::movegen::sliding::SlidingAttackTable;
use std::sync::LazyLock;

/// The 4 directions a rook rays out along: the 4 orthogonal (rank/file) directions.
const ROOK_DIRECTIONS: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// Built once and cached forever -- same "pure function over a small, fixed,
/// never-changing domain" reasoning as `KNIGHT_ATTACKS`/`KING_ATTACKS`, just built from
/// `SlidingAttackTable` instead of a flat per-square table.
static ROOK_ATTACK_TABLE: LazyLock<SlidingAttackTable> = LazyLock::new(|| SlidingAttackTable::build(ROOK_DIRECTIONS));

/// The set of squares a rook on `square` attacks, given the actual board `occupancy`.
pub fn rook_attacks(square: Square, occupancy: Bitboard) -> Bitboard {
    ROOK_ATTACK_TABLE.attacks(square, occupancy)
}

/// Pseudo-legal rook moves for `color`'s rooks on `board`, appended onto the caller's
/// shared `moves` buffer rather than returned in one of its own -- avoids a fresh heap
/// allocation on every call, which matters once this runs at every node of a perft or
/// search tree. Doesn't check whether a move leaves `color`'s own king in check (see the
/// design notes on the two-phase generation split). Shape identical to
/// `knight_moves`/`king_moves`: a destination is legal-in-this-sense whenever the rook
/// attacks it and it isn't occupied by one of `color`'s own pieces.
pub fn rook_moves(board: &Board, color: Color, moves: &mut Vec<Move>) {
    rook_moves_onto(board, color, !board.occupancy(color), moves);
}

/// `rook_moves` restricted to destinations in `targets` -- see `knight_moves_onto`.
pub fn rook_moves_onto(board: &Board, color: Color, targets: Bitboard, moves: &mut Vec<Move>) {
    let rooks = board.bitboard(Piece::new(color, PieceType::Rook));
    let occupancy = board.all_occupancy();

    for from in rooks.squares() {
        let destinations = rook_attacks(from, occupancy) & targets;
        for to in destinations.squares() {
            moves.push(Move::new(from, to, None));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rook_moves_stops_at_blocker_and_can_capture() {
        // White rook on D4, White pawn on D6 (blocks beyond it), Black pawn on B4
        // (capturable, blocks beyond it).
        let (board, _) = Board::from_fen("8/8/3P4/8/1p1R4/8/8/8 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        rook_moves(&board, Color::White, &mut moves);

        let mut destinations: Vec<Square> = moves.iter().map(|mv| mv.to()).collect();
        destinations.sort_by_key(|square| *square as u8);

        let mut expected = vec![
            Square::D5, Square::D3, Square::D2, Square::D1, // vertical, unblocked side
            Square::C4, Square::B4, // horizontal toward the capture
            Square::E4, Square::F4, Square::G4, Square::H4, // horizontal, unblocked side
        ];
        expected.sort_by_key(|square| *square as u8);
        assert_eq!(destinations, expected);
    }
}
