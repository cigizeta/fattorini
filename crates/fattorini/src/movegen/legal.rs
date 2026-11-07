use crate::board::bitboard::Bitboard;
use crate::board::chess_move::{Move, MoveKind};
use crate::board::color::Color;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use crate::board::Board;
use crate::movegen::attacks::{is_in_check, king_square};
use crate::movegen::bishop::bishop_attacks;
use crate::movegen::pseudo_legal_moves;
use crate::movegen::rook::rook_attacks;
use crate::movegen::see::attackers_to;

/// Every legal move available to `color` in `board`: every pseudo-legal move that doesn't
/// leave `color`'s own king in check (`Legality`). The standalone utility version, useful on
/// its own regardless of search (e.g. perft, or answering "what can be played here" for UCI),
/// so it returns an owned `Vec<Move>` rather than asking its own caller to manage a buffer.
pub fn legal_moves(board: &Board, color: Color) -> Vec<Move> {
    let mut candidates = Vec::new();
    pseudo_legal_moves(board, color, &mut candidates);
    let legality = Legality::new(board, color);
    candidates.retain(|&mv| legality.is_legal(board, mv));
    candidates
}

/// What a pseudo-legal move of `color` needs to be judged legal on the board before it, without
/// making it: `color`'s king square, the pieces pinned to it, and the pieces giving check.
/// Computed once per position, then `is_legal` per move, as Stockfish's `Position::legal` with
/// its `blockers_for_king`: making each move, testing for check and unmaking it costs far more,
/// and a search prunes many moves without ever needing the position after them.
pub struct Legality {
    color: Color,
    king: Square,
    pinned: Bitboard,
    checkers: Bitboard,
}

impl Legality {
    pub fn new(board: &Board, color: Color) -> Legality {
        let king = king_square(board, color);
        let enemy = color.opposite();
        let enemies = board.occupancy(enemy);
        let occupancy = board.all_occupancy();
        let checkers = attackers_to(board, king, occupancy) & enemies;

        // An enemy slider aligned with the king on an empty board pins the one piece between
        // them, if exactly one stands there and it is `color`'s.
        let queens = board.bitboard(Piece::new(enemy, PieceType::Queen));
        let straight = board.bitboard(Piece::new(enemy, PieceType::Rook)) | queens;
        let diagonal = board.bitboard(Piece::new(enemy, PieceType::Bishop)) | queens;
        let snipers = (rook_attacks(king, Bitboard::EMPTY) & straight) | (bishop_attacks(king, Bitboard::EMPTY) & diagonal);
        let mut pinned = Bitboard::EMPTY;
        for sniper in snipers.squares() {
            let blockers = between(king, sniper) & occupancy;
            if blockers.count() == 1 && (blockers & board.occupancy(color)) != Bitboard::EMPTY {
                pinned = pinned | blockers;
            }
        }
        Legality { color, king, pinned, checkers }
    }

    /// Whether the pseudo-legal `mv` leaves `color`'s king out of check, judged on `board`, the
    /// position before it. Equals making the move and asking `is_in_check`, exactly.
    pub fn is_legal(&self, board: &Board, mv: Move) -> bool {
        let from = mv.from();
        let to = mv.to();
        match mv.kind() {
            // The generator already checks every square of the king's path.
            MoveKind::Castle => return true,
            // Two pawns leave one rank at once, which can open it onto the king: rare, so tested
            // the plain way.
            MoveKind::EnPassantCapture => {
                let mut after = *board;
                after.make_move(mv);
                return !is_in_check(&after, self.color);
            }
            _ => {}
        }

        if from == self.king {
            // With the king off its square, so a slider checking along the line it steps back on
            // still counts.
            let occupancy = board.all_occupancy() ^ Bitboard::from_square(from);
            let attackers = attackers_to(board, to, occupancy) & board.occupancy(self.color.opposite());
            return attackers == Bitboard::EMPTY;
        }

        let to_square = Bitboard::from_square(to);
        if self.checkers != Bitboard::EMPTY {
            // In double check only the king moves; in single check, the move captures the
            // checker or blocks between it and the king.
            if self.checkers.count() > 1 {
                return false;
            }
            let checker = self.checkers.squares().next().expect("one checker");
            if (to_square & (self.checkers | between(self.king, checker))) == Bitboard::EMPTY {
                return false;
            }
        }
        if (self.pinned & Bitboard::from_square(from)) != Bitboard::EMPTY {
            return (line(self.king, from) & to_square) != Bitboard::EMPTY;
        }
        true
    }
}

