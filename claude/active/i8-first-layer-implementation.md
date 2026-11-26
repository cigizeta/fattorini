# An 8-bit first hidden layer: implementation plan

The plan of i8-first-layer.md. Each step leaves tests and clippy clean.

## Step 1: precision, in the bench

1. `QuantizedPsqNetwork::with_i8_first_layer(&self) -> QuantizedPsqNetwork`, `#[doc(hidden)]`:
   a clone whose every first-layer `QuantizedRows` (all buckets) is requantized from the f32 layer
   at 127 levels per row, stored as i16, with its scales and denominators to match (the sparse
   `columns` rebuilt). `QuantizedRows::quantize` takes the level count (`i16::MAX` or `i8::MAX`).
2. `nnue_bench --i8` replaced: on a network with a quantized first layer, over a PGN or folder
   (an absolute path), the drift of the i8 copy's `evaluate_pawns` from the engine's i16
   `evaluate_pawns`, and both from the f32 network's, each as mean, p50, p90, p99, max in
   centipawns. The old single-hidden-layer `--i8`, with its own kernels, goes: no network of the
   current line has that head.
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

## Step 3: in the engine

1. `QuantizedRows` holds i8 weights (and the block-major copy, if sparse won); the kernel chosen
   once at load, VNNI or AVX2. The i16 path goes, the scalar path becomes u8 x i8: no dead code.
2. If the reordering won: the permutation applied at load to the transform's columns and the first
   layer's, every bucket's.
3. Tests: the vectorized evaluation equals the scalar one exactly on every head; the existing
   vectorized-against-scalar tests cover it.
4. Node counts at depth 9 change (not exact): recorded as the new reference, with nodes/s, both
   networks, on one idle machine.

## Step 4: the duels

Both networks, i8 against i16, 10+0.1, two builds (the i16 build the commit before step 3), one
idle machine, the usual rule. Results in i8-first-layer.md and TODO.md.
