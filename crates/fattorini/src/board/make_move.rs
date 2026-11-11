use super::bitboard::Bitboard;
use super::castling_rights::{CastlingRights, CastlingSide};
use super::chess_move::{Move, MoveKind};
use super::color::Color;
use super::piece::Piece;
use super::piece_type::PieceType;
use super::rank::Rank;
use super::square::Square;
use super::zobrist;
use super::Board;

/// Everything `unmake_move` needs to reverse a `make_move` call that isn't recoverable
/// from the `Move` alone: what was captured (if anything), and what the board's other
/// state (castling rights, en passant square, halfmove clock) was immediately before the
/// move. Fields are private -- this is a pure opaque receipt, produced by `make_move` and
/// consumed only by `unmake_move`; nothing else ever needs to look inside one.
#[derive(Debug, Clone, Copy)]
pub struct UndoInfo {
    captured: Option<PieceType>,
    castling_rights: CastlingRights,
    en_passant_square: Option<Square>,
    halfmove_clock: u8,
}

/// The analogous opaque receipt for `make_null_move`/`unmake_null_move` -- a pass touches only
/// `en_passant_square` (`side_to_move` always just flips back, needing no stored value at all),
/// so this is smaller than `UndoInfo` by construction, not an arbitrary subset of it.
#[derive(Debug, Clone, Copy)]
pub struct NullUndoInfo {
    en_passant_square: Option<Square>,
}

/// The square a double-pushing pawn passed over -- exactly halfway between `from` and
/// `to` on the same file. Plain rank arithmetic, no color-dependent "which way is
/// forward" needed: a double push always covers exactly two ranks, so the midpoint is
/// always a whole rank either way.
fn en_passant_target(from: Square, to: Square) -> Square {
    let passed_rank = (from.rank() as u8 + to.rank() as u8) / 2;
    Square::new(from.file(), Rank::try_from(passed_rank).unwrap())
}

impl Board {
    /// Applies `mv` to this board, mutating it in place, and returns an `UndoInfo` that
    /// later reverses it via `unmake_move`. Assumes `mv` is at least pseudo-legal for the
    /// side to move -- it doesn't re-validate the move itself, only applies it.
    pub fn make_move(&mut self, mv: Move) -> UndoInfo {
        let mover_color = self.side_to_move;
        let moving_piece = self.mailbox[mv.from() as usize].expect("make_move: no piece on the from square");

        let captured_square = match mv.kind() {
            MoveKind::EnPassantCapture => Square::new(mv.to().file(), mv.from().rank()),
            _ => mv.to(),
        };
        let captured = self.remove_piece(captured_square).map(|piece| piece.piece_type());

        self.remove_piece(mv.from());
        let placed_piece = match mv.promotion() {
            Some(promotion_type) => Piece::new(mover_color, promotion_type),
            None => moving_piece,
        };
        self.place_piece(mv.to(), placed_piece);

        if mv.kind() == MoveKind::Castle {
            let squares = castling_side_of(mover_color, mv.to()).squares(mover_color);
            let rook = self.remove_piece(squares.rook_from).unwrap();
            self.place_piece(squares.rook_to, rook);
        }

        let previous_castling_rights = self.castling_rights;
        // XOR the whole old contribution out, then (after it's updated) the whole new one
        // in -- simpler than diffing which specific right actually flipped, and no less
        // correct: a right that didn't change gets XORed out and immediately back in, a
        // no-op either way.
        self.hash ^= zobrist::castling(previous_castling_rights);
        self.update_castling_rights(mover_color, moving_piece, mv.from(), captured_square, captured);
        self.hash ^= zobrist::castling(self.castling_rights);

        let previous_en_passant_square = self.en_passant_square;
        self.hash ^= zobrist::en_passant(previous_en_passant_square);
        self.en_passant_square = match mv.kind() {
            MoveKind::DoublePawnPush => Some(en_passant_target(mv.from(), mv.to())),
            _ => None,
        };
        self.hash ^= zobrist::en_passant(self.en_passant_square);

        let previous_halfmove_clock = self.halfmove_clock;
        self.halfmove_clock =
            if moving_piece.piece_type() == PieceType::Pawn || captured.is_some() { 0 } else { self.halfmove_clock + 1 };

        self.side_to_move = mover_color.opposite();
        // Side to move flips every ply, so this single XOR is the whole side-to-move
        // update -- no before/after comparison needed the way castling/en-passant have one.
        self.hash ^= zobrist::side_to_move();

        UndoInfo {
            captured,
            castling_rights: previous_castling_rights,
            en_passant_square: previous_en_passant_square,
            halfmove_clock: previous_halfmove_clock,
        }
    }

