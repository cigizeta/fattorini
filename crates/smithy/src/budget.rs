//! A fixed budget and a falling schedule: `training.schedule: budget` in a network's config.
//!
//! The run trains for `epochs` passes over its training set, in whole rounds; its rate ramps up
//! through the warmup as under patience, then falls geometrically with the step from the config's
//! first rate to its second, reached at the last step. Nothing is replayed from the losses: the rate
//! is a function of the run step alone, so a resumed run needs nothing but its step count. No stage,
//! no rollback, no lowest-loss candidate: the checkpoints at every quarter epoch from half the budget
//! to its end are kept, and a round robin among them chooses the candidate. Its pros and cons against
//! patience: a run's length is known at its start, and the noise of one round's test loss no longer
//! decides when the rate moves or which checkpoint wins.

/// A budgeted run's figures, all in run steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    /// The last step: the budget in whole rounds.
    pub total_steps: u64,
    pub warmup_steps: u64,
    /// Steps in one pass over the training set: `train_rows / training_batch_size`.
    pub steps_per_epoch: u64,
    pub steps_per_round: u64,
    pub epochs: f64,
    /// The rate at the warmup's end, and at the last step.
    pub start: f64,
    pub end: f64,
}

impl Budget {
    /// The budget of a run over `train_rows` training rows. Panics where `checked` refuses: training
    /// cannot start on such a budget.
    pub fn new(
        train_rows: u64, training_batch_size: u64, steps_per_round: u64, warmup_rounds: u64, epochs: f64, rates: &[f64],
    ) -> Budget {
        Budget::checked(train_rows, training_batch_size, steps_per_round, warmup_rounds, epochs, rates)
            .unwrap_or_else(|message| panic!("{message}"))
    }

    /// `new` as an error, for a reader that must not panic (the status page): a figure
    /// `net_config::read` refuses, a training set smaller than one batch, or a warmup as long as the
    /// budget.
    pub fn checked(
        train_rows: u64, training_batch_size: u64, steps_per_round: u64, warmup_rounds: u64, epochs: f64, rates: &[f64],
    ) -> Result<Budget, String> {
        if training_batch_size == 0 || steps_per_round == 0 {
            return Err("a batch and a round hold at least one step".to_string());
        }
        if !(epochs.is_finite() && epochs > 0.0) {
            return Err(format!("a budget of {epochs} epochs is meaningless"));
        }
        let [start, end] = rates else { return Err(format!("a budget falls from one rate to another, not through {rates:?}")) };
        if !(*end < *start && *end > 0.0) {
            return Err(format!("a budget's rate falls, from {start:e} to {end:e}"));
        }
        let steps_per_epoch = train_rows / training_batch_size;
        if steps_per_epoch == 0 {
            return Err(format!("{train_rows} training rows hold no batch of {training_batch_size}"));
        }
        let total_steps = whole_rounds(epochs * steps_per_epoch as f64, steps_per_round);
        let warmup_steps = warmup_rounds * steps_per_round;
        if warmup_steps >= total_steps {
            return Err(format!("a warmup of {warmup_rounds} rounds leaves nothing of the budget's {total_steps} steps"));
        }
        Ok(Budget { total_steps, warmup_steps, steps_per_epoch, steps_per_round, epochs, start: *start, end: *end })
    }

    pub fn total_rounds(&self) -> u64 {
        self.total_steps / self.steps_per_round
    }

    /// Every candidate's round, whole since a candidate is taken at a round end. The page's loss chart
    /// draws a line at each, the last included, so its lines match the round robin's table one to one.
    pub fn candidate_rounds(&self) -> Vec<u64> {
        self.candidate_steps().into_iter().map(|step| step / self.steps_per_round).collect()
    }

    /// The rate step `step` trains at, the first update being step 1: the warmup's ramp to `start`,
    /// then geometric to `end` at `total_steps`. `StackedAdamW` computes the same on the device.
    pub fn rate(&self, step: u64) -> f64 {
        let warmup = if self.warmup_steps == 0 { 1.0 } else { (step as f64 / self.warmup_steps as f64).min(1.0) };
        warmup * self.decayed(step)
    }

    /// The geometric part alone: `start` up to the warmup's end, `end` from the last step on.
    fn decayed(&self, step: u64) -> f64 {
        let span = (self.total_steps - self.warmup_steps) as f64;
        let progress = ((step as f64 - self.warmup_steps as f64) / span).clamp(0.0, 1.0);
        self.start * (self.end / self.start).powf(progress)
    }

