use crate::board::bitboard::Bitboard;
use crate::board::square::Square;

/// Precomputed PEXT-based sliding-attack data for one piece type (bishop or rook) --
/// built once from a list of ray directions, then looked up via PEXT at runtime.
/// Bishop and rook need identical machinery, differing only in which 4 directions they
/// ray-cast along, so it lives here once rather than duplicated per piece.
pub struct SlidingAttackTable {
    /// Per square, its relevant occupancy mask (see `relevant_mask`) and where its patterns start
    /// in `attacks`: side by side, so a lookup reads one entry, then one slot of `attacks`.
    squares: [SquareEntry; 64],
    /// Every square's precomputed attack bitboards, one per masked-occupancy pattern, square after
    /// square in one flat array (Stockfish's layout): a lookup is one dependent load, where a table
    /// per square (`Vec<Vec<_>>`) made it two.
    attacks: Vec<Bitboard>,
}

#[derive(Clone, Copy)]
struct SquareEntry {
    mask: Bitboard,
    offset: usize,
}

impl SlidingAttackTable {
    /// Builds the full table for a piece that rays out along `directions`.
    pub fn build(directions: [(i8, i8); 4]) -> SlidingAttackTable {
        let mut squares = [SquareEntry { mask: Bitboard::EMPTY, offset: 0 }; 64];
        let mut attacks = Vec::new();

        for square_value in 0u8..64 {
            let square = Square::try_from(square_value).unwrap();
            let mask = relevant_mask(square, &directions);
            squares[square as usize] = SquareEntry { mask, offset: attacks.len() };

            let pattern_count = 1usize << mask.count();
            for index in 0..pattern_count {
                let occupancy_subset = mask.pdep(index as u64);
                attacks.push(ray_attacks(square, &directions, occupancy_subset));
            }
        }

        SlidingAttackTable { squares, attacks }
    }

    /// The attack set for a piece on `square`, given the actual board `occupancy`.
    pub fn attacks(&self, square: Square, occupancy: Bitboard) -> Bitboard {
        let entry = self.squares[square as usize];
        let index = entry.offset + entry.mask.pext(occupancy) as usize;
        // In range by construction: square `s`'s patterns are `offset..offset + 2^mask.count()`, and
        // PEXT of a mask with `n` bits set is below `2^n`.
        unsafe { *self.attacks.get_unchecked(index) }
    }
}

/// The squares that could block one of a slider's rays from `square`, excluding the
/// square itself and the far edge of each ray -- whether that exact square is occupied
/// never changes the attack set (the ray either already stopped before it, or reaches
/// it regardless of what's on it, since there's nothing beyond it to distinguish), so
/// it's excluded to keep the mask, and therefore the attack table, as small as possible.
fn relevant_mask(square: Square, directions: &[(i8, i8); 4]) -> Bitboard {
    let mut mask = Bitboard::EMPTY;

    for &(file_delta, rank_delta) in directions {
        let mut ray = Vec::new();
        let mut current = square;
        while let Some(next) = current.try_offset(file_delta, rank_delta) {
            ray.push(next);
            current = next;
        }
        ray.pop(); // drop the edge square (does nothing if this ray had none at all)

        for square_on_ray in ray {
            mask = mask | Bitboard::from_square(square_on_ray);
        }
    }

    mask
}

/// The actual attack set from `square` along `directions`, given a specific blocker
/// pattern -- walks each ray one step at a time, including (but not going past) the
/// first occupied square in each direction.
fn ray_attacks(square: Square, directions: &[(i8, i8); 4], occupancy: Bitboard) -> Bitboard {
    let mut attacks = Bitboard::EMPTY;

    for &(file_delta, rank_delta) in directions {
        let mut current = square;
        while let Some(next) = current.try_offset(file_delta, rank_delta) {
            attacks = attacks | Bitboard::from_square(next);
            if (occupancy & Bitboard::from_square(next)) != Bitboard::EMPTY {
                break;
            }
            current = next;
        }
    }

    attacks
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOK_DIRECTIONS: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

    #[test]
    fn relevant_mask_excludes_square_itself_and_ray_edges() {
        // A rook on A1: the mask should be A2-A7 (not A8, the edge) and B1-G1 (not H1).
        let mask = relevant_mask(Square::A1, &ROOK_DIRECTIONS);
        let expected = Bitboard::from_square(Square::A2)
            | Bitboard::from_square(Square::A3)
            | Bitboard::from_square(Square::A4)
            | Bitboard::from_square(Square::A5)
            | Bitboard::from_square(Square::A6)
            | Bitboard::from_square(Square::A7)
            | Bitboard::from_square(Square::B1)
            | Bitboard::from_square(Square::C1)
            | Bitboard::from_square(Square::D1)
            | Bitboard::from_square(Square::E1)
            | Bitboard::from_square(Square::F1)
            | Bitboard::from_square(Square::G1);
        assert_eq!(mask, expected);
    }

    #[test]
    fn ray_attacks_stops_at_first_blocker_in_each_direction() {
        // Rook on D4 with a blocker on D6 (two squares up) and nothing else: attacks
        // reach D5 and D6 (captures the blocker) but not D7/D8; unobstructed directions
        // still go all the way to the edge.
        let occupancy = Bitboard::from_square(Square::D6);
        let attacks = ray_attacks(Square::D4, &ROOK_DIRECTIONS, occupancy);

        assert!((attacks & Bitboard::from_square(Square::D5)) != Bitboard::EMPTY);
        assert!((attacks & Bitboard::from_square(Square::D6)) != Bitboard::EMPTY);
        assert!((attacks & Bitboard::from_square(Square::D7)) == Bitboard::EMPTY);
        assert!((attacks & Bitboard::from_square(Square::D8)) == Bitboard::EMPTY);
        assert!((attacks & Bitboard::from_square(Square::A4)) != Bitboard::EMPTY);
        assert!((attacks & Bitboard::from_square(Square::H4)) != Bitboard::EMPTY);
    }

    #[test]
    fn table_lookup_matches_direct_ray_attacks_computation() {
        let table = SlidingAttackTable::build(ROOK_DIRECTIONS);
        let occupancy = Bitboard::from_square(Square::D6) | Bitboard::from_square(Square::B4);

        assert_eq!(table.attacks(Square::D4, occupancy), ray_attacks(Square::D4, &ROOK_DIRECTIONS, occupancy));
    }

    #[test]
    fn table_lookup_on_empty_board_reaches_every_edge() {
        let table = SlidingAttackTable::build(ROOK_DIRECTIONS);
        let attacks = table.attacks(Square::D4, Bitboard::EMPTY);
        assert_eq!(attacks, ray_attacks(Square::D4, &ROOK_DIRECTIONS, Bitboard::EMPTY));
        assert_eq!(attacks.count(), 14); // a rook on D4 sees all 7 squares in each of 2 open lines
    }
}
