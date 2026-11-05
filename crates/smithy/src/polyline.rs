//! Lambda by progress: the polyline a `lambda: polyline` run labels its one arm with, measured once
//! from a corpus's own games.
//!
//! Each searched position gives a pair from its side to move: `s`, the teacher's score read through
//! the teacher's curve (its label at lambda 0), and `z`, the game's result (its label at lambda 1).
//! With `s = v + e_s` and `z = v + e_z`, the errors independent of each other and of the true value
//! v, the blend `(1 - lambda) s + lambda z` closest to v is at the inverse-variance weight; since
//! `Cov(s, z) = Var(v)`,
//!
//! ```text
//! lambda* = (Var(s) - Cov(s, z)) / (Var(s) + Var(z) - 2 Cov(s, z))
//! ```
//!
//! valid where `s` is not squeezed toward 0 (the slope of z on s, `Cov / Var(s)`, at most 1).
//!
//! **By progress:** the i-th of a game's n searched positions, counted from the book exit, has
//! progress `p = i / n`, in ten half-open buckets, bucket `(10 i) / n` in integers so a boundary goes
//! to the upper bucket with no rounding. Each bucket's lambda* is clamped to [0, 1]: below 0 the
//! teacher's s is squeezed, which no blend corrects, so s alone; above 1, z alone. No monotone rule:
//! a weak, over-confident teacher (PeSTO) rises toward the result, an accurate one cautious late
//! (Lc0) falls to 0. A position's lambda is the polyline through the ten, linear between bucket
//! centres (0.05, 0.15, ..., 0.95), flat beyond the end centres: never outside [0, 1], continuous.
//!
//! **Measured from the scattered buckets,** which hold s, z and each position's progress: every
//! position of the training set, read locally, the corpus read once, by the scatter, and no sample.
//!
//! **Left out of the measurement:** a position whose s is a certainty, 1 or -1 and equal to z: a mate
//! score or a win or loss of the tables, not an estimate. The tables' draws stay: a record holds
//! `s = z = 0` for them, the same as an ordinary 0.00 score in a drawn game, so it cannot tell them
//! apart. Each game's final position, which has no score, is not in the buckets. Game length depends
//! on the result, so buckets by progress are partly selected by z: the ten values are a shape, each
//! a lower bound (the search and the game share errors), judged in play, not by the formula.

use std::fs;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use temper_chess::labels::{Curve, LOGISTIC};
use temper_chess::pipeline::segments::SEGMENT_SUFFIX;

use super::buckets;
use super::cache::Progress;
use super::game_labels::{self_play_game_labels, CollectedSelfPlayGame, SelfPlaySegmentReader};
use super::lc0;

/// The buckets of progress.
pub(crate) const BUCKETS: usize = 10;

/// The bucket of the i-th of n searched positions: `(10 i) / n`, in integers.
pub(crate) fn bucket(i: usize, n: usize) -> usize {
    assert!(i < n, "position {i} of {n}");
    BUCKETS * i / n
}

/// The running sums of the (s, z) pairs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Moments {
    n: u64,
    s: f64,
    z: f64,
    ss: f64,
    zz: f64,
    sz: f64,
}

impl Moments {
    fn add(&mut self, s: f64, z: f64) {
        self.n += 1;
        self.s += s;
        self.z += z;
        self.ss += s * s;
        self.zz += z * z;
        self.sz += s * z;
    }

    fn merge(&mut self, other: &Moments) {
        self.n += other.n;
        self.s += other.s;
        self.z += other.z;
        self.ss += other.ss;
        self.zz += other.zz;
        self.sz += other.sz;
    }

    /// Var(s), Var(z) and Cov(s, z); `None` with fewer than two pairs.
    fn spreads(&self) -> Option<(f64, f64, f64)> {
        if self.n < 2 {
            return None;
        }
        let n = self.n as f64;
        let (mean_s, mean_z) = (self.s / n, self.z / n);
        Some((self.ss / n - mean_s * mean_s, self.zz / n - mean_z * mean_z, self.sz / n - mean_s * mean_z))
    }

    /// lambda*, unclamped; `None` with fewer than two pairs, or when s or the blend does not vary.
    fn lambda(&self) -> Option<f64> {
        let (var_s, var_z, cov) = self.spreads()?;
        let total = var_s + var_z - 2.0 * cov;
        (var_s > 0.0 && total > 0.0).then(|| (var_s - cov) / total)
    }

    /// The slope of z on s, `Cov / Var(s)`: above 1, s is squeezed. `None` as `lambda`.
    fn slope(&self) -> Option<f64> {
        let (var_s, _, cov) = self.spreads()?;
        (var_s > 0.0).then(|| cov / var_s)
    }
}