    /// Reverses a `make_move(mv)` call, given the `UndoInfo` it returned. Must be called
    /// with the exact same `mv` -- `Move` alone doesn't carry enough to reverse itself
    /// (deliberately, per `Move`'s own docs); that's exactly what `undo` supplies.
    pub fn unmake_move(&mut self, mv: Move, undo: UndoInfo) {
        self.side_to_move = self.side_to_move.opposite();
        self.hash ^= zobrist::side_to_move();
        let mover_color = self.side_to_move;

        if mv.kind() == MoveKind::Castle {
            let squares = castling_side_of(mover_color, mv.to()).squares(mover_color);
            let rook = self.remove_piece(squares.rook_to).unwrap();
            self.place_piece(squares.rook_from, rook);
        }

        let moved_piece = self.remove_piece(mv.to()).unwrap();
        let original_piece = match mv.promotion() {
            Some(_) => Piece::new(mover_color, PieceType::Pawn),
            None => moved_piece,
        };
        self.place_piece(mv.from(), original_piece);

        if let Some(captured_type) = undo.captured {
            let captured_square = match mv.kind() {
                MoveKind::EnPassantCapture => Square::new(mv.to().file(), mv.from().rank()),
                _ => mv.to(),
            };
            self.place_piece(captured_square, Piece::new(mover_color.opposite(), captured_type));
        }

        self.hash ^= zobrist::castling(self.castling_rights);
        self.castling_rights = undo.castling_rights;
        self.hash ^= zobrist::castling(self.castling_rights);

        self.hash ^= zobrist::en_passant(self.en_passant_square);
        self.en_passant_square = undo.en_passant_square;
        self.hash ^= zobrist::en_passant(self.en_passant_square);

        self.halfmove_clock = undo.halfmove_clock;
    }

    /// "Passing" -- flips the side to move without moving a piece, for null-move pruning's own
    /// use (search.rs's `Search::null_move_in_progress`, not this file). Unrelated to `uci.rs`'s
    /// own, entirely different sense of "null move" (`bestmove 0000`, meaning "no legal move
    /// exists") -- same English phrase, two unrelated concepts kept apart by which module they
    /// live in. Touches only what a pass actually changes: `side_to_move` and
    /// `en_passant_square` (a pass cannot itself be captured en passant on the very next move,
    /// so any pending square is cleared exactly like an ordinary non-double-push move already
    /// clears it in `make_move`). Castling rights and the halfmove clock are untouched -- a pass
    /// invalidates neither.
    pub fn make_null_move(&mut self) -> NullUndoInfo {
        let previous_en_passant_square = self.en_passant_square;
        self.hash ^= zobrist::en_passant(previous_en_passant_square);
        self.en_passant_square = None;
        self.hash ^= zobrist::en_passant(self.en_passant_square);

        self.side_to_move = self.side_to_move.opposite();
        self.hash ^= zobrist::side_to_move();

        NullUndoInfo { en_passant_square: previous_en_passant_square }
    }

