use super::piece_type::PieceType;
use super::square::Square;

/// A single move: a piece traveling from one square to another, plus which piece to
/// promote into if this move reaches the last rank, plus what special rule (if any) it
/// represents. Fields are private -- everything goes through `new`/the dedicated
/// constructors/accessor methods -- so the internal representation can change later (e.g.
/// to a packed encoding, if profiling ever justifies it) without any caller noticing, the
/// same reasoning already used for `Board`'s on-demand-derived occupancy.
///
/// `Move` is a value type: every field is a plain fact about the move itself, decided
/// once at construction, never something that depends on the board it came from or needs
/// the board consulted again to make sense of. Facts about the *position* a move was made
/// from instead -- which piece an ordinary capture removed, what the en passant square or
/// castling rights were beforehand -- deliberately aren't here; they belong on a separate
/// undo/history record instead (see `board::make_move::UndoInfo`), not on `Move`.
///
/// Lives in `board`, not `movegen`, even though `movegen` is what builds `Move`s --
/// applying a move to a `Board` (`make_move`/`unmake_move`) needs this type too, and
/// `board` is the lower layer `movegen` itself depends on, never the reverse. A move is
/// as fundamental a chess concept as a square or a piece, so it belongs at that same
/// layer, distinct from the piece-specific generation logic that produces candidates for
/// it (which does stay in `movegen`).
///
/// (Named `chess_move.rs`, not `move.rs`, since `move` is a reserved keyword in Rust and
/// can't be a module name -- the type itself is still called `Move`.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    from: Square,
    to: Square,
    promotion: Option<PieceType>,
    kind: MoveKind,
}

/// What makes a move special, beyond an ordinary change of square. An ordinary capture
/// isn't its own variant -- whether the destination holds an enemy piece is discoverable
/// from the board at the time, not a fact that needs storing on the move itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveKind {
    /// No special rule applies -- includes ordinary captures.
    Quiet,
    /// A pawn moving two squares from its own starting rank in one move.
    DoublePawnPush,
    /// A pawn capturing an enemy pawn that just double-pushed, landing on the square the
    /// enemy pawn passed over rather than the square it's actually on.
    EnPassantCapture,
    /// Castling: the king moves two squares toward a rook, and that rook jumps to the
    /// square the king crossed. No separate kingside/queenside variant -- the king's own
    /// `to` square already disambiguates (e.g. e1->g1 vs e1->c1), same reasoning as not
    /// having a separate `Capture` variant. The rook's own from/to aren't stored here
    /// either -- they're implied by the king's destination, and moving the rook is a
    /// make/unmake concern, not something `Move` itself needs to carry.
    Castle,
}

impl Move {
    /// Builds an ordinary move from `from` to `to`, promoting to `promotion` if this move
    /// reaches the last rank (only ever `Some` for a pawn move -- knights and kings never
    /// promote -- but the field lives on every `Move`, not just pawn moves specifically).
    /// `kind` defaults to `MoveKind::Quiet` -- use `double_pawn_push`/`en_passant_capture`
    /// instead for those two special cases.
    pub fn new(from: Square, to: Square, promotion: Option<PieceType>) -> Move {
        Move { from, to, promotion, kind: MoveKind::Quiet }
    }

    /// Builds a double pawn push from `from` to `to`. Never a promotion -- a pawn that
    /// just left its own starting rank can't simultaneously reach the last rank.
    pub fn double_pawn_push(from: Square, to: Square) -> Move {
        Move { from, to, promotion: None, kind: MoveKind::DoublePawnPush }
    }

    /// Builds an en passant capture from `from` to `to`. Never a promotion, for the same
    /// reason as `double_pawn_push`.
    pub fn en_passant_capture(from: Square, to: Square) -> Move {
        Move { from, to, promotion: None, kind: MoveKind::EnPassantCapture }
    }

    /// Builds a castling move: the king going from `from` to `to`. Never a promotion --
    /// a king can't promote.
    pub fn castle(from: Square, to: Square) -> Move {
        Move { from, to, promotion: None, kind: MoveKind::Castle }
    }

    /// The square this move starts from.
    pub fn from(self) -> Square {
        self.from
    }

    /// The square this move ends on.
    pub fn to(self) -> Square {
        self.to
    }

    /// The piece this move promotes to, or `None` if it isn't a promotion.
    pub fn promotion(self) -> Option<PieceType> {
        self.promotion
    }

    /// What kind of special rule, if any, this move represents.
    pub fn kind(self) -> MoveKind {
        self.kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_round_trips_through_accessors() {
        let mv = Move::new(Square::E2, Square::E4, None);
        assert_eq!(mv.from(), Square::E2);
        assert_eq!(mv.to(), Square::E4);
        assert_eq!(mv.promotion(), None);
        assert_eq!(mv.kind(), MoveKind::Quiet);
    }

    #[test]
    fn promotion_round_trips_through_accessors() {
        let mv = Move::new(Square::E7, Square::E8, Some(PieceType::Queen));
        assert_eq!(mv.from(), Square::E7);
        assert_eq!(mv.to(), Square::E8);
        assert_eq!(mv.promotion(), Some(PieceType::Queen));
        assert_eq!(mv.kind(), MoveKind::Quiet);
    }

    #[test]
    fn double_pawn_push_round_trips_through_accessors() {
        let mv = Move::double_pawn_push(Square::E2, Square::E4);
        assert_eq!(mv.from(), Square::E2);
        assert_eq!(mv.to(), Square::E4);
        assert_eq!(mv.promotion(), None);
        assert_eq!(mv.kind(), MoveKind::DoublePawnPush);
    }

    #[test]
    fn en_passant_capture_round_trips_through_accessors() {
        let mv = Move::en_passant_capture(Square::D5, Square::E6);
        assert_eq!(mv.from(), Square::D5);
        assert_eq!(mv.to(), Square::E6);
        assert_eq!(mv.promotion(), None);
        assert_eq!(mv.kind(), MoveKind::EnPassantCapture);
    }

    #[test]
    fn castle_round_trips_through_accessors() {
        let mv = Move::castle(Square::E1, Square::G1);
        assert_eq!(mv.from(), Square::E1);
        assert_eq!(mv.to(), Square::G1);
        assert_eq!(mv.promotion(), None);
        assert_eq!(mv.kind(), MoveKind::Castle);
    }
}
