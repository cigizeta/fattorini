//! The transformer's training stream: the scattered buckets, fully shuffled as an NNUE reads them
//! (`buckets::BucketStream`), grouped online by piece count, since the transformer cannot pad and
//! every batch must hold positions of one length.
//!
//! **Groups.** A record goes to the group of its count of pieces beside the kings, 1 to 30; K vs K
//! is skipped. A group reaching the batch size is emitted whole, in arrival order, and starts empty
//! again. No random draw: the buckets' order is already a uniform shuffle, and grouping only reorders
//! it.
//!
//! **Epochs.** After every committed record once, the epoch ends and every group is cleared: a
//! piece count's last, partial batch is dropped. Each epoch holds the same records, so each piece
//! count gives the same full batches every epoch, and the batches of an epoch are a constant
//! (`buckets::PieceCounts::batches_per_epoch`).
//!
//! **Resume.** From the step count: the epoch is `steps / batches_per_epoch`, and the batches done
//! in it are replayed, grouped and thrown away, from the epoch's start. The grouping depends only on
//! the epoch's seed, so the batch returned next is the one an unbroken run trained next. Nothing is
//! persisted beyond the step count.

use super::buckets::{self, BucketStream, Labelling, PieceCounts, PIECE_COUNTS};
use super::cache::CacheRecord;
use super::train_stream::TrainStream;

pub(crate) struct BucketedTrainStream {
    stream: BucketStream,
    batch_size: usize,
    /// `groups[n]` holds the records with `n` pieces beside the kings; `groups[0]` stays empty.
    groups: Vec<Vec<CacheRecord>>,
    /// The records of an epoch, and those read so far in the current one.
    records_per_epoch: u64,
    read_in_epoch: u64,
}

impl BucketedTrainStream {
    /// `dir`'s buckets labelled by `labelling`, `counts` their piece counts, in batches of
    /// `batch_size`, `steps_done` batches into the run.
    pub fn open(dir: &str, labelling: Labelling, counts: &PieceCounts, batch_size: usize, steps_done: u64) -> Result<BucketedTrainStream, String> {
        assert!(batch_size > 0, "a batch holds at least one record");
        let records_per_epoch = buckets::committed_records(dir)?;
        if counts.records() != records_per_epoch {
            return Err(format!("{dir}: piece counts of {} records, but the buckets hold {records_per_epoch}", counts.records()));
        }
        let batches_per_epoch = counts.batches_per_epoch(batch_size as u64);
        if batches_per_epoch == 0 {
            return Err(format!("{dir}: no piece count holds a batch of {batch_size}"));
        }
        let epoch = steps_done / batches_per_epoch;
        let replay = steps_done % batches_per_epoch;
        let stream = BucketStream::open(dir, labelling, epoch * records_per_epoch)?;
        let mut grouped = BucketedTrainStream {
            stream,
            batch_size,
            groups: vec![Vec::new(); PIECE_COUNTS],
            records_per_epoch,
            read_in_epoch: 0,
        };
        if replay > 0 {
            log::info!("resuming in epoch {epoch}: replaying its first {replay} batches, untrained, to reach the batch it stopped before");
            for _ in 0..replay {
                grouped.next_batch(batch_size);
            }
        }
        Ok(grouped)
    }
}

impl TrainStream for BucketedTrainStream {
    /// Never short: a group gives a batch only once it holds `batch_size` records.
    fn next_batch(&mut self, batch_size: usize) -> Vec<CacheRecord> {
        assert_eq!(batch_size, self.batch_size, "the stream was opened for batches of {}", self.batch_size);
        loop {
            if self.read_in_epoch == self.records_per_epoch {
                self.groups.iter_mut().for_each(Vec::clear);
                self.read_in_epoch = 0;
            }
            let record = self.stream.next_record();
            self.read_in_epoch += 1;
            let pieces = record.canonical.pieces.len();
            assert!(pieces < PIECE_COUNTS, "a position with {pieces} pieces beside the kings");
            if pieces == 0 {
                continue;
            }
            let group = &mut self.groups[pieces];
            group.push(record);
            if group.len() == batch_size {
                return std::mem::replace(group, Vec::with_capacity(batch_size));
            }
        }
    }

    /// Deliberately empty: the position is derived from the step count.
    fn cursors(&self) -> Vec<u64> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pgn_stream::StreamFile;
    use fattorini::pipeline::preprocessing_config::ArmLambda;
    use std::sync::atomic::AtomicBool;

