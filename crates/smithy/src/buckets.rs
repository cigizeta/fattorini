//! An NNUE forge's training data, fully shuffled: the training set scattered once into `BUCKETS` bucket
//! files on the local disk (`prepare`), then read back every epoch in a fresh uniformly random order
//! (`BucketStream`), each position exactly once an epoch. No shuffle buffer.
//!
//! **Why not the buffer.** Whole games entered the refilling buffer together, so a game's siblings
//! were trained within one buffer turnover: at a high rate the network tracked the buffer's current
//! games, and at every rate drop the test loss fell while the train loss rose.
//!
//! **Scatter.** Each position goes to a bucket drawn by a hash of its arrival index. A bucket file is
//! a sequence of zstd frames, one per flush of its in-memory part, so a later prepare appends to it,
//! adding only training files not yet scattered. The order within a file never reaches training.
//!
//! **Gather.** An epoch's seed fixes the order of the buckets and a Fisher-Yates permutation within
//! each, applied as a bucket loads; a thread loads and shuffles the next bucket while the current one
//! is trained. Where a run stands is derived from its step count, so a resume lands on the exact
//! record it stopped before, and nothing beyond the step count is persisted.
//!
//! **Records carry no lambda.** A record holds its position and its labels at lambda 0 and lambda 1;
//! the direct blend is linear between them (`labels::compute_labels_direct`: a decisive score gives
//! the same label at both ends, the final position the result at both), so any lambda's label is
//! computed as a batch loads, and a new grid never invalidates the buckets. Each record also holds
//! its progress in its game (`cache::Progress`), so a polyline arm's lambda is computed per position
//! as the batch loads (`polyline::Polyline::at`).
//!
//! **The state** is `buckets.tsv` beside the files: the training files scattered, with their positions,
//! and each bucket's committed bytes and records, written after each batch of `BATCH_POSITIONS`. A
//! prepare cut short keeps its committed batches; the next one truncates every bucket to its
//! committed length and scatters only the files not yet listed.

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use temper_chess::features::CanonicalPosition;
use temper_chess::pipeline::data_root;
use temper_chess::labels::Curve;
use temper_chess::tablebase::SyzygyTablebase;

use temper_chess::pipeline::preprocessing_config::ArmLambda;

use super::cache::{self, CacheRecord, Progress};
use super::polyline::Polyline;
use super::game_labels::KEPT;
use super::pgn_stream::{splitmix64, PgnStream, StreamFile, StreamOptions};
use super::train_stream::TrainStream;

/// The bucket files: about 1M records each for a self-play generation of 1B positions or Lc0's
/// training set of about 1.2B, a bucket's memory once loaded.
pub(crate) const BUCKETS: usize = 1024;

/// A bucket's in-memory part is compressed and appended as one frame once it holds this many bytes:
/// `BUCKETS` of them in memory at once while scattering.
const FLUSH_BYTES: usize = 256 * 1024;

/// zstd's level: low, so scattering is bounded by reading the PGN, not by compressing.
const LEVEL: i32 = 1;

/// Files open at once across the scatter's readers, as training's stream once did.
const OPEN_FILES: usize = 64;

/// Distinct from every other seed in smithy.
const SCATTER_SEED: u64 = 0xB0C4_E75C;
const EPOCH_SEED: u64 = 0xB0C4_E90C;

/// The scatter commits its state after each batch of about this many positions, the only points
/// where every bucket holds its files whole (the readers mix many files into every frame): a Stop or
/// a failed read then loses one batch, not the whole scatter. Lc0's corpus is about 24 of them.
const BATCH_POSITIONS: u64 = 50_000_000;

const STATE: &str = "buckets.tsv";

fn bucket_path(dir: &str, bucket: usize) -> String {
    format!("{dir}/bucket-{bucket:04}.zst")
}

/// One bucket's committed length.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Committed {
    bytes: u64,
    records: u64,
}

/// What `buckets.tsv` holds: which positions of a game were kept (`KEPT`; none in a state
/// scattered before the final position was left out), how they were labelled (`labels_kind`; none
/// before the tables rescored them), the training files scattered, by path, with their positions,
/// and every bucket's committed length.
#[derive(Debug, Default, PartialEq)]
struct State {
    keep: Option<String>,
    labels: Option<String>,
    files: Vec<(String, u64)>,
    buckets: Vec<Committed>,
}

