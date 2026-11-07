//! The one shared training-round loop for every architecture this project trains. Originally
//! written once for NNUE (reading one flat cache file cyclically), then duplicated a second time
//! for the transformer (reading 31 length-bucketed files via a weighted-random pick), then merged
//! when NNUE moved onto that same length-bucketed format -- and now merged again, in the other
//! direction, after NNUE moved back to a flat cache read through a shuffle buffer
//! (`dataset_cache.rs` covers why: a bucketed read makes every NNUE batch materially homogeneous).
//!
//! Two differences between the tracks survive, and both are parameters rather than duplicated code:
//! how a batch of raw records becomes a loss tensor (`to_batch` plus the `Trainee`'s own `loss`), and
//! where the batches come from (`train_stream::TrainStream`). Everything else -- the always-at-least-one-batch loop,
//! checking the real `optimizer.step_count()` against `steps_per_round` directly rather than a
//! separate counter, cross-call duration accumulation, backup/stop handling -- lives here once, for
//! both tracks.
//!
//! The no-grad sweeps below are generic over `train_stream::BatchSource` for the same reason: both
//! cache layouts can be swept once in order, so the sweep is written once.

use std::collections::BTreeMap;
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tch::{Kind, Tensor};

#[cfg(test)]
use super::adamw::AdamW;
use super::cache::CacheRecord;
use super::pipeline;
use super::test_cache::{Position, TestCache};
use super::train_stream::{BatchSource, TrainStream};

/// The network a run trains: its optimizer, the closure that turns an already-built batch into its
/// loss, and the closure that persists its own state when a backup is due.
///
/// `optimizer` is a borrow rather than an owned field because the optimizer, `VarStore` and network
/// live together in the caller's own struct, and `loss` and `on_backup` borrow the other two:
/// splitting the struct's fields is what lets all three coexist.
///
/// Test-only: both tracks train a stack of arms (`main.rs`'s `Stack`). These one-network arms are what
/// the round's own tests drive.
#[cfg(test)]
#[allow(clippy::type_complexity)]
pub struct Trainee<'a, B> {
    pub optimizer: &'a mut AdamW,
    /// `train` is `true` during the training half and `false` during the test pass, for architectures
    /// whose forward pass has training-only behaviour (the transformer's dropout) to gate on; ignored
    /// entirely by architectures that have none (NNUE).
    pub loss: Box<dyn Fn(&B, bool) -> Tensor + 'a>,
    pub on_backup: Box<dyn FnMut(&AdamW, &[u64], u64, f64, f64) + 'a>,
}

/// The arms a round trains, whatever holds them: a slice of `Trainee`s, one network and optimizer per
/// arm (the tests'), or a stacked network, every arm in one set of tensors (`stacked_network.rs`,
/// `stacked_transformer.rs`). The round loop sees only this.
///
/// Losses come back as one `[arms]` tensor left on the device, so the loop sums them there and reads
/// them only at a backup and at the end of the round (`run_training_steps`).
pub trait Arms<B> {
    fn count(&self) -> usize;

    /// Arm `arm`'s cumulative step count: where it stands in its sequence of batches.
    fn step_count(&self, arm: usize) -> u64;

    /// One training step on `batch` of every arm flagged in `stepping`, the rest left exactly as they
    /// were. Returns each arm's training loss on `batch`, detached, `0` for an arm that did not step.
    fn train_step(&mut self, batch: &B, stepping: &[bool]) -> Tensor;

    /// Each arm's test loss on `batch`, `[arms]`. Called under `no_grad`.
    fn test_losses(&self, batch: &B) -> Tensor;

    /// Persists arm `arm`'s state, with what a mid-round resume needs of its round: the stream's
    /// cursors, its rows so far, its loss sum and the round's training seconds.
    fn back_up(&mut self, arm: usize, cursors: &[u64], rows_done: u64, loss_sum: f64, duration_secs: f64);
}

#[cfg(test)]
impl<B> Arms<B> for [Trainee<'_, B>] {
    fn count(&self) -> usize {
        self.len()
    }

    fn step_count(&self, arm: usize) -> u64 {
        self[arm].optimizer.step_count() as u64
    }

    fn train_step(&mut self, batch: &B, stepping: &[bool]) -> Tensor {
        let mut losses: Vec<Option<Tensor>> = Vec::with_capacity(self.len());
        for (trainee, &steps) in self.iter_mut().zip(stepping) {
            if !steps {
                losses.push(None);
                continue;
            }
            let value = (trainee.loss)(batch, true);
            losses.push(Some(value.detach()));
            trainee.optimizer.zero_grad();
            value.backward();
            trainee.optimizer.step();
        }
        let like = losses.iter().flatten().next().expect("at least one arm steps").zeros_like();
        let losses: Vec<Tensor> = losses.into_iter().map(|loss| loss.unwrap_or_else(|| like.shallow_clone())).collect();
        Tensor::stack(&losses, 0)
    }

    fn test_losses(&self, batch: &B) -> Tensor {
        let losses: Vec<Tensor> = self.iter().map(|trainee| (trainee.loss)(batch, false)).collect();
        Tensor::stack(&losses, 0)
    }

    fn back_up(&mut self, arm: usize, cursors: &[u64], rows_done: u64, loss_sum: f64, duration_secs: f64) {
        let Trainee { optimizer, on_backup, .. } = &mut self[arm];
        on_backup(optimizer, cursors, rows_done, loss_sum, duration_secs);
    }
}

