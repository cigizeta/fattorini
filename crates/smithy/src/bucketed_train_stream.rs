//! The transformer's training stream under `labels: self` and `labels: lc0`: the records of
//! `pgn_train_stream::EpochRecords`, under its epoch and resume rules, through one shuffle buffer
//! per piece count, since the transformer cannot pad and every batch must hold positions of one
//! length.
//!
//! **Buckets.** A record goes to the bucket of its piece count, 3 to 32 with the kings: 30
//! buckets. K vs K is dropped, after `EpochRecords` has counted it, so an epoch's count stays its
//! searched positions, the `.meta` total less its games (`cache::is_bare_kings`).
//!
//! **Capacities from the first fill.** The stream first reads `buffer_capacity` records into the
//! buckets, emitting nothing. Each bucket's capacity is then what it holds, but at least two batches,
//! so a bucket's capacity follows its piece count's share of the games, and the total stays near
//! `buffer_capacity`, plus the floors of the rare buckets.
//!
//! **A bucket emits when full.** Records go on being read into their buckets, and the first bucket
//! to reach its capacity gives one batch, drawn uniformly at random from it. Each piece count then
//! gets batches at the rate its records arrive, so over a run the mix of lengths is the games'. A
//! rare bucket's records may wait long, across epochs, but none is lost or given twice.
//!
//! **Batches are always full**, so a resume is derived from the step count as the NNUE's is.

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::cache::{is_bare_kings, CacheRecord};
use super::pgn_stream::StreamFile;
use super::pgn_train_stream::EpochRecords;
use super::train_stream::TrainStream;

/// One bucket per count of pieces beside the kings, 0 to 30; bucket 0, K vs K, is never filled.
const BUCKETS: usize = 31;

pub(crate) struct BucketedTrainStream {
    records: EpochRecords,
    batch_size: usize,
    /// `buckets[n]` holds the records with `n` pieces beside the kings.
    buckets: Vec<Vec<CacheRecord>>,
    /// Each bucket's capacity, set by the first fill.
    capacities: Vec<usize>,
    /// The records the first fill read, every bucket together.
    filled: usize,
    rng: StdRng,
}

impl BucketedTrainStream {
    /// Opens the stream over `files`, labelled at every one of `lambdas` in that order, `emitted`
    /// positions into the run (`batch_size * step_count`), and fills its buckets with
    /// `buffer_capacity` records up front, so the fill reports as its own step.
    pub fn open(
        files: Vec<StreamFile>, lambdas: &[f64], buffer_capacity: usize, batch_size: usize, emitted: u64, threads: usize,
    ) -> BucketedTrainStream {
        assert!(buffer_capacity > 0, "a shuffle buffer holds at least one record");
        assert!(batch_size > 0, "a batch holds at least one record");
        let records = EpochRecords::open(files, lambdas, emitted, threads);
        // Salted apart from `PgnTrainStream`'s 1, though no run holds both.
        let rng = StdRng::seed_from_u64(records.draw_seed(2));
        let mut stream =
            BucketedTrainStream { records, batch_size, buckets: vec![Vec::new(); BUCKETS], capacities: Vec::new(), filled: 0, rng };
        while stream.filled < buffer_capacity {
            let record = stream.next_trainable();
            stream.buckets[bucket_of(&record)].push(record);
            stream.filled += 1;
        }
        stream.capacities = stream.buckets.iter().map(|bucket| bucket.len().max(2 * batch_size)).collect();
        stream
    }

    /// The records the first fill read.
    #[cfg(test)]
    pub fn buffer_capacity(&self) -> usize {
        self.filled
    }

    /// The next record that is not K vs K. Stops on a corpus whose whole epoch is K vs K, which would
    /// otherwise loop here forever.
    fn next_trainable(&mut self) -> CacheRecord {
        let mut skipped = 0u64;
        loop {
            let record = self.records.next_record();
            if !is_bare_kings(&record.canonical) {
                return record;
            }
            skipped += 1;
            assert!(skipped <= self.records.epoch_positions(), "a whole epoch of the training files holds only K vs K");
        }
    }
}

/// The bucket of `record`: its count of pieces beside the kings.
fn bucket_of(record: &CacheRecord) -> usize {
    let pieces = record.canonical.pieces.len();
    assert!(pieces < BUCKETS, "a position with {pieces} pieces beside the kings");
    pieces
}

