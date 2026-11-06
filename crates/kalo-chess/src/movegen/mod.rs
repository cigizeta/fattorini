pub mod attacks;
pub mod bishop;
pub mod castling;
pub mod king;
pub mod knight;
pub mod legal;
pub mod pawn;
pub mod queen;
pub mod rook;
pub mod see;
pub mod sliding;

use crate::board::chess_move::Move;
use crate::board::color::Color;
use crate::board::Board;

/// Every pseudo-legal move available to `color` in `board`, appended onto the caller's
/// shared `moves` buffer -- the union of all seven per-piece-type generators, nothing
/// filtered for check yet (that's `legal_moves`'s job). A thin combinator, not a
/// generator itself: each piece type already knows how to generate its own moves, this
/// just calls all of them with the same buffer. No merging needed (unlike when each
/// generator returned its own Vec) -- they all write into the same target already.
pub fn pseudo_legal_moves(board: &Board, color: Color, moves: &mut Vec<Move>) {
    pawn::pawn_moves(board, color, moves);
    knight::knight_moves(board, color, moves);
    bishop::bishop_moves(board, color, moves);
    rook::rook_moves(board, color, moves);
    queen::queen_moves(board, color, moves);
    king::king_moves(board, color, moves);
    castling::castling_moves(board, color, moves);
}

/// The pseudo-legal moves of `color` that capture or promote, and no other: exactly the moves of
/// `pseudo_legal_moves` that are not quiet, for quiescence outside of check. Built from each
/// generator restricted to the enemy's pieces as destinations (and, for pawns, the push onto the
/// last rank), so the quiet moves are never generated at all. Castling is never noisy.
pub fn noisy_moves(board: &Board, color: Color, moves: &mut Vec<Move>) {
    let enemies = board.occupancy(color.opposite());
    pawn::pawn_noisy_moves(board, color, moves);
    knight::knight_moves_onto(board, color, enemies, moves);
    bishop::bishop_moves_onto(board, color, enemies, moves);
    rook::rook_moves_onto(board, color, enemies, moves);
    queen::queen_moves_onto(board, color, enemies, moves);
    king::king_moves_onto(board, color, enemies, moves);
}

/// The pseudo-legal moves of `color` that neither capture nor promote: the rest of
/// `pseudo_legal_moves` after `noisy_moves`, castling included. Generated only when the move picker
/// reaches the quiet moves, so a node cut off by its TT move or a capture never generates them.
pub fn quiet_moves(board: &Board, color: Color, moves: &mut Vec<Move>) {
    let empty = !board.all_occupancy();
    pawn::pawn_quiet_moves(board, color, moves);
    knight::knight_moves_onto(board, color, empty, moves);
    bishop::bishop_moves_onto(board, color, empty, moves);
    rook::rook_moves_onto(board, color, empty, moves);
    queen::queen_moves_onto(board, color, empty, moves);
    king::king_moves_onto(board, color, empty, moves);
    castling::castling_moves(board, color, moves);
}

