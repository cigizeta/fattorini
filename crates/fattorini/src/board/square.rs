use super::file::File;
use super::rank::Rank;
use num_enum::TryFromPrimitive;

/// A square on the chessboard, numbered using the LERF (Little-Endian Rank-File)
/// convention: A1 = 0, B1 = 1, ..., H1 = 7, A2 = 8, ..., H8 = 63.
#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[repr(u8)]
pub enum Square {
    A1, B1, C1, D1, E1, F1, G1, H1,
    A2, B2, C2, D2, E2, F2, G2, H2,
    A3, B3, C3, D3, E3, F3, G3, H3,
    A4, B4, C4, D4, E4, F4, G4, H4,
    A5, B5, C5, D5, E5, F5, G5, H5,
    A6, B6, C6, D6, E6, F6, G6, H6,
    A7, B7, C7, D7, E7, F7, G7, H7,
    A8, B8, C8, D8, E8, F8, G8, H8,
}

impl Square {
    /// Combines a file and a rank into the corresponding square: `rank * 8 + file`, the
    /// same LERF numbering the enum's own variants already follow.
    pub fn new(file: File, rank: Rank) -> Square {
        Square::try_from(rank as u8 * 8 + file as u8).unwrap()
    }

    /// This square's file.
    pub fn file(self) -> File {
        File::try_from(self as u8 % 8).unwrap()
    }

    /// This square's rank.
    pub fn rank(self) -> Rank {
        Rank::try_from(self as u8 / 8).unwrap()
    }

    /// The square `file_delta` files and `rank_delta` ranks away from this one, or `None`
    /// if that lands off the board. The shared building block for every piece whose
    /// destinations are a fixed offset (or offsets) from its own square -- knight, king,
    /// and pawn moves are all built on this.
    pub fn try_offset(self, file_delta: i8, rank_delta: i8) -> Option<Square> {
        let file = self.file() as i8 + file_delta;
        let rank = self.rank() as i8 + rank_delta;

        if !(0i8..8).contains(&file) || !(0i8..8).contains(&rank) {
            return None;
        }

        Some(Square::new(File::try_from(file as u8).unwrap(), Rank::try_from(rank as u8).unwrap()))
    }

    /// Whether this square is a light square in the standard board coloring. A square's
    /// color is the parity of its file-plus-rank index; a bishop can never change this
    /// parity no matter how far it travels, since every diagonal step changes file and rank
    /// by the same amount, keeping their sum's parity fixed. a1 (file 0, rank 0) is dark, so
    /// light squares are exactly the ones where file+rank is odd.
    pub fn is_light_square(self) -> bool {
        (self.file() as u8 + self.rank() as u8) % 2 == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering_matches_lerf_convention() {
        assert_eq!(Square::A1 as u8, 0);
        assert_eq!(Square::H1 as u8, 7);
        assert_eq!(Square::A8 as u8, 56);
        assert_eq!(Square::H8 as u8, 63);
        assert_eq!(Square::E4 as u8, 28);
    }

    #[test]
    fn u8_round_trips_through_try_from() {
        for value in 0u8..64 {
            assert_eq!(Square::try_from(value).unwrap() as u8, value);
        }
    }

    #[test]
    fn out_of_range_u8_is_rejected() {
        assert!(Square::try_from(64u8).is_err());
        assert!(Square::try_from(255u8).is_err());
    }

    #[test]
    fn new_file_and_rank_round_trip() {
        for value in 0u8..64 {
            let square = Square::try_from(value).unwrap();
            assert_eq!(Square::new(square.file(), square.rank()), square);
        }
    }

    #[test]
    fn try_offset_stays_on_board() {
        assert_eq!(Square::E4.try_offset(1, 1), Some(Square::F5));
        assert_eq!(Square::E4.try_offset(-1, -1), Some(Square::D3));
        assert_eq!(Square::E4.try_offset(0, 0), Some(Square::E4));
    }

    #[test]
    fn try_offset_off_board_is_none() {
        assert_eq!(Square::A1.try_offset(-1, 0), None);
        assert_eq!(Square::A1.try_offset(0, -1), None);
        assert_eq!(Square::H8.try_offset(1, 0), None);
        assert_eq!(Square::H8.try_offset(0, 1), None);
    }

    #[test]
    fn a1_is_dark_and_its_neighbors_are_light() {
        // Standard board coloring, checked against the one square everyone can picture:
        // a1 is dark. Both squares a chess move away by one step -- not just the diagonal
        // (same-color) direction -- must come out light, or a same-axis-only bug (e.g.
        // checking file parity alone) would slip through.
        assert!(!Square::A1.is_light_square());
        assert!(Square::B1.is_light_square());
        assert!(Square::A2.is_light_square());
    }

    #[test]
    fn diagonal_travel_preserves_square_color() {
        // A bishop's whole diagonal, from corner to corner -- every square on it must agree.
        assert!(!Square::A1.is_light_square());
        assert!(!Square::D4.is_light_square());
        assert!(!Square::H8.is_light_square());
    }
}
