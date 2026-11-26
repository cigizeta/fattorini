# A sparse first hidden layer: implementation plan

The plan of sparse-first-layer.md. Each step leaves tests and clippy clean.

## Step 1: measure before any change

1. `zero_squares` (`benches/nnue_bench.rs`): also count all-zero pairs (`chunks(2)`), printed
   beside the blocks of four.
2. `--first-layer [dir]` in `nnue_bench`, on a halfk network with two hidden layers: the whole
   evaluation from built accumulators, the first layer's sums alone over precomputed squares or
   products, and their share, per evaluation. The bench reaches `first_layer_sums` through a `pub`
   method on `QuantizedPsqNetwork`, `#[doc(hidden)]`, taking a board's built accumulators and a
   `sparse: bool` (false until step 3).
3. The developer runs, on one idle machine, for `-screlu-hidden-512-8-budget-g1-a1` and the pairwise
   twin: the zero fractions, `--first-layer` on a test's games, and `bin/fattorini bench` with each
   network. The figures go in sparse-first-layer.md before step 2.

**Stop here if** the first layer's share is small: the gain is bounded by it.

## Step 2: the layout and the kernels

1. `QuantizedRows::columns: Vec<i16>`, built in `quantize` from `weight`: for pair `p`, output `o`,
   `columns[(p * out_dim + o) * 2 + k] = weight[o * in_dim + 2 * p + k]`.
2. `simd.rs`: a 256-entry table of `[u16; 8]` (a `const` built by a `const fn`), and
   `nonzero_pairs_avx2(inputs: &[i16], indices: &mut [u16]) -> usize`.
3. `simd.rs`: `sparse_sums_i16_avx2(inputs: &[i16], indices: &[u16], columns: &[i16], sums: &mut [i32])`,
   `out_dim / 8` accumulators held in a fixed array of `MAX_HIDDEN_OUTPUTS / 8` registers.
4. Tests in `simd.rs`: `sparse_sums_i16_avx2` after `nonzero_pairs_avx2` equals `dots_i16_8_avx2`
   and the scalar sums, on random inputs at several densities, all zero, all nonzero, and one
   nonzero pair at the first and at the last position; at `out_dim` 8, 16 and 32. The index finder
   against a scalar filter.

## Step 3: wired in

1. `first_layer_sums`, vectorized: the indices into a thread-local scratch (at most `in_dim / 2`),
   then `sparse_sums_i16_avx2`. The dense kernel kept, reached by the bench's `sparse: false`.
2. The existing tests comparing the vectorized evaluation with `evaluate_pawns` cover every head
   through it; one added for a `DualCount` network, if none reaches its vectorized path today.

## Step 4: measure after

The developer reruns step 1.3 on the same machine: `--first-layer` (dense against sparse) and
`bin/fattorini bench` for both networks, the node counts equal to step 1's. The figures go in
sparse-first-layer.md, and TODO.md's item 1 is marked done. Fusing the nonzero pass into the
activation kernels is considered only here, from these figures.
