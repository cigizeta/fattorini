//! One run's training: one network (`nnue_network::Network` or `transformer_network::TransformerNetwork`)
//! under one `AdamW`, over one stream, on a budget (`budget.rs`). `train` is the loop, for every track;
//! what differs between the tracks comes in through its closures.
//!
//! It replaced the lockstep of a grid's lambdas over a stacked network (2026-11-26): no config trains a
//! grid any more, and patience, which only the stack could roll back, is no longer trained. A grid's
//! lambdas, while the grid's code lasts, train one after another.

use std::sync::atomic::{AtomicBool, Ordering};

use fattorini::pipeline::preprocessing_config::{self, ArmLambda};
use tch::{nn, Tensor};

use crate::adamw::AdamW;
use crate::budget::{Budget, BudgetStanding};
use crate::cache::CacheRecord;
use crate::game_labels::RoundProgress;
use crate::net_config::{NetConfig, Schedule};
use crate::rounds_display::{print_progress, timing_line};
use crate::{
    attempt_config, batch, checkpoint_store, game_labels, history, nnue_network, round, stages, test_cache, train_stream,
    transformer_batch, transformer_network,
};

/// One run: its own files in `run_dir` (a lambda's directory), where it stands, and `column`, where its
/// labels sit in every batch.
pub(crate) struct Arm {
    pub(crate) lambda: ArmLambda,
    pub(crate) column: usize,
    pub(crate) run_dir: String,
    pub(crate) checkpoints_dir: String,
    pub(crate) model_path: String,
    pub(crate) optimizer_path: String,
    pub(crate) history_path: String,
    pub(crate) round_progress_path: String,
    pub(crate) standing: Standing,
}

/// Where a run stands, by its rule: patience's stages (read by the page for old runs alone), or a
/// budget's progress, which has no stage and no patience.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Standing {
    Patience(stages::Stages),
    Budget(BudgetStanding),
}

impl Standing {
    pub(crate) fn finished(&self) -> bool {
        match self {
            Standing::Patience(stages) => stages.finished,
            Standing::Budget(standing) => standing.finished,
        }
    }

    /// The step of the lowest test loss so far: patience's candidate, a budget's for the page alone.
    pub(crate) fn best_step(&self) -> u64 {
        match self {
            Standing::Patience(stages) => stages.best_step,
            Standing::Budget(standing) => standing.best_step,
        }
    }

    pub(crate) fn best_loss(&self) -> f64 {
        match self {
            Standing::Patience(stages) => stages.best_loss,
            Standing::Budget(standing) => standing.best_loss,
        }
    }
}

/// How a run's rate moves and when it ends, its config's `training.schedule` in figures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Rule {
    /// Deprecated, no longer trained: stages stepped down by patience (`stages.rs`). Kept for the page's
    /// reading of old runs.
    Patience { warmup_rounds: u64, patience_rounds: u64, stage_count: usize },
    /// A fixed budget, the rate falling with the step (`budget.rs`).
    Budget(Budget),
}

impl Rule {
    /// `config`'s rule for a run over `train_rows` training rows.
    pub(crate) fn of(config: &NetConfig, train_rows: u64) -> Rule {
        let warmup_rounds = config.training.warmup_rounds.expect("net_config::load already validated this is Some");
        match config.training.schedule {
            Schedule::Patience => Rule::Patience {
                warmup_rounds,
                patience_rounds: config.training.patience_rounds.expect("net_config::load already validated this is Some"),
                stage_count: config.training.learning_rate.len(),
            },
            Schedule::Budget => Rule::Budget(Budget::new(
                train_rows,
                config.training.training_batch_size as u64,
                config.training.steps_per_round,
                warmup_rounds,
                config.training.epochs.expect("net_config::load already validated this is Some"),
                &config.training.learning_rate,
            )),
        }
    }

    /// Where a run whose rounds are `rounds` stands under this rule.
    pub(crate) fn standing(&self, rounds: &[(u64, f64)], steps_per_round: u64) -> Standing {
        match *self {
            Rule::Patience { warmup_rounds, patience_rounds, stage_count } => {
                Standing::Patience(stages::stage_after(rounds, steps_per_round, warmup_rounds, patience_rounds, stage_count))
            }
            Rule::Budget(budget) => Standing::Budget(budget.standing(rounds)),
        }
    }
}