/// The ten clamped lambdas, one per bucket of progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Polyline {
    values: [f64; BUCKETS],
}

impl Polyline {
    /// From ten measured lambdas, each clamped to [0, 1].
    pub(crate) fn new(values: [f64; BUCKETS]) -> Polyline {
        Polyline { values: values.map(|value| value.clamp(0.0, 1.0)) }
    }

    /// The lambda of the i-th of n searched positions: linear between bucket centres, flat beyond
    /// the end centres.
    pub(crate) fn at(&self, i: usize, n: usize) -> f64 {
        assert!(i < n, "position {i} of {n}");
        let p = i as f64 / n as f64;
        let first = 0.5 / BUCKETS as f64;
        let last = (BUCKETS as f64 - 0.5) / BUCKETS as f64;
        if p <= first {
            return self.values[0];
        }
        if p >= last {
            return self.values[BUCKETS - 1];
        }
        let x = (p - first) * BUCKETS as f64;
        let k = (x.floor() as usize).min(BUCKETS - 2);
        let t = x - k as f64;
        self.values[k] + (self.values[k + 1] - self.values[k]) * t
    }

    /// The ten values as `polyline.txt` holds them, one `bucket<TAB>lambda` line each: compared as
    /// text by a frozen polyline label set, so a polyline read back equals the one measured.
    pub(crate) fn values_text(&self) -> String {
        self.values.iter().enumerate().map(|(bucket, value)| format!("{bucket}\t{value:.6}\n")).collect()
    }

    /// As `polyline.txt` holds it: the ten values, then the positions measured.
    fn text(&self, positions: u64) -> String {
        let mut text = self.values_text();
        text.push_str(&format!("positions\t{positions}\n"));
        text
    }

    fn parse(text: &str) -> Option<(Polyline, u64)> {
        let mut values = [0.0; BUCKETS];
        let mut lines = text.lines();
        for (bucket, value) in values.iter_mut().enumerate() {
            let (key, number) = lines.next()?.split_once('\t')?;
            if key != bucket.to_string() {
                return None;
            }
            *value = number.parse().ok()?;
        }
        let (key, positions) = lines.next()?.split_once('\t')?;
        (key == "positions").then_some(())?;
        Some((Polyline::new(values), positions.parse().ok()?))
    }
}

/// Whether s is a certainty rather than an estimate (the module's text).
fn is_certainty(s: f32, z: f32) -> bool {
    s.abs() == 1.0 && s == z
}

/// The polyline of the buckets `dir`, read by `threads` a bucket at a time, and the positions it was
/// measured on, `bar` counting every record read; `None` when stopped, checked between buckets. A
/// bucket of progress with too few pairs to measure takes lambda 0, s alone.
pub(crate) fn measure(dir: &str, threads: usize, stop: &AtomicBool, bar: &progress::Bar) -> Option<(Polyline, u64)> {
    let next = AtomicUsize::new(0);
    let total = Mutex::new([Moments::default(); BUCKETS]);
    std::thread::scope(|scope| {
        for _ in 0..threads.clamp(1, buckets::BUCKETS) {
            scope.spawn(|| {
                let mut sums = [Moments::default(); BUCKETS];
                while !stop.load(Ordering::SeqCst) {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    if index >= buckets::BUCKETS {
                        break;
                    }
                    let mut read = 0;
                    buckets::for_each_pair(dir, index, |s, z, position, _| {
                        read += 1;
                        if !is_certainty(s, z) {
                            sums[bucket(usize::from(position.i), usize::from(position.n))].add(f64::from(s), f64::from(z));
                        }
                    });
                    bar.inc(read);
                }
                let mut total = total.lock().expect("the sums' lock");
                for (mine, theirs) in total.iter_mut().zip(&sums) {
                    mine.merge(theirs);
                }
            });
        }
    });
    if stop.load(Ordering::SeqCst) {
        return None;
    }
    let sums = total.into_inner().expect("the sums' lock");
    let positions = sums.iter().map(|moments| moments.n).sum();
    Some((Polyline::new(sums.map(|moments| moments.lambda().unwrap_or(0.0))), positions))
}