impl State {
    fn read(dir: &str) -> Result<Option<State>, String> {
        let path = format!("{dir}/{STATE}");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("{path}: {error}")),
        };
        let mut state = State::default();
        for line in text.lines().filter(|line| !line.is_empty()) {
            let fields: Vec<&str> = line.split('\t').collect();
            let number = |index: usize| -> Result<u64, String> {
                fields.get(index).and_then(|field| field.parse().ok()).ok_or_else(|| format!("{path}: a malformed line {line:?}"))
            };
            match fields[0] {
                "keep" if fields.len() == 2 => state.keep = Some(fields[1].to_string()),
                "labels" if fields.len() == 2 => state.labels = Some(fields[1].to_string()),
                "file" if fields.len() == 3 => state.files.push((fields[1].to_string(), number(2)?)),
                "bucket" if fields.len() == 4 => state.buckets.push(Committed { bytes: number(2)?, records: number(3)? }),
                _ => return Err(format!("{path}: a malformed line {line:?}")),
            }
        }
        if state.buckets.len() != BUCKETS {
            return Err(format!("{path}: {} buckets, this build makes {BUCKETS}; delete {dir} to scatter again", state.buckets.len()));
        }
        Ok(Some(state))
    }

    /// Written whole to a temporary file, then renamed over the old one: a crash leaves either.
    fn write(&self, dir: &str) -> Result<(), String> {
        let mut text = String::new();
        if let Some(keep) = &self.keep {
            text.push_str(&format!("keep\t{keep}\n"));
        }
        if let Some(labels) = &self.labels {
            text.push_str(&format!("labels\t{labels}\n"));
        }
        for (path, positions) in &self.files {
            text.push_str(&format!("file\t{path}\t{positions}\n"));
        }
        for (bucket, committed) in self.buckets.iter().enumerate() {
            text.push_str(&format!("bucket\t{bucket}\t{}\t{}\n", committed.bytes, committed.records));
        }
        let path = format!("{dir}/{STATE}");
        let partial = format!("{path}.partial");
        std::fs::write(&partial, text).map_err(|error| format!("{partial}: {error}"))?;
        std::fs::rename(&partial, &path).map_err(|error| format!("{path}: {error}"))
    }

    fn records(&self) -> u64 {
        self.buckets.iter().map(|committed| committed.records).sum()
    }
}

/// One record's bytes: the position (`cache::write_position`), its label at lambda 0 as an `f32`, its
/// label at lambda 1 as an `i8` (the result, or a decisive score's sign, from the side to move), and
/// its progress, i then n, as two `u16`.
fn write_record(out: &mut Vec<u8>, canonical: &CanonicalPosition, at_zero: f32, at_one: f32, progress: Progress) {
    cache::write_position(out, canonical);
    out.extend_from_slice(&at_zero.to_le_bytes());
    out.push(at_one.round() as i8 as u8);
    out.extend_from_slice(&progress.i.to_le_bytes());
    out.extend_from_slice(&progress.n.to_le_bytes());
}

/// The labels of `arms`, in that order: a constant arm's at its lambda, a polyline arm's at
/// `polyline`'s lambda for the record's progress.
#[derive(Clone)]
pub(crate) struct Labelling {
    arms: Vec<ArmLambda>,
    polyline: Option<Polyline>,
}

impl Labelling {
    /// Refused when an arm is the polyline and no polyline is given.
    pub(crate) fn new(arms: &[ArmLambda], polyline: Option<Polyline>) -> Result<Labelling, String> {
        if polyline.is_none() && arms.contains(&ArmLambda::Polyline) {
            return Err("a polyline arm without its polyline".to_string());
        }
        Ok(Labelling { arms: arms.to_vec(), polyline })
    }

    pub(crate) fn arms(&self) -> &[ArmLambda] {
        &self.arms
    }

    pub(crate) fn polyline(&self) -> Option<&Polyline> {
        self.polyline.as_ref()
    }

    pub(crate) fn lambda(&self, arm: ArmLambda, progress: Progress) -> f64 {
        match arm {
            ArmLambda::Constant(lambda) => lambda,
            ArmLambda::Polyline => {
                let polyline = self.polyline.as_ref().expect("Labelling::new requires the polyline");
                polyline.at(usize::from(progress.i), usize::from(progress.n))
            }
        }
    }
}

/// Every record of a bucket's decompressed bytes, labelled by `labelling`.
fn read_records(mut bytes: &[u8], labelling: &Labelling) -> Vec<CacheRecord> {
    let mut records = Vec::new();
    while !bytes.is_empty() {
        let canonical = cache::read_position(&mut bytes);
        let (at_zero, at_one, progress) = read_tail(&mut bytes);
        let labels = labelling.arms.iter().map(|&arm| blend(at_zero, at_one, labelling.lambda(arm, progress))).collect();
        records.push(CacheRecord { canonical, labels, progress });
    }
    records
}

/// What follows a record's position: its label at lambda 0, at lambda 1, and its progress.
fn read_tail(bytes: &mut &[u8]) -> (f32, f32, Progress) {
    let (tail, rest) = bytes.split_at(9);
    *bytes = rest;
    let at_zero = f32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]);
    let at_one = f32::from(tail[4] as i8);
    (at_zero, at_one, Progress { i: u16::from_le_bytes([tail[5], tail[6]]), n: u16::from_le_bytes([tail[7], tail[8]]) })
}

/// The records `prepare` committed to `dir`.
pub(crate) fn committed_records(dir: &str) -> Result<u64, String> {
    Ok(State::read(dir)?.ok_or_else(|| format!("{dir} holds no buckets"))?.records())
}

/// Every record of bucket `bucket` of `dir`, as its labels at lambda 0 and 1 and its progress: what
/// `polyline` measures, the positions skipped over.
pub(crate) fn for_each_pair(dir: &str, bucket: usize, mut visit: impl FnMut(f32, f32, Progress)) {
    let decompressed = decompressed(dir, bucket);
    let mut bytes = decompressed.as_slice();
    while !bytes.is_empty() {
        cache::read_position(&mut bytes);
        let (at_zero, at_one, progress) = read_tail(&mut bytes);
        visit(at_zero, at_one, progress);
    }
}

/// The direct blend at `lambda`, from its two ends.
pub(crate) fn blend(at_zero: f32, at_one: f32, lambda: f64) -> f32 {
    ((1.0 - lambda) * f64::from(at_zero) + lambda * f64::from(at_one)) as f32
}

