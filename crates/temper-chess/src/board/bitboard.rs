use super::square::Square;
use std::ops::{BitAnd, BitOr, BitXor, Not, Shl, Shr};

/// A set of squares, one bit per square, in the same LERF order as `Square` (bit n = the
/// square with index n). This is the primary representation for piece placement and for
/// the intermediate values (occupancy, attacks, ray masks) move generation computes from
/// it -- OR to combine squares, AND to intersect, XOR to toggle exactly two squares when a
/// piece moves, NOT to get the complementary (e.g. empty) squares, and shifts to slide an
/// entire set of squares by a fixed offset (e.g. one rank, for a pawn push).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bitboard(u64);

impl Bitboard {
    /// The empty bitboard: no squares set.
    pub const EMPTY: Bitboard = Bitboard(0);

    /// For every square, the squares one step of each `(file delta, rank delta)` in `offsets` away
    /// that are on the board: the knight's, king's and pawns' attack tables, computed at compile
    /// time. A plain `static` built from it is read with no check that it was initialised, where a
    /// `LazyLock` checked at every read (`Once::call_once_force`, about 1.5% of a labelling search,
    /// 2026-10-12).
    pub const fn step_attack_table(offsets: &[(i8, i8)]) -> [Bitboard; 64] {
        let mut table = [Bitboard::EMPTY; 64];
        let mut square = 0;
        while square < 64 {
            let (file, rank) = ((square % 8) as i8, (square / 8) as i8);
            let mut bits = 0u64;
            let mut k = 0;
            while k < offsets.len() {
                let (target_file, target_rank) = (file + offsets[k].0, rank + offsets[k].1);
                if target_file >= 0 && target_file < 8 && target_rank >= 0 && target_rank < 8 {
                    bits |= 1u64 << (target_rank * 8 + target_file);
                }
                k += 1;
            }
            table[square] = Bitboard(bits);
            square += 1;
        }
        table
    }

    /// The bitboard whose set squares are `bits`'s set bits: constant masks such as a rank or a file.
    pub const fn from_bits(bits: u64) -> Bitboard {
        Bitboard(bits)
    }

    /// The bitboard with only `square`'s bit set.
    pub fn from_square(square: Square) -> Bitboard {
        Bitboard(1u64 << square as u8)
    }

    /// Iterates this bitboard's set squares, lowest square index first, via the standard
    /// bitscan-and-clear-lowest-bit technique: `trailing_zeros()` reads the lowest set
    /// bit's position directly (a single CPU instruction on most hardware), then
    /// `value & (value - 1)` clears exactly that bit (subtracting 1 flips the lowest set
    /// bit and every zero bit below it; ANDing with the original keeps everything else
    /// unchanged) -- repeating until no bits remain. Every future piece type's move
    /// generation needs exactly this, so it lives here once rather than being
    /// reimplemented per piece.
    pub fn squares(self) -> impl Iterator<Item = Square> {
        Squares(self.0)
    }

    /// How many squares are set.
    pub fn count(self) -> u32 {
        self.0.count_ones()
    }

    /// Scatters `index`'s bits into `self`'s set positions (the PDEP instruction) --
    /// the inverse of `pext`. Given `self` as a mask with `n` bits set, calling this for
    /// every `index` in `0..2^n` generates every subset of that mask exactly once, which
    /// is how sliding-piece attack tables enumerate all relevant blocker patterns
    /// without a separate bit-enumeration trick.
    ///
    /// # Safety obligation
    /// Requires the CPU to support BMI2. This project compiles for x86-64-v3 (see
    /// `.cargo/config.toml`), which includes it, and `main` refuses a CPU without it before
    /// anything here runs.
    pub fn pdep(self, index: u64) -> Bitboard {
        Bitboard(unsafe { std::arch::x86_64::_pdep_u64(index, self.0) })
    }

    /// Gathers `occupancy`'s bits at `self`'s set positions into a small, dense index
    /// (the PEXT instruction) -- what the runtime sliding-attack lookup is built on:
    /// `mask.pext(occupancy)` is always in `0..2^mask.count()`, with no gaps, so it can
    /// index directly into a precomputed attack table with no hashing needed.
    ///
    /// # Safety obligation
    /// Same requirement as `pdep`.
    pub fn pext(self, occupancy: Bitboard) -> u64 {
        unsafe { std::arch::x86_64::_pext_u64(occupancy.0, self.0) }
    }
}

struct Squares(u64);

impl Iterator for Squares {
    type Item = Square;

    fn next(&mut self) -> Option<Square> {
        if self.0 == 0 {
            return None;
        }

        let square = Square::try_from(self.0.trailing_zeros() as u8).unwrap();
        self.0 &= self.0 - 1;
        Some(square)
    }
}

