# Psq stack: head layers bucketed by our king, their king and material

**Status:** design, drafted 2026-10-15 from a discussion the developer held in Claude.ai, then
reviewed here. An extension of direction 4 of TODO.md's "Attention in NNUEs beyond HalfK" (output
buckets by context) from one stage of the head to three. Nothing implemented. Comes after
pairwise-products.md and after the `psq_pairwise_24` against `psq_pairwise_78` comparison.

## Why

Psq networks are limited by capacity, not data (pairwise-products.md, Why). HalfK adds capacity by
conditioning the accumulator on the king square, but at about 41k inputs per perspective each king
square sees roughly 1/64 of the data, too little for Rotor's corpus (TODO.md, "halfkp and
halfka_v2_hm: parked"), and a king move forces an accumulator refresh.

The stack keeps the accumulator king-blind (psq, 780 inputs, incremental update unchanged) and puts
the king conditioning in the small dense layers after it, where it costs no refresh and no extra
arithmetic.

**King moves are accepted (the developer, 2026-10-15):** a cheaper answer to a cost HalfK has too.

- **HalfK on a king move:** the king square is part of every feature, so every feature changes and
  the accumulator is rebuilt, about 30 rows of 256 per side summed from scratch instead of the usual
  2 to 4. Stockfish softens it with a cache of accumulators per king square (Finny tables), at a cost
  in memory and code.
- **The stack on a king move:** the accumulator does not change, being king-blind. The next
  evaluation reads a different 8 KB L1 set, probably from L2 cache, a few nanoseconds later. No
  arithmetic added, no code.

What the stack gives up is knowledge: HalfK conditions every piece's value on the king square inside
the accumulator (about 41k features per side), the stack only the small head (a few hundred weight
sets). The bet: far more data per bucket, fitting Rotor's corpus, for a coarser king awareness.

## The shape

    (128 + 128) x 2 -> 256 -> 16 -> 16 -> 1

    accumulator: psq_pairwise, king-blind, unchanged: per side 256, split in halves of 128 + 128
      -> products: per side 128, the two sides concatenated to 256, no weights
      -> L1:  256 -> 16   one weight set per square of OUR king, 64, no mirroring
      -> clamp
      -> L2:  16 -> 16    one weight set per square of THEIR king
      -> clamp
      -> out: 16 -> 1     one weight set per material value, 1 to 78

**Shape decided 2026-10-15 (the developer).** The accumulator and the products are greta's, so the
stack is the only difference from her. Each stage shrinks the vector: the first layer's width sets
most of the cost.

- This is Stockfish's layer stacks generalised. Stockfish makes one choice per evaluation: its piece
  count, `(pieces - 1) / 4`, picks one of 8 complete heads, every layer from the same bucket. The
  stack makes one choice per layer, each by its own key, so 64 + 64 + 78 stored sets compose into
  every combination of king squares and material.
- Each evaluation picks exactly one set per stage. The sets add up rather than multiply: 64 + 64 +
  78 sets, not their product.
- L1 by our king leans toward defence and king safety, L2 by theirs toward attack. Since L2 reads
  L1's output after a nonlinearity, its inputs are already shaped by our king, so the two kings
  interact a little without 64 x 64 buckets.
- Material: pawn 1, knight 3, bishop 3, rook 5, queen 9, both sides, 78 at the start, capped at 78
  for promotions. 78 buckets, 1 to 78: material 0 is K vs K, which the search scores as a draw
  before evaluating (TODO.md, direction 4). Which material scale, or the phase (25 buckets), is
  settled first by `psq_pairwise_24` against `psq_pairwise_78`; the stack uses the winner.
- Limits: the buckets know where the kings are, not what threatens them (threat or king-zone inputs,
  direction 3, could add that). Material cannot tell imbalances apart (Q against R+R).

## Factorisation: required

The buckets get very uneven data: king squares g1 and e1 are common, d5 and h4 rare, and extreme
material values are rare too. A rare bucket with independent weights learns noise.

So every bucketed weight set is trained as

    W_bucket = W_shared + Delta_bucket

- `W_shared` gets gradient from every position, `Delta_bucket` only from its own bucket.
- `Delta` starts at 0, optionally with a small L2 penalty, so a rare bucket stays close to the
  shared weights.
- At export, `W_shared + Delta_bucket` is written per bucket: the `.nnue` and the engine never see
  the split, and it costs nothing at inference.
- Applied to all three stages. This is Stockfish's feature factorisation (TODO.md, direction 2,
  "Kept from it").
- **Always on, no flag to disable it (decided 2026-10-15, the developer).** It touches only training
  and export; the engine is unchanged.

