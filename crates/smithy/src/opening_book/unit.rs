//! Scores and the book's unit. The book is Stockfish's, so its scores are read in Stockfish's own
//! calibration: with `UCI_ShowWDL` on, each search reports its win, draw and loss chances in
//! thousandths, fitted by Stockfish to its own games (its model also weighs the material on the
//! board), and a score's expected value is `win + draw / 2` thousandths. A move's loss is the drop
//! in expected score against the best move, in points of 0 to 100.

/// A score as an engine reports it, from one side's point of view: centipawns, or a mate in N
/// moves (N > 0: this side mates; N <= 0: this side is mated).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Score {
    Cp(i32),
    Mate(i32),
}

impl Score {
    pub fn negated(self) -> Score {
        match self {
            Score::Cp(cp) => Score::Cp(-cp),
            Score::Mate(n) => Score::Mate(-n),
        }
    }

    /// A mate against this side.
    pub fn is_mated(self) -> bool {
        matches!(self, Score::Mate(n) if n <= 0)
    }
}

/// The expected score's whole: it is kept in half-thousandths, `2 win + draw`, so an odd number
/// of draws loses nothing and turning to the other side, `WHOLE - expected`, is exact.
pub const WHOLE: u16 = 2000;

/// A score with its expected value, in half-thousandths (0 to `WHOLE`), from the same side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Judged {
    pub score: Score,
    pub expected: u16,
}

impl Judged {
    /// From `wdl` thousandths, as the engine reports them.
    pub fn from_wdl(score: Score, win: u16, draw: u16) -> Judged {
        Judged { score, expected: 2 * win + draw }
    }

    pub fn negated(self) -> Judged {
        Judged { score: self.score.negated(), expected: WHOLE - self.expected }
    }

    /// The expected score in points, 0 to 100.
    pub fn points(self) -> f64 {
        self.expected as f64 / 20.0
    }
}

/// A move's loss in points against the best, both from the mover's side. Never negative: a later
/// search may score a move a hair above the first search's best. A move into a forced mate loses
/// infinitely, unless the best move is one too: it is never admitted while anything else is.
pub fn loss(best: Judged, other: Judged) -> f64 {
    if other.score.is_mated() && !best.score.is_mated() {
        return f64::INFINITY;
    }
    (best.points() - other.points()).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn judged(cp: i32, win: u16, draw: u16) -> Judged {
        Judged::from_wdl(Score::Cp(cp), win, draw)
    }

    #[test]
    fn the_expected_score_is_win_and_half_the_draws() {
        assert_eq!(judged(100, 500, 500).points(), 75.0);
        assert_eq!(judged(0, 0, 1000).points(), 50.0);
        assert_eq!(judged(0, 0, 1000).negated().points(), 50.0);
        assert_eq!(judged(100, 500, 500).negated().points(), 25.0);
        assert_eq!(judged(10, 101, 899).negated().negated(), judged(10, 101, 899), "exact both ways");
    }

    #[test]
    fn a_loss_is_the_drop_in_points_and_never_negative() {
        // 54 points against 45.
        assert_eq!(loss(judged(30, 100, 880), judged(-40, 20, 860)), 9.0);
        assert_eq!(loss(judged(20, 60, 900), judged(25, 70, 900)), 0.0);
    }

    #[test]
    fn a_move_into_a_forced_mate_is_never_admitted_while_another_is_not() {
        let lost = Judged::from_wdl(Score::Cp(-708), 0, 30);
        let mated = Judged::from_wdl(Score::Mate(-1), 0, 0);
        assert_eq!(loss(lost, mated), f64::INFINITY);
        let also_mated = Judged::from_wdl(Score::Mate(-3), 0, 0);
        assert_eq!(loss(also_mated, mated), 0.0, "every move is mated: no infinite loss");
    }
}
