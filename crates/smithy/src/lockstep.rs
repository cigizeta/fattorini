//! Every lambda of a grid trained at once over one stream (lockstep-lambdas.md), their networks
//! stacked into one (`Stack`), each lambda keeping a run of its own: its weights, optimizer, rates,
//! patience, history, checkpoints and candidate. `train_in_lockstep` is the loop, for every track and
//! every caller; what differs between them comes in through `LockstepParts` and its closures.

use std::sync::atomic::{AtomicBool, Ordering};

use temper_chess::pipeline::preprocessing_config::{self, ArmLambda};

use crate::cache::CacheRecord;
use crate::game_labels::RoundProgress;
use crate::net_config::NetConfig;
use crate::rounds_display::{print_progress, timing_line};
use crate::stacked_adamw::StackedAdamW;
use crate::stacked_network::StackedNetwork;
use crate::stacked_transformer::StackedTransformer;
use crate::{
    arm_slice, batch, checkpoint_store, game_labels, history, round, run_config, stacked_network, stages, test_cache, train_stream,
    transformer_batch,
};

/// One lambda's run inside `train_in_lockstep`: its own `lambda-<l>/` files (in `gen-<g>/` for an
/// NNUE, in the network's folder for a transformer), where it is in its rates, and `column`, where its
/// labels sit in every batch. Its weights and optimizer state are one slice of the `Stack` while it
/// trains.
pub(crate) struct Arm {
    pub(crate) lambda: ArmLambda,
    pub(crate) column: usize,
    pub(crate) run_dir: String,
    pub(crate) checkpoints_dir: String,
    pub(crate) model_path: String,
    pub(crate) optimizer_path: String,
    /// The optimizer at the run's latest best after step 0, written at every new best: what a rollback
    /// to that best restores beside its checkpoint. A best at step 0 has a fresh optimizer instead.
    pub(crate) best_optimizer_path: String,
    pub(crate) history_path: String,
    pub(crate) round_progress_path: String,
    pub(crate) stages: stages::Stages,
}

/// Opens one lambda's run, in `run_dir`, for `train_in_lockstep`: freezes or checks its
/// `run_config.txt` and replays where it is in its rates. Its weights are loaded into the stack later
/// (`open_stack`). `source` is the dataset it trains on.
fn open_arm(
    config: &NetConfig, source: &str, run_dir: String, lambda: ArmLambda, column: usize, train_rows: u64, test_rows: u64,
) -> Arm {
    let warmup_rounds = config.training.warmup_rounds.expect("net_config::load already validated this is Some");
    let patience_rounds = config.training.patience_rounds.expect("net_config::load already validated this is Some");
    let rates = &config.training.learning_rate;
    let steps_per_round = config.training.steps_per_round;

    let checkpoints_dir = format!("{run_dir}/checkpoints");
    std::fs::create_dir_all(&checkpoints_dir)
        .unwrap_or_else(|error| panic!("failed to create checkpoint directory {checkpoints_dir}: {error}"));
    let model_path = format!("{run_dir}/model.vs");
    let optimizer_path = format!("{run_dir}/optimizer.opt");
    let best_optimizer_path = format!("{run_dir}/best.opt");
    let history_path = format!("{run_dir}/history.tsv");
    let round_progress_path = format!("{run_dir}/round_progress.state");

    // Before the weights are even built: everything below derives its position in the run from
    // its run step (`StackedAdamW::run_step`) and its rate from `history.tsv`, so the fields those derivations read
    // have to be the same ones the earlier invocations of this run used. See `run_config`'s own module
    // doc comment for what each frozen field protects.
    run_config::freeze_or_verify(
        &format!("{run_dir}/run_config.txt"),
        &[
            ("architecture", format!("{:?}", config.architecture)),
            // What the output is trained to mean and the loss: a run started under the atan curve and
            // the matching loss records no such line, so it is refused rather than resumed.
            ("curve", format!("{} bce", temper_chess::labels::LOGISTIC.describe())),
            // The dataset actually trained on.
            ("parent", source.to_string()),
            ("lambda", preprocessing_config::lambda_text(lambda)),
            ("learning_rate", rates.iter().map(|rate| format!("{rate:e}")).collect::<Vec<String>>().join(" ")),
            ("training_batch_size", config.training.training_batch_size.to_string()),
            ("steps_per_round", steps_per_round.to_string()),
            ("warmup_rounds", warmup_rounds.to_string()),
            ("patience_rounds", patience_rounds.to_string()),
            ("train_rows", train_rows.to_string()),
            ("test_rows", test_rows.to_string()),
        ],
    );

    let stages = replay_stages(&history_path, steps_per_round, warmup_rounds, patience_rounds, rates.len());
    Arm {
        lambda,
        column,
        run_dir,
        checkpoints_dir,
        model_path,
        optimizer_path,
        best_optimizer_path,
        history_path,
        round_progress_path,
        stages,
    }
}

/// One slot of the `Stack`: what a backup of it writes, and the lambda its lines name.
pub(crate) struct SlotFiles {
    pub(crate) lambda: ArmLambda,
    pub(crate) model_path: String,
    pub(crate) optimizer_path: String,
    pub(crate) round_progress_path: String,
}

/// A network of every arm at once, each parameter with a leading arm dimension: the NNUE's
/// (`StackedNetwork`) or the transformer's (`StackedTransformer`). What `Stack` needs of it.
pub(crate) trait StackedModel {
    type Batch;
    fn parameters(&self) -> &std::collections::HashMap<String, tch::Tensor>;
    fn retain(&mut self, keep: &[usize]);
    /// Every arm's training loss, `[arms]`, against label column `columns[k]`.
    fn training_losses(&self, batch: &Self::Batch, columns: &tch::Tensor) -> tch::Tensor;
    /// Every arm's test loss, `[arms]`.
    fn test_losses(&self, batch: &Self::Batch, columns: &tch::Tensor) -> tch::Tensor;
}

