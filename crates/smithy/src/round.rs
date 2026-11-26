//! The one shared training-round loop for every architecture this project trains. Originally
//! written once for NNUE (reading one flat cache file cyclically), then duplicated a second time
//! for the transformer (reading 31 length-bucketed files via a weighted-random pick), then merged
//! when NNUE moved onto that same length-bucketed format -- and now merged again, in the other
//! direction, after NNUE moved back to a flat cache read through a shuffle buffer
//! (`dataset_cache.rs` covers why: a bucketed read makes every NNUE batch materially homogeneous).
//!
//! Two differences between the tracks survive, and both are parameters rather than duplicated code:
//! how a batch of raw records becomes a loss tensor (`to_batch` plus the `Trainee`'s own losses), and
//! where the batches come from (`train_stream::TrainStream`). Everything else -- the always-at-least-one-batch loop,
//! checking the real step count against `steps_per_round` directly rather than a separate counter,
//! cross-call duration accumulation, backup/stop handling -- lives here once, for both tracks.
//!
//! The no-grad sweeps below are generic over `train_stream::BatchSource` for the same reason: both
//! cache layouts can be swept once in order, so the sweep is written once.
//!
//! One network trains at a time (2026-11-26): the lockstep of a grid's lambdas, several networks
//! stepping on each batch, went with the grid.

use std::collections::BTreeMap;
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tch::{Kind, Tensor};

use super::cache::CacheRecord;
use super::pipeline;
use super::test_cache::{Position, TestCache};
use super::train_stream::{BatchSource, TrainStream};

/// The network a round trains, whatever holds it: training's (`training::OneArm`) or the tests'.
///
/// Losses come back as tensors left on the device, so the loop sums them there and reads them only
/// at a backup and at the end of the round (`run_training_steps`).
pub trait Trainee<B> {
    /// The cumulative step count: where the network stands in its sequence of batches.
    fn step_count(&self) -> u64;

    /// One training step on `batch`. Returns the training loss on `batch`, a detached scalar.
    fn train_step(&mut self, batch: &B) -> Tensor;

    /// The test loss on `batch`, a scalar. Called under `no_grad`.
    fn test_loss(&self, batch: &B) -> Tensor;

    /// Persists the network's state, with what a mid-round resume needs of its round: the stream's
    /// cursors, its rows so far, its loss sum and the round's training seconds.
    fn back_up(&mut self, cursors: &[u64], rows_done: u64, loss_sum: f64, duration_secs: f64);
}

/// How a batch of records becomes what the network reads, in two halves (`pipeline.rs`): `prepare`
/// is all the CPU work and runs on worker threads, several batches at once; `upload` makes the
/// tensors and runs on the main thread, in order, so nothing touching the GPU leaves it. `upload`
/// borrows the prepared batch, so a test batch kept across rounds (`test_cache.rs`) is uploaded again
/// rather than prepared again.
pub struct BatchBuilder<P, U> {
    pub prepare: P,
    pub upload: U,
}

/// What one completed round reports.
pub struct RoundOutcome {
    pub train_loss: f64,
    pub test_loss: f64,
    /// The test loss per piece count (`TestLosses::by_pieces`).
    pub test_loss_by_pieces: BTreeMap<usize, f64>,
    /// Read by the tests alone: training derives the stream's position from the step count.
    #[cfg_attr(not(test), allow(dead_code))]
    pub cursors: Vec<u64>,
    pub train_duration_secs: f64,
    pub test_duration_secs: f64,
}

/// What a test pass measured: the mean over every row, and the same over the rows of each piece
/// count, kings included, of the batches holding a single one (`piece_count_of`).
pub struct TestLosses {
    pub global: f64,
    pub by_pieces: BTreeMap<usize, f64>,
}

/// The piece count of `records`, kings included, when every one has the same, and `None` for a
/// mixed batch, which counts toward the global loss only. The frozen test sets are read grouped by
/// piece count (`grouped_test_set`), so their batches are never mixed.
pub fn piece_count_of(records: &[CacheRecord]) -> Option<usize> {
    let pieces = records.first()?.canonical.pieces.len();
    if records.iter().all(|record| record.canonical.pieces.len() == pieces) {
        return Some(pieces + 2);
    }
    None
}