/// The polyline kept at `path` (`polyline.txt`), or measured on the buckets `dir` and kept there
/// first: once per corpus, a source's beside it; a self-play generation's and a scaled run's in the
/// generation's directory, a scaled run's buckets holding its share alone. `None` when stopped.
pub(crate) fn ensure(path: &str, dir: &str, threads: usize, stop: &AtomicBool, say: &dyn Fn(String)) -> Result<Option<Polyline>, String> {
    if let Ok(text) = fs::read_to_string(path) {
        let (polyline, positions) = Polyline::parse(&text).ok_or_else(|| format!("{path}: not a polyline; delete it to measure again"))?;
        say(format!("lambda by progress from {path}, measured on {} positions: {}", progress::grouped(positions), show(&polyline)));
        return Ok(Some(polyline));
    }
    let records = buckets::committed_records(dir)?;
    say(format!("measuring lambda by progress on the {} positions of {dir}", progress::grouped(records)));
    let bar = progress::resumed(0, records, progress::Unit::Count("positions"), "measuring lambda by progress");
    let measured = measure(dir, threads, stop, &bar);
    bar.finish();
    let Some((polyline, positions)) = measured else {
        return Ok(None);
    };
    if positions == 0 {
        return Err(format!("no position to measure lambda by progress on for {path}"));
    }
    // A sourced run's generation 1 may not have its directory yet: nothing else wrote there first.
    if let Some(parent) = std::path::Path::new(path).parent() {
        fs::create_dir_all(parent).map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    let tmp = format!("{path}.partial");
    fs::write(&tmp, polyline.text(positions)).map_err(|error| format!("failed to write {tmp}: {error}"))?;
    fs::rename(&tmp, path).map_err(|error| format!("failed to rename {tmp}: {error}"))?;
    say(format!("lambda by progress, measured on {} positions: {}", progress::grouped(positions), show(&polyline)));
    Ok(Some(polyline))
}

/// The polyline kept at `path`, if there and readable: for the page, which never measures.
pub(crate) fn read(path: &str) -> Option<Polyline> {
    Polyline::parse(&fs::read_to_string(path).ok()?).map(|(polyline, _)| polyline)
}

/// The hand-run, read-only look at what the clamp hides: `smithy polyline-diagnose <buckets> [one-in]
/// [progress | pieces | plies | steps | whole]`, or from a corpus not scattered, `<games> <one-in> games`. Not
/// in the usage line.
pub(crate) const DIAGNOSE_COMMAND: &str = "polyline-diagnose";

/// The bins of s in a diagnosis, over [-1, 1].
const S_BINS: usize = 20;

/// The bin of `s`, the top one closed at 1.
fn s_bin(s: f32) -> usize {
    (((f64::from(s) + 1.0) * S_BINS as f64 / 2.0).floor() as usize).min(S_BINS - 1)
}

/// How a diagnosis groups its positions.
#[derive(Clone, Copy, PartialEq)]
enum GroupBy {
    /// Progress through the game, as the polyline does. It knows the game's final length, which
    /// the position does not, and that length depends on the result: a position late by progress
    /// is one whose game was about to end. Bins here can show what hindsight alone explains.
    Progress,
    /// The pieces on the board, which the position itself knows, so nothing of the future.
    Pieces,
}

/// The group of pieces besides the kings, in threes from a full board down, the last taking 0 to 3.
fn pieces_group(pieces: usize) -> usize {
    (30usize.saturating_sub(pieces) / 3).min(BUCKETS - 1)
}

impl GroupBy {
    fn name(self, group: usize) -> String {
        match self {
            GroupBy::Progress => format!("progress bucket {group}"),
            GroupBy::Pieces if group == BUCKETS - 1 => "0 to 3 pieces besides the kings".to_string(),
            GroupBy::Pieces => format!("{} to {} pieces besides the kings", 28 - 3 * group, 30 - 3 * group),
        }
    }
}

/// One group of a diagnosis: the sums of `measure`, the same without `s = z = 0`, mostly the tables'
/// draws, and mean z by bin of s. On the diagonal, mean z equals mean s. The bins are believed over
/// the slope, which a curve across the diagonal hides and the caps at 1 and -1 distort.
#[derive(Clone, Copy, Default)]
struct Diagnosis {
    all: Moments,
    without_draws: Moments,
    bins: [Moments; S_BINS],
}

impl Diagnosis {
    fn add(&mut self, s: f32, z: f32) {
        self.all.add(f64::from(s), f64::from(z));
        if s != 0.0 || z != 0.0 {
            self.without_draws.add(f64::from(s), f64::from(z));
        }
        self.bins[s_bin(s)].add(f64::from(s), f64::from(z));
    }

    fn merge(&mut self, other: &Diagnosis) {
        self.all.merge(&other.all);
        self.without_draws.merge(&other.without_draws);
        self.bins.iter_mut().zip(&other.bins).for_each(|(mine, theirs)| mine.merge(theirs));
    }

    /// One line of sums, then one per bin that holds positions.
    fn text(&self, name: &str) -> String {
        let number = |value: Option<f64>| value.map_or("-".to_string(), |value| format!("{value:.4}"));
        let line = |moments: &Moments| {
            let spreads = moments.spreads();
            format!(
                "{} positions, mean s {:.4}, mean z {:.4}, Var(s) {}, Var(z) {}, Cov {}, lambda* {}, slope {}",
                progress::grouped(moments.n),
                moments.s / moments.n.max(1) as f64,
                moments.z / moments.n.max(1) as f64,
                number(spreads.map(|spreads| spreads.0)),
                number(spreads.map(|spreads| spreads.1)),
                number(spreads.map(|spreads| spreads.2)),
                number(moments.lambda()),
                number(moments.slope()),
            )
        };
        let mut text = format!("{name}: {}\n  without s = z = 0: {}\n", line(&self.all), line(&self.without_draws));
        for bin in self.bins.iter().filter(|bin| bin.n > 0) {
            let (mean_s, mean_z) = (bin.s / bin.n as f64, bin.z / bin.n as f64);
            text.push_str(&format!("  mean s {mean_s:>7.4}  mean z {mean_z:>7.4}  {:>15}\n", progress::grouped(bin.n)));
        }
        text
    }
}

/// The diagnosis of one in `one_in` of the buckets `dir`, their positions already shuffled across
/// the files, a fair sample. What `measure` leaves out, it leaves out.
fn diagnose(dir: &str, one_in: usize, threads: usize, by: GroupBy) -> [Diagnosis; BUCKETS] {
    scan(
        dir,
        one_in,
        threads,
        [Diagnosis::default(); BUCKETS],
        |sums, s, z, position, pieces| {
            let group = match by {
                GroupBy::Progress => bucket(usize::from(position.i), usize::from(position.n)),
                GroupBy::Pieces => pieces_group(pieces),
            };
            sums[group].add(s, z);
        },
        |total, sums| total.iter_mut().zip(sums).for_each(|(mine, theirs)| mine.merge(theirs)),
    )
}

/// Sums over one in `one_in` of the buckets `dir`, each thread its own, every pair `measure` would
/// count visited.
fn scan<T: Copy + Send + Sync>(
    dir: &str,
    one_in: usize,
    threads: usize,
    empty: T,
    visit: impl Fn(&mut T, f32, f32, Progress, usize) + Sync,
    merge: impl Fn(&mut T, &T) + Sync,
) -> T {
    let files: Vec<usize> = (0..buckets::BUCKETS).step_by(one_in).collect();
    let next = AtomicUsize::new(0);
    let total = Mutex::new(empty);
    std::thread::scope(|scope| {
        for _ in 0..threads.clamp(1, files.len()) {
            scope.spawn(|| {
                let mut sums = empty;
                while let Some(&file) = files.get(next.fetch_add(1, Ordering::SeqCst)) {
                    buckets::for_each_pair(dir, file, |s, z, position, pieces| {
                        if !is_certainty(s, z) {
                            visit(&mut sums, s, z, position, pieces);
                        }
                    });
                }
                merge(&mut total.lock().expect("the sums' lock"), &sums);
            });
        }
    });
    total.into_inner().expect("the sums' lock")
}

/// The most plies left a table of `PliesLeft` tells apart; every position further from the end shares
/// its last line.
const MOST_PLIES_LEFT: usize = 100;

/// `Var(z - s)` by plies left in the game. At the game's end z has no noise left, so with s's noise
/// constant through a game and each ply left adding its own variance, `Var(z - s at k) = n + k sigma^2`:
/// the intercept is s's noise and the inverse-variance lambda is `n / (n + k sigma^2)`. With and
/// without `s = z = 0`, since the tables' draws, which have no noise at all, gather at the end.
#[derive(Clone, Copy)]
struct PliesLeft {
    all: [Moments; MOST_PLIES_LEFT + 1],
    without_draws: [Moments; MOST_PLIES_LEFT + 1],
}

impl Moments {
    /// `Var(z - s)`; `None` as `spreads`.
    fn gap(&self) -> Option<f64> {
        self.spreads().map(|(var_s, var_z, cov)| var_s + var_z - 2.0 * cov)
    }
}

impl PliesLeft {
    fn add(&mut self, s: f32, z: f32, left: usize) {
        let k = left.min(MOST_PLIES_LEFT);
        self.all[k].add(f64::from(s), f64::from(z));
        if s != 0.0 || z != 0.0 {
            self.without_draws[k].add(f64::from(s), f64::from(z));
        }
    }

    fn merge(&mut self, other: &PliesLeft) {
        self.all.iter_mut().zip(&other.all).for_each(|(mine, theirs)| mine.merge(theirs));
        self.without_draws.iter_mut().zip(&other.without_draws).for_each(|(mine, theirs)| mine.merge(theirs));
    }

    fn text(&self) -> String {
        let number = |value: Option<f64>| value.map_or("-".to_string(), |value| format!("{value:.4}"));
        let mut text = "plies left, Var(z - s), positions; without s = z = 0, the same\n".to_string();
        for k in 1..=MOST_PLIES_LEFT {
            text.push_str(&format!(
                "{k:>4}  {:>7} {:>11}  {:>7} {:>11}\n",
                number(self.all[k].gap()),
                progress::grouped(self.all[k].n),
                number(self.without_draws[k].gap()),
                progress::grouped(self.without_draws[k].n),
            ));
        }
        for last in [20, 40, MOST_PLIES_LEFT - 1] {
            let (all, without) = (fitted(&self.all, last), fitted(&self.without_draws, last));
            let show = |fit: Option<(f64, f64)>| fit.map_or("-".to_string(), |(n, step)| format!("n {n:.4}, sigma^2 {step:.5}"));
            text.push_str(&format!("fitted over 1 to {last} plies left: {}; without s = z = 0: {}\n", show(all), show(without)));
        }
        text
    }
}

/// The line through `Var(z - s)` over plies left 1 to `last`, weighted by positions: its intercept,
/// s's noise, and its slope. `None` when the lines are too few.
fn fitted(moments: &[Moments], last: usize) -> Option<(f64, f64)> {
    let points: Vec<(f64, f64, f64)> =
        (1..=last).filter_map(|k| moments[k].gap().map(|gap| (k as f64, gap, moments[k].n as f64))).collect();
    let weight: f64 = points.iter().map(|point| point.2).sum();
    let mean_k = points.iter().map(|point| point.0 * point.2).sum::<f64>() / weight;
    let mean_gap = points.iter().map(|point| point.1 * point.2).sum::<f64>() / weight;
    let spread: f64 = points.iter().map(|point| point.2 * (point.0 - mean_k).powi(2)).sum();
    if points.len() < 2 || spread <= 0.0 {
        return None;
    }
    let slope = points.iter().map(|point| point.2 * (point.0 - mean_k) * (point.1 - mean_gap)).sum::<f64>() / spread;
    Some((mean_gap - slope * mean_k, slope))
}

/// `Var(s[t] + s[t + 1])` by progress, split by whether the plies left at t are odd or even. s flips
/// sign each ply, being its mover's, so the sum is one step of the score: two searches' errors and
/// one ply of real change, `2 n + sigma^2`, a constant bias cancelling, which the intercept of
/// `PliesLeft` cannot do. A pair with a certainty in it is left out. Consecutive positions are only
/// neighbours in the games, so this reads them, not the shuffled buckets.
#[derive(Clone, Copy, Default)]
struct Steps {
    groups: [[Moments; 2]; BUCKETS],
}

impl Steps {
    /// One game's scores, in order, for their movers.
    fn add_game(&mut self, scores: &[f32]) {
        let n = scores.len();
        for (i, pair) in scores.windows(2).enumerate() {
            if pair[0].abs() < 1.0 && pair[1].abs() < 1.0 {
                self.groups[bucket(i, n)][(n - i) % 2].add(f64::from(pair[0] + pair[1]), 0.0);
            }
        }
    }

    fn merge(&mut self, other: &Steps) {
        for (mine, theirs) in self.groups.iter_mut().flatten().zip(other.groups.iter().flatten()) {
            mine.merge(theirs);
        }
    }

    fn text(&self) -> String {
        let number = |moments: &Moments| moments.spreads().map_or("-".to_string(), |spreads| format!("{:.5}", spreads.0));
        let mut text = "progress bucket: Var(s[t] + s[t + 1]) and pairs, plies left at t even; the same, odd\n".to_string();
        for (group, [even, odd]) in self.groups.iter().enumerate() {
            text.push_str(&format!(
                "{group}  {} {:>11}  {} {:>11}\n",
                number(even),
                progress::grouped(even.n),
                number(odd),
                progress::grouped(odd.n)
            ));
        }
        text
    }
}

/// The steps of one in `one_in` of the files scattered into `dir`, read in the games themselves, and
/// through Google Drive for a source's. Their scores read through `curve`, as the buckets' were.
fn steps(dir: &str, one_in: usize, threads: usize, curve: &Curve) -> Result<Steps, String> {
    let paths: Vec<String> = buckets::scattered_paths(dir)?.into_iter().step_by(one_in).collect();
    Ok(read_games(
        &paths,
        threads,
        Steps::default(),
        |sums, game, game_index| {
            let labels = self_play_game_labels(game, 0.0, game_index, curve);
            sums.add_game(&labels[..game.scores_pawns().len()]);
        },
        Steps::merge,
    ))
}

/// Sums over the games of the segments `paths`, each thread its own.
fn read_games<T: Copy + Send + Sync>(
    paths: &[String],
    threads: usize,
    empty: T,
    visit: impl Fn(&mut T, &CollectedSelfPlayGame, u64) + Sync,
    merge: impl Fn(&mut T, &T) + Sync,
) -> T {
    let next = AtomicUsize::new(0);
    let total = Mutex::new(empty);
    std::thread::scope(|scope| {
        for _ in 0..threads.clamp(1, paths.len().max(1)) {
            scope.spawn(|| {
                let mut sums = empty;
                loop {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    let Some(path) = paths.get(index) else { break };
                    eprintln!("{} of {}: {path}", index + 1, paths.len());
                    let mut reader = SelfPlaySegmentReader::open(path);
                    while let Some((game_index, game)) = reader.next_game(path) {
                        visit(&mut sums, &game, game_index);
                    }
                }
                merge(&mut total.lock().expect("the sums' lock"), &sums);
            });
        }
    });
    total.into_inner().expect("the sums' lock")
}

/// All three tables at once, read from games with no scatter: for a corpus too small or too
/// incomplete to scatter. Labels straight from the games, the tables' rescoring left out.
#[derive(Clone, Copy)]
struct FromGames {
    progress: [Diagnosis; BUCKETS],
    plies: PliesLeft,
    steps: Steps,
}

impl FromGames {
    fn add_game(&mut self, game: &CollectedSelfPlayGame, game_index: u64, curve: &Curve) {
        let scores = self_play_game_labels(game, 0.0, game_index, curve);
        let results = self_play_game_labels(game, 1.0, game_index, curve);
        let n = game.scores_pawns().len();
        for (i, (&s, &z)) in scores.iter().zip(&results).take(n).enumerate() {
            if !is_certainty(s, z) {
                self.progress[bucket(i, n)].add(s, z);
                self.plies.add(s, z, n - i);
            }
        }
        self.steps.add_game(&scores[..n]);
    }

    fn merge(&mut self, other: &FromGames) {
        self.progress.iter_mut().zip(&other.progress).for_each(|(mine, theirs)| mine.merge(theirs));
        self.plies.merge(&other.plies);
        self.steps.merge(&other.steps);
    }

    fn text(&self) -> String {
        let by_progress: String =
            self.progress.iter().enumerate().map(|(group, diagnosis)| diagnosis.text(&GroupBy::Progress.name(group))).collect();
        format!("{by_progress}{}{}", self.plies.text(), self.steps.text())
    }
}

/// Every segment under `dir`, sorted.
fn segment_paths(dir: &str) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    let mut pending = vec![std::path::PathBuf::from(dir)];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|error| format!("{}: {error}", directory.display()))? {
            let path = entry.map_err(|error| format!("{}: {error}", directory.display()))?.path();
            if path.is_dir() {
                pending.push(path);
            } else if let Some(text) = path.to_str().filter(|text| text.ends_with(SEGMENT_SUFFIX)) {
                paths.push(text.replace('\\', "/"));
            }
        }
    }
    paths.sort();
    Ok(paths)
}

