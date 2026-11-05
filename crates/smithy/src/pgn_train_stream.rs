//! The source's segments read straight from their compressed PGN (`pgn_stream`), epoch after epoch
//! without end (`EpochRecords`): what a transformer forge trains on, through one buffer per piece
//! count (`bucketed_train_stream.rs`). An NNUE forge trains on its corpus fully shuffled instead
//! (`buckets.rs`); `PgnTrainStream`, the one shuffle buffer it once read through, is kept for the
//! tests that sweep an epoch whole.
//!
//! **Epochs.** An epoch is every searched position of the files once: the positions their `.meta`
//! record less their games, as each game's final position, never searched, is left out. Each epoch
//! reads the files in a fresh order, shuffled with a seed from the epoch number, and `pgn_stream`
//! mixes them across the corpus within it.
//!
//! **Resume may retrain.** Where a run stands is derived from its step count: `emitted = training_batch_size * step_count` positions, so epoch `emitted / epoch` and
//! `emitted % epoch` positions into it. A resumed run reads that epoch afresh, under a seed of its own
//! so it is not the interrupted order, and stops it short by the positions already trained: in that
//! epoch some positions are trained twice and some not at all, every pick random either way. The next
//! epoch is whole. Nothing is persisted beyond the step count, and nothing is replayed.

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::cache::CacheRecord;
use super::pgn_stream::splitmix64;
use super::pgn_stream::{PgnStream, StreamFile, StreamOptions};
#[cfg(test)]
use super::train_stream::TrainStream;
use temper_chess::pipeline::segments;

use super::{preprocessing_config, remote};

/// A source's training segments on the remote, in the day directories of `{root}/train` (Lc0's
/// `data/chess/lc0`, PeSTO's `data/chess/pesto`), read in place, each with the positions its `.meta` records, by
/// day and then by name. An error when there are none.
pub(crate) fn training_files(root: &str) -> Result<Vec<StreamFile>, String> {
    segment_files(&preprocessing_config::source_train_dir(root))
}

/// A source's test segments, in `{root}/test`, as `training_files` reads its training ones.
pub(crate) fn test_files(root: &str) -> Result<Vec<StreamFile>, String> {
    segment_files(&preprocessing_config::source_test_dir(root))
}

/// The segments of `dir` on the remote, by day and then by name, each with its searched positions:
/// its `.meta` positions less its games, one final position a game; an error when there are none.
fn segment_files(dir: &str) -> Result<Vec<StreamFile>, String> {
    let remote_dir = remote::remote_path(dir);
    let names = segments::list_remote(dir).map_err(|error| format!("failed to list {remote_dir}: {error}"))?;
    if names.is_empty() {
        return Err(format!("{remote_dir} holds no segments"));
    }
    let mut files = Vec::with_capacity(names.len());
    // One `.meta` a segment, each fetched from the remote on its own: minutes for Lc0's 1,400, so
    // the page shows them counted rather than a run that seems stalled.
    let progress = progress::bar(names.len() as u64, progress::Unit::Count("segments"), &format!("listing the segments of {dir}"));
    for name in names {
        let meta = segments::read_remote_meta(dir, &name)?;
        let positions = meta.positions.saturating_sub(meta.games);
        files.push(StreamFile { path: format!("{remote_dir}/{name}{}", segments::SEGMENT_SUFFIX), positions });
        progress.inc(1);
    }
    progress.finish();
    Ok(files)
}

/// Files open at once across the reader's workers: enough that every stretch of an epoch draws from
/// many games of many files.
const OPEN_FILES: usize = 64;

/// Distinct from every other seed in smithy, so an epoch's file order is its own.
const EPOCH_SEED: u64 = 0x5EED_E90C;

/// The reader's threads: half the logical cores, at least 1. The other half builds the batches
/// (`round::BatchBuilder` runs its CPU half on every core). 4 threads streamed 3.4M positions a
/// second, far above what a training step consumes.
pub(crate) fn reader_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(2, |cores| cores.get());
    (cores / 2).max(1)
}