/// How a prepare ended.
#[derive(Debug, PartialEq)]
pub(crate) enum Prepared {
    /// The buckets hold `files`, this many records.
    Done(u64),
    /// Stopped before committing: the next prepare starts over from the committed state.
    Stopped,
}

/// Makes `dir`'s buckets hold exactly the searched positions of `files`: nothing when they already
/// do; the new files appended when `files` only adds to what was scattered; everything again when a
/// scattered file is gone or has changed, or the buckets kept other positions. `threads` read the
/// PGN, as training's stream once did. `say` reports the scatter.
/// Every position the Syzygy tables answer is labelled by them (`self_play_game_tablebase`), so the
/// tables are required: without them the scatter is refused, never made silently without. Every
/// score is read through `curve`, the teacher's (Lc0's curve for Lc0's corpus, `labels::LOGISTIC`
/// for PeSTO and our networks).
pub(crate) fn prepare(
    dir: &str, files: &[StreamFile], threads: usize, stop: &Arc<AtomicBool>, say: &dyn Fn(String), curve: Curve,
) -> Result<Prepared, String> {
    let tablebase = SyzygyTablebase::from_directories(&data_root::syzygy_directories())?;
    prepare_in_batches(dir, files, threads, stop, Some(Arc::clone(stop)), say, BATCH_POSITIONS, Some(Arc::new(tablebase)), curve)
}

/// How a scatter labelled its positions, as `buckets.tsv` records it: by the tables where they answer,
/// or without them (the tests alone), and the curve its scores were read through
/// (`Curve::describe`), and `progress`: every record carries its progress. Buckets labelled
/// otherwise, a line recording no curve or no progress included, are scattered again. `tablebase-2`:
/// plain `tablebase` took a long loss for a draw (`label_result`).
fn labels_kind(tablebase: &Option<Arc<SyzygyTablebase>>, curve: &Curve) -> String {
    let kind = if tablebase.is_some() { "tablebase-2" } else { "plain" };
    format!("{kind} {} progress", curve.describe())
}

/// `files` in order, cut into batches of at least `batch_positions` positions, the last one shorter;
/// a file larger than that is a batch alone.
fn batches(files: &[StreamFile], batch_positions: u64) -> Vec<&[StreamFile]> {
    let mut batches = Vec::new();
    let (mut start, mut positions) = (0, 0);
    for (index, file) in files.iter().enumerate() {
        positions += file.positions;
        if positions >= batch_positions {
            batches.push(&files[start..=index]);
            (start, positions) = (index + 1, 0);
        }
    }
    if start < files.len() {
        batches.push(&files[start..]);
    }
    batches
}

/// `prepare`, committing the state after every batch of `batch_positions`.
#[allow(clippy::too_many_arguments)]
fn prepare_in_batches(
    dir: &str,
    files: &[StreamFile],
    threads: usize,
    stop: &AtomicBool,
    // The same stop, handed to the stream's readers so a read waiting for Google Drive ends at it
    // (`ReadWait`); `None` in the tests, whose fixtures never fail to read.
    read_stop: Option<Arc<AtomicBool>>,
    say: &dyn Fn(String),
    batch_positions: u64,
    tablebase: Option<Arc<SyzygyTablebase>>,
    curve: Curve,
) -> Result<Prepared, String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("{dir}: {error}"))?;
    let wanted: HashMap<&str, u64> = files.iter().map(|file| (file.path.as_str(), file.positions)).collect();
    let labels = labels_kind(&tablebase, &curve);
    let empty = || State {
        keep: Some(KEPT.to_string()),
        labels: Some(labels.clone()),
        files: Vec::new(),
        buckets: vec![Committed::default(); BUCKETS],
    };
    let mut state = match State::read(dir)? {
        Some(state) if state.keep.as_deref() != Some(KEPT) => {
            say(format!("{dir}: scattered keeping other positions, so it is scattered again from the start"));
            empty()
        }
        Some(state) if state.labels.as_deref() != Some(labels.as_str()) => {
            say(format!("{dir}: labelled otherwise ({}, now {labels}), so it is scattered again from the start", state.labels.as_deref().unwrap_or("unrecorded")));
            empty()
        }
        Some(state) if state.files.iter().all(|(path, positions)| wanted.get(path.as_str()) == Some(positions)) => state,
        Some(_) => {
            say(format!("{dir}: the training set changed, so it is scattered again from the start"));
            empty()
        }
        None => empty(),
    };
    let scattered: Vec<&str> = state.files.iter().map(|(path, _)| path.as_str()).collect();
    let new: Vec<StreamFile> = files.iter().filter(|file| !scattered.contains(&file.path.as_str())).cloned().collect();
    if new.is_empty() {
        return Ok(Prepared::Done(state.records()));
    }
    let positions: u64 = new.iter().map(|file| file.positions).sum();
    let batches = batches(&new, batch_positions);
    say(format!(
        "{dir}: scattering {positions} positions of {} training set files into {BUCKETS} buckets, in {} batches",
        new.len(),
        batches.len()
    ));
    let started = Instant::now();

    // Back to the committed state: a scatter cut short left frames past it.
    let mut outputs = Vec::with_capacity(BUCKETS);
    for (bucket, committed) in state.buckets.iter().enumerate() {
        let path = bucket_path(dir, bucket);
        let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).map_err(|error| format!("{path}: {error}"))?;
        file.set_len(committed.bytes).map_err(|error| format!("{path}: {error}"))?;
        outputs.push(BucketWriter { path, file, pending: Vec::new(), committed: *committed });
    }

    // The bar counts the whole training set, so a resume starts at what is committed.
    let total: u64 = files.iter().map(|file| file.positions).sum();
    let bar = progress::resumed(total - positions, total, progress::Unit::Count("positions"), "scattering");
    let mut read: u64 = total - positions;
    for (index, batch) in batches.iter().enumerate() {
        if stop.load(Ordering::SeqCst) {
            return Ok(Prepared::Stopped);
        }
        let options = StreamOptions {
            lambdas: vec![0.0, 1.0],
            threads,
            open_files: OPEN_FILES.max(threads),
            keep_probability: 1.0,
            seed: SCATTER_SEED,
            tablebase: tablebase.clone(),
            curve,
            stop: read_stop.clone(),
        };
        let mut stream = PgnStream::start(batch.to_vec(), options);
        // Numbered on from the records already scattered, so a later file's positions draw afresh.
        let first = state.records();
        let mut in_batch: u64 = 0;
        while let Some(record) = stream.next() {
            if in_batch.is_multiple_of(65_536) {
                if stop.load(Ordering::SeqCst) {
                    return Ok(Prepared::Stopped);
                }
                bar.set_position(read);
            }
            let bucket = (splitmix64(SCATTER_SEED ^ (first + in_batch)) % BUCKETS as u64) as usize;
            outputs[bucket].push(&record)?;
            in_batch += 1;
            read += 1;
        }
        drop(stream);
        // A stop while a read waited for Google Drive ends the stream short: the batch is not whole,
        // so it is never committed, and the next scatter truncates its frames.
        if stop.load(Ordering::SeqCst) {
            return Ok(Prepared::Stopped);
        }
        // The batch committed: the only point where every bucket holds its files whole.
        for output in &mut outputs {
            output.flush()?;
        }
        state.buckets = outputs.iter().map(|output| output.committed).collect();
        state.files.extend(batch.iter().map(|file| (file.path.clone(), file.positions)));
        state.write(dir)?;
        say(format!("{dir}: batch {} of {} committed, {} records", index + 1, batches.len(), state.records()));
    }
    bar.finish();
    let bytes: u64 = state.buckets.iter().map(|committed| committed.bytes).sum();
    say(format!(
        "{dir}: scattered in {:.1} minutes; {} records, {:.1} GB",
        started.elapsed().as_secs_f64() / 60.0,
        state.records(),
        bytes as f64 / 1e9
    ));
    Ok(Prepared::Done(state.records()))
}

