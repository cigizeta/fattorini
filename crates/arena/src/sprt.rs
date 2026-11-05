//! The sequential probability ratio test that decides a gate, implemented exactly as specified and
//! not re-derived here. Three constants are chosen and everything else follows from them.
//!
//! The one thing to know before changing anything in this file: the error bounds do not come from
//! Wald's argument, because these three increments are not the log-likelihood ratios of any pair of
//! three-outcome distributions. They come from `exp(LLR)` being a supermartingale under `H0`, which
//! holds because `exp(A) + exp(B) = 2` exactly when `p0 = 0.5` and because the draw increment is the
//! mean of the other two. Every element of the derivation below is load-bearing for that. In
//! particular `rate` is `-B / slope` and **not** the midpoint `(p0 + p1) / 2`: the two differ in the
//! fifth decimal and belong to two different tests, the midpoint to the normal-approximation SPRT
//! that estimates a variance. Substituting it would silently replace this test with that one.

/// The gain the tests below are written at. A run takes its own: a network's gate from its
/// `net.yaml`, `arena sprt` from the command line.
#[cfg(test)]
pub const DEFAULT_ELO_D: f64 = 35.0;
/// False positive: accepting "at least `elo_d` stronger" when the two are equal. Fixed for every run,
/// the gate and `arena sprt` alike, so "the test passed" means the same thing everywhere; only the
/// margin varies. `Sprt::new` still takes the rates as parameters, so its derivation stays testable
/// at other values.
pub const ALPHA: f64 = 0.05;
/// False negative: rejecting it when the difference really is `elo_d`.
pub const BETA: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Neither bound crossed. More games.
    Continue,
    /// The challenger is at least `elo_d` stronger, at the chosen error rates.
    Promote,
    /// It is not.
    Fail,
}

/// The derived constants plus the running counter, which is the whole of the test's state: an integer
/// count of half-points and a game count. Nothing else, and in particular no draw rate.
pub struct Sprt {
    /// The three chosen numbers, kept so a run can record which test it actually ran.
    elo_d: f64,
    alpha: f64,
    beta: f64,
    /// `1 / (1 + 10^(-elo_d / 400))`, the sigmoid Elo formula -- the single source of truth the rest
    /// of the chain derives from.
    p1: f64,
    /// `ln(p1 / p0)`, the LLR of a win.
    a: f64,
    /// `ln((1 - p1) / (1 - p0))`, the LLR of a loss. Negative, and larger in magnitude than `a` --
    /// that asymmetry is load-bearing rather than incidental: a loss must outweigh a win, or an even
    /// result would not be evidence against `H1` and the test would never terminate at `H0`.
    b: f64,
    /// `a - b`. Equals `ln(10) * elo_d / 400` exactly, but derived from `a` and `b` so that `p1`
    /// stays the one source of truth.
    slope: f64,
    /// `-b / slope`. **Not** the midpoint of `p0` and `p1`; see this module's own header.
    rate: f64,
    /// `ln((1 - beta) / alpha)`.
    upper: f64,
    /// `ln(beta / (1 - alpha))`. Computed from both error rates rather than negated from `upper`, so
    /// that raising one alone still gives the right test.
    lower: f64,
    /// The challenger's score, in half-points: a win is `2`, a draw `1`, a loss `0`. An integer so
    /// the accumulation stays exact over thousands of games, with no floating-point drift.
    score_half: i64,
    games: u64,
    /// Wins, losses and draws from the challenger's side. Reporting only -- the test itself needs
    /// nothing but `score_half` and `games`, and deliberately never estimates a draw rate. Kept
    /// because `+34 -6 =22` says what a run actually looks like in a way a score cannot: two runs
    /// with the same score can be a drawish match and a bloodbath.
    wins: u64,
    losses: u64,
    draws: u64,
}

