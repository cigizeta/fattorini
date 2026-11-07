//! The training server's gate test: a copy of the arena's SPRT (`arena/src/sprt.rs`), whose header
//! explains the derivation; the two binaries share no code beyond `fattorini`.
//! Fed a game pair at a time, as the LAN clients report them: a pair's `r`, its two games' summed
//! `2 * score - 1` for the challenger, is `r + 2` half-points over two games.

pub const ALPHA: f64 = 0.05;
pub const BETA: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Continue,
    Promote,
    Fail,
}

pub struct Sprt {
    slope: f64,
    rate: f64,
    upper: f64,
    lower: f64,
    score_half: i64,
    games: u64,
}

impl Sprt {
    /// The gate's: H0 an even score, H1 `elo_d` Elo stronger.
    pub fn new(elo_d: f64) -> Sprt {
        assert!(elo_d > 0.0, "elo_d must be positive, got {elo_d}");
        Sprt::between(0.0, elo_d)
    }

    /// H0 `elo0` against H1 `elo1`, from the challenger's side: the general form, `new` its case at
    /// `elo0` 0, where `p0` is exactly 0.5 and the constants are the gate's own.
    pub fn between(elo0: f64, elo1: f64) -> Sprt {
        assert!(elo1 > elo0, "elo1 must be above elo0, got {elo0} and {elo1}");
        let expected = |elo: f64| 1.0 / (1.0 + 10.0_f64.powf(-elo / 400.0));
        let p0 = expected(elo0);
        let p1 = expected(elo1);
        let a = (p1 / p0).ln();
        let b = ((1.0 - p1) / (1.0 - p0)).ln();
        let slope = a - b;
        // `-b / slope`, not the midpoint of p0 and p1: see `arena/src/sprt.rs`.
        let rate = -b / slope;
        Sprt { slope, rate, upper: ((1.0 - BETA) / ALPHA).ln(), lower: (BETA / (1.0 - ALPHA)).ln(), score_half: 0, games: 0 }
    }

    /// One pair, the challenger's `r` in -2..=2: two games of `r + 2` half-points.
    pub fn add_pair(&mut self, r: i32) {
        assert!((-2..=2).contains(&r), "a pair's result is -2..=2, got {r}");
        self.score_half += i64::from(r + 2);
        self.games += 2;
    }

    pub fn llr(&self) -> f64 {
        self.slope * (self.score_half as f64 / 2.0 - self.rate * self.games as f64)
    }

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

    /// The challenger's points.
    pub fn score(&self) -> f64 {
        self.score_half as f64 / 2.0
    }

    pub fn bounds(&self) -> (f64, f64) {
        (self.lower, self.upper)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copy_derives_the_arena_s_constants() {
        let sprt = Sprt::new(35.0);
        assert!((sprt.slope - 0.201476).abs() < 1e-6, "slope was {}", sprt.slope);
        assert!((sprt.rate - 0.5251420).abs() < 1e-6, "rate was {}", sprt.rate);
        assert!((sprt.upper - 2.944439).abs() < 1e-6 && (sprt.lower + 2.944439).abs() < 1e-6);
    }

    #[test]
    fn the_general_form_at_zero_is_the_gate_s() {
        let (gate, general) = (Sprt::new(35.0), Sprt::between(0.0, 35.0));
        assert_eq!((gate.slope, gate.rate, gate.upper, gate.lower), (general.slope, general.rate, general.upper, general.lower));
        // H0 at +10: an even score now fails sooner than the gate's, as it should.
        let mut raised = Sprt::between(10.0, 35.0);
        while raised.verdict() == Verdict::Continue {
            raised.add_pair(0);
        }
        assert_eq!(raised.verdict(), Verdict::Fail);
    }

    #[test]
    fn a_strong_challenger_promotes_and_an_even_one_fails() {
        let mut strong = Sprt::new(35.0);
        while strong.verdict() == Verdict::Continue {
            strong.add_pair(if strong.games() % 10 < 6 { 2 } else { 0 });
        }
        assert_eq!(strong.verdict(), Verdict::Promote);
        let mut even = Sprt::new(35.0);
        while even.verdict() == Verdict::Continue {
            even.add_pair(0);
        }
        assert_eq!(even.verdict(), Verdict::Fail);
    }
}
