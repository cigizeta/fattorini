//! Lc0's source, the smithy's Convert job: converts Lc0's published training tars into PGN segments
//! in `test/` and `train/` of its corpus (`LC0_ROOT`, `data/lc0`, each comment carrying the
//! visit counts too), in the shape the server's self-play writes, so the
//! server's `lc0-` runs freeze their test set from the one and train on the other as they read their
//! own games.
//!
//! **A child process.** The Convert job runs this as the smithy's own child (`CHILD_COMMAND`), its
//! lines prefixed into the smithy's terminal, beside the arena job. Its Stop kills it, curls and all,
//! as Ctrl+C ends it: the tar in flight is abandoned, and converted again from its download, or from
//! its start, next time.
//!
//! **Whole tars only.** Tars are taken in date order from `configs/lc0.yaml`'s `from`. Before one is
//! downloaded, its output is predicted from its listed size and the largest ratio of PGN bytes to tar
//! bytes seen so far; if that would take the corpus past `cap_bytes`, the run stops there, and raising
//! the cap later resumes with that same tar. A started tar always finishes, so a low prediction
//! overshoots by its error on one tar. The first tar has nothing to predict from and always converts.
//! A corpus always has a training tar too: while none is in `train/`, the next tar goes there whatever
//! the cap, so a small cap, a dev Convert's, never leaves nothing to train on.
//!
//! **`data/lc0/tars.tsv`** on the remote holds one row per finished tar: where it went, its bytes, the
//! PGN bytes it produced, its games and positions. A row is written only once every segment of its
//! tar is on the remote, so it is also the list of tars done. A run stopped mid-tar leaves no row:
//! the next run converts that tar again, from its download if it finished, writing the same segments.
//!
//! **Standard chess only.** test91's book also opens Chess960 and Chess324 games; they are skipped
//! and counted (`convert::is_standard_start`).
//!
//! **The test set.** Tars go whole to `test/` until it holds `test.games` games, then to `train/`.
//! The earliest tars are the test set, so later tars only ever add training games.

mod convert;
mod moves;
mod record;
pub(crate) mod rewrite;
#[cfg(test)]
mod policy_size;
#[cfg(test)]
mod second_move;
mod tar;

use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::process::{self, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use fattorini::pipeline::{preprocessing_config, segments};

use super::remote;

const CONFIG_PATH: &str = "configs/lc0.yaml";

/// The smithy's own subcommand the Convert job runs this as: no command for anyone to type, so it is
/// in no usage line.
pub(crate) const CHILD_COMMAND: &str = "lc0-convert";

/// The corpus the Convert fills and the forges read: `data/lc0`, every comment carrying the
/// visit counts a policy is trained on (`convert::visit_counts`). Its segments, `tars.tsv` and
/// `complete.txt` all live under it.
pub(crate) const LC0_ROOT: &str = preprocessing_config::LC0_ROOT;
const STAGING_DIR: &str = "data/lc0/staging";

/// The staging directory, where the tars wait for their conversion.
fn staging_dir() -> String {
    STAGING_DIR.to_string()
}
const TARS_PATH: &str = "data/lc0/tars.tsv";
/// Written to the remote when a run stops at the cap: the corpus is whole, and the server may train on it.
const COMPLETE_PATH: &str = "data/lc0/complete.txt";
const TARS_HEADER: &str = "tar\tdir\ttar_bytes\tpgn_bytes\tgames\tskipped\tpositions\tsegments";

/// The tars a run skipped as too slow (`SKIP_AFTER`), one row each time: never done, and passed over
/// by later runs once they have `GIVE_UP_AFTER` rows.
const SKIPPED_PATH: &str = "data/lc0/skipped.tsv";
const SKIPPED_HEADER: &str = "tar\tskipped_at_unix\tbytes\tbytes_per_second";

/// The runs a tar is skipped in before later runs pass it over untried: a run judges a tar once, so
/// its rows count its runs. One (the developer, 2026-10-31: "a skipped tar is skipped for good"): a
/// slow tar's slowness is the file's (the same tars slow in three runs over two days), and retrying
/// cost every restart about 35 minutes before the first new tar, both connections waiting out their
/// two minutes on each. Deleting a tar's rows from `SKIPPED_PATH` has it tried again.
const GIVE_UP_AFTER: usize = 1;

/// The size of an empty tar, which Lc0 still writes some hours: two zero blocks, padded.
const EMPTY_TAR_BYTES: u64 = 10240;

/// The segments' zstd level, as `arena`'s: Drive's space is the tightest.
const COMPRESSION_LEVEL: i32 = 19;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Lc0Config {
    /// The run's directory listing, ending with `/`.
    url: String,
    /// Every tar's name up to its date: `training-run2-test91-`.
    prefix: String,
    /// The first tar's date, `YYYYMMDD-HHMM`.
    from: String,
    cap_bytes: u64,
    /// The tars allowed in staging at once, downloaded or downloading but not yet converted.
    staging_bytes: u64,
    test: TestConfig,
    segment: u64,
    /// The frozen test set every `lc0-` run's generation 1 is judged on, drawn from `test/`.
    test_positions: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TestConfig {
    games: u64,
}

/// One row of `tars.tsv`.
#[derive(Clone, Debug, PartialEq)]
struct TarRow {
    tar: String,
    /// `test`, `train`, or `none` for an empty tar.
    dir: String,
    tar_bytes: u64,
    pgn_bytes: u64,
    /// The games converted.
    games: u64,
    /// The games not converted: not standard chess (Chess960, Chess324).
    skipped: u64,
    positions: u64,
    segments: u64,
}

/// The download workers, one connection each: about 2 MB/s each from storage.lczero.org, within
/// the home line's 5 MB/s (the developer, 2026-10-17). The page's `CONVERT_CONNECTIONS` follows it.
const CONNECTIONS: usize = 2;

/// How often a download's bar reads its `.tmp`'s size: curl runs silent, so the file is the
/// only measure of its progress.
const DOWNLOAD_POLL: Duration = Duration::from_millis(500);

/// A download below this many bytes a second for `SLOW_SECONDS` in a row is abandoned for a new
/// connection: far above the 40 KiB/s of a slow one, far below the 1.5 to 2 MiB/s of a normal one.
/// A connection that never sends its first byte counts too, so with the network down a worker keeps
/// trying, and the Convert goes on by itself when the network returns.
const SLOW_BYTES_PER_SECOND: u64 = 256 * 1024;

/// Short: the next attempt resumes where this one stopped, so dropping a slow connection loses no
/// bytes, only the moment spent on it.
const SLOW_SECONDS: u64 = 15;

/// A tar averaging below `SLOW_BYTES_PER_SECOND` over its first `SKIP_AFTER`, resumes included, is
/// skipped: its slowness is the file's on Lc0's side (2026-10-30, one tar at about 9 KB/s on every
/// connection while its neighbours gave 1.75 MB/s in the same minute), and the converter, taking the
/// tars in date order, would wait hours for it. Long enough that a short dip skips no good tar.
const SKIP_AFTER: Duration = Duration::from_secs(120);

/// Whether a tar that received `bytes` in `elapsed` since its first attempt is skipped: only at
/// `SKIP_AFTER` or later, and only once, its first `SKIP_AFTER` judged; after it, a dip is resumed.
fn too_slow(bytes: u64, elapsed: Duration, judged: bool) -> bool {
    !judged && elapsed >= SKIP_AFTER && bytes < SLOW_BYTES_PER_SECOND * SKIP_AFTER.as_secs()
}

/// How a tar's download ended.
#[derive(Debug, PartialEq)]
enum Fetched {
    Downloaded,
    Skipped { bytes: u64, bytes_per_second: u64 },
}

/// The failed attempts a tar is retried, `RETRY_DELAY` apart, before the run fails. Attempts made
/// while offline (`online`) do not count.
const RETRIES: u32 = 5;

/// How long `online`'s look at the server's listing may take: one page of the tars' names, read in
/// about a second on a working line, so twenty seconds without it is no line.
const ONLINE_TIMEOUT_SECONDS: u64 = 20;

/// The listing a tar's URL sits in: the URL up to its last slash, included.
fn listing_url(tar_url: &str) -> &str {
    &tar_url[..tar_url.rfind('/').map_or(tar_url.len(), |slash| slash + 1)]
}

/// Whether the server answers at all: its listing, the directory of `tar_url`, read whole within
/// `ONLINE_TIMEOUT_SECONDS`. What tells a slow tar from no Internet (the developer, 2026-10-31): both
/// receive too few bytes, but only a slow tar comes with a server that answers, and only the slow
/// tar is skipped for good (`GIVE_UP_AFTER`).
fn online(tar_url: &str) -> bool {
    Command::new("curl.exe")
        .args(["-s", "-f", "-L", "--max-time", &ONLINE_TIMEOUT_SECONDS.to_string(), "-o", "NUL", listing_url(tar_url)])
        .status()
        .is_ok_and(|status| status.success())
}

const RETRY_DELAY: Duration = Duration::from_secs(10);

/// How one of curl's attempts at a tar ended.
#[derive(Debug, PartialEq)]
enum Attempt {
    Whole,
    /// Below `SLOW_BYTES_PER_SECOND` for `SLOW_SECONDS`: curl's "operation too slow", exit 28.
    Slow,
    /// A resumed attempt the server answered without its range: curl's exit 33.
    NoRange,
    Failed,
}

/// The attempt curl's exit `code` reports; `None`, killed, is a failure.
fn attempt(code: Option<i32>) -> Attempt {
    match code {
        Some(0) => Attempt::Whole,
        Some(28) => Attempt::Slow,
        Some(33) => Attempt::NoRange,
        _ => Attempt::Failed,
    }
}

/// curl's arguments for one attempt at `url` into `tmp_path`: `resume` continues the `.tmp` from its
/// size (`-C -`), otherwise curl starts it over.
fn curl_args(url: &str, tmp_path: &str, resume: bool) -> Vec<String> {
    let mut args: Vec<String> = ["-s", "-S", "-f", "-L", "--speed-limit"].iter().map(|arg| arg.to_string()).collect();
    args.extend([SLOW_BYTES_PER_SECOND.to_string(), "--speed-time".to_string(), SLOW_SECONDS.to_string()]);
    if resume {
        args.extend(["-C".to_string(), "-".to_string()]);
    }
    args.extend(["-o".to_string(), tmp_path.to_string(), url.to_string()]);
    args
}

/// Where a pending tar stands. Each slot only moves forward, in this order.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot {
    /// No worker has taken it yet.
    Unclaimed,
    /// A worker has its size and waits for its turn to be approved.
    Sized(u64),
    /// Downloading.
    Approved(u64),
    /// In staging, or empty: ready to convert.
    Ready(u64),
    /// Too slow (`SKIP_AFTER`): recorded in `SKIPPED_PATH`, passed over without a row.
    Skipped(u64),
}

impl Slot {
    fn tar_bytes(self) -> Option<u64> {
        match self {
            Slot::Unclaimed => None,
            Slot::Sized(bytes) | Slot::Approved(bytes) | Slot::Ready(bytes) | Slot::Skipped(bytes) => Some(bytes),
        }
    }
}

/// What the download workers and the converter share, under one lock.
struct Shared {
    rows: Vec<TarRow>,
    /// One per pending tar, in date order.
    slots: Vec<Slot>,
    /// The next tar a worker takes.
    next_claim: usize,
    /// The next tar to approve: tars are approved in date order, so each one's cap check counts
    /// every tar before it.
    next_approve: usize,
    /// The tar converting, or converted next: every tar before it is done.
    converting: usize,
    /// The bytes of the tars approved and not yet converted, downloaded or downloading.
    staged_bytes: u64,
    /// Set by the converter at the cap: the workers take nothing more.
    stopped: bool,
}

impl Shared {
    /// The predicted output of the tars approved and not yet converted.
    fn ahead(&self) -> u64 {
        self.slots[self.converting..self.next_approve]
            .iter()
            .filter(|slot| !matches!(slot, Slot::Skipped(_)))
            .filter_map(|slot| slot.tar_bytes())
            .filter(|&tar_bytes| tar_bytes != EMPTY_TAR_BYTES)
            .map(|tar_bytes| predicted(&self.rows, tar_bytes).unwrap_or(0))
            .sum()
    }
}

type Pipeline = Arc<(Mutex<Shared>, Condvar)>;

/// What to do with the next tar.
#[derive(Debug, PartialEq)]
enum Decision {
    Stop { used: u64, predicted: u64 },
    Convert { dir: &'static str },
}

/// What starts a line of the child's that reports its board, not one to print: the rest is an
/// `Lc0Live` as JSON, which the smithy keeps for its page (`keep`).
pub(crate) const PROGRESS_LINE: &str = "@progress ";

/// How often the child reports its board.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// The Convert job's progress as the page's Corpus section words it, in fixed places so that its
/// height never changes while the job runs: one line per connection, `None` while that worker
/// downloads nothing; the conversion's, `None` while the converter waits for a tar; the corpus
/// towards the cap, the one graphical bar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Lc0Live {
    downloads: Vec<Option<Line>>,
    conversion: Option<Line>,
    cap: Line,
}

/// One place's bytes, as the training's round gives its counts: `rate` in bytes a second and
/// `eta_seconds` at it, both measured this run and `None` before any progress.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Line {
    /// The tar's date; empty for the cap.
    tar: String,
    done: u64,
    total: u64,
    rate: Option<f64>,
    eta_seconds: Option<f64>,
}

