//! Where an NNUE run is in its list of learning rates, replayed from its recorded test losses.
//!
//! The rule: after `warmup_rounds` of warmup, train at the first rate; each time `patience_rounds`
//! rounds pass without a new best test loss, move abruptly to the next rate; when the last rate runs
//! out of patience, the run is finished. Patience counts from the latest of the last new best, the
//! last switch and the end of warmup, so warmup never counts against it and every rate gets its full
//! `patience_rounds`. The best test loss is tracked over the whole run, warmup included.
//!
//! A run's first round is at step 0: its starting weights, measured before any training. At every
//! switch the run rolls back to its best; when that best is still step 0, the optimizer starts fresh
//! again, so a new warmup follows the switch and patience counts from its end.
//!
//! A pure function of the losses rather than a state file: the same losses always give the same
//! switches, so a resumed run lands exactly where the interrupted one was, and there is no second
//! record that could disagree with `history.tsv`.

/// The outcome of replaying a run's rounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stages {
    /// The index, in the list of rates, of the rate the next round trains at, or the last one's when
    /// `finished`.
    pub stage: usize,
    /// Whether the last rate has run out of patience: the run is over.
    pub finished: bool,
    /// The step of the lowest test loss so far, `0` before any round.
    pub best_step: u64,
    /// The lowest test loss so far, infinite before any round. A `NaN` loss is never a best.
    pub best_loss: f64,
    /// The step patience counts from: the latest of `best_step`, the last switch and the end of warmup.
    pub patience_from: u64,
    /// The step of the latest switch, `0` before any, and the rounds of patience the rate before it
    /// had used up.
    pub switch_step: u64,
    pub rounds_used_at_switch: u64,
    /// The end of the latest warmup: the first one's, or the one a switch back to the starting weights
    /// begins (a best still at step 0 when patience runs out).
    pub warmup_end: u64,
    /// The step of the last round read, `0` before any.
    pub last_step: u64,
}

impl Stages {
    /// Rounds since `patience_from` as of `step`: how much patience the current rate has used. `0`
    /// during warmup. At the round of a switch, the patience the previous rate used up, all of it:
    /// that round is the one that ran out, so it reads `5/5`, not `0/5`; the next round counts from
    /// the switch.
    pub fn rounds_without_improvement(&self, step: u64, steps_per_round: u64) -> u64 {
        if self.switch_step > 0 && step == self.switch_step {
            return self.rounds_used_at_switch;
        }
        step.saturating_sub(self.patience_from) / steps_per_round
    }
}

/// Replays the rule over `rounds`, each round's `(step, test_loss)` in step order, for a list of
/// `stage_count` rates. `rounds` stops being read at the round that finishes the run.
pub fn stage_after(
    rounds: &[(u64, f64)], steps_per_round: u64, warmup_rounds: u64, patience_rounds: u64, stage_count: usize,
) -> Stages {
    assert!(stage_count >= 1, "a run needs at least one rate");
    let mut stages = Stages::start(steps_per_round, warmup_rounds);
    for &(step, test_loss) in rounds {
        stages.read(step, test_loss, steps_per_round, warmup_rounds, patience_rounds, stage_count);
        if stages.finished {
            break;
        }
    }
    stages
}

impl Stages {
    /// Before any round.
    fn start(steps_per_round: u64, warmup_rounds: u64) -> Stages {
        let warmup_steps = warmup_rounds * steps_per_round;
        Stages {
            stage: 0,
            finished: false,
            best_step: 0,
            best_loss: f64::INFINITY,
            patience_from: warmup_steps,
            switch_step: 0,
            rounds_used_at_switch: 0,
            warmup_end: warmup_steps,
            last_step: 0,
        }
    }