impl StackedModel for StackedNetwork {
    type Batch = batch::DenseBatch;

    fn parameters(&self) -> &std::collections::HashMap<String, tch::Tensor> {
        StackedNetwork::parameters(self)
    }

    fn retain(&mut self, keep: &[usize]) {
        StackedNetwork::retain(self, keep);
    }

    fn training_losses(&self, batch: &batch::DenseBatch, columns: &tch::Tensor) -> tch::Tensor {
        StackedNetwork::training_losses(self, batch, columns)
    }

    fn test_losses(&self, batch: &batch::DenseBatch, columns: &tch::Tensor) -> tch::Tensor {
        StackedNetwork::test_losses(self, batch, columns)
    }
}

impl StackedModel for StackedTransformer {
    type Batch = transformer_batch::TransformerBatch;

    fn parameters(&self) -> &std::collections::HashMap<String, tch::Tensor> {
        StackedTransformer::parameters(self)
    }

    fn retain(&mut self, keep: &[usize]) {
        StackedTransformer::retain(self, keep);
    }

    fn training_losses(&self, batch: &transformer_batch::TransformerBatch, columns: &tch::Tensor) -> tch::Tensor {
        StackedTransformer::training_losses(self, batch, columns)
    }

    fn test_losses(&self, batch: &transformer_batch::TransformerBatch, columns: &tch::Tensor) -> tch::Tensor {
        StackedTransformer::test_losses(self, batch, columns)
    }
}

/// The arms still training, as one stacked network and optimizer (stacked-lockstep-arms.md): slot `k`
/// is the `k`-th arm of `active`, reading label column `columns[k]`. A round steps every slot at once
/// (`round::Arms`); every file stays the arm's own, written from its slice (`arm_slice`).
pub(crate) struct Stack<N: StackedModel> {
    pub(crate) network: N,
    pub(crate) optimizer: StackedAdamW,
    /// `[slots]`, `Int64`, on the device.
    pub(crate) columns: tch::Tensor,
    pub(crate) files: Vec<SlotFiles>,
    pub(crate) steps_per_round: u64,
    /// Whether a backup writes the stream's cursors and the rows into the round. The NNUE's flat
    /// stream derives both from the step count, so writing them would only make a second copy to go
    /// stale; the transformer's bucket stream cannot, since a bucket's short batch makes the rows
    /// read differ from the steps taken times the batch size.
    pub(crate) persists_position: bool,
}

impl<N: StackedModel> Stack<N> {
    /// Keeps only the slots `keep`, in that order: the arms that finished leave.
    fn retain(&mut self, keep: &[usize]) {
        self.network.retain(keep);
        self.optimizer.retain(keep, self.network.parameters());
        let index = stacked_network::retained_index(keep, self.files.len(), self.columns.device());
        self.columns = self.columns.index_select(0, &index);
        let mut slot = 0;
        self.files.retain(|_| {
            let kept = keep.contains(&slot);
            slot += 1;
            kept
        });
    }

    /// Writes slot `slot`'s weights and optimizer state to its arm's `model.vs` and `optimizer.opt`.
    fn save_slot(&self, slot: usize) {
        let files = &self.files[slot];
        arm_slice::save_arm(self.network.parameters(), slot as i64, &files.model_path);
        self.optimizer.save_arm(slot, &files.optimizer_path);
    }
}

impl<N: StackedModel> round::Arms<N::Batch> for Stack<N> {
    fn count(&self) -> usize {
        self.files.len()
    }

    // The run step, not the optimizer's count: where the arm stands in the stream, which a rollback
    // to an earlier best does not rewind.
    fn step_count(&self, arm: usize) -> u64 {
        self.optimizer.run_step(arm) as u64
    }

    fn train_step(&mut self, batch: &N::Batch, stepping: &[bool]) -> tch::Tensor {
        self.optimizer.set_stepping(stepping);
        // The sum, not the mean: each slot's gradient is then its own loss's (`stacked_network.rs`).
        // A held slot's gradient is computed and discarded by the optimizer.
        let losses = self.network.training_losses(batch, &self.columns);
        self.optimizer.zero_grad();
        losses.sum(tch::Kind::Float).backward();
        self.optimizer.step();
        let losses = losses.detach();
        if stepping.iter().all(|&steps| steps) {
            return losses;
        }
        let flags: Vec<f32> = stepping.iter().map(|&steps| if steps { 1.0 } else { 0.0 }).collect();
        losses * tch::Tensor::from_slice(&flags).to_device(self.columns.device())
    }

    fn test_losses(&self, batch: &N::Batch) -> tch::Tensor {
        self.network.test_losses(batch, &self.columns)
    }

    // Without `persists_position`, `cursors` and `rows_done` are persisted as empty and zero rather
    // than as `run_round` reports them: both are derived from the step count on the way back in, and
    // writing a real value into a field nothing reads is how a stale second copy gets mistaken for
    // state later. `PgnTrainStream::cursors` returns an empty vector for the same reason.
    fn back_up(&mut self, arm: usize, cursors: &[u64], rows_done: u64, loss_sum: f64, duration_secs: f64) {
        self.save_slot(arm);
        let (train_cursors, rows_done) = if self.persists_position { (cursors.to_vec(), rows_done) } else { (Vec::new(), 0) };
        RoundProgress { train_cursors, rows_done, loss_sum, train_duration_secs: duration_secs }
            .save(&self.files[arm].round_progress_path);
        let steps_into_round = self.step_count(arm) % self.steps_per_round;
        // Not "periodic": `run_round` also calls this the moment a stop is seen.
        log::debug!(
            "  lambda {}: backup saved ({steps_into_round}/{} steps into this round)",
            self.files[arm].lambda, self.steps_per_round
        );
    }
}