/// One place's counts: `done` of `total` bytes, `from` where they stood at `started`.
struct Flow {
    tar: String,
    done: u64,
    total: u64,
    from: u64,
    started: Instant,
}

impl Flow {
    fn new(tar: &str, done: u64, total: u64) -> Flow {
        Flow { tar: tar.to_string(), done, total, from: done, started: Instant::now() }
    }

    fn line(&self) -> Line {
        let (observed, seconds) = (self.done.saturating_sub(self.from), self.started.elapsed().as_secs_f64());
        let rate = (observed > 0 && seconds > 0.0).then(|| observed as f64 / seconds);
        let eta_seconds = rate.map(|rate| self.total.saturating_sub(self.done) as f64 / rate);
        Line { tar: self.tar.clone(), done: self.done, total: self.total, rate, eta_seconds }
    }
}

/// The counts behind the child's `Lc0Live`: each worker writes its own download's place, the
/// converter the conversion's and the cap's, the bytes the cap rule counts.
struct Board {
    downloads: [Option<Flow>; CONNECTIONS],
    conversion: Option<Flow>,
    cap: Option<Flow>,
}

static BOARD: Mutex<Board> = Mutex::new(Board { downloads: [const { None }; CONNECTIONS], conversion: None, cap: None });

fn board() -> std::sync::MutexGuard<'static, Board> {
    BOARD.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The board as the page words it; `None` before the cap's counts are known.
fn board_live() -> Option<Lc0Live> {
    let board = board();
    Some(Lc0Live {
        downloads: board.downloads.iter().map(|flow| flow.as_ref().map(Flow::line)).collect(),
        conversion: board.conversion.as_ref().map(Flow::line),
        cap: board.cap.as_ref()?.line(),
    })
}

/// A tar's bytes as the converter reads them, counted on the board's conversion place.
struct Counted<R> {
    inner: R,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        if let Some(flow) = board().conversion.as_mut() {
            flow.done += read as u64;
        }
        Ok(read)
    }
}

/// The child's last report, in the smithy: for its page's `state.json`.
static KEPT: Mutex<Option<Lc0Live>> = Mutex::new(None);

/// Keeps `live` as the child's last report, `None` once the child has ended.
pub(crate) fn keep(live: Option<Lc0Live>) {
    *KEPT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = live;
}

/// The child's last report, if one runs.
pub(crate) fn kept() -> Option<Lc0Live> {
    return KEPT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
}