    /// The candidates, `(epoch, step)` in order: nine, every sixteenth of the budget from its half to
    /// its end (for 4 epochs, every quarter epoch from 2 to 4), each at the first round end at or past
    /// it (checkpoints exist at round ends only), the last at `total_steps`. Fractions of the budget,
    /// not whole quarter epochs, so a scaled run's budget, a fraction of an epoch, still has nine. Two
    /// landing on one round end (a tiny run) give one candidate, the earlier's.
    pub fn candidates(&self) -> Vec<(f64, u64)> {
        let mut candidates: Vec<(f64, u64)> = Vec::new();
        for sixteenth in 8..=16u32 {
            let epoch = self.epochs * f64::from(sixteenth) / 16.0;
            let step = whole_rounds(epoch * self.steps_per_epoch as f64, self.steps_per_round).min(self.total_steps);
            if candidates.last().is_none_or(|&(_, last)| last < step) {
                candidates.push((epoch, step));
            }
        }
        candidates
    }

    /// Where a budgeted run whose rounds are `rounds`, `(step, test_loss)` in step order, stands:
    /// finished once a round reaches `total_steps`, read no further. Its best for the page alone: a
    /// budget's candidate is chosen by its round robin, never by its loss.
    pub fn standing(&self, rounds: &[(u64, f64)]) -> BudgetStanding {
        let mut standing = BudgetStanding { last_step: 0, finished: false, best_step: 0, best_loss: f64::INFINITY };
        for &(step, test_loss) in rounds {
            standing.last_step = step;
            if test_loss < standing.best_loss {
                standing.best_loss = test_loss;
                standing.best_step = step;
            }
            if step >= self.total_steps {
                standing.finished = true;
                break;
            }
        }
        standing
    }

    /// Every candidate's step.
    pub fn candidate_steps(&self) -> Vec<u64> {
        self.candidates().into_iter().map(|(_, step)| step).collect()
    }
}

/// Where a budgeted run stands (`Budget::standing`): no stage and no patience, only how far it has
/// gone, and its lowest test loss so far.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BudgetStanding {
    /// The step of the last round read, `0` before any.
    pub last_step: u64,
    pub finished: bool,
    /// The step of the lowest test loss so far, `0` before any round, and that loss, infinite before
    /// any. A `NaN` loss is never a best.
    pub best_step: u64,
    pub best_loss: f64,
}

/// `steps` rounded up to whole rounds of `steps_per_round`.
fn whole_rounds(steps: f64, steps_per_round: u64) -> u64 {
    (steps / steps_per_round as f64).ceil() as u64 * steps_per_round
}