    /// Reverses a `make_null_move` call, given the `NullUndoInfo` it returned.
    pub fn unmake_null_move(&mut self, undo: NullUndoInfo) {
        self.side_to_move = self.side_to_move.opposite();
        self.hash ^= zobrist::side_to_move();

        self.hash ^= zobrist::en_passant(self.en_passant_square);
        self.en_passant_square = undo.en_passant_square;
        self.hash ^= zobrist::en_passant(self.en_passant_square);
    }

    /// Clears whichever castling rights this move invalidates: the king moving clears
    /// both of its color's rights; a rook moving away from, or being captured on, its own
    /// home square clears just that one side's right. Rights that are already clear are
    /// simply cleared again -- harmless.
    fn update_castling_rights(
        &mut self,
        mover_color: Color,
        moving_piece: Piece,
        move_from: Square,
        captured_square: Square,
        captured: Option<PieceType>,
    ) {
        if moving_piece.piece_type() == PieceType::King {
            self.castling_rights.set_kingside(mover_color, false);
            self.castling_rights.set_queenside(mover_color, false);
        }

        if moving_piece.piece_type() == PieceType::Rook {
            if move_from == CastlingSide::Kingside.squares(mover_color).rook_from {
                self.castling_rights.set_kingside(mover_color, false);
            } else if move_from == CastlingSide::Queenside.squares(mover_color).rook_from {
                self.castling_rights.set_queenside(mover_color, false);
            }
        }

        if captured == Some(PieceType::Rook) {
            let opponent = mover_color.opposite();
            if captured_square == CastlingSide::Kingside.squares(opponent).rook_from {
                self.castling_rights.set_kingside(opponent, false);
            } else if captured_square == CastlingSide::Queenside.squares(opponent).rook_from {
                self.castling_rights.set_queenside(opponent, false);
            }
        }
    }

    /// Places `piece` on `square`, keeping `pieces` and `mailbox` in sync -- this is where
    /// the sync the `mailbox` field's own doc comment promises actually happens.
    fn place_piece(&mut self, square: Square, piece: Piece) {
        self.pieces[piece as usize] = self.pieces[piece as usize] | Bitboard::from_square(square);
        self.mailbox[square as usize] = Some(piece);
        self.hash ^= zobrist::piece_square(piece, square);
        self.correction_keys[zobrist::correction_key(piece)] ^= zobrist::piece_square(piece, square);
    }

    /// Removes whatever's on `square` (if anything), keeping `pieces` and `mailbox` in
    /// sync, and returns what was removed.
    fn remove_piece(&mut self, square: Square) -> Option<Piece> {
        let piece = self.mailbox[square as usize]?;
        self.pieces[piece as usize] = self.pieces[piece as usize] & !Bitboard::from_square(square);
        self.mailbox[square as usize] = None;
        self.hash ^= zobrist::piece_square(piece, square);
        self.correction_keys[zobrist::correction_key(piece)] ^= zobrist::piece_square(piece, square);
        Some(piece)
    }
}

