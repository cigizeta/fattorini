//! Labelled positions streamed straight from compressed self-play PGN, with no dataset built
//! beforehand: N worker threads, each reading several files at once, send whole games' positions down
//! one bounded channel to the training thread, which owns the shuffle buffer and the batches.
//!
//! **Mixing across the corpus happens here, not in the buffer.** A shuffle buffer only spreads
//! neighbouring games: read in file order, a corpus is walked file by file, and when that order
//! carried a drift (the self-play stretch at the end of an epoch) the test loss jumped every epoch
//! and patience reacted to the order, not to convergence. So each worker keeps several files open and
//! takes each next game from one picked at random, weighted by the positions it has left: every open
//! file runs out at about the same time, and every stretch of the stream samples the corpus in
//! proportion. The caller shuffles the file list once per epoch.
//!
//! **Data flows one way.** Nothing goes back to the workers: no acknowledgement, no checkpoint
//! handshake. Dropping the stream drops the receiver, every worker's next send fails, and it exits.
//! No thread ever waits on one that may be waiting on it. The order is not reproducible across runs,
//! and need not be: a resumed epoch may retrain some positions and skip others.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::cache::{CacheRecord, Progress};
use kalo_chess::labels::Curve;
use kalo_chess::tablebase::SyzygyTablebase;

use super::game_labels::{self_play_game_kept, self_play_game_labels, self_play_game_tablebase, ReadWait, SelfPlaySegmentReader, Stopped};

/// SplitMix64's output function: a well-mixed 64-bit hash of `value`, so that consecutive indices
/// give independent-looking draws. Not a seeded library RNG: its output is fixed by this code alone,
/// so no dependency update can change what a fixed seed draws.
pub(crate) fn splitmix64(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// One `.pgn.zst` file of the stream, with the trainable positions its `.meta` records.
#[derive(Clone, Debug)]
pub(crate) struct StreamFile {
    pub path: String,
    pub positions: u64,
}

/// What one worker read, kept or not.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Tally {
    pub games: u64,
    pub positions: u64,
}

/// How the stream reads its files.
#[derive(Clone, Debug)]
pub(crate) struct StreamOptions {
    /// The direct blend's lambdas: every record carries one label per lambda, in this order, as
    /// lockstep training's arms read them.
    pub lambdas: Vec<f64>,
    /// Worker threads, at least 1.
    pub threads: usize,
    /// Files open at once across every worker, at least `threads`; each worker owns an equal share.
    pub open_files: usize,
    /// The probability each searched position is sent, in (0, 1]: random skipping, which thins
    /// out neighbouring, near-identical plies of one game.
    pub keep_probability: f64,
    pub seed: u64,
    /// When set, every position the tables answer is labelled by them at every lambda
    /// (`self_play_game_tablebase`): the NNUE forges' scatter. `None` for the transformer's stream
    /// and the tests.
    pub tablebase: Option<Arc<SyzygyTablebase>>,
    /// What every score is read through, the teacher's (`labels::Curve`): Lc0's curve for Lc0's
    /// corpus, `labels::LOGISTIC` for PeSTO and our networks.
    pub curve: Curve,
    /// The job's stop, which ends a read waiting for Google Drive (`ReadWait`): the worker then exits
    /// and the stream ends short, so a caller passing it checks it again once `next` returns `None`.
    /// `None` waits until the read succeeds.
    pub stop: Option<Arc<AtomicBool>>,
}

/// Games in flight in the channel per worker: enough to smooth over a long game, few enough that a
/// dropped stream throws little parsed work away.
const GAMES_IN_FLIGHT_PER_WORKER: usize = 4;

/// The positions of one pass over its files.
pub(crate) struct PgnStream {
    receiver: Option<Receiver<Vec<CacheRecord>>>,
    pending: std::vec::IntoIter<CacheRecord>,
    handles: Vec<JoinHandle<Tally>>,
}