impl Sprt {
    /// Derives the whole chain from the three chosen numbers. A function rather than a set of
    /// `const`s because Rust cannot evaluate `ln` or `powf` in a `const` -- which turns out to be
    /// convenient, since it makes the derivation testable at parameter values other than the
    /// production ones.
    pub fn new(elo_d: f64, alpha: f64, beta: f64) -> Sprt {
        assert!(elo_d > 0.0, "elo_d must be positive, got {elo_d}");
        assert!(alpha > 0.0 && alpha < 1.0, "alpha must be in (0, 1), got {alpha}");
        assert!(beta > 0.0 && beta < 1.0, "beta must be in (0, 1), got {beta}");

        let p0 = 0.5;
        let p1 = 1.0 / (1.0 + 10.0_f64.powf(-elo_d / 400.0));
        let a = (p1 / p0).ln();
        let b = ((1.0 - p1) / (1.0 - p0)).ln();
        let slope = a - b;
        let rate = -b / slope;
        let upper = ((1.0 - beta) / alpha).ln();
        let lower = (beta / (1.0 - alpha)).ln();

        Sprt {
            elo_d, alpha, beta, p1, a, b, slope, rate, upper, lower,
            score_half: 0, games: 0, wins: 0, losses: 0, draws: 0,
        }
    }

    /// The test at the production error rates and the tests' margin.
    #[cfg(test)]
    pub fn with_default_constants() -> Sprt {
        Sprt::new(DEFAULT_ELO_D, ALPHA, BETA)
    }

    /// Records one game from the challenger's point of view: `1.0`, `0.5` or `0.0`, and nothing else.
    ///
    /// One call per game even when games arrive in colour-reversed pairs -- a completed pair applies
    /// two ordinary updates rather than one pair-level observation, so there is nothing about pairing
    /// for this function to know.
    pub fn add_game(&mut self, challenger_score: f64) {
        let half = match challenger_score {
            1.0 => {
                self.wins += 1;
                2
            }
            0.5 => {
                self.draws += 1;
                1
            }
            0.0 => {
                self.losses += 1;
                0
            }
            other => panic!("a game's score must be 1.0, 0.5 or 0.0, got {other}"),
        };
        self.score_half += half;
        self.games += 1;
    }

    /// Wins, losses and draws, from the challenger's side.
    pub fn record(&self) -> (u64, u64, u64) {
        (self.wins, self.losses, self.draws)
    }

    /// `slope * (S - rate * N)`, the log-likelihood ratio the two bounds are compared against. Linear
    /// in the score, which is what makes the running counter sufficient.
    pub fn llr(&self) -> f64 {
        let score = self.score_half as f64 / 2.0;
        self.slope * (score - self.rate * self.games as f64)
    }

    /// Compared in half-points against `2 * rate * N + 2 * bound / slope`, so the comparison is an
    /// exact integer against a float rather than a difference of two accumulated floats.
    pub fn verdict(&self) -> Verdict {
        let score_half = self.score_half as f64;
        let scaled_games = 2.0 * self.rate * self.games as f64;
        if score_half >= scaled_games + 2.0 * self.upper / self.slope {
            return Verdict::Promote;
        }
        if score_half <= scaled_games + 2.0 * self.lower / self.slope {
            return Verdict::Fail;
        }
        Verdict::Continue
    }

    pub fn games(&self) -> u64 {
        self.games
    }

    /// The challenger's score in points, for reporting only -- the test itself never leaves
    /// half-points.
    pub fn score(&self) -> f64 {
        self.score_half as f64 / 2.0
    }

    pub fn upper_bound(&self) -> f64 {
        self.upper
    }

    pub fn lower_bound(&self) -> f64 {
        self.lower
    }

