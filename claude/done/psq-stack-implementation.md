# Psq stack: implementation plan

**Status:** implemented 2026-10-15; rotor, smithy and arena tests pass. Departures from the plan, decided
while implementing:

- `StackedNetwork::with_pairwise_shape` became `with_head_shape` (pairwise and stack flags);
  `Network::with_pairwise` stays for tests, `Network::with_head` is the constructor.
- The engine's per-bucket L1 sets are each their own `Vec<i16>`, contiguous but not forced to a
  64-byte boundary (the kernels load unaligned); revisit only if the bench shows a cost.
- For a stack, `PsqNetwork::hidden` is empty and `output` is `Layer::default()`; the head is `stack`.
  `evaluate_accumulators` refuses a stack (it has no buckets); `evaluate_stack_accumulators` and
  `evaluate_pawns` take them.
- `nnue_bench --stack` defaults to `--head 16,16` and counts every bucket's parameters. Implements psq-stack.md (every open question
decided there). First network: hilda (`configs/hilda.yaml`). Implemented ahead of its experiments
(the developer, 2026-10-15: "it does not hurt"); no network uses it until hilda trains.

## What is built

    (128 + 128) x 2 -> 256 -> 16 -> 16 -> 1

- **Accumulator and products:** `psq_pairwise`'s exactly (pairwise-products-implementation.md): psq
  rows, width 256 per perspective, each split in halves multiplied element-wise, both perspectives'
  128 products concatenated, side to move first: 256 values.
- **L1**, 256 -> 16, one of 64 weight sets by **our king**: the side to move's king square, rank
  flipped when Black is to move (`features::oriented_square`), no file mirror. Clamp to `[0, 1]`.
- **L2**, 16 -> 16, one of 64 by **their king**: the other king, oriented for the side to move too.
  Clamp.
- **Output**, 16 -> 1, one of 78 by **material**: pawn 1, knight and bishop 3, rook 5, queen 9, both
  sides, kings excluded, capped at 78; bucket `material - 1`, material 0 (king against king, which the
  search never evaluates but old corpora hold) sharing bucket 0 with material 1.
- **Factorisation in training only:** every bucketed layer is `shared + delta[bucket]`, deltas
  starting at 0, no penalty; the export writes the sums, so the engine sees plain per-bucket weights.
- The widths 16 and 16 are the config's `value_head.hidden_dim`, exactly two entries required; the
  bucket counts and keys are fixed (psq-stack.md, decisions 1, 4, 5).

## Step 1: the bucket keys, shared (`crates/rotor/src/stack_buckets.rs`, new)

One definition for trainer, engine and `smithy buckets`, so they cannot drift apart.

- `pub const OUR_KING_BUCKETS: usize = 64; THEIR_KING_BUCKETS = 64; MATERIAL_BUCKETS = 78;`
- `pub const MATERIAL_VALUE: [i32; 6]` (1, 3, 3, 5, 9, 0, indexed by `PieceType`), `MATERIAL_MAX = 78`.
- `pub fn material_bucket(material: u32) -> usize`: `material.clamp(1, 78) - 1`.
- `pub fn buckets_of_canonical(position: &CanonicalPosition) -> [usize; 3]` and
  `pub fn buckets_of_board(board: &Board) -> [usize; 3]`: our king, their king, material bucket. The
  board's material from the ten non-king bitboards' popcounts (no incremental count: ten popcounts an
  evaluation, measured with the bench).
- `smithy::buckets` (the bench) uses `MATERIAL_VALUE`, `MATERIAL_MAX` and the king orientation from
  here instead of its own copies (`kings_of` keeps its name and calls the shared code).
- Tests: the start (e1, e8, material 78 -> bucket 77), Black to move (flipped), kings off their home
  ranks, a promotion past 78 capped, king against king (bucket 0); the two entry points agree on a
  set of FENs.

## Step 2: config (`crates/smithy/src/net_config.rs`)

- `ArchitectureKind::PsqStack` (serde `psq_stack`), doc comment naming psq-stack.md's shape.
- `Architecture::pairwise()` true for `PsqPairwise` and `PsqStack`; new `Architecture::stack()` true
  for `PsqStack` only.
