use super::color::Color;
use super::piece_type::PieceType;
use num_enum::TryFromPrimitive;

/// A chess piece: a color and a kind, combined into one type. Listing white pieces then
/// black pieces gives `White* = 0..5` and `Black* = 6..11` -- `color * 6 + piece_type`
/// -- purely from Rust's default sequential numbering, with no explicit discriminant
/// values needed anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[repr(u8)]
pub enum Piece {
    WhitePawn, WhiteKnight, WhiteBishop, WhiteRook, WhiteQueen, WhiteKing,
    BlackPawn, BlackKnight, BlackBishop, BlackRook, BlackQueen, BlackKing,
}

impl Piece {
    /// Combines a color and a kind into the corresponding piece.
    pub fn new(color: Color, piece_type: PieceType) -> Piece {
        Piece::try_from(color as u8 * 6 + piece_type as u8).unwrap()
    }

    /// The kind of this piece, independent of color.
    pub fn piece_type(self) -> PieceType {
        PieceType::try_from(self as u8 % 6).unwrap()
    }

    /// This piece's color -- the inverse of `piece_type`, splitting `new`'s
    /// `color * 6 + piece_type` numbering back into its other half.
    pub fn color(self) -> Color {
        Color::try_from(self as u8 / 6).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_color_and_piece_type_round_trip() {
        for color_value in 0u8..2 {
            let color = Color::try_from(color_value).unwrap();
            for piece_type_value in 0u8..6 {
                let piece_type = PieceType::try_from(piece_type_value).unwrap();
                let piece = Piece::new(color, piece_type);
                assert_eq!(piece.color(), color);
                assert_eq!(piece.piece_type(), piece_type);
            }
        }
    }
}