/// The squares strictly between `a` and `b` when they share a rank, file or diagonal; empty
/// otherwise. From the slider lookups: each square's attacks with only the other one as a
/// blocker meet exactly between them.
fn between(a: Square, b: Square) -> Bitboard {
    let (from_a, from_b) = (Bitboard::from_square(a), Bitboard::from_square(b));
    if (rook_attacks(a, Bitboard::EMPTY) & from_b) != Bitboard::EMPTY {
        return rook_attacks(a, from_b) & rook_attacks(b, from_a);
    }
    if (bishop_attacks(a, Bitboard::EMPTY) & from_b) != Bitboard::EMPTY {
        return bishop_attacks(a, from_b) & bishop_attacks(b, from_a);
    }
    Bitboard::EMPTY
}

/// The whole line through `a` and `b`, both included, edge to edge, when they share a rank,
/// file or diagonal; empty otherwise.
fn line(a: Square, b: Square) -> Bitboard {
    let (from_a, from_b) = (Bitboard::from_square(a), Bitboard::from_square(b));
    if (rook_attacks(a, Bitboard::EMPTY) & from_b) != Bitboard::EMPTY {
        return (rook_attacks(a, Bitboard::EMPTY) & rook_attacks(b, Bitboard::EMPTY)) | from_a | from_b;
    }
    if (bishop_attacks(a, Bitboard::EMPTY) & from_b) != Bitboard::EMPTY {
        return (bishop_attacks(a, Bitboard::EMPTY) & bishop_attacks(b, Bitboard::EMPTY)) | from_a | from_b;
    }
    Bitboard::EMPTY
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::square::Square;

    /// Legal the plain way: made, tested for check, unmade.
    fn is_legal_by_making(board: &Board, mv: Move) -> bool {
        let mut after = *board;
        after.make_move(mv);
        !is_in_check(&after, board.side_to_move())
    }

    /// Every pseudo-legal move of `board`'s side to move judged alike by `Legality` and by making it.
    fn assert_legality_agrees(board: &Board, context: &str) {
        let color = board.side_to_move();
        let mut moves = Vec::new();
        pseudo_legal_moves(board, color, &mut moves);
        let legality = Legality::new(board, color);
        for mv in moves {
            assert_eq!(
                legality.is_legal(board, mv),
                is_legal_by_making(board, mv),
                "{context}: {} to {}",
                mv.from() as usize,
                mv.to() as usize
            );
        }
    }

    #[test]
    fn legality_equals_making_the_move_on_hand_picked_positions() {
        let fens = [
            // En passant that would open the fifth rank onto the king.
            "8/8/8/K1pP3r/8/8/8/7k w - c6 0 1",
            // A pinned bishop along its diagonal and a pinned knight; a pinned rook taking its pinner.
            "4k3/8/8/1b6/8/3B4/4N3/r1R1K2q w - - 0 1",
            // A king stepping back along the checking rook's line.
            "4r1k1/8/8/8/8/8/8/4K3 w - - 0 1",
            // Double check: knight and rook.
            "4r1k1/8/8/8/8/3n4/8/4K2R w K - 0 1",
            // Single check by a knight: capture it or move the king, no block.
            "6k1/8/8/8/8/3n4/2B5/4K3 w - - 0 1",
            // Check by a pawn that en passant can capture.
            "8/8/8/2k5/3Pp3/8/8/4K3 b - d3 0 1",
            // Kiwipete, both sides.
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        ];
        for fen in fens {
            let (board, _) = Board::from_fen(fen).unwrap();
            assert_legality_agrees(&board, fen);
        }
    }

    #[test]
    fn legality_equals_making_the_move_along_random_games() {
        // Random legal games from a few starts, every position checked: checks, pins, en passant
        // and castling all turn up along hundreds of plies.
        let starts = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        ];
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        for fen in starts {
            for game in 0..40 {
                let (mut board, _) = Board::from_fen(fen).unwrap();
                for ply in 0..120 {
                    assert_legality_agrees(&board, &format!("{fen}, game {game}, ply {ply}"));
                    let moves = legal_moves(&board, board.side_to_move());
                    if moves.is_empty() {
                        break;
                    }
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    board.make_move(moves[(seed >> 33) as usize % moves.len()]);
                }
            }
        }
    }

    #[test]
    fn pinned_piece_cannot_move_off_the_pin_line() {
        // White king e1, White rook e2 pinned by Black rook e8 along the e-file. The
        // pinned rook's own pseudo-legal moves include stepping sideways (e.g. to d2),
        // which would expose the king -- that one specific move must be filtered out,
        // while moves that stay on the pin line (e.g. e2 to e3, still blocking) remain.
        let (board, _) = Board::from_fen("4r3/8/8/8/8/8/4R3/4K3 w - - 0 1").unwrap();
        let legal = legal_moves(&board, Color::White);

        assert!(!legal.iter().any(|mv| mv.from() == Square::E2 && mv.to() == Square::D2));
        assert!(legal.iter().any(|mv| mv.from() == Square::E2 && mv.to() == Square::E3));
    }

    #[test]
    fn king_cannot_move_into_an_attacked_square() {
        // White king e1 (not currently in check -- the rook on f3 attacks neither e1's
        // rank nor file). Its pseudo-legal step to f1 or f2 would land on the f-file,
        // which the rook does attack, so both are illegal; stepping to d1 (off the
        // f-file entirely) remains legal.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/5r2/8/4K3 w - - 0 1").unwrap();
        let legal = legal_moves(&board, Color::White);

        assert!(!legal.iter().any(|mv| mv.from() == Square::E1 && mv.to() == Square::F1));
        assert!(!legal.iter().any(|mv| mv.from() == Square::E1 && mv.to() == Square::F2));
        assert!(legal.iter().any(|mv| mv.from() == Square::E1 && mv.to() == Square::D1));
    }

    #[test]
    fn only_check_resolving_moves_survive_when_already_in_check() {
        // White king e1, Black rook e8 gives check along the e-file. Nothing blocks or
        // captures it, so the only legal replies are king moves that step off the
        // e-file's attack entirely -- every survivor must actually resolve the check,
        // verified below by re-making each one and confirming it does.
        let (board, _) = Board::from_fen("4r3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let legal = legal_moves(&board, Color::White);

        // Every surviving move must actually resolve the check: making it must leave the
        // king safe. This re-derives the definition of legal_moves directly, as an
        // end-to-end sanity check rather than hand-picking individual destinations.
        for mv in &legal {
            let mut scratch = board;
            let undo = scratch.make_move(*mv);
            assert!(!is_in_check(&scratch, Color::White));
            scratch.unmake_move(*mv, undo);
        }
        assert!(!legal.is_empty());
    }

    #[test]
    fn quiet_position_has_no_illegal_pseudo_legal_moves() {
        // A position with no checks or pins in play: legal and pseudo-legal must agree
        // exactly, move for move.
        let (board, _) = Board::from_fen("4k3/8/8/8/3N4/8/8/4K3 w - - 0 1").unwrap();
        let mut legal = legal_moves(&board, Color::White);
        let mut pseudo_legal = Vec::new();
        pseudo_legal_moves(&board, Color::White, &mut pseudo_legal);

        legal.sort_by_key(|mv| (mv.from() as u8, mv.to() as u8));
        pseudo_legal.sort_by_key(|mv| (mv.from() as u8, mv.to() as u8));
        assert_eq!(legal, pseudo_legal);
    }
}
