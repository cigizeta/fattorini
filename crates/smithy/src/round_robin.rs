//! The ranking of the server's round robin among a generation's candidates: every two candidates
//! play the same number of colour-reversed pairs, and the candidates are ranked by score, their
//! points over their games.
//!
//! **Points, not a rating fit.** In a balanced round robin, every pairing the same number of pairs,
//! ranking by points gives the same order as a maximum-likelihood Elo fit (Bradley-Terry): at the
//! fit each player's points equal its expected points, which increase with its rating, so a fit can
//! only rescale the points, never reorder them. The balance is what makes this hold, which is why the
//! winner is named only once every pairing is complete.
//!
//! **Score, while it fills.** Until then the candidates have played unequal numbers of games, and
//! points would favour those that have played more; their score does not. Once every pairing is
//! complete every candidate has played as many games, and score and points give the same order.
//!
//! **Ties.** Equal scores are split by points, then by the points the tied candidates took off each
//! other, then by the earlier lambda in the grid.
//!
//! **Noise.** Each standing carries the standard error of its score fraction, from its own pairs:
//! a pair, one opening with colours swapped, is the unit (the pentanomial model), since its two games
//! are not independent. A reader sees which places are resolved and which are within noise. It does
//! not change the order.

/// One candidate's place in the round robin.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Standing {
    /// Its index in the grid.
    pub(crate) candidate: usize,
    pub(crate) points: f64,
    pub(crate) games: u64,
    /// Its points as a fraction of its games.
    pub(crate) score: f64,
    /// The standard error of `score`, from its own pairs.
    pub(crate) standard_error: f64,
}

/// The `count` candidates ranked by score, best first, from every pairing's pairs: pairing `k` is
/// `players[k]`, `(first, second)`, and `results[k]` its pairs so far, each `r` in -2..=2 the first
/// player's points minus one per game, so the first scores `(r + 2) / 2` of the pair's 2. Ties as the
/// module doc says.
pub(crate) fn standings(count: usize, players: &[(usize, usize)], results: &[Vec<i32>]) -> Vec<Standing> {
    // points[i][j]: what i scored against j.
    let mut points = vec![vec![0.0f64; count]; count];
    // Per candidate: each of its pairs' score fraction, 0 to 1.
    let mut pair_scores: Vec<Vec<f64>> = vec![Vec::new(); count];
    for (pairing, &(first, second)) in players.iter().enumerate() {
        for &r in results.get(pairing).map_or(&[][..], Vec::as_slice) {
            let first_points = f64::from(r + 2) / 2.0;
            points[first][second] += first_points;
            points[second][first] += 2.0 - first_points;
            pair_scores[first].push(first_points / 2.0);
            pair_scores[second].push(1.0 - first_points / 2.0);
        }
    }

    let total = |candidate: usize| -> f64 {
        points[candidate].iter().sum()
    };
    // Its points over its games, none played counting as 0.
    let fraction = |candidate: usize| -> f64 {
        let games = 2 * pair_scores[candidate].len();
        if games == 0 { 0.0 } else { total(candidate) / games as f64 }
    };
    // What `candidate` scored against the others with exactly its points.
    let head_to_head = |candidate: usize| -> f64 {
        let own = total(candidate);
        (0..count).filter(|&other| other != candidate && total(other) == own).map(|other| points[candidate][other]).sum()
    };

    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| {
        fraction(b)
            .total_cmp(&fraction(a))
            .then_with(|| total(b).total_cmp(&total(a)))
            .then_with(|| head_to_head(b).total_cmp(&head_to_head(a)))
            .then_with(|| a.cmp(&b))
    });
    order
        .into_iter()
        .map(|candidate| {
            let pairs = &pair_scores[candidate];
            let n = pairs.len() as f64;
            let score = if pairs.is_empty() { 0.0 } else { pairs.iter().sum::<f64>() / n };
            // The pair scores' variance, over the pairs' count.
            let standard_error = if pairs.is_empty() {
                0.0
            } else {
                let mean_square = pairs.iter().map(|s| s * s).sum::<f64>() / n;
                ((mean_square - score * score).max(0.0) / n).sqrt()
            };
            Standing { candidate, points: total(candidate), games: 2 * pairs.len() as u64, score, standard_error }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(standings: &[Standing]) -> Vec<usize> {
        standings.iter().map(|standing| standing.candidate).collect()
    }

    #[test]
    fn the_candidates_are_ranked_by_points() {
        // 2 beats 0 and 1; 1 beats 0.
        let players = [(0, 1), (0, 2), (1, 2)];
        let results = vec![vec![-2, -1, 0], vec![-2, -2, 1], vec![-1, 0, 0]];
        let standings = standings(3, &players, &results);
        assert_eq!(order(&standings), vec![2, 1, 0]);
        assert_eq!(standings.iter().map(|standing| standing.games).collect::<Vec<_>>(), vec![12, 12, 12]);
        let points: f64 = standings.iter().map(|standing| standing.points).sum();
        assert_eq!(points, 18.0, "every game hands out one point");
    }

    #[test]
    fn equal_points_go_to_the_head_to_head_then_to_the_earlier_lambda() {
        // 0 and 2 both score 5 of 8; 2 took 2.5 of 4 off 0.
        let players = [(0, 1), (0, 2), (1, 2)];
        let results = vec![vec![2, 1], vec![-1, 0], vec![0, -1]];
        let standings = standings(3, &players, &results);
        assert_eq!(standings[0].points, standings[1].points);
        assert_eq!(order(&standings), vec![2, 0, 1]);

        let level = [vec![0, 0]];
        assert_eq!(order(&super::standings(2, &[(0, 1)], &level)), vec![0, 1], "level everywhere: the earlier lambda");
    }

    #[test]
    fn while_it_fills_the_score_ranks_not_the_points() {
        // 0 has played 4 pairs at 50%, 4 points; 2 one pair at 75%, 1.5 points.
        let players = [(0, 1), (0, 2), (1, 2)];
        let results = vec![vec![0, 0, 0, 0], vec![], vec![-1]];
        let standings = standings(3, &players, &results);
        assert_eq!(order(&standings), vec![2, 0, 1]);
        assert!(standings[1].points > standings[0].points, "fewer points, ranked higher");
    }

    #[test]
    fn the_standard_error_is_over_pairs() {
        // Every pair won or lost outright at a 0.5 score: per-pair standard deviation 0.5, over 100 pairs.
        let decisive: Vec<i32> = (0..100).map(|pair| if pair % 2 == 0 { 2 } else { -2 }).collect();
        let standings_decisive = standings(2, &[(0, 1)], &[decisive]);
        assert!((standings_decisive[0].standard_error - 0.05).abs() < 1e-12);
        // Every pair split, a win each or two draws alike: no spread, though the games were decisive.
        let split = standings(2, &[(0, 1)], &[vec![0; 100]]);
        assert_eq!(split[0].standard_error, 0.0);
        assert_eq!(standings(2, &[(0, 1)], &[vec![]])[0].games, 0, "nothing played yet");
    }
}