/// The conversion, to the cap, as the Convert job's child: its output a pipe the smithy prefixes into
/// its own terminal, so it draws no bars, only lines, and reports its board as `PROGRESS_LINE`s.
pub(crate) fn run(rest: &[String]) {
    match rest {
        [] => {}
        _ => fail(format!("{CHILD_COMMAND} takes nothing: everything is in {CONFIG_PATH}")),
    }
    if let Err(error) = remote::check_mounted() {
        fail(error.to_string());
    }
    // Convert writes through Lc0's curve: never into a corpus still in the old one, which is rewritten
    // first; an empty corpus is in Lc0's curve from its first segment.
    if !rewrite::is_rewritten() {
        if !read_rows().is_empty() {
            fail(format!("{LC0_ROOT} is still in the old curve: run `smithy {}` before converting more", rewrite::REWRITE_COMMAND));
        }
        rewrite::mark_rewritten().unwrap_or_else(|error| fail(error));
    }
    let config = load_config(CONFIG_PATH);
    let table = moves::table();
    // The board, reported until the process ends.
    std::thread::spawn(|| loop {
        if let Some(Ok(json)) = board_live().map(|live| serde_json::to_string(&live)) {
            println!("{PROGRESS_LINE}{json}");
        }
        std::thread::sleep(PROGRESS_EVERY);
    });
    for dir in [format!("{LC0_ROOT}/test"), format!("{LC0_ROOT}/train")] {
        // Segments here belong to a tar left unfinished: they are made again.
        if Path::new(&dir).exists() {
            fs::remove_dir_all(&dir).unwrap_or_else(|error| fail(format!("failed to clear {dir}: {error}")));
        }
    }
    let staging = staging_dir();
    fs::create_dir_all(&staging).unwrap_or_else(|error| fail(format!("failed to create {staging}: {error}")));
    // A `.tar` in staging is complete (renamed from `.tmp` once curl succeeded) and is converted
    // without downloading it again; a `.tmp` is a download cut short. A run resumes its own `.tmp`s
    // (`download`), but not another run's, whose file nothing vouches is the same tar: Ctrl+C deletes
    // the `.tmp`s and ends the run, and startup deletes any left by a run ended otherwise.
    remove_staged("tmp");
    ctrlc::set_handler(|| {
        STOPPING.store(true, Ordering::SeqCst);
        // The curls die of the same Ctrl+C, but close their `.tmp`s a moment after this runs, and
        // Windows deletes no open file: try again until they let go (2026-10-17, both were missed).
        let mut removed = remove_staged("tmp");
        for _ in 0..30 {
            if removed {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            removed = remove_staged("tmp");
        }
        let partial = if removed { "partial downloads deleted" } else { "partial downloads left to the next start" };
        eprintln!("\nstopped: {partial}, complete tars kept in {}", staging_dir());
        process::exit(130);
    })
    .unwrap_or_else(|error| fail(format!("failed to install the Ctrl+C handler: {error}")));

    let rows = read_rows();
    let used: u64 = rows.iter().map(|row| row.pgn_bytes).sum();
    board().cap = Some(Flow::new("", used, config.cap_bytes));
    let given_up = read_given_up();
    if !given_up.is_empty() {
        println!(
            "passed over, skipped as too slow before ({SKIPPED_PATH}, delete their rows to try them again): {}",
            given_up.join(", ")
        );
    }
    let pending: Arc<Vec<String>> = Arc::new(
        list_tars(&config)
            .into_iter()
            .filter(|stem| !rows.iter().any(|row| row.tar == *stem) && !given_up.contains(stem))
            .collect(),
    );
    let shared = Shared {
        rows,
        slots: vec![Slot::Unclaimed; pending.len()],
        next_claim: 0,
        next_approve: 0,
        converting: 0,
        staged_bytes: 0,
        stopped: false,
    };
    let pipeline: Pipeline = Arc::new((Mutex::new(shared), Condvar::new()));
    // The workers download whenever the cap and staging allow, independently of the conversion, so
    // a connection never waits for a conversion to end. They are never joined: at the cap, any
    // download still running ends with the process, and the next run starts that tar over: a run
    // resumes its own `.tmp`s alone.
    for worker in 0..CONNECTIONS {
        let (pipeline, pending) = (Arc::clone(&pipeline), Arc::clone(&pending));
        let (url, prefix, cap_bytes, staging_bytes) = (config.url.clone(), config.prefix.clone(), config.cap_bytes, config.staging_bytes);
        std::thread::spawn(move || fetch_tars(worker, &pipeline, &pending, &url, &prefix, cap_bytes, staging_bytes));
    }

    // The converter takes the tars in date order: the earliest are the test set.
    let (lock, wake) = &*pipeline;
    let mut invariance_printed = false;
    let mut skipped: Vec<String> = Vec::new();
    for (index, stem) in pending.iter().enumerate() {
        let shared = wake.wait_while(locked(lock), |shared| shared.slots[index] == Slot::Unclaimed).expect("a worker panicked");
        let tar_bytes = shared.slots[index].tar_bytes().expect("a claimed tar has its size");
        let mut dir = "none";
        if tar_bytes != EMPTY_TAR_BYTES {
            match decide(&shared.rows, tar_bytes, config.cap_bytes, config.test.games) {
                Decision::Stop { used, predicted } => {
                    let mut shared = shared;
                    shared.stopped = true;
                    wake.notify_all();
                    println!(
                        "stopping before {stem}: data/lc0 holds {used} bytes, and its predicted {predicted} would pass the cap of {}",
                        config.cap_bytes
                    );
                    write_complete(&shared.rows);
                    // The tars downloaded ahead on their predicted outputs are never converted: a
                    // complete corpus is never extended (`write_complete`), so they only take disk.
                    if !remove_staged("tar") {
                        println!("failed to delete every tar left in {}", staging_dir());
                    }
                    say_skipped(&skipped);
                    return;
                }
                Decision::Convert { dir: chosen } => dir = chosen,
            }
        }
        // Every tar before this one is converted and staging holds nothing, so a worker always
        // approves it: the wait ends. The wait goes unannounced: the tar's download bar shows it.
        let mut shared =
            wake.wait_while(shared, |shared| !matches!(shared.slots[index], Slot::Ready(_) | Slot::Skipped(_))).expect("a worker panicked");
        // Too slow to wait for: no row, so not done, and recorded so later runs pass it over
        // (`GIVE_UP_AFTER`).
        if let Slot::Skipped(_) = shared.slots[index] {
            skipped.push(stem.clone());
            shared.converting = index + 1;
            wake.notify_all();
            continue;
        }
        if tar_bytes == EMPTY_TAR_BYTES {
            let row = TarRow { tar: stem.clone(), dir: dir.to_string(), tar_bytes, pgn_bytes: 0, games: 0, skipped: 0, positions: 0, segments: 0 };
            append_row(&mut shared.rows, row);
            shared.converting = index + 1;
            wake.notify_all();
            continue;
        }
        drop(shared);

        let used_before: u64 = locked(lock).rows.iter().map(|row| row.pgn_bytes).sum();
        let row = convert_tar(&config, &table, stem, dir, tar_bytes, &mut invariance_printed, |pgn_bytes| {
            if let Some(cap) = board().cap.as_mut() {
                cap.done = used_before + pgn_bytes;
            }
        });
        let line = format!(
            "{stem}: {} games ({} not standard chess, skipped), {} positions, {} PGN bytes in {} segment(s) to {dir}",
            row.games, row.skipped, row.positions, row.pgn_bytes, row.segments
        );
        let mut shared = locked(lock);
        append_row(&mut shared.rows, row);
        shared.staged_bytes -= tar_bytes;
        shared.converting = index + 1;
        wake.notify_all();
        // The bytes the cap rule counts (`passes_cap`), this tar's included: 100% is where the run stops.
        let used: u64 = shared.rows.iter().map(|row| row.pgn_bytes).sum();
        drop(shared);
        println!("{line} ({:.1}% of the cap)", 100.0 * used as f64 / config.cap_bytes as f64);
    }
    if skipped.is_empty() {
        println!("every tar from {} on is converted", config.from);
    } else {
        println!("every tar from {} on is converted but those skipped", config.from);
    }
    say_skipped(&skipped);
}

/// The run's last word on the tars it skipped, if any.
fn say_skipped(skipped: &[String]) {
    if !skipped.is_empty() {
        println!(
            "skipped as too slow, for good ({SKIPPED_PATH}, delete their rows to try them again): {}",
            skipped.join(", ")
        );
    }
}

/// `skipped.tsv`'s row for `tar`, skipped at `skipped_at` (Unix seconds) after `bytes` at
/// `bytes_per_second`.
fn skipped_row(tar: &str, skipped_at: u64, bytes: u64, bytes_per_second: u64) -> String {
    format!("{tar}\t{skipped_at}\t{bytes}\t{bytes_per_second}\n")
}

/// The tars `skipped` (`SKIPPED_PATH`'s text) holds `GIVE_UP_AFTER` rows or more of, in date order.
fn given_up(skipped: &str) -> Vec<String> {
    let mut runs: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for line in skipped.lines().skip(1) {
        if let Some(tar) = line.split('\t').next().filter(|tar| !tar.is_empty()) {
            *runs.entry(tar).or_default() += 1;
        }
    }
    runs.into_iter().filter(|&(_, count)| count >= GIVE_UP_AFTER).map(|(tar, _)| tar.to_string()).collect()
}

/// `given_up` of `SKIPPED_PATH` on the remote; none without it, or when it cannot be read, so every
/// tar is tried.
fn read_given_up() -> Vec<String> {
    if !Path::new(&remote::remote_path(SKIPPED_PATH)).exists() {
        return Vec::new();
    }
    match remote::cat(SKIPPED_PATH) {
        Ok(text) => given_up(&text),
        Err(error) => {
            println!("failed to read {SKIPPED_PATH}, every skipped tar tried again: {error}");
            Vec::new()
        }
    }
}

/// Appends `tar`'s skip to `SKIPPED_PATH` on the remote, its header first when it is new. A failure
/// is only said: the skip itself needs no record to be tried again, `tars.tsv` lacking its row.
fn record_skipped(tar: &str, bytes: u64, bytes_per_second: u64) {
    let mut text = if Path::new(&remote::remote_path(SKIPPED_PATH)).exists() {
        remote::cat(SKIPPED_PATH).unwrap_or_else(|_| format!("{SKIPPED_HEADER}\n"))
    } else {
        format!("{SKIPPED_HEADER}\n")
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    text.push_str(&skipped_row(tar, now, bytes, bytes_per_second));
    if let Err(error) = remote::write_small(SKIPPED_PATH, &text) {
        println!("failed to record {tar}'s skip in {SKIPPED_PATH}: {error}");
    }
}

fn locked(lock: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    lock.lock().expect("a worker panicked")
}

/// One download worker, number `worker` on the board: takes the next tar in date order, reads its
/// size, waits for its turn to be approved, downloads it, and takes the next, until every tar is
/// taken or the converter stops.
fn fetch_tars(worker: usize, pipeline: &Pipeline, pending: &[String], url: &str, prefix: &str, cap_bytes: u64, staging_bytes: u64) {
    let (lock, wake) = &**pipeline;
    loop {
        let index = {
            let mut shared = locked(lock);
            if shared.stopped || shared.next_claim == pending.len() {
                return;
            }
            shared.next_claim += 1;
            shared.next_claim - 1
        };
        let stem = &pending[index];
        let tar_url = format!("{url}{stem}.tar");
        let tar_bytes = remote_size(&tar_url);
        let tar_path = format!("{}/{stem}.tar", staging_dir());
        // An empty tar, or one left whole in staging by an earlier run, needs no connection.
        let fetch = tar_bytes != EMPTY_TAR_BYTES && !Path::new(&tar_path).exists();

        let mut shared = locked(lock);
        shared.slots[index] = Slot::Sized(tar_bytes);
        wake.notify_all();
        let mut shared = wake
            .wait_while(shared, |shared| {
                let turn = shared.next_approve == index;
                !shared.stopped
                    && !(turn && (!fetch || may_fetch(&shared.rows, shared.ahead(), shared.staged_bytes, tar_bytes, cap_bytes, staging_bytes)))
            })
            .expect("a worker panicked");
        if shared.stopped {
            return;
        }
        shared.next_approve += 1;
        if tar_bytes != EMPTY_TAR_BYTES {
            shared.staged_bytes += tar_bytes;
        }
        shared.slots[index] = if fetch { Slot::Approved(tar_bytes) } else { Slot::Ready(tar_bytes) };
        wake.notify_all();
        drop(shared);

        if fetch {
            let date = &stem[prefix.len()..];
            let fetched = download(&tar_url, &tar_path, tar_bytes, worker, date);
            let mut shared = locked(lock);
            shared.slots[index] = match fetched {
                Fetched::Downloaded => Slot::Ready(tar_bytes),
                Fetched::Skipped { bytes, bytes_per_second } => {
                    println!(
                        "{date}: skipped, {} KiB/s over its first {} s with the server answering; recorded in {SKIPPED_PATH}, for good",
                        bytes_per_second / 1024,
                        SKIP_AFTER.as_secs()
                    );
                    record_skipped(stem, bytes, bytes_per_second);
                    // Its room in staging is free again: it holds nothing.
                    shared.staged_bytes -= tar_bytes;
                    Slot::Skipped(tar_bytes)
                }
            };
            wake.notify_all();
        }
    }
}

/// Whether a tar of `tar_bytes` may be downloaded now: its predicted output fits under `cap_bytes`
/// beside `rows` and the `ahead` predicted for the approved tars before it, and staging, holding
/// `staged_bytes`, has room for it within `staging_bytes`. An empty staging always has room, so a
/// tar larger than `staging_bytes` cannot stall the run. With no training tar yet and none approved
/// before it, it is the one `decide` sends to `train` whatever the cap, and may be fetched.
fn may_fetch(rows: &[TarRow], ahead: u64, staged_bytes: u64, tar_bytes: u64, cap_bytes: u64, staging_bytes: u64) -> bool {
    let fits = !passes_cap(rows, ahead, tar_bytes, cap_bytes) || (ahead == 0 && !has_train(rows));
    fits && (staged_bytes == 0 || staged_bytes + tar_bytes <= staging_bytes)
}

/// Whether a tar has gone to `train`.
fn has_train(rows: &[TarRow]) -> bool {
    rows.iter().any(|row| row.dir == "train")
}

/// Deletes every file in staging ending in `.{extension}`, and whether none is left: `tmp` for the
/// partial downloads, `tar` for the whole ones. A `.tmp` a curl still holds open cannot be deleted
/// yet, and stays.
fn remove_staged(extension: &str) -> bool {
    let Ok(entries) = fs::read_dir(staging_dir()) else { return true };
    let mut all_removed = true;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|found| found == extension) && fs::remove_file(&path).is_err() {
            all_removed = false;
        }
    }
    all_removed
}

