# Betty's head: activations once, the tail in registers

**Status:** done 2026-10-07: implemented and measured (see Result). As built: the
activations go to a thread-local buffer (`ACTIVATION_SCRATCH`), sized on the first evaluation,
not a stack array of the widest input, which would be zeroed on every call (an 8 KB memset, about
as long as the saving); the sums and values are stack arrays of `MAX_HIDDEN_OUTPUTS = 64`, which
`quantize` enforces instead of a cap on the inputs. The kernels are `activations_i16_avx2`,
`dots_i16_8_avx2` (replacing `activation_dots_i16_8_avx2`) and `hidden_values_avx2`. Off the label path by construction: every value the head
produces stays bit-identical, so node counts must not move (betty 1,643,715 at depth 9).

## The profile (2026-10-07, samply, idle the Lichess machine, betty, `nnue_bench --search-only --depth 11`)

Self time, inverted call tree: `activation_dots_i16_8_avx2` 21%; the `HEAD_SCRATCH` closure in
`QuantizedPsqNetwork::evaluate_lanes` (the f32 tail: `hidden_value`, the scratch `Vec`s, the
`RefCell` borrow) 6.3%; `dot_product_avx2` (the output layer) 2.0%. The head is about 29% of
betty's search. Then "unknown" 15% (unattributed), `update_i16_avx2` 7.6%,
`MovePicker::next` 5.3%.

## Why it is slow

Betty's first layer is 512 inputs by 16 outputs, `i16` weights: 512 `_mm256_madd_epi16` per
evaluation, about 60-70 ns of arithmetic, against 141 ns measured.

- `activation_dots_i16_8_avx2` turns the 512 accumulator lanes into activations (clamp, round,
  shift) inside its loop, so the two groups of 8 rows compute all of them twice.
- The tail pushes 16 `hidden_value`s into a thread-local `Vec` behind a `RefCell`, then calls
  `dot_product_avx2` on it: for 16 floats, the bookkeeping costs more than the arithmetic.

## Design

1. **Activations once.** `evaluate_lanes` computes the activations of `own` then `opponent` into a
   stack array of `i16` (the `madd` input), with `activations_16` as today; the 8-row kernel
   takes that array instead of the raw lanes (`dots_i16_8_avx2(rows, activations)`). The array
   is `[i16; MAX_HEAD_INPUTS]`, `MAX_HEAD_INPUTS = 2048`; `quantize` refuses a wider input, as it
   refuses an accumulator that cannot fit `i16`. Integer arithmetic: exact.
2. **The tail without scratch, when the first hidden layer is the last** (betty; a deeper head
   keeps today's path). The sums go to a stack array of `f32`, each by the arithmetic of
   `hidden_value`, vectorized: `_mm256_cvtepi32_ps`, divided by the denominators, plus the
   biases, clamped with `max`/`min` to `0..=1`. Every one of those operations is correctly
   rounded per lane, as the scalar ones are, and the denominators `127 * scale` are computed at
   load exactly as `hidden_value` computes them now, so each value is bit-identical. The output
   stays `output.bias[0] + dot_product_avx2(&output.weight, values)`, over the stack array, so
   its summation order is unchanged. `hidden_value` stays the scalar reference.

Caroline (no hidden layer) and hand-crafted are untouched.

## Tests

- A kernel test: the 8-row dot product from precomputed activations equals scalar sums, as the
  current kernel's test does.
- The vectorized tail equals `hidden_value` element by element, bit for bit (`to_bits`), on sums
  and scales spanning the clamp at both ends.
- The existing check that the vectorized evaluation equals the scalar one exactly, run on betty's
  shape.
- Node counts: betty 1,643,715, caroline 1,697,834.

## Measurement

`nnue_bench` on idle the Lichess machine, before and after: betty's head timing (141 ns today) and
`--search-only` at depth 9 (1.01 s today). Expected: betty's head down by about a quarter,
search 6-8% faster.

## Result (2026-10-07, idle the Lichess machine)

Node counts unchanged (betty 1,643,715). The head alone 141 to 128 ns (-9%); betty's search at
depth 9, 1.01 s to 1.00-1.02 s, within noise (about 1%): the head's cost is its 512 `madd`s, not
what this removed. Kept anyway: the `i8` kernel below needs the same structure (activations once,
eight rows per pass, the tail in registers). The fused update shows the same in the bench
(64.8 ns per update, about as before), so caroline's +13% against hand-crafted's +10% that day
may owe little to it.

## Not in this plan

`i8` first-layer weights with `_mm256_maddubs_epi16` (Stockfish's form, half the multiplies):
measured too lossy for betty on 2026-10-07 (3.8 cp mean, 34 cp p99 from f32), so it would need a
duel, and perhaps weight clipping in training first (nnue-inference-speed.md). **Decided
2026-10-07 by the developer: generation 2 trains with weight clipping**, the first hidden layer's
weights kept within a fixed range (Stockfish's is +-127/64) so that a fixed `i8` scale loses
almost nothing; then `i8` with `maddubs`, judged by duel.