/// Opens one run's training, in `run_dir`: freezes or checks its `attempt_config.txt` and replays where
/// it stands. Its weights are loaded later (`open_one_arm`). `source` is the dataset it trains on.
#[allow(clippy::too_many_arguments)]
fn open_arm(
    config: &NetConfig, budget: &Budget, source: &str, run_dir: String, lambda: ArmLambda, column: usize, train_rows: u64,
    test_rows: u64,
) -> Arm {
    let warmup_rounds = config.training.warmup_rounds.expect("net_config::load already validated this is Some");
    let rates = &config.training.learning_rate;
    let steps_per_round = config.training.steps_per_round;

    let checkpoints_dir = format!("{run_dir}/checkpoints");
    std::fs::create_dir_all(&checkpoints_dir)
        .unwrap_or_else(|error| panic!("failed to create checkpoint directory {checkpoints_dir}: {error}"));
    let model_path = format!("{run_dir}/model.vs");
    let optimizer_path = format!("{run_dir}/optimizer.opt");
    let history_path = format!("{run_dir}/history.tsv");
    let round_progress_path = format!("{run_dir}/round_progress.state");

    // Before the weights are even built: everything below derives its position in the run from its
    // step count and its rate from the budget, so the fields those derivations read have to be the same
    // ones the earlier invocations of this attempt used. See `attempt_config`'s own module doc comment
    // for what each frozen field protects. The lines and their order are the lockstep's, so a budget
    // attempt begun under it resumes.
    let frozen: Vec<(&str, String)> = vec![
        ("architecture", format!("{:?}", config.architecture)),
        // What the output is trained to mean and the loss, the config's unit (`NetConfig::curve`): an
        // attempt started under the atan curve and the matching loss records no such line, so it is
        // refused rather than resumed; one begun under the old S, before `unit:`, resumes under it.
        ("curve", format!("{} bce", config.curve().describe())),
        // The dataset actually trained on.
        ("parent", source.to_string()),
        ("lambda", preprocessing_config::lambda_text(lambda)),
        ("learning_rate", rates.iter().map(|rate| format!("{rate:e}")).collect::<Vec<String>>().join(" ")),
        ("training_batch_size", config.training.training_batch_size.to_string()),
        ("steps_per_round", steps_per_round.to_string()),
        ("warmup_rounds", warmup_rounds.to_string()),
        ("schedule", "budget".to_string()),
        ("epochs", budget.epochs.to_string()),
        ("train_rows", train_rows.to_string()),
        ("test_rows", test_rows.to_string()),
    ];
    attempt_config::freeze_or_verify(&format!("{run_dir}/{}", attempt_config::FILE), &frozen);

    let standing = replay(&history_path, steps_per_round, &Rule::Budget(*budget));
    Arm { lambda, column, run_dir, checkpoints_dir, model_path, optimizer_path, history_path, round_progress_path, standing }
}

/// A network one run trains: its losses against label column `column` of a batch.
pub(crate) trait Model {
    type Batch;
    fn training_loss(&self, batch: &Self::Batch, column: usize) -> Tensor;
    fn test_loss(&self, batch: &Self::Batch, column: usize) -> Tensor;
}

impl Model for nnue_network::Network {
    type Batch = batch::DenseBatch;

    fn training_loss(&self, batch: &batch::DenseBatch, column: usize) -> Tensor {
        nnue_network::Network::training_loss(self, batch, column)
    }

    fn test_loss(&self, batch: &batch::DenseBatch, column: usize) -> Tensor {
        nnue_network::Network::test_loss(self, batch, column)
    }
}

impl Model for transformer_network::TransformerNetwork {
    type Batch = transformer_batch::TransformerBatch;

    fn training_loss(&self, batch: &transformer_batch::TransformerBatch, column: usize) -> Tensor {
        transformer_network::TransformerNetwork::training_loss(self, batch, column)
    }

    fn test_loss(&self, batch: &transformer_batch::TransformerBatch, column: usize) -> Tensor {
        transformer_network::TransformerNetwork::test_loss(self, batch, column)
    }
}