/// Set by the Ctrl+C handler, which then ends the process.
static STOPPING: AtomicBool = AtomicBool::new(false);

fn fail(message: String) -> ! {
    // The curls share the console and die of the same Ctrl+C: their failures are not errors, and
    // the handler's own message and exit end the process.
    if STOPPING.load(Ordering::SeqCst) {
        loop {
            std::thread::park();
        }
    }
    eprintln!("{message}");
    process::exit(1);
}

/// Whether the corpus, `LC0_ROOT`, has stopped at its cap, so its `train` no longer grows: a training
/// run's epoch is fixed at its first round, and a run never resumes over another.
pub(crate) fn is_complete() -> bool {
    Path::new(&remote::remote_path(COMPLETE_PATH)).exists()
}

/// Records on the remote that the corpus is whole, with what it holds. Raising the cap afterwards
/// makes a new corpus: `complete.txt` is then deleted by hand, and new networks are trained on it.
fn write_complete(rows: &[TarRow]) {
    let train: Vec<&TarRow> = rows.iter().filter(|row| row.dir == "train").collect();
    let text = format!(
        "tars\t{}\ntrain_positions\t{}\ntrain_segments\t{}\npgn_bytes\t{}\n",
        rows.iter().filter(|row| row.games > 0).count(),
        train.iter().map(|row| row.positions).sum::<u64>(),
        train.iter().map(|row| row.segments).sum::<u64>(),
        rows.iter().map(|row| row.pgn_bytes).sum::<u64>()
    );
    remote::write_small(COMPLETE_PATH, &text).unwrap_or_else(|error| fail(format!("failed to write {COMPLETE_PATH}: {error}")));
    println!("{LC0_ROOT} is complete: {} on the remote says what it holds", remote::remote_path(COMPLETE_PATH));
}

fn load_config(path: &str) -> Lc0Config {
    read_config(path).unwrap_or_else(|message| fail(message))
}

