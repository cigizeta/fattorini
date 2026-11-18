# King buckets: a census of the test set and the corpus

**Status:** agreed with the developer 2026-11-18; implemented the same day (`census.rs`,
`freeze::read_positions`), to test and run. The corpus is read with the freezer's own reader
(`SelfPlaySegmentReader`, `self_play_game_kept`), the test set's rule exactly; the sample's size an
argument, `--sample <positions>`, the test set's by default (the developer, 2026-11-18).

## Result (2026-11-18, run by the developer, the default sample)

The test set, 4,194,304 positions, against 4,194,069 sampled of the corpus's 1,189,195,370.

- **Where the kings stand:** about 61% of positions have the king on its first rank, b1 (g1,
  castled) 28%, d1 (e1, not castled yet) 23%; the eighth rank about 0.7%; the rarest bucket a8 (h8),
  0.133% for the side to move, 0.135% for the opponent, about 210 times rarer than b1.
- **The test set is sound:** every bucket within a few hundredths of a point of the corpus. The
  largest gap, b1, 27.97% against 28.23% (-8.2 and -8.7 "standard errors"), is overstated: the
  errors assume independent positions, while a game's dozens of positions often share a king square
  (the test set holds 65,536 games, about 64 positions each), so the true errors are several times
  larger. What remains is a mild shift toward uncastled, central and advanced kings, likely because
  the test set is January's tars whole while the corpus spans later months of a run whose network
  kept changing. Not a sampling fault; drawing the test set across the period would remove it.
- **The rare buckets are thin, not starved:** a8 has about 1.6 million positions a pass over today's
  corpus, about 5.7 million over 4 x 1024^3; each position touches a few dozen of a bucket's 832
  rows, so tens of thousands of updates a row a pass today. With the 256 run's train-test gap at
  zero after 4.5 passes, 4 x 1024^3 for width 512 stands as a reasonable next corpus.

Done; to `done/`.

## Why

How much data a halfk network needs (the halfk line, its data question): the network has 32 king
buckets a perspective, each with 832 rows of its own, and a rare bucket's rows are trained only by
the positions that put a king there. The average test loss cannot show those rows undertrained: rare
buckets are as rare in the test set. Two questions, answered by counting where the kings stand, never
by labels or losses:

1. **Is the frozen test set sound?** Its kings' distribution should match the training corpus's, up
   to noise. A clear difference would mean the reservoir or the kept-position filter went wrong.
2. **How thin are the rare buckets?** A bucket's share times the corpus gives its rows' positions a
   pass, the figure to weigh 4 x 1024^3 positions against 1.2 billion.

The developer's rule that Claude never looks at the test set is about choosing by its losses; this
counts kings only, to check the set itself (the developer, 2026-11-18).

## The buckets

`features::halfk::king_bucket`: a king oriented for its perspective (`features::oriented_square`,
black's flipped), files e to h mirrored onto d to a, its bucket `rank * 4 + file`, 0 to 31. Each
position has two: the side to move's king's and the opponent's, each the bucket of its own
perspective's accumulator.

## The command

`bin/smithy king-buckets`, read-only, CPU only, in no usage line beside the jobs' (as `lc0`'s children
are not): a one-off check, run by hand. Safe beside Lichess, a few minutes.

1. **The test set:** Lc0's frozen test set, its `test.positions` read through `cache::read_position`
   (`freeze.rs`'s reading, its labels left unread), every position counted.
2. **The corpus:** as many positions as the test set holds, from the training segments spread evenly
   over the corpus (every k-th segment, so January's tars do not stand for all of it), through the
   same kept-position filter the test set was drawn under (`game_labels::KEPT`), K vs K left out as
   there. Which reader does this with least new code, the scatter's or the training stream's, is
   settled on reading them.
3. **Printed, for each of the two sets and each perspective (side to move, opponent):** a board of 8
   ranks by 4 files, each bucket's share in percent, rank 8 on top as a diagram reads; then the
   rarest bucket, its count and share; then, between the sets, each bucket's difference in shares and
   the largest one, in standard errors, so "matches up to noise" is a number.
4. **Also printed:** each bucket's positions a pass over 1.2 billion and over 4 x 1024^3, at its
   corpus share.

## Steps

1. The command and its counting, with a test on a few hand-made positions (a king on each mirrored
   half lands in one bucket; black's oriented).
2. The developer runs it:

```bash
cargo test -p smithy
```

```bash
bin/smithy king-buckets
```

3. The result here, and the data question's next step decided from it.