/// The stack of `arms[active]` over `network`, on `device`: each slot resumed from its arm's backup, or
/// started from `starting_weights`, with its rate, warmup and `bounds` set.
///
/// `starting_weights` is called at most once, when the first slot starts fresh, and every fresh slot
/// gets the same weights from it: every lambda starts from the same point, so lambdas are compared and
/// not starting points.
#[allow(clippy::too_many_arguments)]
pub(crate) fn open_stack<N: StackedModel>(
    config: &NetConfig, network: N, bounds: Vec<(&'static str, f64)>, arms: &[Arm], active: &[usize],
    starting_weights: impl FnOnce() -> std::collections::HashMap<String, tch::Tensor>, persists_position: bool,
    device: tch::Device,
) -> Stack<N> {
    let rates = &config.training.learning_rate;
    let warmup_rounds = config.training.warmup_rounds.expect("net_config::load already validated this is Some");
    let steps_per_round = config.training.steps_per_round;
    let mut optimizer = StackedAdamW::new(network.parameters(), rates[0]);
    // Set on resumed slots too: the ramp is derived from each restored step count, so a run
    // interrupted inside it continues it, and one past it is unaffected.
    optimizer.set_warmup_steps((warmup_rounds * steps_per_round) as i64);
    let mut starting_weights = Some(starting_weights);
    let mut starting: Option<std::collections::HashMap<String, tch::Tensor>> = None;
    for (slot, &index) in active.iter().enumerate() {
        let arm = &arms[index];
        if std::path::Path::new(&arm.model_path).exists() {
            arm_slice::load_arm(network.parameters(), slot as i64, &arm.model_path);
            optimizer.load_arm(slot, &arm.optimizer_path);
            // As the round lines count: the round under way, and how far into it, as a backup says.
            let steps = optimizer.run_step(slot) as u64;
            log::info!(
                "lambda {}: resumed from {} in round {} ({}/{steps_per_round} steps into it)",
                arm.lambda,
                arm.run_dir,
                steps / steps_per_round + 1,
                steps % steps_per_round
            );
        } else {
            let weights = starting
                .get_or_insert_with(|| (starting_weights.take().expect("called once, the first time"))());
            for (name, value) in weights.iter() {
                arm_slice::fill_slice(&network.parameters()[name], slot as i64, value, name);
            }
        }
        optimizer.set_learning_rate(slot, rates[arm.stages.stage]);
    }
    // After the weights load, so a start from a network trained without the bounds begins inside them.
    for (name, bound) in bounds {
        optimizer.bound(name, bound);
    }
    let columns: Vec<i64> = active.iter().map(|&index| arms[index].column as i64).collect();
    let files = active
        .iter()
        .map(|&index| {
            let arm = &arms[index];
            SlotFiles {
                lambda: arm.lambda,
                model_path: arm.model_path.clone(),
                optimizer_path: arm.optimizer_path.clone(),
                round_progress_path: arm.round_progress_path.clone(),
            }
        })
        .collect();
    Stack {
        network,
        optimizer,
        columns: tch::Tensor::from_slice(&columns).to_device(device),
        files,
        steps_per_round,
        persists_position,
    }
}

/// What differs between the tracks for `train_in_lockstep`, beyond its type parameters.
pub(crate) struct LockstepParts<'a> {
    /// The dataset trained on, as `run_config.txt` freezes it.
    pub(crate) source: &'a str,
    /// Every lambda of the network, the trained ones included: the round block shows them all.
    pub(crate) grid: Vec<ArmLambda>,
    /// Each lambda's run directory.
    pub(crate) lambda_dir: &'a dyn Fn(ArmLambda) -> String,
    /// `Stack::persists_position`: whether the stream's position is read back from a backup rather
    /// than derived from the step count.
    pub(crate) persists_position: bool,
    pub(crate) device: tch::Device,
}