/// The network a run trains, its weights' store and its optimizer, with the files a backup writes:
/// what `round::run_round` steps, as one arm.
pub(crate) struct OneArm<M: Model> {
    pub(crate) network: M,
    pub(crate) vs: nn::VarStore,
    pub(crate) optimizer: AdamW,
    column: usize,
    lambda: ArmLambda,
    model_path: String,
    optimizer_path: String,
    round_progress_path: String,
    steps_per_round: u64,
}

impl<M: Model> OneArm<M> {
    /// Writes the weights and the optimizer's state to the run's `model.vs` and `optimizer.opt`.
    fn save(&self) {
        self.vs.save(&self.model_path).unwrap_or_else(|error| panic!("failed to save {}: {error}", self.model_path));
        self.optimizer.save(&self.optimizer_path);
    }

    fn run_step(&self) -> u64 {
        self.optimizer.step_count() as u64
    }
}

impl<M: Model> round::Arms<M::Batch> for OneArm<M> {
    fn count(&self) -> usize {
        1
    }

    // Under a budget nothing rolls back, so the optimizer's count is the run step.
    fn step_count(&self, _arm: usize) -> u64 {
        self.run_step()
    }

    fn train_step(&mut self, batch: &M::Batch, stepping: &[bool]) -> Tensor {
        assert!(stepping[0], "one arm always stands where the stream does");
        let loss = self.network.training_loss(batch, self.column);
        self.optimizer.zero_grad();
        loss.backward();
        self.optimizer.step();
        loss.detach().reshape([1])
    }

    fn test_losses(&self, batch: &M::Batch) -> Tensor {
        self.network.test_loss(batch, self.column).reshape([1])
    }

    // The stream's position and the rows into the round are derived from the step count on the way
    // back in, so they are persisted as empty and zero: a real value in a field nothing reads is how a
    // stale second copy gets mistaken for state later. The bucket streams' `cursors` return an empty
    // vector for the same reason.
    fn back_up(&mut self, _arm: usize, _cursors: &[u64], _rows_done: u64, loss_sum: f64, duration_secs: f64) {
        self.save();
        RoundProgress { train_cursors: Vec::new(), rows_done: 0, loss_sum, train_duration_secs: duration_secs }
            .save(&self.round_progress_path);
        // Not "periodic": `run_round` also calls this the moment a stop is seen.
        log::debug!(
            "  lambda {}: backup saved ({}/{} steps into this round)",
            self.lambda,
            self.run_step() % self.steps_per_round,
            self.steps_per_round
        );
    }
}

/// The run `arm` over `network`, whose weights `vs` holds: resumed from its backup, or started from
/// `starting_weights`, which fills `vs` (the teacher's, or the fresh initialisation left as it is);
/// then its budget and `bounds` set.
pub(crate) fn open_one_arm<M: Model>(
    config: &NetConfig, budget: Budget, arm: &Arm, network: M, mut vs: nn::VarStore, bounds: Vec<(&'static str, f64)>,
    starting_weights: impl FnOnce(&mut nn::VarStore),
) -> OneArm<M> {
    let steps_per_round = config.training.steps_per_round;
    let resumed = std::path::Path::new(&arm.model_path).exists();
    if resumed {
        vs.load(&arm.model_path).unwrap_or_else(|error| panic!("failed to load {}: {error}", arm.model_path));
    } else {
        starting_weights(&mut vs);
    }
    // After the weights load: the optimizer tracks `vs`'s tensors, which a load copies into in place.
    let mut optimizer = AdamW::new(&vs, config.training.learning_rate[0]);
    if resumed {
        optimizer.load(&arm.optimizer_path);
        // As the round lines count: the round under way, and how far into it, as a backup says.
        let steps = optimizer.step_count() as u64;
        log::info!(
            "lambda {}: resumed from {} in round {} ({}/{steps_per_round} steps into it)",
            arm.lambda,
            arm.run_dir,
            steps / steps_per_round + 1,
            steps % steps_per_round
        );
    }
    // Set on a resumed run too: the warmup and the fall are derived from its restored step count.
    optimizer.set_budget(budget);
    // After the weights load, so a start from a network trained without the bounds begins inside them.
    for (name, bound) in bounds {
        optimizer.bound(name, bound);
    }
    OneArm {
        network,
        vs,
        optimizer,
        column: arm.column,
        lambda: arm.lambda,
        model_path: arm.model_path.clone(),
        optimizer_path: arm.optimizer_path.clone(),
        round_progress_path: arm.round_progress_path.clone(),
        steps_per_round,
    }
}

/// What a run's training reads beyond its type parameters.
pub(crate) struct RunParts<'a> {
    /// The dataset trained on, as `attempt_config.txt` freezes it.
    pub(crate) source: &'a str,
    /// The lambda trained, its run's directory, and its labels' column in every batch.
    pub(crate) lambda: ArmLambda,
    pub(crate) run_dir: String,
    pub(crate) column: usize,
    pub(crate) device: tch::Device,
}