/// Where an interrupted round resumes: the rows already trained on in the current round and its
/// loss sum so far; and the training seconds already spent.
pub struct RoundResume {
    pub rows_done: u64,
    pub loss_sum: f64,
    pub duration_secs: f64,
}

/// Runs one evaluation round: training batches from `train_stream` until the trainee's cumulative
/// step count reaches the next multiple of `steps_per_round`, then one full no-grad pass over every
/// row of every one of `test_readers`. `to_batch(records)` builds whichever batch type the network
/// reads. Returns `None` if `stop_requested` fired mid-round.
///
/// `on_progress`'s train call reports the real, absolute step count, not a row count and not a
/// count relative to this round: a row count would be an arbitrary product with no connection to
/// the real dataset size or to the training run's own actual length, and a round-relative count
/// would go stale the moment `steps_per_round` ever changes between two runs of the same job --
/// exactly the failure `run_training_steps` was rewritten to avoid (see this module's own git
/// history/the implementation notes for the bug that motivated it).
///
/// `resume`'s third field and the returned `train_duration_secs` are this round's own accumulated
/// training wall-clock cost -- time only counted while a process was actually running it, never
/// idle time between a `Ctrl+C` and a later resume, summed across however many separate
/// invocations it took to actually finish the training half. `test_duration_secs` is simpler: the
/// test pass is never interrupted mid-sweep, so it's timed here, once, with no resume bookkeeping
/// of its own. Kept as two separate returned values rather than one summed total -- the two
/// phases' very different row counts make a combined figure useless for computing either one's
/// own throughput.
///
/// `test_cache`, when given, is the run's kept test batches (`test_cache.rs`): filled by the first
/// round's test pass, read by every later one.
#[allow(clippy::too_many_arguments)]
pub fn run_round<B, H: Send, T: BatchSource + Send>(
    trainee: &mut impl Trainee<B>, train_stream: &mut (impl TrainStream + Send), test_readers: &mut [T],
    test_cache: Option<&mut TestCache<H>>, steps_per_round: u64, batch_size: usize, resume: Option<RoundResume>,
    backup_interval: Duration, to_batch: &BatchBuilder<impl Fn(&[CacheRecord]) -> H + Sync, impl Fn(&H) -> B>,
    on_progress: &mut impl FnMut(&str, u64, u64), stop_requested: &AtomicBool,
) -> Option<RoundOutcome> {
    let resume = resume.unwrap_or(RoundResume { rows_done: 0, loss_sum: 0.0, duration_secs: 0.0 });
    let (train_loss, cursors, train_duration_secs) = run_training_steps(
        trainee, train_stream, steps_per_round, batch_size, resume, backup_interval, to_batch,
        &mut |absolute_step| on_progress("train", absolute_step, steps_per_round), stop_requested,
    )?;

    let (test_losses, test_duration_secs) = run_test_round(trainee, test_readers, test_cache, batch_size, to_batch, on_progress);

    Some(RoundOutcome {
        train_loss,
        test_loss: test_losses.global,
        test_loss_by_pieces: test_losses.by_pieces,
        cursors,
        train_duration_secs,
        test_duration_secs,
    })
}

/// A test pass alone, with no training before it: `run_round`'s test half, for the round at step 0
/// that measures the generation's starting weights, every run's first best. Returns the test losses
/// and the pass's seconds.
pub fn run_test_round<B, H: Send, T: BatchSource + Send>(
    trainee: &impl Trainee<B>, test_readers: &mut [T], test_cache: Option<&mut TestCache<H>>, batch_size: usize,
    to_batch: &BatchBuilder<impl Fn(&[CacheRecord]) -> H + Sync, impl Fn(&H) -> B>,
    on_progress: &mut impl FnMut(&str, u64, u64),
) -> (TestLosses, f64) {
    let test_start = Instant::now();
    let test_loss = tch::no_grad(|| {
        run_test_pass(trainee, test_readers, test_cache, batch_size, to_batch, &mut |processed, total| {
            on_progress("test", processed, total)
        })
    });
    (test_loss, test_start.elapsed().as_secs_f64())
}

