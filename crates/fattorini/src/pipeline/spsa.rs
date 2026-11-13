//! SPSA as fishtest runs it (`spsa_handler.py`, `spsa_workflow.py`, fetched 2026-10-16), over one
//! kind of parameters (`TuneKind`): the search's ten `SearchParameters`, or the time management's
//! 22 `TimeParameters`. Pure functions, no games and no files, so the method is tested on its own. A
//! pair of games, colours reversed on one opening, plays theta + c_k Delta against theta - c_k
//! Delta; its result moves each parameter along Delta, then clips it to its range. `smithy start
//! --spsa` runs it, the arenas playing the pairs.
//!
//! Chosen with the developer on 2026-10-16, `r_end` revised on 2026-10-20: one `r_end` for all
//! parameters (`R_END`), and each parameter's range and `c_end` from its scale (`SEARCH_SPECS`,
//! `TIME_SPECS`). `a_end = R_END * c_end^2` is scale-free, so one `R_END` serves both kinds. The
//! number of pairs N is each tune's own, its config's `length:`, `default` (`DEFAULT_TUNE_PAIRS`) or
//! a number. A search tune starts from its teacher's tune (`specs_from`), or from `SEARCH_SPECS`'
//! starts, the default parameters, when its player has no tuned teacher (the developer, 2026-11-08,
//! reversing 2026-10-23's "every tune from the defaults": each generation's teacher plays its tune in
//! self-play, and the next tune goes on from it). theta is kept as real numbers; a search player
//! rounds each value to the nearest integer when it plays, a time player plays them as they are.

use crate::search::{SearchParameters, TimeParameters};

/// `length: default`: a tune of 65,536 games, every generation's (the developer, 2026-11-08: each
/// teacher tuned as well as a release, since it plays its tune in self-play), about 15 hours on the
/// arenas of 2026-11-08; 16,384 pairs before, which gained the lc0 network about 130 Elo with its
/// parameters still moving at the end.
pub const DEFAULT_TUNE_PAIRS: u32 = 32_768;

/// The tests' tune length, the N of 2026-10-16's first tunes: about four hours on one machine at 10+0.1
/// with every game played out.
#[cfg(test)]
pub const PAIRS: u32 = 2_500;

/// The step at the end of the schedule, relative to `c_end^2` (`a_end = R_END * c_end^2`), for every
/// tune: Stockfish's own, the `0.0020` its `TUNE` macro writes into every fishtest SPSA line
/// (`tune.cpp`, checked 2026-10-20), adopted by the developer on 2026-10-20 over the 0.005 chosen for
/// 2,500 pairs. A pair's last step is `R_END * c_end * r`, so a tune's reach grows with its pairs:
/// the default's 16,384 has about 2.6 times the reach of 2,500 pairs at 0.005, ending about 0.63
/// times as noisy. To revisit, with `c_end` and the pairs, once the
/// first tunes' trajectories are in.
pub const R_END: f64 = 0.002;

/// The range of a client's clock scaling, against a bench gone wrong.
pub const MIN_CLOCK_FACTOR: f64 = 0.25;
pub const MAX_CLOCK_FACTOR: f64 = 4.0;

/// How far from 1 a clock factor must be to scale at all: two benches of one machine differ by
/// about 2% (a machine against itself, 2026-10-20), which would give the server's own machine a clock
/// of 2.04+0.02 for 2+0.02.
const CLOCK_FACTOR_DEAD_BAND: f64 = 0.05;

/// What a tune tunes: the search's parameters, or the time management's, played with the network's
/// tuned search (the developer, 2026-11-13: a network is trained, then its search tuned, then its
/// time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TuneKind {
    Search,
    Time,
}

impl TuneKind {
    /// As `spec.txt` and a config's `tune:` spell it.
    pub fn text(self) -> &'static str {
        match self {
            TuneKind::Search => "search",
            TuneKind::Time => "time",
        }
    }

    pub fn parse(text: &str) -> Result<TuneKind, String> {
        match text {
            "search" => Ok(TuneKind::Search),
            "time" => Ok(TuneKind::Time),
            _ => Err(format!("a tune's kind is search or time, got {text:?}")),
        }
    }

    /// Its parameters' names, in their order.
    pub fn names(self) -> &'static [&'static str] {
        match self {
            TuneKind::Search => &SearchParameters::NAMES,
            TuneKind::Time => &TimeParameters::NAMES,
        }
    }

    /// Its parameters' starts, ranges and perturbations.
    pub fn specs(self) -> &'static [Spec] {
        match self {
            TuneKind::Search => &SEARCH_SPECS,
            TuneKind::Time => &TIME_SPECS,
        }
    }
}

