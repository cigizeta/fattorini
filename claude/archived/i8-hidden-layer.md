# Betty's first hidden layer in i8

**Status: shelved 2026-10-07** (the developer), after step 1. The kernels pass the speed gate, but
the expected gain, about 7% of betty's search with VNNI, is worth perhaps +5 to +10 Elo at 10+0.1,
far below the +34 the duel rule can confirm in 1000 games, against 1.5 cp more error. Worth a retry
when the first layer is a larger share of a node (a wider hidden layer, emily). `nnue_bench --i8`
stays, to rerun then. The engine was not changed.

## Result of step 1 (2026-10-07, the training machine, replay paused)

`nnue_bench --i8` on betty-0001, over 643,550 positions of her generation-1 selection games:

| | mean | p50 | p90 | p99 | max |
|---|---|---|---|---|---|
| `i16` (today) | 2.47 cp | 0.95 | 6.26 | 23.60 | 70.28 |
| `i8` | 3.95 cp | 1.60 | 10.14 | 36.73 | 107.34 |

The first layer per evaluation (16 x 512), the three kernels checked against scalar sums: `i16`
`madd` 116.5 ns, `i8` `maddubs` (AVX2) 87.6 ns (1.33x), `i8` `dpbusd` (VNNI) 70.8 ns (1.65x).
The training machine mixes performance and efficiency cores, so the ratios are firmer than the nanoseconds.
Against about 600 ns per betty node, VNNI would save about 7% of her search, AVX2 about 5%
(estimates, not measured in a search).

The rest of this document is the design as drafted.

## Why

Betty's head is about 29% of her search (betty-head-speed.md's profile), and its cost is the first
layer's 512 `_mm256_madd_epi16` per evaluation (16 rows x 512 inputs, `i16` weights). Stockfish
runs that layer in `i8` with `_mm256_maddubs_epi16`. Generation 2 trains with weight clipping
(weight-clipping.md) for this.

## What clipping does and does not buy

The engine scales each row at load, `127 / max |w|` of the row, as it does today for `i16`. The
bound of +/-127/64 only guarantees that scale is at least 64; per row it is usually finer (betty-0001:
row maxima 0.67 to 1.94, scales 65 to 189). So **clipping does not make `i8` more precise**: the
loss measured on betty-0001 with per-row `i8` scales, 3.8 cp mean and 34 cp p99 from f32 (against
2.4 and 23 for today's `i16`), is what to expect. Stockfish needs clipping for a fixed scale; Rotor
does not use one. Clipping still keeps a single outlier from coarsening a whole row.

## The arithmetic

- Activations are `0..=127` (unchanged), packed to `u8` once per evaluation
  (`_mm256_packus_epi16`, then a lane permute to undo AVX2's in-lane packing).
- Weights `i8`, `round(w * scale_r)` per row, `|w8| <= 127`.
- `_mm256_maddubs_epi16(activations, weights)` multiplies `u8 x i8` and adds adjacent pairs into
  saturating `i16`. A pair reaches at most `2 * 127 * 127 = 32,258 < 32,767`: **no saturation**,
  which is why activations stop at 127. Then `_mm256_madd_epi16` with ones widens to `i32`.
- The value is `hidden_value(bias, sum, scale_r)` as today: integer sums, exact in any order, so the
  vectorized path still equals the scalar one bit for bit.

Per 32 inputs: AVX2 `i8` is 3 instructions (`maddubs`, `madd`, `add`) against `i16`'s 4, and half
the weight bytes. **AVX-VNNI** (`_mm256_dpbusd_avx_epi32`) is 1. Estimate, not measured: a few
percent of betty's search under AVX2, more under VNNI.

## Plan, with a gate before the duel

1. `nnue_bench --i8`: quantize betty's first layer to `i8` per row, and report both the precision
   (as `--quantization` does) and the head's time against `i16`, for both kernels, AVX2
   (`maddubs`) and AVX-VNNI (`dpbusd`). Scalar reference plus the two kernels only: no engine change
   yet.
2. **Gate:** if the head is not clearly faster (say under 10% of the head), stop here and record
   why. Otherwise:
3. `QuantizedHead::Hidden` holds `i8` weights instead of `i16`; `dots_i8_8_vnni` and
   `dots_i8_8_avx2` beside the activation packing, chosen once at load by
   `is_x86_feature_detected!("avxvnni")`, the AVX2 kernel the fallback, since CCRL testers' CPUs vary.
   Both give the same integer sums, so the choice never changes a value. `i16` goes (no dead code),
   unless the duel keeps it.
4. Duel: betty `i8` against betty `i16` at 10+0.1 on an idle machine with VNNI, cutechess-cli with two
   builds, as int8 against f32 was (nnue-inference-speed.md). One duel, the usual rule.

## Tests

- The `i8` kernel equals a scalar `u8 x i8` dot product, including rows at +/-127 and activations at
  127 (the saturation edge).
- The vectorized evaluation equals the scalar one exactly, on betty's shape.
- Node counts of amanda and caroline unchanged (they have no hidden layer); betty's change, and are
  recorded as the new reference.

## Open questions

1. VNNI: `is_x86_feature_detected!` checks at run time; a CPU without VNNI takes the AVX2 kernel,
   slower but computing the same values.
2. Should step 1 measure on betty-0001 now, or wait for generation 2's betty, whose weights are what
   will ship? Proposal: now, since the precision barely depends on the clipping (above).
3. If the loss is the problem, the remedy is quantization-aware training (rounding simulated in the
   forward pass), a trainer change of its own. Out of scope here.