/// The lambdas of `lambdas`, every one's run unfinished, trained in lockstep (lockstep-lambdas.md),
/// for either track: one stream over the dataset, each batch carrying every lambda's labels, built and
/// uploaded once and stepped on by the stack of every lambda's network (`Stack`). Each lambda keeps
/// everything a run of its own has -- its weights, optimizer, rates, patience, history and
/// checkpoints -- and leaves the lockstep when its stop rule holds, `finish` ending its run, while the
/// others go on. Returns whether every one got there: `false` after a Ctrl+C, which leaves the runs to
/// resume when the server starts again.
///
/// What the tracks supply: the test readers, `open_stack` (the stacked network of `arms[active]`),
/// `open_stream` (the training stream, given the stream's step count and, when the position persists,
/// the cursors a backup at that count wrote), `to_batch`, the size of a kept test batch, and `finish`;
/// `after_round`, called after every round once every arm's files are written, the smithy's moment
/// to copy them (none of them being replaced meanwhile, which Windows would refuse).
#[allow(clippy::too_many_arguments)]
pub(crate) fn train_in_lockstep<N: StackedModel, H: Send, T: train_stream::BatchSource + Send, S: train_stream::TrainStream + Send>(
    config: &NetConfig, parts: &LockstepParts, lambdas: &[ArmLambda], train_rows: u64, test_readers: &mut [T],
    open_stack: impl FnOnce(&[Arm], &[usize]) -> Stack<N>, open_stream: impl FnOnce(u64, Option<Vec<u64>>) -> S,
    to_batch: &round::BatchBuilder<impl Fn(&[CacheRecord]) -> H + Sync, impl Fn(&H) -> N::Batch>, host_bytes: fn(&H) -> usize,
    finish: impl Fn(&Arm), after_round: impl Fn(), stop_requested: &AtomicBool,
) -> bool {
    if lambdas.is_empty() {
        return true;
    }
    // No earlier training's pass shown before this one's first; none left once it returns.
    crate::rounds_display::end_pass();
    let _pass_end = crate::rounds_display::PassEnd;
    let source = parts.source;
    log::info!("training lambdas {} in lockstep on the dataset of {source}", preprocessing_config::lambdas_text(lambdas));
    let warmup_rounds = config.training.warmup_rounds.expect("net_config::load already validated this is Some");
    let patience_rounds = config.training.patience_rounds.expect("net_config::load already validated this is Some");
    let rates = config.training.learning_rate.clone();
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
    log::info!(
        "rates {}: {warmup_rounds} warmup rounds, then {patience_rounds} rounds without a new best move to the next \
         rate, and at the last one end the run",
        rates.iter().map(|rate| format!("{rate:e}")).collect::<Vec<String>>().join(" ")
    );

    log::info!("device = {:?}", parts.device);
    let mut arms: Vec<Arm> = lambdas
        .iter()
        .enumerate()
        .map(|(column, &lambda)| {
            open_arm(config, source, (parts.lambda_dir)(lambda), lambda, column, train_rows, test_rows)
        })
        .collect();
    // The arms still training, by index into `arms`, ascending: a run interrupted between meeting its
    // rule and finishing has met it already, so it finishes now and never joins.
    let mut active: Vec<usize> = Vec::new();
    for (index, arm) in arms.iter().enumerate() {
        if arm.stages.finished {
            finish(arm);
        } else {
            active.push(index);
        }
    }
    if active.is_empty() {
        return true;
    }
    let mut stack = open_stack(&arms, &active);
    // One stream for every arm, standing at the lowest arm's count. Counts differ after a crash between
    // two arms' backups, or for runs begun one after another: an arm further ahead skips the batches it
    // has trained on until the stream reaches it, then joins (`round::run_round`).
    let steps_done = (0..active.len()).map(|slot| stack.optimizer.run_step(slot) as u64).min().expect("an active arm");
    for (slot, &index) in active.iter().enumerate() {
        let arm_steps = stack.optimizer.run_step(slot) as u64;
        if arm_steps > steps_done {
            log::info!(
                "lambda {} stands at step {arm_steps}, ahead of the stream at step {steps_done}: it joins when the \
                 stream reaches it",
                arms[index].lambda
            );
        }
    }
    // A round's running loss sum is never derivable from the step count, one per arm; the seconds are
    // the lowest arm's, the round being the stream's. With a derived position, the rows into an arm's
    // round are its steps into it times the batch size; with a persisted one, its backup's, along with
    // the stream's cursors of an arm at the stream's count.
    let mut round_duration_secs = 0.0;
    let mut loss_sums = Vec::new();
    let mut rows_done = Vec::new();
    let mut stream_cursors = None;
    for (slot, &index) in active.iter().enumerate() {
        let arm = &arms[index];
        let arm_steps = stack.optimizer.run_step(slot) as u64;
        if !std::path::Path::new(&arm.round_progress_path).exists() {
            rows_done.push(if parts.persists_position { 0 } else { (arm_steps % steps_per_round) * batch_size as u64 });
            loss_sums.push(0.0);
            continue;
        }
        let progress = RoundProgress::load(&arm.round_progress_path);
        if arm_steps == steps_done {
            round_duration_secs = progress.train_duration_secs;
            if parts.persists_position && !progress.train_cursors.is_empty() {
                stream_cursors = Some(progress.train_cursors.clone());
            }
        }
        rows_done.push(if parts.persists_position { progress.rows_done } else { (arm_steps % steps_per_round) * batch_size as u64 });
        loss_sums.push(progress.loss_sum);
    }

    // The test batches this run keeps across rounds, filled by its first test pass (`test_cache.rs`).
    let mut kept_test_batches = test_cache::TestCache::new(
        test_cache::budget().unwrap_or_else(|error| {
            log::warn!("test cache: {error}, so every test pass prepares its batches");
            0
        }),
        host_bytes,
    );

    // Step 0, every run's first best: the generation's starting weights, measured before any training.
    // A high first rate can damage a promoted champion's weights; when patience runs out, an arm rolls
    // back to its best, so with step 0 among the bests a rate that only made things worse is undone
    // back to the champion itself. Recorded as a round of its own, with no train loss, and
    // checkpointed like any round, so that a run never beating it exports them as its candidate.
    // An arm that has stepped, or already recorded step 0 before a crash, is not measured again.
    let unmeasured: Vec<usize> = (0..active.len())
        .filter(|&slot| {
            let arm = &arms[active[slot]];
            stack.optimizer.run_step(slot) == 0 && !has_history_row(&arm.history_path, 0)
        })
        .collect();
    if !unmeasured.is_empty() {
        let (test_losses, test_duration_secs) = round::run_test_round(
            &stack, &mut *test_readers, Some(&mut kept_test_batches), batch_size, to_batch,
            &mut |label: &str, processed: u64, total: u64| print_progress(label, processed, total, "round 0"),
        );
        for &slot in &unmeasured {
            let arm = &mut arms[active[slot]];
            // History first, checkpoint second, as at every round's end.
            history::append(
                &arm.history_path,
                &history::EvalRecord {
                    step: 0,
                    learning_rate: stack.optimizer.learning_rate(slot),
                    train_loss: f64::NAN,
                    test_loss: test_losses.global[slot],
                    train_duration_secs: 0.0,
                    test_duration_secs,
                },
            );
            history::append_pieces(&pieces_path(&arm.run_dir), 0, &round::losses_of_arm(&test_losses.by_pieces, slot));
            arm_slice::save_arm(stack.network.parameters(), slot as i64, &checkpoint_store::checkpoint_path(&arm.checkpoints_dir, 0));
            arm.stages = replay_stages(&arm.history_path, steps_per_round, warmup_rounds, patience_rounds, rates.len());
            log::info!("lambda {}: starting weights measured, test loss {:.6}, the run's first best", arm.lambda, test_losses.global[slot]);
        }
    }

    let mut train_stream = open_stream(steps_done, stream_cursors);
    for &index in &active {
        let stage = arms[index].stages.stage;
        log::info!("lambda {}: training at rate {:e}, stage {} of {}", arms[index].lambda, rates[stage], stage + 1, rates.len());
    }

    // Only the first round of this process resumes a partially-done round; every later one starts
    // its counters at zero, which is exactly what `None` means to `run_round`.
    let mut resume =
        Some(round::RoundResume { rows_done, loss_sums, duration_secs: round_duration_secs });

    loop {
        // The stream's count: the lowest arm's.
        let step_at_round_start =
            (0..active.len()).map(|slot| stack.optimizer.run_step(slot) as u64).min().expect("an active arm");
        let steps_into_round = step_at_round_start % steps_per_round;
        if steps_into_round > 0 {
            log::info!("resuming an interrupted evaluation round: {steps_into_round} steps already accounted for");
        }
        let round_label = format!("round {}", step_at_round_start / steps_per_round + 1);

        let cache_was_filled = kept_test_batches.is_filled();
        // Slot `k` of the stack is `active[k]`, the order of the round's losses.
        let outcome = round::run_round(
            &mut stack, &mut train_stream, &mut *test_readers, Some(&mut kept_test_batches),
            steps_per_round, batch_size, resume.take(), game_labels::BACKUP_INTERVAL, to_batch,
            &mut |label: &str, processed: u64, total: u64| print_progress(label, processed, total, &round_label),
            stop_requested,
        );
        let Some(outcome) = outcome else {
            log::info!("training stopped mid-round; it resumes from its backups");
            return false;
        };
        if !cache_was_filled {
            log::info!(
                "test cache: {:.2} GB, {} of {} positions (budget {:.2} GB)",
                test_cache::gigabytes(kept_test_batches.bytes()), kept_test_batches.rows(), test_rows,
                test_cache::gigabytes(kept_test_batches.budget())
            );
        }

        // The round of every arm that completed it, in `active`'s order, the order of the round's
        // losses. An arm still ahead of the stream, or that had recorded this boundary before a crash,
        // did not, and records nothing.
        let mut finished = Vec::new();
        let mut switched = Vec::new();
        let mut completed = Vec::new();
        let mut step = step_at_round_start;
        for (slot, &index) in active.iter().enumerate() {
            if !outcome.completed[slot] {
                continue;
            }
            let arm = &mut arms[index];
            step = stack.optimizer.run_step(slot) as u64;
            let (train_loss, test_loss) = (outcome.train_loss[slot], outcome.test_loss[slot]);
            let learning_rate = stack.optimizer.learning_rate(slot);
            // History first, archive second, prune third: the stages replayed from the history name the
            // best, the one checkpoint kept, so the row has to exist before the file it describes. A
            // crash between the first two leaves a history row with no checkpoint, which is harmless; a
            // crash before the prune leaves an extra checkpoint, which the next round's prune deletes.
            history::append(
                &arm.history_path,
                &history::EvalRecord {
                    step,
                    learning_rate,
                    train_loss,
                    test_loss,
                    train_duration_secs: outcome.train_duration_secs,
                    test_duration_secs: outcome.test_duration_secs,
                },
            );
            // For reading only, after the row the run replays from.
            history::append_pieces(&pieces_path(&arm.run_dir), step, &round::losses_of_arm(&outcome.test_loss_by_pieces, slot));
            let checkpoint_archive_path = checkpoint_store::checkpoint_path(&arm.checkpoints_dir, step);
            arm_slice::save_arm(stack.network.parameters(), slot as i64, &checkpoint_archive_path);
            let previous_stage = arm.stages.stage;
            arm.stages = replay_stages(&arm.history_path, steps_per_round, warmup_rounds, patience_rounds, rates.len());
            let stages = arm.stages;
            checkpoint_store::keep_only(&arm.checkpoints_dir, stages.best_step);
            completed.push(index);
            // A new best keeps its optimizer beside its checkpoint, for a rollback to it. Through a
            // staging name, so that a crash never leaves a half-written one to roll back to.
            if stages.best_step == step {
                let staging_path = format!("{}.partial", arm.best_optimizer_path);
                stack.optimizer.save_arm(slot, &staging_path);
                std::fs::rename(&staging_path, &arm.best_optimizer_path)
                    .unwrap_or_else(|error| panic!("failed to rename {staging_path} to {}: {error}", arm.best_optimizer_path));
            }
            let switches = !stages.finished && stages.stage != previous_stage;
            if switches {
                stack.optimizer.set_learning_rate(slot, rates[stages.stage]);
            }
            // Every switch rolls the arm back to its best, so a rate that only made things worse leaves
            // nothing behind. Keyed on the switch's step rather than on `switches`: a round redone after a
            // crash re-appends its row, the replay then finds the stage already moved on, and the
            // rollback must still happen. It sets the whole slot from files, so doing it twice is the
            // same as once.
            if !stages.finished && stages.switch_step == step {
                roll_back(&mut stack, slot, arm);
            }

            // The backup last, so that one taken at a switch's step already holds what the switch did.
            stack.save_slot(slot);
            // The stream's cursors at the boundary: empty for a derived position
            // (`PgnTrainStream::cursors`).
            RoundProgress { train_cursors: outcome.cursors.clone(), rows_done: 0, loss_sum: 0.0, train_duration_secs: 0.0 }
                .save(&arm.round_progress_path);

            if stages.finished {
                finished.push(index);
                continue;
            }
            // Announced after the round's line, like a finish.
            if switches {
                let back_to = if stages.best_step == 0 {
                    "the starting weights, warming up again".to_string()
                } else {
                    format!("round {}", stages.best_step / steps_per_round)
                };
                switched.push(format!(
                    "lambda {}: no new best since round {}, back to {back_to}, moving from rate {:e} to {:e}, stage {} of {}",
                    arm.lambda, stages.best_step / steps_per_round, rates[previous_stage], rates[stages.stage], stages.stage + 1,
                    rates.len()
                ));
            }
        }
        // The round as one event: its time, the grid's best so far, and the lambdas that set a new best
        // in it; every lambda's numbers are the page's training table. Then its switches and finishes.
        if !completed.is_empty() {
            let mut line = timing_line(step / steps_per_round, outcome.train_duration_secs, outcome.test_duration_secs);
            if let Some((lambda, loss)) = grid_best(parts, steps_per_round, warmup_rounds, patience_rounds, rates.len()) {
                line.push_str(&format!("; best lambda {lambda}, loss {loss:.6}"));
            }
            let new_bests: Vec<String> =
                completed.iter().filter(|&&index| arms[index].stages.best_step == step).map(|&index| arms[index].lambda.to_string()).collect();
            if !new_bests.is_empty() {
                line.push_str(&format!("; a new best for lambda {}", new_bests.join(", ")));
            }
            log::info!("{line}");
        }
        for line in &switched {
            log::info!("{line}");
        }
        for &index in &finished {
            finish(&arms[index]);
        }
        // A finished arm leaves the lockstep and the stack; the stream goes on for the others, its
        // column unread.
        if finished.len() == active.len() {
            return true;
        }
        if !finished.is_empty() {
            let keep: Vec<usize> = (0..active.len()).filter(|&slot| !finished.contains(&active[slot])).collect();
            stack.retain(&keep);
            active.retain(|index| !finished.contains(index));
        }
        // Every arm's files written and none being written: the one moment they may be copied.
        after_round();
        if stop_requested.load(Ordering::SeqCst) {
            log::info!("training stopped after step {step}; it resumes from there");
            return false;
        }
    }
}