/// One parameter's start, range and perturbation at the end of the schedule, as real numbers: a
/// search parameter's are whole, a time parameter's are not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    pub start: f64,
    pub min: f64,
    pub max: f64,
    /// The gap at which theta +/- c_end differ by a few Elo. `c_k` shrinks by only 1.1 to 1.2 over
    /// the run, so this is in effect the whole run's perturbation.
    pub c_end: f64,
}

const fn spec(start: f64, min: f64, max: f64, c_end: f64) -> Spec {
    Spec { start, min, max, c_end }
}

/// The ten search parameters, in `SearchParameters::NAMES`' order, each starting at its default (a
/// test holds the two together). Since the defaults moved (2026-11-08): a range re-centred where the
/// new value sat near its edge, reverse futility's floor at half its value and the aspiration
/// window's floor near where the last tune ended; and `c_end` kept at its old share of its value, an
/// eighth for the margins and a fifth for the aspiration window, so a smaller value is not perturbed
/// by a larger part of itself.
pub const SEARCH_SPECS: [Spec; 10] = [
    spec(150.0, 50.0, 400.0, 19.0),
    spec(100.0, 25.0, 300.0, 12.5),
    spec(200.0, 50.0, 400.0, 25.0),
    spec(200.0, 50.0, 400.0, 25.0),
    spec(200.0, 50.0, 400.0, 25.0),
    spec(150.0, 50.0, 400.0, 19.0),
    spec(50.0, 0.0, 200.0, 10.0),
    spec(10.0, 3.0, 40.0, 2.0),
    spec(-35.0, -100.0, -5.0, 5.0),
    spec(-140.0, -512.0, 0.0, 32.0),
];

/// The 22 time parameters, in `TimeParameters::NAMES`' order, each starting at its default (accepted
/// by the developer, 2026-11-13; revisited after the first tune's traces). `c_end` about a tenth of
/// the value's scale; a slope near zero perturbed by its own size rather than its value's. Ranges
/// wide enough that no start sits near an edge, a factor's floor below its intercept, and
/// `multiplier_min < multiplier_max` and `trend_min < trend_max` held by the ranges themselves.
/// `soft_rise` may pass `soft_max`: the time manager floors the soft fraction.
pub const TIME_SPECS: [Spec; 22] = [
    spec(0.0594, 0.03, 0.10, 0.006),
    spec(0.0492, 0.02, 0.08, 0.006),
    spec(0.0386, 0.01, 0.10, 0.006),
    spec(0.7281, 0.30, 0.90, 0.06),
    spec(3.0, 1.5, 6.0, 0.4),
    spec(0.5, 0.2, 0.9, 0.06),
    spec(2.0, 1.2, 4.0, 0.25),
    spec(2.66, 1.5, 4.0, 0.3),
    spec(2.6554, 1.0, 4.0, 0.3),
    spec(0.546, 0.3, 0.9, 0.06),
    spec(0.7426, 0.5, 1.2, 0.08),
    spec(0.048, 0.0, 0.15, 0.008),
    spec(0.7214, 0.4, 1.0, 0.08),
    spec(1.4031, 1.0, 2.5, 0.15),
    spec(1.2881, 0.8, 2.0, 0.12),
    spec(0.044, 0.0, 0.15, 0.008),
    spec(0.716, 0.4, 1.0, 0.08),
    spec(1.2664, 0.8, 2.0, 0.12),
    spec(0.0416, 0.0, 0.15, 0.008),
    spec(0.8642, 0.5, 1.0, 0.08),
    spec(1.15, 0.7, 2.0, 0.12),
    spec(0.2526, 0.0, 1.0, 0.04),
];

/// theta, the tune's current point, as real numbers in its kind's names' order.
pub type Theta = Vec<f64>;

/// The search parameters as real numbers, in `NAMES`' order.
pub fn search_values(parameters: &SearchParameters) -> Vec<f64> {
    parameters.values().iter().map(|&value| f64::from(value)).collect()
}

/// `specs` starting from `start` instead of their own starts, each value clamped into its
/// parameter's range: a tune going on from its teacher's tune. The ranges and perturbations stay.
pub fn specs_from(specs: &[Spec], start: &[f64]) -> Vec<Spec> {
    specs.iter().zip(start).map(|(spec, &value)| Spec { start: value.clamp(spec.min, spec.max), ..*spec }).collect()
}

/// The specs' starts as theta.
pub fn start_theta(specs: &[Spec]) -> Theta {
    specs.iter().map(|spec| spec.start).collect()
}

