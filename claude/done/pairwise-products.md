# Pairwise products: every pair of pieces through the accumulator's two halves

**Status:** design, drafted 2026-10-14 with the developer; name, width and quantization settled
2026-10-15. Direction 1 of TODO.md's
"Attention in NNUEs beyond HalfK". Nothing implemented.

## Why

Psq networks are limited by capacity, not by data: train and test loss end nearly equal (caroline
gen-1, lambda 0.95: 0.0637 against 0.0640 after about 2.9 passes over 362M rows), and every run
stops because it cannot improve further. So the lever is what the network can represent.

Amanda's evaluation is linear in the pieces after the clamp: each active (piece, square) feature adds
its row to the accumulator, and the head is one weighted sum of the clamped values. A piece's
contribution cannot depend on another piece, except through the clamp. Betty's hidden layer of 16
adds interactions only through its own clamps.

Multiplying the two halves of the accumulator gives every pair of pieces an explicit interaction
term, at almost no cost, with the incremental update unchanged.

## The idea

Per perspective, the psq transform gives 256 values, clamped to `[0, 1]` (clipped ReLU, as today,
`NetworkPsq::pawns`). Split them into `a` (values 0 to 127) and `b` (values 128 to 255). Both come
from the same active features, through different rows of the same transform:

- `a_i = sum over active features f of u_i[f]` (plus bias, clamped)
- `b_i = sum over active features g of v_i[g]` (plus bias, clamped)

So, before the clamps, `a_i * b_i = sum over f, sum over g of u_i[f] * v_i[g]`: a term for every
pair of active pieces, a pair counted in both orders, and a piece paired with itself acting as a
per-piece term. Each `i` is one learned rank-1 pair pattern, "group A present AND group B present";
the 128 together are a rank-128 approximation of the full 780 x 780 table of pair interactions. The
clamps keep it an AND: if either half is 0, the product is 0.

Amanda with products:

    eval = sum_i w_i  * a_own_i * b_own_i
         + sum_i w'_i * a_opp_i * b_opp_i
         + c

A weighted dot product of the two halves per perspective. The weights are needed: every product is
in `[0, 1]`, so without them the output could never fall below `c`; the sign of `w_i` makes a pair
pattern good or bad, and its size scales it.

### With a hidden layer (betty's shape)

Per perspective, in order:

1. Transform: 256 values, clamped to `[0, 1]`.
2. Split: `a` (128) and `b` (128).
3. Elementwise product: `p = a * b`, 128 values, not summed.
4. Hidden layer: each of the 16 units computes `sum_i W_ki * p_i + c_k`, a weighted dot product of
   `a` and `b` with its own weights, then clamps.
5. Output layer: a weighted sum of the 16 clamped units.

