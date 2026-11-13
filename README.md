# Fattorini

Fattorini is a UCI chess engine written in Rust. Its evaluation networks are trained on a limited compute
budget. A release is one `.exe` for Windows, its network embedded: about 3250 on CCRL Blitz's
scale, by Fattorini's own estimate (below).

To build Fattorini or train its networks, see [DEV.md](DEV.md).

Fattorini's account on Lichess is [fattorini-bot](https://lichess.org/@/fattorini-bot).

## Strength

An estimate, until CCRL rates Fattorini: **about 3250 on CCRL Blitz, one CPU**, for the network of
the first release, from three full matches against engines on CCRL Blitz (list of 2026-11-07, single
CPU), their official Windows builds.

| Anchor | CCRL Blitz | Games | Wins - Losses - Draws | Score | Elo vs anchor | Estimate |
|---|---|---|---|---|---|---|
| Weiss 1.3 (popcnt) | 3137 ± 15 | 400 | 213 - 102 - 85 | 63.9% | +99 ± 31 | 3236 ± 34 |
| Halogen 10 (x64-popcnt-avx2) | 3210 ± 10 | 400 | 154 - 118 - 128 | 54.5% | +31 ± 28 | 3241 ± 30 |
| Velvet 3.2.0 (x86_64-avx2) | 3303 ± 16 | 400 | 121 - 153 - 126 | 46.0% | -28 ± 28 | 3275 ± 32 |

How it was measured:

- **Time control:** CCRL Blitz is 2'+1" on an Intel i7-4770K, and its testers scale it to their
  machine by Stockfish 10's `bench`, 2,054 ms on that reference. Here it took 1,052 ms (the official
  `sf_10` popcnt build, five runs on an idle machine, the bench run alone), so the clock was halved
  to 1'+0.5".
- **Conditions:** 128 MB hash and one thread each, pondering off, 6-piece Syzygy tablebases for
  both sides, openings from the UHO book in colour-reversed pairs, every game played to its end,
  10 games at once.
- **The estimate:** each match's score turned into an Elo difference and added to the anchor's
  rating; the three combined weighted by the inverse of their variance, 3251 ± 18. Margins are 95%
  and count each anchor's own CCRL error. The three agree within 40 Elo; the combined margin
  leaves out what the matches share (the machine, the book, the load), so the real uncertainty is
  larger.

## Running Fattorini

Download the latest `.exe` from this repository's Releases and add it to any UCI GUI, such as
Cute Chess, Arena or BanksiaGUI. The network is inside the `.exe`: there is nothing else to
install or set.

Requirements:

- Windows, 64-bit.
- A CPU of the x86-64-v3 level, which Fattorini is compiled for (`-C target-cpu=x86-64-v3`: AVX,
  AVX2, BMI1, BMI2, F16C, FMA, LZCNT, MOVBE): Intel Haswell (2013) or later, AMD Excavator (2015) or
  later. AMD CPUs before Zen 3 run it slowly, since they execute BMI2's PEXT, which move generation
  relies on, in microcode. There is no build for older CPUs; on one, Fattorini says so and exits.

### UCI options

| Option | Default | Range | |
|---|---|---|---|
| `Hash` | 16 | 1 to 1024 | Transposition table size, in MB. |
| `Threads` | 1 | 1 to 128 | Search threads. |
| `MultiPV` | 1 | 1 to 500 | Lines to report. |
| `Ponder` | false | | Think on the opponent's time. |
| `SyzygyPath` | empty | | Syzygy tablebase folders. |
| `Move Overhead` | 100 | 0 to 5000 | Milliseconds kept back from each move for lag. |

## How Fattorini learns

Fattorini improves one generation at a time. The teacher, at first the PeSTO evaluation, then each
generation's network, plays games against itself, searching every position, and those searches
become the training labels. One network is trained on them, and it replaces the teacher only if it
beats it over the board.

Along the other route, the first generation trains on Leela Chess Zero's published training games,
from its test91 run, each position with the score of Leela's search; its winner must beat the PeSTO
evaluation over the board, and from there on it teaches the next generation as above.

### Lambda by progress

Each label blends two opinions about a position: `s`, the teacher's search score, and `z`, the game's
result, with a weight lambda on the result:

```text
label = (1 - lambda) * s + lambda * z
```

How much should the result count? It depends on how far through the game the position is, and on
how good the teacher is. So Fattorini does not pick a lambda: it measures one from the teacher's own
games, before training.

Think of `s` and `z` as two witnesses to the position's true value, each with its own errors, and
trust each in inverse proportion to how noisy it is. Neither error can be seen directly, but what
`s` and `z` have in common can only be the true value, so whatever `s` does not share with `z` is the
search's noise. That gives the best weight:

```text
lambda* = (Var(s) - Cov(s, z)) / (Var(s) + Var(z) - 2 * Cov(s, z))
```

It is measured in ten stages of the game, by progress from the first position to the last, and
each position takes its lambda from a line through the ten, never below 0 or above 1.

The shape belongs to the teacher:

- **A weak teacher,** such as PeSTO, stays about equally wrong all game, while the result gets more
  certain as the game ends. Lambda rises from 0.11 to 0.97: early the search is trusted, late the
  result.
- **A strong teacher,** such as Leela Chess Zero, is already accurate. Lambda stays near 0.03, and
  from mid-game it is 0: the formula finds no noise left to correct, the scores being right about who
  is winning and only a little timid about how much.

So as Fattorini's own networks improve, their lambda is measured afresh each generation and should move
from the first shape toward the second, with no schedule to design.

## Inside

- **Search:** alpha-beta with iterative deepening, aspiration windows, a transposition table, null
  move pruning, late move reductions, futility pruning, razoring, SEE pruning, killer and history
  heuristics, singular extensions and quiescence search. Several threads share one table.
- **Evaluation:** an NNUE in integer arithmetic with AVX2, embedded in the `.exe`. The first
  release's network, `lc0-psq-threats-screlu` generation 1, has 4,876 inputs per side: 780
  piece-square ones (pieces on squares, castling rights, the en passant file) and 4,096 simplified
  threats, one for each square pair where a piece of the side attacks an opponent's (no pins, no
  x-rays). Both sides' inputs feed 256 neurons each, squared after clipping (SCReLU), straight to
  the output. It was trained on Leela Chess Zero's data (below), the likely main source of its
  strength. The piece-square part is updated incrementally; the threats are recomputed at each
  evaluation.
- **Search parameters:** the pruning and reduction margins tuned by SPSA with the network they ship
  with.
- **Endgames:** Syzygy tablebases, both WDL and DTZ.

## Acknowledgments

Some of Fattorini's networks, the first release's among them, are trained with the self-play games of [Leela Chess Zero](https://lczero.org),
from its test91 run's training data (storage.lczero.org), made available under the
[Open Database License (ODbL) 1.0](https://opendatacommons.org/licenses/odbl/1-0/), its contents
under the [Database Contents License (DbCL) 1.0](https://opendatacommons.org/licenses/dbcl/1-0/).
The conversion of that data into Fattorini's training positions is Fattorini's own, in `crates/smithy/src/lc0/`.

## License

Copyright (C) 2026 Calogero Zarba

This program is free software: you can redistribute it and/or modify it
under the terms of the GNU Affero General Public License as published by
the Free Software Foundation, version 3.

This program is distributed in the hope that it will be useful, but
WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY
or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public
License for more details: the full text is in `LICENSE`.
