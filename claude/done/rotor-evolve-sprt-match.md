# rotor-arena sprt -- an SPRT match between two named players

**Command line superseded, see arena-match-rules.md**: the `sprt` subcommand became
`match --sprt <elo_d>`, and alpha and beta are fixed at 5%. The runner described here stands.

Status: plan, awaiting approval. Revised 2026-09-27 after the binaries were reorganised
(three-binaries.md) and against the decisions recorded in TODO.md. Builds on rotor-evolve-match.md
(the best-of-12 `match` and its players) and rotor-network-gate.md / sprt.md (the gate and its
test).

## Goal

Two ways to pit two players against each other, both in `rotor-arena`:

- `rotor-arena match <one> <two>`: unchanged, best of 12 at 3+2.
- `rotor-arena sprt <one> <two> <elo_d> <alpha> <beta>`: new. Colour-reversed pairs at 3+2 until
  the sequential test resolves.

Players are the set `match` takes: `hand-crafted`, `stockfish-<level>`, `stockfish-elo-<rating>`,
`<network>-NNNN`, every one a UCI subprocess.

Two changes ride along, decided with the developer:

- Every clocked match player gets `Hash` 64 instead of 16, `match` included.
- The gate draws random openings with system entropy, as `match` already does.

## Decisions

1. **A subcommand, `sprt`**, with `<elo_d> <alpha> <beta>` positional, in that order, all required.
   No defaults: a test's meaning is its parameters, so the command line always says which one ran.
2. **Always sequential.** One game at a time and nothing else in the process, so neither side's
   clock competes with another game for the machine. No `--threads`; passing it is an unknown
   flag. This holds for every clocked match in `rotor-arena`, `match` and `sprt` alike. The gate,
   at fixed depth where timing does not matter, keeps its `--threads`.
3. **`Hash` 64, `Threads` 1** for every clocked match player (`EQUAL_FOOTING_OPTIONS` in
   `match_player.rs`). 16 was only both engines' default and fills quickly at about 5 s per move.
4. **Random openings in the gate too**, one pick per pair from `RandomBook` with `rand::rng()`.
5. **A match folder records what it is** (`spec.txt`), since `match` and `sprt` share
   `data/matches/<one>-vs-<two>/`.

## The test