impl TrainStream for BucketedTrainStream {
    /// Never short: a bucket gives a batch only once it holds its capacity, at least two batches.
    fn next_batch(&mut self, batch_size: usize) -> Vec<CacheRecord> {
        assert_eq!(batch_size, self.batch_size, "the stream was opened for batches of {}", self.batch_size);
        loop {
            let record = self.next_trainable();
            let bucket = bucket_of(&record);
            self.buckets[bucket].push(record);
            if self.buckets[bucket].len() < self.capacities[bucket] {
                continue;
            }
            let records = &mut self.buckets[bucket];
            let mut batch = Vec::with_capacity(batch_size);
            for _ in 0..batch_size {
                let slot = self.rng.random_range(0..records.len());
                batch.push(records.swap_remove(slot));
            }
            return batch;
        }
    }

    /// Deliberately empty, as `PgnTrainStream`'s: the position is derived from the step count.
    fn cursors(&self) -> Vec<u64> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three games, 10 positions: a white rook alone beside the kings (3 positions), rook and knight
    /// (4), and the kings alone (3, K vs K).
    const GAMES: &str = concat!(
        "[Result \"1-0\"]\n[FEN \"4k3/8/8/8/8/8/8/R3K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Kd2 {1.50} Kd7 {-1.20}\n1-0\n\n",
        "[Result \"0-1\"]\n[FEN \"4k3/8/8/8/8/8/8/RN2K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Ke2 {-0.40} Ke7 {0.30} 2. Kf2 {-0.90}\n0-1\n\n",
        "[Result \"1/2-1/2\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Kd2 {0.00} Kd7 {0.00}\n1/2-1/2\n\n",
    );

    fn fixture(name: &str, repeats: &[u64]) -> (String, Vec<StreamFile>) {
        let dir = std::env::temp_dir().join(format!("smithy-bucketed-train-stream-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("failed to create the fixture directory");
        let dir = dir.to_str().unwrap().replace('\\', "/");
        let files = repeats
            .iter()
            .enumerate()
            .map(|(index, &count)| {
                let path = format!("{dir}/gamma-{index}.pgn.zst");
                let compressed = zstd::encode_all(GAMES.repeat(count as usize).as_bytes(), 3).expect("failed to compress");
                std::fs::write(&path, compressed).expect("failed to write the fixture");
                StreamFile { path, positions: count * 10 }
            })
            .collect();
        (dir, files)
    }

    #[test]
    fn every_batch_is_full_of_one_piece_count_and_never_k_vs_k() {
        let (dir, files) = fixture("one-length", &[3, 4]);
        let mut stream = BucketedTrainStream::open(files, &[0.7, 1.0], 8, 2, 0, 2);
        assert_eq!(stream.buffer_capacity(), 8);
        for _ in 0..200 {
            let batch = stream.next_batch(2);
            assert_eq!(batch.len(), 2);
            let pieces = batch[0].canonical.pieces.len();
            assert!(pieces > 0, "a K vs K batch");
            assert!(batch.iter().all(|record| record.canonical.pieces.len() == pieces), "a batch mixes piece counts");
            assert!(batch.iter().all(|record| record.labels.len() == 2), "one label per lambda");
        }
        assert!(stream.cursors().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn each_piece_count_gets_batches_in_proportion_to_its_records() {
        let (dir, files) = fixture("shares", &[5, 5]);
        let mut stream = BucketedTrainStream::open(files, &[1.0], 16, 2, 0, 2);
        // The rook alone is 3 of every 7 trainable positions, rook and knight the other 4.
        let batches = 3000;
        let rook_alone = (0..batches).filter(|_| stream.next_batch(2)[0].canonical.pieces.len() == 1).count();
        let share = rook_alone as f64 / batches as f64;
        assert!((0.38..=0.48).contains(&share), "the rook alone got {share:.3} of the batches, about 3/7 expected");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_resume_starts_where_the_step_count_says() {
        let (dir, files) = fixture("resume", &[2, 3]);
        // 50 positions an epoch; 120 emitted is epoch 2, 20 into it.
        let stream = BucketedTrainStream::open(files, &[1.0], 4, 2, 120, 1);
        assert_eq!(stream.records.epoch(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    #[should_panic(expected = "holds only K vs K")]
    fn a_corpus_of_k_vs_k_alone_is_refused() {
        let dir = std::env::temp_dir().join(format!("smithy-bucketed-train-stream-test-bare-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("failed to create the fixture directory");
        let path = format!("{}/gamma-0.pgn.zst", dir.to_str().unwrap().replace('\\', "/"));
        let bare = "[Result \"1/2-1/2\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Kd2 {0.00} Kd7 {0.00}\n1/2-1/2\n\n";
        std::fs::write(&path, zstd::encode_all(bare.repeat(4).as_bytes(), 3).expect("failed to compress")).expect("failed to write");
        let _ = BucketedTrainStream::open(vec![StreamFile { path, positions: 12 }], &[1.0], 4, 2, 0, 1);
    }
}
