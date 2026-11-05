# Temper Games

Temper Chess is a UCI chess engine written in Rust. Its evaluation networks are trained on one PC with one
consumer GPU. Its training is starting again from scratch, along two routes: networks that learn
only from Temper itself, the first generation from the PeSTO evaluation and every later one from the
previous winner; and networks whose first generation learns from the training games Leela Chess
Zero publishes. No other engine's network is used.

Support for Go, as `temper-go`, is planned but not yet started.

To build Temper or train its networks, see [DEV.md](DEV.md).

Temper Chess's account on Lichess is [temper-chess](https://lichess.org/@/temper-chess).

## Running Temper

There is no release yet, so there is no `temper-chess.exe` to download. Until the first one, Temper
has to be built from source, as [DEV.md](DEV.md) describes, and then added to any UCI GUI.

Requirements:

- Windows, 64-bit.
- An x86-64-v3 CPU (BMI2, AVX2, FMA): Intel Haswell (2013) or later, AMD Zen 3 or later. AMD Zen 1
  and Zen 2 run it, but slowly. There is no build for older CPUs; on one, Temper says so and exits.

### UCI options

| Option | Default | Range | |
|---|---|---|---|
| `Hash` | 16 | 1 to 1024 | Transposition table size, in MB. |
| `Threads` | 1 | 1 to 128 | Search threads. |
| `MultiPV` | 1 | 1 to 500 | Lines to report. |
| `Ponder` | false | | Think on the opponent's time. |
| `SyzygyPath` | empty | | Syzygy tablebase folders. |

## How Temper learns

Temper improves one generation at a time. The teacher, at first the PeSTO evaluation, then each
generation's winner, plays games against itself, searching every position, and those searches
become the training labels. Nine networks are trained on them, one per blend of label; they play a
round robin, and the winner replaces the teacher only if it beats it over the board.

Along the other route, the first generation trains on Leela Chess Zero's published training games,
from its test91 run, each position with the score of Leela's search; its winner must beat the PeSTO
evaluation over the board, and from there on it teaches the next generation as above.

Each label blends one position's search score with the game's result, as Leela's own training
does.

## Inside

- **Search:** alpha-beta with iterative deepening, aspiration windows, a transposition table, null
  move pruning, late move reductions, futility pruning, razoring, SEE pruning, killer and history
  heuristics, singular extensions and quiescence search. Several threads share one table.
- **Evaluation:** an NNUE, updated incrementally, in integer arithmetic with AVX2. A release ships
  the strongest of the architectures on trial (see [DEV.md](DEV.md)). The first networks of the new
  training have 780 inputs (pieces on squares, castling rights, the en passant file) seen from both
  sides, 256 neurons per side, straight to the output.
- **Endgames:** Syzygy tablebases, both WDL and DTZ.

## Acknowledgments

Some of Temper's networks are trained with the self-play games of [Leela Chess Zero](https://lczero.org),
from its test91 run's training data (storage.lczero.org), made available under the
[Open Database License (ODbL) 1.0](https://opendatacommons.org/licenses/odbl/1-0/), its contents
under the [Database Contents License (DbCL) 1.0](https://opendatacommons.org/licenses/dbcl/1-0/).
The conversion of that data into Temper's training positions is Temper's own, in `crates/smithy/src/lc0/`.

## License

Copyright (C) 2026 Calogero Zarba

This program is free software: you can redistribute it and/or modify it
under the terms of the GNU Affero General Public License as published by
the Free Software Foundation, version 3.

This program is distributed in the hope that it will be useful, but
WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY
or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public
License for more details: the full text is in `LICENSE`.
