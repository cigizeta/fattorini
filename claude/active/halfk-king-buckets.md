# Fewer king buckets for halfk

**Status:** design, 2026-11-18, settled; the plan next, no code. The developer's layouts: 8 buckets
first, then 10 as a second experiment (the developer, 2026-11-18).

## Why

The census (king-buckets-census.md) put about 61% of positions' kings on their first rank, b1 and d1
(g1 and e1 mirrored) 28% and 23%, and the eighth rank at about 0.7%: today's 32 buckets, one a square
of the mirrored half-board, give the rarest (a8) about 1.6 million positions a pass over the 1.2
billion, about 210 times fewer than b1. Endgames, where a walking king lands in those buckets, are
where evaluation decides games. Merging the far squares shares their rows' data: the rarest of the
layouts below gets about 33 million positions a pass, 20 times more, on the same corpus.

Peers (2026-11-18, the search): Stockfish keeps 32 mirrored, Berserk, Integral 16, Renegade 14,
Leorik 5, all fine at home and coarse far away. Authors report the count barely matters for
strength (32 against 64 "not even 1 Elo"); grouping into real buckets is proposed, untested, as a
way to smaller networks; one worry, untested, is endgames in coarse buckets. Leorik's 5 split the
wings (a, b) from the center (c to f) on rank 1, as the 8 below do.

## The layouts, on the mirrored half-board (files a to d)

**8 (the developer's), first:**

```
8   7 7 8 8
7   7 7 8 8
6   7 7 8 8
5   7 7 8 8
4   5 5 6 6
3   5 5 6 6
2   3 3 4 4
1   1 1 2 2
    a b c d
```

Shares (the side to move's, the census): 1 (a1, b1) 32.0%, 2 (c1, d1) 28.6%, 3 (a2, b2) 10.7%,
4 (c2, d2) 8.3%, 5 (a3 to b4) 5.6%, 6 (c3 to d4) 8.1%, 7 (a5 to b8) 2.8%, 8 (c5 to d8) 3.8%.

**10, second:** the 8 with rank 1 split into its four squares: a1 (the king in the corner, h1) 4.0%,
b1 (castled short, g1) 28.0%, c1 (castled long, and f1) 5.0%, d1 (not castled yet, e1) 23.6%. What it
measures: whether the castled and uncastled kings, merged in the 8 (and in Leorik's 5), are worth
rows of their own.

## What changes

- **The rows:** a bucket keeps its 704 rows (11 identities x 64 squares); the 4,096 threat rows stay
  shared outside the buckets; the factor's 704 stay. With 8 buckets, `8 x 704 + 4,096 = 9,728` rows a
  perspective, against 26,624; with 10, 11,136. The transform is about a third of today's at the
  same width, so a wider one costs what today's does.
- **The layout as a table:** `king_bucket` reads a 32-entry table, the mirrored square to its bucket,
  instead of `rank * 4 + file`; today's 32 is the identity table. Every count derived from
  `BUCKETS` (`HALFK_ROWS`, `THREAT_ROW`, `ROWS`, `TRAINING_ROWS`, the state rows' bases) follows the
  layout: today constants, about 94 uses in 10 files of the engine and the trainer.
- **Which layout a network has:** to settle. Either an architecture field (`king_buckets: 32 | 8 |
  10`, a layout by name, 32 the default) or kinds of their own. A field keeps the halfk kinds as they
  are and makes the layout one change against the same kind, the halfk line's rule; the `.nnue`
  header records it, so the engine loads any layout and refuses one it does not know.
- **The engine's refresh:** today every king move changes the bucket, so it refreshes the king's
  perspective (through the refresh cache, the "Finny tables"). With large buckets a king move inside
  its bucket, its mirror unchanged, is an ordinary incremental update: the king's own row out and in.
  A refresh only on crossing into another bucket or the mirror: fewer refreshes in endgames, where
  the king walks. The refresh cache keeps one entry a bucket and mirror, so it shrinks with them.
  The exactness check against a from-scratch evaluation covers the new case.
- **The trainer:** batches and the transform's shape follow the layout's row counts; the factor rows,
  shared across buckets, unchanged.
- **The census** reports the shares of any layout, so each candidate layout's buckets are read
  before training.

## The experiments

1. **8 against today's 32,** the same network otherwise (screlu-hidden, 256, the same corpus), both
   under the budget, so the layout is the one difference: a bench for speed, test losses, then depth
   9 and 10+0.1 as every step. After the budget's validation run, which gives screlu-hidden's budget
   baseline.
2. **10 against 8,** the same way.

## Settled

- **The layout's selection: an architecture field** (the developer, 2026-11-18), `king_buckets: 32 |
  8 | 10`, 32 the default; the halfk kinds stay as they are.
- Today's networks' files do not change: a field defaulting to 32 keeps every existing `.nnue`
  loading unchanged, the header gaining the layout only for the new ones.
- The table lives in `features::halfk`, shared by the engine and the trainer, as the formula is.