/// A `Vec` of trainees is its slice's arms.
#[cfg(test)]
impl<B> Arms<B> for Vec<Trainee<'_, B>> {
    fn count(&self) -> usize {
        self.as_slice().count()
    }

    fn step_count(&self, arm: usize) -> u64 {
        self.as_slice().step_count(arm)
    }

    fn train_step(&mut self, batch: &B, stepping: &[bool]) -> Tensor {
        self.as_mut_slice().train_step(batch, stepping)
    }

    fn test_losses(&self, batch: &B) -> Tensor {
        self.as_slice().test_losses(batch)
    }

    fn back_up(&mut self, arm: usize, cursors: &[u64], rows_done: u64, loss_sum: f64, duration_secs: f64) {
        self.as_mut_slice().back_up(arm, cursors, rows_done, loss_sum, duration_secs);
    }
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

/// What one completed round reports, per arm in the order of the arms: whether the arm completed
/// its round in this call, and its losses (meaningful only where it did).
pub struct RoundOutcome {
    pub completed: Vec<bool>,
    pub train_loss: Vec<f64>,
    pub test_loss: Vec<f64>,
    /// The test loss per piece count (`TestLosses::by_pieces`).
    pub test_loss_by_pieces: BTreeMap<usize, Vec<f64>>,
    pub cursors: Vec<u64>,
    pub train_duration_secs: f64,
    pub test_duration_secs: f64,
}

/// What a test pass measured: the mean over every row, per arm, and the same over the rows of each
/// piece count, kings included, of the batches holding a single one (`piece_count_of`), per arm.
pub struct TestLosses {
    pub global: Vec<f64>,
    pub by_pieces: BTreeMap<usize, Vec<f64>>,
}

