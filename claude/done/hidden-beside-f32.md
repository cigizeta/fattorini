# The hidden layer beside the output in f32

**Status:** plan approved 2026-10-27 (the developer). Applies to `QuantizedHead::HiddenBeside`
alone (screlu_hidden, tag 18; pairwise_hidden, tag 19). Step 1 written the same day, the whole
products computed by a scalar loop on every path (`whole_products`), a kernel only if the bench asks
for one; step 2's weights test added, the existing tolerances (0.06 pawns) kept, not tightened
blind. **First bench (2026-10-27):** pairwise_hidden's lambda 0.03 at p99 29.41 cp, max 148.79,
2.4% at 20 cp or more (from 49.55, 226.85, 5.1%; SCReLU 26.13, 1.2%); but the head alone 680 to
716 ns for pairwise_hidden (213 to 224 before) and 972 to 978 for screlu_hidden (257 to 266), the
scalar `whole_products` the likely cost. So a kernel after all, `whole_products_f32_avx2`, taken
when a perspective's products are a multiple of 16. **Second bench:** tests (97) and clippy clean;
the head alone 335 to 338 ns for pairwise_hidden, 460 to 542 for screlu_hidden, still 1.5x to 2x
the 7-bit form's: the 16 rows' `dot_product_avx2`, four horizontal sums each. So
`dot_products_8_avx2`, `clipped_dot_products_8_avx2` without the clip (the two sharing their
reduction, `sums_of_8`), eight rows at once, chosen by the layer's shape alone so that both paths
agree. **Third bench:** tests and clippy clean; the head alone 291 ns for pairwise_hidden (two runs,
291.5 and 290.8), about 33% above the 7-bit form's 218, a node 325 against about 254; screlu_hidden
355 and 425 ns (noisy), against about 262. In the search, a node about 0.80 us all told, so the f32
layer costs pairwise_hidden roughly 9% of its speed: a few Elo at 10+0.1, nothing at depth 9. Kept
as it is; the duels decide whether it is worth more work (e.g. the screlu path still computes the
7-bit squares that only its scalar skip reads).

**Step 4, the depth 9 duel (2026-10-27, a dev smithy at `--scale 1` on one machine, the runs'
files copied from Drive into its tree, the fixed engine from the working tree):** test 1 of the dev
tree, `lc0-psq-threats-pairwise-hidden-g1-r1@default` against `lc0-psq-threats-screlu-g1-r1@default`,
904 games, 405.5 points, 44.9%, about -36 Elo, won by the champion. Test 17, the same duel on the
7-bit engine: 42.4%, about -53. The fix is worth about +17 Elo, within the noise of two duels of
this size, and pairwise_hidden still loses clearly at equal depth: the quantization tail was a
part of the loss, not its cause. The 10+0.1 duel not played: slower than before, it cannot do better
than at depth 9. So betty's lesson stands for pairwise_hidden (psq-threats-pairwise-hidden.md); the
fix is kept, every hidden head now evaluating closer to what was trained, screlu_hidden's coming
run included.

**Dorothy, the same day:** whether the older evidence for betty's lesson (dorothy, psq_threats with
a plain hidden layer of 16, against caroline, the same without) was a quantization artifact too.
`--quantization` over the same 3,242,784 positions: dorothy mean 2.56, p99 23.12, max 92.08, 1.7%
at 20 cp or more; caroline 1.43, 10.70, 26.13, 0.0%. Dorothy's tail is about twice caroline's (part
of it its coarser accumulator, 1.0 = 254 at shift 1 against 508 at shift 2), but under half of
pairwise_hidden's before the fix (49.55, 5.1%), whose own fix bought only about 17 Elo. Not enough to
explain dorothy's losses: the older evidence stands, and `QuantizedHead::Hidden` is left as it is.

## Why

pairwise_hidden lost both duels against SCReLU (psq-threats-pairwise-hidden.md, tests 17 and 18)
though its test loss was about 7% lower. Its quantization check found a tail four times SCReLU's
(p99 49.55 cp against 26.13, 5.1% of positions off by 20 cp or more against 1.2%), and
`--hidden-precision` (2026-10-27) found the cause: the hidden layer reads the same 7-bit products as
the skip (`product_of`, `0..=127`), and the head's weights on its 16 values, up to 10 or 11 against
the skip's 1 at most, magnify their rounding. Simulated with the whole product (`a * b / 65025`,
each lane still at 8 bits) feeding the hidden layer alone, p99 falls to 13.26 cp and the 20 cp share
to 0.3%, below SCReLU's own; the skip's input precision changes nothing.

