# The release's robustness checks

**Status:** approved (the developer, 2026-11-11): cutechess-cli, not the arena nor fastchess.
Commands below; to run on the training machine, idle. The three checks TODO.md lists under "Before the
release or CCRL" that the arena cannot run: 8 threads, a repeating control, Move Overhead. Each
plays the release's own build, so each runs on a snapshot or the dry run's `.exe`, at the default
settings a tester would leave alone.

## Why not the arena

The arena plays Fischer clocks only (`--time <base+inc>`) and gives each side one thread; it refuses
`--threads`. Teaching it `movestogo` and per-side threads is real code, tests and a design, for
checks run once per release. A tool CCRL testers themselves use answers the same question with no
code, and plays the engine as they will.

## Proposed: cutechess-cli

- **The tool:** cutechess-cli, the Cute Chess installer's, on `PATH`. GPLv3, only run, never linked
  or shipped.
- **The engine:** the snapshot of the commit to release, `bin/arena snapshot
  lc0-psq-threats-screlu-g1-r1@tuned`, or the dry run's `target/release-build/fattorini-v0.0.0-dry-run.exe`.
  Both sides the same `.exe`.
- **Common to all:** `proto=uci`, Hash 128 (CCRL Blitz's), the 6-piece Syzygy tables, the UHO book
  in colour-reversed pairs (`-repeat`), `-recover` off so that a crash is seen, `-pgnout` to
  `data/checks/<check>/<date>/`, `-ratinginterval 10`. Every game played: no adjudication.
- **What fails a check,** read from cutechess's result lines and the PGN: any game ending in a time
  forfeit, a crash or disconnect ("disconnects", "stalls", "illegal move"), or a stop not answered.

### 1. Eight threads

48 games at CCRL Blitz scaled to the machine, 1'+0.5" on the machine of ccrl-conditions.md (to be
rescaled elsewhere): one side `option.Threads=8 option.Hash=512` (CCRL's exception allows four times
the hash for 4 CPUs; 512 is within the engine's 1,024), the other `Threads=1 Hash=128`.
`-concurrency 1`, on a machine with at least 8 physical cores and nothing else running. Passes with
no failure of the kind above. The score is noted but decides nothing.

### 2. A repeating control

48 games at `tc=40/60`, both sides one thread, Hash 128, `-concurrency 4`. Besides the failures
above, from the PGN's clock comments (`%clk`, cutechess writes them): the clock left at moves 38 to
40, and no move near the control spending more than half the time left. Fails on any forfeit; then
a margin in the time management is designed (TODO's "Clock safety at default settings").

### 3. Move Overhead

cutechess cannot measure the gap from the deadline to `bestmove`, but it can enforce it: games at
`st=0.1` with `timemargin=0` lose on time any move over 100 ms. 200 games, both sides one thread,
default Move Overhead (20 ms), `-concurrency 1`. Then the same at a near-empty Fischer clock,
`tc=1+0.01`. Passes with no forfeit. Any forfeit: the gaps are measured properly, a few lines of
Bash timing `go movetime` over UCI, and the default raised.

## Commands

From the repository's root in Git Bash, at the commit to release. The engine is the release's dry
run, not a snapshot (the developer, 2026-11-11): a new snapshot replaces the one lichess-bot is
running, while the dry run builds locally, never on Drive, and is the release's exact build (the C
runtime static, the notices). It needs `cargo about` on that machine (DEV.md):

    bin/arena release lc0-psq-threats-screlu-g1-r1@tuned --dry-run

    mkdir -p data/checks && cp target/release-build/fattorini-v0.0.0-dry-run.exe data/checks/fattorini.exe

Common to every check:

    SYZYGY="data/syzygy/3-4-5-wdl;data/syzygy/6-wdl;data/syzygy/3-4-5-dtz;data/syzygy/6-dtz"
    BOOK="file=data/books/UHO_Lichess_4852_v1.epd format=epd order=random"

1. Eight threads, a smoke test of about 10 minutes (the developer, 2026-11-11: no 1.5-hour run);
   10+0.1 stops the threads more often than Blitz would, which is what is checked:

       cutechess-cli -engine name=t8 cmd=data/checks/fattorini.exe option.Threads=8 option.Hash=512 -engine name=t1 cmd=data/checks/fattorini.exe option.Threads=1 option.Hash=128 -each proto=uci tc=10+0.1 "option.SyzygyPath=$SYZYGY" -openings $BOOK -repeat -games 2 -rounds 10 -concurrency 1 -ratinginterval 10 -pgnout data/checks/threads-8.pgn

2. A repeating control:

       cutechess-cli -engine name=a cmd=data/checks/fattorini.exe -engine name=b cmd=data/checks/fattorini.exe -each proto=uci tc=40/20 option.Hash=128 "option.SyzygyPath=$SYZYGY" -openings $BOOK -repeat -games 2 -rounds 8 -concurrency 4 -ratinginterval 10 -pgnout data/checks/repeating-40-60.pgn

3. Move Overhead, a fixed 100 ms a move with no tolerance, then a near-empty clock:

       cutechess-cli -engine name=a cmd=data/checks/fattorini.exe -engine name=b cmd=data/checks/fattorini.exe -each proto=uci st=0.1 timemargin=0 option.Hash=128 "option.SyzygyPath=$SYZYGY" -openings $BOOK -repeat -games 2 -rounds 10 -concurrency 1 -ratinginterval 20 -pgnout data/checks/movetime-100ms.pgn

       cutechess-cli -engine name=a cmd=data/checks/fattorini.exe -engine name=b cmd=data/checks/fattorini.exe -each proto=uci tc=1+0.01 timemargin=0 option.Hash=128 "option.SyzygyPath=$SYZYGY" -openings $BOOK -repeat -games 2 -rounds 10 -concurrency 1 -ratinginterval 20 -pgnout data/checks/clock-1-0.01.pgn

Each run ends with its score line. Then, for every PGN:

    grep -c "loses on time\|disconnects\|stalls\|illegal move\|crash" data/checks/<check>.pgn

0 is a pass. Anything else: the games are read before anything is changed.

## Decided (the developer, 2026-11-11)

- **cutechess-cli:** independent of Fattorini, as a tester's GUI is; no arena code for checks run
  once a release. fastchess is not installed.
- **Smoke tests, about 10 minutes each** (the developer, 2026-11-11): 20 games for 8 threads at
  10+0.1, 16 at 40/20 (repeating, shorter than 40/60 and so harsher), 20 at 100 ms a move, 20 at
  1+0.01. Enough to see a failure that happens often, not a rare one.

## Results

**2026-11-11, all four passed** on the training machine, the dry run's `.exe` of 2026.11.0: no
game lost on time, no disconnect, stall, illegal move or crash in any PGN.

| Check | Games | PGN |
|---|---|---|
| 8 threads against 1, 10+0.1 | 20 | `data/checks/threads-8.pgn` |
| Repeating 40/20 | 16 | `data/checks/repeating-40-20.pgn` |
| 100 ms a move, `timemargin=0` | 20 | `data/checks/movetime-100ms.pgn` |
| 1+0.01, `timemargin=0` | 20 | `data/checks/clock-1-0.01.pgn` |

So no margin is added to the time management for the release; TODO's "Clock safety at default
settings" stays open as a question for later, not a blocker.