/// Each piece count's loss for arm `slot`, as `history::append_pieces` writes them.
pub fn losses_of_arm(by_pieces: &BTreeMap<usize, Vec<f64>>, slot: usize) -> BTreeMap<usize, f64> {
    by_pieces.iter().map(|(&pieces, losses)| (pieces, losses[slot])).collect()
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

/// Where an interrupted round resumes, per arm in the order of the arms: the rows each has
/// already trained on in its current round and its loss sum so far; and the training seconds
/// already spent.
pub struct RoundResume {
    pub rows_done: Vec<u64>,
    pub loss_sums: Vec<f64>,
    pub duration_secs: f64,
}

/// Runs one evaluation round: training batches from `train_stream` until the optimizer's cumulative
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
///
/// `arms` are trained in lockstep (lockstep-lambdas.md): every batch is built and uploaded once, then
/// every arm steps on it (`Arms::train_step`), one after another for a slice of trainees, at once for
/// the stacked network; the test pass likewise uploads each batch once for every arm. A single run is
/// a one-arm slice.
///
/// Arms may stand at different step counts, as a crash between two arms' backups leaves them. The
/// stream then stands at the lowest, and an arm steps on a batch only when its own count equals the
/// stream's: an arm further ahead skips the batches it already trained on, then joins (each batch of
/// its sequence stepped on once, up to the shuffle's restart inexactness). The round ends where the
/// stream's count reaches the next multiple of `steps_per_round`; an arm completed it only if it got
/// there in this call (`RoundOutcome::completed`): one that was already past, or that had already
/// recorded that boundary before the crash, did not, and records nothing.
#[allow(clippy::too_many_arguments)]
pub fn run_round<B, H: Send, T: BatchSource + Send>(
    arms: &mut (impl Arms<B> + ?Sized), train_stream: &mut (impl TrainStream + Send), test_readers: &mut [T],
    test_cache: Option<&mut TestCache<H>>, steps_per_round: u64, batch_size: usize, resume: Option<RoundResume>,
    backup_interval: Duration, to_batch: &BatchBuilder<impl Fn(&[CacheRecord]) -> H + Sync, impl Fn(&H) -> B>,
    on_progress: &mut impl FnMut(&str, u64, u64), stop_requested: &AtomicBool,
) -> Option<RoundOutcome> {
    let count = arms.count();
    assert!(count > 0, "a round needs at least one arm");
    let resume = resume.unwrap_or(RoundResume { rows_done: vec![0; count], loss_sums: vec![0.0; count], duration_secs: 0.0 });
    assert_eq!(resume.loss_sums.len(), count, "a resumed round needs one loss sum per arm");
    assert_eq!(resume.rows_done.len(), count, "a resumed round needs one row count per arm");
    let (completed, train_loss, next_cursors, train_duration_secs) = run_training_steps(
        arms, train_stream, steps_per_round, batch_size, resume, backup_interval, to_batch,
        &mut |absolute_step| on_progress("train", absolute_step, steps_per_round), stop_requested,
    )?;

    let (test_losses, test_duration_secs) = run_test_round(arms, test_readers, test_cache, batch_size, to_batch, on_progress);

    Some(RoundOutcome {
        completed,
        train_loss,
        test_loss: test_losses.global,
        test_loss_by_pieces: test_losses.by_pieces,
        cursors: next_cursors,
        train_duration_secs,
        test_duration_secs,
    })
}

/// A test pass alone, with no training before it: `run_round`'s test half, for the round at step 0
/// that measures the generation's starting weights, every run's first best. Returns each arm's test
/// losses and the pass's seconds.
pub fn run_test_round<B, H: Send, T: BatchSource + Send>(
    arms: &(impl Arms<B> + ?Sized), test_readers: &mut [T], test_cache: Option<&mut TestCache<H>>, batch_size: usize,
    to_batch: &BatchBuilder<impl Fn(&[CacheRecord]) -> H + Sync, impl Fn(&H) -> B>,
    on_progress: &mut impl FnMut(&str, u64, u64),
) -> (TestLosses, f64) {
    let test_start = Instant::now();
    let test_loss = tch::no_grad(|| {
        run_test_pass(arms, test_readers, test_cache, batch_size, to_batch, &mut |processed, total| {
            on_progress("test", processed, total)
        })
    });
    (test_loss, test_start.elapsed().as_secs_f64())
}

/// The training half of one evaluation round: runs batches of up to `batch_size` rows (the
/// bucketed stream can return fewer, whenever the bucket it picked runs low -- "no padding, ever"),
/// updating weights through `optimizer` after every batch, until
/// `optimizer.step_count()` itself lands on a multiple of `steps_per_round` -- always at least one
/// batch, even if the count already sits on a multiple at entry (otherwise a round that just
/// completed, or a resumed run started exactly at a boundary, would "complete" a second round for
/// free with no training at all). Checking the real, persisted, ever-increasing step count
/// directly, rather than a separate counter that only ever tracked "how far into a round of
/// exactly this size", means a `steps_per_round` that changes between two runs of the same job can
/// never desynchronize this decision. Returns `Some((train_loss, cursors, rows_done,
/// duration_secs))` for a run that reached that boundary, or `None` if `stop_requested` cut it
/// short instead.
///
/// `initial_duration_secs`/the returned `duration_secs` accumulate this round's own training time
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
///
/// Every arm whose step count equals the stream's steps on each batch (`run_round`); each keeps its own rows and loss sum. Returns, per arm, whether it completed its
/// round in this call and its train loss.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn run_training_steps<B, H: Send>(
    arms: &mut (impl Arms<B> + ?Sized), stream: &mut (impl TrainStream + Send), steps_per_round: u64,
    batch_size: usize, resume: RoundResume, backup_interval: Duration,
    to_batch: &BatchBuilder<impl Fn(&[CacheRecord]) -> H + Sync, impl Fn(&H) -> B>, on_progress: &mut impl FnMut(u64),
    stop_requested: &AtomicBool,
) -> Option<(Vec<bool>, Vec<f64>, Vec<u64>, f64)> {
    let RoundResume { mut rows_done, mut loss_sums, duration_secs: initial_duration_secs } = resume;
    let count = arms.count();
    // The stream stands at the lowest arm's count; `stream_step` is the count of the batch about to
    // be stepped on.
    let start_step = (0..count).map(|arm| arms.step_count(arm)).min().expect("at least one arm");
    let mut stream_step = start_step;
    let mut stepped = vec![false; count];
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
    let mut unread_losses: Option<Tensor> = None;
    let mut finished = None;
    pipeline::pipelined(pipeline::worker_count(), source, prepare, |(prepared, rows_this_batch, cursors)| {
        // Uploaded once, stepped on by every arm that stands where the stream does.
        let batch = (to_batch.upload)(&prepared);
        let stepping: Vec<bool> = (0..count).map(|arm| arms.step_count(arm) == stream_step).collect();
        for (arm, &steps) in stepping.iter().enumerate() {
            if steps {
                stepped[arm] = true;
                rows_done[arm] += rows_this_batch;
            }
        }
        // In double, as the loss sums are: a round sums thousands of batches. An arm that did not
        // step adds its `0`.
        let weighted = arms.train_step(&batch, &stepping).to_kind(Kind::Double) * rows_this_batch as f64;
        unread_losses = Some(match unread_losses.take() {
            Some(sum) => sum + weighted,
            None => weighted,
        });
        stream_step += 1;
        // The stream's count, which every arm that stepped now shares.
        let step = stream_step;
        on_progress(step);

        let duration_secs = initial_duration_secs + call_start.elapsed().as_secs_f64();
        let stop = stop_requested.load(Ordering::SeqCst);
        if stop || last_backup.elapsed() >= backup_interval {
            read_losses(&mut unread_losses, &mut loss_sums);
            for arm in 0..count {
                // An arm still waiting for the stream has nothing new to save.
                if stepped[arm] {
                    arms.back_up(arm, &cursors, rows_done[arm], loss_sums[arm], duration_secs);
                }
            }
            last_backup = Instant::now();
        }
        if stop {
            return ControlFlow::Break(());
        }
        if step % steps_per_round == 0 {
            read_losses(&mut unread_losses, &mut loss_sums);
            // Completed here: stepped in this call, onto the boundary.
            let completed: Vec<bool> = (0..count).map(|arm| stepped[arm] && arms.step_count(arm) == step).collect();
            let train_losses = loss_sums
                .iter()
                .zip(&rows_done)
                .map(|(loss_sum, &rows)| if rows == 0 { f64::NAN } else { loss_sum / rows as f64 })
                .collect();
            finished = Some((completed, train_losses, cursors, duration_secs));
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    });
    finished
}

/// Adds the per-arm losses summed on the device since the last read, if any, to `loss_sums`: the one
/// place a training round waits for the GPU (`run_training_steps`).
fn read_losses(unread_losses: &mut Option<Tensor>, loss_sums: &mut [f64]) {
    if let Some(sum) = unread_losses.take() {
        let sums = Vec::<f64>::try_from(&sum.to_device(tch::Device::Cpu)).expect("one loss sum per arm");
        assert_eq!(sums.len(), loss_sums.len(), "one loss sum per arm");
        for (loss_sum, value) in loss_sums.iter_mut().zip(sums) {
            *loss_sum += value;
        }
    }
}

/// One full, deterministic, no-grad pass over every row of every one of `readers`, each swept in
/// file order -- no random pick and no cursor to maintain across calls: every reader with any rows
/// at all is read from its own row `0` through exactly one wraparound (`read_batch_stop_short`
/// always returns cursor `0` once its rows are exhausted), which is precisely one full sweep, no
/// more and no fewer. The NNUE track passes a one-element slice holding the flat test cache; the
/// transformer track passes one reader per piece-count bucket per stem. `on_progress` reports
/// against the grand total across every reader combined. The `loss` is called with `train = false`
/// throughout, and the mean over every row is returned, with the mean over the rows of each piece
/// count, from the batches holding a single one (`piece_count_of`).
///
/// Pipelined as the training half is (`pipeline::pipelined`): the sweep is read in order on one
/// thread and its batches prepared on every other core.
///
/// With a `cache` (`test_cache.rs`), the first pass offers it every batch as it is consumed, each
/// with the position it starts at, and the cache keeps a prefix; every later pass uploads the kept
/// batches first, then reads and prepares only the rest, from where the cache says it starts. Either
/// way the batches reach the loss in the same order, so the result is the same.
fn run_test_pass<B, H: Send, T: BatchSource + Send>(
    arms: &(impl Arms<B> + ?Sized), readers: &mut [T], mut cache: Option<&mut TestCache<H>>, batch_size: usize,
    to_batch: &BatchBuilder<impl Fn(&[CacheRecord]) -> H + Sync, impl Fn(&H) -> B>, on_progress: &mut impl FnMut(u64, u64),
) -> TestLosses {
    let total: u64 = readers.iter().map(|reader| reader.row_count()).sum();
    let reader_count = readers.len();
    let mut processed = 0u64;
    // Summed on the device and read once at the end, as in `run_training_steps`; one sum per arm,
    // and the same per piece count, with the rows each covers.
    let mut unread_losses: Option<Tensor> = None;
    let mut unread_by_pieces: BTreeMap<usize, (Option<Tensor>, u64)> = BTreeMap::new();
    let mut add_loss = |prepared: &H, rows_this_batch: u64, pieces: Option<usize>| {
        // Uploaded once, evaluated by every arm. Each piece count's sum keeps the rows it covers, to
        // divide by at the end.
        let batch = (to_batch.upload)(prepared);
        let weighted = arms.test_losses(&batch).to_kind(Kind::Double) * rows_this_batch as f64;
        if let Some(pieces) = pieces {
            let (sum, rows) = unread_by_pieces.entry(pieces).or_insert((None, 0));
            *sum = Some(match sum.take() {
                Some(sum) => sum + &weighted,
                None => weighted.shallow_clone(),
            });
            *rows += rows_this_batch;
        }
        unread_losses = Some(match unread_losses.take() {
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
    let mut loss_sums = vec![0.0; arms.count()];
    read_losses(&mut unread_losses, &mut loss_sums);
    let global = loss_sums.iter().map(|loss_sum| loss_sum / total.max(1) as f64).collect();
    let mut by_pieces = BTreeMap::new();
    for (pieces, (mut sum, rows)) in unread_by_pieces {
        let mut sums = vec![0.0; arms.count()];
        read_losses(&mut sum, &mut sums);
        by_pieces.insert(pieces, sums.iter().map(|sum| sum / rows.max(1) as f64).collect());
    }
    TestLosses { global, by_pieces }
}

// The tests spell out their batch builders' closure types, and their loops over arms and rounds read
// one index across several vectors.
#[cfg(test)]
#[allow(clippy::type_complexity, clippy::needless_range_loop)]
mod tests {
    use super::*;
    use fattorini::board::castling_rights::CastlingRights;
    use fattorini::board::color::Color;
    use fattorini::board::square::Square;
    use tch::{nn, Reduction};

    const TEST_BATCH_SIZE: usize = 2;

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
    /// to a sigmoid output, wrapped as a trainee's own `loss` closure, the same shape `main.rs` builds
    /// for either real track. What the network actually computes doesn't matter to anything tested
    /// here (the resume/backup/stop/duration bookkeeping, not prediction quality) --
    /// `nnue_network.rs`'s and `transformer_network.rs`'s own tests already cover their real forward
    /// passes.
    fn sample_loss(vs: &nn::VarStore) -> impl Fn(&Tensor, bool) -> Tensor {
        let linear = nn::linear(vs.root() / "probe", 1, 1, Default::default());
        move |label: &Tensor, _train: bool| {
            let batch_size = label.size()[0];
            let input = Tensor::ones([batch_size, 1], (tch::Kind::Float, tch::Device::Cpu));
            let prediction = input.apply(&linear).sigmoid();
            prediction.binary_cross_entropy(label, None::<Tensor>, Reduction::Mean)
        }
    }

    /// A trainee over `optimizer`, with a backup callback that does nothing.
    fn sample_trainee<'a>(vs: &'a nn::VarStore, optimizer: &'a mut AdamW) -> Trainee<'a, Tensor> {
        Trainee {
            optimizer,
            loss: Box::new(sample_loss(vs)),
            on_backup: Box::new(|_: &AdamW, _: &[u64], _: u64, _: f64, _: f64| {}),
        }
    }

    /// Bare kings, one record per label: these track-agnostic loop tests only care about row counts
    /// and labels reaching `compute_loss`, never about the positions.
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
                run_test_pass(std::slice::from_ref(&trainee), &mut readers, Some(&mut cache), TEST_BATCH_SIZE, &to_batch, &mut |_, _| {})
            })
            .global[0];
            prepared_per_pass[pass] = prepared.load(Ordering::SeqCst) - before;
        }
        let uncached = tch::no_grad(|| {
            run_test_pass(std::slice::from_ref(&trainee), &mut readers, None, TEST_BATCH_SIZE, &to_batch, &mut |_, _| {})
        })
        .global[0];
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

    fn fresh_resume(arms: usize) -> RoundResume {
        RoundResume { rows_done: vec![0; arms], loss_sums: vec![0.0; arms], duration_secs: 0.0 }
    }

    /// Records of bare kings with one label per arm, from `labels[arm][row]`.
    fn lockstep_records(labels: &[Vec<f32>]) -> Vec<CacheRecord> {
        use fattorini::features::CanonicalPosition;
        (0..labels[0].len())
            .map(|row| CacheRecord {
                canonical: CanonicalPosition {
                    side_to_move: Color::White,
                    pieces: vec![],
                    white_king: Square::E1,
                    black_king: Square::E8,
                    castling_rights: CastlingRights::NONE,
                    en_passant_file: None,
                },
                labels: labels.iter().map(|arm_labels| arm_labels[row]).collect(),
                progress: crate::cache::Progress::UNKNOWN,
            })
            .collect()
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

    /// The batch every arm reads: the `[rows, arms]` label matrix.
    fn lockstep_to_batch() -> BatchBuilder<impl Fn(&[CacheRecord]) -> (Vec<f32>, usize) + Sync, impl Fn(&(Vec<f32>, usize)) -> Tensor> {
        BatchBuilder {
            prepare: |records: &[CacheRecord]| super::super::cache::label_matrix(records),
            upload: |(labels, arms): &(Vec<f32>, usize)| Tensor::from_slice(labels).view([-1, *arms as i64]),
        }
    }

    /// `sample_loss`'s network against arm `arm`'s column, every weight set to `start` so that a
    /// lockstep arm and its sequential twin start alike. Set, not seeded: `tch::manual_seed` is one
    /// generator for the whole process, and tests running in parallel draw from it between a seed and
    /// the initialisation it was meant for.
    fn arm_loss_from(vs: &nn::VarStore, start: f64, arm: usize) -> impl Fn(&Tensor, bool) -> Tensor {
        let loss = sample_loss(vs);
        tch::no_grad(|| {
            for (_, mut tensor) in vs.variables() {
                let _ = tensor.fill_(start);
            }
        });
        move |labels: &Tensor, train: bool| loss(&labels.narrow(1, arm as i64, 1), train)
    }

    /// Every variable of `vs`, by name, as plain numbers.
    fn weights(vs: &nn::VarStore) -> Vec<(String, Vec<f32>)> {
        let mut variables: Vec<(String, Vec<f32>)> = vs
            .variables()
            .into_iter()
            .map(|(name, tensor)| (name, Vec::<f32>::try_from(&tensor.flatten(0, -1)).unwrap()))
            .collect();
        variables.sort_by(|a, b| a.0.cmp(&b.0));
        variables
    }

    #[test]
    fn arms_in_lockstep_end_as_the_same_arms_trained_one_after_another() {
        let labels = vec![vec![0.1f32, 0.9, 0.3, 0.7, 0.5, 0.2], vec![0.8f32, 0.4, 0.6, 0.0, 1.0, 0.3]];
        let records = lockstep_records(&labels);
        let test_records = lockstep_records(&[vec![0.2f32, 0.6, 0.9], vec![0.5f32, 0.1, 0.7]]);
        let stop_requested = AtomicBool::new(false);
        let steps_per_round = 3;
        let rounds = 2;

        // Lockstep: both arms over one stream.
        let stores: Vec<nn::VarStore> = (0..2).map(|_| nn::VarStore::new(tch::Device::Cpu)).collect();
        let losses: Vec<_> = stores.iter().enumerate().map(|(arm, vs)| arm_loss_from(vs, 0.1 + 0.2 * arm as f64, arm)).collect();
        let mut optimizers: Vec<AdamW> = stores.iter().map(|vs| AdamW::new(vs, 1e-2)).collect();
        let mut stream = CyclingStream { records: records.clone(), next: 0 };
        let mut lockstep_outcomes = Vec::new();
        {
            let mut trainees: Vec<Trainee<Tensor>> = optimizers
                .iter_mut()
                .zip(&losses)
                .map(|(optimizer, loss)| Trainee {
                    optimizer,
                    loss: Box::new(loss),
                    on_backup: Box::new(|_: &AdamW, _: &[u64], _: u64, _: f64, _: f64| {}),
                })
                .collect();
            for _ in 0..rounds {
                let mut test_readers = [RecordsSource(test_records.clone())];
                lockstep_outcomes.push(
                    run_round(
                        &mut trainees, &mut stream, &mut test_readers, None, steps_per_round, 2, None,
                        Duration::from_secs(3600), &lockstep_to_batch(), &mut |_: &str, _: u64, _: u64| {},
                        &stop_requested,
                    )
                    .expect("never stopped"),
                );
            }
        }

        // One after another: each arm alone, from the same weights, over the same batch sequence.
        for arm in 0..2 {
            let vs = nn::VarStore::new(tch::Device::Cpu);
            let loss = arm_loss_from(&vs, 0.1 + 0.2 * arm as f64, arm);
            let mut optimizer = AdamW::new(&vs, 1e-2);
            let mut stream = CyclingStream { records: records.clone(), next: 0 };
            let mut trainee = Trainee {
                optimizer: &mut optimizer,
                loss: Box::new(&loss),
                on_backup: Box::new(|_: &AdamW, _: &[u64], _: u64, _: f64, _: f64| {}),
            };
            for round in 0..rounds {
                let mut test_readers = [RecordsSource(test_records.clone())];
                let outcome = run_round(
                    std::slice::from_mut(&mut trainee), &mut stream, &mut test_readers, None, steps_per_round, 2, None,
                    Duration::from_secs(3600), &lockstep_to_batch(), &mut |_: &str, _: u64, _: u64| {},
                    &stop_requested,
                )
                .expect("never stopped");
                assert_eq!(outcome.train_loss[0], lockstep_outcomes[round].train_loss[arm], "arm {arm}, round {round}");
                assert_eq!(outcome.test_loss[0], lockstep_outcomes[round].test_loss[arm], "arm {arm}, round {round}");
            }
            drop(trainee);
            assert_eq!(weights(&vs), weights(&stores[arm]), "arm {arm}'s weights");
        }
    }

    #[test]
    fn a_stack_trains_as_its_arms_would_with_one_network_each() {
        use super::super::batch::{self, DenseBatch, HostBatch};
        use super::super::feature_set::FeatureSet;
        use super::super::nnue_network::Network;
        use super::super::stacked_adamw::StackedAdamW;
        use super::super::stacked_network::StackedNetwork;
        use super::super::arm_slice;
        use super::super::lockstep::{SlotFiles, Stack};
        use fattorini::board::Board;
        use fattorini::features::extract_canonical;

        let fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
            "8/5k2/8/3P4/8/8/2K5/8 b - - 0 1",
            "r3k2r/ppp2ppp/2n1bn2/3pp3/3PP3/2N1BN2/PPP2PPP/R3K2R w KQkq d6 0 8",
            "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1",
            "rnbqkb1r/pppp1ppp/5n2/4p3/2B1P3/8/PPPP1PPP/RNBQK1NR w KQkq - 2 3",
        ];
        let labels = [[0.1f32, 0.8], [0.9, -0.4], [-0.3, 0.6], [0.7, 0.0], [0.5, -1.0], [-0.2, 0.3]];
        let records: Vec<CacheRecord> = fens
            .iter()
            .zip(labels)
            .map(|(fen, labels)| CacheRecord {
                canonical: extract_canonical(&Board::from_fen(fen).unwrap().0),
                labels: labels.to_vec(),
                progress: crate::cache::Progress::UNKNOWN,
            })
            .collect();
        let to_batch = BatchBuilder {
            prepare: |records: &[CacheRecord]| batch::host_batch(records, FeatureSet::Psq),
            upload: |host: &HostBatch| batch::upload(host, tch::Device::Cpu),
        };
        let never = AtomicBool::new(false);
        let (steps_per_round, rounds) = (3, 2);

        // One network and optimizer per arm, each bounded as training bounds a psq network.
        let stores: Vec<nn::VarStore> = (0..2).map(|_| nn::VarStore::new(tch::Device::Cpu)).collect();
        let networks: Vec<Network> = stores.iter().map(|vs| Network::new(&vs.root(), FeatureSet::Psq, 8, &[4])).collect();
        let mut optimizers: Vec<AdamW> = stores.iter().map(|vs| AdamW::new(vs, 1e-2)).collect();
        for (optimizer, network) in optimizers.iter_mut().zip(&networks) {
            for (name, bound) in network.weight_bounds() {
                optimizer.bound(name, bound);
            }
        }

        // The stack, from the same (already bounded) weights.
        let network = StackedNetwork::with_shape(FeatureSet::Psq, 8, &[4], 2, tch::Device::Cpu);
        for (arm, vs) in stores.iter().enumerate() {
            for (name, value) in vs.variables() {
                arm_slice::fill_slice(&network.parameters()[&name], arm as i64, &value, &name);
            }
        }
        let mut optimizer = StackedAdamW::new(network.parameters(), 1e-2);
        for (name, bound) in network.weight_bounds() {
            optimizer.bound(name, bound);
        }
        // Never written: the backup interval below is longer than the test.
        let files = (0..2)
            .map(|arm| SlotFiles {
                lambda: fattorini::pipeline::preprocessing_config::ArmLambda::Constant(arm as f64),
                model_path: String::new(),
                optimizer_path: String::new(),
                round_progress_path: String::new(),
            })
            .collect();
        let mut stack =
            Stack { network, optimizer, columns: Tensor::from_slice(&[0i64, 1]), files, steps_per_round, persists_position: false };

        let mut one_each = Vec::new();
        let mut stacked = Vec::new();
        {
            let mut trainees: Vec<Trainee<DenseBatch>> = optimizers
                .iter_mut()
                .zip(&networks)
                .enumerate()
                .map(|(arm, (optimizer, network))| Trainee {
                    optimizer,
                    loss: Box::new(move |batch: &DenseBatch, train: bool| {
                        if train {
                            return network.training_loss(batch, arm);
                        }
                        network.test_loss(batch, arm)
                    }),
                    on_backup: Box::new(|_: &AdamW, _: &[u64], _: u64, _: f64, _: f64| {}),
                })
                .collect();
            let mut stream = CyclingStream { records: records.clone(), next: 0 };
            for _ in 0..rounds {
                one_each.push(
                    run_round(
                        &mut trainees, &mut stream, &mut [RecordsSource(records.clone())], None, steps_per_round, 2,
                        None, Duration::from_secs(3600), &to_batch, &mut |_: &str, _: u64, _: u64| {}, &never,
                    )
                    .expect("never stopped"),
                );
            }
        }
        let mut stream = CyclingStream { records: records.clone(), next: 0 };
        for _ in 0..rounds {
            stacked.push(
                run_round(
                    &mut stack, &mut stream, &mut [RecordsSource(records.clone())], None, steps_per_round, 2, None,
                    Duration::from_secs(3600), &to_batch, &mut |_: &str, _: u64, _: u64| {}, &never,
                )
                .expect("never stopped"),
            );
        }

        let close = |a: f64, b: f64| (a - b).abs() <= 1e-5 * b.abs().max(1e-3);
        for round in 0..rounds {
            assert_eq!(stacked[round].completed, vec![true, true]);
            for arm in 0..2 {
                let (stacked_train, own_train) = (stacked[round].train_loss[arm], one_each[round].train_loss[arm]);
                let (stacked_test, own_test) = (stacked[round].test_loss[arm], one_each[round].test_loss[arm]);
                assert!(close(stacked_train, own_train), "round {round} arm {arm} train: {stacked_train} against {own_train}");
                assert!(close(stacked_test, own_test), "round {round} arm {arm} test: {stacked_test} against {own_test}");
            }
        }
        for (arm, vs) in stores.iter().enumerate() {
            assert_eq!(stack.step_count(arm), optimizers[arm].step_count() as u64);
            for (name, value) in vs.variables() {
                let slice = stack.network.parameters()[&name].get(arm as i64);
                assert!(slice.allclose(&value, 1e-4, 1e-5, false), "arm {arm} {name}");
            }
        }
    }

    #[test]
    fn an_arm_ahead_skips_until_the_stream_reaches_it_and_ends_as_if_never_interrupted() {
        let labels = vec![vec![0.1f32, 0.9, 0.3, 0.7, 0.5, 0.2, 0.8, 0.4], vec![0.8f32, 0.4, 0.6, 0.0, 1.0, 0.3, 0.2, 0.9]];
        let records = lockstep_records(&labels);
        let test_records = lockstep_records(&[vec![0.2f32, 0.6], vec![0.5f32, 0.1]]);
        let steps_per_round = 4;
        let never = AtomicBool::new(false);
        let no_backup = || Box::new(|_: &AdamW, _: &[u64], _: u64, _: f64, _: f64| {});

        // Uninterrupted: both arms, one round, from batch 0.
        let stores: Vec<nn::VarStore> = (0..2).map(|_| nn::VarStore::new(tch::Device::Cpu)).collect();
        let losses: Vec<_> = stores.iter().enumerate().map(|(arm, vs)| arm_loss_from(vs, 0.1 + 0.2 * arm as f64, arm)).collect();
        let mut optimizers: Vec<AdamW> = stores.iter().map(|vs| AdamW::new(vs, 1e-2)).collect();
        let uninterrupted = {
            let mut trainees: Vec<Trainee<Tensor>> = optimizers
                .iter_mut()
                .zip(&losses)
                .map(|(optimizer, loss)| Trainee { optimizer, loss: Box::new(loss), on_backup: no_backup() })
                .collect();
            run_round(
                &mut trainees, &mut CyclingStream { records: records.clone(), next: 0 },
                &mut [RecordsSource(test_records.clone())], None, steps_per_round, 2, None, Duration::from_secs(3600),
                &lockstep_to_batch(), &mut |_: &str, _: u64, _: u64| {}, &never,
            )
            .expect("never stopped")
        };

        // Interrupted: arm 0 alone takes one step and is stopped, its backup kept; then both resume
        // from the stream's start, where arm 1 still stands.
        let resumed_stores: Vec<nn::VarStore> = (0..2).map(|_| nn::VarStore::new(tch::Device::Cpu)).collect();
        let resumed_losses: Vec<_> =
            resumed_stores.iter().enumerate().map(|(arm, vs)| arm_loss_from(vs, 0.1 + 0.2 * arm as f64, arm)).collect();
        let mut resumed_optimizers: Vec<AdamW> = resumed_stores.iter().map(|vs| AdamW::new(vs, 1e-2)).collect();
        let backup = std::cell::Cell::new((0u64, 0.0f64));
        {
            let mut ahead = Trainee {
                optimizer: &mut resumed_optimizers[0],
                loss: Box::new(&resumed_losses[0]),
                on_backup: Box::new(|_: &AdamW, _: &[u64], rows: u64, loss_sum: f64, _: f64| backup.set((rows, loss_sum))),
            };
            let stop = AtomicBool::new(true);
            let result = run_training_steps(
                std::slice::from_mut(&mut ahead), &mut CyclingStream { records: records.clone(), next: 0 }, steps_per_round,
                2, fresh_resume(1), Duration::from_secs(3600), &lockstep_to_batch(), &mut |_: u64| {}, &stop,
            );
            assert!(result.is_none());
        }
        assert_eq!(resumed_optimizers[0].step_count(), 1);
        let (ahead_rows, ahead_loss_sum) = backup.get();
        let resumed = {
            let mut trainees: Vec<Trainee<Tensor>> = resumed_optimizers
                .iter_mut()
                .zip(&resumed_losses)
                .map(|(optimizer, loss)| Trainee { optimizer, loss: Box::new(loss), on_backup: no_backup() })
                .collect();
            let resume =
                RoundResume { rows_done: vec![ahead_rows, 0], loss_sums: vec![ahead_loss_sum, 0.0], duration_secs: 0.0 };
            run_round(
                &mut trainees, &mut CyclingStream { records: records.clone(), next: 0 },
                &mut [RecordsSource(test_records.clone())], None, steps_per_round, 2, Some(resume),
                Duration::from_secs(3600), &lockstep_to_batch(), &mut |_: &str, _: u64, _: u64| {}, &never,
            )
            .expect("never stopped")
        };

        assert_eq!(resumed.completed, vec![true, true]);
        for arm in 0..2 {
            assert_eq!(weights(&resumed_stores[arm]), weights(&stores[arm]), "arm {arm}'s weights");
            assert_eq!(resumed_optimizers[arm].step_count(), steps_per_round as i64);
            // Summed in another order across the interruption: equal up to rounding.
            assert!((resumed.train_loss[arm] - uninterrupted.train_loss[arm]).abs() < 1e-12, "arm {arm}'s train loss");
            assert_eq!(resumed.test_loss[arm], uninterrupted.test_loss[arm], "arm {arm}'s test loss");
        }
    }

    #[test]
    fn an_arm_that_already_recorded_the_boundary_does_not_complete_it_again() {
        let records = lockstep_records(&[vec![0.1f32, 0.9, 0.3, 0.7], vec![0.8f32, 0.4, 0.6, 0.0]]);
        let stores: Vec<nn::VarStore> = (0..2).map(|_| nn::VarStore::new(tch::Device::Cpu)).collect();
        let losses: Vec<_> = stores.iter().enumerate().map(|(arm, vs)| arm_loss_from(vs, 0.1, arm)).collect();
        let mut optimizers: Vec<AdamW> = stores.iter().map(|vs| AdamW::new(vs, 1e-2)).collect();
        let never = AtomicBool::new(false);
        // Arm 0 finishes a round of 2 alone, as if the crash came after its round was recorded.
        {
            let mut alone = Trainee {
                optimizer: &mut optimizers[0],
                loss: Box::new(&losses[0]),
                on_backup: Box::new(|_: &AdamW, _: &[u64], _: u64, _: f64, _: f64| {}),
            };
            run_training_steps(
                std::slice::from_mut(&mut alone), &mut CyclingStream { records: records.clone(), next: 0 }, 2, 2,
                fresh_resume(1), Duration::from_secs(3600), &lockstep_to_batch(), &mut |_: u64| {}, &never,
            )
            .expect("never stopped");
        }
        let before = weights(&stores[0]);
        let mut trainees: Vec<Trainee<Tensor>> = optimizers
            .iter_mut()
            .zip(&losses)
            .map(|(optimizer, loss)| Trainee {
                optimizer,
                loss: Box::new(loss),
                on_backup: Box::new(|_: &AdamW, _: &[u64], _: u64, _: f64, _: f64| {}),
            })
            .collect();
        let outcome = run_round(
            &mut trainees, &mut CyclingStream { records: records.clone(), next: 0 },
            &mut [RecordsSource(records.clone())], None, 2, 2, None, Duration::from_secs(3600), &lockstep_to_batch(),
            &mut |_: &str, _: u64, _: u64| {}, &never,
        )
        .expect("never stopped");
        drop(trainees);

        assert_eq!(outcome.completed, vec![false, true]);
        assert_eq!(weights(&stores[0]), before, "the arm ahead never stepped");
        assert_eq!(optimizers[1].step_count(), 2);
    }

    #[test]
    fn run_training_steps_produces_a_finite_row_weighted_loss_and_reports_the_stream_s_cursors() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let mut optimizer = AdamW::new(&vs, 1e-3);
        let mut trainee = sample_trainee(&vs, &mut optimizer);
        let mut stream = CyclingStream { records: records_of(&[0.3; 5]), next: 0 };
        let stop_requested = AtomicBool::new(false);
        let (completed, train_losses, next_cursors, _duration_secs) = run_training_steps(
            std::slice::from_mut(&mut trainee), &mut stream, 3, TEST_BATCH_SIZE, fresh_resume(1),
            Duration::from_secs(3600), &sample_to_batch(), &mut |_: u64| {},
            &stop_requested,
        )
        .expect("run_training_steps should complete when stop_requested is never set");

        let train_loss = train_losses[0];
        assert!(train_loss.is_finite() && train_loss >= 0.0, "train_loss = {train_loss}");
        assert_eq!(completed, vec![true]);
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
        let mut trainee: Trainee<Tensor> = Trainee {
            optimizer: &mut optimizer,
            loss: Box::new(sample_loss(&vs)),
            on_backup: Box::new(|current_optimizer: &AdamW, _: &[u64], _: u64, _: f64, _: f64| {
                backup_calls.borrow_mut().push(current_optimizer.step_count())
            }),
        };

        let result = run_training_steps(
            std::slice::from_mut(&mut trainee), &mut stream, 3, TEST_BATCH_SIZE, fresh_resume(1),
            Duration::from_secs(3600), &sample_to_batch(), &mut |_: u64| {}, &stop_requested,
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
            std::slice::from_mut(&mut trainee), &mut stream, &mut test_readers, None, 2, TEST_BATCH_SIZE, None,
            Duration::from_secs(3600), &sample_to_batch(), &mut |_: &str, _: u64, _: u64| {},
            &stop_requested,
        )
        .expect("run_round should complete when stop_requested is never set");

        let (train_loss, test_loss) = (outcome.train_loss[0], outcome.test_loss[0]);
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
            std::slice::from_mut(&mut trainee), &mut stream, &mut test_readers, None, 3, TEST_BATCH_SIZE, None,
            Duration::from_secs(3600), &sample_to_batch(), &mut on_progress, &stop_requested,
        );

        assert!(result.is_none(), "a stop request seen mid-training must report the whole round as unfinished");
        assert!(
            calls.borrow().iter().all(|(label, _, _)| label == "train"),
            "the test pass must never run once the training pass was cut short: {:?}", calls.borrow()
        );
    }
}