impl PgnStream {
    /// Starts the workers over `files`, read in this order as files free up.
    pub fn start(files: Vec<StreamFile>, options: StreamOptions) -> PgnStream {
        assert!(options.threads > 0, "a stream needs at least one worker");
        assert!(!options.lambdas.is_empty(), "a stream labels at least one lambda");
        assert!(options.open_files >= options.threads, "every worker needs at least one open file");
        assert!(
            options.keep_probability > 0.0 && options.keep_probability <= 1.0,
            "the keep probability is in (0, 1], not {}",
            options.keep_probability
        );
        let files = Arc::new(files);
        let next_file = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = mpsc::sync_channel(GAMES_IN_FLIGHT_PER_WORKER * options.threads);
        let handles = (0..options.threads)
            .map(|index| {
                let files = Arc::clone(&files);
                let next_file = Arc::clone(&next_file);
                let sender = sender.clone();
                let options = options.clone();
                // Every worker gets at least its share, the first ones one more while some remain.
                let slots = options.open_files / options.threads + usize::from(index < options.open_files % options.threads);
                let seed = splitmix64(options.seed ^ index as u64);
                std::thread::spawn(move || worker(&files, &next_file, slots, &options, seed, &sender))
            })
            .collect();
        drop(sender);
        PgnStream { receiver: Some(receiver), pending: Vec::new().into_iter(), handles }
    }

    /// The next position, or `None` once every file is drained.
    pub fn next(&mut self) -> Option<CacheRecord> {
        loop {
            if let Some(labelled) = self.pending.next() {
                return Some(labelled);
            }
            let game = self.receiver.as_ref().expect("a live stream holds its receiver").recv().ok()?;
            self.pending = game.into_iter();
        }
    }

    /// Ends the stream and returns what every worker read: dropping the receiver makes each worker's
    /// next send fail.
    #[cfg(test)]
    pub fn finish(mut self) -> Tally {
        self.stop()
    }

    fn stop(&mut self) -> Tally {
        self.receiver = None;
        let mut total = Tally::default();
        for handle in self.handles.drain(..) {
            let tally = handle.join().expect("a stream worker panicked");
            total.games += tally.games;
            total.positions += tally.positions;
        }
        total
    }
}