An alternative for the material stage: about 13 coarse buckets, interpolated by material as a
tapered evaluation interpolates mg and eg.

## Cost

**Arithmetic: the buckets add none.** Each evaluation uses one set per stage, the same
multiplications as an unbucketed head of the same widths:

| Stage | Multiply-adds | One set (`i16`) | All sets |
|---|---|---|---|
| products | 256 multiplies | | |
| L1, 256 -> 16 | 4,096 | 8 KB | 64 x 8 KB = 512 KB |
| L2, 16 -> 16 | 256 | 512 bytes | 64 x 512 bytes = 32 KB |
| out, 16 -> 1 | 16 | 32 bytes | 78 x 32 bytes = 2.5 KB |

About 4,400 multiply-adds against greta's 256: the hidden layers, not the buckets, are what costs.
L1 is nearly all of it. To weigh against the accumulator update, about 100 ns of today's evaluation
(nnue-inference-speed.md).

**Memory: all sets stored, one read per evaluation.** About 550 KB in all. A core of the Lichess machine has 48 KB
of L1 cache and 1 MB of L2: one set per stage fits in L1, all of them fit in L2.

**The only overhead is cache misses on a king move.** The set in use stays hot because kings rarely
move; after a king move the next evaluations fetch the new set from L2. The hardware handles this
with no code: the engine only indexes `weights[bucket]`. Each set must be contiguous and start on a
64-byte boundary, as the current weights do. An explicit prefetch on king moves, like the TT
prefetch, only if the bench shows a cost.

Expected: the bucket misses a few percent of nodes/s at most; the hidden layers' 4,400
multiply-adds more, comparable to betty's head (128 ns against amanda's 40, betty-head-speed.md).
Estimates, to measure.

## The experiments

The buckets are nearly free at inference, so test loss can rank them against an unbucketed head of
the same widths. The hidden layers are not free, so the stack as a whole against greta is judged by
games.

1. **Baseline:** the winner of `psq_pairwise_24` against `psq_pairwise_78`, factorised.
2. **Plus L1 by our king,** 256 -> 16 (the hidden layer's worth is settled by generation 3's duels
   first).
3. **Plus L2 by their king,** 16 -> 16. The least proven stage.
4. **Optionally:** 78 factorised material buckets against about 13 interpolated ones.

The deeper head needs more data to pay off, so compare networks trained on the full corpus only.

## Implementation sketch

1. **`net_config.rs`:** a new kind `psq_stack`, with the widths 16 and 16; keys and bucket counts
   fixed (64/64/78).
2. **Trainer:** the bucket indices computed per position beside the features; each stage gathers its
   set by index (shared plus delta) before the batched matrix multiply. Stacked lockstep arms
   (stacked-lockstep-arms.md) must carry the bucket dimension.
3. **Export:** `W_shared + Delta` folded per bucket; the header records the stages and bucket keys.
4. **Engine:** bucket indices from the king squares and the material count, both kept incrementally
   on the board; `weights[bucket]` per stage, quantized as today's head.
5. **Tests:** a factorised network with every delta 0 equals the unbucketed one; the exported per
   bucket weights equal shared plus delta; quantized within tolerance of f32; node counts of existing
   networks unchanged.

## Measurement

- Test loss of each step against the previous one, at every lambda, same test set.
- `nnue_bench` on idle the Lichess machine, random network of the new shape: the head's time and nodes/s against
  the baseline.
- Train seconds per round on the training machine, alone.

## Open questions

1. **Mirroring. Decided 2026-10-15 (the developer): 64 L1 sets, no mirroring, so 64/64/78.** The
   Claude.ai design used 32 L1 sets, horizontally mirrored, and L2 "in the mirrored frame", as HalfK
   engines mirror the whole board. With a king-blind accumulator nothing else is mirrored: a king
   on h1 using the a1 set would read products of an unflipped board, so one set would serve two
   different situations. Mirroring the products too would bring back the refresh the stack avoids.
   64 sets cost 512 KB, still within L2, and each sees half the positions, which factorisation
   covers. Also easier to understand.
2. **The name of the kind. Decided 2026-10-15 (the developer): `psq_stack`**, as Stockfish's
   "layer stacks". The configuration is hilda.
3. **The delta penalty. Decided 2026-10-15 (the developer): none.** The deltas start at 0, so rare
   buckets already stay near the shared weights; AdamW's weight decay applies as to every weight. A
   penalty only if rare buckets look noisy.
4. **The order of the keys. Decided 2026-10-15 (the developer): fixed, our king, their king,
   material**, not configurable. Another order is new code.
5. **The output stage's key. Decided 2026-10-15 (the developer): material only, 78 buckets.** If
   `psq_pairwise_24` (phase) beats `_78`, phase is added then.