So the dot products happen inside the first layer after the transform, not before it: summing the
128 products into one number before the hidden layer would leave it 2 inputs instead of 256 and lose
almost everything the products carry. With a linear head (amanda's shape), steps 4 and 5 collapse
into one weighted sum, the output, so the whole head is one weighted dot product; with a hidden
layer of 16, there are 16 of them, each clamped, then combined.

**No weights of its own.** The product has no parameters: the only weights are the output layer's
(or the hidden layer's), which every network has anyway. Signed weights are needed there, since every
product is in `[0, 1]`.

Known elsewhere as factorization machines (Rendle, 2010: every pairwise interaction of sparse
features through a low-rank factorization, in linear time) and gated linear units (`(Ux) * (Vx)`
elementwise, as in today's transformers' feed-forward layers). Stockfish's feature transformer is
believed to do the same since about 2022 (to confirm, open question 1).

## Shape and cost

| | amanda today | amanda with products |
|---|---|---|
| Transform | 780 x 256 | 780 x 256, unchanged |
| Incremental update | | unchanged |
| Into the head | 512 clamped values | 256 products |
| Head parameters | 513 | 257 |
| Arithmetic after the update | 512 clamps + 512 multiply-adds | 512 clamps + 256 multiplies + 256 multiply-adds |

About as fast: the multiplies cost what the halved head saves, and the update, the largest part of an
evaluation (about 100 ns against about 30 ns of head arithmetic, nnue-inference-speed.md), does not
change. Training costs the same: the transform, most of a round, is unchanged.

## The experiment

One new architecture beside amanda, on the same corpus and teacher, so that the only difference is
the product: psq, embedding 256, no hidden layer, products. Judged first by test loss against amanda
at the same lambda, then by games (`arena select`, then a duel against amanda's candidate).

A capacity-limited network should show the gain in train and test loss together. A lower test loss
alone does not decide: games do, as for threats and the hidden layer.

Later, separately: products with betty's hidden layer, and with psq_threats (caroline's shape).

## Implementation sketch (detailed in pairwise-products-implementation.md)

1. **`net_config.rs`:** a new kind `psq_pairwise` (open question 4), psq's features with the halves
   multiplied. First config: `configs/greta.yaml`, `embedding_dim: 256`, no hidden layer.
2. **Trainer:** `NetworkPsq::pawns` and `StackedNetwork`'s psq path split each perspective's clamped
   transform output and multiply the halves before the value head; the head's input width becomes
   `embedding_dim` instead of `embedding_dim * 2`. The sparse transform (sparse-transform.md) is
   untouched.
3. **Export:** the `.nnue` header records the option, so the engine knows the shape.
4. **Engine:** halves quantized to `0..=255`, 1.0 at 255 (the accumulator's scale
   `255 << accumulator_shift`); the product is `(a_q * b_q) >> 9`, in `0..=127`, where 1.0 is
   `255 * 255 / 512`, about 127.002. That constant goes into the head's f32 rescale
   (`output_value`) in place of 127, so the integer path matches the f32 network exactly up to
   rounding. The head's `i16` weights are quantized as today.
5. **Tests:** the stacked network equals the unstacked one with products; the quantized evaluation
   stays within tolerance of the f32 one; gradients reach both halves; node counts of existing
   networks unchanged.

## Measurement

- Test loss against amanda, at every lambda, same test set.
- `nnue_bench` on idle the Lichess machine: the head's time and nodes/s against amanda's.
- Train seconds per round on the training machine, alone if possible (the generation 3 per-round seconds were not
  clean, TODO.md's sparse-transform item).

## Open questions

1. **What Stockfish does exactly. Answered 2026-10-15 (master of Stockfish and nnue-pytorch).**
   Each perspective's accumulator (1024) is split into halves, not doubled, and the halves are
   multiplied element-wise with no weights. Integer: each half clamped to `0..=255` (1.0 = 256),
   product divided by 512 (a shift by 9), giving `0..=127` where the next layer's 1.0 is 128:
   `256 * 256 / 512 / 128 = 1`, exact because every scale is a power of two. Float: each half
   clamped to `[0, 255/256]`, the product taken as is, no correction factor; the 512 exists in the
   engine only. Stockfish needs powers of two because its path is integer to the end; Rotor's ends
   in an f32 division, so any constant folds into it (question 5).
2. **Split or double. Decided 2026-10-14 (the developer): split, for now.** 256 into 128 + 128:
   the same update cost as amanda, so the comparison isolates the product. Doubling (two 256
   accumulators, update twice the cost) would mix products with width. The split can still
   represent linear units (a half saturated at 1, and each piece's self-pair), so a worse loss
   would mean 128 units are too few; doubling, or 256 linear values beside the products, would then
   be the next experiment.
3. **Which generation, which corpus. Decided 2026-10-14 (the developer): at least generation 3.**
   Its corpus is in hand and amanda gen-3 is the baseline on the same data and teacher, and the
   answer comes before generation 4's labelling ends. Its dataset must therefore be kept until this
   run has trained, whatever generation 4's cleanup deletes. Generation 4 may train it too.
4. **The architecture's name. Decided 2026-10-15 (the developer): `psq_pairwise`**, naming the
   operation (supersedes `psq_dot`, 2026-10-14). The configuration is greta. If greta defeats
   amanda, `psq_pairwise` becomes the new `psq`.
5. **The product's scaling in the integer engine. Decided 2026-10-15 (the developer):** the halves
   mirror Stockfish's `0..=255` and `>> 9` (one bit more per half than today's `0..=127`, one bit
   less accumulator headroom), but with 1.0 at 255 rather than Stockfish's 256, so the float
   trainer keeps clamping to `[0, 1]`; the leftover constant folds into the f32 rescale
   (implementation step 4). The head's input stays `0..=127`, within the `maddubs` bound. Training
   stays f32 with no fake quantization: the integer range is a load-time choice, so it can change
   without retraining. Measure the error in cp with `nnue_bench --quantization` against caroline's
   and betty's 1 to 2 cp. Not pursued: power-of-two scales, `i8` or
   mixed-precision training (the round is a memory-bound sparse gather, not dense arithmetic),
   squared clipped ReLU (it acts on hidden layers; greta has none).
