# A sparse i8 first hidden layer

**Why (2026-11-28, the developer):** before training pairwise at width 768 (screlu-understood.md),
know whether Reckless's zero skipping pays on our i8 first layer. Sparse at i16 lost
(sparse-first-layer.md: both kernels bound by loads); its own conclusion was to retry at 8 bits,
where an index covers four inputs and a load twice as many weights, with the lanes reordered.

## Today

`first_layer_sums` (`nnue.rs`), vectorized: the squares or products packed to u8 (`pack_u8_avx2`),
then `dots_i8_8` eight rows at a time over the row-major `[out][in]` i8 weights, by `dpbusd` with
AVX-VNNI, `maddubs` then `madd` otherwise. At 16 outputs: screlu-hidden 1024 inputs, 209 ns of a
462 ns evaluation; pairwise-hidden 512, 130 of 431 (screlu-understood.md, trial 35's positions).
All-zero blocks of four: screlu 35.1% (46.4% reordered), pairwise 25.5% (41.7% reordered).

## The change

1. **The skipped unit is a block of four u8 inputs, one i32.** Reckless's and Stockfish's.
2. **A second weight layout, built at load:** `QuantizedRows` gains `blocks: Vec<i8>`,
   block-major: for block `b`, output `o`, `blocks[(b * out_dim + o) * 4 + k] = weight[o * in_dim
   + 4 * b + k]`. At 16 outputs a block's weights are 64 bytes, two `ymm`. The file format does not
   change; the row-major `weight` stays, for the scalar path and the dense kernel.
3. **Finding the nonzero blocks:** per 32 packed bytes (eight blocks), `cmpgt_epi32` against zero
   (the bytes are at most 127, so a block read as i32 is non-negative), `movemask_ps` to an 8-bit
   mask, a 256-entry table of eight u16 indices stored unaligned, advanced by the mask's popcount.
4. **Accumulating:** per nonzero block, `set1_epi32` of its four bytes, then with each of the
   `out_dim / 8` weight registers `dpbusd` into an i32 accumulator (VNNI), or `maddubs` then `madd`
   with ones (AVX2); one lane an output, no horizontal reduction. Without VNNI, two blocks a step,
   their `maddubs` added in i16 before one `madd`, as Stockfish does, only if the bench asks: two
   products of at most `2 * 127 * 127` each overflow i16, so this needs care, and it is left out of
   the first version.
5. **Exact:** the same integer products summed in i32, so equal to the dense kernel bit for bit.
   Node counts at a fixed depth unchanged.
6. **Reordering, in the bench only at first:** the lanes sorted by how often their input is zero
   (`--first-layer` already computes the order). In the engine it would be a permutation applied at
   load to the transform's columns and the first layer's, exact, no retraining; written only if the
   bench shows it is what makes sparse pay.

## Measured, before anything is wired in

`nnue_bench --first-layer` gains the sparse timing beside the dense: the finding pass and the sums,
on the inputs as they are and reordered. The engine is not changed until these figures say so.
Run on one idle machine, both networks, trial 35's positions. Of interest: whether the CPU has
AVX-VNNI (the bench prints which kernel ran), since the two paths may differ.

**Pays** if the sparse first layer, finding included, is clearly below the dense one (10% or more of
the layer) on pairwise. At width 768 the layer reads 768 products: the same zero shares would give
the same ratio.

## Implementation plan

1. `simd.rs`: the index table (`const fn`), `nonzero_blocks_avx2(packed: &[u8], indices: &mut
   [u16]) -> usize`, `sparse_sums_i8(indices, packed, blocks, out_dim, sums)` with its VNNI and AVX2
   bodies, dispatched as `dots_i8_8`.
2. Tests in `simd.rs`: the sparse sums equal `dots_i8_8` and a scalar sum on random inputs at several
   densities, all zero, all nonzero, one nonzero block first and last; at `out_dim` 8, 16 and 32;
   both bodies where the CPU allows. The finder against a scalar filter.
3. `QuantizedRows::blocks` built in `quantize` (every quantized first layer, each `DualCount` set
   too), and a `#[doc(hidden)]` `first_layer_sums_sparse_for_bench` beside
   `first_layer_sums_for_bench`.
4. `--first-layer`: dense, sparse, sparse reordered (the reordered blocks built in the bench from the
   same permutation), each in ns and against dense; which kernel ran.
5. The developer runs it; figures here. Only then, a step to wire it into `first_layer_sums`.

## Run log

- **2026-11-28, steps 1 to 4 written:** `NONZERO_POSITIONS`, `nonzero_blocks_avx2`,
  `sparse_sums_i8` (VNNI and AVX2 bodies), `block_major` and their two tests in `simd.rs`;
  `QuantizedRows::blocks`; the two bench helpers; `--first-layer`'s sparse lines, each checked equal
  to dense on every position before it is timed.
- **2026-11-28, first bench, trial 35's 91,428 positions, VNNI:** sparse loses badly. Pairwise:
  dense 134.4 ns, sparse 345.5 (0.39x), reordered 290.9 (0.46x). Screlu: dense 222.2, sparse
  600.0 (0.37x), reordered 525.6 (0.42x). Two causes: too few blocks to skip (25.5% of pairwise's
  all zero, 41.7% reordered; Stockfish's pays at far higher sparsity), and the kernel's two
  accumulators at 16 outputs, each `dpbusd` waiting about five cycles on the last, where dense
  keeps eight in flight. The second fixed: `SPARSE_SETS` (4) accumulator sets over consecutive
  blocks, `out_dim` a const generic (8, 16, 32). Even so the first caps the gain near 1.3x
  (pairwise reordered) before overheads.
- **2026-11-28, rerun with the four sets, pairwise, same positions:** dense 127.5 ns, sparse 142.3
  (0.90x), reordered 139.4 (0.91x). The latency fix took sparse from 0.39x to 0.90x, but it stays
  slower than dense, and reordering, which skips 41.7% of the blocks against 25.5%, gains almost
  nothing: what is left is the fixed cost (packing, finding, an index and a broadcast a block), not
  the multiply-adds. **The sparse i8 layer is closed, as the i16 one was: the engine stays dense.**
  The kernels, their tests and the bench's lines are kept, for a network whose inputs are far
  sparser. Width 768 does not change this: the same zero shares, the same ratio.