/// `path` read and checked, for the smithy too, which must not exit on an error. A source is never
/// scaled: a scaled run reads a share of it.
fn read_config(path: &str) -> Result<Lc0Config, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("failed to read {path}: {error}"))?;
    let config: Lc0Config = serde_yaml::from_str(&text).map_err(|error| format!("{path}: {error}"))?;
    if config.segment == 0 || config.test.games == 0 || config.staging_bytes == 0 || config.test_positions == 0 || !config.url.ends_with('/') {
        return Err(format!("{path}: segment, test.games, test_positions and staging_bytes must be positive, and url must end with /"));
    }
    Ok(config)
}

/// The frozen test set every `lc0-` run's generation 1 is judged on, `configs/lc0.yaml`'s.
pub(crate) fn test_positions() -> Result<u64, String> {
    Ok(read_config(CONFIG_PATH)?.test_positions)
}

/// The page's line for the corpus: complete or not, and the tars downloaded and converted. The bytes
/// against the cap are the Convert's own line, under it while it runs (`Lc0Live`), not repeated here
/// (the developer, 2026-10-24).
pub(crate) fn summary() -> String {
    let rows = match remote_rows() {
        Ok(rows) => rows,
        Err(message) => return message,
    };
    // Staging is local: beside no running Convert, its tars are an earlier run's leftovers, most of
    // them converted since on another machine, so only the remote's count is shown.
    let staged = kept().is_some().then(staged_tars);
    summary_line(&rows, staged, is_complete())
}

/// `summary`'s words: the downloads only when `staged` is known, the tars staged by a Convert running here.
fn summary_line(rows: &[TarRow], staged: Option<usize>, complete: bool) -> String {
    let tars = rows.iter().filter(|row| row.games > 0).count();
    let state = if complete { "complete" } else { "not complete" };
    match staged {
        // A converted tar is deleted from staging, so the downloads are the rows plus the tars staged.
        Some(staged) => format!(
            "the Convert's {LC0_ROOT}, with visit counts, {state}: {} tars downloaded, {tars} converted",
            rows.len() + staged
        ),
        None => format!("the Convert's {LC0_ROOT}, with visit counts, {state}: {} tars converted", progress::grouped(tars)),
    }
}

/// The corpus's bytes the cap rule counts, and the cap, as `tars.tsv` has them: the page's corpus line
/// and bar while no Convert runs, so a finished corpus still shows how much of the cap it filled.
/// `None` when either cannot be read; `summary` says why.
pub(crate) fn cap_bytes() -> Option<(u64, u64)> {
    let config = read_config(CONFIG_PATH).ok()?;
    let rows = remote_rows().ok()?;
    Some((rows.iter().map(|row| row.pgn_bytes).sum(), config.cap_bytes))
}

/// `tars.tsv`'s rows on the remote, none before the first tar is done.
fn remote_rows() -> Result<Vec<TarRow>, String> {
    if !Path::new(&remote::remote_path(TARS_PATH)).exists() {
        return Ok(Vec::new());
    }
    remote::cat(TARS_PATH).map_err(|error| error.to_string()).and_then(|text| parse_rows(&text))
}

/// The tars downloaded whole into staging and not yet converted: a download in flight is still a
/// `.tmp`, and a tar converted is deleted.
fn staged_tars() -> usize {
    let Ok(entries) = fs::read_dir(staging_dir()) else { return 0 };
    entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "tar"))
        .count()
}

/// The next tar's fate: stop when its predicted output would pass the cap; otherwise its directory,
/// `test` while the test tars hold fewer than `test_games` games. A corpus always has a test tar and a
/// training tar (the developer, 2026-10-23): the first tar always converts, into `test`, and while no
/// tar is in `train` the next goes there whatever the cap, so a small cap (a dev Convert's, scaled)
/// cannot end with nothing to train on.
fn decide(rows: &[TarRow], tar_bytes: u64, cap_bytes: u64, test_games: u64) -> Decision {
    if passes_cap(rows, 0, tar_bytes, cap_bytes) {
        if !has_train(rows) {
            return Decision::Convert { dir: "train" };
        }
        let used: u64 = rows.iter().map(|row| row.pgn_bytes).sum();
        let predicted = predicted(rows, tar_bytes).expect("a tar passes the cap only by a prediction");
        return Decision::Stop { used, predicted };
    }
    let games_in_test: u64 = rows.iter().filter(|row| row.dir == "test").map(|row| row.games).sum();
    Decision::Convert { dir: if games_in_test < test_games { "test" } else { "train" } }
}

/// A tar's predicted PGN bytes: its size times the largest ratio of PGN bytes to tar bytes in
/// `rows`; `None` before any tar with games is converted.
fn predicted(rows: &[TarRow], tar_bytes: u64) -> Option<u64> {
    let ratio = rows
        .iter()
        .filter(|row| row.tar_bytes > 0 && row.games > 0)
        .map(|row| row.pgn_bytes as f64 / row.tar_bytes as f64)
        .fold(None, |largest: Option<f64>, ratio| Some(largest.map_or(ratio, |largest| largest.max(ratio))));
    ratio.map(|ratio| (tar_bytes as f64 * ratio).ceil() as u64)
}

/// Whether a tar of `tar_bytes` would take `data/lc0` past `cap_bytes`, beside what `rows` hold and
/// `ahead` more bytes predicted for the tars before it. Never with nothing to predict from.
fn passes_cap(rows: &[TarRow], ahead: u64, tar_bytes: u64, cap_bytes: u64) -> bool {
    let used: u64 = rows.iter().map(|row| row.pgn_bytes).sum();
    predicted(rows, tar_bytes).is_some_and(|predicted| used + ahead + predicted > cap_bytes)
}

/// The tars of the listing from `from` on, by name, which is date order.
fn list_tars(config: &Lc0Config) -> Vec<String> {
    let listing = curl_text(&["-s", "-S", "-f", "-L", &config.url]);
    let mut stems = tar_stems(&listing, &config.prefix);
    stems.retain(|stem| &stem[config.prefix.len()..] >= config.from.as_str());
    stems
}

/// Every `{prefix}YYYYMMDD-HHMM.tar` named in `listing`, without `.tar`, sorted and once each.
fn tar_stems(listing: &str, prefix: &str) -> Vec<String> {
    let mut stems = Vec::new();
    let mut rest = listing;
    while let Some(start) = rest.find(prefix) {
        let candidate = &rest[start..];
        let date = candidate.get(prefix.len()..prefix.len() + 13).unwrap_or("");
        let well_formed = date.len() == 13
            && date.char_indices().all(|(index, character)| if index == 8 { character == '-' } else { character.is_ascii_digit() })
            && candidate[prefix.len() + 13..].starts_with(".tar");
        if well_formed {
            stems.push(format!("{prefix}{date}"));
        }
        rest = &candidate[prefix.len()..];
    }
    stems.sort();
    stems.dedup();
    stems
}

/// A remote file's size, from its `Content-Length`.
fn remote_size(url: &str) -> u64 {
    let headers = curl_text(&["-s", "-S", "-f", "-I", "-L", url]);
    let length = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .filter_map(|(_, value)| value.trim().parse::<u64>().ok())
        .next_back();
    length.unwrap_or_else(|| fail(format!("{url} gave no Content-Length")))
}