/// The tables of one in `one_in` of the segments under `dir`.
fn from_games(dir: &str, one_in: usize, threads: usize, curve: &Curve) -> Result<FromGames, String> {
    let paths: Vec<String> = segment_paths(dir)?.into_iter().step_by(one_in).collect();
    if paths.is_empty() {
        return Err(format!("{dir} holds no segments"));
    }
    let empty = FromGames {
        progress: [Diagnosis::default(); BUCKETS],
        plies: PliesLeft { all: [Moments::default(); MOST_PLIES_LEFT + 1], without_draws: [Moments::default(); MOST_PLIES_LEFT + 1] },
        steps: Steps::default(),
    };
    Ok(read_games(&paths, threads, empty, |sums, game, game_index| sums.add_game(game, game_index, curve), FromGames::merge))
}

/// The table of plies left of one in `one_in` of the buckets `dir`.
fn plies_left(dir: &str, one_in: usize, threads: usize) -> PliesLeft {
    let empty = PliesLeft { all: [Moments::default(); MOST_PLIES_LEFT + 1], without_draws: [Moments::default(); MOST_PLIES_LEFT + 1] };
    scan(
        dir,
        one_in,
        threads,
        empty,
        |sums, s, z, position, _| sums.add(s, z, usize::from(position.n) - usize::from(position.i)),
        PliesLeft::merge,
    )
}

