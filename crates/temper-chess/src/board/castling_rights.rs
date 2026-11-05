use super::color::Color;
use super::square::Square;

/// The four castling rights (White/Black x kingside/queenside), packed into the low 4
/// bits of a single byte -- xxxxBBBB, top 4 bits unused. Bit order matches FEN's own
/// castling field, "KQkq": bit 0 = White kingside, bit 1 = White queenside, bit 2 = Black
/// kingside, bit 3 = Black queenside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastlingRights(u8);

impl CastlingRights {
    /// No castling rights at all -- all four flags clear.
    pub const NONE: CastlingRights = CastlingRights(0);

    /// Whether `color` still has its kingside castling right.
    pub fn kingside(self, color: Color) -> bool {
        self.0 & (1 << Self::kingside_bit(color)) != 0
    }

    /// Whether `color` still has its queenside castling right.
    pub fn queenside(self, color: Color) -> bool {
        self.0 & (1 << Self::queenside_bit(color)) != 0
    }

    /// Sets or clears `color`'s kingside castling right.
    pub fn set_kingside(&mut self, color: Color, value: bool) {
        Self::set_bit(&mut self.0, Self::kingside_bit(color), value);
    }

    /// Sets or clears `color`'s queenside castling right.
    pub fn set_queenside(&mut self, color: Color, value: bool) {
        Self::set_bit(&mut self.0, Self::queenside_bit(color), value);
    }

    /// White's kingside right is bit 0, Black's is bit 2 -- `color * 2`, same
    /// "numbering does the work" pattern used by `Piece`'s `color * 6 + piece_type`.
    fn kingside_bit(color: Color) -> u8 {
        color as u8 * 2
    }

    /// The queenside right sits one bit above its color's kingside right.
    fn queenside_bit(color: Color) -> u8 {
        Self::kingside_bit(color) + 1
    }

    fn set_bit(byte: &mut u8, bit: u8, value: bool) {
        if value {
            *byte |= 1 << bit;
        } else {
            *byte &= !(1 << bit);
        }
    }
}

/// One of the two directions a color can castle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastlingSide {
    Kingside,
    Queenside,
}

/// The king's and rook's start/end squares for one color castling to one side -- pure
/// board geometry (these squares never change, no matter the position), shared by move
/// generation (which needs the king's path and the squares that must be empty) and by
/// move application (which needs to actually relocate the rook).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastlingSquares {
    pub king_from: Square,
    pub king_to: Square,
    pub rook_from: Square,
    pub rook_to: Square,
}

impl CastlingSide {
    /// `color`'s king/rook squares for castling to this side.
    pub fn squares(self, color: Color) -> CastlingSquares {
        match (self, color) {
            (CastlingSide::Kingside, Color::White) => {
                CastlingSquares { king_from: Square::E1, king_to: Square::G1, rook_from: Square::H1, rook_to: Square::F1 }
            }
            (CastlingSide::Kingside, Color::Black) => {
                CastlingSquares { king_from: Square::E8, king_to: Square::G8, rook_from: Square::H8, rook_to: Square::F8 }
            }
            (CastlingSide::Queenside, Color::White) => {
                CastlingSquares { king_from: Square::E1, king_to: Square::C1, rook_from: Square::A1, rook_to: Square::D1 }
            }
            (CastlingSide::Queenside, Color::Black) => {
                CastlingSquares { king_from: Square::E8, king_to: Square::C8, rook_from: Square::A8, rook_to: Square::D8 }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_layout_matches_fen_order() {
        assert!(CastlingRights(0b0001).kingside(Color::White));
        assert!(CastlingRights(0b0010).queenside(Color::White));
        assert!(CastlingRights(0b0100).kingside(Color::Black));
        assert!(CastlingRights(0b1000).queenside(Color::Black));
    }

    #[test]
    fn each_flag_is_independent() {
        let rights = CastlingRights(0b0001);
        assert!(rights.kingside(Color::White));
        assert!(!rights.queenside(Color::White));
        assert!(!rights.kingside(Color::Black));
        assert!(!rights.queenside(Color::Black));
    }

    #[test]
    fn set_and_clear_round_trip() {
        let mut rights = CastlingRights(0);
        rights.set_kingside(Color::White, true);
        assert!(rights.kingside(Color::White));
        rights.set_kingside(Color::White, false);
        assert!(!rights.kingside(Color::White));

        rights.set_queenside(Color::Black, true);
        assert!(rights.queenside(Color::Black));
        // setting one flag doesn't disturb the others
        assert!(!rights.kingside(Color::White));
        assert!(!rights.queenside(Color::White));
        assert!(!rights.kingside(Color::Black));
    }

    #[test]
    fn kingside_squares_are_correct_for_both_colors() {
        let white = CastlingSide::Kingside.squares(Color::White);
        assert_eq!(white, CastlingSquares { king_from: Square::E1, king_to: Square::G1, rook_from: Square::H1, rook_to: Square::F1 });

        let black = CastlingSide::Kingside.squares(Color::Black);
        assert_eq!(black, CastlingSquares { king_from: Square::E8, king_to: Square::G8, rook_from: Square::H8, rook_to: Square::F8 });
    }

    #[test]
    fn queenside_squares_are_correct_for_both_colors() {
        let white = CastlingSide::Queenside.squares(Color::White);
        assert_eq!(white, CastlingSquares { king_from: Square::E1, king_to: Square::C1, rook_from: Square::A1, rook_to: Square::D1 });

        let black = CastlingSide::Queenside.squares(Color::Black);
        assert_eq!(black, CastlingSquares { king_from: Square::E8, king_to: Square::C8, rook_from: Square::A8, rook_to: Square::D8 });
    }
}
