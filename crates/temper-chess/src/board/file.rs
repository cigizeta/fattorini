use num_enum::TryFromPrimitive;

/// A file (column) on the chessboard, A through H, numbered 0 through 7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[repr(u8)]
pub enum File {
    A, B, C, D, E, F, G, H,
}

impl File {
    /// Parses a lowercase file letter ('a'..'h'), as used in FEN and algebraic notation.
    pub fn from_char(c: char) -> Option<File> {
        match c {
            'a' => Some(File::A),
            'b' => Some(File::B),
            'c' => Some(File::C),
            'd' => Some(File::D),
            'e' => Some(File::E),
            'f' => Some(File::F),
            'g' => Some(File::G),
            'h' => Some(File::H),
            _ => None,
        }
    }

    /// The lowercase file letter for this file.
    pub fn to_char(self) -> char {
        match self {
            File::A => 'a',
            File::B => 'b',
            File::C => 'c',
            File::D => 'd',
            File::E => 'e',
            File::F => 'f',
            File::G => 'g',
            File::H => 'h',
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_round_trips_through_every_file() {
        for value in 0u8..8 {
            let file = File::try_from(value).unwrap();
            assert_eq!(File::from_char(file.to_char()), Some(file));
        }
    }

    #[test]
    fn invalid_char_is_rejected() {
        assert_eq!(File::from_char('z'), None);
        assert_eq!(File::from_char('A'), None);
    }
}