/// Where the run in `history_path` is in its list of rates, replayed from its recorded test losses:
/// stage 0 before any round.
fn replay_stages(
    history_path: &str, steps_per_round: u64, warmup_rounds: u64, patience_rounds: u64, stage_count: usize,
) -> stages::Stages {
    let rounds = recorded_rounds(history_path);
    stages::stage_after(&rounds, steps_per_round, warmup_rounds, patience_rounds, stage_count)
}

/// The latest round of `history_path` and the stages replayed up to it; `None` before its first
/// round. The status page's training table, and each round's event line (`grid_best`).
pub(crate) fn latest_round(
    history_path: &str, steps_per_round: u64, warmup_rounds: u64, patience_rounds: u64, stage_count: usize,
) -> Option<(history::EvalRecord, stages::Stages)> {
    if !std::path::Path::new(history_path).exists() {
        return None;
    }
    let mut records = history::read_all(history_path);
    let rounds: Vec<(u64, f64)> = records.iter().map(|record| (record.step, record.test_loss)).collect();
    let stages = stages::stage_after(&rounds, steps_per_round, warmup_rounds, patience_rounds, stage_count);
    Some((records.pop()?, stages))
}

/// Every round of `history_path` as `(step, test_loss)`, in the order recorded; none without the file.
fn recorded_rounds(history_path: &str) -> Vec<(u64, f64)> {
    if !std::path::Path::new(history_path).exists() {
        return Vec::new();
    }
    history::read_all(history_path).iter().map(|record| (record.step, record.test_loss)).collect()
}

