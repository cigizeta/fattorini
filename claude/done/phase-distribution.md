# Phase distribution of a corpus: `smithy phases`

**Status:** done 2026-10-15: applied, run on another machine over amanda-0002's games.

## Result (amanda-0002, 2026-10-15)

717 segments, 564,253 games, 73,613,518 positions after the book.

- No phase starved: the rarest is 23, 871,430 rows (1.18%), 1,698 a parameter of a 513-parameter
  head; the commonest 4, 7.9M (10.7%).
- Odd phases are rarer than even ones throughout (23: 1.2%, 22: 7.1%): an odd phase needs unequal
  minor pieces, which self-play passes through quickly.
- Endgames dominate: phases 0 to 7 hold 42.8% of the rows.
- Past the cap: 480 rows, 0.001%. The cap does not matter.
- Queens: impossible for both below 8, nearly always both from 18 up; between 8 and 16 one phase
  holds queenless and queened positions alike (12: 1,446,994 with none, 1,143,556 with both).

Decision: 25 buckets, one per phase value; a queen split is a later refinement (TODO.md). For TODO.md's "Attention in NNUEs", direction
4 (output buckets by context): before choosing how many buckets, see how the training positions
spread over Fruit's game phase.

## Question

Output buckets by game phase (knight and bishop 1, rook 2, queen 4, capped at 24, as
`rotor::pesto::PHASE_INC` and `PHASE_MAX`) are proposed at one bucket per phase value, 25 heads.
amanda's head is 513 parameters, so even a phase value holding 1% of a generation's rows would give
its head millions. This checks that no value is starved before anything is built, and whether
Stockfish's queen axis would split the data differently from the phase.

Corpus: the games amanda-0002 played, the source of the dataset greta and amanda-0003 train on.

## From the games, not the dataset

The datasets live only on the training machine (`smithy prepare` builds them there; Drive holds games and
networks, not datasets), and the training machine is busy. The games are on Drive, the upstream source, readable
from another machine or the Lichess machine. Reading them through the same code `prepare` uses gives the same rows:

- `remote::list(&self_play_dir("amanda/gen-0002"), ...)` for the finished segments
  (`*.pgn.zst`), as `prepare::self_play_segments` lists them, read in place on the remote
  (`remote::remote_path`): Drive's stream mode fetches each as it is read, nothing is copied into
  `data/`.
- `length_cache::SelfPlaySegmentReader` per segment, game by game, and
  `length_cache::self_play_game_positions` per game: every position after the book, through the final
  one, exactly the rows a dataset holds.

Two differences from the dataset, both harmless here: no train/test split (the counts are of every
row), and every finished segment, where `prepare` may stop at a cut of the corpus's size
(`prepare::self_play_segments_for`); the segments past the cut are more games of the same player.

## Command

    smithy phases amanda-0002

The teacher, as `smithy teacher` names it (`generation::Teacher::parse`: `hand-crafted` or
`<network>-NNNN`), its games found through `Teacher::source` and `self_play_dir`. No config and no
lambda: only positions are read. Needs Drive mounted (`remote::check_mounted`), and no tch at run time (smithy still links it).

## What it computes

Per position, from `CanonicalPosition::pieces` through `features::unpack_canonical_piece`:

- **Phase:** the sum of `PHASE_INC` over the pieces, uncapped, then capped at `PHASE_MAX`. Calling
  `pesto`'s constants, not copying them, so the buckets the network would use and the phase measured
  here cannot drift apart.
- **Queens, relative to the side to move** (the head sees the position from the side to move's
  view): none, side to move only, opponent only, both.

## What it prints

Segments and games read, then:

1. Rows per capped phase 0 to 24: count, percent, cumulative percent, and rows per head parameter at
   amanda's 513 (how much data each head would have).
2. Rows whose uncapped phase exceeded 24 (promotions): count and percent, to see whether the cap
   matters.
3. Phase by queen state, 25 x 4 counts: whether the queen axis cuts across the phase or mostly
   repeats it (a queen alone is 4 of the 24).

Plain text columns, as `nnue_bench --quantization` prints. A progress line per segment, since a
generation is millions of rows through a network drive.

## Code

- `crates/smithy/src/phases.rs`: `phase_of(&CanonicalPosition) -> (u32, u32)` (capped, uncapped),
  `queen_state(&CanonicalPosition) -> QueenState`, a `Histogram` with `add(&CanonicalPosition)` and
  its printing, and the pass over a source's segments.
- `crates/smithy/src/main.rs`: the `phases` subcommand, beside `check`, `calibrate` and
  `correlate`.

## Tests

- `phase_of` on positions of known phase: the start (24, 24), kings and pawns (0), a lone queen
  each (8), a third queen after a promotion (uncapped 28, capped 24), one minor each (2).
- `queen_state` for each of the four states, with Black to move as well as White, so the side to
  move is honoured.
- The histogram over a small fixture segment (a `.pgn.zst` of two games, written as
  `length_cache.rs`'s tests write theirs) equals the hand-counted one, book plies excluded.

## Measurement

`smithy phases amanda-0002` on another machine or the Lichess machine. Then decide in TODO.md: 25 buckets as proposed,
or merge only the phase values the histogram shows starved into a neighbour.
