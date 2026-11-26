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

## Step 1's figures (2026-11-26)

`nnue_bench --i8` on the bot's Lichess games (33,064 positions), both `-512-8-budget-g1-a1`, one
machine. **It has AVX-VNNI.**

| centipawns          | mean | p50  | p90  | p99   | max   |
|---------------------|------|------|------|-------|-------|
| screlu, i8 from i16 | 1.34 | 0.62 | 2.91 | 13.31 | 24.71 |
| screlu, i16 from f32| 1.10 | 0.66 | 2.66 | 6.61  | 14.32 |
| screlu, i8 from f32 | 1.83 | 0.96 | 4.27 | 14.50 | 30.18 |
| pairwise, i8 from i16 | 1.93 | 1.52 | 4.13 | 8.64 | 19.65 |
| pairwise, i16 from f32| 1.27 | 0.75 | 3.12 | 7.43 | 17.55 |
| pairwise, i8 from f32 | 2.27 | 1.61 | 5.21 | 11.15 | 32.00 |

**The precision gate passes on both** (mean under 2 cp, p99 under 20), pairwise's mean close to
the limit.

The first layer, the bench's kernels, 1000 positions: screlu i16 210.3 ns, AVX2 153.5 (1.37x), VNNI
120.0 (1.75x); pairwise i16 92.3 ns, AVX2 85.9 (1.07x), VNNI 82.4 (1.12x). The kernels then
allocated their result on every call, a fixed cost weighing most on pairwise's small layer: rerun
with the result on the stack before the speed gate is judged.

**Rerun, the result on the stack:** screlu i16 179.6 ns, AVX2 127.7 (1.41x), VNNI 91.9 (1.95x):
about 88 ns saved per evaluation of about 550, **the speed gate passed**. Pairwise i16 69.6 ns,
AVX2 64.1 (1.08x), VNNI 61.3 (1.14x): about 8 ns of about 430, under 2%, **not clearly faster**.
Per instruction, pairwise's i8 layer is slower than screlu's (61.3 ns for 256 `dpbusd` against 91.9
for 512): fixed costs (packing, reductions, the eight-row chains' latency over only 16 blocks)
that 512 inputs cannot spread as 1024 can.

**A block-major kernel tried (the developer, 2026-11-26: tune the kernel, then decide):** weights
`[block of 4][16 outputs][4]`, one broadcast of four input bytes and two multiply-adds for all
sixteen outputs, no reduction, four blocks in flight. Pairwise: AVX2 1.09x, VNNI 1.08x, no better.
Screlu: AVX2 1.45x, VNNI 1.77x (the row-major VNNI 1.59x in the same run). Three kernels landing at
62 to 65 ns on pairwise points to a floor common to all of them, most likely the bench itself: it
timed over 1000 positions' stored inputs, 1 to 2 MB, so every call streamed its inputs from L3 or
memory, which the engine's freshly computed inputs never do. Rerun timed over 16 positions in cache.

**Timed in cache, the floor was not the bench (2026-11-26):** pairwise i16 69.4 ns, row-major AVX2
1.10x, VNNI 1.19x, block-major AVX2 1.08x, VNNI 0.89x; screlu i16 181.0 ns, row-major AVX2 1.49x,
VNNI 1.55x, block-major AVX2 1.47x, VNNI 1.15x. The timings are noisy: screlu's block-major VNNI
was 102.5 ns in the previous run and 156.9 here, the code unchanged (the machine not idle, or the
bench moved between core types). What holds across every run: **screlu's first layer about 1.45x to
1.6x faster in i8, about 60 to 90 ns off an evaluation of about 550; pairwise's 1.07x to 1.19x with
every kernel tried, about 10 ns of about 430, under 2%, for 1.93 cp mean drift.** More kernel tuning
in the bench is not expected to change that.

## Then the duels

**Both networks (the developer, 2026-11-26):** screlu-hidden i8 against screlu-hidden i16, and
pairwise i8 against pairwise i16, each at 10+0.1, two builds, on one idle machine, the usual rule.
i8 is not exact, so only a duel says whether the speed buys more than the error costs. If it ships,
it ships for both, and the pairwise against screlu-hidden duels (TODO.md) run on that build.
