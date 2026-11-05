//! The duel: which of two engines is stronger, with no Elo margin anywhere. Its unit is the
//! colour-reversed pair: a pair's score `y` is the challenger's points in it, halved (0, 0.25, 0.5,
//! 0.75 or 1), and S the mean of the pairs' `y`s. After every pair, an engine whose S is past
//! `0.5 + C * sigma_hat / sqrt(P)` wins; at `MAX_PAIRS` with neither past it, the duel is drawn.
//!
//! **The pair, not the game (the developer, 2026-10-31, duel-per-pair).** A pair's two games share a
//! UHO opening, so they are not independent: the side with the opening's edge tends to win it, and
//! pairs split 1-1 more often than two independent games would. Taking games as independent, as the
//! rule did until 2026-10-31, is safe only while pairs split; pairs that reinforced each other
//! (2-0 or 0-2 more often than chance) would have given it 18% false winners between equal engines.
//!
//! **`sigma_hat`, measured within each duel:** the standard deviation of its pairs' `y`, so the
//! threshold follows any two engines, many draws and cancelling pairs giving a finer one, engines
//! that never draw one near the old rule's. The old rule took the largest deviation a game can have,
//! 0.5, which at 45% draws put its threshold near 4.2 real standard errors, not 3.09: a bar so high
//! that every search change of 2026-10-31 was drawn. One 0-2 and one 2-0 pseudo-pair are added to the
//! deviation (not to S), so that a few equal pairs early on (a deviation of 0) cannot make a lucky
//! streak a win; their weight fades as pairs accumulate, about 2% at 100 pairs.
//!
//! `C` is `Phi^-1(0.999)`, a 0.1% chance per check that luck alone puts one particular equal engine
//! past it, not fitted to any engine (the developer, 2026-10-31: "C cannot depend on our networks;
//! it must be general"). Checked after every pair, up to `MAX_PAIRS` times, the chances accumulate:
//! measured on equal engines (`false_winners_per_pair`), under 5% false winners for the whole duel,
//! both engines counted, wherever pairs are not reinforcing (0.9% to 3.6% over independent games
//! and cancelling pairs, 6.9% at the reinforcing worst case, where the old rule gave 18%). Without
//! the cap, any constant `C` would be crossed eventually (the law of the iterated logarithm), which
//! is why the cap is part of the rule.
//!
//! **`MAX_PAIRS`, 400 pairs, 800 games (the developer, 2026-10-31),** a cost: the most a duel may
//! take. A drawn duel ends near it, so it sets a drawn duel's length; the old 1,000 games' draws ran
//! to about 900, and "what is annoying is having to wait 900-1000 games for a duel that is drawn".
//!
//! **Curtailed once a verdict is impossible** (the developer, 2026-10-08): after the win checks, a
//! duel neither of whose engines could pass its bound at any later check, whatever the pairs still
//! to come, ends drawn at once, exactly the verdict the cap would give (`can_still_win`). Nothing is
//! estimated, so it never costs a win, and the false-winner rate stands. A rule that stops earlier
//! on a projection was tried on 2026-10-08 and lost about a sixth of a +40 Elo engine's wins.

use super::runner::{Outcome, SequentialTest, Tally};

/// `Phi^-1(0.999) = 3.090232...`, rounded: the extra digits move the threshold by less than 0.0001
/// at `MAX_PAIRS`.
pub const C: f64 = 3.09;

/// The duel ends here regardless, drawn if neither engine is past its threshold.
pub const MAX_PAIRS: u64 = 400;

/// What the pairs played add up to, for one engine: a pair's score in half-points `q` (0 to 4, so
/// `y = q / 4`), their count, sum and sum of squares, exact in integers.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pairs {
    count: u64,
    sum: u64,
    squares: u64,
}

impl Pairs {
    /// `sigma_hat`: the standard deviation of `y` over these pairs and the two pseudo-pairs (`q` 0
    /// and 4), divided by its count less one.
    fn sigma(self) -> f64 {
        let n = (self.count + 2) as f64;
        let sum = (self.sum + 4) as f64;
        let squares = (self.squares + 16) as f64;
        ((squares - sum * sum / n) / (n - 1.0)).max(0.0).sqrt() / 4.0
    }

    /// S: the mean of `y`.
    fn mean(self) -> f64 {
        self.sum as f64 / (4 * self.count) as f64
    }

