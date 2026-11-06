//! SPSA as fishtest runs it (`spsa_handler.py`, `spsa_workflow.py`, fetched 2026-10-16), over the
//! search's ten `SearchParameters`: pure functions, no games and no files, so the method is tested on
//! its own. A pair of games, colours reversed on one opening, plays theta + c_k Delta against
//! theta - c_k Delta; its result moves each parameter along Delta, then clips it to its range.
//! `smithy start --spsa` runs it, the arenas playing the pairs.
//!
//! Chosen with the developer on 2026-10-16, `r_end` revised on 2026-10-20: one `r_end` for all
//! parameters (`R_END`), and each
//! parameter's range and `c_end` from its scale (`SPECS`). The number of pairs N is each tune's own,
//! its config's `length:`, `default` (`DEFAULT_TUNE_PAIRS`) or a number. Every tune starts from
//! `SPECS`' starts, the default parameters (the developer, 2026-10-23: no fine-tune from another
//! player's values). theta is kept as real numbers, a player rounding each value to the nearest
//! integer when it plays.

use crate::search::SearchParameters;

/// `length: default`: a tune of 32,768 games, about 14 hours on two machines together (an
/// estimate, 2026-10-20).
pub const DEFAULT_TUNE_PAIRS: u32 = 16_384;

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

/// One parameter's start, range and perturbation at the end of the schedule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    pub start: i32,
    pub min: i32,
    pub max: i32,
    /// The gap at which theta +/- c_end differ by a few Elo. `c_k` shrinks by only 1.1 to 1.2 over
    /// the run, so this is in effect the whole run's perturbation.
    pub c_end: f64,
}

/// The ten parameters, in `SearchParameters::NAMES`' order, each starting at its default.
pub const SPECS: [Spec; 10] = [
    Spec { start: 200, min: 50, max: 400, c_end: 25.0 },
    Spec { start: 200, min: 50, max: 400, c_end: 25.0 },
    Spec { start: 200, min: 50, max: 400, c_end: 25.0 },
    Spec { start: 200, min: 50, max: 400, c_end: 25.0 },
    Spec { start: 200, min: 50, max: 400, c_end: 25.0 },
    Spec { start: 200, min: 50, max: 400, c_end: 25.0 },
    Spec { start: 50, min: 0, max: 200, c_end: 10.0 },
    Spec { start: 20, min: 5, max: 60, c_end: 4.0 },
    Spec { start: -35, min: -100, max: -5, c_end: 5.0 },
    Spec { start: -128, min: -512, max: 0, c_end: 32.0 },
];

/// theta, the tune's current point, as real numbers in `SearchParameters::NAMES`' order.
pub type Theta = [f64; 10];

/// The specs' starts as theta.
pub fn start_theta(specs: &[Spec; 10]) -> Theta {
    specs.map(|spec| spec.start as f64)
}

/// fishtest's gain sequences, each parameter with its own `c` and `a` from its own `c_end`:
/// `c_k = c / (k + 1)^0.101`, `a_k = a / (A + k + 1)^0.602`, `A = N / 10`, worked back from the end,
/// `c = c_end * N^0.101`, `a = a_end * (A + N)^0.602` with `a_end = r_end * c_end^2`.
#[derive(Clone, Debug)]
pub struct Schedule {
    stability: f64,
    c: [f64; 10],
    a: [f64; 10],
}

const GAMMA: f64 = 0.101;
const ALPHA: f64 = 0.602;

