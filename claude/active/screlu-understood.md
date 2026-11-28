# Our screlu, understood

**Why (2026-11-28):** screlu-hidden and pairwise-hidden are level on the clock (trial 35, 48.6% for
pairwise over 762 games; 51.0% before the grain fix). Our screlu-hidden is not the crowd's
SCReLU: before choosing the base for the piece-count buckets, and before keeping screlu at all,
we want to know what ours does and why it is level with pairwise.

## What ours is

Halfk_threats_screlu_hidden (feature set 21):

1. Every transform lane clipped to [0, 1] and squared: 2 x 512 = 1024 squares, both perspectives.
2. The squares quantized as pairwise's products are, 0..127 (`PRODUCT_ONE`), each one the lane's
   product with itself (`reads_products`), into an i8 first layer.
3. 16 values clipped, then 32 clipped, then 1 output, no skip.

Pairwise-hidden at the same width: each lane clipped, its two halves multiplied, 2 x 256 = 512
products into the same head.

## What the crowd does

- **Single-layer networks** (768 -> N -> 1, Bullet's default) put SCReLU on the transform, the
  square computed in i32 from lanes scaled to 255, unrounded, straight into the output.
- **Networks with hidden layers** (Stockfish, Reckless, most strong engines) put pairwise on the
  transform; any squaring is inside the small hidden layers.

Ours is the second shape with the first's activation: pairwise with each lane multiplied by
itself instead of by its partner.

## What may matter

- **The head's input twice pairwise's** (1024 against 512) at equal transform width. Trial 35's
  depths are equal, so it costs no measurable time at 16 outputs.
- **Small values lost:** a square rounds to 0 below about 0.09 of a lane's range. Sparser
  (cheaper), but the low range a single-layer SCReLU keeps is gone.
- **A square against a product:** the square is a convex response to one feature; a product
  pairs two. Pairwise level with half the inputs suggests a product is worth more an input.

## The steps

1. **Statistics, no training.** On a sample of the bench and validation positions, for both
   networks: the share of the head's inputs that are 0, the share rounded to 0 from a nonzero
   lane (screlu's squares, pairwise's products), and the first layer's nnz. **Done in
   `nnue_bench --first-layer`** (2026-11-28): its zero shares already there, `zero_squares` adds the
   inputs rounded to zero from active lanes, the lanes active and saturated, and the mean nonzero
   input.
2. **Controlled variants, only if step 1 points at something.** The transform width is capped at
   768 (512 + 256, the developer, 2026-11-28); 1024 is too much. Both variants feed the head
   768 inputs, so they compare a square with a product at equal head input:
   - **screlu-hidden at width 384:** 768 squares, a smaller transform than today's.
   - **pairwise-hidden at width 768:** 768 products, a larger transform than today's.
   Each duelled against today's two at 10+0.1, `@untuned`. Step 1 may instead suggest squares at a
   finer scale (fewer rounded to 0), at width 512 unchanged.

## The next experiment (agreed 2026-11-28)

- **Network:** `lc0-halfk-threats-pairwise-hidden-768-8-budget`, pairwise-hidden-512-8-budget's
  config with `embedding_dim` 768 and nothing else changed.
- **Shape:** 768 lanes a perspective, two perspectives; each perspective's halves multiplied, 384
  products, 768 in all; then 16 -> 32 -> 1, clipped; dense i8 first layer (sparse closed,
  sparse-i8-first-layer.md); 8 king buckets; no piece-count buckets; `unit: lc0`. Reckless's body
  less its two extra king buckets and its output buckets.
- **Question:** is pairwise worse per lane or per input? Its final test loss against screlu-512's
  0.6141 and pairwise-512's 0.6148: at or below 0.6141 means per lane.
- **Then:** `nnue_bench --first-layer` (a cheaper first layer than screlu-512's, a dearer update),
  and a duel against screlu-hidden at 10+0.1, both `@untuned`.

**After it (the developer, 2026-11-28): screlu-hidden at 768 too,** for the fair pair at equal
width: 768 lanes a perspective, 1536 squares into the same 16 -> 32 -> 1. Its first layer reads
twice pairwise-768's inputs (about 300 ns at today's 209 for 1024), so its loss is weighed against
its cost: the duel decides, not the loss.

**Then, a candidate (the developer, 2026-11-28): pairwise at 1024,** lifting the 768 cap for this
one experiment, only if pairwise-768 wins its duel against screlu-hidden clearly on the clock. A
draw means the speed ate the loss gain, and 1024 would fare worse. Its cost predicted first from
768's `nnue_bench` against 512's (the transform update and threat rows linear in the width, the
first layer at 1024 products as screlu-512's, the refresh cache about 256 KB a thread); training
about 105 s a round, 17 to 18 hours. After screlu-768.

## Decision this informs

The base for the piece-count buckets. Until the steps say otherwise, both remain candidates:
pairwise-hidden compliant now, screlu-hidden needing its castling and en passant rows.

## Run log

- **2026-11-28, pairwise-768 at round 17:** the mean test loss over rounds 8 to 17 (steps 28,672
  to 65,536) 0.62079, against screlu-512's 0.62112 and pairwise-512's 0.62169 at the same steps,
  below both in every one of the ten rounds, its lead over screlu growing (level at round 2).
  Pairwise at 512 lacked inputs, not quality per input. Final loss, `nnue_bench` and the duel to
  come.
- **Round 59:** rounds 50 to 59 (steps 204,800 to 241,664) 0.61784, against screlu-512's 0.61843
  (+0.00059, up from +0.00034) and pairwise-512's 0.61883 (+0.00098): further ahead of screlu than
  screlu ever was of pairwise-512.

- **2026-11-28, `nnue_bench --first-layer` on trial 35's 91,428 positions, one machine:**

  | | screlu-hidden | pairwise-hidden |
  |---|---|---|
  | whole evaluation | 461.7 ns | 431.0 ns |
  | first layer | 209.3 ns (45.3%), 1024 inputs | 130.1 ns (30.2%), 512 inputs |
  | inputs zero | 76.4% | 72.0% |
  | rounded to zero from active lanes | 4.5% | 1.2% |
  | lanes active | 28.1% | 53.3% |
  | lanes saturated | 0.2% | 7.3% |
  | mean nonzero input (of 127) | 17.6 | 24.8 |
  | nonzero inputs a position | about 242 | about 143 |

  Read: screlu's transform is mostly idle (72% of lanes clipped to 0) and its active lanes small
  (almost none reach 1, a typical one about 0.37); pairwise uses its lanes twice as often and to
  their top. The rounding costs little: 4.5% of the inputs, each worth under half a step of 127,
  so finer squares are not worth a variant. Screlu reads 70% more nonzero inputs for the same
  strength, its first layer 60% dearer; the 7% a whole evaluation did not show as depth in trial 35.
- **2026-11-28, the two training histories** (`history.tsv`, same data, schedule, curve and
  steps; only the transform differs): pairwise's test loss above screlu's from the first round to
  the last, by a nearly constant gap: 0.6307 against 0.6299 at step 4096, 0.6148 against 0.6141
  at the end (2.42M steps). Both flattening at the end. A gap there from the start and never
  closing reads as capacity, not as slower optimization: pairwise's head reads 512 inputs,
  screlu's 1024, from the same 512 lanes. The gap is not Elo: trial 35 is level. Pairwise
  halves the head's input to pay for a wider transform: Reckless runs it at 768 a side (384
  products a side, 768 in all); at our 512 we take the halving without the width. **Then:**
  pairwise at width 768, Reckless's (768 products, still a cheaper first layer than screlu's 1024
  squares), asks whether
  pairwise is worse per lane or per input: its loss at or below screlu-512's means per lane.

  **Next (lanes):** the width-384 screlu (768 squares, a cheaper first layer and update) is the variant
  that tests whether screlu's idle lanes are spare: equal to today's screlu means they were.