    /// Reads one round, the rule of the module's comment; sets `finished` when the last rate runs out.
    fn read(
        &mut self, step: u64, test_loss: f64, steps_per_round: u64, warmup_rounds: u64, patience_rounds: u64,
        stage_count: usize,
    ) {
        let warmup_steps = warmup_rounds * steps_per_round;
        let patience_steps = patience_rounds * steps_per_round;
        self.last_step = step;
        if test_loss < self.best_loss {
            self.best_loss = test_loss;
            self.best_step = step;
        }
        self.patience_from = self.best_step.max(self.switch_step).max(self.warmup_end);
        if step >= self.patience_from + patience_steps {
            if self.stage + 1 == stage_count {
                self.finished = true;
                return;
            }
            self.stage += 1;
            self.rounds_used_at_switch = (step - self.patience_from) / steps_per_round;
            self.switch_step = step;
            // A switch rolls the run back to its best. Back to step 0, the starting weights, the
            // optimizer is fresh again and warms up again, so the next rate gets its warmup rounds
            // before its patience counts, as the first rate did.
            if self.best_step == 0 {
                self.warmup_end = step + warmup_steps;
            }
            self.patience_from = step.max(self.warmup_end);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Rounds of 10 steps, 2 warmup rounds (warmup ends at step 20), patience of 3 rounds (30 steps).
    const ROUND: u64 = 10;
    const WARMUP: u64 = 2;
    const PATIENCE: u64 = 3;

    /// One round per loss, at steps 10, 20, 30, ...
    fn rounds(losses: &[f64]) -> Vec<(u64, f64)> {
        losses.iter().enumerate().map(|(index, &loss)| ((index as u64 + 1) * ROUND, loss)).collect()
    }

    fn replay(losses: &[f64], stage_count: usize) -> Stages {
        stage_after(&rounds(losses), ROUND, WARMUP, PATIENCE, stage_count)
    }

    #[test]
    fn no_rounds_is_the_first_rate() {
        let stages = replay(&[], 3);
        assert_eq!((stages.stage, stages.finished, stages.best_step), (0, false, 0));
        assert_eq!(stages.best_loss, f64::INFINITY);
    }

    #[test]
    fn warmup_rounds_never_count_against_patience() {
        // The best is at step 10, in warmup; patience counts from step 20, so rounds at 30, 40 are
        // not enough and the switch comes at 50.
        assert_eq!(replay(&[0.5, 0.6, 0.6, 0.6], 3).stage, 0);
        let stages = replay(&[0.5, 0.6, 0.6, 0.6, 0.6], 3);
        assert_eq!(stages.stage, 1);
        assert_eq!(stages.best_step, 10, "the warmup round's loss is still the best");
    }

    #[test]
    fn a_rate_moves_on_after_patience_rounds_without_a_new_best() {
        // Improving through step 40, then flat: 50, 60, 70 are three rounds without a best.
        assert_eq!(replay(&[0.9, 0.8, 0.7, 0.6, 0.6, 0.6], 3).stage, 0);
        let stages = replay(&[0.9, 0.8, 0.7, 0.6, 0.6, 0.6, 0.6], 3);
        assert_eq!((stages.stage, stages.patience_from), (1, 70));
    }

    #[test]
    fn patience_restarts_at_a_switch() {
        // Switch at 70 (best at 40). The next rate gets its own three rounds: 80, 90 are not enough,
        // 100 is.
        let flat_after_40 = [0.9, 0.8, 0.7, 0.6, 0.6, 0.6, 0.6, 0.6, 0.6];
        assert_eq!(replay(&flat_after_40, 3).stage, 1);
        let stages = replay(&[&flat_after_40[..], &[0.6]].concat(), 3);
        assert_eq!(stages.stage, 2);
    }

    #[test]
    fn a_new_best_restarts_patience() {
        // Switch at 70; a new best at 90 on the second rate pushes its patience to 120.
        let losses = [0.9, 0.8, 0.7, 0.6, 0.6, 0.6, 0.6, 0.6, 0.5, 0.5, 0.5];
        let stages = replay(&losses, 3);
        assert_eq!((stages.stage, stages.best_step, stages.patience_from), (1, 90, 90));
        assert_eq!(replay(&[&losses[..], &[0.5]].concat(), 3).stage, 2);
    }

    #[test]
    fn the_last_rate_running_out_of_patience_finishes_the_run() {
        let flat = [0.5; 12];
        // One rate: finished at step 50, the first moment patience runs out after warmup.
        let stages = replay(&flat, 1);
        assert!(stages.finished);
        assert_eq!(stages.stage, 0);
        assert!(!replay(&flat[..4], 1).finished);
        // Two rates: switch at 50, finished at 80; the rounds after it are never read.
        let stages = replay(&flat, 2);
        assert_eq!((stages.stage, stages.finished), (1, true));
        assert_eq!(stages.patience_from, 50, "the finishing round is not a switch");
    }

    #[test]
    fn a_nan_loss_is_never_a_best() {
        let stages = replay(&[0.5, f64::NAN], 3);
        assert_eq!((stages.best_step, stages.best_loss), (10, 0.5));
    }

    #[test]
    fn rounds_without_improvement_is_zero_in_warmup_and_counts_after() {
        let stages = replay(&[0.9, 0.8, 0.7, 0.7], 3);
        assert_eq!(stages.patience_from, 30);
        assert_eq!(stages.rounds_without_improvement(40, ROUND), 1);
        assert_eq!(replay(&[0.9], 3).rounds_without_improvement(10, ROUND), 0, "warmup");
    }

    #[test]
    fn the_round_that_runs_out_of_patience_reads_full_and_the_next_counts_from_the_switch() {
        // Best at 40, switch at 70: that round has used all three rounds, the next one, one.
        let to_switch = [0.9, 0.8, 0.7, 0.6, 0.6, 0.6, 0.6];
        let stages = replay(&to_switch, 3);
        assert_eq!(stages.switch_step, 70);
        assert_eq!(stages.rounds_without_improvement(70, ROUND), PATIENCE);
        let stages = replay(&[&to_switch[..], &[0.6]].concat(), 3);
        assert_eq!(stages.rounds_without_improvement(80, ROUND), 1);
    }

    /// Round 0 at step 0, the starting weights, then one round per loss at steps 10, 20, 30, ...
    fn replay_from_zero(start: f64, losses: &[f64], stage_count: usize) -> Stages {
        let all = [&[(0, start)][..], &rounds(losses)].concat();
        stage_after(&all, ROUND, WARMUP, PATIENCE, stage_count)
    }

    #[test]
    fn a_switch_back_to_the_starting_weights_warms_up_again_before_patience_counts() {
        // Nothing beats step 0. Warmup to 20, patience to 50: switch at 50, back to step 0, so a second
        // warmup to 70; patience then counts from 70, and the next switch comes at 100, not 80.
        let worse = [0.6; 10];
        let stages = replay_from_zero(0.5, &worse[..5], 3);
        assert_eq!((stages.stage, stages.best_step, stages.switch_step), (1, 0, 50));
        assert_eq!((stages.warmup_end, stages.patience_from), (70, 70));
        assert_eq!(stages.rounds_without_improvement(60, ROUND), 0, "the second warmup does not count");
        assert_eq!(replay_from_zero(0.5, &worse[..9], 3).stage, 1);
        let stages = replay_from_zero(0.5, &worse, 3);
        assert_eq!((stages.stage, stages.switch_step, stages.warmup_end), (2, 100, 120));
    }

    #[test]
    fn a_switch_back_to_a_later_best_has_no_second_warmup() {
        // Best at 10, switch at 50: the best's optimizer comes back past warmup, patience counts from 50.
        let stages = replay_from_zero(0.9, &[0.5, 0.6, 0.6, 0.6, 0.6], 3);
        assert_eq!((stages.stage, stages.best_step, stages.switch_step), (1, 10, 50));
        assert_eq!((stages.warmup_end, stages.patience_from), (20, 50));
    }

    #[test]
    fn last_step_says_whether_the_last_round_read_was_a_switch() {
        let stages = replay_from_zero(0.5, &[0.6; 5], 3);
        assert_eq!((stages.switch_step, stages.last_step), (50, 50));
        let stages = replay_from_zero(0.5, &[0.6; 6], 3);
        assert_eq!((stages.switch_step, stages.last_step), (50, 60));
    }

    #[test]
    fn a_tie_in_loss_keeps_the_older_best() {
        // Pruning keeps `best_step` alone, so a tie must name one step, the first to reach the loss.
        let stages = replay(&[0.9, 0.5, 0.5], 3);
        assert_eq!(stages.best_step, 20);
    }

    #[test]
    fn the_round_that_finishes_the_run_reads_full() {
        let flat = [0.5; 5];
        let stages = replay(&flat, 1);
        assert!(stages.finished);
        assert_eq!(stages.rounds_without_improvement(50, ROUND), PATIENCE);
    }
}
