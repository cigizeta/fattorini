# Kalo Chess 0.1.0

Kalo Chess is a UCI chess engine written in Rust, with a neural network evaluation (NNUE) trained
on a limited compute budget. This release plays with the network `NETWORK@tuned`, embedded in the
executable: there is no network file to set.

## Requirements

- Windows, 64-bit.
- A CPU with x86-64-v3 (AVX2, BMI2, FMA): most desktop CPUs since 2013 (Intel Haswell, AMD
  Excavator and Zen). On an older CPU the engine says so and exits.
- Nothing to install: one `.exe`, which writes no file.

## Use

Add `kalo-chess-v0.1.0.exe` to any UCI GUI (Cute Chess, Arena, BanksiaGUI, Nibbler, ...). Options:

| Option | Default | |
|---|---|---|
| `Hash` | 16 | transposition table, MB, 1 to 1024 |
| `Threads` | 1 | search threads, 1 to 128 |
| `SyzygyPath` | empty | Syzygy tablebase directories, WDL and DTZ |
| `MultiPV` | 1 | lines reported, 1 to 500 |
| `Ponder` | false | thinking on the opponent's time |

`kalo-chess about` prints what the executable is: its version, network and build.

## Strength

No rating is claimed. Two matches against Stockfish at fixed Skill Levels bracket the strength:
the highest level this release beats and the lowest that beats it. 400 games each, all played, in
colour-reversed pairs from the UHO book, at 10+0.1, one thread and 64 MB each, both sides with
6-piece Syzygy tablebases.

| Opponent | Games | Wins - Losses - Draws | Score |
|---|---|---|---|
| Stockfish VERSION, Skill Level N | 400 | W - L - D | S% |
| Stockfish VERSION, Skill Level N+1 | 400 | W - L - D | S% |

## License

Kalo Chess is free software under the GNU Affero General Public License, version 3 only. The source
of this release is the repository at its tag, `v0.1.0`. `kalo-chess licenses` prints the notices of
the third-party code built into the executable.

The network was trained on positions from the self-play games of
[Leela Chess Zero](https://lczero.org), test91 run's training data (storage.lczero.org), made
available under the [Open Database License (ODbL) 1.0](https://opendatacommons.org/licenses/odbl/1-0/),
its contents under the [Database Contents License (DbCL) 1.0](https://opendatacommons.org/licenses/dbcl/1-0/).
The conversion of that data into Kalo Chess's training positions is in `crates/smithy/src/lc0/`.
