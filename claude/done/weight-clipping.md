# Weight clipping: betty's first hidden layer within +/-127/64

**Status:** done; the trainer clips, and every generation from 2 trained with it (checked
2026-10-14). The `i8` kernel is its own plan.

Decided 2026-10-07 (TODO.md, generation 2): betty's generation 2 clips her first hidden layer's
weights to +/-127/64 during training, so that the engine can later read that layer as `i8` at
Stockfish's fixed scale of 64 and use `maddubs`, half the multiplies of today's `i16`. This plan
covers the trainer only; the `i8` kernel is a separate plan, judged by duel.

## Decisions

- **Always on**, no config field (the developer, 2026-10-07). The bound belongs to the `i8` format
  the engine will read, not to the training recipe. A new field would also be frozen in
  `run_config.txt` from a run's first invocation. Generation 1 is finished, so no trained run
  changes.
- **Only the first hidden layer's weights.** Its bias stays f32. The transform keeps its
  `i16` accumulator, scaled at load, and later layers stay f32. Networks with no hidden layer
  (amanda, caroline), halfkp and halfka_v2_hm are untouched.
- **The bound is `127.0 / 64.0`** (1.984375), the same as nnue-pytorch's.

## Measured: generation 1 already fits (2026-10-07, the training machine)

`nnue_bench --weights` on `betty/gen-0001/champion.nnue`, 16 x 512:
|w| p50 0.0924, p90 0.3172, p99 0.9563, p99.9 1.5657, max 1.9431 (row 13). **None of 8192 is
beyond the bound.** So the clip is a safeguard: generation 2 starts from these weights and the
clip acts only if training pushes a weight past 1.98.

Its consequence for the later `i8` kernel: the cost there is precision, not range. One `i8` step is
1/64, about 0.0156, so the median weight is about 6 steps. Rounding to `i8` per row
(scale 127/max, at least 65 here) already left betty at 3.8 cp mean from f32 (nnue.rs's
`QuantizedHead` comment), and a fixed 64 is no finer. The `i8` plan should expect at least that.

## Implementation

Revised while implementing: the clip lives in the optimizer, not in a new `Trainee` closure, which
would have touched ten construction sites in `round.rs`'s tests. `AdamW` is Rotor's own and already
holds every parameter by name.

1. `src/train/adamw.rs`: `AdamW::bound(name, bound)` clamps that parameter at once and records the
   bound; `step` clamps every bounded parameter right after its update. The moments are left as
   they are. Bounds are not persisted: they are set again on every start.
2. `src/train/nnue_network.rs`: `FIRST_HIDDEN_WEIGHT_BOUND = 127.0 / 64.0` and
   `FIRST_HIDDEN_WEIGHT = "value_head.hidden0.weight"`, and `Network::weight_bounds()`: that one
   entry for psq and psq_threats with a hidden layer, none otherwise. `ValueHead::has_hidden_layer`.
3. `src/train/mod.rs`, where each lockstep arm is built: after the weights load (resume or
   `seed_network`), `optimizer.bound` for each of `network.weight_bounds()`. This covers the warm
   start too: round 0's test loss is that of the clipped network (a no-op on betty-0001).
4. Export (`src/train/export.rs`, `weights_from`): panics on a first hidden layer weight beyond the
   bound, as it already does on a wrong shape.
5. `nnue_bench --weights` stays, for checking a run's champion.

## Tests

- `adamw.rs`: a bound clamps at once and after every step at a rate of 10, holding the weights
  exactly on it, while an unbounded parameter beside it moves freely; an unknown name panics.
- `nnue_network.rs`: psq and psq_threats with hidden layers bound exactly `value_head.hidden0.weight`,
  a name `tch` really registers; no hidden layer, halfkp and halfka_v2_hm bound nothing.
- `export.rs`: a first hidden weight of 2.5 is refused.

## Verification (Windows, the training machine)

- `cargo test` passes.
- Resume betty's generation 2 for one round, then `nnue_bench --weights` on its checkpoint: zero
  beyond the bound.