    /// The same pairs from the other engine's side: `q` becomes `4 - q`.
    fn other_side(self) -> Pairs {
        Pairs { count: self.count, sum: 4 * self.count - self.sum, squares: 16 * self.count + self.squares - 8 * self.sum }
    }

    /// Whether this engine is past its threshold.
    fn passes(self) -> bool {
        self.count > 0 && self.mean() > Duel::threshold(self.count, self.sigma())
    }

    /// Whether this engine could still pass its threshold at some later check, up to `MAX_PAIRS`,
    /// whatever the pairs to come. For `r` more pairs scoring `k` half-points in all, its best
    /// chance is the lowest final deviation, the `r` scores as equal as the grid allows (`k / r`
    /// each, the remainder one half-point higher): the sum of squares is the least for that sum.
    /// So checking every `r` and `k` is exact, from all won down, which finds a passing case at once
    /// while one is easy.
    fn can_still_win(self) -> bool {
        let left = MAX_PAIRS.saturating_sub(self.count);
        for more in (1..=left).rev() {
            for half_points in (0..=4 * more).rev() {
                let (base, extra) = (half_points / more, half_points % more);
                let after = Pairs {
                    count: self.count + more,
                    sum: self.sum + half_points,
                    squares: self.squares + more * base * base + extra * (2 * base + 1),
                };
                if after.passes() {
                    return true;
                }
            }
        }
        false
    }
}

pub struct Duel {
    tally: Tally,
    /// The challenger's score in the first game of the pair under way, until its second arrives.
    first: Option<f64>,
    /// The challenger's pairs.
    pairs: Pairs,
    /// Whether to curtail; off only for the tests' reference.
    curtails: bool,
}

impl Duel {
    pub fn new() -> Duel {
        Duel { tally: Tally::new(), first: None, pairs: Pairs { count: 0, sum: 0, squares: 0 }, curtails: true }
    }

    /// `0.5 + C * sigma / sqrt(pairs)`, the mean pair score either engine has to exceed.
    pub fn threshold(pairs: u64, sigma: f64) -> f64 {
        0.5 + C * sigma / (pairs as f64).sqrt()
    }

    /// Whether neither engine can pass its bound any more, whatever the pairs left (`can_still_win`).
    fn is_curtailed(&self) -> bool {
        let pairs = self.pairs.count;
        self.curtails
            && pairs > 0
            && pairs < MAX_PAIRS
            && !self.pairs.can_still_win()
            && !self.pairs.other_side().can_still_win()
    }
}

/// A game's score in half-points: 2, 1 or 0.
fn half_points(score: f64) -> u64 {
    (score * 2.0) as u64
}

impl SequentialTest for Duel {
    fn add_game(&mut self, challenger_score: f64) {
        self.tally.add_game(challenger_score);
        match self.first.take() {
            None => self.first = Some(challenger_score),
            Some(first) => {
                let q = half_points(first) + half_points(challenger_score);
                self.pairs.count += 1;
                self.pairs.sum += q;
                self.pairs.squares += q * q;
            }
        }
    }

    /// A win is checked before the cap, so a duel that crosses the threshold on its last pair is won,
    /// not drawn; curtailment after both, before the cap.
    fn outcome(&self) -> Option<Outcome> {
        if self.pairs.count == 0 {
            return None;
        }
        if self.pairs.passes() {
            return Some(Outcome::ChallengerWins);
        }
        if self.pairs.other_side().passes() {
            return Some(Outcome::ChampionWins);
        }
        if self.pairs.count >= MAX_PAIRS || self.is_curtailed() {
            return Some(Outcome::Drawn);
        }
        None
    }

    fn record(&self) -> (u64, u64, u64) {
        self.tally.record()
    }

    fn score(&self) -> f64 {
        self.tally.score()
    }

    fn games(&self) -> u64 {
        self.tally.games()
    }

    fn status(&self) -> String {
        let pairs = self.pairs.count;
        if pairs == 0 {
            return "no pairs yet".to_string();
        }
        // The champion's mean, as every number printed (`runner::summary`); the bounds are symmetric.
        let champion = self.pairs.other_side().mean();
        // `sigma_hat` is not printed: the bounds carry it, `(threshold - 0.5) * sqrt(pairs) / C`.
        let threshold = Duel::threshold(pairs, self.pairs.sigma());
        let bounds = format!("S {champion:.4} [{:.4}, {threshold:.4}]", 1.0 - threshold);
        // So that a curtailed draw is never mistaken for one at the cap.
        if self.is_curtailed() {
            return format!(
                "{bounds}, curtailed at {} pairs: neither can pass its bound by {}",
                progress::grouped(pairs),
                progress::grouped(MAX_PAIRS)
            );
        }
        bounds
    }

