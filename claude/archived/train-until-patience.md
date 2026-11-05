# NNUE training until patience, then one gate

**Superseded, see one-learning-rate.md** (the same day): epochs, `MAX_EPOCHS`, `StopReason`,
per-arm patience and `epoch-NNNN/` are gone; one run steps down a list of rates, and one gate per
generation remains. The lineage names below are those of 2026-09-28, since reused.

Status: design agreed with the developer on 2026-09-28; implemented, built, tested, committed. Context: nnue-architecture-revision.md (betty's generation 1 was gated after one epoch,
with every arm still improving).

## The rule, as agreed

1. Train epoch 1 in full, every arm of the learning-rate grid, as today.
2. Keep training until the patience rule stops it, or the epoch cap does.
3. Gate the challenger once: the best arm's best checkpoint, as today.
4. Pass: promote, as today. Fail: no promotion, the champion stays, and training of this
   generation is over. The developer decides what comes next (more data, a lower rate, another
   architecture); the pipeline does not.

## Decisions

- **Patience: 12 rounds, plain.** An arm has exhausted its patience when 12 rounds have gone by
  without a new best test loss of its own. No `min_delta`, no noise estimate: the developer wants
  nothing in the code base that is not understood, and 12 rounds (3/8 of an epoch) is generous
  enough that a new best from noise alone becomes unlikely once the curve is flat. Checked on
  amanda's and betty's generation-1 histories: both keep finding new bests up to the end of
  epoch 1, so neither would have stopped there.
- **Counted from the start of epoch 1.** "Best so far" includes epoch 1's rounds, and stopping is
  allowed only once epoch 1 is complete. Restarting the count at epoch 2 would force 12 more rounds
  even after a flat epoch 1.
- **Per arm, and the run ends when every arm has exhausted its patience at the same time.** The
  arms train in lockstep over one decode of the cache, so an arm cannot stop on its own without
  restructuring `round::run_round`; an arm whose patience is exhausted keeps stepping, and a later
  new best of its own renews it. The run therefore ends at the first round boundary where no arm
  has improved in its last 12 rounds. This is the conservative reading: a slower arm still
  improving keeps the run going, since it could overtake.
- **Epoch cap: 4 epochs.** Bounds how long a generation can hold up the next one's self-play. A run
  stopped by the cap says so, since an arm still improving there is worth knowing about.
- **Both constants in code**, `PATIENCE_ROUNDS = 12` and `MAX_EPOCHS = 4` in `src/train/mod.rs`,
  not in `net.yaml`: neither is a property of a network, and one value for every lineage keeps runs
  comparable.
- **No new state file.** Each arm's best step is already rebuilt from `history.tsv` on resume
  (`best_recorded`), so an arm's rounds without improvement are `(step - best_step) /
  steps_per_round`, derivable at any time, across restarts and epoch boundaries.
- **One invocation trains to the stop**, across epoch boundaries, instead of one invocation per
  epoch. Ctrl+C and rerun resume as today. A side benefit: the inexact shuffle-buffer restart the
  code comments describe (about 1.4% of a pass) is now paid once per interruption, not once per
  epoch.
- **The challenger goes into `epoch-NNNN/`**, NNNN being the epoch the run stopped in (partial or
  complete). `rotor-arena gate` and `promote` already read the latest epoch directory and need no
  change.

## Implementation plan

### `src/pipeline/generation.rs`

- Replace `epoch_gate_check(steps_done, steps_per_epoch, generation_dir)` with
  `training_state_check(generation_dir) -> Result<(), String>`: training may run only while the
  generation has no challenger yet (`latest_epoch` is `None`). Once one exists, the message says
  what to do instead: no verdict, run the gate; pass, promote; fail, training of this generation
  is over and the champion stays.
- `promotable_challenger`: the fail message no longer says "train another epoch".
- Tests updated accordingly.

### `src/train/mod.rs`

- `PATIENCE_ROUNDS`, `MAX_EPOCHS`.
- A pure function `stop_reason(step, steps_per_round, steps_per_epoch, best_steps) ->
  Option<StopReason>`: `None` before the end of epoch 1; `Some(Patience)` when every arm's
  `(step - best_step) >= PATIENCE_ROUNDS * steps_per_round`; `Some(EpochCap)` at `MAX_EPOCHS *
  steps_per_epoch`. Unit-tested.
- `run_train_nnue`: the gate check at startup becomes `training_state_check`; the epoch-boundary
  exit becomes a `stop_reason` check after every round (and once at startup, for a run interrupted
  between meeting the rule and exporting), which prints why it stopped, the standings, and exports
  the challenger. The "epoch did not improve on the generation's best" refusal goes: with one
  export per generation it can no longer happen.
- Doc comments of `run_train_nnue`, `run_train`, `parse_job_flags`, the module and `USAGE` updated:
  one invocation trains to the stop, not one epoch. References to claude/ documents in the comments
  touched are removed, per CLAUDE.md.
- The `--patience`/`--min-steps` flags stay transformer-only; the NNUE refusal message changes.

## Progress

- 2026-09-28: plan written.
- 2026-09-28: implemented (not yet built): `generation::training_state_check` replaces
  `epoch_gate_check`; `stop_reason`, `finish_training`, `PATIENCE_ROUNDS`, `MAX_EPOCHS` in
  `src/train/mod.rs` with unit tests; messages after a failed gate (`rotor-arena gate`, `promote`,
  the challenger export line) no longer say "train another epoch"; config comments updated in
  amanda, betty and caroline. Developer added `configs/caroline.yaml`: betty's shape with the
  sqrt(2) learning-rate grid, betty keeping its original grid.
- 2026-09-28: built, tested, committed. Caroline generation 1 training under the new rule. At round
  53 (epoch 2) the leading arm has drifted down with training time: 1e-3 at mid-epoch 1 (0.04606),
  7e-4 at the end of epoch 1 (0.04297) and mid-epoch 2 (0.04150), 5e-4 now (0.04094), 3.5e-4 closing
  in (0.04115). Every arm is already below betty's one-epoch 0.04338. The best fixed rate depends on
  how long training runs, which patience now decides; if 3.5e-4, the bottom of the grid, ends up
  winning, the optimum may lie below the grid. Candidate for dorothy: learning-rate decay (reduce on
  plateau, or a fixed schedule) instead of a grid of fixed rates.