/// fishtest's gain sequences, each parameter with its own `c` and `a` from its own `c_end`:
/// `c_k = c / (k + 1)^0.101`, `a_k = a / (A + k + 1)^0.602`, `A = N / 10`, worked back from the end,
/// `c = c_end * N^0.101`, `a = a_end * (A + N)^0.602` with `a_end = r_end * c_end^2`.
#[derive(Clone, Debug)]
pub struct Schedule {
    stability: f64,
    c: Vec<f64>,
    a: Vec<f64>,
}

const GAMMA: f64 = 0.101;
const ALPHA: f64 = 0.602;

impl Schedule {
    pub fn new(pairs: u32, r_end: f64, specs: &[Spec]) -> Schedule {
        let n = pairs as f64;
        let stability = n / 10.0;
        let c = specs.iter().map(|spec| spec.c_end * n.powf(GAMMA)).collect();
        let a = specs.iter().map(|spec| r_end * spec.c_end * spec.c_end * (stability + n).powf(ALPHA)).collect();
        Schedule { stability, c, a }
    }

    /// Parameter `i`'s perturbation at pair `k` (0-based).
    pub fn c_k(&self, i: usize, k: u32) -> f64 {
        self.c[i] / (k as f64 + 1.0).powf(GAMMA)
    }

    /// Parameter `i`'s gain at pair `k`.
    pub fn a_k(&self, i: usize, k: u32) -> f64 {
        self.a[i] / (self.stability + k as f64 + 1.0).powf(ALPHA)
    }
}

/// A pair's direction: a random sign per parameter.
pub type Delta = Vec<i32>;

/// The most parameters a Delta can sign: one bit of a 64-bit draw each.
pub const MAX_PARAMETERS: usize = 64;