/// One bucket being appended to.
struct BucketWriter {
    path: String,
    file: std::fs::File,
    pending: Vec<u8>,
    committed: Committed,
}

impl BucketWriter {
    fn push(&mut self, record: &CacheRecord) -> Result<(), String> {
        write_record(&mut self.pending, &record.canonical, record.labels[0], record.labels[1], record.progress);
        self.committed.records += 1;
        if self.pending.len() >= FLUSH_BYTES {
            self.flush()?;
        }
        Ok(())
    }

    /// The pending records as one more frame at the file's end.
    fn flush(&mut self) -> Result<(), String> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let frame = zstd::encode_all(self.pending.as_slice(), LEVEL).map_err(|error| format!("{}: {error}", self.path))?;
        let end = self.file.metadata().map_err(|error| format!("{}: {error}", self.path))?.len();
        std::io::Seek::seek(&mut self.file, std::io::SeekFrom::Start(end)).map_err(|error| format!("{}: {error}", self.path))?;
        self.file.write_all(&frame).map_err(|error| format!("{}: {error}", self.path))?;
        self.committed.bytes = end + frame.len() as u64;
        self.pending.clear();
        Ok(())
    }
}

/// The records of `dir`'s buckets, epoch after epoch without end, each epoch in a fresh uniformly
/// random order.
pub(crate) struct BucketStream {
    receiver: Option<Receiver<Vec<CacheRecord>>>,
    loader: Option<JoinHandle<()>>,
    current: std::vec::IntoIter<CacheRecord>,
}

/// An epoch's bucket order and the seed of each bucket's permutation.
fn epoch_order(epoch: u64) -> (Vec<usize>, u64) {
    let seed = splitmix64(EPOCH_SEED ^ epoch);
    let mut order: Vec<usize> = (0..BUCKETS).collect();
    let mut rng = StdRng::seed_from_u64(seed);
    for index in (1..order.len()).rev() {
        let other = rng.random_range(0..=index);
        order.swap(index, other);
    }
    (order, seed)
}