/// Which side a castling move's king destination `king_to` belongs to, for `color`.
/// `king_to` is always one of the two squares `CastlingSide::Kingside`/`Queenside` give
/// for `color`, since it came from a `Move` built by `Move::castle` -- comparing against
/// the kingside destination is enough to tell the two apart.
fn castling_side_of(color: Color, king_to: Square) -> CastlingSide {
    if king_to == CastlingSide::Kingside.squares(color).king_to {
        return CastlingSide::Kingside;
    }
    CastlingSide::Queenside
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every legal move and its reply, from positions holding every kind (castling both ways, en
    /// passant, promotions with and without a capture): the correction keys made incrementally
    /// equal the ones built from scratch, and unmaking restores them.
    #[test]
    fn the_correction_keys_follow_every_kind_of_move() {
        use crate::movegen::legal::legal_moves;
        for fen in [
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "n1n5/PPPk4/8/8/8/8/4Kppp/5N1N b - - 0 1",
            "8/8/8/3pP3/8/8/8/4K2k w - d6 0 1",
        ] {
            let (mut board, _) = Board::from_fen(fen).unwrap();
            let original = zobrist::compute_correction_keys(&board);
            assert_eq!(board.correction_keys, original, "{fen}: from the FEN");
            for mv in legal_moves(&board, board.side_to_move()) {
                let undo = board.make_move(mv);
                assert_eq!(board.correction_keys, zobrist::compute_correction_keys(&board), "{fen}: {mv:?}");
                for reply in legal_moves(&board, board.side_to_move()) {
                    let reply_undo = board.make_move(reply);
                    assert_eq!(board.correction_keys, zobrist::compute_correction_keys(&board), "{fen}: {mv:?} {reply:?}");
                    board.unmake_move(reply, reply_undo);
                }
                board.unmake_move(mv, undo);
                assert_eq!(board.correction_keys, original, "{fen}: {mv:?} unmade");
            }
        }
    }

    #[test]
    fn quiet_move_round_trips_and_increments_halfmove_clock() {
        let (mut board, _) = Board::from_fen("8/8/8/8/8/8/8/N7 w - - 5 1").unwrap();
        let original_fen = board.to_fen(1);
        let original_hash = board.hash();

        let undo = board.make_move(Move::new(Square::A1, Square::B3, None));
        assert_eq!(board.to_fen(1), "8/8/8/8/8/1N6/8/8 b - - 6 1");
        assert_eq!(board.hash(), zobrist::compute(&board));

        board.unmake_move(Move::new(Square::A1, Square::B3, None), undo);
        assert_eq!(board.to_fen(1), original_fen);
        assert_eq!(board.hash(), original_hash);
    }

    #[test]
    fn capture_round_trips_and_resets_halfmove_clock() {
        let (mut board, _) = Board::from_fen("8/8/8/8/8/8/2p5/N7 w - - 5 1").unwrap();
        let original_fen = board.to_fen(1);
        let original_hash = board.hash();

        let mv = Move::new(Square::A1, Square::C2, None);
        let undo = board.make_move(mv);
        assert_eq!(board.to_fen(1), "8/8/8/8/8/8/2N5/8 b - - 0 1");
        assert_eq!(board.hash(), zobrist::compute(&board));

        board.unmake_move(mv, undo);
        assert_eq!(board.to_fen(1), original_fen);
        assert_eq!(board.hash(), original_hash);
    }

    #[test]
    fn en_passant_capture_round_trips() {
        let (mut board, _) = Board::from_fen("8/8/8/3pP3/8/8/8/8 w - d6 0 1").unwrap();
        let original_fen = board.to_fen(1);
        let original_hash = board.hash();

        let mv = Move::en_passant_capture(Square::E5, Square::D6);
        let undo = board.make_move(mv);
        assert_eq!(board.to_fen(1), "8/8/3P4/8/8/8/8/8 b - - 0 1");
        assert_eq!(board.hash(), zobrist::compute(&board));

        board.unmake_move(mv, undo);
        assert_eq!(board.to_fen(1), original_fen);
        assert_eq!(board.hash(), original_hash);
    }

    #[test]
    fn promotion_round_trips() {
        let (mut board, _) = Board::from_fen("8/4P3/8/8/8/8/8/8 w - - 0 1").unwrap();
        let original_fen = board.to_fen(1);
        let original_hash = board.hash();

        let mv = Move::new(Square::E7, Square::E8, Some(PieceType::Queen));
        let undo = board.make_move(mv);
        assert_eq!(board.to_fen(1), "4Q3/8/8/8/8/8/8/8 b - - 0 1");
        assert_eq!(board.hash(), zobrist::compute(&board));

        board.unmake_move(mv, undo);
        assert_eq!(board.to_fen(1), original_fen);
        assert_eq!(board.hash(), original_hash);
    }

    #[test]
    fn castling_round_trips_and_clears_the_moved_right() {
        let (mut board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let original_fen = board.to_fen(1);
        let original_hash = board.hash();

        let mv = Move::castle(Square::E1, Square::G1);
        let undo = board.make_move(mv);
        assert_eq!(board.to_fen(1), "4k3/8/8/8/8/8/8/5RK1 b - - 1 1");
        assert_eq!(board.hash(), zobrist::compute(&board));

        board.unmake_move(mv, undo);
        assert_eq!(board.to_fen(1), original_fen);
        assert_eq!(board.hash(), original_hash);
    }

    #[test]
    fn king_move_clears_both_of_its_own_rights() {
        let (mut board, _) = Board::from_fen("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        board.make_move(Move::new(Square::E1, Square::E2, None));
        assert!(!board.castling_rights().kingside(Color::White));
        assert!(!board.castling_rights().queenside(Color::White));
        assert_eq!(board.hash(), zobrist::compute(&board));
    }

    #[test]
    fn rook_move_clears_only_that_sides_right() {
        let (mut board, _) = Board::from_fen("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        board.make_move(Move::new(Square::H1, Square::G1, None));
        assert!(!board.castling_rights().kingside(Color::White));
        assert!(board.castling_rights().queenside(Color::White));
        assert_eq!(board.hash(), zobrist::compute(&board));
    }

    #[test]
    fn capturing_a_rook_on_its_home_square_clears_the_opponents_right() {
        // White rook a1 captures Black's still-unmoved rook on a8 -- clears both White's
        // own queenside right (its rook left a1) and Black's queenside right (its rook
        // was captured on a8), in the same move.
        let (mut board, _) = Board::from_fen("r3k3/8/8/8/8/8/8/R3K3 w Qq - 0 1").unwrap();
        board.make_move(Move::new(Square::A1, Square::A8, None));
        assert!(!board.castling_rights().queenside(Color::White));
        assert!(!board.castling_rights().queenside(Color::Black));
        assert_eq!(board.hash(), zobrist::compute(&board));
    }

    #[test]
    fn double_push_sets_en_passant_square_and_it_round_trips() {
        let (mut board, _) = Board::from_fen("8/8/8/8/8/8/4P3/8 w - - 0 1").unwrap();
        let original_fen = board.to_fen(1);
        let original_hash = board.hash();

        let mv = Move::double_pawn_push(Square::E2, Square::E4);
        let undo = board.make_move(mv);
        assert_eq!(board.en_passant_square(), Some(Square::E3));
        assert_eq!(board.hash(), zobrist::compute(&board));

        board.unmake_move(mv, undo);
        assert_eq!(board.to_fen(1), original_fen);
        assert_eq!(board.hash(), original_hash);
    }

    #[test]
    fn en_passant_square_does_not_survive_an_unrelated_move() {
        let (mut board, _) = Board::from_fen("8/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        board.make_move(Move::new(Square::E1, Square::E2, None));
        assert_eq!(board.en_passant_square(), None);
        assert_eq!(board.hash(), zobrist::compute(&board));
    }

    #[test]
    fn null_move_round_trips_flipping_only_the_side_to_move() {
        let (mut board, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 5 1").unwrap();
        let original_fen = board.to_fen(1);
        let original_hash = board.hash();

        let undo = board.make_null_move();
        assert_eq!(board.to_fen(1), "4k3/8/8/8/8/8/8/4K3 b - - 5 1");
        assert_eq!(board.hash(), zobrist::compute(&board));

        board.unmake_null_move(undo);
        assert_eq!(board.to_fen(1), original_fen);
        assert_eq!(board.hash(), original_hash);
    }

    #[test]
    fn null_move_clears_a_pending_en_passant_square_and_it_round_trips() {
        let (mut board, _) = Board::from_fen("8/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        let original_hash = board.hash();

        let undo = board.make_null_move();
        assert_eq!(board.en_passant_square(), None);
        assert_eq!(board.hash(), zobrist::compute(&board));

        board.unmake_null_move(undo);
        assert_eq!(board.en_passant_square(), Some(Square::D6));
        assert_eq!(board.hash(), original_hash);
    }
}
