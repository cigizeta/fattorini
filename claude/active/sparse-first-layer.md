# A sparse first hidden layer

Decided 2026-11-26 (the developer). Item 1 of TODO.md's "Optimize both screlu's and pairwise's
inference"; the 8-bit first layer (item 2) is a separate design, after this one's bench.

## Today

Every head with a quantized first hidden layer goes through `first_layer_sums` (`nnue.rs`):
screlu's `TwoHidden`, pairwise's `TwoHidden`, `Stack`, `Dual` and `DualCount`. Vectorized, it calls
`dots_i16_8_avx2` (`simd.rs`), dense over the row-major `[out][in]` i16 weights, eight rows at a
time. At `-512-8-budget` the input is 1024 squares (screlu) or 512 products (pairwise), into 16
outputs. A square is zero when its lane is clipped; a product when either of its lanes is.

## The change

1. **The skipped unit is a pair of i16 inputs (one i32),** not four: the inputs stay i16, and
   `madd_epi16` already pairs neighbours. Groups of four bytes belong to the 8-bit layer, whose
   inputs are u8. A pair is zero at least as often as a group of four.
2. **A second weight layout, built at load:** `QuantizedRows` keeps its row-major `weight` (the
   scalar reference and the dense kernel) and gains `columns`, pair-major: for each input pair, its
   `out_dim x 2` i16 weights contiguous (`[pair][out][2]`), 64 bytes at 16 outputs, two `ymm`
   loads. 32 KB per layer at screlu's 1024 inputs; DualCount holds one per bucket. The network file
   format does not change.
3. **Finding the nonzero pairs,** Stockfish's `find_nnz`: per eight pairs (one `ymm` of i16),
   `cmpgt_epi32` against zero (inputs are non-negative), `movemask_ps` to an 8-bit mask, a 256-entry
   table of eight u16 indices, stored unaligned and advanced by the mask's popcount. A separate pass
   over the squares or products already computed; fused into `screlu_activations_i16_avx2` and
   `pairwise_activations_i16_avx2` only if the bench shows the pass costs something.
4. **Accumulating:** per nonzero pair, `set1_epi32` of the pair, `madd_epi16` with each of the
   `out_dim / 8` weight registers, added into `out_dim / 8` i32 accumulators, one lane an output:
   no horizontal reduction.
5. **Exact:** the same integer products, summed in i32; wrapping addition gives the same bits in any
   order, so the sums equal the dense kernel's even if one wrapped. Node counts at a fixed depth are
   unchanged.
6. **When it applies:** the vectorized path, `out_dim` a multiple of 8 and the input a multiple of
   16, as today's dense kernel. The scalar path is untouched and stays the reference.
7. **Out of scope:** the old `Hidden` and `HiddenBeside` heads, which do not go through
   `first_layer_sums` and which no network of the current line uses.

## Measured before and after

- **Zero fraction** on test positions, both networks: `zero_squares` in `nnue_bench.rs` already
  reports zero squares and all-zero blocks of four; it adds all-zero pairs, what this design skips.
- **The first layer's share of an evaluation:** a new `--first-layer` diagnostic in `nnue_bench`,
  as `--threat-parts`: the whole evaluation, the first layer's sums alone (dense, then sparse once
  written), and their share.
- **The bench,** `bin/fattorini bench` with each network on one idle machine (CLAUDE.md), before
  and after: nodes/s, and the node counts equal, which checks exactness end to end.

No claim on which kind gains more stands before the bench (TODO.md, 2026-11-25).
