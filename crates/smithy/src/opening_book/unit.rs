//! Scores and the book's unit. An engine reports centipawns or a mate; the book measures a move's
//! loss in expected-score points by the Elo formula, `100 / (1 + 10^(-cp / 400))`, the convention
//! Fattorini's networks are trained in (`fattorini::labels::S_PAWNS`): 400 centipawns mean the
//! odds of a 400-point rating gap, ten to one. A loss of a pawn's worth costs about 10 points in an
//! even position and less once the game is decided, so decided positions need no cutoff of their
//! own.

/// A score as an engine reports it, from one side's point of view: centipawns, or a mate in N
/// moves (N > 0: this side mates; N <= 0: this side is mated).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Score {
    Cp(i32),
    Mate(i32),
}

impl Score {
    /// The same score from the other side's point of view.
    pub fn negated(self) -> Score {
        match self {
            Score::Cp(cp) => Score::Cp(-cp),
            Score::Mate(n) => Score::Mate(-n),
        }
    }

    /// The expected score in points, 0 to 100, of the side the score is from: a mate gives 100,
    /// being mated 0.
    pub fn points(self) -> f64 {
        match self {
            Score::Cp(cp) => 100.0 / (1.0 + 10f64.powf(-cp as f64 / 400.0)),
            Score::Mate(n) if n > 0 => 100.0,
            Score::Mate(_) => 0.0,
        }
    }
}

/// A move's loss in points: how far its score falls short of the best, both from the mover's side.
/// Never negative: a later search may score a move a hair above the first search's best, and that
/// move then loses nothing.
pub fn loss(best: Score, score: Score) -> f64 {
    (best.points() - score.points()).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_elo_formula_gives_the_points() {
        assert_eq!(Score::Cp(0).points(), 50.0);
        assert!((Score::Cp(400).points() - 100.0 / 1.1).abs() < 1e-9, "ten to one");
        assert!((Score::Cp(-400).points() - 100.0 / 11.0).abs() < 1e-9);
    }

    #[test]
    fn a_mate_is_all_or_nothing() {
        assert_eq!(Score::Mate(3).points(), 100.0);
        assert_eq!(Score::Mate(-3).points(), 0.0);
        assert_eq!(Score::Mate(0).points(), 0.0, "mated now");
        assert_eq!(Score::Mate(2).negated(), Score::Mate(-2));
    }

    #[test]
    fn seventy_centipawns_below_equality_lose_about_ten_points() {
        let lost = loss(Score::Cp(0), Score::Cp(-70));
        assert!((lost - 10.0).abs() < 0.2, "{lost}");
    }

    #[test]
    fn a_decided_position_loses_less_for_the_same_drop() {
        assert!(loss(Score::Cp(600), Score::Cp(530)) < loss(Score::Cp(0), Score::Cp(-70)) / 2.0);
    }

    #[test]
    fn a_loss_is_never_negative() {
        assert_eq!(loss(Score::Cp(20), Score::Cp(25)), 0.0);
    }
}
