# Fattorini 2026.11.0

Fattorini is a UCI chess engine written in Rust, with a neural network evaluation (NNUE) trained
on a limited compute budget. This release plays with the network `lc0-psq-threats-screlu-g1-r1@tuned`,
embedded in the executable: there is no network file to set. Its inputs are piece-square features
with simplified threat features, its activation SCReLU; it was trained on positions from Leela
Chess Zero's games (below), the likely main source of its strength, and its search parameters
tuned with it by SPSA.

## Requirements

- Windows, 64-bit.
- A CPU of the x86-64-v3 level, which Fattorini is compiled for (`-C target-cpu=x86-64-v3`: AVX,
  AVX2, BMI1, BMI2, F16C, FMA, LZCNT, MOVBE): Intel Haswell (2013) or later, AMD Excavator (2015) or
  later. AMD CPUs before Zen 3 run it slowly, since they execute BMI2's PEXT, which move generation
  relies on, in microcode. On an older CPU, the engine says so and exits.
- Nothing to install: one `.exe`, which writes no file.

## Use

Add `fattorini-v2026.11.0.exe` to any UCI GUI (Cute Chess, Arena, BanksiaGUI, Nibbler, ...). Options:

| Option | Default | |
|---|---|---|
| `Hash` | 16 | transposition table, MB, 1 to 1024 |
| `Threads` | 1 | search threads, 1 to 128 |
| `SyzygyPath` | empty | Syzygy tablebase directories, WDL and DTZ |
| `MultiPV` | 1 | lines reported, 1 to 500 |
| `Ponder` | false | thinking on the opponent's time |
| `Move Overhead` | 20 | ms kept back from each move for lag, 0 to 5000 |

`fattorini about` prints what the executable is: its version, network and build.

## Strength

An estimate, until CCRL rates it: **about 3250 on CCRL Blitz, one CPU**, from three full matches
against engines on CCRL Blitz (list of 2026-11-07, single CPU), their official Windows builds.

| Anchor | CCRL Blitz | Games | Wins - Losses - Draws | Score | Elo vs anchor | Estimate |
|---|---|---|---|---|---|---|
| Weiss 1.3 (popcnt) | 3137 ± 15 | 400 | 213 - 102 - 85 | 63.9% | +99 ± 31 | 3236 ± 34 |
| Halogen 10 (x64-popcnt-avx2) | 3210 ± 10 | 400 | 154 - 118 - 128 | 54.5% | +31 ± 28 | 3241 ± 30 |
| Velvet 3.2.0 (x86_64-avx2) | 3303 ± 16 | 400 | 121 - 153 - 126 | 46.0% | -28 ± 28 | 3275 ± 32 |

Conditions: CCRL Blitz's 2'+1" scaled to the machine by Stockfish 10's `bench` (CCRL's reference
figure, 2,054 ms; 1,052 ms here, the bench run alone), so 1'+0.5"; 128 MB hash, one thread, pondering
off, 6-piece Syzygy tablebases for both sides; openings from the UHO book in colour-reversed pairs;
every game played; 10 games at once. Each estimate is the anchor's rating plus the match's
difference; the three are combined weighted by the inverse of their variance, 3251 ± 18. Margins
are 95% and count the anchors' own CCRL errors as independent; the combined margin leaves out what
the three matches share (the machine, the book, the load), so the real uncertainty is larger.

## License

Fattorini is free software under the GNU Affero General Public License, version 3 only. The source
of this release is the repository at its tag, `v2026.11.0`. `fattorini licenses` prints the notices of
the third-party code built into the executable.

The network was trained on positions from the self-play games of
[Leela Chess Zero](https://lczero.org), test91 run's training data (storage.lczero.org), made
available under the [Open Database License (ODbL) 1.0](https://opendatacommons.org/licenses/odbl/1-0/),
its contents under the [Database Contents License (DbCL) 1.0](https://opendatacommons.org/licenses/dbcl/1-0/).
The conversion of that data into Fattorini's training positions is in `crates/smithy/src/lc0/`.
