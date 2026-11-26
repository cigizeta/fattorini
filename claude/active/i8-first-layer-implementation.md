# An 8-bit first hidden layer: implementation plan

The plan of i8-first-layer.md. Each step leaves tests and clippy clean.

## Step 1: precision, in the bench

1. `QuantizedPsqNetwork::with_i8_first_layer(&self) -> QuantizedPsqNetwork`, `#[doc(hidden)]`:
   a clone whose every first-layer `QuantizedRows` (all buckets) is requantized from the f32 layer
   at 127 levels per row, stored as i16, with its scales and denominators to match (the sparse
   `columns` rebuilt). `QuantizedRows::quantize` takes the level count (`i16::MAX` or `i8::MAX`).
2. `nnue_bench --i8` extended, not replaced: its AVX2 and VNNI i8 kernels already work on any input
   in `0..=127`, squares and products alike. On a network with two hidden layers, over a PGN or
   folder (an absolute path): the drift of the i8 copy's `evaluate_pawns` from the engine's i16
   `evaluate_pawns`, and both from the f32 network's, each as mean, p50, p90, p99, max in
   centipawns; then the first layer's time with i16 `madd`, i8 `maddubs` and i8 `dpbusd`, which is
   step 2's dense half. **Written 2026-11-26.** The hook is
   `QuantizedPsqNetwork::quantize_with_i8_first_layer(&PsqNetwork)`.
3. The developer runs it on both `-512-8-budget-g1-a1` networks. **Gate:** mean over 2 cp or p99
   over 20 cp against i16, on either, stops here.

## Step 2: the kernels, in the bench

1. `simd.rs`: `pack_u8_avx2` (i16 `0..=127` to u8, in order), `dots_i8_8_avx2` (`maddubs`, `madd`
   with ones) and `dots_i8_8_vnni` (`dpbusd`, behind `#[target_feature(enable = "avxvnni")]`), eight
   rows at once as `dots_i16_8_avx2`. Tests: both equal a scalar u8 x i8 sum, including weights at
   +/-127 and inputs at 127.
2. A sparse i8 kernel on blocks of four: `nonzero_blocks_avx2` (as `nonzero_pairs_avx2`, on u8
   blocks of four) and `sparse_sums_i8_avx2`, weights block-major (`[block][out][4]`). Tests as the
   i16 sparse kernel's.
3. `--first-layer` times, beside i16 dense: i8 dense (AVX2, and VNNI when present), and i8 sparse on
   the inputs as they are and reordered by zero frequency, each checked equal to the scalar i8 sums
   before it is timed. It prints whether the CPU has AVX-VNNI.
4. The developer runs it on both networks. **Gate:** the i8 layer clearly faster on both.

## Step 3: in the engine (written 2026-11-26, i8 for both: the developer)

Done as below, the row-major kernel (`dots_i8_8`: VNNI when the CPU reports it, checked per call
through `is_x86_feature_detected!`'s cache, else AVX2), packing through a per-thread scratch. Every
head with a `QuantizedRows` first layer is i8: `TwoHidden`, `Dual`, `DualCount`, `Stack` and the old
`Kings`; the old single-layer `Hidden` stays i16, its own `quantize_rows` call. The i16 sparse
kernels, the bench's i8 copy of the network and the bench's sparse timings are removed; the zero and
reordering statistics stay in `--first-layer`, the bench's i16 and i8 kernel copies in `--i8`.

1. `QuantizedRows` holds i8 weights (and the block-major copy, if sparse won); the kernel chosen
   once at load, VNNI or AVX2. The i16 path goes, the scalar path becomes u8 x i8: no dead code.
2. If the reordering won: the permutation applied at load to the transform's columns and the first
   layer's, every bucket's.
3. Tests: the vectorized evaluation equals the scalar one exactly on every head; the existing
   vectorized-against-scalar tests cover it.
4. Node counts at depth 9 change (not exact): recorded as the new reference, with nodes/s, both
   networks, on one idle machine.

**The new reference (2026-11-26),** `nnue_bench --search-only --depth 9`, 49 positions, one thread,
one run each, the same machine as the i16 figures:

| network       | nodes, i16 -> i8          | nodes/s, i16 -> i8      | time, i16 -> i8 |
|---------------|---------------------------|-------------------------|-----------------|
| screlu-hidden | 2,483,194 -> 2,633,639    | 1,410,737 -> 1,563,411  | 1.76 -> 1.68 s  |
| pairwise      | 1,385,108 -> 1,008,066    | 1,566,534 -> 1,508,564  | 0.88 -> 0.67 s  |

Nodes/s: screlu about +11%, pairwise about -4%, both within what these short runs swing (screlu's
i16 measured 1,492,733 in a second run). The node counts moved a lot (pairwise -27%): a search tree
at depth 9 over 49 positions is chaotic, and small evaluation changes reshape it, so time to depth
says little here. The duels decide.

## Step 4: the duels

Both networks, i8 against i16, 10+0.1, two builds (the i16 build the commit before step 3), one
idle machine, the usual rule. Results in i8-first-layer.md and TODO.md.