/// `DIAGNOSE_COMMAND`: prints the diagnosis of a directory of buckets, writing nothing.
pub(crate) fn run_diagnose(args: &[String]) {
    fn fail(message: String) -> ! {
        eprintln!("{message}");
        std::process::exit(1);
    }
    let [dir, rest @ ..] = args else {
        fail(format!(
            "usage: smithy {DIAGNOSE_COMMAND} <buckets directory> [one-in, default 100] [progress | pieces | plies | steps | whole]\n       \
             smithy {DIAGNOSE_COMMAND} <directory of games> <one-in> games"
        ))
    };
    let (one_in, by) = match rest {
        [] => (None, "progress"),
        [value] => (Some(value), "progress"),
        [value, by] => (Some(value), by.as_str()),
        _ => fail("too many arguments".to_string()),
    };
    let one_in = one_in.map_or(100, |value| {
        value.parse::<usize>().ok().filter(|&value| value > 0).unwrap_or_else(|| fail(format!("{value:?} is not a positive number")))
    });
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    if by == "games" {
        let curve = if dir.contains("pesto") { Ok(LOGISTIC) } else { lc0::rewrite::corpus_curve() };
        match curve.and_then(|curve| from_games(dir, one_in, threads, &curve)) {
            Ok(tables) => print!("{}", tables.text()),
            Err(error) => fail(error),
        }
        return;
    }
    if let Err(error) = buckets::committed_records(dir) {
        fail(error);
    }
    if by == "steps" {
        // Only Lc0's corpus has a curve of its own; every other teacher's scores are logistic.
        let curve = if dir.ends_with("pesto") { Ok(LOGISTIC) } else { lc0::rewrite::corpus_curve() };
        match curve.and_then(|curve| steps(dir, one_in, threads, &curve)) {
            Ok(steps) => print!("{}", steps.text()),
            Err(error) => fail(error),
        }
        return;
    }
    if by == "whole" {
        let whole = scan(dir, one_in, threads, Diagnosis::default(), |sums, s, z, _, _| sums.add(s, z), Diagnosis::merge);
        print!("{}", whole.text("the whole corpus"));
        return;
    }
    let by = match by {
        "progress" => Some(GroupBy::Progress),
        "pieces" => Some(GroupBy::Pieces),
        "plies" => None,
        other => fail(format!("{other:?} is none of progress, pieces, plies, steps and whole")),
    };
    let Some(by) = by else {
        print!("{}", plies_left(dir, one_in, threads).text());
        return;
    };
    for (group, diagnosis) in diagnose(dir, one_in, threads, by).iter().enumerate() {
        print!("{}", diagnosis.text(&by.name(group)));
    }
}

