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
   lane (screlu's squares, pairwise's products), and the first layer's nnz. A small read-only
   command or a `nnue_bench` mode; a design line of its own before it is written.
2. **Controlled variants, only if step 1 points at something.** The transform width is capped at
   768 (512 + 256, the developer, 2026-11-28); 1024 is too much. Both variants feed the head
   768 inputs, so they compare a square with a product at equal head input:
   - **screlu-hidden at width 384:** 768 squares, a smaller transform than today's.
   - **pairwise-hidden at width 768:** 768 products, a larger transform than today's.
   Each duelled against today's two at 10+0.1, `@untuned`. Step 1 may instead suggest squares at a
   finer scale (fewer rounded to 0), at width 512 unchanged.

## Decision this informs

The base for the piece-count buckets. Until the steps say otherwise, both remain candidates:
pairwise-hidden compliant now, screlu-hidden needing its castling and en passant rows.

## Run log

(empty)
