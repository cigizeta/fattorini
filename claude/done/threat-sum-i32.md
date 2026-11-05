# The threat sum in i32

Status: plan, agreed 2026-10-16 (the developer chose option 1 of four), not implemented.

## The problem

`bin/arena select dorothy` (2026-10-16) panicked loading dorothy's generation 1: λ 0.96 to 0.99
were refused by `QuantizedPsqNetwork::quantize` (`nnue.rs`), "an accumulator lane can reach 269.6
to 297.7, beyond i16 even at scale 127". λ 0.95 and 1.00 loaded. Caroline's psq_threats networks
had loaded before.

`quantize` bounds a lane by its bias plus its `MAX_ACTIVE_ROWS + MAX_THREAT_ROWS` = 37 + 128 = 165
largest row weights in magnitude, and requires `127 * bound` to fit i16: about 257. The threat
part of that bound (16 pieces each attacking 8 opponent pieces, every one at its lane's largest
weight) is far beyond any position, so the refusal is the bound's, not a real overflow. But the
bound is the only proof the i16 accumulator never wraps, and wrapping would be silent.

## The idea

The threat rows are already recomputed at every evaluation (`evaluate_by_color_cp`, into
`THREAT_SCRATCH`), never kept incrementally. So they need not live in i16 at all:

1. **The piece-square accumulator stays i16, incremental, unchanged.** Its bound covers its own
   `MAX_ACTIVE_ROWS` only: bias plus 37 largest weights. That is what `quantize` checks and
   picks `accumulator_shift` by, for psq_threats as for psq.
2. **The threat rows are summed in i32**, per perspective, from zero.
3. **The two are added in i32 and saturated to i16** into `THREAT_SCRATCH`'s copies, which feed
   `evaluate_lanes` exactly as today. Saturation is exact here, not an approximation: every head
   reads a lane only through `activation_of` (or the AVX2 kernels doing the same), which clamps to
   `0..=accumulator_one()`, and `accumulator_one()` is at most `i16::MAX`. A lane saturated to
   `i16::MAX` and one at its true value above `accumulator_one()` give the same activation. The
   head and every kernel after `THREAT_SCRATCH` stay untouched.
4. **Each quantized weight must still fit i16 on its own:** `quantize` adds a check that every
   transform weight and bias, threat rows included, times `127 << accumulator_shift`, fits i16
   (rounded). Today's lane bound implied it; the narrower one does not.

A larger `accumulator_shift` may result for psq_threats networks (the bound no longer counts 128
threat rows), so their quantization error should shrink, not grow.

## Changes

- `nnue.rs`, `QuantizedPsqNetwork::quantize`: the lane bound over `MAX_ACTIVE_ROWS` alone, for
  either kind; the per-weight check; doc comment updated (it explains why threats are not in the
  bound).
- `nnue.rs`, `evaluate_by_color_cp`'s psq_threats branch: an i32 threat sum per perspective
  (thread-local scratch, like `THREAT_SCRATCH`), then psq + threats saturated into the i16 copies.
  An AVX2 kernel for the i32 row sum (`_mm256_cvtepi16_epi32` and `_mm256_add_epi32`) and one for
  the widen-add-saturate (`_mm256_packs_epi32` saturates, but interleaves 128-bit lanes: mind
  the permute), beside the existing `add_assign_i16_avx2`.
- `MAX_THREAT_ROWS`: no longer read by `quantize`. Removed if nothing else reads it.
- `refresh` already widens to i32: unchanged. Wherever a test checks it against the i16 range,
  the check becomes "the piece-square part fits i16".

## Tests

- **The refusal that motivated this:** a psq_threats sample network whose threat rows carry
  weights large enough that today's bound refuses it (lane bound over 257) while its piece-square
  rows fit. It must now quantize, and along random games its `evaluate_by_color_cp` must equal the
  f32 `evaluate_pawns` within the quantization tolerance the other tests use.
- **Saturation exactness:** a lane whose psq + threats exceeds i16 gives the same evaluation as
  the same position computed unsaturated in i32 (the scalar reference widened).
- **The existing ones stay green:**
  `psq_threats_incremental_evaluation_matches_a_from_scratch_one_along_random_games`,
  `a_threat_row_reaches_the_accumulator`, the vectorized-equals-scalar tests.
- **A weight too large on its own** is still refused, with its own message.

```bash
cargo test -p rotor nnue
```

## Measurements (on the Lichess machine, nothing else running)

1. **Speed:** psq_threats evaluation and nodes per second, caroline before and after (she loads
   today, so both builds can run her). The threat sum runs at half the SIMD width of i16: a few
   percent of nodes per second is the expectation, not a measurement.
2. **Quantization error:** `nnue_bench --quantization` on caroline before and after (shift may
   rise), and on dorothy's six after.

## Afterwards

- Rerun `bin/arena select dorothy`.
- Open question for later generations, not part of this plan: clipping transform weights in
  training (Stockfish's trainer has a `WeightClipper`), so that the piece-square bound is never
  the next one to fail.