    /// The whole derived chain as one line, printed when a gate starts so the run's own log records
    /// precisely which test produced its verdict. Worth having rather than trusting that the constants
    /// in the config were the ones in effect: a gate's result is only as interpretable as the test
    /// behind it, and this is the cheapest possible way to make that auditable after the fact.
    pub fn describe(&self) -> String {
        format!(
            "elo_d {:.1}, alpha {:.3}, beta {:.3} -> p1 {:.7}, A {:+.6}, B {:+.6}, slope {:.6}, rate {:.7}, bounds {:+.3} / {:+.3}",
            self.elo_d, self.alpha, self.beta, self.p1, self.a, self.b, self.slope, self.rate, self.upper, self.lower
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table in `sprt.md`, which exists to check exactly this. Six decimal places, because the
    /// point of pinning them is to catch a substituted formula rather than a rounding difference --
    /// `rate` against the midpoint is a discrepancy in the fifth decimal.
    #[test]
    fn the_derived_chain_matches_the_specified_values_at_the_production_constants() {
        let sprt = Sprt::with_default_constants();
        assert!((sprt.p1 - 0.5501993).abs() < 1e-6, "p1 was {}", sprt.p1);
        assert!((sprt.a - 0.095672).abs() < 1e-6, "A was {}", sprt.a);
        assert!((sprt.b - -0.105804).abs() < 1e-6, "B was {}", sprt.b);
        assert!((sprt.slope - 0.201476).abs() < 1e-6, "slope was {}", sprt.slope);
        assert!((sprt.rate - 0.5251420).abs() < 1e-6, "rate was {}", sprt.rate);
        assert!((sprt.upper - 2.944439).abs() < 1e-6, "upper was {}", sprt.upper);
        assert!((sprt.lower - -2.944439).abs() < 1e-6, "lower was {}", sprt.lower);
    }

    /// The one substitution that would silently turn this into a different test. They agree to four
    /// decimals, which is exactly why a test has to look at the fifth.
    #[test]
    fn rate_is_not_the_midpoint_of_the_two_hypotheses() {
        let sprt = Sprt::with_default_constants();
        let midpoint = (0.5 + sprt.p1) / 2.0;
        assert!((midpoint - 0.5250997).abs() < 1e-6, "the midpoint was {midpoint}");
        assert!(
            (sprt.rate - midpoint).abs() > 1e-5,
            "rate {} should differ from the midpoint {midpoint} in the fifth decimal",
            sprt.rate
        );
    }

    /// `slope = a - b` and `ln(10) * elo_d / 400` are the same number. Pinned because the second form
    /// is the tempting one to write, and it would decouple `slope` from `p1`.
    #[test]
    fn slope_agrees_with_the_closed_form_in_elo() {
        for elo_d in [1.0, 10.0, 35.0, 200.0] {
            let sprt = Sprt::new(elo_d, 0.05, 0.05);
            let closed_form = 10.0_f64.ln() * elo_d / 400.0;
            assert!((sprt.slope - closed_form).abs() < 1e-12, "at elo_d {elo_d}: {} vs {closed_form}", sprt.slope);
        }
    }

    /// The `ELO_D = 10` column of the same table, so the derivation is checked away from the
    /// production values and not only at them.
    #[test]
    fn the_derived_chain_is_right_at_ten_elo_too() {
        let sprt = Sprt::new(10.0, 0.05, 0.05);
        assert!((sprt.p1 - 0.5143870).abs() < 1e-6, "p1 was {}", sprt.p1);
        assert!((sprt.a - 0.028368).abs() < 1e-6, "A was {}", sprt.a);
        assert!((sprt.b - -0.029196).abs() < 1e-6, "B was {}", sprt.b);
        assert!((sprt.slope - 0.057565).abs() < 1e-6, "slope was {}", sprt.slope);
        assert!((sprt.rate - 0.5071944).abs() < 1e-6, "rate was {}", sprt.rate);
        // The bounds depend on alpha and beta alone, so they do not move with elo_d at all -- which is
        // the whole reason a smaller elo_d costs more games rather than fewer.
        assert!((sprt.upper - 2.944439).abs() < 1e-6, "upper was {}", sprt.upper);
    }

    /// Raising one error rate alone must still give a valid, asymmetric test -- the reason `lower` is
    /// derived from both rather than negated from `upper`.
    #[test]
    fn the_two_bounds_are_independent_of_each_other() {
        let sprt = Sprt::new(35.0, 0.05, 0.20);
        assert!((sprt.upper - (0.80_f64 / 0.05).ln()).abs() < 1e-12);
        assert!((sprt.lower - (0.20_f64 / 0.95).ln()).abs() < 1e-12);
        assert!(
            (sprt.upper + sprt.lower).abs() > 1e-3,
            "with alpha != beta the bounds should not be symmetric, got {} and {}",
            sprt.upper,
            sprt.lower
        );
    }

    /// The score counts a draw as half a point.
    #[test]
    fn the_score_counts_wins_and_half_draws() {
        let mut sprt = Sprt::with_default_constants();
        for score in [1.0, 0.0, 0.5, 0.5, 1.0] {
            sprt.add_game(score);
        }
        assert_eq!(sprt.score(), 3.0);
        assert_eq!(sprt.games(), 5);
    }

    #[test]
    fn a_fresh_test_has_a_zero_llr_and_continues() {
        let sprt = Sprt::with_default_constants();
        assert_eq!(sprt.llr(), 0.0);
        assert_eq!(sprt.verdict(), Verdict::Continue);
        assert_eq!(sprt.games(), 0);
    }

    /// Half-points are integers, so 1000 draws leave the score exactly at 500 -- the property the
    /// integer counter exists for.
    #[test]
    fn a_thousand_draws_accumulate_without_drift() {
        let mut sprt = Sprt::with_default_constants();
        for _ in 0..1000 {
            sprt.add_game(0.5);
        }
        assert_eq!(sprt.games(), 1000);
        assert_eq!(sprt.score(), 500.0);
    }

    /// All draws is an even result, which is evidence against `H1`: the test must fail rather than run
    /// forever. This is what the win/loss asymmetry buys.
    #[test]
    fn an_all_draw_run_fails_rather_than_continuing_indefinitely() {
        let mut sprt = Sprt::with_default_constants();
        let mut verdict = Verdict::Continue;
        for _ in 0..5000 {
            sprt.add_game(0.5);
            verdict = sprt.verdict();
            if verdict != Verdict::Continue {
                break;
            }
        }
        assert_eq!(verdict, Verdict::Fail, "after {} games, llr {}", sprt.games(), sprt.llr());
    }

    /// A 60% challenger is comfortably above `H1`, so it promotes, and in the hundreds of games the
    /// specification predicts rather than thousands.
    #[test]
    fn a_sixty_percent_challenger_promotes() {
        let mut sprt = Sprt::with_default_constants();
        let mut verdict = Verdict::Continue;
        // 3 wins, 2 losses per 10 games is 60%, spread out rather than front-loaded so the walk has
        // to actually accumulate.
        for game in 0..5000 {
            sprt.add_game(if game % 10 < 6 { 1.0 } else { 0.0 });
            verdict = sprt.verdict();
            if verdict != Verdict::Continue {
                break;
            }
        }
        assert_eq!(verdict, Verdict::Promote, "after {} games, llr {}", sprt.games(), sprt.llr());
        assert!(sprt.games() < 1000, "a 60% score should resolve in the hundreds, took {}", sprt.games());
    }

    /// A 40% challenger is the mirror image and must fail.
    #[test]
    fn a_forty_percent_challenger_fails() {
        let mut sprt = Sprt::with_default_constants();
        let mut verdict = Verdict::Continue;
        for game in 0..5000 {
            sprt.add_game(if game % 10 < 4 { 1.0 } else { 0.0 });
            verdict = sprt.verdict();
            if verdict != Verdict::Continue {
                break;
            }
        }
        assert_eq!(verdict, Verdict::Fail, "after {} games, llr {}", sprt.games(), sprt.llr());
    }

    /// The promotion threshold stated in points rather than as an LLR: a score exceeding `rate` of the
    /// games by `upper / slope` points promotes. `sprt.md` gives 52.512% and 14.61 points, and says
    /// that means 56.2% at 400 games -- checked here against the implementation.
    #[test]
    fn the_promotion_threshold_matches_the_stated_percentages() {
        let sprt = Sprt::with_default_constants();
        let margin_points = sprt.upper / sprt.slope;
        assert!((margin_points - 14.614).abs() < 0.01, "the margin was {margin_points} points");
        let needed_at_400 = sprt.rate * 400.0 + margin_points;
        assert!((needed_at_400 / 400.0 - 0.562).abs() < 0.001, "at 400 games it needed {}", needed_at_400 / 400.0);
    }

    /// The LLR is the same function of `(S, N)` however the games arrived, which is what makes pairing
    /// a scheduling decision and not a change to the statistic.
    #[test]
    fn the_llr_depends_only_on_the_score_and_the_game_count_not_on_the_order() {
        let mut in_order = Sprt::with_default_constants();
        for score in [1.0, 1.0, 0.5, 0.0] {
            in_order.add_game(score);
        }
        let mut shuffled = Sprt::with_default_constants();
        for score in [0.0, 0.5, 1.0, 1.0] {
            shuffled.add_game(score);
        }
        assert_eq!(in_order.llr(), shuffled.llr());
    }

    #[test]
    #[should_panic(expected = "must be 1.0, 0.5 or 0.0")]
    fn a_score_that_is_not_a_chess_result_is_rejected() {
        let mut sprt = Sprt::with_default_constants();
        sprt.add_game(0.75);
    }
}