/// Trains one run to its budget's end, for either track: one stream over the dataset, one network
/// stepping on each batch, its history, checkpoints and candidates its own. Returns whether it got
/// there: `false` after a stop, which leaves the run to resume when the smithy starts again.
///
/// What the tracks supply: the test readers, `open` (the network and optimizer, `open_one_arm`),
/// `open_stream` (the training stream, given the step count), `to_batch`, the size of a kept test batch,
/// and `finish`; `after_round`, called after every round once the run's files are written, the
/// smithy's moment to copy them (none of them being replaced meanwhile, which Windows would refuse).
#[allow(clippy::too_many_arguments)]
pub(crate) fn train<M: Model, H: Send, T: train_stream::BatchSource + Send, S: train_stream::TrainStream + Send>(
    config: &NetConfig, parts: RunParts, train_rows: u64, test_readers: &mut [T], open: impl FnOnce(Budget, &Arm) -> OneArm<M>,
    open_stream: impl FnOnce(u64) -> S, to_batch: &round::BatchBuilder<impl Fn(&[CacheRecord]) -> H + Sync, impl Fn(&H) -> M::Batch>,
    host_bytes: fn(&H) -> usize, finish: impl Fn(&Arm, &Budget), after_round: impl Fn(), stop_requested: &AtomicBool,
) -> bool {
    // No earlier training's pass shown before this one's first; none left once it returns.
    crate::rounds_display::end_pass();
    let _pass_end = crate::rounds_display::PassEnd;
    let source = parts.source;
    log::info!("training lambda {} on the dataset of {source}", parts.lambda);
    let warmup_rounds = config.training.warmup_rounds.expect("net_config::load already validated this is Some");
    let steps_per_round = config.training.steps_per_round;
    let batch_size = config.training.training_batch_size as usize;
    let test_rows: u64 = test_readers.iter().map(|reader| reader.row_count()).sum();
    log::info!("{train_rows} train rows, {test_rows} test rows");
    // A dataset shorter than a round is fine -- the read is cyclic -- but one shorter than a single
    // batch is not: the cyclic read would wrap inside a batch and put the same position in it several
    // times, which is a duplicated gradient rather than a small one.
    assert!(
        train_rows >= config.training.training_batch_size as u64,
        "the dataset of {source} holds {train_rows} train rows, fewer than one batch of {}. Build a larger dataset, or \
         lower training.training_batch_size.",
        config.training.training_batch_size
    );
    log::info!(
        "one round is {steps_per_round} steps, {:.4} passes over the train cache",
        (steps_per_round * batch_size as u64) as f64 / train_rows as f64
    );
    let Rule::Budget(budget) = Rule::of(config, train_rows) else {
        panic!("patience is no longer trained: give the config `schedule: budget` and its `epochs`")
    };
    log::info!(
        "a budget of {} rounds, {} epochs: {warmup_rounds} warmup rounds, then the rate from {:e} to {:e}, geometric; \
         candidates at rounds {}",
        budget.total_rounds(),
        budget.epochs,
        budget.start,
        budget.end,
        budget.candidate_steps().iter().map(|step| (step / steps_per_round).to_string()).collect::<Vec<String>>().join(", ")
    );
    log::info!("device = {:?}", parts.device);

    let mut arm = open_arm(config, &budget, source, parts.run_dir, parts.lambda, parts.column, train_rows, test_rows);
    // A run interrupted between meeting its budget's end and finishing finishes now.
    if arm.standing.finished() {
        finish(&arm, &budget);
        return true;
    }
    let mut one = open(budget, &arm);
    let steps_done = one.run_step();

    // A round's running loss sum and seconds are never derivable from the step count; the rows into
    // the round are its steps into it times the batch size.
    let rows_done = (steps_done % steps_per_round) * batch_size as u64;
    let (loss_sum, round_duration_secs) = if std::path::Path::new(&arm.round_progress_path).exists() {
        let progress = RoundProgress::load(&arm.round_progress_path);
        (progress.loss_sum, progress.train_duration_secs)
    } else {
        (0.0, 0.0)
    };

    // The test batches this run keeps across rounds, filled by its first test pass (`test_cache.rs`).
    let mut kept_test_batches = test_cache::TestCache::new(
        test_cache::budget().unwrap_or_else(|error| {
            log::warn!("test cache: {error}, so every test pass prepares its batches");
            0
        }),
        host_bytes,
    );

    // Step 0: the starting weights, measured before any training, for the history alone: a budget
    // never rolls back, and step 0 is never a candidate. Recorded as a round of its own, with no train
    // loss. A run that has stepped, or already recorded step 0 before a crash, is not measured again.
    if steps_done == 0 && !has_history_row(&arm.history_path, 0) {
        let (test_losses, test_duration_secs) = round::run_test_round(
            &one, &mut *test_readers, Some(&mut kept_test_batches), batch_size, to_batch,
            &mut |label: &str, processed: u64, total: u64| print_progress(label, processed, total, "round 0"),
        );
        let loss = test_losses.global[0];
        history::append(
            &arm.history_path,
            &history::EvalRecord {
                step: 0,
                learning_rate: budget.rate(0),
                train_loss: f64::NAN,
                test_loss: loss,
                train_duration_secs: 0.0,
                test_duration_secs,
            },
        );
        history::append_pieces(&pieces_path(&arm.run_dir), 0, &round::losses_of_arm(&test_losses.by_pieces, 0));
        arm.standing = replay(&arm.history_path, steps_per_round, &Rule::Budget(budget));
        log::info!("lambda {}: starting weights measured, test loss {loss:.6}", arm.lambda);
    }

    let mut train_stream = open_stream(steps_done);
    log::info!(
        "lambda {}: training from round {} of {}, at rate {:e}",
        arm.lambda,
        steps_done / steps_per_round + 1,
        budget.total_rounds(),
        budget.rate(steps_done + 1)
    );

    // Only the first round of this process resumes a partially-done round; every later one starts
    // its counters at zero, which is exactly what `None` means to `run_round`.
    let mut resume =
        Some(round::RoundResume { rows_done: vec![rows_done], loss_sums: vec![loss_sum], duration_secs: round_duration_secs });

    loop {
        let step_at_round_start = one.run_step();
        let steps_into_round = step_at_round_start % steps_per_round;
        if steps_into_round > 0 {
            log::info!("resuming an interrupted evaluation round: {steps_into_round} steps already accounted for");
        }
        let round_label = format!("round {}", step_at_round_start / steps_per_round + 1);

        let cache_was_filled = kept_test_batches.is_filled();
        let outcome = round::run_round(
            &mut one, &mut train_stream, &mut *test_readers, Some(&mut kept_test_batches),
            steps_per_round, batch_size, resume.take(), game_labels::BACKUP_INTERVAL, to_batch,
            &mut |label: &str, processed: u64, total: u64| print_progress(label, processed, total, &round_label),
            stop_requested,
        );
        let Some(outcome) = outcome else {
            log::info!("training stopped mid-round; it resumes from its backup");
            return false;
        };
        if !cache_was_filled {
            log::info!(
                "test cache: {:.2} GB, {} of {} positions (budget {:.2} GB)",
                test_cache::gigabytes(kept_test_batches.bytes()), kept_test_batches.rows(), test_rows,
                test_cache::gigabytes(kept_test_batches.budget())
            );
        }

        let step = one.run_step();
        // History first, archive second, prune third: the standing replayed from the history names the
        // checkpoints kept, so the row has to exist before the file it describes. A crash between the
        // first two leaves a history row with no checkpoint, which is harmless; a crash before the
        // prune leaves an extra checkpoint, which the next round's prune deletes.
        history::append(
            &arm.history_path,
            &history::EvalRecord {
                step,
                learning_rate: budget.rate(step),
                train_loss: outcome.train_loss[0],
                test_loss: outcome.test_loss[0],
                train_duration_secs: outcome.train_duration_secs,
                test_duration_secs: outcome.test_duration_secs,
            },
        );
        // For reading only, after the row the run replays from.
        history::append_pieces(&pieces_path(&arm.run_dir), step, &round::losses_of_arm(&outcome.test_loss_by_pieces, 0));
        let checkpoint = checkpoint_store::checkpoint_path(&arm.checkpoints_dir, step);
        one.vs.save(&checkpoint).unwrap_or_else(|error| panic!("failed to save {checkpoint}: {error}"));
        arm.standing = replay(&arm.history_path, steps_per_round, &Rule::Budget(budget));
        // The candidates kept for the round robin, and this round's for nothing but a look (a resume
        // reads `model.vs`, never a checkpoint).
        let mut keep = budget.candidate_steps();
        keep.push(step);
        checkpoint_store::keep_steps(&arm.checkpoints_dir, &keep);
        // The backup last, at the round's boundary.
        one.save();
        RoundProgress { train_cursors: Vec::new(), rows_done: 0, loss_sum: 0.0, train_duration_secs: 0.0 }
            .save(&arm.round_progress_path);

        let mut line = timing_line(step / steps_per_round, outcome.train_duration_secs, outcome.test_duration_secs);
        if arm.standing.best_step() == step {
            line.push_str(&format!("; a new best, loss {:.6}", arm.standing.best_loss()));
        }
        log::info!("{line}");
        if arm.standing.finished() {
            finish(&arm, &budget);
            return true;
        }
        // The run's files written and none being written: the one moment they may be copied.
        after_round();
        if stop_requested.load(Ordering::SeqCst) {
            log::info!("training stopped after step {step}; it resumes from there");
            return false;
        }
    }
}