`sprt.rs` unchanged. One-sided, as for the gate: H0 "one and two are equal", H1 "one is at least
`elo_d` stronger than two". **Argument order matters**: `sprt amanda-0001 stockfish-elo-1500 35
0.05 0.05` asks whether amanda is at least 35 Elo above Stockfish at 1500. Reported as:

- upper bound crossed: `H1: amanda-0001 is at least 35 Elo stronger than stockfish-elo-1500`
- lower bound crossed: `H0: amanda-0001 is not 35 Elo stronger than stockfish-elo-1500`

`Sprt::new` asserts its parameters. `sprt` validates them first (`elo_d > 0`, `alpha` and `beta` in
`(0, 1)`) and exits with the usage text instead of panicking.

What it costs, so it is not a surprise: at 3+2 a game takes about 10 minutes, and a test at
`elo_d` 35 typically needs a few hundred games, so days of one machine. A smaller `elo_d` costs
quadratically more. Being wrong about which side is stronger does not stop the test: it ends at
H0, usually sooner.

## Implementation

### 1. gate.rs becomes the SPRT runner for both

`play_gate` already is an SPRT runner: whole-pair atomic recording, replay-on-resume, discarding
pairs finished after resolution, Ctrl+C mirroring. With one worker it is exactly the sequential
match decision 2 asks for, so `sprt` needs no loop of its own. Three things hard-wired today
become parameters, gathered in one `SprtRun` struct so the argument list does not grow further:

- **the control**: `Control::FixedDepth(depth)` becomes `control: Control`. The gate passes
  `FixedDepth(generation::SEARCH_DEPTH)`, `sprt` passes `Control::parse("3+2")`.
- **the wording**: the `gate:` prefix and the `PROMOTE`/`FAIL`/`PASS` lines, supplied by the
  caller.
- **per-move output**: the gate prints none (sixteen games interleaved would be noise); `sprt`
  prints each move as `match` does, since only one game is ever in flight.

The return value stays "the winner's name, or `None` if interrupted". Faults keep the gate's rule:
a fault stops the run and the half pair is not recorded, so the run is resumable. A crashed or
illegal-moving engine is not evidence for either hypothesis.

**Half pairs are recorded for `sprt`, not for the gate** (`record_half_pairs` in `SprtRun`). The
gate drops a half pair because with sixteen workers several are in flight and finish out of order,
and one could finish after the test resolved. With one game in flight none of that can happen: at
most one pair is half-played, always the last, and no pair starts after resolution. So for `sprt`:

- a pair's first game is recorded as soon as it ends, PGN first and then `results.db`, as `match`
  does, so a database row always has its opening in the PGN;
- the test does not see it until its partner exists: `replay` consumes whole pairs only, so the LLR
  stays exactly the statistic over completed pairs;
- a resume finishes a trailing half pair before anything else: the second game, colours reversed,
  from the opening in the PGN's last `[FEN]` tag;
- a fault still stops the run, and the recorded first game waits for the resume.

A Ctrl+C then loses at most the game in flight, as in `match`. The gate keeps the whole-pair rule:
its fixed-depth games last seconds, and out-of-order finishing is its real reason.

The file keeps its name; its header says `sprt` uses it too.

### 2. Random openings in the runner

`play_gate` opens `RandomBook` once (one scan of the book, about a second) and each worker takes
its own `book.reader()` and `rand::rng()`. `next_pair_index` stays: it no longer picks the opening,
but it still numbers the pairs, and `game_index` uniqueness across a resume depends on it. Its doc
comment and the `replay_takes_the_next_opening_...` test are reworded from "opening" to "pair
index"; the logic does not change. `book::opening_fen` loses its last caller and is deleted.

No FEN recovery on resume is needed, unlike `match`: only whole pairs are recorded, so a resume
never finishes a half pair.

### 3. The `sprt` subcommand (arena/main.rs)

`run_sprt(one, two, spec)`:

- players via `match_player::resolve_player`, the same checks as `run_match` (two different names).
- folder: `data/matches/<one>-vs-<two>/<YYYYMMDD-HHMM>/`, chosen by `choose_match_dir`. See 4.
- `results.db`, `games.pgn`, round `1`, event `"<one> vs <two> sprt"`.
- one worker, the 3+2 control, per-move output on.
- at the end, `result.txt`: the record line, the LLR with its bounds, and the verdict line, via
  `match_player::write_result`. As for `match`, its presence closes the folder.

`MATCH_TIME_CONTROL` (3+2) is shared by `match` and `sprt`.

### 4. A folder knows what it is

`choose_match_dir` resumes the latest unfinished folder of a pairing. Without a guard an
interrupted best-of-12 could be resumed as an SPRT, or an SPRT with other parameters, mixing two
tests in one database.

So each folder gets `spec.txt` when it is created, one line:

- `best-of 12 at 3+2`
- `sprt elo_d 35 alpha 0.05 beta 0.05 at 3+2`

On resume the line must equal the one this run would write, or the run exits naming the folder and
both lines. It does not silently start a new folder: an unfinished run the developer forgot about
should be noticed, not buried. A folder without `spec.txt` predates this change and is read as
`best-of 12 at 3+2`, all it could be. `Hash` is not in the line: it is the same for every match
from now on, and a best-of-12 resumed across this change finishing at 64 is harmless.

### 5. Hash 64

`EQUAL_FOOTING_OPTIONS` becomes `[("Hash", 64), ("Threads", 1)]`, its test and doc comment with it,
and README.md's match paragraph.

## Tests

- `sprt` argument parsing: valid; each parameter out of range; missing parameters; a stray flag.
- `spec.txt`: written on a new folder; matching resume accepted; mismatching resume refused;
  missing file read as best-of-12.
- the verdict line for each bound.
- gate replay tests: unchanged apart from the renamed one.
- `EQUAL_FOOTING_OPTIONS`: `Hash` 64.

The random opening wiring is not unit-tested; the first real gate run checks it (distinct `[FEN]`
tags per pair in `gate.pgn`).

## Out of scope

- Two-sided tests (`elo0`/`elo1`, as fishtest uses). The one-sided test answers "is A at least d
  above B", which is the question both the gate and a calibration run against Stockfish ask.
- An Elo estimate with error bars from the recorded games. Useful, separate.
