use crate::board::bitboard::Bitboard;
use crate::board::chess_move::Move;
use crate::board::color::Color;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use crate::board::Board;

/// The 8 one-step king-move offsets: (file delta, rank delta).
const KING_OFFSETS: [(i8, i8); 8] = [
    (1, 0), (1, 1), (0, 1), (-1, 1),
    (-1, 0), (-1, -1), (0, -1), (1, -1),
];

/// Computes the king attack set for `square` from scratch, by checking each of the 8
/// offsets against board edges: the reference `KING_ATTACKS` is tested against. Only the
/// squares a king threatens; castling is a separate concern.
#[cfg(test)]
fn compute_king_attacks(square: Square) -> Bitboard {
    let mut attacks = Bitboard::EMPTY;

    for (file_offset, rank_offset) in KING_OFFSETS {
        if let Some(target) = square.try_offset(file_offset, rank_offset) {
            attacks = attacks | Bitboard::from_square(target);
        }
    }

    attacks
}

/// One king attack set per square, computed at compile time (`Bitboard::step_attack_table`).
static KING_ATTACKS: [Bitboard; 64] = Bitboard::step_attack_table(&KING_OFFSETS);

/// The set of squares a king on `square` attacks. Does not include castling -- that's a
/// compound king+rook move with its own preconditions, not part of "squares threatened."
pub fn king_attacks(square: Square) -> Bitboard {
    KING_ATTACKS[square as usize]
}

/// Pseudo-legal king moves for `color`'s king on `board`, appended onto the caller's
/// shared `moves` buffer rather than returned in one of its own -- avoids a fresh heap
/// allocation on every call, which matters once this runs at every node of a perft or
/// search tree. Doesn't check whether a move leaves `color`'s own king in check (see the
/// design notes on the two-phase generation split), and doesn't include castling (a
/// separate, later concern). A destination is legal-in-this-sense whenever the king
/// attacks it and it isn't occupied by one of `color`'s own pieces (an enemy-occupied
/// destination is a legal capture).
pub fn king_moves(board: &Board, color: Color, moves: &mut Vec<Move>) {
    king_moves_onto(board, color, !board.occupancy(color), moves);
}

/// `king_moves` restricted to destinations in `targets` -- see `knight_moves_onto`.
pub fn king_moves_onto(board: &Board, color: Color, targets: Bitboard, moves: &mut Vec<Move>) {
    let king = board.bitboard(Piece::new(color, PieceType::King));

    for from in king.squares() {
        let destinations = king_attacks(from) & targets;
        for to in destinations.squares() {
            moves.push(Move::new(from, to, None));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compile_time_table_matches_the_offsets_on_every_square() {
        for square_value in 0u8..64 {
            let square = Square::try_from(square_value).unwrap();
            assert_eq!(king_attacks(square), compute_king_attacks(square), "{square:?}");
        }
    }

    #[test]
    fn corner_square_has_three_attacks() {
        let attacks = king_attacks(Square::A1);
        let expected = Bitboard::from_square(Square::A2)
            | Bitboard::from_square(Square::B2)
            | Bitboard::from_square(Square::B1);
        assert_eq!(attacks, expected);
    }

    #[test]
    fn center_square_has_eight_attacks() {
        let attacks = king_attacks(Square::D4);
        let expected = Bitboard::from_square(Square::C3)
            | Bitboard::from_square(Square::C4)
            | Bitboard::from_square(Square::C5)
            | Bitboard::from_square(Square::D3)
            | Bitboard::from_square(Square::D5)
            | Bitboard::from_square(Square::E3)
            | Bitboard::from_square(Square::E4)
            | Bitboard::from_square(Square::E5);
        assert_eq!(attacks, expected);
    }

    #[test]
    fn king_moves_excludes_own_piece_and_includes_capture() {
        // White king on A1 (attacks A2, B1, B2); White pawn on B1 blocks that
        // destination, Black pawn on B2 is a legal capture.
        let (board, _) = Board::from_fen("8/8/8/8/8/8/1p6/KP6 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        king_moves(&board, Color::White, &mut moves);

        let mut destinations: Vec<Square> = moves.iter().map(|mv| mv.to()).collect();
        let mut expected = vec![Square::A2, Square::B2];
        destinations.sort_by_key(|square| *square as u8);
        expected.sort_by_key(|square| *square as u8);
        assert_eq!(destinations, expected);

        for mv in &moves {
            assert_eq!(mv.from(), Square::A1);
            assert_eq!(mv.promotion(), None);
        }
    }

    #[test]
    fn king_moves_from_center_on_empty_board() {
        let (board, _) = Board::from_fen("8/8/8/8/3K4/8/8/8 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        king_moves(&board, Color::White, &mut moves);
        assert_eq!(moves.len(), 8);
        for mv in &moves {
            assert_eq!(mv.from(), Square::D4);
        }
    }
}