/// Where the run in `history_path` stands under `rule`, replayed from its recorded test losses.
fn replay(history_path: &str, steps_per_round: u64, rule: &Rule) -> Standing {
    rule.standing(&recorded_rounds(history_path), steps_per_round)
}

/// `latest_round` of `records` already read: the status page reads them itself, tolerating a line
/// the trainer is still writing (`history::read_complete`). `None` before its first round.
pub(crate) fn latest_of(mut records: Vec<history::EvalRecord>, steps_per_round: u64, rule: &Rule) -> Option<(history::EvalRecord, Standing)> {
    let rounds: Vec<(u64, f64)> = records.iter().map(|record| (record.step, record.test_loss)).collect();
    let standing = rule.standing(&rounds, steps_per_round);
    Some((records.pop()?, standing))
}

/// Every round of `history_path` as `(step, test_loss)`, in the order recorded; none without the file.
fn recorded_rounds(history_path: &str) -> Vec<(u64, f64)> {
    if !std::path::Path::new(history_path).exists() {
        return Vec::new();
    }
    history::read_all(history_path).iter().map(|record| (record.step, record.test_loss)).collect()
}

/// A run's `pieces.tsv`, beside its `history.tsv`: each round's test loss per piece count
/// (`history::append_pieces`).
fn pieces_path(run_dir: &str) -> String {
    format!("{run_dir}/pieces.tsv")
}

