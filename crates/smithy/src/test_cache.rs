//! The test batches a run prepares once and keeps, so that later rounds' test passes upload them
//! instead of preparing them again. The test half never changes within a run, yet preparing it is
//! CPU work: for psq_threats, a `Board` and its attacks on every position.
//!
//! What is kept is a prefix of the test pass's batches, as many as fit a memory budget; the rest is
//! read and prepared every round as before. The batches, their order and the test loss are the same
//! whichever way a batch arrives.

use kalo_chess::pipeline::system_memory::{self, SystemMemory};

/// The bytes left to the operating system before the budget is taken.
const OS_RESERVE_BYTES: u64 = 1 << 30;

/// The share of the rest the cache may hold. The rest of the rest is the trainer's own memory, which
/// the total does not subtract: libtorch and CUDA's host side, the shuffle buffer, the batches in
/// flight in the pipeline. Half, and no more, because nothing else is meant to run on a machine
/// while it trains: the margin covers the trainer, not other programs.
const BUDGET_SHARE: f64 = 0.5;

/// Where a test pass is in its readers: the reader's index and the row within it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub reader: usize,
    pub row: u64,
}

/// The kept prefix of a run's test batches, and where the rest of the pass starts.
pub struct TestCache<H> {
    budget: u64,
    bytes_of: fn(&H) -> usize,
    /// Each kept batch, with its row count and its piece count (`round::piece_count_of`), so a pass
    /// replaying it attributes it to its piece count as the first pass did.
    batches: Vec<(H, u64, Option<usize>)>,
    bytes: u64,
    /// Set by the first pass: where the first batch that was not kept starts, or past the last
    /// reader if every batch was kept.
    rest: Option<Position>,
}

impl<H> TestCache<H> {
    /// An empty cache that may hold `budget` bytes, measuring each batch with `bytes_of`.
    pub fn new(budget: u64, bytes_of: fn(&H) -> usize) -> TestCache<H> {
        TestCache { budget, bytes_of, batches: Vec::new(), bytes: 0, rest: None }
    }

    /// Whether a first pass has filled it.
    pub fn is_filled(&self) -> bool {
        self.rest.is_some()
    }

    /// The kept batches, in pass order, with their row counts and piece counts.
    pub fn batches(&self) -> &[(H, u64, Option<usize>)] {
        &self.batches
    }

    /// Where the part of the pass not kept starts, once filled.
    pub fn rest(&self) -> Option<Position> {
        self.rest
    }

    /// The bytes held.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The budget, in bytes.
    pub fn budget(&self) -> u64 {
        self.budget
    }

    /// The rows held.
    pub fn rows(&self) -> u64 {
        self.batches.iter().map(|(_, rows, _)| rows).sum()
    }

    /// During the first pass: keeps `batch`, of `rows` rows and piece count `pieces`, which starts at
    /// `start`, if it fits and every batch before it was kept; otherwise records `start` as where the
    /// rest begins and keeps nothing more. Returns `batch` back when it was not kept.
    pub fn offer(&mut self, batch: H, rows: u64, pieces: Option<usize>, start: Position) -> Option<H> {
        if self.rest.is_some() {
            return Some(batch);
        }
        let size = (self.bytes_of)(&batch) as u64;
        if self.bytes + size > self.budget {
            self.rest = Some(start);
            return Some(batch);
        }
        self.bytes += size;
        self.batches.push((batch, rows, pieces));
        None
    }

    /// Ends the first pass: if every batch was kept, the rest starts at `end`, past the last reader.
    pub fn finish_filling(&mut self, end: Position) {
        if self.rest.is_none() {
            self.rest = Some(end);
        }
    }
}

/// This machine's cache budget, read once (`kalo_chess::pipeline::system_memory`), or why it cannot be.
pub fn budget() -> Result<u64, String> {
    system_memory::read().map(budget_from)
}

/// `min((total - 1 GB) * 50%, available)`. From the total, so that the budget depends on the machine
/// alone and every run on it keeps the same batches; the available memory only as a guard, in case
/// something else does run.
fn budget_from(memory: SystemMemory) -> u64 {
    let share = (memory.total_bytes.saturating_sub(OS_RESERVE_BYTES) as f64 * BUDGET_SHARE) as u64;
    share.min(memory.available_bytes)
}

/// `bytes` in GB, for the run's log.
pub fn gigabytes(bytes: u64) -> f64 {
    bytes as f64 / (1u64 << 30) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    #[test]
    fn the_budget_is_half_of_total_less_one_gigabyte() {
        assert_eq!(budget_from(SystemMemory { total_bytes: 9 * GIB, available_bytes: 8 * GIB }), 4 * GIB);
    }

    #[test]
    fn the_budget_never_exceeds_the_available_memory() {
        assert_eq!(budget_from(SystemMemory { total_bytes: 9 * GIB, available_bytes: GIB }), GIB);
    }

    #[test]
    fn a_cache_keeps_a_prefix_and_records_where_the_rest_starts() {
        let mut cache = TestCache::new(10, |batch: &Vec<u8>| batch.len());
        assert_eq!(cache.offer(vec![0; 4], 4, Some(5), Position { reader: 0, row: 0 }), None);
        assert_eq!(cache.offer(vec![0; 4], 4, None, Position { reader: 0, row: 4 }), None);
        // Does not fit: the rest starts here, and nothing later is kept, even what would fit.
        assert_eq!(cache.offer(vec![0; 4], 4, Some(6), Position { reader: 0, row: 8 }), Some(vec![0; 4]));
        assert_eq!(cache.offer(vec![0; 1], 1, Some(6), Position { reader: 0, row: 12 }), Some(vec![0; 1]));
        cache.finish_filling(Position { reader: 1, row: 0 });
        assert_eq!(cache.rows(), 8);
        assert_eq!(cache.bytes(), 8);
        assert_eq!(cache.rest(), Some(Position { reader: 0, row: 8 }));
        let kept: Vec<Option<usize>> = cache.batches().iter().map(|(_, _, pieces)| *pieces).collect();
        assert_eq!(kept, vec![Some(5), None], "each kept batch keeps its piece count");
    }

    #[test]
    fn a_cache_that_keeps_everything_has_its_rest_past_the_last_reader() {
        let mut cache = TestCache::new(100, |batch: &Vec<u8>| batch.len());
        assert_eq!(cache.offer(vec![0; 4], 4, None, Position { reader: 0, row: 0 }), None);
        assert!(!cache.is_filled());
        cache.finish_filling(Position { reader: 1, row: 0 });
        assert!(cache.is_filled());
        assert_eq!(cache.rest(), Some(Position { reader: 1, row: 0 }));
    }
}
