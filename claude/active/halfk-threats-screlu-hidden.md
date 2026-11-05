# halfk_threats_screlu_hidden: design

Status: designed 2026-11-02, decisions by the developer one at a time; the implementation plan,
halfk-threats-screlu-hidden-implementation.md.
Step 2 of the halfk line (TODO.md): halfk_threats_screlu (halfk-threats-screlu.md) plus hidden
layers, duelled against step 1's winner, depth 9 then 10+0.1, both `@default`. Decided from scratch
(the developer: "without comparison from other architectures"), then checked against two engines'
sources, read 2026-11-02: Reckless's (`src/nnue.rs`, `src/nnue/forward/`) and Stockfish's
(`src/nnue/nnue_architecture.h`).

## The network

```
(26,624 -> 256) x 2, each lane clipped and squared: 512 values
512 -> 16, clipped to [0, 1]
 16 -> 32, clipped to [0, 1]
 32 -> 1
```

Step 1's transform and features unchanged (halfk, threats, SCReLU, the 32 king buckets factorised in
training). Only the head differs: step 1's 512 -> 1 becomes two hidden layers.

## Decisions (the developer, 2026-11-02)

1. **Two hidden layers, 512 -> A -> B -> 1**, not one. The first layer is the cost (512 x A
   multiply-adds an evaluation); a second, A x B, is about 6% of it, and depth combines the first
   layer's bends where width would cost 512 a unit.
2. **A = 16, B = 32.** A sets both the cost and the bottleneck, a multiple of 8 for the kernels; B
   is nearly free. Reckless's head is the same, 768 -> 16 -> 32 -> 1; Stockfish's is
   1024 -> 32 -> 32 -> 1.
3. **A plain clip to [0, 1] after both hidden layers**, Reckless's (Stockfish puts a clip and a
   squared clip side by side after both, its output reading both layers).
4. **No skip:** the output reads the second layer's 32 values alone, as Reckless's. Stockfish adds
   a raw difference of two first-layer outputs and reads the first layer's values too.
5. **Trained end to end.** With no skip a detached head would leave the transform no gradient, so
   the transform learns through the hidden layers, as both engines' do.
6. **From scratch:** every weight drawn as a fresh network's, the transform as step 1's (factor
   and threat rows drawn, piece rows zero), a full generation 1 as step 1's. Starting from step 1's
   transform would compare something else, and the trainer has no such start.
7. **Reckless's arithmetic, measured against int16:** the first layer in integers, the second layer
   and the output in f32, being cheap either way. Found in the code afterwards: the engine already
   has this head in int16 (a stack's, `stack_head_value`: the first layer's weights quantized per
   row, `QuantizedRows`, over the squares in `0..=127`), and the trainer's plain head with
   `hidden_dim: [16, 32]` is the same shape. Int8 was measured once, on betty (2026-10-07): 3.8 cp
   mean and 34 cp p99 away from its own f32 evaluation, against caroline's 1.2 and 7.8 in int16; one
   network, so a warning, not a verdict. Decided (the developer, 2026-11-02, "measure"): **both
   built, chosen on numbers**: Reckless's int8 (the squares as unsigned 8 bits, int8 weights, the
   nonzero inputs only) and the existing int16, compared by the bench's speed and by each one's gap
   to the f32 evaluation, on a random network before training and on the trained one after. The
   loser removed before the duels.

## For the implementation plan

- **Tag 21, `halfk_threats_screlu_hidden`**, `ArchitectureKind::HalfkThreatsScreluHidden`,
  `configs/lc0-halfk-threats-screlu-hidden.yaml` (step 1's with its kind and `hidden_dim: [16, 32]`),
  and its pesto twin if wanted, as step 1's.
- **The quantization of the squares:** each lane clipped at the transform's quantized one (as
  today's SCReLU), squared, and scaled down to 0..127 or 0..255 by a shift; the first layer's weight
  scale (Reckless: 64, so a weight within about +-2) and the dequantization into f32. To fix against
  a test of the integer head against the f32 reference.
- **Sparsity:** the nonzero 4-byte chunks of the 512 inputs found first, only their weights read
  (Reckless's `find_nnz`); worth it only if many squares are zero, to measure on real positions.
- **The trainer:** the head as a new shape beside today's (`hidden_dim: [16, 32]`, the clip
  `clamp(0, 1)`), the stacked network's arm, the export of 512 x 16, 16 x 32 and 32 x 1 with their
  biases, and the tch-against-engine agreement test through a file, as step 1's.
- **The bench before training**, as step 1's: `nnue_bench` with a random network of this shape
  against step 1's, then a search at depth 9; the developer decides whether to train.
- **Duels:** against step 1's winner, depth 9 then 10+0.1, both `@default`.