/// The training half of one evaluation round: runs batches of up to `batch_size` rows (the
/// bucketed stream can return fewer, whenever the bucket it picked runs low -- "no padding, ever"),
/// stepping the trainee after every batch, until its step count itself lands on a multiple of
/// `steps_per_round` -- always at least one batch, even if the count already sits on a multiple at
/// entry (otherwise a round that just completed, or a resumed run started exactly at a boundary,
/// would "complete" a second round for free with no training at all). Checking the real, persisted,
/// ever-increasing step count directly, rather than a separate counter that only ever tracked "how
/// far into a round of exactly this size", means a `steps_per_round` that changes between two runs
/// of the same job can never desynchronize this decision. Returns `Some((train_loss, cursors,
/// duration_secs))` for a run that reached that boundary, or `None` if `stop_requested` cut it short
/// instead.
///
/// `resume.duration_secs`/the returned `duration_secs` accumulate this round's own training time
/// (never test-pass time) across however many separate calls it takes to actually reach the
/// boundary: `call_start` only ever measures this one call's own elapsed wall-clock time, added on
/// top of whatever earlier, now-finished calls already contributed -- so a round interrupted by
/// `Ctrl+C` and resumed later never loses the time already spent on it, and never counts the idle
/// time between the interruption and the resume either.
///
/// `stream` carries its own cursors and its own randomness, whichever layout it reads: the bucketed
/// stream's weighted bucket pick needs no resume bookkeeping (its weights are recomputed fresh from
/// the cursors every call), and the shuffled stream's buffer contents are deliberately not
/// persisted, so in both cases the cursors this returns are the whole of what a resume needs.
///
/// The batches are read, prepared and consumed through `pipeline::pipelined`: read in order on one
/// thread, prepared on every other core, stepped on here in the order they were read. The reader
/// reads exactly the batches this round still needs, one per step up to the boundary, so the stream
/// ends the round where a one-thread loop would have, and the next round continues from there. Each
/// batch carries the cursors read right after it, as the one-thread loop read them.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn run_training_steps<B, H: Send>(
    trainee: &mut impl Trainee<B>, stream: &mut (impl TrainStream + Send), steps_per_round: u64,
    batch_size: usize, resume: RoundResume, backup_interval: Duration,
    to_batch: &BatchBuilder<impl Fn(&[CacheRecord]) -> H + Sync, impl Fn(&H) -> B>, on_progress: &mut impl FnMut(u64),
    stop_requested: &AtomicBool,
) -> Option<(f64, Vec<u64>, f64)> {
    let RoundResume { mut rows_done, mut loss_sum, duration_secs: initial_duration_secs } = resume;
    let start_step = trainee.step_count();
    let mut last_backup = Instant::now();
    let call_start = Instant::now();
    // At least one, even on a boundary already: see above.
    let batches_needed = steps_per_round - start_step % steps_per_round;
    let mut batches_read = 0u64;
    let source = || {
        if batches_read == batches_needed {
            return None;
        }
        batches_read += 1;
        let records = stream.next_batch(batch_size);
        let cursors = stream.cursors();
        Some((records, cursors))
    };
    // Only the CPU half goes to the workers; the upload half stays on this thread.
    let prepare_records = &to_batch.prepare;
    let prepare = |(records, cursors): (Vec<CacheRecord>, Vec<u64>)| {
        (prepare_records(&records), records.len() as u64, cursors)
    };
    // **The loss stays on the device until it is needed.** Reading it every step (`f64::try_from`)
    // makes this thread wait for the GPU to finish the step before it can queue the next, so the two
    // never overlap; for a small network, whose step is a few dozen tiny kernels, that wait is most
    // of the step. The weighted losses are summed on the device instead and read only at a backup
    // and at the end of the round, the two places that need the number.
    let mut unread_loss: Option<Tensor> = None;
    let mut finished = None;
    pipeline::pipelined(pipeline::worker_count(), source, prepare, |(prepared, rows_this_batch, cursors)| {
        let batch = (to_batch.upload)(&prepared);
        rows_done += rows_this_batch;
        // In double, as the loss sum is: a round sums thousands of batches.
        let weighted = trainee.train_step(&batch).to_kind(Kind::Double) * rows_this_batch as f64;
        unread_loss = Some(match unread_loss.take() {
            Some(sum) => sum + weighted,
            None => weighted,
        });
        let step = trainee.step_count();
        on_progress(step);

        let duration_secs = initial_duration_secs + call_start.elapsed().as_secs_f64();
        let stop = stop_requested.load(Ordering::SeqCst);
        if stop || last_backup.elapsed() >= backup_interval {
            loss_sum += read_loss(&mut unread_loss);
            trainee.back_up(&cursors, rows_done, loss_sum, duration_secs);
            last_backup = Instant::now();
        }
        if stop {
            return ControlFlow::Break(());
        }
        if step % steps_per_round == 0 {
            loss_sum += read_loss(&mut unread_loss);
            let train_loss = if rows_done == 0 { f64::NAN } else { loss_sum / rows_done as f64 };
            finished = Some((train_loss, cursors, duration_secs));
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    });
    finished
}

/// The loss summed on the device since the last read, `0` if none, and the sum cleared: the one
/// place a training round waits for the GPU (`run_training_steps`).
fn read_loss(unread_loss: &mut Option<Tensor>) -> f64 {
    unread_loss.take().map_or(0.0, |sum| f64::try_from(&sum.to_device(tch::Device::Cpu)).expect("a scalar loss sum"))
}

/// One full, deterministic, no-grad pass over every row of every one of `readers`, each swept in
/// file order -- no random pick and no cursor to maintain across calls: every reader with any rows
/// at all is read from its own row `0` through exactly one wraparound (`read_batch_stop_short`
/// always returns cursor `0` once its rows are exhausted), which is precisely one full sweep, no
/// more and no fewer. The NNUE track passes a one-element slice holding the flat test cache; the
/// transformer track passes one reader per piece-count bucket per stem. `on_progress` reports
/// against the grand total across every reader combined. The mean over every row is returned, with
/// the mean over the rows of each piece count, from the batches holding a single one
/// (`piece_count_of`).
///
/// Pipelined as the training half is (`pipeline::pipelined`): the sweep is read in order on one
/// thread and its batches prepared on every other core.
///
/// With a `cache` (`test_cache.rs`), the first pass offers it every batch as it is consumed, each
/// with the position it starts at, and the cache keeps a prefix; every later pass uploads the kept
/// batches first, then reads and prepares only the rest, from where the cache says it starts. Either
/// way the batches reach the loss in the same order, so the result is the same.
fn run_test_pass<B, H: Send, T: BatchSource + Send>(
    trainee: &impl Trainee<B>, readers: &mut [T], mut cache: Option<&mut TestCache<H>>, batch_size: usize,
    to_batch: &BatchBuilder<impl Fn(&[CacheRecord]) -> H + Sync, impl Fn(&H) -> B>, on_progress: &mut impl FnMut(u64, u64),
) -> TestLosses {
    let total: u64 = readers.iter().map(|reader| reader.row_count()).sum();
    let reader_count = readers.len();
    let mut processed = 0u64;
    // Summed on the device and read once at the end, as in `run_training_steps`; the same per piece
    // count, with the rows each covers.
    let mut unread_loss: Option<Tensor> = None;
    let mut unread_by_pieces: BTreeMap<usize, (Option<Tensor>, u64)> = BTreeMap::new();
    let mut add_loss = |prepared: &H, rows_this_batch: u64, pieces: Option<usize>| {
        // Each piece count's sum keeps the rows it covers, to divide by at the end.
        let batch = (to_batch.upload)(prepared);
        let weighted = trainee.test_loss(&batch).to_kind(Kind::Double) * rows_this_batch as f64;
        if let Some(pieces) = pieces {
            let (sum, rows) = unread_by_pieces.entry(pieces).or_insert((None, 0));
            *sum = Some(match sum.take() {
                Some(sum) => sum + &weighted,
                None => weighted.shallow_clone(),
            });
            *rows += rows_this_batch;
        }
        unread_loss = Some(match unread_loss.take() {
            Some(sum) => sum + weighted,
            None => weighted,
        });
        processed += rows_this_batch;
        on_progress(processed, total);
    };

    // The kept batches first, and the read starts where they end.
    let mut start = Position { reader: 0, row: 0 };
    if let Some(rest) = cache.as_deref().and_then(|cache| cache.rest()) {
        for (prepared, rows_this_batch, pieces) in cache.as_deref().expect("a rest implies a cache").batches() {
            add_loss(prepared, *rows_this_batch, *pieces);
        }
        start = rest;
    }
    // Only the first pass offers its batches; later ones read what was not kept and drop it.
    let filling = cache.as_deref().is_some_and(|cache| !cache.is_filled());

    let mut reader_index = start.reader;
    let mut cursor = start.row;
    let source = || loop {
        let reader = readers.get_mut(reader_index)?;
        if reader.row_count() == 0 {
            reader_index += 1;
            continue;
        }
        let batch_start = Position { reader: reader_index, row: cursor };
        let (records, next_cursor) = reader.read_batch_stop_short(cursor, batch_size);
        cursor = next_cursor;
        if cursor == 0 {
            // `read_batch_stop_short` only ever returns `0` by wrapping: this bucket's one full sweep
            // is done.
            reader_index += 1;
        }
        return Some((records, batch_start));
    };
    let prepare_records = &to_batch.prepare;
    let prepare = |(records, batch_start): (Vec<CacheRecord>, Position)| {
        (prepare_records(&records), records.len() as u64, piece_count_of(&records), batch_start)
    };
    pipeline::pipelined(pipeline::worker_count(), source, prepare, |(prepared, rows_this_batch, pieces, batch_start)| {
        add_loss(&prepared, rows_this_batch, pieces);
        if filling
            && let Some(cache) = cache.as_deref_mut() {
                cache.offer(prepared, rows_this_batch, pieces, batch_start);
            }
        ControlFlow::Continue(())
    });
    if filling
        && let Some(cache) = cache {
            cache.finish_filling(Position { reader: reader_count, row: 0 });
        }
    let global = read_loss(&mut unread_loss) / total.max(1) as f64;
    let by_pieces = unread_by_pieces
        .into_iter()
        .map(|(pieces, (mut sum, rows))| (pieces, read_loss(&mut sum) / rows.max(1) as f64))
        .collect();
    TestLosses { global, by_pieces }
}

#[cfg(test)]
#[allow(clippy::type_complexity)]
mod tests {
    use super::*;
    use crate::adamw::AdamW;
    use fattorini::board::castling_rights::CastlingRights;
    use fattorini::board::color::Color;
    use fattorini::board::square::Square;
    use tch::{nn, Reduction};

    const TEST_BATCH_SIZE: usize = 2;

    /// A network the tests train: its optimizer, the closure that turns a batch into its loss, and
    /// the closure called at a backup.
    struct Probe<'a, B> {
        optimizer: &'a mut AdamW,
        loss: Box<dyn Fn(&B) -> Tensor + 'a>,
        on_backup: Box<dyn FnMut(&AdamW, &[u64], u64, f64, f64) + 'a>,
    }

    impl<B> Trainee<B> for Probe<'_, B> {
        fn step_count(&self) -> u64 {
            self.optimizer.step_count() as u64
        }

        fn train_step(&mut self, batch: &B) -> Tensor {
            let value = (self.loss)(batch);
            self.optimizer.zero_grad();
            value.backward();
            self.optimizer.step();
            value.detach()
        }

        fn test_loss(&self, batch: &B) -> Tensor {
            (self.loss)(batch)
        }

        fn back_up(&mut self, cursors: &[u64], rows_done: u64, loss_sum: f64, duration_secs: f64) {
            (self.on_backup)(&*self.optimizer, cursors, rows_done, loss_sum, duration_secs);
        }
    }

    /// These loop tests' stand-in for a real batch: just the labels, `[rows, 1]`. The loop itself
    /// never looks inside a batch, so the smallest thing a trainee's `loss` can be computed from is
    /// enough.
    fn sample_to_batch() -> BatchBuilder<impl Fn(&[CacheRecord]) -> Vec<f32> + Sync, impl Fn(&Vec<f32>) -> Tensor> {
        BatchBuilder {
            prepare: |records: &[CacheRecord]| records.iter().map(|record| record.label()).collect::<Vec<f32>>(),
            upload: |labels: &Vec<f32>| Tensor::from_slice(labels).view([labels.len() as i64, 1]),
        }
    }

    /// A trivial network for these track-agnostic loop tests: one linear layer from a constant input
    /// to a sigmoid output. What the network actually computes doesn't matter to anything tested
    /// here (the resume/backup/stop/duration bookkeeping, not prediction quality) --
    /// `nnue_network.rs`'s and `transformer_network.rs`'s own tests already cover their real forward
    /// passes.
    fn sample_loss(vs: &nn::VarStore) -> impl Fn(&Tensor) -> Tensor {
        let linear = nn::linear(vs.root() / "probe", 1, 1, Default::default());
        move |label: &Tensor| {
            let batch_size = label.size()[0];
            let input = Tensor::ones([batch_size, 1], (tch::Kind::Float, tch::Device::Cpu));
            let prediction = input.apply(&linear).sigmoid();
            prediction.binary_cross_entropy(label, None::<Tensor>, Reduction::Mean)
        }
    }

    /// A probe over `optimizer`, with a backup callback that does nothing.
    fn sample_trainee<'a>(vs: &'a nn::VarStore, optimizer: &'a mut AdamW) -> Probe<'a, Tensor> {
        Probe {
            optimizer,
            loss: Box::new(sample_loss(vs)),
            on_backup: Box::new(|_: &AdamW, _: &[u64], _: u64, _: f64, _: f64| {}),
        }
    }

    fn fresh_resume() -> RoundResume {
        RoundResume { rows_done: 0, loss_sum: 0.0, duration_secs: 0.0 }
    }

    /// Bare kings, one record per label: these track-agnostic loop tests only care about row counts
    /// and labels reaching the loss, never about the positions.
    fn records_of(labels: &[f32]) -> Vec<CacheRecord> {
        use fattorini::features::CanonicalPosition;
        let canonical = CanonicalPosition {
            side_to_move: Color::White,
            pieces: vec![],
            white_king: Square::E1,
            black_king: Square::E8,
            castling_rights: CastlingRights::NONE,
            en_passant_file: None,
        };
        labels.iter().map(|&label| CacheRecord::new(canonical.clone(), label)).collect()
    }

    /// A test set of `records_of(labels)`, as a sweep takes it.
    fn test_set(labels: &[f32]) -> [RecordsSource; 1] {
        [RecordsSource(records_of(labels))]
    }

    /// A training stream that cycles through fixed records in order: the same batch sequence on
    /// every run.
    struct CyclingStream {
        records: Vec<CacheRecord>,
        next: usize,
    }

    impl TrainStream for CyclingStream {
        fn next_batch(&mut self, batch_size: usize) -> Vec<CacheRecord> {
            let batch = (0..batch_size).map(|offset| self.records[(self.next + offset) % self.records.len()].clone()).collect();
            self.next = (self.next + batch_size) % self.records.len();
            batch
        }

        fn cursors(&self) -> Vec<u64> {
            vec![self.next as u64]
        }
    }

    /// A test half held in memory.
    struct RecordsSource(Vec<CacheRecord>);

    impl BatchSource for RecordsSource {
        fn row_count(&self) -> u64 {
            self.0.len() as u64
        }

        fn read_batch_stop_short(&mut self, cursor: u64, batch_size: usize) -> (Vec<CacheRecord>, u64) {
            let start = cursor as usize;
            let end = (start + batch_size).min(self.0.len());
            (self.0[start..end].to_vec(), if end == self.0.len() { 0 } else { end as u64 })
        }
    }

    #[test]
    fn a_batch_s_piece_count_is_its_records_when_they_share_one_and_none_when_mixed() {
        // `pieces` white knights beside the kings, on a1, a2 and so on: only the count matters.
        let record = |pieces: u16| {
            let canonical = fattorini::features::CanonicalPosition {
                side_to_move: fattorini::board::color::Color::White,
                pieces: (0..pieces).map(|square| square * 10 + 2).collect(),
                white_king: fattorini::board::square::Square::E1,
                black_king: fattorini::board::square::Square::E8,
                castling_rights: fattorini::board::castling_rights::CastlingRights::NONE,
                en_passant_file: None,
            };
            CacheRecord::new(canonical, 0.0)
        };
        assert_eq!(piece_count_of(&[record(1), record(1)]), Some(3), "kings included");
        assert_eq!(piece_count_of(&[record(1), record(2)]), None);
        assert_eq!(piece_count_of(&[]), None);
    }

    /// Two test passes over seven rows in batches of two, through a cache of `budget` bytes, with
    /// how many batches each pass prepared, and the loss of an uncached pass to compare.
    fn cached_test_passes(budget: u64) -> ([f64; 2], [usize; 2], f64, TestCache<Vec<f32>>) {
        let mut readers = test_set(&[0.1, 0.9, 0.3, 0.7, 0.5, 0.2, 0.8]);

        let vs = nn::VarStore::new(tch::Device::Cpu);
        let mut optimizer = AdamW::new(&vs, 1e-3);
        let trainee = sample_trainee(&vs, &mut optimizer);
        let prepared = std::sync::atomic::AtomicUsize::new(0);
        let sample = sample_to_batch();
        let to_batch = BatchBuilder {
            prepare: |records: &[CacheRecord]| {
                prepared.fetch_add(1, Ordering::SeqCst);
                (sample.prepare)(records)
            },
            upload: &sample.upload,
        };
        let mut cache = TestCache::new(budget, |labels: &Vec<f32>| labels.len() * std::mem::size_of::<f32>());
        let mut losses = [0.0; 2];
        let mut prepared_per_pass = [0; 2];
        for pass in 0..2 {
            let before = prepared.load(Ordering::SeqCst);
            losses[pass] = tch::no_grad(|| {
                run_test_pass(&trainee, &mut readers, Some(&mut cache), TEST_BATCH_SIZE, &to_batch, &mut |_, _| {})
            })
            .global;
            prepared_per_pass[pass] = prepared.load(Ordering::SeqCst) - before;
        }
        let uncached =
            tch::no_grad(|| run_test_pass(&trainee, &mut readers, None, TEST_BATCH_SIZE, &to_batch, &mut |_, _| {})).global;
        (losses, prepared_per_pass, uncached, cache)
    }

    #[test]
    fn a_cache_that_holds_every_batch_gives_the_same_loss_and_prepares_nothing_again() {
        let (losses, prepared, uncached, cache) = cached_test_passes(1000);
        assert_eq!(losses, [uncached, uncached]);
        assert_eq!(prepared, [4, 0]);
        assert_eq!(cache.rows(), 7);
    }

    #[test]
    fn a_cache_that_holds_a_prefix_gives_the_same_loss_and_prepares_only_the_rest() {
        // One batch of two rows is 8 bytes: the first fits in 10, the second does not.
        let (losses, prepared, uncached, cache) = cached_test_passes(10);
        assert_eq!(losses, [uncached, uncached]);
        assert_eq!(prepared, [4, 3]);
        assert_eq!(cache.rows(), 2);
    }

    #[test]
    fn a_cache_that_holds_nothing_gives_the_same_loss_and_prepares_everything() {
        let (losses, prepared, uncached, cache) = cached_test_passes(0);
        assert_eq!(losses, [uncached, uncached]);
        assert_eq!(prepared, [4, 4]);
        assert_eq!(cache.rows(), 0);
    }

    #[test]
    fn run_training_steps_produces_a_finite_row_weighted_loss_and_reports_the_stream_s_cursors() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let mut optimizer = AdamW::new(&vs, 1e-3);
        let mut trainee = sample_trainee(&vs, &mut optimizer);
        let mut stream = CyclingStream { records: records_of(&[0.3; 5]), next: 0 };
        let stop_requested = AtomicBool::new(false);
        let (train_loss, next_cursors, _duration_secs) = run_training_steps(
            &mut trainee, &mut stream, 3, TEST_BATCH_SIZE, fresh_resume(), Duration::from_secs(3600), &sample_to_batch(),
            &mut |_: u64| {}, &stop_requested,
        )
        .expect("run_training_steps should complete when stop_requested is never set");

        assert!(train_loss.is_finite() && train_loss >= 0.0, "train_loss = {train_loss}");
        // 3 steps of 2 rows over 5: the stream stands at row 6 mod 5.
        assert_eq!(next_cursors, vec![1]);
    }

    #[test]
    fn stop_requested_interrupts_the_pass_mid_round_and_backs_up_the_stopped_point() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let mut optimizer = AdamW::new(&vs, 1e-3);
        let mut stream = CyclingStream { records: records_of(&[0.3; 8]), next: 0 };
        let stop_requested = AtomicBool::new(true);
        let backup_calls = std::cell::RefCell::new(Vec::new());
        let mut trainee: Probe<Tensor> = Probe {
            optimizer: &mut optimizer,
            loss: Box::new(sample_loss(&vs)),
            on_backup: Box::new(|current_optimizer: &AdamW, _: &[u64], _: u64, _: f64, _: f64| {
                backup_calls.borrow_mut().push(current_optimizer.step_count())
            }),
        };

        let result = run_training_steps(
            &mut trainee, &mut stream, 3, TEST_BATCH_SIZE, fresh_resume(), Duration::from_secs(3600), &sample_to_batch(),
            &mut |_: u64| {}, &stop_requested,
        );

        assert!(result.is_none(), "a stop request seen mid-pass must report the pass as unfinished");
        drop(trainee);
        assert_eq!(backup_calls.into_inner(), vec![1], "expected exactly one forced backup, after only one step");
    }

    #[test]
    fn run_round_produces_finite_losses_and_advances_the_cursor_by_exactly_steps_times_batch_size() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let mut optimizer = AdamW::new(&vs, 1e-3);
        let mut trainee = sample_trainee(&vs, &mut optimizer);
        let mut stream = CyclingStream { records: records_of(&[0.3; 5]), next: 0 };
        let mut test_readers = test_set(&[0.3; 3]);
        let stop_requested = AtomicBool::new(false);
        let outcome = run_round(
            &mut trainee, &mut stream, &mut test_readers, None, 2, TEST_BATCH_SIZE, None, Duration::from_secs(3600),
            &sample_to_batch(), &mut |_: &str, _: u64, _: u64| {}, &stop_requested,
        )
        .expect("run_round should complete when stop_requested is never set");

        let (train_loss, test_loss) = (outcome.train_loss, outcome.test_loss);
        assert!(train_loss.is_finite() && train_loss >= 0.0, "train_loss = {train_loss}");
        assert!(test_loss.is_finite() && test_loss >= 0.0, "test_loss = {test_loss}");
        assert_eq!(outcome.cursors, vec![2 * TEST_BATCH_SIZE as u64]);
    }

    #[test]
    fn run_round_returns_none_and_never_runs_the_test_pass_when_stopped_mid_training() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let mut optimizer = AdamW::new(&vs, 1e-3);
        let mut trainee = sample_trainee(&vs, &mut optimizer);
        let mut stream = CyclingStream { records: records_of(&[0.3; 8]), next: 0 };
        let mut test_readers = test_set(&[0.3; 3]);
        let stop_requested = AtomicBool::new(true);
        let calls = std::cell::RefCell::new(Vec::new());
        let mut on_progress =
            |label: &str, processed: u64, total: u64| calls.borrow_mut().push((label.to_string(), processed, total));

        let result = run_round(
            &mut trainee, &mut stream, &mut test_readers, None, 3, TEST_BATCH_SIZE, None, Duration::from_secs(3600),
            &sample_to_batch(), &mut on_progress, &stop_requested,
        );

        assert!(result.is_none(), "a stop request seen mid-training must report the whole round as unfinished");
        assert!(
            calls.borrow().iter().all(|(label, _, _)| label == "train"),
            "the test pass must never run once the training pass was cut short: {:?}", calls.borrow()
        );
    }
}