/// The records of `files`, endlessly, epoch after epoch, under the rules of this module's header:
/// what both training streams read from. `PgnTrainStream` shuffles them through one buffer,
/// `bucketed_train_stream::BucketedTrainStream` through one buffer per piece count.
pub(crate) struct EpochRecords {
    files: Vec<StreamFile>,
    lambdas: Vec<f64>,
    threads: usize,
    /// The positions of one epoch: the files' searched positions summed.
    epoch_positions: u64,
    epoch: u64,
    /// Positions the current epoch still reads.
    left_in_epoch: u64,
    stream: PgnStream,
    /// The seed the first epoch read was opened under, from which a stream seeds its own draws.
    seed: u64,
}

impl EpochRecords {
    /// The records of `files`, labelled at every one of `lambdas` in that order, `emitted` positions
    /// into the run (`training_batch_size * step_count`).
    pub fn open(files: Vec<StreamFile>, lambdas: &[f64], emitted: u64, threads: usize) -> EpochRecords {
        assert!(!files.is_empty(), "the training stream has no files");
        let epoch_positions: u64 = files.iter().map(|file| file.positions).sum();
        assert!(epoch_positions > 0, "the training files hold no positions");
        let epoch = emitted / epoch_positions;
        let into_epoch = emitted % epoch_positions;
        // A resumed epoch gets a seed of its own, so its order is not the interrupted one.
        let seed = if into_epoch == 0 { epoch_seed(epoch) } else { splitmix64(epoch_seed(epoch) ^ splitmix64(emitted)) };
        if into_epoch > 0 {
            log::info!(
                "resuming epoch {} at {into_epoch} of {epoch_positions} positions: the rest of it is read in a fresh order",
                epoch + 1
            );
        }
        let stream = start_epoch(&files, lambdas, threads, seed);
        EpochRecords {
            files,
            lambdas: lambdas.to_vec(),
            threads,
            epoch_positions,
            epoch,
            left_in_epoch: epoch_positions - into_epoch,
            stream,
            seed,
        }
    }

    /// A seed for a stream's own draws, distinct for each `salt`, derived from where the run stands.
    pub fn draw_seed(&self, salt: u64) -> u64 {
        splitmix64(self.seed ^ salt)
    }

    #[cfg(test)]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn epoch_positions(&self) -> u64 {
        self.epoch_positions
    }

    /// The next record read from the files, starting the next epoch when this one is done.
    pub fn next_record(&mut self) -> CacheRecord {
        let mut fresh = false;
        loop {
            if self.left_in_epoch > 0
                && let Some(record) = self.stream.next()
            {
                self.left_in_epoch -= 1;
                return record;
            }
            // `.count` files that overcount end an epoch early; one that yields nothing at all is a
            // corpus with no games, which would otherwise loop here forever.
            assert!(!fresh, "a whole epoch of the training files yielded no position");
            self.epoch += 1;
            self.left_in_epoch = self.epoch_positions;
            // Replacing the stream drops the old one, which stops and joins its workers.
            self.stream = start_epoch(&self.files, &self.lambdas, self.threads, epoch_seed(self.epoch));
            fresh = true;
        }
    }
}

#[cfg(test)]
pub(crate) struct PgnTrainStream {
    records: EpochRecords,
    buffer: Vec<CacheRecord>,
    rng: StdRng,
    /// Whether a drawn slot is refilled with the next record read (training's endless stream), or the
    /// buffer drains (`open_one_pass`).
    refill: bool,
}

#[cfg(test)]
impl PgnTrainStream {
    /// Opens the stream over `files`, labelled at every one of `lambdas` in that order, `emitted`
    /// positions into the run (`training_batch_size * step_count`), and fills a buffer of
    /// `buffer_capacity` records up front, so the fill reports as its own step.
    pub fn open(
        files: Vec<StreamFile>, lambdas: &[f64], buffer_capacity: usize, emitted: u64, threads: usize,
    ) -> PgnTrainStream {
        assert!(buffer_capacity > 0, "a shuffle buffer holds at least one record");
        let mut records = EpochRecords::open(files, lambdas, emitted, threads);
        let rng = StdRng::seed_from_u64(records.draw_seed(1));
        let mut buffer = Vec::with_capacity(buffer_capacity);
        while buffer.len() < buffer_capacity {
            buffer.push(records.next_record());
        }
        PgnTrainStream { records, buffer, rng, refill: true }
    }