/// The bytes of bucket `bucket` of `dir`.
fn decompressed(dir: &str, bucket: usize) -> Vec<u8> {
    let path = bucket_path(dir, bucket);
    let compressed = std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    // A bucket no record was drawn into is an empty file, which holds no zstd frame to decode.
    if compressed.is_empty() {
        return Vec::new();
    }
    zstd::decode_all(compressed.as_slice()).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// Bucket `bucket` of `dir`, labelled by `labelling`, in epoch `seed`'s permutation.
fn load(dir: &str, bucket: usize, labelling: &Labelling, seed: u64) -> Vec<CacheRecord> {
    let bytes = decompressed(dir, bucket);
    let mut records = read_records(&bytes, labelling);
    let mut rng = StdRng::seed_from_u64(splitmix64(seed ^ (bucket as u64 + 1)));
    for index in (1..records.len()).rev() {
        let other = rng.random_range(0..=index);
        records.swap(index, other);
    }
    records
}

impl BucketStream {
    /// `dir`'s buckets, as the last `prepare` committed them, labelled by `labelling`, `emitted`
    /// records into the run (`training_batch_size * step_count`).
    pub fn open(dir: &str, labelling: Labelling, emitted: u64) -> Result<BucketStream, String> {
        let state = State::read(dir)?.ok_or_else(|| format!("{dir} holds no buckets: the forge prepares them before training"))?;
        let counts: Vec<u64> = state.buckets.iter().map(|committed| committed.records).collect();
        let total: u64 = counts.iter().sum();
        if total == 0 {
            return Err(format!("{dir}: the buckets hold no records"));
        }
        let dir = dir.to_string();
        // Where the run stands: the epoch, the bucket in its order, and the records of it trained.
        let mut epoch = emitted / total;
        let mut skip = emitted % total;
        let (order, _) = epoch_order(epoch);
        let mut start = 0;
        while skip >= counts[order[start]] {
            skip -= counts[order[start]];
            start += 1;
        }
        // One bucket ahead: the loader waits for the training thread to take the last one.
        let (sender, receiver) = mpsc::sync_channel(1);
        let loader = std::thread::spawn(move || {
            loop {
                let (order, seed) = epoch_order(epoch);
                for &bucket in &order[start..] {
                    let mut records = load(&dir, bucket, &labelling, seed);
                    records.drain(..skip as usize);
                    skip = 0;
                    if sender.send(records).is_err() {
                        return;
                    }
                }
                epoch += 1;
                start = 0;
            }
        });
        Ok(BucketStream { receiver: Some(receiver), loader: Some(loader), current: Vec::new().into_iter() })
    }
}

impl TrainStream for BucketStream {
    /// Never short: the epochs never end, and a batch runs on into the next bucket.
    fn next_batch(&mut self, batch_size: usize) -> Vec<CacheRecord> {
        let mut batch = Vec::with_capacity(batch_size);
        while batch.len() < batch_size {
            match self.current.next() {
                Some(record) => batch.push(record),
                None => {
                    let next = self.receiver.as_ref().expect("a live stream holds its receiver").recv();
                    self.current = next.expect("the bucket loader stopped: its panic is above").into_iter();
                }
            }
        }
        batch
    }

    /// Deliberately empty: the position is derived from the step count.
    fn cursors(&self) -> Vec<u64> {
        Vec::new()
    }
}

impl Drop for BucketStream {
    /// Dropping the receiver fails the loader's next send, and it returns.
    fn drop(&mut self) {
        self.receiver = None;
        if let Some(loader) = self.loader.take() {
            loader.join().ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use temper_chess::labels::{LC0_FILES, LOGISTIC};

    /// Lc0's corpus as the tests' fixtures are written: through the frozen curve.
    const LC0_CURVE: Curve = Curve::Atan(LC0_FILES);

    /// Two games of different lengths and scores, 3 and 4 positions, one with a decisive score.
    const GAMES: &str = concat!(
        "[Result \"1-0\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Kd2 {1.50} Kd7 {-1.20}\n1-0\n\n",
        "[Result \"0-1\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Ke2 {-0.40} Ke7 {#3} 2. Kf2 {-0.90}\n0-1\n\n",
    );

    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("smithy-buckets-test-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_str().unwrap().replace('\\', "/")
    }

    /// `copies[i]` copies of `GAMES` in file `i`, 7 positions a copy, 5 of them searched.
    fn training_set(dir: &str, copies: &[u64]) -> Vec<StreamFile> {
        copies
            .iter()
            .enumerate()
            .map(|(index, &count)| {
                let path = format!("{dir}/train-{index}.pgn.zst");
                std::fs::write(&path, zstd::encode_all(GAMES.repeat(count as usize).as_bytes(), 3).unwrap()).unwrap();
                StreamFile { path, positions: count * 7 }
            })
            .collect()
    }

    fn quiet(_: String) {}

    /// `prepare` without the tables, which a test machine may lack: its labels are the games' own.
    /// Shadows the module's `prepare`, which requires them.
    fn prepare(dir: &str, files: &[StreamFile], threads: usize, stop: &AtomicBool, say: &dyn Fn(String)) -> Result<Prepared, String> {
        prepare_in_batches(dir, files, threads, stop, None, say, BATCH_POSITIONS, None, LC0_CURVE)
    }

    /// `dir`'s buckets labelled at constant `lambdas`, as the grid opens them.
    fn open(dir: &str, lambdas: &[f64], emitted: u64) -> BucketStream {
        let arms: Vec<ArmLambda> = lambdas.iter().map(|&lambda| ArmLambda::Constant(lambda)).collect();
        BucketStream::open(dir, Labelling::new(&arms, None).unwrap(), emitted).unwrap()
    }

    /// A record as a comparable key: its position's bytes and its labels' bits.
    fn key(record: &CacheRecord) -> (Vec<u8>, Vec<u32>) {
        let mut bytes = Vec::new();
        cache::write_position(&mut bytes, &record.canonical);
        (bytes, record.labels.iter().map(|label| label.to_bits()).collect())
    }

    fn sorted(records: &[CacheRecord]) -> Vec<(Vec<u8>, Vec<u32>)> {
        let mut keys: Vec<_> = records.iter().map(key).collect();
        keys.sort();
        keys
    }

    /// Every searched record of `files` at `lambdas`, as the stream that read them for training
    /// labelled them.
    fn direct(files: &[StreamFile], lambdas: &[f64]) -> Vec<CacheRecord> {
        let options =
            StreamOptions { lambdas: lambdas.to_vec(), threads: 1, open_files: 1, keep_probability: 1.0, seed: 1, tablebase: None, curve: LC0_CURVE, stop: None };
        let mut stream = PgnStream::start(files.to_vec(), options);
        let mut records = Vec::new();
        while let Some(record) = stream.next() {
            records.push(record);
        }
        records
    }

    #[test]
    fn an_epoch_gives_every_position_once_labelled_as_the_direct_blend() {
        let dir = scratch("epoch");
        let files = training_set(&dir,&[3, 5]);
        let buckets = format!("{dir}/buckets");
        assert_eq!(prepare(&buckets, &files, 2, &AtomicBool::new(false), &quiet), Ok(Prepared::Done(40)));
        let lambdas = [0.0, 0.3, 0.7, 1.0];
        let mut stream = open(&buckets, &lambdas, 0);
        let epoch = stream.next_batch(40);
        // The blend at load, from its two ends, is the blend the PGN stream computes, within an f32.
        let expected = direct(&files, &lambdas);
        let round = |records: &[CacheRecord]| -> Vec<(Vec<u8>, Vec<i64>)> {
            let mut keys: Vec<_> = records
                .iter()
                .map(|record| (key(record).0, record.labels.iter().map(|&label| (f64::from(label) * 1e5).round() as i64).collect()))
                .collect();
            keys.sort();
            keys
        };
        assert_eq!(round(&epoch), round(&expected));
        let second = stream.next_batch(40);
        assert_eq!(sorted(&second), sorted(&epoch), "the next epoch holds the same records");
        assert_ne!(second.iter().map(key).collect::<Vec<_>>(), epoch.iter().map(key).collect::<Vec<_>>(), "in another order");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_resume_yields_what_an_uninterrupted_run_would() {
        let dir = scratch("resume");
        let files = training_set(&dir,&[4, 4]);
        let buckets = format!("{dir}/buckets");
        prepare(&buckets, &files, 2, &AtomicBool::new(false), &quiet).unwrap();
        let mut whole = open(&buckets, &[0.5], 0);
        let run: Vec<_> = whole.next_batch(150).iter().map(key).collect();
        // 40 an epoch: 50 is 10 into the second.
        let mut resumed = open(&buckets, &[0.5], 50);
        assert_eq!(resumed.next_batch(100).iter().map(key).collect::<Vec<_>>(), run[50..]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_new_file_is_appended_and_a_changed_training_set_scattered_again() {
        let dir = scratch("append");
        let files = training_set(&dir,&[2, 3, 1]);
        let buckets = format!("{dir}/buckets");
        let never = AtomicBool::new(false);
        assert_eq!(prepare(&buckets, &files[..2], 1, &never, &quiet), Ok(Prepared::Done(25)));
        assert_eq!(prepare(&buckets, &files[..2], 1, &never, &quiet), Ok(Prepared::Done(25)), "nothing new");
        assert_eq!(prepare(&buckets, &files, 1, &never, &quiet), Ok(Prepared::Done(30)), "the third file appended");
        let epoch = open(&buckets, &[1.0], 0).next_batch(30);
        assert_eq!(sorted(&epoch), sorted(&direct(&files, &[1.0])));
        assert_eq!(prepare(&buckets, &files[1..], 1, &never, &quiet), Ok(Prepared::Done(20)), "a file gone: again");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn buckets_kept_otherwise_are_scattered_again() {
        let dir = scratch("keep");
        let files = training_set(&dir, &[2, 2]);
        let buckets = format!("{dir}/buckets");
        let never = AtomicBool::new(false);
        // 4 copies of two games, 2 final positions a copy left out.
        assert_eq!(prepare(&buckets, &files, 1, &never, &quiet), Ok(Prepared::Done(20)));
        assert_eq!(prepare(&buckets, &files, 1, &never, &quiet), Ok(Prepared::Done(20)), "nothing new");
        // A state kept otherwise, as `every` once kept the final positions too, is scattered again.
        let state = format!("{buckets}/{STATE}");
        let text = std::fs::read_to_string(&state).unwrap();
        let every: String =
            text.lines().map(|line| if line.starts_with("keep\t") { "keep\tevery\n".to_string() } else { format!("{line}\n") }).collect();
        std::fs::write(&state, every).unwrap();
        assert_eq!(prepare(&buckets, &files, 1, &never, &quiet), Ok(Prepared::Done(20)), "again");
        assert_eq!(State::read(&buckets).unwrap().unwrap().keep.as_deref(), Some("searched"));
        // A state written before the keep was recorded is scattered again too.
        let text = std::fs::read_to_string(&state).unwrap();
        std::fs::write(&state, text.lines().filter(|line| !line.starts_with("keep\t")).map(|line| format!("{line}\n")).collect::<String>())
            .unwrap();
        assert_eq!(prepare(&buckets, &files, 1, &never, &quiet), Ok(Prepared::Done(20)));
        assert_eq!(State::read(&buckets).unwrap().unwrap().keep.as_deref(), Some("searched"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_stopped_scatter_is_undone_by_the_next() {
        let dir = scratch("stopped");
        let files = training_set(&dir,&[2, 2]);
        let buckets = format!("{dir}/buckets");
        assert_eq!(prepare(&buckets, &files[..1], 1, &AtomicBool::new(false), &quiet), Ok(Prepared::Done(10)));
        // The next scatter stops at its first record: nothing new is committed.
        assert_eq!(prepare(&buckets, &files, 1, &AtomicBool::new(true), &quiet), Ok(Prepared::Stopped));
        assert_eq!(State::read(&buckets).unwrap().unwrap().records(), 10);
        // Bytes past the committed length, as a scatter cut short leaves them, are cut away.
        std::fs::OpenOptions::new().append(true).open(bucket_path(&buckets, 0)).unwrap().write_all(b"torn frame").unwrap();
        assert_eq!(prepare(&buckets, &files, 1, &AtomicBool::new(false), &quiet), Ok(Prepared::Done(20)));
        let epoch = open(&buckets, &[1.0], 0).next_batch(20);
        assert_eq!(sorted(&epoch), sorted(&direct(&files, &[1.0])));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn files_are_cut_into_batches_in_order() {
        let files: Vec<StreamFile> =
            [3, 4, 10, 1, 2].iter().map(|&positions| StreamFile { path: format!("{positions}"), positions }).collect();
        let sizes: Vec<Vec<u64>> =
            batches(&files, 6).iter().map(|batch| batch.iter().map(|file| file.positions).collect()).collect();
        assert_eq!(sizes, [vec![3, 4], vec![10], vec![1, 2]]);
    }

    #[test]
    fn a_stopped_scatter_keeps_its_committed_batches() {
        let dir = scratch("batches");
        let files = training_set(&dir, &[2, 3, 1]);
        let buckets = format!("{dir}/buckets");
        // A file a batch (14, 21 and 7 positions), stopped once the first is committed.
        let stop = AtomicBool::new(false);
        let say = |message: String| {
            if message.contains("batch 1 of") {
                stop.store(true, Ordering::SeqCst);
            }
        };
        assert_eq!(prepare_in_batches(&buckets, &files, 1, &stop, None, &say, 1, None, LC0_CURVE), Ok(Prepared::Stopped));
        let state = State::read(&buckets).unwrap().unwrap();
        assert_eq!(state.files, [(files[0].path.clone(), 14)]);
        assert_eq!(state.records(), 10);
        // The next scatter adds only the rest.
        let said = std::cell::RefCell::new(Vec::new());
        let record = |message: String| said.borrow_mut().push(message);
        assert_eq!(prepare_in_batches(&buckets, &files, 1, &AtomicBool::new(false), None, &record, 1, None, LC0_CURVE), Ok(Prepared::Done(30)));
        assert!(said.borrow()[0].contains("in 2 batches"), "{:?}", said.borrow());
        let epoch = open(&buckets, &[1.0], 0).next_batch(30);
        assert_eq!(sorted(&epoch), sorted(&direct(&files, &[1.0])));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn batches_change_nothing_but_frames() {
        let dir = scratch("same");
        let files = training_set(&dir, &[2, 3, 1]);
        let never = AtomicBool::new(false);
        let (one, many) = (format!("{dir}/one"), format!("{dir}/many"));
        assert_eq!(prepare(&one, &files, 2, &never, &quiet), Ok(Prepared::Done(30)));
        assert_eq!(prepare_in_batches(&many, &files, 2, &never, None, &quiet, 1, None, LC0_CURVE), Ok(Prepared::Done(30)));
        let epoch = |dir: &str| sorted(&open(dir, &[0.5], 0).next_batch(30));
        assert_eq!(epoch(&many), epoch(&one));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn buckets_labelled_otherwise_are_scattered_again() {
        let dir = scratch("labels");
        let files = training_set(&dir, &[2, 2]);
        let buckets = format!("{dir}/buckets");
        let never = AtomicBool::new(false);
        assert_eq!(prepare(&buckets, &files, 1, &never, &quiet), Ok(Prepared::Done(20)));
        let lc0_labels = labels_kind(&None, &LC0_CURVE);
        assert_eq!(lc0_labels, "plain one_pawn 0.500000 ceiling 32 progress");
        assert_eq!(State::read(&buckets).unwrap().unwrap().labels.as_deref(), Some(lc0_labels.as_str()));
        // A state from before the tables rescored, without the line, is scattered again.
        let state = format!("{buckets}/{STATE}");
        let text = std::fs::read_to_string(&state).unwrap();
        std::fs::write(&state, text.lines().filter(|line| !line.starts_with("labels\t")).map(|line| format!("{line}\n")).collect::<String>())
            .unwrap();
        let said = std::cell::RefCell::new(Vec::new());
        let record = |message: String| said.borrow_mut().push(message);
        assert_eq!(prepare(&buckets, &files, 1, &never, &record), Ok(Prepared::Done(20)));
        assert!(said.borrow()[0].contains("labelled otherwise"), "{:?}", said.borrow());
        assert_eq!(State::read(&buckets).unwrap().unwrap().labels.as_deref(), Some(lc0_labels.as_str()));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A polyline arm is labelled per position at the polyline's lambda for its own progress, the
    /// constant arm beside it as the grid labels it, and every record keeps its progress.
    #[test]
    fn a_polyline_arm_blends_each_position_at_its_own_lambda() {
        let dir = scratch("polyline");
        let files = training_set(&dir, &[2, 3]);
        let buckets = format!("{dir}/buckets");
        assert_eq!(prepare(&buckets, &files, 2, &AtomicBool::new(false), &quiet), Ok(Prepared::Done(25)));
        let polyline = Polyline::new([0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9]);
        let labelling = Labelling::new(&[ArmLambda::Polyline, ArmLambda::Constant(0.3)], Some(polyline)).unwrap();
        let epoch = BucketStream::open(&buckets, labelling, 0).unwrap().next_batch(25);
        let as_key = |record: &CacheRecord, labels: Vec<f32>| {
            let rounded: Vec<i64> = labels.iter().map(|&label| (f64::from(label) * 1e5).round() as i64).collect();
            (key(record).0, record.progress.i, record.progress.n, rounded)
        };
        let mut expected: Vec<_> = direct(&files, &[0.0, 1.0])
            .iter()
            .map(|record| {
                let (i, n) = (usize::from(record.progress.i), usize::from(record.progress.n));
                assert!(i < n, "a searched position's progress, {i} of {n}");
                let at = |lambda: f64| blend(record.labels[0], record.labels[1], lambda);
                as_key(record, vec![at(polyline.at(i, n)), at(0.3)])
            })
            .collect();
        let mut got: Vec<_> = epoch.iter().map(|record| as_key(record, record.labels.clone())).collect();
        expected.sort();
        got.sort();
        assert_eq!(got, expected);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Every record is read back, and the measurement stops between buckets when asked.
    #[test]
    fn the_polyline_is_measured_from_the_buckets() {
        let dir = scratch("measure");
        let files = training_set(&dir, &[2, 3]);
        let buckets = format!("{dir}/buckets");
        assert_eq!(prepare(&buckets, &files, 2, &AtomicBool::new(false), &quiet), Ok(Prepared::Done(25)));
        assert_eq!(committed_records(&buckets), Ok(25));
        let mut read = 0;
        for bucket in 0..BUCKETS {
            for_each_pair(&buckets, bucket, |_, _, progress| {
                assert!(progress.i < progress.n);
                read += 1;
            });
        }
        assert_eq!(read, 25);
        let bar = progress::resumed(0, 25, progress::Unit::Count("positions"), "test");
        let (_, positions) = crate::polyline::measure(&buckets, 2, &AtomicBool::new(false), &bar).unwrap();
        assert!(positions > 0 && positions < 25, "the decisive score left out: {positions}");
        assert_eq!(bar.position(), 25, "the bar counts every record read");
        assert!(crate::polyline::measure(&buckets, 2, &AtomicBool::new(true), &bar).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_polyline_arm_without_its_polyline_is_refused() {
        assert!(Labelling::new(&[ArmLambda::Constant(0.5), ArmLambda::Polyline], None).is_err());
        assert!(Labelling::new(&[ArmLambda::Constant(0.5)], None).is_ok());
    }

    /// A line written before the curve was recorded, naming the tables alone, is scattered again
    /// whatever the curve; so are buckets read through another curve.
    #[test]
    fn buckets_from_before_the_curve_or_of_another_are_scattered_again() {
        let dir = scratch("curve");
        let files = training_set(&dir, &[2, 2]);
        let buckets = format!("{dir}/buckets");
        let never = AtomicBool::new(false);
        assert_eq!(prepare(&buckets, &files, 1, &never, &quiet), Ok(Prepared::Done(20)));
        let state = format!("{buckets}/{STATE}");
        let text = std::fs::read_to_string(&state).unwrap();
        let old: String =
            text.lines().map(|line| if line.starts_with("labels\t") { "labels\tplain\n".to_string() } else { format!("{line}\n") }).collect();
        std::fs::write(&state, old).unwrap();
        let said = std::cell::RefCell::new(Vec::new());
        let record = |message: String| said.borrow_mut().push(message);
        assert_eq!(prepare(&buckets, &files, 1, &never, &record), Ok(Prepared::Done(20)));
        assert!(said.borrow()[0].contains("labelled otherwise"), "{:?}", said.borrow());
        assert_eq!(State::read(&buckets).unwrap().unwrap().labels, Some(labels_kind(&None, &LC0_CURVE)));

        // The logistic: someone else's labels.
        let said = std::cell::RefCell::new(Vec::new());
        let record = |message: String| said.borrow_mut().push(message);
        assert_eq!(prepare_in_batches(&buckets, &files, 1, &never, None, &record, BATCH_POSITIONS, None, LOGISTIC), Ok(Prepared::Done(20)));
        assert!(said.borrow()[0].contains("labelled otherwise"), "{:?}", said.borrow());
        assert_eq!(State::read(&buckets).unwrap().unwrap().labels, Some(labels_kind(&None, &LOGISTIC)));
        // And back to Lc0's: scattered again.
        let said = std::cell::RefCell::new(Vec::new());
        let record = |message: String| said.borrow_mut().push(message);
        assert_eq!(prepare(&buckets, &files, 1, &never, &record), Ok(Prepared::Done(20)));
        assert!(said.borrow()[0].contains("labelled otherwise"), "{:?}", said.borrow());
        std::fs::remove_dir_all(&dir).ok();
    }
}
