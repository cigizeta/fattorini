use crate::board::bitboard::Bitboard;
use crate::board::castling_rights::CastlingSide;
use crate::board::chess_move::Move;
use crate::board::color::Color;
use crate::board::square::Square;
use crate::board::Board;
use crate::movegen::attacks::is_square_attacked;

/// Everything needed to check and generate one castling option (one color, one side):
/// where the king starts, the square it crosses, where it ends up, and which squares
/// must be empty for the move to even be considered. Built from `CastlingSide::squares`
/// (the raw king/rook geometry, shared with move application) plus the generation-
/// specific `empty_squares` set.
struct CastlingPath {
    king_from: Square,
    transit_square: Square,
    king_to: Square,
    empty_squares: Bitboard,
}

/// White or Black's kingside castling geometry: king e1/e8 to g1/g8, crossing f1/f8. The
/// king's transit square and the rook's landing square are always the same square (the
/// rook lands exactly where the king crosses), so `transit_square` comes straight from
/// `squares.rook_to` rather than being its own separate fact to keep in sync.
fn kingside_path(color: Color) -> CastlingPath {
    let squares = CastlingSide::Kingside.squares(color);
    CastlingPath {
        king_from: squares.king_from,
        transit_square: squares.rook_to,
        king_to: squares.king_to,
        empty_squares: Bitboard::from_square(squares.rook_to) | Bitboard::from_square(squares.king_to),
    }
}

/// White or Black's queenside castling geometry: king e1/e8 to c1/c8, crossing d1/d8.
/// One extra square (b1/b8) must also be empty for the rook to slide through, even though
/// the king never passes through it -- it's always one file outward from the rook's own
/// home square, in the direction away from the king, regardless of color (a1/a8 are
/// always the outermost queenside square), so it's derived via `try_offset` rather than
/// written out per color.
fn queenside_path(color: Color) -> CastlingPath {
    let squares = CastlingSide::Queenside.squares(color);
    let outer_empty_square = squares.rook_from.try_offset(1, 0).unwrap();
    CastlingPath {
        king_from: squares.king_from,
        transit_square: squares.rook_to,
        king_to: squares.king_to,
        empty_squares: Bitboard::from_square(outer_empty_square)
            | Bitboard::from_square(squares.rook_to)
            | Bitboard::from_square(squares.king_to),
    }
}

/// Pseudo-legal castling moves for `color` on `board`, appended onto the caller's shared
/// `moves` buffer rather than returned in one of its own -- avoids a fresh heap
/// allocation on every call, which matters once this runs at every node of a perft or
/// search tree. Unlike the other piece-specific generators, "pseudo-legal" here already
/// includes the opponent-attack check along the king's path -- castling has no separate,
/// cheaper notion of legality to defer to a later filtering pass, since the rule itself
/// is about check, not just occupancy.
pub fn castling_moves(board: &Board, color: Color, moves: &mut Vec<Move>) {
    let rights = board.castling_rights();
    let occupancy = board.all_occupancy();
    let opponent = color.opposite();

    if rights.kingside(color) {
        try_add_castle(moves, board, occupancy, opponent, kingside_path(color));
    }
    if rights.queenside(color) {
        try_add_castle(moves, board, occupancy, opponent, queenside_path(color));
    }
}

/// Adds `path`'s castling move if nothing blocks it: the squares between king and rook
/// must all be empty, and no square on the king's own from -> transit -> to path may be
/// attacked by `opponent` -- checking `king_from` this way is exactly "the king isn't
/// currently in check," folded into the same loop as the other two squares rather than
/// needing its own separate check.
fn try_add_castle(moves: &mut Vec<Move>, board: &Board, occupancy: Bitboard, opponent: Color, path: CastlingPath) {
    if (occupancy & path.empty_squares) != Bitboard::EMPTY {
        return;
    }

    for square in [path.king_from, path.transit_square, path.king_to] {
        if is_square_attacked(board, square, opponent) {
            return;
        }
    }

    moves.push(Move::castle(path.king_from, path.king_to));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::chess_move::MoveKind;

    #[test]
    fn kingside_castle_generated_when_clear() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let mut moves = Vec::new();
        castling_moves(&board, Color::White, &mut moves);
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].from(), Square::E1);
        assert_eq!(moves[0].to(), Square::G1);
        assert_eq!(moves[0].kind(), MoveKind::Castle);
    }

    #[test]
    fn queenside_castle_generated_when_clear() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/R3K3 w Q - 0 1").unwrap();
        let mut moves = Vec::new();
        castling_moves(&board, Color::White, &mut moves);
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].from(), Square::E1);
        assert_eq!(moves[0].to(), Square::C1);
        assert_eq!(moves[0].kind(), MoveKind::Castle);
    }

    #[test]
    fn no_rights_means_no_castling_moves() {
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/R3K2R w - - 0 1").unwrap();
        let mut moves = Vec::new();
        castling_moves(&board, Color::White, &mut moves);
        assert!(moves.is_empty());
    }

    #[test]
    fn blocked_path_prevents_castling() {
        // Bishop on f1 blocks kingside; queenside is still clear.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/R3K1BR w KQ - 0 1").unwrap();
        let mut moves = Vec::new();
        castling_moves(&board, Color::White, &mut moves);
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].to(), Square::C1);
    }

    #[test]
    fn cannot_castle_out_of_check() {
        // Black rook on e-file pins the White king in check on e1.
        let (board, _) = Board::from_fen("4r3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let mut moves = Vec::new();
        castling_moves(&board, Color::White, &mut moves);
        assert!(moves.is_empty());
    }

    #[test]
    fn cannot_castle_through_attacked_square() {
        // Black rook on the f-file attacks f1, the square the king must cross.
        let (board, _) = Board::from_fen("4k3/5r2/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let mut moves = Vec::new();
        castling_moves(&board, Color::White, &mut moves);
        assert!(moves.is_empty());
    }

    #[test]
    fn black_castling_generated_symmetrically() {
        let (board, _) = Board::from_fen("r3k2r/8/8/8/8/8/8/4K3 b kq - 0 1").unwrap();
        let mut moves = Vec::new();
        castling_moves(&board, Color::Black, &mut moves);
        let mut destinations: Vec<Square> = moves.iter().map(|mv| mv.to()).collect();
        destinations.sort_by_key(|square| *square as u8);
        let mut expected = vec![Square::G8, Square::C8];
        expected.sort_by_key(|square| *square as u8);
        assert_eq!(destinations, expected);
    }
}
