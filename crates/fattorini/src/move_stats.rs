//! What the main search did with each quiet move, by the move's history score and the node's
//! remaining depth. Measures where a move picker's quiet thresholds should sit: how real scores are
//! spread, how many quiets late-move pruning leaves unreached, and how often a quiet with a given
//! score causes the cutoff.
//!
//! Recorded into one global table, which `take` reads and resets. A lock per node is slow, but this
//! is only ever built for a single-threaded bench.

use std::sync::Mutex;

use crate::board::chess_move::Move;

/// The lowest score bucketed; anything lower lands in the first bucket. `-(8192 + 32768)`, the
/// lowest flat plus continuation history can sum to.
pub const SCORE_MIN: i32 = -40960;

/// The width of one score bucket, in history points. Fine, because real scores within one search
/// mostly stay within a few hundred points of zero.
pub const BUCKET_WIDTH: i32 = 16;

/// `[-40960, 40960)` in buckets of 16; `40960` itself lands in the last one.
pub const BUCKETS: usize = 5120;

/// Remaining depths `0..DEPTHS`; deeper nodes land in the last.
pub const DEPTHS: usize = 21;

/// What became of one quiet move at its node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The loop ended (late-move pruning, or a cutoff by an earlier move) before reaching it.
    NotReached = 0,
    /// Reached, but skipped by futility, late-move, history or SEE pruning.
    Pruned = 1,
    /// Searched, and some other move (or none) caused the cutoff.
    Searched = 2,
    /// Searched, and it caused the cutoff.
    Cutoff = 3,
}

pub const OUTCOMES: usize = 4;

pub struct MoveStats {
    /// Indexed by `depth * BUCKETS + bucket`, one count per outcome.
    pub counts: Vec<[u64; OUTCOMES]>,
    /// Nodes recorded, per remaining depth.
    pub nodes: Vec<u64>,
    /// Per remaining depth, the quiets scoring exactly `0` (mostly entries never updated in this
    /// search), per outcome. Also counted in `counts`.
    pub zeros: Vec<[u64; OUTCOMES]>,
    /// Per remaining depth, the losing captures (SEE below `0`), per outcome: what a quiet with
    /// a very bad score is compared against when choosing where bad quiets go.
    pub losing_captures: Vec<[u64; OUTCOMES]>,
}

impl MoveStats {
    const fn empty() -> MoveStats {
        return MoveStats { counts: Vec::new(), nodes: Vec::new(), zeros: Vec::new(), losing_captures: Vec::new() };
    }

    pub fn count(&self, depth: usize, bucket: usize) -> [u64; OUTCOMES] {
        return self.counts.get(depth * BUCKETS + bucket).copied().unwrap_or([0; OUTCOMES]);
    }

    /// Quiets recorded at `depth`, per outcome.
    pub fn depth_totals(&self, depth: usize) -> [u64; OUTCOMES] {
        let mut totals = [0; OUTCOMES];
        for bucket in 0..BUCKETS {
            let count = self.count(depth, bucket);
            for outcome in 0..OUTCOMES {
                totals[outcome] += count[outcome];
            }
        }
        return totals;
    }
}

static STATS: Mutex<MoveStats> = Mutex::new(MoveStats::empty());

/// The bucket `score` falls in.
pub fn bucket(score: i32) -> usize {
    return ((score.clamp(SCORE_MIN, -SCORE_MIN - 1) - SCORE_MIN) / BUCKET_WIDTH) as usize;
}

/// The lowest score of bucket `index`.
pub fn bucket_low(index: usize) -> i32 {
    return SCORE_MIN + index as i32 * BUCKET_WIDTH;
}

/// One quiet move's outcome, from what its node did: the move that caused the cutoff, the quiets
/// searched without causing it, and every move the loop reached.
pub fn outcome(mv: Move, cutoff: Option<Move>, searched: &[Move], reached: &[Move]) -> Outcome {
    if cutoff == Some(mv) {
        return Outcome::Cutoff;
    }
    if searched.contains(&mv) {
        return Outcome::Searched;
    }
    if reached.contains(&mv) {
        return Outcome::Pruned;
    }
    return Outcome::NotReached;
}

/// Records one node: its remaining depth, each legal quiet move (the TT move excluded) with its
/// history score when the node began, and its legal losing captures (the TT move excluded).
/// `searched` is every move searched without causing the cutoff, quiet or not.
pub fn record(
    remaining: u32, scored: &[(Move, i32)], losing_captures: &[Move], cutoff: Option<Move>, searched: &[Move],
    reached: &[Move],
) {
    let depth = (remaining as usize).min(DEPTHS - 1);
    let mut stats = STATS.lock().expect("move stats lock");
    if stats.counts.is_empty() {
        stats.counts = vec![[0; OUTCOMES]; DEPTHS * BUCKETS];
        stats.nodes = vec![0; DEPTHS];
        stats.zeros = vec![[0; OUTCOMES]; DEPTHS];
        stats.losing_captures = vec![[0; OUTCOMES]; DEPTHS];
    }
    stats.nodes[depth] += 1;
    for &(mv, score) in scored {
        let outcome = outcome(mv, cutoff, searched, reached);
        stats.counts[depth * BUCKETS + bucket(score)][outcome as usize] += 1;
        if score == 0 {
            stats.zeros[depth][outcome as usize] += 1;
        }
    }
    for &mv in losing_captures {
        let outcome = outcome(mv, cutoff, searched, reached);
        stats.losing_captures[depth][outcome as usize] += 1;
    }
}

/// Everything recorded since the last `take`, leaving the table empty.
pub fn take() -> MoveStats {
    let mut stats = STATS.lock().expect("move stats lock");
    return std::mem::replace(&mut *stats, MoveStats::empty());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::square::Square;

    #[test]
    fn the_extreme_scores_land_in_the_first_and_last_buckets_and_zero_in_the_middle() {
        assert_eq!(bucket(SCORE_MIN), 0);
        assert_eq!(bucket(i32::MIN), 0);
        assert_eq!(bucket(-SCORE_MIN), BUCKETS - 1);
        assert_eq!(bucket(0), BUCKETS / 2);
        assert_eq!(bucket(-1), BUCKETS / 2 - 1);
        assert_eq!(bucket_low(BUCKETS / 2), 0);
    }

    #[test]
    fn a_move_is_classified_by_the_first_thing_that_happened_to_it() {
        let a = Move::new(Square::E2, Square::E4, None);
        let b = Move::new(Square::D2, Square::D4, None);
        let c = Move::new(Square::G1, Square::F3, None);
        let d = Move::new(Square::B1, Square::C3, None);
        let reached = [a, b, c];
        let searched = [a];
        assert_eq!(outcome(a, Some(b), &searched, &reached), Outcome::Searched);
        assert_eq!(outcome(b, Some(b), &searched, &reached), Outcome::Cutoff);
        assert_eq!(outcome(c, Some(b), &searched, &reached), Outcome::Pruned);
        assert_eq!(outcome(d, Some(b), &searched, &reached), Outcome::NotReached);
    }
}