impl BitAnd for Bitboard {
    type Output = Bitboard;
    fn bitand(self, rhs: Bitboard) -> Bitboard {
        Bitboard(self.0 & rhs.0)
    }
}

impl BitOr for Bitboard {
    type Output = Bitboard;
    fn bitor(self, rhs: Bitboard) -> Bitboard {
        Bitboard(self.0 | rhs.0)
    }
}

impl BitXor for Bitboard {
    type Output = Bitboard;
    fn bitxor(self, rhs: Bitboard) -> Bitboard {
        Bitboard(self.0 ^ rhs.0)
    }
}

impl Not for Bitboard {
    type Output = Bitboard;
    fn not(self) -> Bitboard {
        Bitboard(!self.0)
    }
}

impl Shl<u32> for Bitboard {
    type Output = Bitboard;
    fn shl(self, rhs: u32) -> Bitboard {
        Bitboard(self.0 << rhs)
    }
}

impl Shr<u32> for Bitboard {
    type Output = Bitboard;
    fn shr(self, rhs: u32) -> Bitboard {
        Bitboard(self.0 >> rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitor_combines_squares() {
        assert_eq!(Bitboard(0b0001) | Bitboard(0b0010), Bitboard(0b0011));
    }

    #[test]
    fn bitand_intersects_squares() {
        assert_eq!(Bitboard(0b0110) & Bitboard(0b0011), Bitboard(0b0010));
    }

    #[test]
    fn bitxor_toggles_squares() {
        assert_eq!(Bitboard(0b0110) ^ Bitboard(0b0011), Bitboard(0b0101));
    }

    #[test]
    fn not_complements_all_64_bits() {
        assert_eq!(!Bitboard(0), Bitboard(u64::MAX));
        assert_eq!(!Bitboard(u64::MAX), Bitboard(0));
    }

    #[test]
    fn shl_shifts_bits_toward_higher_squares() {
        assert_eq!(Bitboard(1) << 8, Bitboard(1 << 8));
    }

    #[test]
    fn shr_shifts_bits_toward_lower_squares() {
        assert_eq!(Bitboard(1 << 8) >> 8, Bitboard(1));
    }

    #[test]
    fn from_square_sets_only_that_bit() {
        assert_eq!(Bitboard::from_square(Square::A1), Bitboard(1));
        assert_eq!(Bitboard::from_square(Square::H8), Bitboard(1 << 63));
    }

    #[test]
    fn empty_has_no_bits_set() {
        assert_eq!(Bitboard::EMPTY, Bitboard(0));
    }

    #[test]
    fn squares_yields_nothing_for_an_empty_bitboard() {
        let squares: Vec<Square> = Bitboard::EMPTY.squares().collect();
        assert_eq!(squares, vec![]);
    }

    #[test]
    fn squares_yields_set_squares_in_ascending_order() {
        // C2 (index 10) and B3 (index 17) -- deliberately built out of order to check
        // squares() sorts by index, not by which was OR'd in first.
        let bitboard = Bitboard::from_square(Square::B3) | Bitboard::from_square(Square::C2);
        let squares: Vec<Square> = bitboard.squares().collect();
        assert_eq!(squares, vec![Square::C2, Square::B3]);
    }

    #[test]
    fn squares_yields_all_64_for_a_full_bitboard() {
        let full = !Bitboard::EMPTY;
        assert_eq!(full.squares().count(), 64);
    }

    #[test]
    fn count_returns_number_of_set_bits() {
        assert_eq!(Bitboard::EMPTY.count(), 0);
        assert_eq!(Bitboard::from_square(Square::A1).count(), 1);
        assert_eq!((Bitboard::from_square(Square::A1) | Bitboard::from_square(Square::H8)).count(), 2);
    }

    #[test]
    fn pdep_scatters_into_mask_positions() {
        let mask =
            Bitboard::from_square(Square::A1) | Bitboard::from_square(Square::C1) | Bitboard::from_square(Square::E1);
        // index 0b101 = bits 0 and 2 set -> scattered into mask's 1st and 3rd set
        // positions (A1 and E1), leaving C1 (the 2nd position) clear.
        let scattered = mask.pdep(0b101);
        let expected = Bitboard::from_square(Square::A1) | Bitboard::from_square(Square::E1);
        assert_eq!(scattered, expected);
    }

    #[test]
    fn pdep_and_pext_are_inverses() {
        let mask =
            Bitboard::from_square(Square::B2) | Bitboard::from_square(Square::D4) | Bitboard::from_square(Square::F6);
        for index in 0u64..8 {
            let subset = mask.pdep(index);
            assert_eq!(mask.pext(subset), index);
        }
    }
}