/// Whether `pseudo_legal_moves` would generate `mv` for `color` on `board`, checked without
/// generating them: the move picker asks it of a TT move or a killer before trying one, since with
/// staged generation the moves they would be found among may not exist yet. A TT move can come from
/// another position sharing the hash, so anything at all must be answered correctly.
pub fn is_pseudo_legal(board: &Board, color: Color, mv: Move) -> bool {
    use crate::board::chess_move::MoveKind;
    use crate::board::piece_type::PieceType;

    let Some(piece) = board.piece_at(mv.from()) else { return false };
    if piece.color() != color {
        return false;
    }
    if piece.piece_type() == PieceType::Pawn {
        return pawn::is_pseudo_legal_pawn_move(board, color, mv);
    }
    if mv.kind() == MoveKind::Castle {
        if piece.piece_type() != PieceType::King {
            return false;
        }
        // Rare enough (a castling TT move or killer) that generating the at most two castles is fine.
        let mut castles = Vec::new();
        castling::castling_moves(board, color, &mut castles);
        return castles.contains(&mv);
    }
    if mv.kind() != MoveKind::Quiet || mv.promotion().is_some() {
        return false;
    }
    let occupancy = board.all_occupancy();
    let attacks = match piece.piece_type() {
        PieceType::Knight => knight::knight_attacks(mv.from()),
        PieceType::Bishop => bishop::bishop_attacks(mv.from(), occupancy),
        PieceType::Rook => rook::rook_attacks(mv.from(), occupancy),
        PieceType::Queen => queen::queen_attacks(mv.from(), occupancy),
        PieceType::King => king::king_attacks(mv.from()),
        PieceType::Pawn => unreachable!("pawns are answered above"),
    };
    let targets = attacks & !board.occupancy(color);
    (targets & crate::board::bitboard::Bitboard::from_square(mv.to())) != crate::board::bitboard::Bitboard::EMPTY
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Positions covering every kind of move for both colours: castling rights, en passant,
    /// promotions by push and capture, a push blocked on the last rank, and a quiet start.
    const VARIED_FENS: [&str; 11] = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
        "rnbqkbnr/pppp1ppp/8/8/3PpP2/8/PPP1P1PP/RNBQKBNR b KQkq f3 0 3",
        "r1n1k3/1P1P2P1/8/8/8/8/1p1p2p1/R1N1K3 w - - 0 1",
        "r1n1k3/1P1P2P1/8/8/8/8/1p1p2p1/R1N1K3 b - - 0 1",
        "n3k3/P7/8/8/8/8/7p/4K2N w - - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    ];

    fn sort_key(mv: &Move) -> (usize, usize, usize, usize) {
        (mv.from() as usize, mv.to() as usize, mv.promotion().map_or(0, |p| p as usize + 1), mv.kind() as usize)
    }

    #[test]
    fn noisy_and_quiet_moves_are_exactly_the_pseudo_legal_moves() {
        for fen in VARIED_FENS {
            let (board, _) = Board::from_fen(fen).unwrap();
            let color = board.side_to_move();
            let mut expected = Vec::new();
            pseudo_legal_moves(&board, color, &mut expected);
            let mut staged = Vec::new();
            noisy_moves(&board, color, &mut staged);
            quiet_moves(&board, color, &mut staged);
            expected.sort_by_key(sort_key);
            staged.sort_by_key(sort_key);
            assert_eq!(staged, expected, "{fen}");
        }
    }

    #[test]
    fn is_pseudo_legal_answers_exactly_the_generated_moves() {
        use crate::board::piece_type::PieceType;
        use crate::board::square::Square;
        // Every from-to pair, as every kind of move and every promotion, for both sides to move:
        // the answer must be membership in `pseudo_legal_moves`, for moves of the other side too.
        for fen in VARIED_FENS {
            let (board, _) = Board::from_fen(fen).unwrap();
            for color in [Color::White, Color::Black] {
                let mut generated = Vec::new();
                pseudo_legal_moves(&board, color, &mut generated);
                for from in 0u8..64 {
                    for to in 0u8..64 {
                        let (from, to) = (Square::try_from(from).unwrap(), Square::try_from(to).unwrap());
                        let mut candidates = vec![
                            Move::new(from, to, None),
                            Move::double_pawn_push(from, to),
                            Move::en_passant_capture(from, to),
                            Move::castle(from, to),
                        ];
                        for promotion in [PieceType::Knight, PieceType::Bishop, PieceType::Rook, PieceType::Queen] {
                            candidates.push(Move::new(from, to, Some(promotion)));
                        }
                        for mv in candidates {
                            assert_eq!(is_pseudo_legal(&board, color, mv), generated.contains(&mv), "{fen} {color:?} {mv:?}");
                        }
                    }
                }
            }
        }
    }

    /// Captures or promotes on `board`: a destination holding a piece, en passant, or a promotion.
    fn is_noisy(board: &Board, mv: Move) -> bool {
        use crate::board::chess_move::MoveKind;
        board.piece_at(mv.to()).is_some() || mv.kind() == MoveKind::EnPassantCapture || mv.promotion().is_some()
    }

    /// `noisy_moves` is exactly the noisy part of `pseudo_legal_moves`, as a multiset, for both
    /// colours: captures by every piece type, en passant, promotions by push and by capture
    /// (including a push blocked on the last rank), castling rights present, and a quiet start.
    #[test]
    fn noisy_moves_are_exactly_the_noisy_pseudo_legal_moves() {
        let fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
            "rnbqkbnr/pppp1ppp/8/8/3PpP2/8/PPP1P1PP/RNBQKBNR b KQkq f3 0 3",
            "r1n1k3/1P1P2P1/8/8/8/8/1p1p2p1/R1N1K3 w - - 0 1",
            "r1n1k3/1P1P2P1/8/8/8/8/1p1p2p1/R1N1K3 b - - 0 1",
            "n3k3/P7/8/8/8/8/7p/4K2N w - - 0 1",
            "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
            "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        ];
        for fen in fens {
            let (board, _) = Board::from_fen(fen).unwrap();
            let color = board.side_to_move();
            let mut expected = Vec::new();
            pseudo_legal_moves(&board, color, &mut expected);
            expected.retain(|&mv| is_noisy(&board, mv));
            let mut noisy = Vec::new();
            noisy_moves(&board, color, &mut noisy);
            let key = |mv: &Move| (mv.from() as usize, mv.to() as usize, mv.promotion().map_or(0, |p| p as usize + 1));
            expected.sort_by_key(key);
            noisy.sort_by_key(key);
            assert_eq!(noisy, expected, "{fen}");
        }
    }

    #[test]
    fn starting_position_has_twenty_pseudo_legal_moves() {
        // The textbook fact: White's first move has exactly 20 options (16 pawn moves --
        // 8 single, 8 double -- plus 4 knight moves; nothing else can move yet). No
        // checks are possible this early, so pseudo-legal and legal coincide here.
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let mut moves = Vec::new();
        pseudo_legal_moves(&board, Color::White, &mut moves);
        assert_eq!(moves.len(), 20);
    }

    #[test]
    fn combines_every_generator_without_double_counting() {
        // King, rook, and castling all apply here (kingside right only) -- nothing else
        // is on the board. The combined count should be exactly the sum of what each
        // generator produces on its own, no more and no less.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let mut combined = Vec::new();
        pseudo_legal_moves(&board, Color::White, &mut combined);

        let mut king_only = Vec::new();
        king::king_moves(&board, Color::White, &mut king_only);
        let mut rook_only = Vec::new();
        rook::rook_moves(&board, Color::White, &mut rook_only);
        let mut castling_only = Vec::new();
        castling::castling_moves(&board, Color::White, &mut castling_only);

        assert_eq!(combined.len(), king_only.len() + rook_only.len() + castling_only.len());
    }
}
