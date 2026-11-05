//! The shapes a training round can read from, behind one trait each, so `round.rs` holds one loop.
//!
//! `TrainStream` is what training reads: an NNUE's training set fully shuffled through buckets
//! (`buckets.rs`), a transformer's PGN segments through one buffer per piece count
//! (`bucketed_train_stream.rs`), since it cannot pad and every batch it sees must hold positions of
//! exactly one length.
//!
//! `BatchSource` is the smaller, read-only half: everything the per-round test pass needs, which is
//! a row count and a stop-at-the-end batch read.

use super::cache::CacheRecord;

/// One test set that can be swept in order, exactly once, from row `0`. `read_batch_stop_short` never
/// wraps mid-batch and returns cursor `0` once it reaches the end, which is what makes "loop until
/// the cursor comes back to `0`" a complete sweep with no row read twice and none missed.
pub trait BatchSource {
    fn row_count(&self) -> u64;
    fn read_batch_stop_short(&mut self, cursor: u64, batch_size: usize) -> (Vec<CacheRecord>, u64);
}

/// Whichever source a caller picked at run time.
impl<T: BatchSource + ?Sized> BatchSource for Box<T> {
    fn row_count(&self) -> u64 {
        (**self).row_count()
    }

    fn read_batch_stop_short(&mut self, cursor: u64, batch_size: usize) -> (Vec<CacheRecord>, u64) {
        (**self).read_batch_stop_short(cursor, batch_size)
    }
}

/// Where a training round's batches come from, and enough state to persist and resume it.
pub trait TrainStream {
    /// One training batch. Callers weight by `records.len()` rather than assuming a fixed width.
    fn next_batch(&mut self, batch_size: usize) -> Vec<CacheRecord>;

    /// This stream's resume state as of now, to be handed to `RoundProgress` and back to the
    /// constructor on the next run. Empty for a stream whose position is derivable from the
    /// optimizer's own persisted step count, and so must not be persisted separately -- every stream
    /// today.
    fn cursors(&self) -> Vec<u64>;
}