/// Rolls `arm`, slot `slot` of `stack`, back to its best: the best step's checkpoint into its weights,
/// and its optimizer as it was there, `best.opt`, or a fresh one for a best at step 0, which then warms
/// up again. Its run step is kept: the stream does not rewind. Sets the whole slot from files, whatever
/// it held, so a second call changes nothing.
fn roll_back<N: StackedModel>(stack: &mut Stack<N>, slot: usize, arm: &Arm) {
    let best_step = arm.stages.best_step;
    let checkpoint = checkpoint_store::checkpoint_path(&arm.checkpoints_dir, best_step);
    assert!(
        std::path::Path::new(&checkpoint).exists(),
        "lambda {}: its best, step {best_step}, has no checkpoint at {checkpoint} to roll back to",
        arm.lambda
    );
    arm_slice::load_arm(stack.network.parameters(), slot as i64, &checkpoint);
    if best_step == 0 {
        stack.optimizer.reset_arm(slot);
    } else {
        assert!(
            std::path::Path::new(&arm.best_optimizer_path).exists(),
            "lambda {}: its best, step {best_step}, has no {} to roll back to (a run begun before rollbacks \
             existed keeps no optimizer at its best)",
            arm.lambda,
            arm.best_optimizer_path
        );
        stack.optimizer.restore_arm(slot, &arm.best_optimizer_path);
    }
}

