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

use super::buckets;

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

    /// lambda*, unclamped; `None` with fewer than two pairs, or when s or the blend does not vary.
    fn lambda(&self) -> Option<f64> {
        if self.n < 2 {
            return None;
        }
        let n = self.n as f64;
        let (mean_s, mean_z) = (self.s / n, self.z / n);
        let var_s = self.ss / n - mean_s * mean_s;
        let var_z = self.zz / n - mean_z * mean_z;
        let cov = self.sz / n - mean_s * mean_z;
        let total = var_s + var_z - 2.0 * cov;
        (var_s > 0.0 && total > 0.0).then(|| (var_s - cov) / total)
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
                    buckets::for_each_pair(dir, index, |s, z, position| {
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
