//! Single-elimination bracket seeding: which two participants meet in round 1, and which byes
//! (empty bracket slots, for non-power-of-two participant counts) let their top seeds skip
//! straight to round 2. Following the standard construction described in the tournament design's
//! "Tournament: single-elimination knockout" section -- seed 1 and seed 2 can only meet in the
//! final, seeds 1-4 only in the semifinal or later, and so on -- so the strongest seeds never
//! collide early. `participants` is assumed strongest-first (seed 1 = `participants[0]`), the
//! same ordering convention `config.yaml` itself already uses.

pub struct Pairing {
    /// The stronger (lower-seeded) side of the pairing.
    pub engine_one: String,
    pub engine_two: String,
}

/// Round 1's own real (both-sides-present) pairings, in bracket-position order. A participant
/// count that isn't a power of two leaves some bracket slots empty ("byes"): the seed on the
/// other side of an empty slot advances automatically, with no round-1 game played, so byes
/// don't appear in this list at all.
pub fn round_one_pairings(participants: &[String]) -> Vec<Pairing> {
    let size = participants.len().next_power_of_two() as u32;
    let order = seed_order(size);
    let mut pairings = Vec::new();
    for slots in order.chunks(2) {
        let (a, b) = (slots[0], slots[1]);
        let a_name = participants.get((a - 1) as usize);
        let b_name = participants.get((b - 1) as usize);
        if let (Some(a_name), Some(b_name)) = (a_name, b_name) {
            let (stronger, weaker) = if a < b { (a_name, b_name) } else { (b_name, a_name) };
            pairings.push(Pairing { engine_one: stronger.clone(), engine_two: weaker.clone() });
        }
    }
    pairings
}

/// The bracket-slot sequence for a `size`-slot bracket (`size` a power of two), as seed numbers
/// `1..=size` in the order they occupy consecutive slots -- slots `2i`/`2i+1` are round 1's
/// `i`-th match. `order(1) = [1]`; `order(2k)` interleaves each of `order(k)`'s seeds with its
/// bracket complement `2k + 1 - s`, the recursive construction that keeps top seeds apart for as
/// long as possible.
fn seed_order(size: u32) -> Vec<u32> {
    if size == 1 {
        return vec![1];
    }
    let half = seed_order(size / 2);
    let mut order = Vec::with_capacity(size as usize);
    for seed in half {
        order.push(seed);
        order.push(size + 1 - seed);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_order_matches_the_standard_eight_seed_bracket() {
        assert_eq!(seed_order(8), vec![1, 8, 4, 5, 2, 7, 3, 6]);
    }

    #[test]
    fn round_one_pairings_gives_the_top_eleven_seeds_byes_for_twenty_one_participants() {
        let participants: Vec<String> = (0..=20).rev().map(|skill| format!("stockfish-skill-{skill}")).collect();
        let pairings = round_one_pairings(&participants);
        let flattened: Vec<&str> =
            pairings.iter().flat_map(|pairing| [pairing.engine_one.as_str(), pairing.engine_two.as_str()]).collect();

        assert_eq!(pairings.len(), 5);
        // The two strongest seeds never meet before the final -- with N=21 they don't even play
        // in round 1 at all, both drawing a bye.
        assert!(!flattened.contains(&"stockfish-skill-20"));
        assert!(!flattened.contains(&"stockfish-skill-19"));
    }
}