impl Schedule {
    pub fn new(pairs: u32, r_end: f64, specs: &[Spec; 10]) -> Schedule {
        let n = pairs as f64;
        let stability = n / 10.0;
        let c = specs.map(|spec| spec.c_end * n.powf(GAMMA));
        let a = specs.map(|spec| r_end * spec.c_end * spec.c_end * (stability + n).powf(ALPHA));
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
pub type Delta = [i32; 10];

/// Pair `k`'s Delta, from `seed` and `k` alone, so a pair's direction is reproducible from its index:
/// splitmix64 over the two, one bit per sign.
pub fn draw_delta(seed: u64, k: u32) -> Delta {
    let mut state = seed ^ (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut next = || -> u64 {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let bits = next();
    std::array::from_fn(|i| if (bits >> i) & 1 == 1 { 1 } else { -1 })
}

/// A tune's seed from its player's name (FNV-1a), so the same network always draws the same Deltas
/// and a rerun from scratch repeats the run (the developer, 2026-10-18).
pub fn seed_of(player: &str) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for byte in player.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

fn clip(value: f64, spec: &Spec) -> f64 {
    value.clamp(spec.min as f64, spec.max as f64)
}

/// The parameters one side of pair `k` plays: theta + `sign` c_k Delta, `sign` +1 or -1, clipped to
/// each range and rounded.
pub fn point(theta: &Theta, schedule: &Schedule, specs: &[Spec; 10], k: u32, delta: &Delta, sign: i32) -> SearchParameters {
    let values: [i32; 10] = std::array::from_fn(|i| {
        let shifted = theta[i] + (sign * delta[i]) as f64 * schedule.c_k(i, k);
        clip(shifted, &specs[i]).round() as i32
    });
    SearchParameters::from_values(values)
}

/// theta after pair `k`, whose result from the plus side is `r = wins - losses` (-2 to 2): each
/// parameter moves by `a_k / c_k * r * Delta_i` (fishtest's `R_k * c_k * r * flip`, `R_k = a_k /
/// c_k^2`), then is clipped to its range.
pub fn update(theta: &Theta, schedule: &Schedule, specs: &[Spec; 10], k: u32, delta: &Delta, r: i32) -> Theta {
    std::array::from_fn(|i| {
        let step = schedule.a_k(i, k) / schedule.c_k(i, k) * r as f64 * delta[i] as f64;
        clip(theta[i] + step, &specs[i])
    })
}

/// theta rounded, as the tune's result.
pub fn rounded(theta: &Theta) -> SearchParameters {
    SearchParameters::from_values(theta.map(|value| value.round() as i32))
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
    fn the_specs_start_at_the_search_defaults_inside_their_ranges() {
        assert_eq!(rounded(&start_theta(&SPECS)), SearchParameters::DEFAULT);
        for (i, spec) in SPECS.iter().enumerate() {
            assert!(spec.min <= spec.start && spec.start <= spec.max, "{}", SearchParameters::NAMES[i]);
            assert!(spec.c_end > 0.0);
        }
    }

    #[test]
    fn the_schedule_ends_at_c_end_and_a_end() {
        let schedule = Schedule::new(PAIRS, R_END, &SPECS);
        for (i, spec) in SPECS.iter().enumerate() {
            assert!(close(schedule.c_k(i, PAIRS - 1), spec.c_end), "{i}: {}", schedule.c_k(i, PAIRS - 1));
            assert!(close(schedule.a_k(i, PAIRS - 1), R_END * spec.c_end * spec.c_end), "{i}");
            assert!(schedule.c_k(i, 0) > schedule.c_k(i, PAIRS - 1), "c_k shrinks");
            assert!(schedule.a_k(i, 0) > schedule.a_k(i, PAIRS - 1), "a_k shrinks");
        }
    }

    #[test]
    fn a_delta_depends_on_the_seed_and_k_alone_and_is_balanced() {
        assert_eq!(draw_delta(7, 42), draw_delta(7, 42));
        assert_ne!(draw_delta(7, 42), draw_delta(7, 43));
        assert_ne!(draw_delta(7, 42), draw_delta(8, 42));
        let draws = 10_000;
        for i in 0..10 {
            let plus = (0..draws).filter(|&k| draw_delta(seed_of("amanda-0001"), k)[i] == 1).count();
            assert!((4_700..=5_300).contains(&plus), "parameter {i}: {plus} of {draws} positive");
        }
        assert_eq!(seed_of("amanda-0001"), seed_of("amanda-0001"));
        assert_ne!(seed_of("amanda-0001"), seed_of("amanda-0002"));
    }

    #[test]
    fn a_point_moves_by_c_k_along_delta_clipped_and_rounded() {
        let schedule = Schedule::new(PAIRS, R_END, &SPECS);
        let theta = start_theta(&SPECS);
        let delta: Delta = [1, -1, 1, -1, 1, -1, 1, -1, 1, -1];
        let k = PAIRS - 1;
        let plus = point(&theta, &schedule, &SPECS, k, &delta, 1).values();
        let minus = point(&theta, &schedule, &SPECS, k, &delta, -1).values();
        assert_eq!(plus[0], 225);
        assert_eq!(minus[0], 175);
        assert_eq!(plus[1], 175);
        assert_eq!(plus[7], 16, "20 - 4");
        // Clipped at the range: -6 + 5 is past the top, -5.
        let mut high = theta;
        high[8] = -6.0;
        assert_eq!(point(&high, &schedule, &SPECS, k, &delta, 1).values()[8], -5);
        // Rounded to the nearest integer.
        let mut fractional = theta;
        fractional[0] = 200.4;
        assert_eq!(point(&fractional, &schedule, &SPECS, k, &delta, 1).values()[0], 225);
    }

    #[test]
    fn an_update_moves_along_delta_with_the_result_and_clips() {
        let schedule = Schedule::new(PAIRS, R_END, &SPECS);
        let theta = start_theta(&SPECS);
        let delta: Delta = [1, -1, 1, -1, 1, -1, 1, -1, 1, -1];
        assert_eq!(update(&theta, &schedule, &SPECS, 0, &delta, 0), theta, "a drawn pair moves nothing");
        let won = update(&theta, &schedule, &SPECS, 0, &delta, 2);
        let step = schedule.a_k(0, 0) / schedule.c_k(0, 0) * 2.0;
        assert!(close(won[0], 200.0 + step), "{}", won[0]);
        assert!(close(won[1], 200.0 - step));
        let lost = update(&theta, &schedule, &SPECS, 0, &delta, -2);
        assert!(close(lost[0], 200.0 - step));
        // At the end, a 2-0 pair moves a margin by 2 * r_end * c_end.
        let k = PAIRS - 1;
        let end = update(&theta, &schedule, &SPECS, k, &delta, 2);
        assert!(close(end[0] - 200.0, 2.0 * R_END * 25.0));
        // Clipped at the range.
        let mut low = theta;
        low[0] = 50.0;
        assert_eq!(update(&low, &schedule, &SPECS, 0, &delta, -2)[0], 50.0);
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