    /// Three games: a rook alone beside the kings (2 searched positions), a rook and a knight (3), and
    /// the kings alone (2, K vs K). 10 positions a copy, 7 of them searched.
    const GAMES: &str = concat!(
        "[Result \"1-0\"]\n[FEN \"4k3/8/8/8/8/8/8/R3K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Kd2 {1.50} Kd7 {-1.20}\n1-0\n\n",
        "[Result \"0-1\"]\n[FEN \"4k3/8/8/8/8/8/8/RN2K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Ke2 {-0.40} Ke7 {0.30} 2. Kf2 {-0.90}\n0-1\n\n",
        "[Result \"1/2-1/2\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Kd2 {0.00} Kd7 {0.00}\n1/2-1/2\n\n",
    );

    /// A scratch directory with `copies[i]` copies of `GAMES` in file `i`, scattered into its
    /// `buckets`, and their piece counts.
    fn fixture(name: &str, copies: &[u64]) -> (String, String, PieceCounts) {
        let dir = std::env::temp_dir().join(format!("smithy-bucketed-train-stream-test-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("failed to create the fixture directory");
        let dir = dir.to_str().unwrap().replace('\\', "/");
        let files: Vec<StreamFile> = copies
            .iter()
            .enumerate()
            .map(|(index, &count)| {
                let path = format!("{dir}/gamma-{index}.pgn.zst");
                std::fs::write(&path, zstd::encode_all(GAMES.repeat(count as usize).as_bytes(), 3).unwrap()).unwrap();
                StreamFile { path, positions: count * 10 }
            })
            .collect();
        let bucket_dir = format!("{dir}/buckets");
        buckets::prepare_for_tests(&bucket_dir, &files, 2);
        let counts = buckets::ensure_piece_counts(&bucket_dir, 2, &AtomicBool::new(false), &|_| {}).unwrap().unwrap();
        (dir, bucket_dir, counts)
    }

    fn labelling() -> Labelling {
        Labelling::new(&[ArmLambda::Constant(0.5)], None).unwrap()
    }

    /// A batch as a comparable key: its positions' bytes and its labels' bits.
    fn key(batch: &[CacheRecord]) -> Vec<(Vec<u8>, Vec<u32>)> {
        batch
            .iter()
            .map(|record| {
                let mut bytes = Vec::new();
                crate::cache::write_position(&mut bytes, &record.canonical);
                (bytes, record.labels.iter().map(|label| label.to_bits()).collect())
            })
            .collect()
    }

    #[test]
    fn every_batch_is_full_of_one_piece_count_and_never_k_vs_k() {
        let (dir, buckets, counts) = fixture("one-length", &[3, 4]);
        let mut stream = BucketedTrainStream::open(&buckets, labelling(), &counts, 4, 0).unwrap();
        for _ in 0..100 {
            let batch = stream.next_batch(4);
            assert_eq!(batch.len(), 4);
            let pieces = batch[0].canonical.pieces.len();
            assert!(pieces > 0, "a K vs K batch");
            assert!(batch.iter().all(|record| record.canonical.pieces.len() == pieces), "a batch mixes piece counts");
        }
        assert!(stream.cursors().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_epoch_gives_its_full_batches_and_drops_the_partial_ones() {
        // 5 copies: 10 positions of a rook alone, 15 of a rook and a knight, 10 of K vs K, 35 an epoch.
        let (dir, buckets, counts) = fixture("epoch", &[2, 3]);
        assert_eq!(counts.batches_per_epoch(4), 5);
        let mut stream = BucketedTrainStream::open(&buckets, labelling(), &counts, 4, 0).unwrap();
        // Each epoch: 2 batches of a rook alone, 3 of a rook and a knight, whatever the order.
        for epoch in 0..3 {
            let mut lengths: Vec<usize> = (0..5).map(|_| stream.next_batch(4)[0].canonical.pieces.len()).collect();
            lengths.sort();
            assert_eq!(lengths, [1, 1, 2, 2, 2], "epoch {epoch}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_resume_returns_the_batch_an_unbroken_run_returned() {
        let (dir, buckets, counts) = fixture("resume", &[2, 3]);
        let mut whole = BucketedTrainStream::open(&buckets, labelling(), &counts, 4, 0).unwrap();
        let run: Vec<_> = (0..17).map(|_| key(&whole.next_batch(4))).collect();
        // 5 batches an epoch: step 0, the end of an epoch (10), and inside one (7, 13).
        for steps_done in [0u64, 7, 10, 13] {
            let mut resumed = BucketedTrainStream::open(&buckets, labelling(), &counts, 4, steps_done).unwrap();
            let rest: Vec<_> = (steps_done..17).map(|_| key(&resumed.next_batch(4))).collect();
            assert_eq!(rest, run[steps_done as usize..], "resumed at step {steps_done}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_batch_no_piece_count_fills_is_refused() {
        let (dir, buckets, counts) = fixture("too-large", &[1]);
        // 2 of a rook alone, 3 of a rook and a knight: no batch of 4.
        let error = BucketedTrainStream::open(&buckets, labelling(), &counts, 4, 0).err().expect("refused");
        assert!(error.contains("no piece count holds a batch of 4"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
