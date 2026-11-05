use crate::board::bitboard::Bitboard;
use crate::board::chess_move::Move;
use crate::board::color::Color;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use crate::board::Board;

/// The 8 knight-move offsets: (file delta, rank delta).
const KNIGHT_OFFSETS: [(i8, i8); 8] = [
    (1, 2), (2, 1), (2, -1), (1, -2),
    (-1, -2), (-2, -1), (-2, 1), (-1, 2),
];

/// Computes the knight attack set for `square` from scratch, by checking each of the 8
/// offsets against board edges: the reference `KNIGHT_ATTACKS` is tested against.
#[cfg(test)]
fn compute_knight_attacks(square: Square) -> Bitboard {
    let mut attacks = Bitboard::EMPTY;

    for (file_offset, rank_offset) in KNIGHT_OFFSETS {
        if let Some(target) = square.try_offset(file_offset, rank_offset) {
            attacks = attacks | Bitboard::from_square(target);
        }
    }

    attacks
}

/// One knight attack set per square, computed at compile time (`Bitboard::step_attack_table`).
static KNIGHT_ATTACKS: [Bitboard; 64] = Bitboard::step_attack_table(&KNIGHT_OFFSETS);

/// The set of squares a knight on `square` attacks.
pub fn knight_attacks(square: Square) -> Bitboard {
    KNIGHT_ATTACKS[square as usize]
}

/// Pseudo-legal knight moves for `color`'s knights on `board`, appended onto the caller's
/// shared `moves` buffer rather than returned in one of its own -- avoids a fresh heap
/// allocation on every call, which matters once this runs at every node of a perft or
/// search tree. Doesn't check whether a move leaves `color`'s own king in check (see the
/// design notes on the two-phase generation split). For each of `color`'s knights, a
/// destination is legal-in-this-sense whenever the knight attacks it and it isn't
/// occupied by one of `color`'s own pieces (an enemy-occupied destination is a legal
/// capture).
pub fn knight_moves(board: &Board, color: Color, moves: &mut Vec<Move>) {
    knight_moves_onto(board, color, !board.occupancy(color), moves);
}

/// `knight_moves` restricted to destinations in `targets`: the enemy's occupancy gives the
/// captures alone (`noisy_moves`). `targets` must exclude `color`'s own pieces.
pub fn knight_moves_onto(board: &Board, color: Color, targets: Bitboard, moves: &mut Vec<Move>) {
    let knights = board.bitboard(Piece::new(color, PieceType::Knight));

    for from in knights.squares() {
        let destinations = knight_attacks(from) & targets;
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
            assert_eq!(knight_attacks(square), compute_knight_attacks(square), "{square:?}");
        }
    }

    #[test]
    fn corner_square_has_two_attacks() {
        // A knight on A1 can only reach B3 and C2.
        let attacks = knight_attacks(Square::A1);
        let expected = Bitboard::from_square(Square::B3) | Bitboard::from_square(Square::C2);
        assert_eq!(attacks, expected);
    }

    #[test]
    fn center_square_has_eight_attacks() {
        let attacks = knight_attacks(Square::D4);
        let expected = Bitboard::from_square(Square::B3)
            | Bitboard::from_square(Square::B5)
            | Bitboard::from_square(Square::C2)
            | Bitboard::from_square(Square::C6)
            | Bitboard::from_square(Square::E2)
            | Bitboard::from_square(Square::E6)
            | Bitboard::from_square(Square::F3)
            | Bitboard::from_square(Square::F5);
        assert_eq!(attacks, expected);
    }

    #[test]
    fn knight_moves_excludes_own_piece_and_includes_capture() {
        // White knight on D4 (attacks B3, B5, C2, C6, E2, E6, F3, F5); White pawn on B5
        // blocks that destination, Black pawn on C6 is a legal capture.
        let (board, _) = Board::from_fen("8/8/2p5/1P6/3N4/8/8/8 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        knight_moves(&board, Color::White, &mut moves);

        let mut destinations: Vec<Square> = moves.iter().map(|mv| mv.to()).collect();
        let mut expected =
            vec![Square::B3, Square::C2, Square::C6, Square::E2, Square::E6, Square::F3, Square::F5];
        destinations.sort_by_key(|square| *square as u8);
        expected.sort_by_key(|square| *square as u8);
        assert_eq!(destinations, expected);

        for mv in &moves {
            assert_eq!(mv.from(), Square::D4);
            assert_eq!(mv.promotion(), None);
        }
    }

    #[test]
    fn knight_moves_generates_for_every_knight_of_that_color() {
        let (board, _) = Board::from_fen("7N/8/8/8/8/8/8/N7 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        knight_moves(&board, Color::White, &mut moves);
        assert_eq!(moves.len(), 4);

        let mut from_a1: Vec<Square> =
            moves.iter().filter(|mv| mv.from() == Square::A1).map(|mv| mv.to()).collect();
        let mut expected_a1 = vec![Square::B3, Square::C2];
        from_a1.sort_by_key(|square| *square as u8);
        expected_a1.sort_by_key(|square| *square as u8);
        assert_eq!(from_a1, expected_a1);

        let mut from_h8: Vec<Square> =
            moves.iter().filter(|mv| mv.from() == Square::H8).map(|mv| mv.to()).collect();
        let mut expected_h8 = vec![Square::F7, Square::G6];
        from_h8.sort_by_key(|square| *square as u8);
        expected_h8.sort_by_key(|square| *square as u8);
        assert_eq!(from_h8, expected_h8);
    }
}
