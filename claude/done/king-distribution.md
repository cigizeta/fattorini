# King distribution of a corpus: `smithy phases` counts the kings too

**Status:** done 2026-10-15: implemented, tests passing, run on the Lichess machine over amanda-0002's games.
`smithy phases` renamed `smithy buckets` the same day (the developer), since it now counts every
bucket key, not only the phase; the module is `buckets.rs`.

## Result (amanda-0002, 2026-10-15, 73,613,518 positions)

- **Our king:** g1 21.95%, e1 11.81%; no square under 0.1%, the rarest h8 with 81,947 rows, 19 a
  parameter of a 4,112-parameter L1 set; about 26 squares under 100 a parameter (rank 8, the far
  ranks and the a- and h-files). Factorisation carries these squares: their deltas stay near 0.
- **Their king:** the mirror image (g8 21.91%, e8 11.18%); the rarest, h1, still 305 rows a parameter
  of a 272-parameter L2 set: ample everywhere.
- Decision: 64/64 stands (psq-stack.md). A coarser our-king key for L1 only if rare squares prove
  noisy.
- Found on the way: 358,646 rows at material 0 (king against king), self-play playing on in dead
  positions; fixed by insufficient-material-ends-the-game.md. For psq-stack.md: before building 64 L1 sets by
our king and 64 L2 sets by their king, see how the training positions spread over the two kings'
squares, as phase-distribution.md did for the phase and the material.

## Question

The stack's L1 picks one of 64 weight sets by our king's square, L2 one of 64 by theirs. Kings sit
on g1, e1 and their neighbours most of the game and rarely on d5 or h4. How rare is rare: does any
square get too few rows for its set, even with factorisation falling back to the shared weights?
And how much more data does each L1 set get than each L2 set needs (an L1 set is 256 x 16 + 16 =
4,112 parameters, an L2 set 16 x 16 + 16 = 272)?

## Same command, one more count

Added to `smithy phases`, not a new subcommand: it already reads every training position of a
teacher's games once, through the code `prepare` uses (phase-distribution.md, "From the games, not
the dataset"), and that pass over Drive is the slow part. One pass, three distributions.

    smithy phases amanda-0002

## Orientation

As the stack would see the squares, both in the side to move's frame:

- **Our king:** the side to move's king, `features::oriented_square` for the side to move (rank
  flipped when Black is to move, so our back rank is always rank 1), as psq's own perspective
  orients its pieces. No horizontal mirroring (psq-stack.md, open question 1: 64 sets).
- **Their king:** the other king, oriented for the side to move too, so its home rank is rank 8.
  The L2 set reads products whose first half is the side to move's view, so "where is their king as
  I see it" is the natural key.

`oriented_square` is private to `features.rs` today; it becomes `pub`, so the bench and the future
trainer orient alike and cannot drift apart.

## What it prints

After the phase and material tables, for each king:

1. **An 8 x 8 board of percentages**, rank 8 at the top, files a to h, so the shape is seen at a
   glance.
2. **Rows per square,** sorted from rarest to commonest: square, rows, percent, rows per parameter
   of its set (4,112 for our king, 272 for theirs).
3. **The rarest and commonest squares,** and how many squares hold under 0.1% of the rows.

Plain text columns, as the phase tables.

## Code

- `crates/rotor/src/features.rs`: `oriented_square` made `pub`.
- `crates/smithy/src/phases.rs`: a `KingDistribution` (counts `[u64; 64]`, its parameter count, a
  name) with `add(Square)` and its printing; `Histogram` gains `our_king` and `their_king`, filled in
  `add` from `side_to_move`, `white_king` and `black_king`. The module doc comment says it counts
  the kings too.

## Tests

- Our king and their king on known positions, White and Black to move: the start, White to move
  (our e1, their e8), and Black to move (our e1 after flipping, their e8).
- A king off its home rank, Black to move: Black's king on g6 is our g3; White's on c2 is their c7.
- The fixture segment of the existing test: every position's kings on e1 and e8 (no king moved).

## Measurement

`smithy phases amanda-0002` on another machine or the Lichess machine. Then, in psq-stack.md: keep 64/64, or note squares so
rare that factorisation alone carries them.