The engine played the duels with the tail. The network is already trained and its file does not
change, so after the fix the duels can be replayed with no GPU time.

## Decisions (proposed)

1. **The hidden layer in f32, reading the whole products.** `HiddenBeside`'s `rows: QuantizedRows`
   become an f32 layer: the network's own hidden weights, each row divided by 65025 at load, and its
   biases. Its input is each product or square as the integer `a * b` (`0..=65025`, `a` and `b` the
   lanes' 8-bit activations), converted to f32, exact. This is the variant the bench measured, so
   its accuracy is known before it is built.
2. **The skip unchanged:** its 7-bit products and i16 weights, measured harmless.
3. **The file and the tags unchanged:** quantization happens at load, from the f32 weights the file
   already holds. No re-export, no migration.
4. **Scalar and vectorized bit for bit the same,** as every head today: the whole products are exact
   integers in either path, and both paths compute each row with the same `dot_product_avx2`, in the
   same order.
5. **Kings (tag 17) out of scope.** Its quantized head has the same shape (`QuantizedRows` over the
   squares, magnified by an f32 head), so likely the same tail; checked only if kings2 is ever run
   again, with `--hidden-precision` extended to it first.

**Rejected: wider integers.** The products at 15 bits (`a * b >> 1`, at most 32,512) fit `i16`, but a
row's i32 sum then needs its weights scaled to about 12 bits (a row's sum of |w| is about 25 times
its largest), giving back much of the precision gained, with a new overflow bound to prove. Worth it
only if the f32 layer's cost is too high.

## Cost

Today the hidden layer is 16 rows of 256 i16 multiply-adds, 16 lanes a `madd`: about 256 `madd`s.
In f32, 8 lanes an FMA: about 512 FMAs, plus one pass converting 256 products to f32. Guess: the head
about 1.3 to 1.5 times its cost today, a node 10% to 20% more; benched in step 3. At depth 9 the cost
does not count (the same tree); at 10+0.1 it does, which is why both duels are replayed.

## Steps

1. **Engine (`nnue.rs`, `simd.rs` if a kernel is wanted):**
   - `QuantizedHead::HiddenBeside { hidden: Layer, skip, skip_scale, head }`, `hidden` built in
     `quantize` from the network's hidden layer, its weights divided by 65025.
   - The whole products into a per-thread f32 scratch, own first: pairwise, `a * b` of each half's
     lanes; screlu, `a * a`. Vectorized when a perspective is a multiple of 16 lanes: activations
     widened to i32, multiplied (`_mm256_mullo_epi32`, or `_mm256_madd_epi16` against zeros
     interleaved), converted (`_mm256_cvtepi32_ps`); the scalar path the same integers.
   - Each row: `(bias + dot_product_avx2(row, products)).clamp(0.0, 1.0)`, then squared beside it and
     the head as today. `first_layer_values` stays for kings and the stack.
   - The skip as today, from the 7-bit products, which the screlu path keeps computing for it.
2. **Tests:** the existing quantized-against-f32 tests of both heads, their tolerance tightened to
   what the f32 layer allows; scalar and vectorized equal bit for bit (an existing test, if it
   covers `HiddenBeside`, else a new one); a test that `quantize` keeps the file's hidden weights
   exactly, divided by 65025.
3. **Bench:** `--quantization` on pairwise_hidden's lambda 0.03 over `round-robin.pgn`, expected
   near the simulation (p99 about 13 to 20 cp, the 20 cp share under 1%); the speed bench
   (`--threats --pairwise --beside --head "16"`, and screlu's) against the table of
   psq-threats-pairwise-hidden.md's step 2, two runs each, on an idle machine.
4. **Duels, from the page,** `lc0-psq-threats-pairwise-hidden-g1-r1` against
   `lc0-psq-threats-screlu-g1-r1`, both `@default`, the arenas pulled and rebuilt first: depth 9,
   then 10+0.1. The other machines' arenas rebuild the engine; the training machine is not needed.
5. **Docs:** this plan's results; psq-threats-pairwise-hidden.md and TODO.md updated with the
   duels' verdict, and with what it means for screlu_hidden's coming run (which then plays on the
   fixed engine) and for the hidden-layer grid.

## Open

- Whether to keep the 7-bit `rows` path behind a switch for an A/B: not planned, the bench's
  simulation and the duels' replay against tests 17 and 18 are the comparison.