    /// One epoch of `files` read whole into the buffer, then drawn without refilling, so every
    /// position comes out exactly once in a uniformly random order (a Fisher-Yates shuffle, drawn as
    /// it goes): what the tests check a whole epoch against. The refilling buffer cannot give that even when it
    /// holds the whole epoch: each draw's slot takes the next epoch's record, so over one epoch's draws
    /// a position comes out with probability about 1 - 1/e, some twice, some never. `next_batch` is
    /// short at the pass's end and empty after it.
    #[cfg(test)]
    pub fn open_one_pass(files: Vec<StreamFile>, lambdas: &[f64], threads: usize) -> PgnTrainStream {
        let mut records = EpochRecords::open(files, lambdas, 0, threads);
        let rng = StdRng::seed_from_u64(records.draw_seed(1));
        let positions = records.epoch_positions() as usize;
        let mut buffer = Vec::with_capacity(positions);
        while buffer.len() < positions {
            buffer.push(records.next_record());
        }
        PgnTrainStream { records, buffer, rng, refill: false }
    }

    #[cfg(test)]
    pub fn buffer_capacity(&self) -> usize {
        self.buffer.len()
    }
}

fn epoch_seed(epoch: u64) -> u64 {
    splitmix64(EPOCH_SEED ^ epoch)
}

/// A `PgnStream` over `files` in an order shuffled under `seed`.
fn start_epoch(files: &[StreamFile], lambdas: &[f64], threads: usize, seed: u64) -> PgnStream {
    let mut order = files.to_vec();
    let mut rng = StdRng::seed_from_u64(seed);
    for index in (1..order.len()).rev() {
        let other = rng.random_range(0..=index);
        order.swap(index, other);
    }
    // The searched positions, as `segment_files` counts them: never a game's final position.
    // Lc0's curve, its corpus rewritten into it (`lc0::rewrite`; training refuses the corpus before):
    // no transformer is trained now, and a self-play teacher's curve would reach it as the NNUE
    // forges' scatter takes it, when one is.
    let options = StreamOptions {
        lambdas: lambdas.to_vec(),
        threads,
        open_files: OPEN_FILES.max(threads),
        keep_probability: 1.0,
        seed,
        tablebase: None,
        curve: temper_chess::labels::Curve::Atan(*temper_chess::labels::LC0),
        // The transformer's stream has no job stop to hand: a read waits until it succeeds.
        stop: None,
    };
    PgnStream::start(order, options)
}

#[cfg(test)]
impl TrainStream for PgnTrainStream {
    /// Never short when refilling: every pick is replaced by the next record read, and the epochs
    /// never end. Draining (`open_one_pass`), short at the pass's end.
    fn next_batch(&mut self, batch_size: usize) -> Vec<CacheRecord> {
        let mut batch = Vec::with_capacity(batch_size);
        if !self.refill {
            while batch.len() < batch_size && !self.buffer.is_empty() {
                let slot = self.rng.random_range(0..self.buffer.len());
                batch.push(self.buffer.swap_remove(slot));
            }
            return batch;
        }
        for _ in 0..batch_size {
            let slot = self.rng.random_range(0..self.buffer.len());
            let next = self.records.next_record();
            batch.push(std::mem::replace(&mut self.buffer[slot], next));
        }
        batch
    }

    /// Deliberately empty: the position is derived from the step count, and persisting a cursor beside
    /// it would be a second counter that can only disagree with it.
    fn cursors(&self) -> Vec<u64> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two games of different lengths and scores, 3 and 4 positions, 2 and 3 of them searched.
    const GAMES: &str = concat!(
        "[Result \"1-0\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Kd2 {1.50} Kd7 {-1.20}\n1-0\n\n",
        "[Result \"0-1\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Ke2 {-0.40} Ke7 {0.30} 2. Kf2 {-0.90}\n0-1\n\n",
    );

