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
- **The bench,** `nnue_bench --search-only` with each network (`bin/fattorini bench` embeds none) on one idle machine (CLAUDE.md), before
  and after: nodes/s, and the node counts equal, which checks exactness end to end.

No claim on which kind gains more stands before the bench (TODO.md, 2026-11-25).

## Step 1's figures (2026-11-26)

`--first-layer` on the bot's Lichess games (33,064 positions), both `-512-8-budget-g1-a1`, one
machine, dense kernel:

| network       | whole evaluation | first layer      | zero inputs | zero pairs | zero blocks of 4 |
|---------------|------------------|------------------|-------------|------------|------------------|
| screlu-hidden | 556.3 ns         | 244.5 ns (44.0%) | 76.5%       | 58.7%      | 35.5%            |
| pairwise      | 429.7 ns         | 121.7 ns (28.3%) | 72.4%       | 52.1%      | 26.4%            |

- **The first layer is a large share, so the work goes on.** The sparse kernel does as many
  multiply-adds per instruction as the dense one (two inputs into eight rows against sixteen inputs
  into one row), so the layer's time should fall roughly with the zero pairs, less the finding pass:
  at best about 143 ns off screlu's evaluation (26%) and 63 ns off pairwise's (15%).
- **The hypothesis that pairwise saves the larger fraction is refuted:** fewer of its pairs are
  zero (52.1% against 58.7%). A product is zero when either of its lanes is, but pairwise's lanes are
  zero less often than screlu's. In absolute time screlu gains about twice as much, which narrows
  pairwise's speed edge.
- **Nodes/s before, dense,** `nnue_bench --search-only --depth 9`, 49 positions, one thread, one
  run each (short, so noisy): screlu-hidden 1,410,737 (2,483,194 nodes in 1.76 s), pairwise
  1,566,534 (1,385,108 nodes in 0.88 s).
- **The sparse kernel, written and exact, does not pay (2026-11-26).** The same bench and machine:
  screlu's first layer 242.6 ns dense, 252.7 ns sparse (0.96x); pairwise's 120.9 and 134.9 ns
  (0.90x). The sums equal on every position, and `--search-only` gave the same node counts as
  before (2,483,194 and 1,385,108), its nodes/s within noise (1,492,733 and 1,554,072). Why: at i16
  both kernels are bound by loads, not multiply-adds. The dense one loads one input block and
  sixteen weight registers per sixteen inputs, about 1,088 loads for screlu. The sparse one loads an
  index, the pair and two weight registers per nonzero pair, 211 of 512, about 844, and the finding
  pass makes up the rest. **The engine stays dense;** the kernels, their tests and the bench's
  dense-against-sparse timing are kept.
- **Next, two levers (the developer, 2026-11-26):** zeros clustered by reordering the lanes, as
  Stockfish does: 76% of inputs are zero but only 59% of pairs, so they are scattered. Exact: the
  same permutation applied at load to the transform's columns and the first layer's weights, with no
  retraining. Measured first in the bench alone (`--first-layer` reprints the zero pairs and the
  sparse timing on reordered inputs). Then the 8-bit layer (item 2), where an index covers four
  inputs and a load twice the products: the setting in which Stockfish's sparse layer pays.
- **Reordering measured, too little (2026-11-26).** The inputs sorted by how often each is zero
  over the same positions, one order for both perspectives:

  | network       | zero pairs     | zero blocks of 4 | inputs never nonzero | sparse, reordered, against dense |
  |---------------|----------------|------------------|----------------------|----------------------------------|
  | screlu-hidden | 58.7% -> 63.1% | 35.5% -> 46.3%   | 0 of 512             | 1.07x                            |
  | pairwise      | 52.1% -> 58.2% | 26.4% -> 41.9%   | 0 of 256             | 0.93x                            |

  No input is always zero, and the zero frequencies are too even to cluster much. The screlu run
  was disturbed (its whole evaluation 721 ns against 547 before), so its 1.07x is uncertain and at
  best marginal. **The sparse layer at i16 is closed.** Blocks of four gain the most from
  reordering, and they are the 8-bit layer's unit: sparse with reordering is retried there, measured
  the same way, and nowhere sooner.
- **Pairs, not blocks of four, confirmed:** at i16, a block of four is all zero far less often (35.5%
  and 26.4%).