    /// The cap: the duel may resolve earlier, so the time left it gives is an upper bound.
    fn most_pairs(&self) -> Option<u64> {
        Some(MAX_PAIRS)
    }

    /// The pairs left if the leader's mean and `sigma_hat` stay where they are, to whichever comes
    /// first: its threshold falls to it after `(C * sigma / d)^2` pairs, `d` its distance above an
    /// even score; or, its mean `p` too low for that, the duel is curtailed once `p * P + (MAX_PAIRS -
    /// P)` no longer reaches the cap's bound, after `MAX_PAIRS * (1 - bound) / (1 - p)` pairs; or the
    /// cap. The cap alone overstated a clear duel fourfold (2026-10-19: "ETA 41 min" one pair before
    /// it resolved at 220 games), and an even one by a tenth (2026-10-31). A projection, not a bound:
    /// early on the score still moves, and so does the ETA.
    fn pairs_left_estimate(&self) -> Option<u64> {
        let pairs = self.pairs.count;
        if pairs == 0 {
            return Some(MAX_PAIRS);
        }
        let sigma = self.pairs.sigma();
        let distance = (self.pairs.mean() - 0.5).abs();
        let won = if distance > 0.0 { (C * sigma / distance).powi(2).ceil() } else { f64::INFINITY };
        let bound = Duel::threshold(MAX_PAIRS, sigma);
        let curtailed = (MAX_PAIRS as f64 * (1.0 - bound) / (0.5 - distance)).ceil();
        let projected = won.min(curtailed).min(MAX_PAIRS as f64) as u64;
        Some(projected.max(pairs) - pairs)
    }

