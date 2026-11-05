# Pawn output for the NNUE networks

Implementation plan. Branch: `alessio`. Applies `architectures.md`'s "Output units and training
loss" to naive, halfkp, and halfka_v2_hm; the big five already follows it (`big-five-training.md`).

## Now

The value head's raw output `y` is read as label space: the training and test losses are both
`mse(y, t)`, and the engine's naive plays `100 * clamped_pawns_from_score(y)` centipawns. halfkp and
halfka_v2_hm exist in the trainer only (no engine forward pass).

## Target

`y` is in pawns. The prediction is `score(y)` (`labels.rs`), training follows the matching gradient
`score(y) - t` with no backpropagation through `score`, and the engine plays
`100 * clamp(y, ±CEILING_PAWNS)`.

## Decisions

Settled in discussion.

- **One implementation, in `nnue_network::Network`.** Each kind's own `forward` becomes `pawns`,
  unchanged inside (its value head's output, no new layer). `Network::pawns(batch)` dispatches;
  `Network::forward = score(pawns)` for every kind; `Network::training_loss` is the big five's,
  moved up and shared: value the label-space MSE, gradient the matching one
  (`mse + matching - matching.detach()`). `test_loss` stays `mse(forward, t)`. halfkp and
  halfka_v2_hm need nothing of their own.
- **No output scale.** `pawns` is the head's output, as in the big five. If amanda's first run
  converges visibly slower than label-space runs did, revisit with a fixed scale.
- **The engine's naive.** `evaluate_raw` becomes `evaluate_pawns` (the big five's name), and
  `evaluate` plays `100 * clamp(pawns, ±CEILING_PAWNS)`; `evaluate_by_color_cp` the same.
  `clamped_pawns_from_score` leaves `nnue.rs`.
- **Unchanged:** the export and the `.nnue` format (only the output's meaning changes), and
  `check`, `calibrate`, `correlate`, which read `forward`, still label space: their conversion back
  to pawns (`clamped_pawns_from_score(score(y))`) is `clamp(y, ±32)`.
- **The transformer stays in label space** (its own network and rounds, outside `Network`), a
  follow-up in `TODO.md`. Nobody trains it now.
- No checkpoint compatibility: amanda and betty start fresh lineages.

## Steps

1. `src/train/nnue_network.rs`: per-kind `forward` renamed `pawns`; `Network::pawns`;
   `Network::forward = score(pawns)`; the shared `training_loss`. Doc comments: the value is pawns.
2. `src/train/big_five_network.rs`: its `training_loss` and `forward` go (the shared ones cover
   it); `evaluate` is its `pawns`. Its `score` moves where `nnue_network.rs` can share it.
3. `src/nnue.rs`: `evaluate_pawns`, the clamp, `evaluate_by_color_cp`; doc comments.
4. Check every other user of `Network::forward` and of a loss: `nnue_round.rs`, `window.rs`,
   `round.rs`, `mod.rs` (`check`, `calibrate`, `correlate`), `export.rs` tests, `nnue_bench`.
   Any path computing its own `mse(forward, label)` as a training loss must use `training_loss`.
5. `src/train/value_head.rs` and `labels.rs` comments that say the networks output label space.
6. `TODO.md`: the item done, the transformer follow-up. `architectures.md` unchanged (it already
   says this).

## As implemented

- `score` and the shared loss, `matching_loss(pawns, label)`, live in `nnue_network.rs`;
  `big_five_network.rs` lost its `score`, `forward`, and `training_loss`, and its `evaluate` is
  `pawns`. Its two loss tests moved to `nnue_network.rs`, made generic
  (`the_matching_loss_has_the_matching_gradient`, `a_dense_networks_training_loss_reports_the_label_space_mse`).
- The engine's conversion is one function, `nnue::centipawns`, used by `evaluate` and
  `evaluate_by_color_cp`; `the_centipawn_conversion_stays_far_inside_the_mate_band` became
  `centipawns_are_a_hundred_times_the_pawns_clamped_to_the_ceiling`, with exact values.
- `nnue_round.rs` and `window.rs`, which call `Network::forward` with their own losses, are not
  declared in any module and never compiled: left alone, their deletion noted in `TODO.md`.

## Tests

- **Engine against `tch`** (`export.rs`): compares `evaluate_pawns` with `Network::pawns`.
- **Centipawns:** naive's `evaluate` is `100 * clamp(pawns, ±32)`, the clamp included (a network
  driven past 32 pawns through its output bias).
- **Matching gradient, for a dense network:** the shared `training_loss`'s value equals
  `mse(score(pawns), t)`, and its gradient on the pawns is `score(pawns) - t` (the big five's test,
  made generic).
- The big five's tests pass unchanged (step 0 is PeSTO, the export round trip).

## Verification

```
cargo test --release
```

```
cargo test --release --features training --bin rotor-train
```

Then amanda's first run on the GPU machine: its test loss is directly comparable with betty's
(both label-space MSE).