/// The ten values, for the log.
pub(crate) fn show(polyline: &Polyline) -> String {
    polyline.values.iter().map(|value| format!("{value:.3}")).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent noises give the inverse-variance weight: v over four values, e_s of variance
    /// 0.01, e_z of 0.04, so lambda* = 0.01 / 0.05 = 0.2.
    #[test]
    fn independent_noises_give_the_inverse_variance_weight() {
        let mut moments = Moments::default();
        for v in [-0.6, -0.2, 0.2, 0.6] {
            for a in [-0.1, 0.1] {
                for b in [-0.2, 0.2] {
                    moments.add(v + a, v + b);
                }
            }
        }
        assert!((moments.lambda().unwrap() - 0.2).abs() < 1e-12);
    }

    /// A squeezed s, `s = c v`: lambda* negative though s has an error, the slope 1 / c.
    #[test]
    fn a_squeezed_score_reads_as_a_slope_above_1_and_a_negative_lambda() {
        let mut moments = Moments::default();
        for v in [-0.6, -0.2, 0.2, 0.6] {
            for noise in [-0.3, 0.3] {
                moments.add(0.8 * v, v + noise);
            }
        }
        assert!((moments.slope().unwrap() - 1.25).abs() < 1e-12);
        assert!(moments.lambda().unwrap() < 0.0);
    }

    #[test]
    fn the_bins_of_s_cover_its_range() {
        assert_eq!((s_bin(-1.0), s_bin(0.0), s_bin(1.0)), (0, 10, S_BINS - 1));
    }

    #[test]
    fn a_diagnosis_sets_the_drawn_pairs_apart() {
        let mut diagnosis = Diagnosis::default();
        diagnosis.add(0.0, 0.0);
        diagnosis.add(0.5, 1.0);
        assert_eq!((diagnosis.all.n, diagnosis.without_draws.n), (2, 1));
        assert!(diagnosis.text("group").starts_with("group: 2 positions"), "{}", diagnosis.text("group"));
    }

    #[test]
    fn segments_are_found_at_any_depth_and_sorted() {
        let dir = std::env::temp_dir().join(format!("smithy-polyline-segments-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("20261104")).unwrap();
        std::fs::write(dir.join("20261104").join(format!("b{SEGMENT_SUFFIX}")), "").unwrap();
        std::fs::write(dir.join(format!("a{SEGMENT_SUFFIX}")), "").unwrap();
        std::fs::write(dir.join("20261104").join(format!("b{SEGMENT_SUFFIX}.meta")), "").unwrap();
        let paths = segment_paths(dir.to_str().unwrap()).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(paths.len(), 2, "{paths:?}");
        assert!(paths[0].ends_with(&format!("20261104/b{SEGMENT_SUFFIX}")), "digits sort first: {paths:?}");
    }

    /// Each step's sum falls in the bucket of its first position, split by plies left, a certainty
    /// leaving out both of its pairs.
    #[test]
    fn a_step_sums_two_neighbours_and_skips_a_certainty() {
        let mut steps = Steps::default();
        steps.add_game(&[0.5, -0.25, 1.0, 0.0]);
        assert_eq!(steps.groups[0][0].n, 1, "i = 0, 4 left");
        assert_eq!(steps.groups[0][0].s, 0.25);
        assert_eq!(steps.groups.iter().flatten().map(|moments| moments.n).sum::<u64>(), 1);
    }

    /// `Var(z - s) = 0.01 + 0.002 k`, from pairs whose gap is exactly that spread.
    #[test]
    fn the_line_through_the_gaps_finds_the_noise_and_the_step() {
        let mut moments = [Moments::default(); 11];
        for (k, line) in moments.iter_mut().enumerate().skip(1) {
            let spread = (0.01 + 0.002 * k as f64).sqrt();
            line.add(0.0, spread);
            line.add(0.0, -spread);
        }
        let (noise, step) = fitted(&moments, 10).unwrap();
        assert!((noise - 0.01).abs() < 1e-12 && (step - 0.002).abs() < 1e-12, "{noise} {step}");
    }

    #[test]
    fn plies_left_cap_at_the_last_line_and_set_the_drawn_pairs_apart() {
        let mut table = PliesLeft { all: [Moments::default(); MOST_PLIES_LEFT + 1], without_draws: [Moments::default(); MOST_PLIES_LEFT + 1] };
        table.add(0.0, 0.0, 1);
        table.add(0.5, 1.0, 1);
        table.add(0.5, 1.0, 500);
        assert_eq!((table.all[1].n, table.without_draws[1].n, table.all[MOST_PLIES_LEFT].n), (2, 1, 1));
        assert!(table.text().contains("fitted over 1 to 20"));
    }

    #[test]
    fn the_groups_of_pieces_cover_a_game() {
        assert_eq!((pieces_group(30), pieces_group(28), pieces_group(27), pieces_group(3), pieces_group(0)), (0, 0, 1, 9, 9));
        assert_eq!(GroupBy::Pieces.name(0), "28 to 30 pieces besides the kings");
        assert_eq!(GroupBy::Pieces.name(1), "25 to 27 pieces besides the kings");
        assert_eq!(GroupBy::Pieces.name(8), "4 to 6 pieces besides the kings");
        assert_eq!(GroupBy::Pieces.name(9), "0 to 3 pieces besides the kings");
    }

    #[test]
    fn a_boundary_goes_to_the_upper_bucket_with_no_rounding() {
        assert_eq!(bucket(0, 20), 0);
        assert_eq!(bucket(1, 20), 0);
        assert_eq!(bucket(2, 20), 1, "p = 0.1 exactly");
        assert_eq!(bucket(19, 20), 9);
        assert_eq!(bucket(0, 1), 0);
        assert_eq!(bucket(6, 7), 8);
    }

    #[test]
    fn each_value_is_clamped_to_the_unit_interval() {
        let polyline = Polyline::new([-0.2, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 1.3]);
        assert_eq!(polyline.values[0], 0.0);
        assert_eq!(polyline.values[9], 1.0);
    }

    /// At a bucket centre its value; halfway between two centres their mean; flat beyond the ends.
    #[test]
    fn the_polyline_is_linear_between_centres_and_flat_beyond() {
        let values = [0.111, 0.122, 0.149, 0.187, 0.240, 0.342, 0.493, 0.636, 0.844, 0.972];
        let polyline = Polyline::new(values);
        let near = |got: f64, want: f64| assert!((got - want).abs() < 1e-9, "{got} against {want}");
        near(polyline.at(1, 20), values[0]);
        near(polyline.at(3, 20), values[1]);
        near(polyline.at(19, 20), values[9]);
        near(polyline.at(1, 10), (values[0] + values[1]) / 2.0);
        near(polyline.at(5, 10), (values[4] + values[5]) / 2.0);
        near(polyline.at(0, 100), values[0]);
        near(polyline.at(99, 100), values[9]);
        for n in [1, 7, 40, 333] {
            for i in 0..n {
                let lambda = polyline.at(i, n);
                assert!((0.0..=1.0).contains(&lambda), "{i} of {n}: {lambda}");
            }
        }
    }

    #[test]
    fn the_file_round_trips_its_values_and_positions() {
        let polyline = Polyline::new([0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9]);
        assert_eq!(Polyline::parse(&polyline.text(12345)), Some((polyline, 12345)));
        assert_eq!(Polyline::parse("0\t0.1\n"), None, "truncated");
    }
}