/// A lambda run's `pieces.tsv`, beside its `history.tsv`: each round's test loss per piece count
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

/// The grid's lambda with the lowest best loss so far, and that loss, from every lambda's history: the
/// whole grid, not only the arms, since a lambda that finished in an earlier invocation is not one.
/// `None` before any round.
fn grid_best(parts: &LockstepParts, steps_per_round: u64, warmup_rounds: u64, patience_rounds: u64, stage_count: usize) -> Option<(ArmLambda, f64)> {
    parts
        .grid
        .iter()
        .filter_map(|&lambda| {
            let history_path = format!("{}/history.tsv", (parts.lambda_dir)(lambda));
            let (_, stages) = latest_round(&history_path, steps_per_round, warmup_rounds, patience_rounds, stage_count)?;
            Some((lambda, stages.best_loss))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_stages_is_the_first_rate_for_a_run_with_no_history() {
        let stages = replay_stages("data/chess/networks/train-main-test-no-such-run/history.tsv", 512, 2, 8, 6);
        assert_eq!((stages.stage, stages.finished), (0, false));
    }

    #[test]
    fn replay_stages_reads_the_recorded_test_losses() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-main-test-{pid}-history.tsv"));
        let path = path.to_str().unwrap();
        std::fs::remove_file(path).ok();
        // No warmup, patience of 2 rounds of 512: best at 1024, flat at 1536 and 2048, so the switch
        // comes at 2048.
        for (step, test_loss) in [(512u64, 0.31f64), (1024, 0.29), (1536, 0.30), (2048, 0.30)] {
            history::append(
                path,
                &history::EvalRecord {
                    step, learning_rate: 1.0e-3, train_loss: 0.3, test_loss, train_duration_secs: 1.0,
                    test_duration_secs: 1.0,
                },
            );
        }

        let stages = replay_stages(path, 512, 0, 2, 3);
        assert_eq!((stages.stage, stages.best_step, stages.best_loss), (1, 1024, 0.29));

        std::fs::remove_file(path).ok();
    }

    use crate::bucketed_train_stream::BucketedTrainStream;
    use crate::candidates::export_best;
    use crate::feature_set::FeatureSet;
    use crate::grouped_test_set::GroupedTestSet;
    use crate::pgn_stream::StreamFile;
    use crate::pgn_train_stream::PgnTrainStream;
    use crate::train_stream::TrainStream;
    use crate::{net_config, nnue_network, transformer_network};
    use temper_chess::pipeline::generation;

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
    /// training segments of labelled games, a rook up so that no position is K vs K: the config, the
    /// segments, and the test set the server would build from them, read once and grouped by piece count.
    fn fixture(test: &str, config: &str, lambdas: &[f64]) -> (String, NetConfig, Vec<StreamFile>, GroupedTestSet) {
        let dir = std::env::temp_dir().join(format!("smithy-lockstep-{test}-{}", std::process::id()));
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
        let mut test = PgnTrainStream::open_one_pass(files.clone(), lambdas, 1);
        let mut records = Vec::new();
        loop {
            let batch = test.next_batch(64);
            if batch.is_empty() {
                break;
            }
            records.extend(batch);
        }
        let grouped = GroupedTestSet::load(&mut Flat(records), config.training.test_batch_size as usize);
        (dir, config, files, grouped)
    }

    /// Every lambda's run ended, each with its history, its rounds at the same steps as the others'.
    fn assert_lockstep_runs(dir: &str, lambdas: &[f64], engine_file: bool) {
        let histories: Vec<Vec<u64>> = lambdas
            .iter()
            .map(|&lambda| {
                let run_dir = generation::lambda_dir(dir, ArmLambda::Constant(lambda));
                assert!(crate::candidates::read_candidate_step(&run_dir).is_ok(), "lambda {lambda} exported its candidate");
                assert_eq!(std::path::Path::new(&generation::candidate_network_path(&run_dir)).exists(), engine_file);
                let records = history::read_all(&format!("{run_dir}/history.tsv"));
                assert!(records.len() > 1, "lambda {lambda} recorded its rounds");
                assert!(records[0].train_loss.is_nan(), "lambda {lambda}: step 0 is measured, not trained");
                records.iter().map(|record| record.step).collect()
            })
            .collect();
        // One stream: every round of either lambda ends at the same steps, the shorter run's a prefix.
        let shorter = histories[0].len().min(histories[1].len());
        assert_eq!(histories[0][..shorter], histories[1][..shorter]);
        // Round 0 at step 0, the starting weights, then one round every 2 steps.
        assert!(histories.iter().all(|steps| steps.iter().enumerate().all(|(round, &step)| step == 2 * round as u64)));
    }

    /// The server's training of an NNUE, as `Run::train` wires it: two lambdas in lockstep from random
    /// weights, each to its stop with its own history and candidate.
    #[test]
    fn two_lambdas_train_in_lockstep_to_their_candidates() {
        let lambdas = [0.70, 0.80];
        let (dir, config, files, grouped) = fixture(
            "nnue",
            "labels: lc0\n\
             architecture:\n  kind: psq\n  embedding_dim: 8\n  value_head:\n    hidden_dim: [4]\n\
             label:\n  lambda: [0.70, 0.80]\n\
             training:\n  learning_rate: [1.0e-2, 1.0e-3]\n  training_batch_size: 8\n  test_batch_size: 8\n  \
             steps_per_round: 2\n  warmup_rounds: 0\n  patience_rounds: 1\n",
            &lambdas,
        );
        let device = tch::Device::Cpu;
        let arms = lambdas.map(ArmLambda::Constant);
        let lambda_dir = |lambda: ArmLambda| generation::lambda_dir(&dir, lambda);
        let parts = LockstepParts { source: "fixture", grid: arms.to_vec(), lambda_dir: &lambda_dir, persists_position: false, device };
        let to_batch = round::BatchBuilder {
            prepare: |records: &[CacheRecord]| batch::host_batch(records, FeatureSet::Psq),
            upload: |host: &batch::HostBatch| batch::upload(host, device),
        };
        let stack_of = |arms: &[Arm], active: &[usize]| {
            let network = StackedNetwork::new(FeatureSet::Psq, &config.architecture, active.len() as i64, device);
            let bounds = network.weight_bounds();
            let starting_weights = || {
                let vs = tch::nn::VarStore::new(device);
                let _ = nnue_network::Network::of(&vs.root(), FeatureSet::Psq, &config.architecture);
                vs.variables()
            };
            open_stack(&config, network, bounds, arms, active, starting_weights, false, device)
        };
        let train_rows = files.iter().map(|file| file.positions).sum();
        let stream_from = |steps_done: u64, _: Option<Vec<u64>>| {
            PgnTrainStream::open(files.clone(), &lambdas, 16, steps_done * 8, 1)
        };
        let finish = |arm: &Arm| {
            export_best(&config, &arm.run_dir, arm.lambda, &arm.stages);
        };
        let stop_requested = AtomicBool::new(false);
        let finished = train_in_lockstep(
            &config, &parts, &arms, train_rows, &mut [grouped], stack_of, stream_from, &to_batch, batch::HostBatch::bytes, finish,
            || {}, &stop_requested,
        );
        let outcome = std::panic::catch_unwind(|| {
            assert!(finished, "no stop was requested, so every lambda ends");
            assert_lockstep_runs(&dir, &lambdas, true);
        });
        std::fs::remove_dir_all(&dir).ok();
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }

    /// The server's training of a transformer, as `Run::train` wires it: two lambdas in lockstep,
    /// stacked, with dropout, its stream one shuffle buffer per piece count, each lambda to its stop with
    /// its own history and its `candidate.txt` alone, since the engine cannot run it.
    #[test]
    fn a_transformers_lambdas_train_in_lockstep_to_their_stops() {
        let lambdas = [0.70, 0.80];
        let (dir, config, files, grouped) = fixture(
            "transformer",
            "labels: lc0\n\
             architecture:\n  kind: transformer\n  embedding_dim: 8\n  value_head:\n    hidden_dim: [4]\n  num_layers: 2\n  \
             num_heads: 2\n  feedforward_dim: 16\n  dropout: 0.1\n\
             label:\n  lambda: [0.70, 0.80]\n\
             training:\n  attention: full\n  learning_rate: [1.0e-2, 1.0e-3]\n  training_batch_size: 8\n  test_batch_size: 8\n  \
             steps_per_round: 2\n  shuffle_buffer: 16\n  warmup_rounds: 0\n  patience_rounds: 1\n",
            &lambdas,
        );
        let device = tch::Device::Cpu;
        let attention = config.training.attention.unwrap();
        let arms = lambdas.map(ArmLambda::Constant);
        let lambda_dir = |lambda: ArmLambda| generation::lambda_dir(&dir, lambda);
        let parts = LockstepParts { source: "fixture", grid: arms.to_vec(), lambda_dir: &lambda_dir, persists_position: false, device };
        let to_batch = round::BatchBuilder {
            prepare: |records: &[CacheRecord]| transformer_batch::host_batch(records, attention),
            upload: |host: &transformer_batch::HostTransformerBatch| transformer_batch::upload(host, device),
        };
        let stack_of = |arms: &[Arm], active: &[usize]| {
            let network = StackedTransformer::new(&config.architecture, active.len() as i64, device);
            let starting_weights = || {
                let vs = tch::nn::VarStore::new(device);
                let _ = transformer_network::TransformerNetwork::new(&vs.root(), &config.architecture);
                vs.variables()
            };
            open_stack(&config, network, Vec::new(), arms, active, starting_weights, false, device)
        };
        let train_rows = files.iter().map(|file| file.positions).sum();
        let stream_from = |steps_done: u64, _: Option<Vec<u64>>| {
            BucketedTrainStream::open(files.clone(), &lambdas, 16, 8, steps_done * 8, 1)
        };
        let finish = |arm: &Arm| {
            export_best(&config, &arm.run_dir, arm.lambda, &arm.stages);
        };
        let stop_requested = AtomicBool::new(false);
        let finished = train_in_lockstep(
            &config, &parts, &arms, train_rows, &mut [grouped], stack_of, stream_from, &to_batch,
            transformer_batch::HostTransformerBatch::bytes, finish, || {}, &stop_requested,
        );
        let outcome = std::panic::catch_unwind(|| {
            assert!(finished, "no stop was requested, so every lambda ends");
            assert_lockstep_runs(&dir, &lambdas, false);
        });
        std::fs::remove_dir_all(&dir).ok();
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }
}
