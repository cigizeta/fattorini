# Keep only the best checkpoint

**Status:** designed 2026-10-25; steps 1 to 5 done the same day, tests passing; step 6 done: a dev smithy smoke run of lc0-psq left
one checkpoint in each of its nine lambdas, each its `candidate.txt`'s step. Done. Step 3 also
removed `stage_of_each_round`, read only by `stage_best_steps`; a test now pins that a tie in loss
keeps the older step as best.

## Why

A lambda's `checkpoints/` keeps today the 4 lowest test losses (`BEST_CHECKPOINTS_KEPT`) plus every
learning-rate stage's best (`stage_best_steps`), 7 files in the taper's smoke run, about 4.8 MB each
at width 256: about 34 MB a lambda, 300 MB an attempt of 9 lambdas.

The stage bests were protected for selection's round robin of one candidate per stage, which
selection no longer plays (the developer, 2026-10-25). What still reads a checkpoint:

| reader | step |
|---|---|
| `roll_back` at a stage switch (`lockstep.rs`) | `stages.best_step` |
| the candidate's export (`candidates::export_best`) | the run's best, `stages.best_step` |
| the next generation's warm start (`smithy.rs`, `read_candidate_step`) | the winner's candidate step |
| `smithy check`, `calibrate`, `correlate` | any retained step |

Only the best is ever needed (the developer, 2026-10-25: keep just one).

## Decisions (the developer, 2026-10-25)

1. **One checkpoint a lambda:** the best, as `Stages` defines it, every other deleted after each round.
2. **No ranking by loss:** `BEST_CHECKPOINTS_KEPT = 1` would not do. The prune breaks a tie in loss
   towards the newer step, `Stages` keeps the older (a best is replaced only by a strictly lower
   loss), so with one kept a tie would delete the very checkpoint `roll_back` and the export load.
   The prune reads `stages.best_step` itself, so the two cannot disagree.

## Design

In `lockstep.rs`, a completed round of an arm becomes:

1. The history row, then the round's checkpoint, as today (the row first, so a crash never leaves a
   checkpoint without its row).
2. `replay_stages`, moved before the prune, so `arm.stages.best_step` counts this round.
3. `checkpoint_store::keep_only(checkpoints_dir, best_step)`: every checkpoint but the best's deleted.

Cases:

- **Nothing beats the starting weights yet:** the best is step 0, whose checkpoint is written before
  training, and stays.
- **A NaN loss** is never a best (`Stages`), so a diverged round's checkpoint goes at once.
- **A crash between the save and the delete** leaves an extra file, deleted after the next round.
- **A resume** reads the arm's model and optimizer files, not the checkpoints: unchanged.
- **The best's checkpoint missing** (it cannot be, being never deleted): `keep_only` deletes the rest
  all the same, and `roll_back`'s existing assertion names the missing file.
- **Runs already on disk** keep their extra checkpoints until their next round; finished ones keep them
  for good, harmless, deletable by hand.

## Plan

1. **`checkpoint_store.rs`:** `keep_only(checkpoints_dir, step)`, deleting every
   `checkpoint_steps` entry but `step`; `prune_checkpoints`, `BEST_CHECKPOINTS_KEPT` and their four
   tests removed; the module comment on retention by test loss rewritten. Tests: three checkpoints,
   the middle one kept; the kept step being 0; an empty directory.
2. **`lockstep.rs`:** the order above; `stage_best_steps` (the lockstep one) and the stale comments on
   the stage round robin removed.
3. **`stages.rs`:** `stage_best_steps` and its test removed, if nothing else reads them.
4. **`candidates.rs`:** `resolve_checkpoint`'s comment ("pruned down to its few best rounds") updated.
5. **Docs:** DEV.md if it describes the retained checkpoints, TODO.md.
6. `cargo test -p smithy`, then a dev smithy smoke run: each `lambda-*/checkpoints/` holds one file,
   the candidate exported from it.