- `load`: for `psq_stack`, the even-width check as pairwise, and `value_head.hidden_dim` of exactly two
  entries, naming the field and the reason.
- Tests: a fixture `psq_stack` config loads with `stack()` and `pairwise()` true; one hidden width, or
  three, refused.
- `configs/hilda.yaml`: `value_head: hidden_dim: [16, 16]`, and its header comments rewritten for the
  stack (they are greta's today).

## Step 3: batches carry the buckets (`crates/smithy/src/batch.rs`)

- `HostBatch::Psq` and `PsqThreats` gain `buckets: Vec<i32>`, three per row, computed in
  `active_indices`' loop from `record.canonical` by `buckets_of_canonical`. Always, for every psq
  batch: three integers a row, cheaper than threading the architecture into `host_batch`.
- `HostBatch::bytes` counts them (the test cache's budget).
- `DenseBatchPsq` gains `buckets: Tensor` (`Int64[rows, 3]`), built in `psq_on`, moved by `to_device`;
  `from_dense` (tests) gets zeros. An accessor `buckets()`.
- Tests: a host batch of known positions carries their buckets; `bytes` grows by `12 * rows`.

## Step 4: the training head (`crates/smithy/src/stack_head.rs`, new)

`StackHead` beside `ValueHead`, for one network:

- Parameters, under `value_head`: for each stage `l1`, `l2`, `output`: `{stage}.shared.weight`
  `[out, in]`, `{stage}.shared.bias` `[out]`, `{stage}.delta.weight` `[buckets, out, in]`,
  `{stage}.delta.bias` `[buckets, out]`; deltas initialised to 0.
- `forward(x [batch, 256], buckets [batch, 3]) -> [batch, 1]`, per stage, without materialising a
  weight matrix per row: every bucket's delta output at once, `x @ delta.weight.view([B * out,
  in]).T` -> `[batch, B, out]`, then the row's own bucket gathered (`gather` on dim 1) -> `[batch,
  out]`, plus `x @ shared.weight.T + shared.bias + delta.bias[bucket]`. Clamp after L1 and L2.
  Cost per stage: `batch * B * out * in`, about 1.1 G multiply-adds a batch of 4096 for L1, fine on
  the GPU; memory `[batch, B * out]`, small.
- `NetworkPsq` (`nnue_network.rs`) gains a head enum, `Head::Plain(ValueHead) | Head::Stack(StackHead)`;
  `pawns(own, opponent)` gains `buckets`, ignored by the plain head; `Network::pawns` passes
  `batch.buckets()`. `Network::of` builds the stack head when `architecture.stack()`.
- No first-hidden-weight bound for the stack (`weight_bounds` empty): the bound served the shelved
  `i8` layer, and a bound on `shared` and `delta` separately would not bound their sum.
- Tests: with every delta 0 the stack head equals a plain head with the shared weights, per stage;
  with deltas set by hand, a row's output equals the one computed with its bucket's summed weights;
  gradients reach `shared` from every row and a bucket's `delta` only from its rows.

## Step 5: stacked arms (`crates/smithy/src/stacked_network.rs`)

- Parameters come from the unstacked network as today (`with_pairwise_shape`, renamed in passing to
  `with_head_shape` taking the architecture flags), so names and `[arms, ...]` shapes follow; `arm_slice`
  and `stacked_adamw` need nothing.
- `head`: for a stack, per stage, `baddbmm` of `[arms, batch, in]` against the delta weight viewed
  `[arms, in, B * out]` -> `[arms, batch, B, out]`, gathered by the batch's bucket (the same for every
  arm, expanded over arms), plus the shared stage and the gathered delta bias.
- Tests: the existing stacked-equals-unstacked tests run for a stack network too (`pawns`,
  `training_losses`, `test_losses`, sparse and dense).

## Step 6: export (`crates/smithy/src/export.rs`)

- `stack_network_from(vs, embedding_dim, hidden_dims, step, lr)`: transform as psq; for each stage and
  bucket, `shared + delta[bucket]` (weight and bias), asserted shapes, written as the engine's
  per-bucket `Layer`s.
- `write_engine_network` (`main.rs`) calls it when `architecture.stack()`.
- Tests: the engine's f32 forward pass agrees with tch for a stack checkpoint with nonzero deltas,
  through a written and reloaded file.

## Step 7: engine (`crates/rotor/src/nnue.rs`, `simd.rs`)

**Format.** `FEATURE_SET_PSQ_STACK = 7`, `shape_of(7) = (psq::ROWS, 1)`, name `"psq_stack"`. Header as
today (`embedding_dim`, `hidden_count` = 2, the two widths, step, learning rate); body: transform and
bias, then L1's 64 layers, L2's 64, the output's 78, each weight `[out][in]` then bias, in bucket
order. `VERSION` unchanged: the tag selects the body, so existing files load as before and an older
build refuses tag 7 as unknown.

**f32 (`PsqNetwork`).** Gains `stack: Option<StackLayers { l1: Vec<Layer>, l2: Vec<Layer>, output:
Vec<Layer> }>`; `hidden`/`output` unused (empty) for a stack. `evaluate_pawns(board)` computes the
buckets (`buckets_of_board`) and a new `evaluate_stack_accumulators(own, opponent, buckets)`: scalar
products into `HEAD_SCRATCH`, then the three bucket layers with clamps. `validate` checks every
bucket's shapes and the counts; `write` writes tag 7.

**Quantized (`QuantizedPsqNetwork`).** `QuantizedHead::Stack { l1: Vec<QuantizedLayer>, l2:
Vec<Layer>, output: Vec<Layer> }`, `QuantizedLayer` being today's `Hidden` first layer (i16 weights,
a scale, denominator and bias per row), one per bucket, quantized as `quantize` quantizes a first
hidden layer with the products' 1.0 (`PRODUCT_ONE`). L2 and output stay f32, as `rest` and `output`
do today.

- `evaluate_by_color_cp(accumulators, board)` computes the buckets once and passes them down.
- The products into `ACTIVATION_SCRATCH` by `pairwise_activations_i16_avx2`, as pairwise with a hidden
  layer does; then `dots_i16_8_avx2` over the L1 bucket's 16 rows (two groups of 8);
  `hidden_values_avx2`; then L2's bucket in f32 through `HEAD_SCRATCH` with `clipped_relu_avx2`;
  then the output bucket's dot. Scalar fallback when a half is not a multiple of 16.
- Each bucket's weights contiguous, each L1 set 64-byte aligned (psq-stack.md, Cost).
- Incremental update untouched.

## Step 8: smithy plumbing and bench

- `main.rs`: every `match` on `ArchitectureKind` gains `PsqStack` beside `PsqPairwise` (`run_train`,
  `run_check`, `run_calibrate`, `run_correlate`, `export_candidate`); `engine_runs` true;
  `feature_set_of(PsqStack) = Psq`, with its test.
- `nnue_bench.rs`: `--stack`, a random stack network (`random_stack_network(width)`), so
  `--quantization` and the speed modes run on it; the kind label.

## Tests, overall

- Steps 1 to 7's tests.
- **Engine:** vectorized equals scalar exactly on a stack network; quantized within tolerance of f32
  on the sample positions; a round trip of the format; a tag-7 file with a wrong bucket count or
  width refused.
- **Unchanged networks:** psq, psq_threats and psq_pairwise evaluations and node counts identical
  (the search tests' and bench's fixed counts).

For the developer, after implementation:

    cargo test --release -p rotor

    cargo test --release -p smithy

    cargo test --release -p arena

## Measurement

On idle the Lichess machine, after the tests pass: `nnue_bench --stack` on a random network, the head's time and
nodes/s against betty's shape and greta's (psq-stack.md expects the head at about half of betty's,
plus the bucket misses); `--quantization` on hilda's first candidate.

## Out of scope

A coarser our-king key, a queen split, phase as the output key, configurable key order (psq-stack.md
decisions).
