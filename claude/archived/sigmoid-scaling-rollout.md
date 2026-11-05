# Sigmoid scaling rollout

**Archived: superseded in full, see training.md.** Its "Still current" section no longer is: the
sigmoid label and `binary_cross_entropy` gave way to MSE against `[-1, 1]` labels, then to pawns
(`CEILING_PAWNS = 32`); `rotor-curate` and the MoE network are gone.

## Status

Complete and confirmed: `cargo build`, `cargo test --bin rotor-train --features training` (72
passed, 0 failed), `cargo test --bin rotor-curate` (1 passed). `src/bin/rotor_calibrate.rs`
deleted.

**Superseded since, on two points:**

- The `Network = Bypass + sigmoid` shape this rollout landed was later deleted entirely (not
  "restored to MoE" as this document originally anticipated at the time) once `rotor-halfkp.md`
  replaced the whole mixture outright. This document's own "restoring MoE later" plan no longer
  applies to anything live.
- The per-*position* train/test split this rollout introduced (beyond its own original plan, at
  the developer's direction) was later reverted to per-*game* once `training.md`/
  `centipawn-calibration.md` settled per-game as correct -- averaging a position's score across
  different games is a category error once each label is a property of its own specific
  trajectory.

**Still current**: the centipawn scaling this rollout implemented --
`score(cp) = sigmoid(log(2.0) * cp / 100.0)` -- is the fixed conversion `training.md`'s own
"Centipawn scaling" section still specifies, and `binary_cross_entropy` (replacing `mse_loss`) is
still the loss function in use.

## What landed

- `rotor-calibrate` binary and its `nalgebra`/`levenberg-marquardt` dependencies removed.
- `rotor_curate.rs`: dropped the `phase` column and the calibration-pool sampling machinery
  entirely -- no fitted win-probability curve anywhere in the pipeline.
- `labels.rs`: `score(cp) -> f64` replaces `adjust_score`/`adjust_outcome`/`is_mate_scale`;
  `compute_labels`'s recursive step uses `score(scores[t])` directly, base case is `outcome`
  unmodified -- no `divisor`/`m`.
- `cache.rs`/`reader.rs`/`dataset.rs`: `rms_divisor`/`compute_divisor_and_m` removed.
- `epoch.rs`: loss changed from `mse_loss` to `binary_cross_entropy`.