    fn fixture(name: &str, pairs: &[u64]) -> (String, Vec<StreamFile>) {
        let dir = std::env::temp_dir().join(format!("smithy-pgn-train-stream-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("failed to create the fixture directory");
        let dir = dir.to_str().unwrap().replace('\\', "/");
        let files = pairs
            .iter()
            .enumerate()
            .map(|(index, &count)| {
                let path = format!("{dir}/gamma-{index}.pgn.zst");
                let compressed = zstd::encode_all(GAMES.repeat(count as usize).as_bytes(), 3).expect("failed to compress");
                std::fs::write(&path, compressed).expect("failed to write the fixture");
                StreamFile { path, positions: count * 5 }
            })
            .collect();
        (dir, files)
    }

    #[test]
    fn the_stream_never_ends_and_every_batch_is_full() {
        let (dir, files) = fixture("endless", &[2, 3]);
        // 25 positions an epoch: 20 batches of 8 cross four epochs and more.
        let mut stream = PgnTrainStream::open(files, &[0.7, 1.0], 10, 0, 2);
        assert_eq!((stream.records.epoch_positions(), stream.buffer_capacity()), (25, 10));
        for _ in 0..20 {
            let batch = stream.next_batch(8);
            assert_eq!(batch.len(), 8);
            assert!(batch.iter().all(|record| record.labels.len() == 2), "one label per lambda");
        }
        assert!(stream.records.epoch() >= 4, "epoch {}", stream.records.epoch());
        assert!(stream.cursors().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_epoch_reads_every_position_once() {
        let (dir, files) = fixture("epoch", &[1, 4, 2]);
        // A buffer of 1 passes every record straight through: the first 35 out are one epoch.
        let mut stream = PgnTrainStream::open(files, &[1.0], 1, 0, 2);
        let mut labels: Vec<f32> = (0..35).map(|_| stream.next_batch(1)[0].labels[0]).collect();
        assert_eq!(stream.records.epoch(), 1, "the buffer already holds the next epoch's first record, not yet out");
        labels.sort_by(f32::total_cmp);
        // At lambda 1 every label is the game's result from the side to move: of 7 copies of each
        // game, the 1-0 game's searched positions give +,- and the 0-1 game's -,+,- (White moves
        // first in both; the final positions left out).
        let positive = labels.iter().filter(|&&label| label > 0.0).count();
        assert_eq!((labels.len(), positive), (35, 7 + 7));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn one_pass_gives_every_position_exactly_once_then_nothing() {
        let (dir, files) = fixture("one-pass", &[1, 4, 2]);
        let mut stream = PgnTrainStream::open_one_pass(files, &[1.0, 0.0], 2);
        assert_eq!(stream.buffer_capacity(), 35);
        let mut records = Vec::new();
        loop {
            let batch = stream.next_batch(10);
            if batch.is_empty() {
                break;
            }
            assert!(batch.len() == 10 || records.len() == 30, "only the last batch is short");
            records.extend(batch);
        }
        // As `an_epoch_reads_every_position_once`: at lambda 1 the labels are the results, 14 of 35
        // positive; at lambda 0 the direct blend is each position's own score.
        let positive = records.iter().filter(|record| record.labels[0] > 0.0).count();
        assert_eq!((records.len(), positive), (35, 14));
        let mut scores: Vec<f32> = records.iter().map(|record| record.labels[1]).collect();
        scores.sort_by(f32::total_cmp);
        scores.dedup();
        assert!(scores.len() >= 5, "the five annotated scores, at least, come through: {scores:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_resume_reads_only_the_rest_of_its_epoch() {
        let (dir, files) = fixture("resume", &[2, 3]);
        // 25 an epoch; 40 emitted is epoch 1, 15 into it, so 10 are left before epoch 2.
        let stream = PgnTrainStream::open(files, &[1.0], 5, 40, 1);
        assert_eq!(stream.records.epoch(), 1);
        assert_eq!(stream.records.left_in_epoch, 10 - 5, "the fill took 5 of the 10");
        std::fs::remove_dir_all(&dir).ok();
    }
}
