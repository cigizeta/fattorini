# An 8-bit first hidden layer

Decided 2026-11-26 (the developer). Item 2 of TODO.md's "Optimize both screlu's and pairwise's
inference", after the sparse layer closed at i16 (sparse-first-layer.md). A retry of
i8-hidden-layer.md (archived), shelved on 2026-10-07 because betty's first layer was too small a
share of her search.

## What the earlier attempt settles

- **`maddubs` never saturates:** inputs stop at 127 and per-row i8 weights at +/-127, so a pair
  reaches at most 2 x 127 x 127 = 32,258 < 32,767. Nothing to measure (TODO.md's "how often
  `maddubs` saturates" is zero by construction).
- **Its figures, betty-0001:** against f32, i16 2.47 cp mean and 23.6 cp p99, i8 3.95 and 36.7. The
  first layer (16 x 512): i16 `madd` 116.5 ns, i8 `maddubs` (AVX2) 1.33x, i8 `dpbusd` (AVX-VNNI)
  1.65x.

## Why retry

The first layer is now 44% of screlu-hidden's evaluation (245 ns) and 28% of pairwise's (122 ns)
(sparse-first-layer.md). At betty's ratios, AVX2 would save about 61 ns and 30 ns, VNNI about 97
and 48: very roughly 8 to 13% of screlu's nodes/s and 4 to 7% of pairwise's. Estimates only: the
bench decides.

## The design

1. **Precision through the engine's own path.** A bench-only hook builds a copy of the quantized
   network whose first layer is rounded per row to +/-127 (`127 / max |w|` of the row), still stored
   as i16 with its scales and denominators: the engine's scalar `evaluate_pawns` then gives the i8
   result exactly, with no new evaluation code. Every head with a quantized first layer
   (`TwoHidden`, `Stack`, `Dual`, `DualCount`), squares or products alike.
2. **The arithmetic:** inputs as u8 (already `0..=127`), packed once per evaluation
   (`packus_epi16`, then a lane permute); weights i8, row-major as today; `maddubs_epi16` then
   `madd_epi16` with ones, or `dpbusd` where the CPU has AVX-VNNI. Integer sums, exact in any order,
   so the two i8 kernels and the scalar i8 path agree bit for bit. In the engine, the kernel is
   chosen once at load by `is_x86_feature_detected!("avxvnni")`, the AVX2 kernel the fallback:
   CCRL testers' CPUs vary, and whether the measuring machine has VNNI is not known (the developer,
   2026-11-26). The bench reports what it finds.
3. **Sparse retried on blocks of four,** the lanes reordered by zero frequency (46.3% and 41.9% of
   blocks all zero, sparse-first-layer.md): one index then covers four inputs into sixteen outputs
   in two weight loads, twice the work per load that lost at i16. Timed beside the dense i8 kernels.
   The reordering, if kept, is exact: one permutation applied at load to the transform's columns
   and the first layer's.

## Gates, before any engine change

- **Precision (the developer, 2026-11-26):** stop if the evaluation's drift from i16, on the bot's
  Lichess positions, has a mean over **2 cp** or a p99 over **20 cp**, on either network. The
  remedy then is quantization-aware training in smithy, a design of its own.
- **Speed:** the i8 first layer clearly faster than i16 on both networks, under the kernel the
  machine has; then `nnue_bench --search-only` nodes/s, on one idle machine.

## Then the duels

**Both networks (the developer, 2026-11-26):** screlu-hidden i8 against screlu-hidden i16, and
pairwise i8 against pairwise i16, each at 10+0.1, two builds, on one idle machine, the usual rule.
i8 is not exact, so only a duel says whether the speed buys more than the error costs. If it ships,
it ships for both, and the pairwise against screlu-hidden duels (TODO.md) run on that build.
