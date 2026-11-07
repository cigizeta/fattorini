use num_enum::TryFromPrimitive;

/// The kind of a chess piece, independent of color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[repr(u8)]
pub enum PieceType {
    Pawn, Knight, Bishop, Rook, Queen, King,
}

impl PieceType {
    /// Every variant, in the same order they're declared in -- the one place this list is
    /// written down, for the handful of call sites that need to walk all six piece types
    /// (`Board::occupancy`, `eval.rs`) rather than each hand-writing its
    /// own copy that could quietly drift out of sync with the enum itself.
    pub const ALL: [PieceType; 6] =
        [PieceType::Pawn, PieceType::Knight, PieceType::Bishop, PieceType::Rook, PieceType::Queen, PieceType::King];
}
