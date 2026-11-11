use num_enum::TryFromPrimitive;

/// A rank (row) on the chessboard, 1 through 8, numbered 0 through 7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[repr(u8)]
pub enum Rank {
    R1, R2, R3, R4, R5, R6, R7, R8,
}

impl Rank {
    /// Parses a rank digit ('1'..'8'), as used in FEN and algebraic notation.
    pub fn from_char(c: char) -> Option<Rank> {
        match c {
            '1' => Some(Rank::R1),
            '2' => Some(Rank::R2),
            '3' => Some(Rank::R3),
            '4' => Some(Rank::R4),
            '5' => Some(Rank::R5),
            '6' => Some(Rank::R6),
            '7' => Some(Rank::R7),
            '8' => Some(Rank::R8),
            _ => None,
        }
    }

    /// The rank digit for this rank.
    pub fn to_char(self) -> char {
        match self {
            Rank::R1 => '1',
            Rank::R2 => '2',
            Rank::R3 => '3',
            Rank::R4 => '4',
            Rank::R5 => '5',
            Rank::R6 => '6',
            Rank::R7 => '7',
            Rank::R8 => '8',
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_round_trips_through_every_rank() {
        for value in 0u8..8 {
            let rank = Rank::try_from(value).unwrap();
            assert_eq!(Rank::from_char(rank.to_char()), Some(rank));
        }
    }

    #[test]
    fn invalid_char_is_rejected() {
        assert_eq!(Rank::from_char('9'), None);
        assert_eq!(Rank::from_char('a'), None);
    }
}
