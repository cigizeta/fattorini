# Rollback to the best checkpoint at every rate switch

Design, agreed with the developer 2026-10-16. Implementation plan to follow.

## Why

Amanda generation 3 failed the gate against generation 2. The likely cause: the first rate,
3e-3, damaged generation 2's weights, and since a rate switch today continues from the current
weights, every later stage inherited the damage. Only at the very end is the best checkpoint
exported, too late to help the stages in between.

## The rule

- Every new best saves the arm's weights and its optimizer state.
- When patience runs out, the arm **rolls back** to its latest best (weights, optimizer, warmup
  progress), then moves to the next rate.
- A stage that only made things worse is undone, as if it had never run.

## 1. The starting point is the first best

Before step 1, every arm's test loss is measured on the generation's starting weights (the previous
champion's `champion.vs`, or `initial.vs` for generation 1), and recorded as a `history.tsv` row at
step 0. It is the run's first best.

If 3e-3 never beats it, the rollback returns exactly to the previous generation's weights, and
1e-3 starts from there: a harmful rate is skipped by itself. If every rate fails, the run ends with
the starting weights as its best, and the candidate equals the champion: an honest result.

## 2. What rolls back, what moves forward

A rollback restores the arm's whole training state at the best point: weights, Adam's moments, the
optimizer's step count `t`, and so warmup progress. It does not restore the run's step or the data
stream:

- the run's step keeps growing; the next round after a switch at step 50,000 is still step 50,001;
- the data stream continues where it is, so the rolled-back arm sees new batches;
- `history.tsv` keeps every row, the discarded stage's included.

**Warmup reruns by itself after a rollback to step 0**, since warmup is part of the restored state.
`StackedAdamW` computes the warmup factor from the arm's `t`, so restoring `t = 0` ramps the rate up
again, from `rate / warmup_steps` to the arm's **new** rate (1e-3 after 3e-3), not the old one. A
rollback to a later best restores its moments and a `t` past warmup: no ramp.

Since warmup is one round and test loss is measured only at round ends, a best never falls partway
through warmup: it is either step 0 or past warmup.

`stages.rs` stays a pure function of `history.tsv`: a switch with `best_step == 0` is a rollback to
step 0, so the replay derives that warmup's end, `switch_step + warmup_rounds * steps_per_round`, and
counts patience from there, as it does after the first warmup.

## 3. Two counters per arm, per-arm rollback

Today the run's step **is** the optimizer's `t` (`StackedAdamW::step_count`): the stream cursor
(`(steps_done * batch_size) % train_rows`) and the round accounting are derived from it. A rollback
must restore `t`, for two reasons:

- bias correction uses `t`: zero moments with a large `t` make the first update about
  `3 * lr * sign(grad)`, worse than the jolt warmup exists to prevent;
- warmup is computed from `t`.

So each arm gets **two counters**:

- **the run's step:** only grows; drives the cursor, the rounds and `history.tsv`; shared by the
  arms in lockstep;
- **the optimizer's `t`:** restored by a rollback; drives bias correction and warmup.

They are equal until an arm's first rollback.

The run's step must be **persisted in the backup** next to `t` (a second scalar in `optimizer.opt`,
or in `round_progress.state`), and the cursor derived from it. It cannot come from `history.tsv`,
since mid-round backups fall between rows.

Lockstep arms roll back one at a time. `StackedAdamW` already keeps a rate and a `t` per arm, and
`save_arm`/`load_arm` already write and read one arm's slice of the moments plus its `t`. A rollback
of arm `a`: load its best weights into slot `a` (`arm_slice`), `load_arm` its best optimizer state,
leave the other arms alone.

## 4. Storage

- **`best.opt`, one per arm,** in `lambda-<l>/`: written by `save_arm` at every new best of that arm,
  overwriting the previous one, through a staging name and a rename like the other saves.
- **The best weights need nothing new:** they are the best step's file in `checkpoints/`.
  `prune_checkpoints` keeps the best checkpoints, so the one a rollback needs is never pruned.
- **Step 0's optimizer needs no file:** it is fresh (zero moments, `t = 0`), rebuilt rather than
  loaded. `best.opt` is first written when a later round beats the starting point. Its weights are
  checkpointed like any round's (`checkpoints/step_000000000.vs`, decided while implementing, so that
  export, `check` and the arena need no special case for a best at step 0).

Size: Adam's two moments are about twice a weights file, a few MB per psq arm. Negligible.

## 5. Crashes and resume

Order of writes at a round's end today (`main.rs`, the lockstep loop):

1. append the `history.tsv` row;
2. write the checkpoint to `checkpoints/`;
3. prune checkpoints;
4. back up `model.vs` and `optimizer.opt` (`save_slot`);
5. replay the stages; on a switch, set the new rate in memory.

`run_round` also backs up mid-round (`BACKUP_INTERVAL`). A crash between 1 and 4 is already handled:
the rerun resumes from the last mid-round backup, retrains to the boundary, finds the row already
recorded and records nothing. The rate is never persisted: it is rederived from the replay on every
start.

**The rollback goes before step 4:** at a switch, replay the stages, roll the arm back in memory,
then `save_slot`. So a backup at exactly the switch step only ever exists after its rollback.

The rollback is idempotent: it sets the arm's whole state from the best files (or the rebuilt step 0),
whatever the arm held before. On resume, for an arm whose last `history.tsv` row is a switch:

- **backup's run step < switch step:** the crash came before the boundary's backup. The arm retrains
  to the boundary; the "already recorded this boundary" branch must then **apply the rollback**,
  which it does not do today (rates need nothing there, being rederived);
- **backup's run step = switch step:** already rolled back; nothing to do;
- **backup's run step > switch step:** trained past it; nothing to do.

No marker file: `history.tsv` stays the only record of where a run is.