    fn describe(&self) -> String {
        format!(
            "duel: pairs, c {C} (Phi^-1(0.999)), threshold 0.5 + c * sigma / sqrt(P), sigma the pair scores' deviation \
             with a 0-2 and a 2-0 pair added, drawn at {MAX_PAIRS} pairs ({} games), or as soon as neither engine can \
             pass its bound by then",
            2 * MAX_PAIRS
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn duel_of(scores: &[f64]) -> Duel {
        let mut duel = Duel::new();
        for &score in scores {
            duel.add_game(score);
        }
        duel
    }

    /// `pairs` pairs, each the two games `pair` gives for its index.
    fn games_of(pairs: u64, pair: impl Fn(u64) -> [f64; 2]) -> Vec<f64> {
        (0..pairs).flat_map(pair).collect()
    }

    /// A small linear congruential generator: deterministic, and no dependency for the tests.
    fn generator(seed: u64) -> impl FnMut() -> f64 {
        let mut state = seed;
        move || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    #[test]
    fn the_threshold_matches_the_documented_values() {
        assert!((Duel::threshold(100, 0.5) - 0.6545).abs() < 1e-4);
        assert!((Duel::threshold(400, 0.22) - 0.5340).abs() < 1e-4);
    }

    /// A pair's games make one pair score, its half-points: 2-0 is 4, 1-1 and two draws are 2.
    #[test]
    fn two_games_make_one_pair() {
        let duel = duel_of(&[1.0, 1.0, 1.0, 0.0, 0.5, 0.5, 0.0]);
        assert_eq!(duel.pairs, Pairs { count: 3, sum: 8, squares: 24 });
        assert_eq!(duel.first, Some(0.0));
        assert_eq!(duel.games(), 7);
    }

    /// Every pair won 2-0: with the pseudo-pairs, `sigma_hat` is `1 / sqrt(P + 2)`, and the threshold
    /// falls below 1 once `P * (P + 2) > C^2 * 4`, at 6 pairs.
    #[test]
    fn no_engine_can_win_before_six_pairs() {
        assert_eq!(duel_of(&[1.0; 10]).outcome(), None, "5 pairs 2-0 are not enough");
        assert_eq!(duel_of(&[1.0; 12]).outcome(), Some(Outcome::ChallengerWins), "6 pairs 2-0 are the earliest win");
    }

    #[test]
    fn the_duel_is_symmetric() {
        assert_eq!(duel_of(&[0.0; 12]).outcome(), Some(Outcome::ChampionWins));
    }

    /// Without curtailment, the reference the tests compare against.
    fn uncurtailed() -> Duel {
        let mut duel = Duel::new();
        duel.curtails = false;
        duel
    }

    /// Pairs alternately 2-0 and 0-2: never past the threshold, so drawn at the cap.
    #[test]
    fn an_even_score_continues_until_the_cap_and_is_then_drawn() {
        let feed = |mut duel: Duel, pairs: u64| {
            for game in games_of(pairs, |pair| if pair % 2 == 0 { [1.0, 1.0] } else { [0.0, 0.0] }) {
                duel.add_game(game);
            }
            duel.outcome()
        };
        assert_eq!(feed(uncurtailed(), MAX_PAIRS - 1), None);
        assert_eq!(feed(uncurtailed(), MAX_PAIRS), Some(Outcome::Drawn));
    }

    /// Every pair 1-1: curtailed before the cap, said so, and not at the start.
    #[test]
    fn an_even_duel_is_curtailed_once_neither_can_pass_its_bound() {
        let even = |pairs: u64| games_of(pairs, |_| [1.0, 0.0]);
        assert_eq!(duel_of(&even(10)).outcome(), None);
        let first = (1..MAX_PAIRS).find(|&pairs| duel_of(&even(pairs)).outcome().is_some()).expect("curtailed before the cap");
        let duel = duel_of(&even(first));
        assert_eq!(duel.outcome(), Some(Outcome::Drawn));
        assert!(duel.status().contains(&format!("curtailed at {} pairs", progress::grouped(first))), "{}", duel.status());
    }

    /// Curtailment never changes a verdict, only when a draw is declared: seeded duels, a +40 Elo
    /// engine's and equal engines', on pairs of independent games, each played with and without it.
    #[test]
    fn curtailment_gives_the_verdict_the_cap_would() {
        let mut next = generator(0x9e37_79b9_7f4a_7c15);
        let play = |mut duel: Duel, games: &[f64]| -> (Outcome, u64) {
            for pair in games.chunks(2) {
                duel.add_game(pair[0]);
                duel.add_game(pair[1]);
                if let Some(outcome) = duel.outcome() {
                    return (outcome, duel.pairs.count);
                }
            }
            panic!("a duel always resolves by its cap");
        };
        let mut curtailed = 0;
        // +40 Elo with half the games drawn is 30.7% wins and 19.3% losses; equal is 25% and 25%.
        for win in [0.307, 0.25] {
            for _ in 0..200 {
                let games: Vec<f64> = (0..2 * MAX_PAIRS)
                    .map(|_| {
                        let roll = next();
                        if roll < win { 1.0 } else if roll < 0.5 { 0.0 } else { 0.5 }
                    })
                    .collect();
                let (with, at) = play(Duel::new(), &games);
                let (without, _) = play(uncurtailed(), &games);
                assert_eq!(with, without, "curtailment changed a verdict");
                curtailed += u32::from(with == Outcome::Drawn && at < MAX_PAIRS);
            }
        }
        assert!(curtailed > 0, "some drawn duel should have been curtailed");
    }

    /// A clear lead projects its crossing, near now; an even duel its curtailment, before the cap;
    /// no pairs, the cap.
    #[test]
    fn the_pairs_left_follow_the_score_not_the_cap() {
        let clear = duel_of(&games_of(20, |pair| if pair % 4 == 0 { [1.0, 0.0] } else { [1.0, 1.0] }));
        let left = clear.pairs_left_estimate().unwrap();
        assert!(left <= 5, "a clear lead resolves soon: {left} pairs");
        let even = duel_of(&games_of(100, |_| [1.0, 0.0]));
        let left = even.pairs_left_estimate().unwrap();
        assert!(left > 0 && left < MAX_PAIRS - 100, "an even duel is curtailed before the cap: {left} pairs");
        assert_eq!(Duel::new().pairs_left_estimate(), Some(MAX_PAIRS));
    }

    /// The pairs drawn from `split`, the shares of a pair's half-points 0 to 4, for equal engines:
    /// how many of `duels` end with a winner.
    fn false_winners(split: [f64; 5], duels: u32, seed: u64) -> u32 {
        let mut next = generator(seed);
        let mut winners = 0;
        for _ in 0..duels {
            let mut duel = uncurtailed();
            loop {
                let roll = next();
                let mut cumulative = 0.0;
                let q = split.iter().position(|share| {
                    cumulative += share;
                    roll < cumulative
                });
                let games = match q.unwrap_or(4) {
                    0 => [0.0, 0.0],
                    1 => [0.5, 0.0],
                    2 => [0.5, 0.5],
                    3 => [1.0, 0.5],
                    _ => [1.0, 1.0],
                };
                duel.add_game(games[0]);
                duel.add_game(games[1]);
                match duel.outcome() {
                    None => continue,
                    Some(Outcome::Drawn) => break,
                    Some(_) => {
                        winners += 1;
                        break;
                    }
                }
            }
        }
        winners
    }

    /// The reason `C` is 3.09 and not 1.645: two equal engines, checked after every pair up to the
    /// cap, must produce a false winner at most about 5% of the time. Seeded: independent games at
    /// 45% draws, and pairs that cancel as UHO's are meant to.
    #[test]
    fn two_equal_engines_rarely_produce_a_winner() {
        let win = 0.275;
        let independent = [win * win, 2.0 * win * 0.45, 0.45 * 0.45 + 2.0 * win * win, 2.0 * win * 0.45, win * win];
        for (name, split) in [("independent, 45% draws", independent), ("cancelling", [0.05, 0.20, 0.50, 0.20, 0.05])] {
            let duels = 2000;
            let rate = false_winners(split, duels, 0x2545_f491_4f6c_dd1d) as f64 / duels as f64;
            assert!(rate <= 0.05, "{name}: false-winner rate {rate}");
        }
    }

    /// The old rule's threshold (until 2026-10-31): games taken as independent, the deviation at its
    /// largest, `0.5 + C * 0.5 / sqrt(N)`, the cap 1,000 games. Kept for the comparison below.
    fn old_threshold(games: u64) -> f64 {
        0.5 + C * 0.5 / (games as f64).sqrt()
    }

    /// The false winners of this rule beside the old one's, between equal engines, over splits of the
    /// five pair results from friendly to the reinforcing worst case (duel-per-pair, 2026-10-31).
    /// Curtailment is left out: it never changes a verdict. Seeded; 4,000 duels a split.
    #[test]
    #[ignore = "a measurement, read by eye: cargo test -p arena --release false_winners_per_pair -- --ignored --nocapture"]
    fn false_winners_per_pair() {
        // Independent games with `draws` drawn, the rest won and lost evenly, as a pair split.
        let independent = |draws: f64| -> [f64; 5] {
            let win = (1.0 - draws) / 2.0;
            [win * win, 2.0 * win * draws, draws * draws + 2.0 * win * win, 2.0 * win * draws, win * win]
        };
        let splits: [(&str, [f64; 5]); 6] = [
            ("independent games, no draws", independent(0.0)),
            ("independent games, 45% draws", independent(0.45)),
            ("independent games, 70% draws", independent(0.70)),
            ("pairs cancelling (UHO-like)", [0.05, 0.20, 0.50, 0.20, 0.05]),
            ("pairs cancelling, many draws", [0.01, 0.12, 0.74, 0.12, 0.01]),
            ("pairs reinforcing (worst)", [0.40, 0.0, 0.20, 0.0, 0.40]),
        ];
        let duels = 4000;
        for (name, split) in splits {
            let proposed = false_winners(split, duels, 0x5851_f42d_4c95_7f2d);
            // The old rule on the same kind of pairs: a pair's half-points are its two games' points,
            // twice over.
            let mut next = generator(0x5851_f42d_4c95_7f2d);
            let mut old = 0;
            for _ in 0..duels {
                let mut points = 0.0;
                for pair in 1..=500u64 {
                    let roll = next();
                    let mut cumulative = 0.0;
                    let q = split.iter().position(|share| {
                        cumulative += share;
                        roll < cumulative
                    });
                    points += q.unwrap_or(4) as f64 / 2.0;
                    let games = 2 * pair;
                    let share = points / games as f64;
                    if share > old_threshold(games) || 1.0 - share > old_threshold(games) {
                        old += 1;
                        break;
                    }
                }
            }
            let rate = |count: u32| 100.0 * count as f64 / duels as f64;
            println!("{name:>30}: this rule {:5.2}% false winners, the old one {:5.2}%", rate(proposed), rate(old));
        }
    }
}