/// Pair `k`'s Delta over `count` parameters, from `seed` and `k` alone, so a pair's direction is
/// reproducible from its index: splitmix64 over the two, one bit per sign. A shorter Delta is the
/// start of a longer one, so the search's ten signs are what they were before the time tune.
pub fn draw_delta(seed: u64, k: u32, count: usize) -> Delta {
    assert!(count <= MAX_PARAMETERS, "a Delta signs at most {MAX_PARAMETERS} parameters");
    let mut state = seed ^ (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut next = || -> u64 {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let bits = next();
    (0..count).map(|i| if (bits >> i) & 1 == 1 { 1 } else { -1 }).collect()
}

/// A tune's seed from its player's name and kind (FNV-1a), so the same network always draws the same
/// Deltas and a rerun from scratch repeats the run (the developer, 2026-10-18). A search tune's seed
/// is its player's name alone, as before the time tune; a time tune's its name and ` time`, so its
/// Deltas are not the search tune's.
pub fn seed_of(player: &str, kind: TuneKind) -> u64 {
    let text = match kind {
        TuneKind::Search => player.to_string(),
        TuneKind::Time => format!("{player} time"),
    };
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for byte in text.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

fn clip(value: f64, spec: &Spec) -> f64 {
    value.clamp(spec.min, spec.max)
}

/// The values one side of pair `k` plays: theta + `sign` c_k Delta, `sign` +1 or -1, clipped to each
/// range, not rounded (`search_point`, `time_point`).
pub fn point(theta: &[f64], schedule: &Schedule, specs: &[Spec], k: u32, delta: &[i32], sign: i32) -> Vec<f64> {
    (0..specs.len()).map(|i| clip(theta[i] + (sign * delta[i]) as f64 * schedule.c_k(i, k), &specs[i])).collect()
}

/// `values`, ten in `SearchParameters::NAMES`' order, each rounded to the nearest integer.
pub fn search_parameters(values: &[f64]) -> SearchParameters {
    let rounded: [i32; 10] = std::array::from_fn(|i| values[i].round() as i32);
    SearchParameters::from_values(rounded)
}

/// `values`, 22 in `TimeParameters::NAMES`' order, as they are.
pub fn time_parameters(values: &[f64]) -> TimeParameters {
    TimeParameters::from_values(std::array::from_fn(|i| values[i]))
}

/// theta after pair `k`, whose result from the plus side is `r = wins - losses` (-2 to 2): each
/// parameter moves by `a_k / c_k * r * Delta_i` (fishtest's `R_k * c_k * r * flip`, `R_k = a_k /
/// c_k^2`), then is clipped to its range.
pub fn update(theta: &[f64], schedule: &Schedule, specs: &[Spec], k: u32, delta: &[i32], r: i32) -> Theta {
    (0..specs.len())
        .map(|i| {
            let step = schedule.a_k(i, k) / schedule.c_k(i, k) * r as f64 * delta[i] as f64;
            clip(theta[i] + step, &specs[i])
        })
        .collect()
}

/// The clock factor of a client of speed `client_nps` against the server's `reference_nps`: above 1
/// for a slower client, which then gets more time, so every game searches about as many nodes;
/// exactly 1 within `CLOCK_FACTOR_DEAD_BAND`.
pub fn clock_factor(reference_nps: f64, client_nps: f64) -> f64 {
    let factor = (reference_nps / client_nps.max(1.0)).clamp(MIN_CLOCK_FACTOR, MAX_CLOCK_FACTOR);
    if (factor - 1.0).abs() < CLOCK_FACTOR_DEAD_BAND { 1.0 } else { factor }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(left: f64, right: f64) -> bool {
        (left - right).abs() < 1e-9 * right.abs().max(1.0)
    }

    #[test]
    fn the_specs_start_at_the_defaults_inside_their_ranges() {
        assert_eq!(search_parameters(&start_theta(&SEARCH_SPECS)), SearchParameters::DEFAULT);
        assert_eq!(time_parameters(&start_theta(&TIME_SPECS)), TimeParameters::DEFAULT);
        for kind in [TuneKind::Search, TuneKind::Time] {
            assert_eq!(kind.specs().len(), kind.names().len());
            assert!(kind.specs().len() <= MAX_PARAMETERS);
            assert_eq!(TuneKind::parse(kind.text()), Ok(kind));
            for (spec, name) in kind.specs().iter().zip(kind.names()) {
                assert!(spec.min <= spec.start && spec.start <= spec.max, "{name}");
                assert!(spec.c_end > 0.0, "{name}");
            }
        }
        // The bounds that must stay ordered, held by their ranges.
        let by_name = |name: &str| TIME_SPECS[TimeParameters::NAMES.iter().position(|&known| known == name).unwrap()];
        assert!(by_name("multiplier_min").max < by_name("multiplier_max").min);
        assert!(by_name("trend_min").max <= by_name("trend_max").min);
    }

    #[test]
    fn a_tune_from_its_teacher_s_tune_starts_there_clamped_its_ranges_kept() {
        assert_eq!(specs_from(&SEARCH_SPECS, &search_values(&SearchParameters::DEFAULT)), SEARCH_SPECS.to_vec(), "from the defaults, the specs themselves");
        let teacher = SearchParameters { reverse_futility_margin: 96, aspiration_delta: 1, good_quiet_threshold: -600, ..SearchParameters::DEFAULT };
        let specs = specs_from(&SEARCH_SPECS, &search_values(&teacher));
        assert_eq!(search_parameters(&start_theta(&specs)), SearchParameters { aspiration_delta: 3, good_quiet_threshold: -512, ..teacher }, "clamped");
        for (spec, default) in specs.iter().zip(SEARCH_SPECS) {
            assert_eq!((spec.min, spec.max, spec.c_end), (default.min, default.max, default.c_end));
        }
    }

    #[test]
    fn the_schedule_ends_at_c_end_and_a_end() {
        for specs in [&SEARCH_SPECS[..], &TIME_SPECS[..]] {
            let schedule = Schedule::new(PAIRS, R_END, specs);
            for (i, spec) in specs.iter().enumerate() {
                assert!(close(schedule.c_k(i, PAIRS - 1), spec.c_end), "{i}: {}", schedule.c_k(i, PAIRS - 1));
                assert!(close(schedule.a_k(i, PAIRS - 1), R_END * spec.c_end * spec.c_end), "{i}");
                assert!(schedule.c_k(i, 0) > schedule.c_k(i, PAIRS - 1), "c_k shrinks");
                assert!(schedule.a_k(i, 0) > schedule.a_k(i, PAIRS - 1), "a_k shrinks");
            }
        }
    }

    #[test]
    fn a_delta_depends_on_the_seed_and_k_alone_and_is_balanced() {
        assert_eq!(draw_delta(7, 42, 22), draw_delta(7, 42, 22));
        assert_ne!(draw_delta(7, 42, 22), draw_delta(7, 43, 22));
        assert_ne!(draw_delta(7, 42, 22), draw_delta(8, 42, 22));
        assert_eq!(draw_delta(7, 42, 10)[..], draw_delta(7, 42, 22)[..10], "the search's signs, the start of the time's");
        let seed = seed_of("amanda-0001", TuneKind::Time);
        let draws = 10_000;
        for i in 0..22 {
            let plus = (0..draws).filter(|&k| draw_delta(seed, k, 22)[i] == 1).count();
            assert!((4_700..=5_300).contains(&plus), "parameter {i}: {plus} of {draws} positive");
        }
        assert_eq!(seed_of("amanda-0001", TuneKind::Search), seed_of("amanda-0001", TuneKind::Search));
        assert_ne!(seed_of("amanda-0001", TuneKind::Search), seed_of("amanda-0002", TuneKind::Search));
        assert_ne!(seed_of("amanda-0001", TuneKind::Search), seed_of("amanda-0001", TuneKind::Time));
    }

    /// A search tune's seed is what it was before kinds existed, so a tune under way resumes with
    /// the same Deltas: FNV-1a of the name alone.
    #[test]
    fn a_search_tunes_seed_is_its_players_name_alone() {
        assert_eq!(seed_of("", TuneKind::Search), 0xCBF2_9CE4_8422_2325);
    }

    #[test]
    fn a_point_moves_by_c_k_along_delta_clipped_and_rounded() {
        let schedule = Schedule::new(PAIRS, R_END, &SEARCH_SPECS);
        let theta = start_theta(&SEARCH_SPECS);
        let delta: Delta = vec![1, -1, 1, -1, 1, -1, 1, -1, 1, -1];
        let k = PAIRS - 1;
        let side = |theta: &[f64], sign| search_parameters(&point(theta, &schedule, &SEARCH_SPECS, k, &delta, sign)).values();
        let plus = side(&theta, 1);
        let minus = side(&theta, -1);
        // Futility and null move: margins of 200, perturbed by 25 at the end, Delta +1 and -1.
        assert_eq!(plus[2], 225);
        assert_eq!(minus[2], 175);
        assert_eq!(plus[3], 175);
        assert_eq!(plus[7], 8, "10 - 2");
        // Clipped at the range: -6 + 5 is past the top, -5.
        let mut high = theta.clone();
        high[8] = -6.0;
        assert_eq!(side(&high, 1)[8], -5);
        // Rounded to the nearest integer.
        let mut fractional = theta.clone();
        fractional[2] = 200.4;
        assert_eq!(side(&fractional, 1)[2], 225);
    }

    #[test]
    fn a_time_point_is_played_unrounded() {
        let schedule = Schedule::new(PAIRS, R_END, &TIME_SPECS);
        let theta = start_theta(&TIME_SPECS);
        let delta = vec![1; 22];
        let plus = time_parameters(&point(&theta, &schedule, &TIME_SPECS, PAIRS - 1, &delta, 1));
        assert!(close(plus.soft_max, 0.0594 + 0.006));
        assert!(close(plus.hard_soft_ratio, 3.0 + 0.4));
    }

    #[test]
    fn an_update_moves_along_delta_with_the_result_and_clips() {
        let schedule = Schedule::new(PAIRS, R_END, &SEARCH_SPECS);
        let theta = start_theta(&SEARCH_SPECS);
        let delta: Delta = vec![1, -1, 1, -1, 1, -1, 1, -1, 1, -1];
        assert_eq!(update(&theta, &schedule, &SEARCH_SPECS, 0, &delta, 0), theta, "a drawn pair moves nothing");
        // Futility and null move: margins of 200 with the same spec, Delta +1 and -1.
        let won = update(&theta, &schedule, &SEARCH_SPECS, 0, &delta, 2);
        let step = schedule.a_k(2, 0) / schedule.c_k(2, 0) * 2.0;
        assert!(close(won[2], 200.0 + step), "{}", won[2]);
        assert!(close(won[3], 200.0 - step));
        let lost = update(&theta, &schedule, &SEARCH_SPECS, 0, &delta, -2);
        assert!(close(lost[2], 200.0 - step));
        // At the end, a 2-0 pair moves a margin by 2 * r_end * c_end.
        let k = PAIRS - 1;
        let end = update(&theta, &schedule, &SEARCH_SPECS, k, &delta, 2);
        assert!(close(end[2] - 200.0, 2.0 * R_END * 25.0));
        // Clipped at the range.
        let mut low = theta.clone();
        low[2] = 50.0;
        assert_eq!(update(&low, &schedule, &SEARCH_SPECS, 0, &delta, -2)[2], 50.0);
    }

    #[test]
    fn a_slower_client_gets_a_longer_clock_within_its_clamps() {
        assert_eq!(clock_factor(1000.0, 500.0), 2.0);
        assert_eq!(clock_factor(1000.0, 2000.0), 0.5);
        assert_eq!(clock_factor(1000.0, 980.0), 1.0, "inside the dead band");
        assert_eq!(clock_factor(1000.0, 1.0), MAX_CLOCK_FACTOR);
        assert_eq!(clock_factor(1000.0, 1e9), MIN_CLOCK_FACTOR);
    }
}
