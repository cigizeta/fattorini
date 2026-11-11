//! A best of n, played in colour-reversed pairs: the engine past n/2 wins, and n/2 each after n
//! games is a drawn match, with no tie-break. n is always even, since results arrive in pairs. A lead
//! the other side can no longer catch ends the match early, at the end of the pair that made it,
//! unless the match is full: then all n games are played, so its score is always over n (the
//! release's strength table compares scores over the same number of games).

use super::runner::{Outcome, SequentialTest, Tally};

pub struct BestOf {
    n: u32,
    full: bool,
    tally: Tally,
}

impl BestOf {
    pub fn new(n: u32, full: bool) -> BestOf {
        assert!(n >= 2 && n.is_multiple_of(2), "a best of n needs an even n of at least 2, got {n}");
        BestOf { n, full, tally: Tally::new() }
    }
}

impl SequentialTest for BestOf {
    fn add_game(&mut self, challenger_score: f64) {
        self.tally.add_game(challenger_score);
    }

    fn outcome(&self) -> Option<Outcome> {
        if self.full && self.tally.games() < self.n as u64 {
            return None;
        }
        let half = self.n as f64 / 2.0;
        let challenger = self.tally.score();
        let champion = self.tally.games() as f64 - challenger;
        if challenger > half {
            return Some(Outcome::ChallengerWins);
        }
        if champion > half {
            return Some(Outcome::ChampionWins);
        }
        if self.tally.games() >= self.n as u64 {
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

    /// Nothing: `runner::summary` already prints both sides' points, and `describe` states the target
    /// once at the start; repeating it on every line read as a count of games.
    fn status(&self) -> String {
        String::new()
    }

    fn describe(&self) -> String {
        let half = self.n / 2;
        if self.full {
            return format!(
                "best of {} games, all played, a win 1 point and a draw half: the side with more than {half} points wins the match, {half} - {half} draws it",
                self.n
            );
        }
        format!(
            "best of {} games, a win 1 point and a draw half: the first side with more than {half} points wins the match, {half} - {half} after all {} games draws it",
            self.n, self.n
        )
    }

    /// All n games: the most a best of n can take.
    fn most_pairs(&self) -> Option<u64> {
        Some(self.n as u64 / 2)
    }

    /// The pairs left if every remaining game is drawn: the leader then passes n/2 after the fewest
    /// games k with `leader + k/2 > n/2`, and a level match runs to n. Not a bound either way (the
    /// trailer may win games back, the leader may win more), but centred on what happens, where all
    /// n games is the worst case only.
    fn pairs_left_estimate(&self) -> Option<u64> {
        let left = self.n as u64 - self.tally.games().min(self.n as u64);
        if self.full {
            return Some(left.div_ceil(2));
        }
        let challenger = self.tally.score();
        let leader = challenger.max(self.tally.games() as f64 - challenger);
        let half = self.n as f64 / 2.0;
        if leader > half {
            return Some(0);
        }
        // Half-points are exact in f64, so the floor is exact too.
        let games = ((2.0 * (half - leader)).floor() as u64 + 1).min(left);
        Some(games.div_ceil(2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn best_of(n: u32, scores: &[f64]) -> BestOf {
        let mut match_ = BestOf::new(n, false);
        for &score in scores {
            match_.add_game(score);
        }
        match_
    }

    #[test]
    fn the_first_past_half_wins() {
        assert_eq!(best_of(12, &[1.0; 6]).outcome(), None, "6 of 12 is not past half");
        assert_eq!(best_of(12, &[1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 0.5]).outcome(), Some(Outcome::ChallengerWins));
        assert_eq!(best_of(12, &[0.0; 8]).outcome(), Some(Outcome::ChampionWins));
    }

    #[test]
    fn a_level_match_after_n_games_is_drawn() {
        assert_eq!(best_of(4, &[1.0, 0.0, 0.5, 0.5]).outcome(), Some(Outcome::Drawn));
    }

    #[test]
    fn a_best_of_n_takes_at_most_n_over_2_pairs() {
        assert_eq!(BestOf::new(400, false).most_pairs(), Some(200));
    }

    /// The challenger out of reach after three games of four, yet a full match plays the fourth.
    #[test]
    fn a_full_match_plays_all_n_games() {
        let mut match_ = BestOf::new(4, true);
        for score in [1.0, 1.0, 1.0] {
            match_.add_game(score);
        }
        assert_eq!(match_.outcome(), None);
        assert_eq!(match_.pairs_left_estimate(), Some(1));
        match_.add_game(0.0);
        assert_eq!(match_.outcome(), Some(Outcome::ChallengerWins));
        assert_eq!(match_.pairs_left_estimate(), Some(0));
    }

    #[test]
    fn a_level_full_match_is_drawn() {
        let mut match_ = BestOf::new(4, true);
        for score in [1.0, 0.0, 0.5, 0.5] {
            match_.add_game(score);
        }
        assert_eq!(match_.outcome(), Some(Outcome::Drawn));
    }

    /// The leader needs more than n/2 at half a point a game.
    #[test]
    fn the_pairs_left_assume_every_remaining_game_drawn() {
        assert_eq!(BestOf::new(400, false).pairs_left_estimate(), Some(200), "level: all of them");
        // 36.0 - 32.0 after 68: 36 + k/2 > 200 first at k = 329 games, 165 pairs, within the 166 left.
        let mut scores = vec![1.0; 20];
        scores.extend(vec![0.0; 16]);
        scores.extend(vec![0.5; 32]);
        assert_eq!(best_of(400, &scores).pairs_left_estimate(), Some(165));
        // 34.0 - 34.0 after 68: level, the 166 pairs left.
        assert_eq!(best_of(400, &[0.5; 68]).pairs_left_estimate(), Some(166));
    }

    #[test]
    #[should_panic(expected = "even n")]
    fn an_odd_n_is_refused() {
        BestOf::new(7, false);
    }
}
