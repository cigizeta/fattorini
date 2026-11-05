# Rollback to the best checkpoint: implementation plan

**Status (2026-10-16): steps 1 to 7 implemented, `cargo test --release -p smithy` passing on the Lichess machine.
Step 6's resume rule came out simpler than planned: the rollback is keyed on `stages.switch_step ==
step` at a round's end, which also covers a round redone after a crash (its row is re-appended and
the replay finds the switch at the same step). No `rolled_back_at` field: `Stages` gained
`warmup_end` and `last_step` instead. Step 7 (messages) was done within step 6. Step 8: tests pass
on the training machine too (335, after updating zelda's config test to its Lc0 round and patience); amanda's
generation 3 set aside (its `lambda-*` deleted) to retrain under this rule.**

Implements rollback-to-best.md. Smithy only; the engine is untouched. Every step keeps
`cargo test --release -p smithy` passing.

## Step 1: two counters in `StackedAdamW` (`stacked_adamw.rs`)

- Add `run_steps: Vec<i64>` beside `counts` (the optimizer's `t`). `step()` increments both for
  every stepping arm, so they stay equal until a rollback.
- `step_count(arm)` keeps meaning `t` (bias correction, warmup); add `run_step(arm)` for the run's
  step.
- `save_arm` also writes a scalar `"step"` (the run's step) beside `"t"`, in the same file, so the two
  can never disagree after a crash.
- `load_arm` reads `"step"` when present and falls back to `"t"` when absent: backups written before
  this change hold one counter, and there the two were equal.
- New `restore_arm(arm, path)`: like `load_arm`, but sets moments and `t` only, leaving `run_steps[arm]`
  as it is. New `reset_arm(arm)`: zero moments, `t = 0`, `run_steps[arm]` kept (the rollback to
  step 0).
- `retain` keeps `run_steps` in step with `counts`.

Tests: both counters advance together; `restore_arm` and `reset_arm` leave the run's step alone and
put `t` back; a file without `"step"` loads with `run_step == t`; warmup reruns after `reset_arm`
(the first step's displacement equals a fresh optimizer's at the arm's current rate).

## Step 2: the run's step drives the loop (`main.rs`, `round.rs`)

- `Arms::step_count` becomes the run's step: `Stack::step_count` returns `optimizer.run_step(arm)`.
  Its doc already says "where it stands in its sequence of batches", which is the run's step.
- Every other reader of `stack.optimizer.step_count(slot)` in the lockstep loop (the stream's
  count, `step_at_round_start`, the history row's `step`, the checkpoint's name, the resume message
  in `open_stack`) switches to `run_step`.
- The test-only `Trainee` arms keep `AdamW::step_count`: they never roll back.

Until step 4 nothing rolls back, so behaviour is unchanged: the existing tests are the check.

## Step 3: step 0 is the first best

- `round.rs`: make the test pass callable alone (a `pub fn run_test_only` around `run_test_pass`, with
  the kept test cache), returning each arm's test loss.
- `train_in_lockstep`: after `open_stack`, for every active arm whose `history.tsv` has no row at
  step 0 and whose run step is 0, run one test pass over those arms and append a row: step 0, the
  current rate, train loss `NaN` (no training yet), the test loss, the test seconds. Then replay the
  stages. A crash before the append just redoes the pass.
- **Changed while implementing:** step 0 is checkpointed like any round (history row first, then
  `checkpoints/step_000000000.vs`), so `finish_training`, `export_candidate`, `check` and the arena
  need no special case: a best at step 0 exports the starting weights through the usual path.
- `round_line` shows round 0 for that row; check that a `NaN` train loss prints sensibly and that
  `history::read_all` and `prune_checkpoints` accept the row (prune reads only steps that have files).
- `stages.rs`: `NaN` is never a best already; a train loss of `NaN` is not read.

Tests: a fresh run records a step-0 row before training; a run whose every later loss is worse ends
with `best_step == 0` and exports the starting weights.

## Step 4: the second warmup in `stages.rs`

- In `stage_after`, keep a running `warmup_end`, starting at `warmup_rounds * steps_per_round`. At a
  switch with `best_step == 0`, set `warmup_end = step + warmup_rounds * steps_per_round`; patience
  then counts from it (`patience_from` already takes the max with `warmup_end`).
- Add to `Stages` what the loop needs: `rolled_back_at: Option<u64>`, the step of the latest switch
  when that switch is the last recorded row (the switch step itself), used by step 6's resume rule.
  (A switch is always a rollback, so `switch_step` plus "is it the last row" is enough.)

Tests: a switch with best at step 0 gets a fresh warmup round that does not count against patience;
a switch with a later best does not; replays are unchanged for histories without a step-0 row.

## Step 5: `best.opt` at every new best

- `Arm` gets `best_optimizer_path` (`lambda-<l>/best.opt`).
- At a round's end, after the history row and the checkpoint, if this row set a new best
  (`stages.best_step == step`), write `best.opt` with `optimizer.save_arm` through a staging name and
  a rename.
- At step 0 nothing is written: the rollback to step 0 uses `reset_arm`.

## Step 6: the rollback

A function `roll_back(stack, slot, arm, stages)`:

- `best_step == 0`: load the starting weights into slot `slot` (`arm_slice::fill_slice`, as
  `open_stack` does for a fresh slot) and `optimizer.reset_arm(slot)`;
- otherwise: `arm_slice::load_arm` the best step's checkpoint into the slot, and
  `optimizer.restore_arm(slot, best.opt)`;
- then clamp to the weight bounds again (`optimizer.bound`), as after any load.

Idempotent: it sets the slot's whole state from files, whatever the slot held.

**At a round's end**, reorder the per-arm block so the rollback precedes the backup:

1. append the history row;
2. write the checkpoint, prune;
3. write `best.opt` if a new best (step 5);
4. replay the stages; on a switch, set the new rate and `roll_back`;
5. `save_slot` and `RoundProgress`.

**On resume**, in `train_in_lockstep` after `open_stack`, for each active arm whose last row is a
switch (`rolled_back_at == Some(s)`):

- backup's run step `== s`: already rolled back, nothing to do;
- backup's run step `> s`: trained past it, nothing to do;
- backup's run step `< s`: the arm retrains up to `s`; in the round-end branch for an arm that
  "had recorded this boundary before a crash", call `roll_back` there too (today that branch records
  nothing and needs nothing, rates being rederived).

Tests (on the test-only one-network arms or a small stack): a stage whose losses never beat the best
ends with the arm's weights and moments equal to the best's, and its `t` equal to the best's; the run's
step keeps growing; a rollback to step 0 reruns warmup at the new rate; a simulated crash between the
history row and `save_slot` at a switch resumes to the same state as no crash.

## Step 7: the round line and messages

- The switch message says where the arm rolled back to: "no new best since round N, back to round N,
  moving from rate a to b". At step 0: "back to the starting weights".

## Step 8: on the training machine

- `cargo test --release -p smithy` on the Lichess machine, then on the training machine.
- A finished generation is never trained again (`generation_files::training_state_check`), so the
  gate's failed amanda generation 3 does not retrain by itself. Whether to retrain it under this rule,
  and how its directory is set aside, is the developer's decision when this is in.

## Not changed

- The transformer track shares `train_in_lockstep` and `Stack`, so it gets the same rule; it is
  parked, and nothing here is tuned for it.
- `checkpoints/` retention, `history.tsv`'s format and the stream's cursor derivation stay as they
  are, the cursor now from the run's step.
