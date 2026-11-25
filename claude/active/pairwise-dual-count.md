# Pairwise dual with piece-count buckets: design

Status: decided 2026-11-25 (the developer: "be optimistic that dual wins. Plan and implement"), so
written before the dual attempt has run, ready for the day it wins its duels. Its plan:
pairwise-dual-count-implementation.md. The step after pairwise-dual-activation.md, the
piece-count buckets of TODO.md (2026-11-23, "the mainstream way, on the best network").

## The network

halfk_threats_pairwise_dual's, its whole head in 8 copies, one picked per position by the number
of pieces on the board, kings included, `(count - 1) / 4` (`stack_buckets::count_bucket`, the
key the batches already carry, column 4):

```
(9,740 -> 512) x 2, each lane clipped; first 256 lanes times last 256: 512 products, shared
bucket b = (pieces - 1) / 4, 0 to 7
512 -> 16 (bucket b's)
  outputs 1 to 15: clipped, then as they are and squared: 30 values
     30 -> 32 (bucket b's), clipped
     32 -> 1 (bucket b's)
  output 16, raw, added
```

8 complete dual heads (the developer, 2026-11-25): each its own three layers; the transform and
the products shared. One head computed per evaluation, so the speed is the dual network's; the
memory 8 heads, about 75,000 weights against the transform's millions.

## Decisions (the developer, 2026-11-25)

1. **Independent buckets, the mainstream way** (Stockfish, Reckless, PlentyChess, Alexandria,
   Obsidian): each bucket's weights trained only by its positions, no shared part and no deltas,
   unlike Fattorini's stack and material heads. With about 5B positions no bucket lacks data.
2. **Every bucket starts as the same head:** one draw, copied to all 8, as nnue-pytorch starts its
   layer stacks; they then diverge with their own positions.
3. **The kind `halfk_threats_pairwise_dual_count`**, as `psq_threats_screlu_count` named its key,
   its config `lc0-halfk-threats-pairwise-dual-count-512-8-budget`, `unit: elo`, the dual
   attempt's in everything else.

## How it is judged

Against the dual network it extends, the one change: test loss round by round, then duels at
depth 9 and 10+0.1.