/// `curl.exe`'s output, or a stop with its error.
fn curl_text(args: &[&str]) -> String {
    let output = Command::new("curl.exe")
        .args(args)
        .output()
        .unwrap_or_else(|error| fail(format!("failed to run curl.exe: {error}")));
    if !output.status.success() {
        fail(format!("curl.exe {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr)));
    }
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Downloads `url`, a tar of `tar_bytes` dated `date`, to `path`, through `{path}.tmp`, with curl
/// silent: worker `worker`'s place on the board follows the `.tmp`'s size instead, and is emptied
/// once the tar is whole.
///
/// storage.lczero.org sometimes resets a connection mid-tar (curl error 56, 2026-10-17), and a
/// failure here ends the whole run, so a failed attempt is retried, up to `RETRIES` times a tar. It
/// also sometimes serves a connection at about 40 KiB/s for hours while a new one gets 1.5 MiB/s
/// (2026-10-30), so an attempt too slow (`SLOW_BYTES_PER_SECOND`) is abandoned for a new connection,
/// without limit: slowness is no failure. The server serves byte ranges (a `206` at the start and
/// 400 MB into a tar, 2026-10-30; curl error 33 on 2026-10-17), so every attempt after the first
/// resumes the `.tmp` where the last stopped; one refused its range starts the `.tmp` over. A tar
/// slow over its first `SKIP_AFTER`, resumes included, is the file's slowness, not a connection's:
/// it is given up, its `.tmp` deleted (`too_slow`), but only while the server answers (`online`).
/// Offline, a tar is neither skipped nor failed: the worker waits for the line, and the tar's
/// `SKIP_AFTER` starts again once it is back.
fn download(url: &str, path: &str, tar_bytes: u64, worker: usize, date: &str) -> Fetched {
    let tmp_path = format!("{path}.tmp");
    let mut failures = 0;
    let mut resume = false;
    // When the tar's `SKIP_AFTER` began: its first attempt, or the network's return.
    let mut first = Instant::now();
    let mut judged = false;
    loop {
        let start = if resume { fs::metadata(&tmp_path).map_or(0, |metadata| metadata.len()) } else { 0 };
        // An attempt that got every byte yet did not exit cleanly: the size check below decides.
        if start >= tar_bytes {
            break;
        }
        // Each attempt's rate and ETA are measured afresh, from where it resumed, not from the one
        // before (2026-10-17: 64 KiB/s and 3.5 hours shown at a real 2 MiB/s).
        board().downloads[worker] = Some(Flow::new(date, start, tar_bytes));
        let mut child = Command::new("curl.exe")
            .args(curl_args(url, &tmp_path, resume))
            .spawn()
            .unwrap_or_else(|error| fail(format!("failed to run curl.exe: {error}")));
        let status = loop {
            if let Some(status) = child.try_wait().unwrap_or_else(|error| fail(format!("failed to wait for curl.exe: {error}"))) {
                break status;
            }
            // No `.tmp` yet, before curl's first byte, reads as 0.
            let size = fs::metadata(&tmp_path).map_or(0, |metadata| metadata.len());
            if let Some(flow) = board().downloads[worker].as_mut() {
                flow.done = size;
            }
            if too_slow(size, first.elapsed(), judged) {
                if !online(url) {
                    // No line, not a slow tar: judged afresh once the line is back, its bytes kept.
                    println!("{date}: the server does not answer, offline; waiting for the line, the tar not skipped");
                    while !online(url) {
                        std::thread::sleep(RETRY_DELAY);
                    }
                    println!("{date}: the server answers again, the tar judged afresh");
                    first = Instant::now();
                    continue;
                }
                let _ = child.kill();
                let _ = child.wait();
                remove_tmp(&tmp_path);
                board().downloads[worker] = None;
                let bytes_per_second = size / first.elapsed().as_secs().max(1);
                return Fetched::Skipped { bytes: size, bytes_per_second };
            }
            judged |= first.elapsed() >= SKIP_AFTER;
            std::thread::sleep(DOWNLOAD_POLL);
        };
        match attempt(status.code()) {
            Attempt::Whole => break,
            Attempt::Slow => {
                println!("{date}: below {} KiB/s for {SLOW_SECONDS} s, reconnecting where it stopped", SLOW_BYTES_PER_SECOND / 1024);
                resume = true;
            }
            Attempt::NoRange => {
                println!("{date}: the server refused to resume, starting the tar over");
                let _ = fs::remove_file(&tmp_path);
                resume = false;
            }
            Attempt::Failed => {
                if online(url) {
                    failures += 1;
                    if failures > RETRIES {
                        fail(format!("failed to download {url}"));
                    }
                    std::thread::sleep(RETRY_DELAY);
                } else {
                    // No line: no failure of the tar's, and its `SKIP_AFTER` starts again once the
                    // line is back.
                    println!("{date}: the server does not answer, offline; waiting for the line");
                    while !online(url) {
                        std::thread::sleep(RETRY_DELAY);
                    }
                    println!("{date}: the server answers again, resuming");
                    first = Instant::now();
                }
                resume = true;
            }
        }
    }
    // curl's success alone is not trusted: a resume that appended wrongly would leave a file other
    // than the tar's length, which the conversion would read past its end or short of it. A mismatch
    // stops the run, since it means the resume or the server's file is wrong.
    let size = fs::metadata(&tmp_path).map_or(0, |metadata| metadata.len());
    if size != tar_bytes {
        let _ = fs::remove_file(&tmp_path);
        fail(format!("{url} downloaded as {size} bytes, but its Content-Length is {tar_bytes}: the partial file is deleted"));
    }
    board().downloads[worker] = None;
    fs::rename(&tmp_path, path).unwrap_or_else(|error| fail(format!("failed to rename {tmp_path} to {path}: {error}")));
    Fetched::Downloaded
}

/// Deletes a skipped tar's `.tmp`, trying again a moment while its killed curl still holds it:
/// Windows deletes no open file.
fn remove_tmp(tmp_path: &str) {
    for _ in 0..30 {
        if fs::remove_file(tmp_path).is_ok() || !Path::new(tmp_path).exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("failed to delete {tmp_path}: the next run deletes it");
}

/// Converts and uploads one tar, already downloaded to staging, and returns its row; `sealed` is
/// told the tar's PGN bytes so far at each segment sealed, for the cap's place on the board.
fn convert_tar(
    config: &Lc0Config, table: &[moves::IndexedMove], stem: &str, dir: &'static str, tar_bytes: u64,
    invariance_printed: &mut bool, sealed: impl Fn(u64),
) -> TarRow {
    let tar_path = format!("{}/{stem}.tar", staging_dir());
    // The day directory of the tar's date, which follows the prefix in its name: made locally, and
    // kept on the remote.
    let day_dir = segments::day_dir(&format!("{LC0_ROOT}/{dir}"), &stem[config.prefix.len()..]);
    let (local_dir, remote_dir) = (day_dir.clone(), remote::remote_path(&day_dir));
    fs::create_dir_all(&local_dir).unwrap_or_else(|error| fail(format!("failed to create {local_dir}: {error}")));

    let file = fs::File::open(&tar_path).unwrap_or_else(|error| fail(format!("failed to open {tar_path}: {error}")));
    board().conversion = Some(Flow::new(&stem[config.prefix.len()..], 0, tar_bytes));
    let mut archive = tar::TarReader::new(BufReader::new(Counted { inner: file }));
    let mut row =
        TarRow { tar: stem.to_string(), dir: dir.to_string(), tar_bytes, pgn_bytes: 0, games: 0, skipped: 0, positions: 0, segments: 0 };
    let mut segment = Segment::default();
    while let Some((name, gzipped)) = archive.next_file().unwrap_or_else(|message| fail(format!("{stem}: {message}"))) {
        let Some(game_id) = game_id(&name) else { continue };
        let mut game = Vec::new();
        flate2::read::GzDecoder::new(&gzipped[..])
            .read_to_end(&mut game)
            .unwrap_or_else(|error| fail(format!("{stem}: {name}: failed to gunzip: {error}")));
        let converted = convert::convert_game(game_id, &game, table, true).unwrap_or_else(|message| fail(format!("{stem}: {name}: {message}")));
        let Some(converted) = converted else {
            row.skipped += 1;
            continue;
        };
        if !*invariance_printed {
            println!("invariance_info of the first game ({name}): {}", converted.invariance_info);
            *invariance_printed = true;
        }
        segment.text.push_str(&converted.text);
        segment.games += 1;
        segment.positions += converted.positions;
        if segment.positions >= config.segment {
            seal_and_upload(&local_dir, &remote_dir, &format!("{stem}-{:02}", row.segments), &mut segment, &mut row);
            sealed(row.pgn_bytes);
        }
    }
    if segment.games > 0 {
        seal_and_upload(&local_dir, &remote_dir, &format!("{stem}-{:02}", row.segments), &mut segment, &mut row);
        sealed(row.pgn_bytes);
    }
    board().conversion = None;
    fs::remove_file(&tar_path).unwrap_or_else(|error| fail(format!("failed to remove {tar_path}: {error}")));
    row
}

/// `training.<game-id>.gz`'s game id; `None` for any other member.
fn game_id(member: &str) -> Option<&str> {
    let file_name = member.rsplit('/').next()?;
    file_name.strip_prefix("training.")?.strip_suffix(".gz")
}

/// The games of the segment being filled.
#[derive(Default)]
struct Segment {
    text: String,
    games: u64,
    positions: u64,
}

/// Compresses `segment` into `{local_dir}/{name}.pgn.zst` beside its `.meta`, uploads the two to
/// `remote_dir`, `.meta` last as on every segment, removes them locally, adds them to `row`, and
/// empties `segment`.
fn seal_and_upload(local_dir: &str, remote_dir: &str, name: &str, segment: &mut Segment, row: &mut TarRow) {
    let segment_path = format!("{local_dir}/{name}.pgn.zst");
    let tmp_path = format!("{segment_path}.tmp");
    let output = fs::File::create(&tmp_path).unwrap_or_else(|error| fail(format!("failed to create {tmp_path}: {error}")));
    let mut encoder = zstd::Encoder::new(BufWriter::new(output), COMPRESSION_LEVEL)
        .and_then(|mut encoder| {
            encoder.include_checksum(true)?;
            encoder.multithread(std::thread::available_parallelism().map_or(1, |count| count.get()) as u32)?;
            Ok(encoder)
        })
        .unwrap_or_else(|error| fail(format!("failed to open a zstd encoder for {tmp_path}: {error}")));
    encoder.write_all(segment.text.as_bytes()).unwrap_or_else(|error| fail(format!("failed to write {tmp_path}: {error}")));
    let writer = encoder.finish().unwrap_or_else(|error| fail(format!("failed to finish {tmp_path}: {error}")));
    let output = writer.into_inner().unwrap_or_else(|error| fail(format!("failed to flush {tmp_path}: {error}")));
    output.sync_all().unwrap_or_else(|error| fail(format!("failed to sync {tmp_path}: {error}")));
    drop(output);
    fs::rename(&tmp_path, &segment_path).unwrap_or_else(|error| fail(format!("failed to rename {tmp_path}: {error}")));
    let meta_path = format!("{segment_path}{}", segments::META_SUFFIX);
    let meta = segments::SegmentMeta { positions: segment.positions, games: segment.games };
    fs::write(&meta_path, meta.text()).unwrap_or_else(|error| fail(format!("failed to write {meta_path}: {error}")));

    let pgn_bytes = fs::metadata(&segment_path).map(|metadata| metadata.len()).unwrap_or_else(|error| fail(format!("{segment_path}: {error}")));
    for path in [&segment_path, &meta_path] {
        let file_name = Path::new(path).file_name().and_then(|name| name.to_str()).expect("a file name").to_string();
        remote::copy_files(local_dir, remote_dir, &|candidate: &str| candidate == file_name)
            .unwrap_or_else(|error| fail(format!("failed to upload {path}: {error}")));
    }
    for path in [&segment_path, &meta_path] {
        fs::remove_file(path).unwrap_or_else(|error| fail(format!("failed to remove {path}: {error}")));
    }
    row.pgn_bytes += pgn_bytes;
    row.games += segment.games;
    row.positions += segment.positions;
    row.segments += 1;
    *segment = Segment::default();
}

/// `tars.tsv` from the remote; empty before the first tar.
fn read_rows() -> Vec<TarRow> {
    if !Path::new(&remote::remote_path(TARS_PATH)).exists() {
        return Vec::new();
    }
    let text = remote::cat(TARS_PATH).unwrap_or_else(|error| fail(error.to_string()));
    parse_rows(&text).unwrap_or_else(|message| fail(format!("{TARS_PATH}: {message}")))
}

fn parse_rows(text: &str) -> Result<Vec<TarRow>, String> {
    let mut lines = text.lines();
    if lines.next() != Some(TARS_HEADER) {
        return Err(format!("the first line is not {TARS_HEADER:?}"));
    }
    let mut rows = Vec::new();
    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        let [tar, dir, numbers @ ..] = fields.as_slice() else { return Err(format!("malformed row {line:?}")) };
        let numbers: Vec<u64> = numbers.iter().map(|field| field.parse::<u64>()).collect::<Result<_, _>>().map_err(|_| format!("malformed row {line:?}"))?;
        let [tar_bytes, pgn_bytes, games, skipped, positions, segments] = numbers.as_slice() else {
            return Err(format!("malformed row {line:?}"));
        };
        rows.push(TarRow {
            tar: tar.to_string(),
            dir: dir.to_string(),
            tar_bytes: *tar_bytes,
            pgn_bytes: *pgn_bytes,
            games: *games,
            skipped: *skipped,
            positions: *positions,
            segments: *segments,
        });
    }
    Ok(rows)
}

fn format_rows(rows: &[TarRow]) -> String {
    let mut text = format!("{TARS_HEADER}\n");
    for row in rows {
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            row.tar, row.dir, row.tar_bytes, row.pgn_bytes, row.games, row.skipped, row.positions, row.segments
        ));
    }
    text
}

/// Adds `row` and writes the whole table to the remote: the tar is done.
fn append_row(rows: &mut Vec<TarRow>, row: TarRow) {
    rows.push(row);
    remote::write_small(TARS_PATH, &format_rows(rows)).unwrap_or_else(|error| fail(format!("failed to write {TARS_PATH}: {error}")));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(dir: &str, tar_bytes: u64, pgn_bytes: u64, games: u64) -> TarRow {
        TarRow { tar: "t".to_string(), dir: dir.to_string(), tar_bytes, pgn_bytes, games, skipped: 3, positions: 0, segments: 1 }
    }

    #[test]
    fn the_summary_counts_staging_only_beside_a_running_convert() {
        let rows = [row("test", 10, 5, 2), row("train", 10, 5, 3), row("none", 10, 0, 0)];
        assert_eq!(
            summary_line(&rows, Some(4), false),
            format!("the Convert's {LC0_ROOT}, with visit counts, not complete: 7 tars downloaded, 2 converted")
        );
        assert_eq!(summary_line(&rows, None, true), format!("the Convert's {LC0_ROOT}, with visit counts, complete: 2 tars converted"));
    }

    #[test]
    fn a_tar_is_skipped_only_at_its_first_two_minutes_and_only_when_slow_over_them() {
        let threshold = SLOW_BYTES_PER_SECOND * SKIP_AFTER.as_secs();
        assert!(!too_slow(0, SKIP_AFTER - Duration::from_secs(1), false), "judged at two minutes, not before");
        assert!(too_slow(threshold - 1, SKIP_AFTER, false));
        assert!(!too_slow(threshold, SKIP_AFTER, false), "at the threshold, kept");
        assert!(!too_slow(0, SKIP_AFTER * 3, true), "past its first two minutes, a dip is only resumed");
    }

    #[test]
    fn a_skip_is_recorded_as_one_row_under_its_header() {
        let row = skipped_row("training-run2-test91-20260104-1817", 1_800_000_000, 1_000_000, 8_716);
        assert_eq!(row, "training-run2-test91-20260104-1817\t1800000000\t1000000\t8716\n");
        assert_eq!(row.trim_end().split('\t').count(), SKIPPED_HEADER.split('\t').count());
    }

    /// `online` reads the listing the tar sits in, the configured `url`.
    #[test]
    fn a_tar_s_listing_is_its_url_up_to_the_last_slash() {
        let url = "https://storage.lczero.org/files/training_data/test91/";
        assert_eq!(listing_url(&format!("{url}training-run2-test91-20260101-0017.tar")), url);
    }

    /// Skipped in any run, a tar is passed over, once in the list however many its rows, in date
    /// order. Blank lines and the header count for nothing.
    #[test]
    fn a_skipped_tar_is_given_up() {
        let text = format!(
            "{SKIPPED_HEADER}\n{}{}{}{}\n",
            skipped_row("training-run2-test91-20260105-0017", 1, 2, 3),
            skipped_row("training-run2-test91-20260104-1817", 1, 2, 3),
            skipped_row("training-run2-test91-20260105-0017", 4, 5, 6),
            skipped_row("training-run2-test91-20260110-0817", 4, 5, 6),
        );
        assert_eq!(
            given_up(&text),
            ["training-run2-test91-20260104-1817", "training-run2-test91-20260105-0017", "training-run2-test91-20260110-0817"]
        );
        assert!(given_up(&format!("{SKIPPED_HEADER}\n")).is_empty());
    }

    #[test]
    fn a_skipped_tar_counts_nothing_ahead_of_the_converter() {
        let mut shared = Shared {
            rows: vec![row("train", 800_000_000, 400_000_000, 10)],
            slots: vec![Slot::Approved(800_000_000), Slot::Approved(EMPTY_TAR_BYTES)],
            next_claim: 2,
            next_approve: 2,
            converting: 0,
            staged_bytes: 0,
            stopped: false,
        };
        assert!(shared.ahead() > 0, "an approved tar's output is predicted");
        shared.slots[0] = Slot::Skipped(800_000_000);
        assert_eq!(shared.ahead(), 0, "a skipped one's is not");
    }

    #[test]
    fn a_resumed_attempt_alone_continues_the_tmp() {
        let first = curl_args("https://x/t.tar", "t.tar.tmp", false);
        let resumed = curl_args("https://x/t.tar", "t.tar.tmp", true);
        assert!(!first.iter().any(|arg| arg == "-C"));
        assert_eq!(resumed.windows(2).filter(|pair| pair[0] == "-C" && pair[1] == "-").count(), 1);
        assert_eq!(resumed.last().map(String::as_str), Some("https://x/t.tar"), "the URL stays last");
        assert_eq!(first.len() + 2, resumed.len());
    }

    #[test]
    fn curl_s_exit_codes_end_an_attempt() {
        assert_eq!(attempt(Some(0)), Attempt::Whole);
        assert_eq!(attempt(Some(28)), Attempt::Slow);
        assert_eq!(attempt(Some(33)), Attempt::NoRange);
        assert_eq!(attempt(Some(56)), Attempt::Failed);
        assert_eq!(attempt(None), Attempt::Failed);
    }

    #[test]
    fn the_first_tar_always_converts_into_the_test_directory() {
        assert_eq!(decide(&[], 900_000_000, 1, 65_536), Decision::Convert { dir: "test" });
    }

    #[test]
    fn a_tar_predicted_past_the_cap_stops_the_run_before_its_download() {
        // A ratio of 0.1 and 0.2: the larger predicts.
        let rows = [row("test", 1000, 100, 65_536), row("train", 1000, 200, 10)];
        assert_eq!(decide(&rows, 1000, 500, 65_536), Decision::Convert { dir: "train" });
        assert_eq!(decide(&rows, 1001, 500, 65_536), Decision::Stop { used: 300, predicted: 201 });
    }

    #[test]
    fn a_corpus_always_gets_a_training_tar_whatever_the_cap() {
        // The test tar alone fills the cap, its test set short or not: the next tar still trains.
        for test_games in [10, 65_536] {
            let rows = [row("test", 1000, 300, test_games)];
            assert_eq!(decide(&rows, 1000, 300, 65_536), Decision::Convert { dir: "train" });
            assert!(may_fetch(&rows, 0, 0, 1000, 300, 3000), "fetched for it");
            assert!(!may_fetch(&rows, 1, 0, 1000, 300, 3000), "but not a second one, behind it");
        }
        let rows = [row("test", 1000, 300, 65_536), row("train", 1000, 300, 10)];
        assert!(matches!(decide(&rows, 1000, 300, 65_536), Decision::Stop { .. }), "once it has one, the cap holds");
    }

    #[test]
    fn a_tar_is_fetched_ahead_only_if_it_fits_beside_the_tars_before_it() {
        // Ratio 0.1, 300 bytes used: a tar of 1000 predicts 100.
        let rows = [row("test", 1000, 100, 10), row("test", 2000, 200, 10)];
        assert!(!passes_cap(&rows, 0, 1000, 400));
        assert!(passes_cap(&rows, 1, 1000, 400), "one byte more ahead in the queue tips it over");
        assert!(!passes_cap(&[], u64::MAX / 2, 1000, 0), "nothing to predict from: never past the cap");
    }

    #[test]
    fn a_tar_is_downloaded_only_within_the_cap_and_staging() {
        // Ratio 0.1, 300 bytes used: a tar of 1000 predicts 100.
        let rows = [row("test", 1000, 100, 10), row("test", 2000, 200, 10)];
        assert!(may_fetch(&rows, 0, 2000, 1000, 400, 3000));
        assert!(!may_fetch(&rows, 1, 2000, 1000, 400, 3000), "past the cap");
        assert!(!may_fetch(&rows, 0, 2001, 1000, 400, 3000), "past staging");
        assert!(may_fetch(&rows, 0, 0, 5000, 400 + 500, 3000), "an empty staging takes even a tar larger than it");
    }

    #[test]
    fn tars_go_to_the_test_directory_until_it_holds_its_games() {
        let rows = [row("test", 1000, 100, 60_000), row("none", 10240, 0, 0)];
        assert_eq!(decide(&rows, 1000, u64::MAX, 65_536), Decision::Convert { dir: "test" });
        let rows = [row("test", 1000, 100, 65_536)];
        assert_eq!(decide(&rows, 1000, u64::MAX, 65_536), Decision::Convert { dir: "train" });
    }

    #[test]
    fn the_table_reads_back_what_it_wrote() {
        let rows = vec![row("test", 411_152_384, 50_000_000, 16_000), row("none", 10240, 0, 0)];
        assert_eq!(parse_rows(&format_rows(&rows)).unwrap(), rows);
        assert!(parse_rows("tar\tdir\n").is_err());
    }

    #[test]
    fn a_listing_names_each_tar_once_in_date_order() {
        let prefix = "training-run2-test91-";
        let listing = format!(
            "<a href=\"{prefix}20260101-0117.tar\">{prefix}20260101-0117.tar</a> <a href=\"{prefix}20260101-0017.tar\">x</a> \
             {prefix}2026-bad.tar LICENSE.txt"
        );
        assert_eq!(tar_stems(&listing, prefix), vec![format!("{prefix}20260101-0017"), format!("{prefix}20260101-0117")]);
    }

    #[test]
    fn a_member_s_game_id_is_its_name_between_training_and_gz() {
        assert_eq!(game_id("run/training.229828019.gz"), Some("229828019"));
        assert_eq!(game_id("run/"), None);
        assert_eq!(game_id("run/LICENSE"), None);
    }

    /// A diagnostic, run by hand (2026-11-02): the first freeze of `data/lc0` found its test segments
    /// holding one position a game more than their `.meta` files say. Reads the first test segment on
    /// the remote twice, as raw text and through the freeze's reader, and prints its `.meta`'s counts,
    /// both reads' totals, and the first games where a game's comments + 1 (what the Convert counts)
    /// differ from its positions as the reader counts them.
    #[test]
    #[ignore]
    fn lc0_segment_counts() {
        use std::io::Read as _;

        use crate::game_labels::SelfPlaySegmentReader;
        use crate::pgn_train_stream::test_files;

        let files = test_files(LC0_ROOT).unwrap();
        let file = &files[0];
        let meta = segments::SegmentMeta::read(&format!("{}{}", file.path, segments::META_SUFFIX)).unwrap();
        println!("{}: .meta {} games, {} positions", file.path, meta.games, meta.positions);

        let mut text = String::new();
        zstd::Decoder::new(fs::File::open(&file.path).unwrap()).unwrap().read_to_string(&mut text).unwrap();
        let raw: Vec<&str> = text.split("[Event ").skip(1).collect();
        let mut reader = SelfPlaySegmentReader::open(&file.path);
        let mut read = Vec::new();
        while let Some((_, collected)) = reader.next_game(&file.path) {
            read.push(collected.position_count() as u64);
        }
        let comments: Vec<u64> = raw.iter().map(|game| game.matches('{').count() as u64 + 1).collect();
        println!("raw text: {} games, {} positions as comments + 1", raw.len(), comments.iter().sum::<u64>());
        println!("reader:   {} games, {} positions", read.len(), read.iter().sum::<u64>());

        let mut shown = 0;
        for (index, (game, (&counted, &reader_count))) in raw.iter().zip(comments.iter().zip(&read)).enumerate() {
            if counted != reader_count && shown < 3 {
                shown += 1;
                let head: String = game.chars().take(600).collect();
                println!("\ngame {index}: comments + 1 = {counted}, reader = {reader_count}\n[Event {head}");
            }
        }
        let differing = comments.iter().zip(&read).filter(|(a, b)| a != b).count();
        println!("\n{differing} of {} games differ", read.len().min(raw.len()));
    }
}
