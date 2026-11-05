# check, calibrate and correlate removed

**Status: decided and planned 2026-10-26 (the developer: "Remove all of them"); steps 1 to 5
applied the same day, also `players::latest_trained_attempt` (only `run_lambda` used it); step 6:
`cargo test -p smithy` passed the same day, once the unused `players` import in `main.rs` went;
clippy then found the one-network forward path unread outside the tests (`sweep_checkpoint` was
its last caller): `nnue_network.rs` allows dead code outside the tests, as `transformer_network.rs`
does, so do `stack_head.rs`'s `Stage`, `StackHead` and `stage_forward`, and `to_dense_batch_on`
and `CacheRecord::label` are `#[cfg(test)]`. Clippy builds with no error, its warnings older.**

**Left for later:** every caller of `rounds_display::print_progress` now passes a suffix, so its
terminal bar (`PassBar`, `PASS_BAR`, `needs_new_bar`) is reached by no caller: it goes with
indicatif's removal.

## Why

The three commands read a lambda's checkpoint against its test set, written when a network's raw
output was a label-space value in `[-1, 1]`. Since then `forward` returns pawns, the loss maps them
through `score`, and the engine reads them as pawns with a pawn scale measured at load:

- `check` sends the pawns through `clamped_pawns_from_score` as if they were a label: its errors
  mean nothing, and even fixed they would restate the test loss in another unit.
- `calibrate` measured the raw output toward a centipawn formula no longer needed.
- `correlate`'s Pearson r between pawns and a label related to them by `atan` understates the fit;
  the test loss, `pieces.tsv` and the gate answer its question for every run.
- One checkpoint a lambda is kept (best-checkpoint-only.md), so no rounds to compare either.

## Plan

1. `main.rs`: the dispatch arm, the usage lines, `parse_lambda_flag`, `parse_step`, `Inspected`,
   `inspected_checkpoint`, `sweep_checkpoint`, `run_check`, `run_calibrate`, `run_correlate`, their
   imports; the module comment.
2. `round.rs`: `run_cp_check`, `run_calibration_sweep`, `run_correlation_sweep`, their reports and
   tests, the `clamped_pawns_from_score` import.
3. `candidates.rs`: `resolve_checkpoint`. `smithy.rs`: `RunLambda`, `run_lambda`, and whatever only
   they used.
4. `labels.rs`: `clamped_pawns_from_score` and its tests; `pawns_from_score` stays (Lc0's
   conversion).
5. Comments naming the three, in `checkpoint_store.rs`, `freeze.rs`, `net_config.rs`,
   `train_stream.rs`, `rounds_display.rs`, `players.rs`, `labels.rs`, `lib.rs`; DEV.md's section.
6. Check: `cargo test -p smithy` and `-p rotor`, clippy, by the developer.