/// An epoch as a candidate's name spells it, `2.000`, `2.250`, ...: three decimals, so a scaled run's
/// candidates, a fraction of an epoch apart, never share one.
pub fn epoch_label(epoch: f64) -> String {
    format!("{epoch:.3}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// About `lc0-halfk-threats-screlu-hidden`'s figures: 1.2G rows, batches of 4096, rounds of 4096.
    fn full() -> Budget {
        Budget::new(1_200_000_000, 4096, 4096, 1, 4.0, &[1.0e-3, 1.0e-5])
    }

    #[test]
    fn the_budget_is_whole_rounds_covering_every_epoch() {
        let budget = full();
        assert_eq!(budget.steps_per_epoch, 292_968);
        assert_eq!(budget.total_steps % 4096, 0);
        assert!(budget.total_steps >= 4 * 292_968 && budget.total_steps - 4 * 292_968 < 4096);
        assert_eq!(budget.total_rounds(), 287);
    }

    #[test]
    fn the_candidates_rounds_are_nine_the_last_at_the_end() {
        let budget = full();
        let rounds = budget.candidate_rounds();
        assert_eq!(rounds.len(), 9);
        assert_eq!(rounds.first(), Some(&144), "half of four epochs of 292,968 steps, at the next round end");
        assert_eq!(rounds.last(), Some(&budget.total_rounds()));
        assert!(rounds.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn the_rate_ramps_then_falls_geometrically_to_the_end() {
        let budget = full();
        assert_eq!(budget.rate(0), 0.0);
        assert!((budget.rate(2048) - 0.5e-3).abs() < 1e-12, "halfway through the warmup, half the start");
        assert!((budget.rate(budget.warmup_steps) - 1.0e-3).abs() < 1e-12, "the start at the warmup's end");
        assert!((budget.rate(budget.total_steps) - 1.0e-5).abs() < 1e-15, "the end at the last step");
        let halfway = budget.warmup_steps + (budget.total_steps - budget.warmup_steps) / 2;
        assert!((budget.rate(halfway) - 1.0e-4).abs() < 1e-9, "the geometric mean halfway: {}", budget.rate(halfway));
        assert!((budget.rate(budget.total_steps + 4096) - 1.0e-5).abs() < 1e-15, "never below the end");
    }

    #[test]
    fn the_rate_without_a_warmup_starts_at_the_start() {
        let budget = Budget::new(1_000_000, 100, 10, 0, 2.0, &[1.0e-3, 1.0e-5]);
        assert_eq!(budget.rate(0), 1.0e-3, "no ramp: the start itself");
        // The fall begins at once: step 1 is one 20,000th of the way down, about 0.02% below.
        assert!((budget.rate(1) / 1.0e-3 - 1.0).abs() < 1e-3, "{}", budget.rate(1));
        assert!(budget.rate(2) < budget.rate(1));
    }

    #[test]
    fn nine_candidates_from_epoch_two_to_four_the_last_at_the_budget_s_end() {
        let budget = full();
        let candidates = budget.candidates();
        let epochs: Vec<f64> = candidates.iter().map(|&(epoch, _)| epoch).collect();
        assert_eq!(epochs, vec![2.0, 2.25, 2.5, 2.75, 3.0, 3.25, 3.5, 3.75, 4.0]);
        for &(epoch, step) in &candidates {
            assert_eq!(step % 4096, 0, "epoch {epoch} at a round end");
            let at = epoch * budget.steps_per_epoch as f64;
            assert!(step as f64 >= at && (step as f64) < at + 4096.0, "epoch {epoch}: the first round end past it");
        }
        assert_eq!(candidates.last().unwrap().1, budget.total_steps);
        assert_eq!(epoch_label(2.25), "2.250");
    }

    #[test]
    fn any_budget_has_nine_candidates_over_its_second_half() {
        let three = Budget::new(1_000_000, 100, 10, 0, 3.0, &[1.0e-3, 1.0e-5]);
        let epochs: Vec<f64> = three.candidates().iter().map(|&(epoch, _)| epoch).collect();
        assert_eq!(epochs, vec![1.5, 1.6875, 1.875, 2.0625, 2.25, 2.4375, 2.625, 2.8125, 3.0]);

        // A scaled run's budget, a fraction of an epoch: still nine, the last at its end, each labelled
        // apart from the others.
        let scaled = Budget::new(1_000_000, 100, 10, 0, 4.0 / 27.0, &[1.0e-3, 1.0e-5]);
        let candidates = scaled.candidates();
        assert_eq!(candidates.len(), 9);
        assert_eq!(candidates.last().unwrap().1, scaled.total_steps);
        let labels: Vec<String> = candidates.iter().map(|&(epoch, _)| epoch_label(epoch)).collect();
        assert!(labels.windows(2).all(|pair| pair[0] != pair[1]), "{labels:?}");
    }

    #[test]
    fn a_tiny_run_s_candidates_never_repeat_a_step() {
        // 40 steps an epoch, rounds of 32: quarters fall on few round ends.
        let tiny = Budget::new(400, 10, 32, 0, 4.0, &[1.0e-3, 1.0e-5]);
        let steps = tiny.candidate_steps();
        assert!(steps.windows(2).all(|pair| pair[0] < pair[1]), "{steps:?}");
        assert_eq!(*steps.last().unwrap(), tiny.total_steps);
    }

    #[test]
    fn a_budget_stands_where_its_rounds_went_and_finishes_at_its_last_step() {
        // 8 steps an epoch, rounds of 10, 10 epochs: 80 steps. Flat losses never end it early.
        let budget = Budget::new(80, 10, 10, 1, 10.0, &[1.0e-3, 1.0e-5]);
        assert_eq!(budget.total_steps, 80);
        let rounds = |losses: &[f64]| -> Vec<(u64, f64)> {
            losses.iter().enumerate().map(|(index, &loss)| ((index as u64 + 1) * 10, loss)).collect()
        };
        let standing = budget.standing(&rounds(&[0.5, 0.4, 0.6, 0.6, 0.6, 0.6, 0.6]));
        assert_eq!(standing, BudgetStanding { last_step: 70, finished: false, best_step: 20, best_loss: 0.4 });
        let standing = budget.standing(&rounds(&[0.5, 0.4, 0.6, 0.6, 0.6, 0.6, 0.6, 0.6, 0.6]));
        assert!(standing.finished);
        assert_eq!(standing.last_step, 80, "read up to the last step, no further");
    }

    #[test]
    #[should_panic(expected = "leaves nothing")]
    fn a_warmup_as_long_as_the_budget_is_refused() {
        let _ = Budget::new(400, 10, 10, 4, 1.0, &[1.0e-3, 1.0e-5]);
    }
}