impl Drop for PgnStream {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One file a worker is reading.
struct OpenFile {
    path: String,
    reader: SelfPlaySegmentReader,
    /// The positions its `.count` says are still unread, the weight of its next pick.
    left: u64,
}

/// The next file of the list, opened, or `None` once every file has been claimed; `Stopped` if its
/// open waited for Google Drive and the job was stopped meanwhile.
fn claim(files: &[StreamFile], next_file: &AtomicUsize, wait: &ReadWait) -> Result<Option<OpenFile>, Stopped> {
    let index = next_file.fetch_add(1, Ordering::SeqCst);
    let Some(file) = files.get(index) else { return Ok(None) };
    let reader = SelfPlaySegmentReader::open_waiting(&file.path, wait.clone())?;
    Ok(Some(OpenFile { path: file.path.clone(), reader, left: file.positions }))
}

/// How the stream's reads wait: until the job's stop when it has one, forever otherwise.
fn read_wait(options: &StreamOptions) -> ReadWait {
    options.stop.clone().map_or_else(ReadWait::forever, ReadWait::until)
}

/// The index of `open` a draw from 0 to their total weight lands on, a file's weight being the
/// positions it has left (at least 1, so a file whose `.count` undercounts still gets read).
fn pick(open: &[OpenFile], rng: &mut StdRng) -> usize {
    let total: u64 = open.iter().map(|file| file.left.max(1)).sum();
    let mut draw = rng.random_range(0..total);
    for (index, file) in open.iter().enumerate() {
        let weight = file.left.max(1);
        if draw < weight {
            return index;
        }
        draw -= weight;
    }
    unreachable!("a draw below the total lands on some file");
}

/// One worker: keeps `slots` files open, reads a game from one picked by `pick`, labels and replays
/// it, keeps each position with the options' probability, and sends the kept ones as one message.
/// Returns once its files and the list are drained, or as soon as a send fails.
fn worker(
    files: &[StreamFile], next_file: &AtomicUsize, slots: usize, options: &StreamOptions, seed: u64,
    sender: &SyncSender<Vec<CacheRecord>>,
) -> Tally {
    let mut rng = StdRng::seed_from_u64(seed);
    let wait = read_wait(options);
    let mut tally = Tally::default();
    // A stop while a read waits for Google Drive ends the worker, as a failed send does.
    let mut open: Vec<OpenFile> = Vec::with_capacity(slots);
    for _ in 0..slots {
        match claim(files, next_file, &wait) {
            Ok(Some(file)) => open.push(file),
            Ok(None) => break,
            Err(Stopped) => return tally,
        }
    }
    // A keep threshold over 2^53, so the comparison is exact for a probability of 1.
    let threshold = (options.keep_probability * (1u64 << 53) as f64) as u64;
    while !open.is_empty() {
        let index = pick(&open, &mut rng);
        let file = &mut open[index];
        let next = match file.reader.next_game_waiting(&file.path) {
            Ok(next) => next,
            Err(Stopped) => return tally,
        };
        let Some((game_index, collected)) = next else {
            open.swap_remove(index);
            match claim(files, next_file, &wait) {
                Ok(Some(next)) => open.push(next),
                Ok(None) => {}
                Err(Stopped) => return tally,
            }
            continue;
        };
        let count = collected.position_count() as u64;
        file.left = file.left.saturating_sub(count);
        let (positions, keeps) = self_play_game_kept(&collected, game_index);
        let mut labels: Vec<Vec<f32>> =
            options.lambdas.iter().map(|&lambda| self_play_game_labels(&collected, lambda, game_index, &options.curve)).collect();
        if let Some(tablebase) = &options.tablebase {
            for (ply, label) in self_play_game_tablebase(&collected, tablebase, game_index).into_iter().enumerate() {
                if let Some(label) = label {
                    labels.iter_mut().for_each(|set| set[ply] = label);
                }
            }
        }
        // A kept position is a searched one, so its ply is below the searched count.
        let n = u16::try_from(collected.scores_pawns().len()).expect("a game of fewer than 65536 searched positions");
        let kept: Vec<CacheRecord> = positions
            .into_iter()
            .enumerate()
            .filter(|&(ply, _)| keeps[ply] && rng.random_range(0..1u64 << 53) < threshold)
            .map(|(ply, canonical)| CacheRecord {
                canonical,
                labels: labels.iter().map(|set| set[ply]).collect(),
                progress: Progress { i: ply as u16, n },
            })
            .collect();
        tally.games += 1;
        tally.positions += count;
        if !kept.is_empty() && sender.send(kept).is_err() {
            return tally;
        }
    }
    tally
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_is_fixed_by_this_code() {
        // The stream draws its orders from it: changing it would change them.
        assert_eq!(splitmix64(0), 0xE220_A839_7B1D_CDAF);
    }

    /// Two games of different lengths and scores, so their labels differ.
    const GAMES: &str = concat!(
        "[Result \"1-0\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Kd2 {1.50} Kd7 {-1.20}\n1-0\n\n",
        "[Result \"0-1\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Ke2 {-0.40} Ke7 {0.30} 2. Kf2 {-0.90}\n0-1\n\n",
    );
    /// The positions of one copy of `GAMES`: 3 and 4.
    const PAIR_POSITIONS: u64 = 7;
    /// The searched ones, every position but each game's final one: 2 and 3.
    const PAIR_SEARCHED: u64 = 5;

    fn fixture_dir(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("smithy-pgn-stream-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("failed to create the fixture directory");
        dir.to_str().unwrap().replace('\\', "/")
    }

    /// One file per entry of `pairs`, each holding that many copies of `GAMES`.
    fn fixture_files(dir: &str, pairs: &[u64]) -> Vec<StreamFile> {
        pairs
            .iter()
            .enumerate()
            .map(|(index, &count)| {
                let path = format!("{dir}/gamma-{index}.pgn.zst");
                let compressed = zstd::encode_all(GAMES.repeat(count as usize).as_bytes(), 3).expect("failed to compress");
                std::fs::write(&path, compressed).expect("failed to write the fixture");
                StreamFile { path, positions: count * PAIR_POSITIONS }
            })
            .collect()
    }

    fn options(threads: usize, open_files: usize, keep_probability: f64) -> StreamOptions {
        StreamOptions {
            lambdas: vec![0.7, 1.0],
            threads,
            open_files,
            keep_probability,
            seed: 1,
            tablebase: None,
            curve: Curve::Atan(kalo_chess::labels::LC0_FILES),
            stop: None,
        }
    }

    #[test]
    fn positions_not_kept_are_read_and_counted_but_never_sent() {
        let dir = fixture_dir("searched");
        let files = fixture_files(&dir, &[3, 2]);
        let mut stream = PgnStream::start(files, options(2, 2, 1.0));
        let mut sent = 0u64;
        while stream.next().is_some() {
            sent += 1;
        }
        // 5 copies of two games, 7 positions a copy, 2 of them final.
        assert_eq!(stream.finish(), Tally { games: 10, positions: 5 * PAIR_POSITIONS });
        assert_eq!(sent, 5 * PAIR_SEARCHED);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_searched_position_of_every_file_comes_out_exactly_once() {
        let dir = fixture_dir("once");
        // More files than open slots, of unequal sizes: 24 copies of two games.
        let files = fixture_files(&dir, &[1, 5, 2, 8, 3, 1, 4]);
        let read: u64 = files.iter().map(|file| file.positions).sum();
        let expected = 24 * PAIR_SEARCHED;
        let mut stream = PgnStream::start(files, options(3, 4, 1.0));
        let mut labels: Vec<(f32, f32)> = Vec::new();
        while let Some(record) = stream.next() {
            assert_eq!(record.labels.len(), 2, "one label per lambda");
            labels.push((record.labels[0], record.labels[1]));
        }
        assert_eq!(stream.finish(), Tally { games: 48, positions: read });
        assert_eq!(labels.len() as u64, expected);

        // The same multiset of label pairs as the games labelled one by one, each lambda in its
        // column, each game's final position left out.
        let mut reader_labels = Vec::new();
        for index in 0..7 {
            let path = format!("{dir}/gamma-{index}.pgn.zst");
            let mut reader = SelfPlaySegmentReader::open(&path);
            while let Some((game_index, collected)) = reader.next_game(&path) {
                let keeps = self_play_game_kept(&collected, game_index).1;
                let first = self_play_game_labels(&collected, 0.7, game_index, &Curve::Atan(kalo_chess::labels::LC0_FILES));
                let second = self_play_game_labels(&collected, 1.0, game_index, &Curve::Atan(kalo_chess::labels::LC0_FILES));
                reader_labels.extend(first.into_iter().zip(second).zip(keeps).filter(|&(_, keep)| keep).map(|(pair, _)| pair));
            }
        }
        let order = |a: &(f32, f32), b: &(f32, f32)| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1));
        labels.sort_by(order);
        reader_labels.sort_by(order);
        assert_eq!(labels, reader_labels);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn random_skipping_keeps_about_its_share() {
        let dir = fixture_dir("skip");
        let files = fixture_files(&dir, &[300, 300]);
        let mut stream = PgnStream::start(files, options(2, 2, 0.25));
        let mut kept = 0u64;
        while stream.next().is_some() {
            kept += 1;
        }
        let tally = stream.finish();
        assert_eq!(tally.positions, 600 * PAIR_POSITIONS, "every position is read, kept or not");
        assert!((650..=850).contains(&kept), "kept {kept} of 3000 at 0.25");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dropping_the_stream_mid_epoch_ends_every_worker() {
        let dir = fixture_dir("drop");
        let files = fixture_files(&dir, &[500, 500, 500, 500]);
        let mut stream = PgnStream::start(files, options(4, 4, 1.0));
        assert!(stream.next().is_some());
        // Joins every worker: a worker blocked on the full channel must notice the receiver is gone.
        let tally = stream.finish();
        assert!(tally.positions < 2000 * PAIR_POSITIONS, "stopped well before the end, at {}", tally.positions);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_pick_follows_the_positions_left() {
        let dir = fixture_dir("pick");
        let files = fixture_files(&dir, &[1, 1]);
        let mut open: Vec<OpenFile> = files
            .iter()
            .map(|file| OpenFile { path: file.path.clone(), reader: SelfPlaySegmentReader::open(&file.path), left: 0 })
            .collect();
        open[0].left = 900;
        open[1].left = 100;
        let mut rng = StdRng::seed_from_u64(3);
        let first = (0..10_000).filter(|_| pick(&open, &mut rng) == 0).count();
        assert!((8_700..=9_300).contains(&first), "picked the heavier file {first} times in 10,000");
        std::fs::remove_dir_all(&dir).ok();
    }
}