/// Whether `history_path` exists and records a round at `step`.
fn has_history_row(history_path: &str, step: u64) -> bool {
    if !std::path::Path::new(history_path).exists() {
        return false;
    }
    history::read_all(history_path).iter().any(|record| record.step == step)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bucketed_train_stream::BucketedTrainStream;
    use crate::candidates::export_budget_candidates;
    use crate::feature_set::FeatureSet;
    use crate::grouped_test_set::GroupedTestSet;
    use crate::net_config;
    use crate::pgn_stream::StreamFile;
    use crate::train_stream::TrainStream;
    use fattorini::pipeline::generation;

    #[test]
    fn a_budget_replays_as_unfinished_for_a_run_with_no_history() {
        let budget = Budget::new(80, 10, 2, 1, 2.0, &[1e-2, 1e-4]);
        let standing = replay("data/networks/train-main-test-no-such-run/history.tsv", 2, &Rule::Budget(budget));
        assert!(!standing.finished());
    }

    /// A flat test set over `records`, read in order and stopping short at its end.
    struct Flat(Vec<CacheRecord>);

    impl train_stream::BatchSource for Flat {
        fn row_count(&self) -> u64 {
            self.0.len() as u64
        }

        fn read_batch_stop_short(&mut self, cursor: u64, batch_size: usize) -> (Vec<CacheRecord>, u64) {
            let start = cursor as usize;
            let end = (start + batch_size).min(self.0.len());
            (self.0[start..end].to_vec(), if end == self.0.len() { 0 } else { end as u64 })
        }
    }

    /// A fresh temporary directory named after `test`, with `config` as its `net.yaml`, and two
    /// training segments of labelled games, a rook up so that no position is K vs K, scattered into its
    /// `buckets` as the server scatters a training set: the config, the bucket directory with its 40
    /// records, and a test set of those records at lambda 0.7, grouped by piece count.
    fn fixture(test: &str, config: &str) -> (String, NetConfig, String, GroupedTestSet) {
        let dir = std::env::temp_dir().join(format!("smithy-training-{test}-{}", std::process::id()));
        let dir = dir.to_str().unwrap().replace('\\', "/");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = format!("{dir}/net.yaml");
        std::fs::write(&config_path, config).unwrap();
        let config = net_config::load(&config_path);
        // Two games of 3 and 4 positions, as `pgn_train_stream`'s fixture counts them.
        let games = concat!(
            "[Result \"1-0\"]\n[FEN \"4k3/8/8/8/8/8/8/R3K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Kd2 {1.50} Kd7 {-1.20}\n1-0\n\n",
            "[Result \"0-1\"]\n[FEN \"4k3/8/8/8/8/8/8/R3K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Ke2 {-0.40} Ke7 {0.30} 2. Kf2 {-0.90}\n0-1\n\n",
        );
        let files: Vec<StreamFile> = [3u64, 5]
            .iter()
            .enumerate()
            .map(|(index, &pairs)| {
                let path = format!("{dir}/gamma-{index}.pgn.zst");
                std::fs::write(&path, zstd::encode_all(games.repeat(pairs as usize).as_bytes(), 3).unwrap()).unwrap();
                StreamFile { path, positions: pairs * 7 }
            })
            .collect();
        let buckets = format!("{dir}/buckets");
        assert_eq!(crate::buckets::prepare_for_tests(&buckets, &files, 1), 40);
        let records = crate::buckets::BucketStream::open(&buckets, labelling(), 0).unwrap().next_batch(40);
        let grouped = GroupedTestSet::load(&mut Flat(records), config.training.test_batch_size as usize);
        (dir, config, buckets, grouped)
    }

    const LAMBDA: ArmLambda = ArmLambda::Constant(0.7);

    fn labelling() -> crate::buckets::Labelling {
        crate::buckets::Labelling::new(&[LAMBDA], None).unwrap()
    }

    /// A budget of 2 epochs over 40 rows in batches of 8, rounds of 2 steps: 5 steps an epoch, 10 in
    /// all, rounded up to 10, so 5 rounds.
    const TRAINING: &str = "training:\n  schedule: budget\n  epochs: 2\n  learning_rate: [1.0e-2, 1.0e-4]\n  \
                            training_batch_size: 8\n  test_batch_size: 8\n  steps_per_round: 2\n  warmup_rounds: 1\n";

    /// The run ended: its history from step 0, one round every 2 steps to the budget's end, and its
    /// candidates recorded.
    fn assert_trained(run_dir: &str) {
        let records = history::read_all(&format!("{run_dir}/history.tsv"));
        assert!(records[0].train_loss.is_nan(), "step 0 is measured, not trained");
        let steps: Vec<u64> = records.iter().map(|record| record.step).collect();
        assert_eq!(steps, vec![0, 2, 4, 6, 8, 10]);
        assert!(crate::candidates::read_budget_candidates(run_dir).is_ok(), "its candidates are recorded");
    }

    /// Runs `body`, then removes `dir` whatever happened, then passes on a panic.
    fn cleaned_up(dir: &str, body: impl FnOnce() + std::panic::UnwindSafe) {
        let outcome = std::panic::catch_unwind(body);
        std::fs::remove_dir_all(dir).ok();
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }

    /// The smithy's training of an NNUE, as `Run::train` wires it, from random weights to its budget's
    /// end, then a second call, which finds it finished.
    #[test]
    fn an_nnue_trains_to_its_budget_s_end() {
        let (dir, config, buckets, mut grouped) = fixture(
            "nnue",
            &format!("labels: lc0\narchitecture:\n  kind: psq\n  embedding_dim: 8\n  value_head:\n    hidden_dim: [4]\n{TRAINING}"),
        );
        let device = tch::Device::Cpu;
        let run_dir = generation::lambda_dir(&dir, LAMBDA);
        let to_batch = round::BatchBuilder {
            prepare: |records: &[CacheRecord]| batch::host_batch(records, FeatureSet::Psq),
            upload: |host: &batch::HostBatch| batch::upload(host, device),
        };
        let run = |grouped: &mut GroupedTestSet| {
            let open = |budget: Budget, arm: &Arm| {
                let vs = nn::VarStore::new(device);
                let network = nnue_network::Network::of(&vs.root(), FeatureSet::Psq, &config.architecture).with_s(config.s_pawns());
                let bounds = network.weight_bounds();
                open_one_arm(&config, budget, arm, network, vs, bounds, |_| {})
            };
            let stream_from = |steps_done: u64| crate::buckets::BucketStream::open(&buckets, labelling(), steps_done * 8).unwrap();
            let finish = |arm: &Arm, budget: &Budget| {
                export_budget_candidates(&config, &arm.run_dir, budget);
            };
            let parts = RunParts { source: "fixture", lambda: LAMBDA, run_dir: run_dir.clone(), column: 0, device };
            train(
                &config, parts, 40, std::slice::from_mut(grouped), open, stream_from, &to_batch, batch::HostBatch::bytes, finish,
                || {}, &AtomicBool::new(false),
            )
        };
        let first = run(&mut grouped);
        let again = run(&mut grouped);
        let run_dir_checked = run_dir.clone();
        cleaned_up(&dir, move || {
            assert!(first, "no stop was requested, so the run ends");
            assert!(again, "a finished run finishes again at once");
            assert_trained(&run_dir_checked);
        });
    }

    /// The smithy's training of a transformer, its stream the scattered buckets grouped by piece count.
    #[test]
    fn a_transformer_trains_to_its_budget_s_end() {
        let (dir, config, buckets, grouped) = fixture(
            "transformer",
            &format!(
                "labels: lc0\narchitecture:\n  kind: transformer\n  embedding_dim: 8\n  value_head:\n    hidden_dim: [4]\n  \
                 num_layers: 2\n  num_heads: 2\n  feedforward_dim: 16\n{TRAINING}  attention: full\n"
            ),
        );
        let device = tch::Device::Cpu;
        let attention = config.training.attention.unwrap();
        let run_dir = generation::lambda_dir(&dir, LAMBDA);
        let to_batch = round::BatchBuilder {
            prepare: |records: &[CacheRecord]| transformer_batch::host_batch(records, attention),
            upload: |host: &transformer_batch::HostTransformerBatch| transformer_batch::upload(host, device),
        };
        let open = |budget: Budget, arm: &Arm| {
            let vs = nn::VarStore::new(device);
            let network = transformer_network::TransformerNetwork::new(&vs.root(), &config.architecture);
            open_one_arm(&config, budget, arm, network, vs, Vec::new(), |_| {})
        };
        // Every position a rook up, 40 of them: 5 batches of 8 an epoch.
        let counts = crate::buckets::ensure_piece_counts(&buckets, 1, &AtomicBool::new(false), &|_| {}).unwrap().unwrap();
        let train_rows = counts.batches_per_epoch(8) * 8;
        let stream_from = |steps_done: u64| BucketedTrainStream::open(&buckets, labelling(), &counts, 8, steps_done).unwrap();
        let finish = |arm: &Arm, budget: &Budget| {
            export_budget_candidates(&config, &arm.run_dir, budget);
        };
        let parts = RunParts { source: "fixture", lambda: LAMBDA, run_dir: run_dir.clone(), column: 0, device };
        let finished = train(
            &config, parts, train_rows, &mut [grouped], open, stream_from, &to_batch,
            transformer_batch::HostTransformerBatch::bytes, finish, || {}, &AtomicBool::new(false),
        );
        cleaned_up(&dir, move || {
            assert!(finished, "no stop was requested, so the run ends");
            assert_trained(&run_dir);
        });
    }
}
