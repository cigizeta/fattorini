# `rotor-arena match` at a fixed depth, with threads

Status: plan written 2026-09-28, awaiting the developer's approval.

## Why

Stockfish's way: a new network is kept only if an SPRT says it beats the previous best. Here that
means dorothy-0001 against caroline-0001 at 10 Elo. `match --sprt` plays 3+2, one game at a time, 8
to 10 minutes a game: an SPRT at 10 Elo would take days. The gate plays the same test at a fixed
depth 9 with pairs in parallel, which is the condition the networks were trained for, and takes
about an hour. `match` should be able to do the same between any two named players.

## The command

```
rotor-arena match dorothy-0001 caroline-0001 --sprt 10 --depth 9 --threads 8
```

- `--depth <d>` and `--threads <n>` go together, and only with `--sprt`: fixed depth, `n` pairs in
  flight, as the gate. Without them, `match` is unchanged (3+2, one game at a time).
- `--best-of` stays clock-only: a best-of match is for watching games, not for a verdict.

## Decisions

- **Players at a fixed depth play in-process where they can**, exactly as in the gate:
  `hand-crafted` as `EngineKind::Builtin`, `<network>-NNNN` as `EngineKind::Network` with its
  `champion.nnue`. Same search, same 16 MB table as the gate, no subprocess per game. Stockfish
  stays a UCI subprocess, sent `go depth`.
- **The record says its control**: `spec.txt` reads `sprt elo_d 10 at depth 9`, so rerunning the
  same command resumes it and a rerun with another control is refused, as today.
- **Moves are not printed** at a fixed depth with several pairs in flight, as in the gate; the
  score line after every pair is.
- **Same folder layout**: `data/matches/<one>-vs-<two>/<timestamp>/`, with `games.pgn`,
  `results.db`, `spec.txt`, `result.txt`.

## Implementation plan

- `src/arena/match_player.rs`: `resolve_player` takes whether the match is at a fixed depth; at a
  fixed depth, `hand-crafted` and networks resolve to in-process entries. Tests for both.
- `src/arena/main.rs`: `parse_match_args` accepts `--depth` and `--threads` after `--sprt`, both
  or neither, each at least 1; `MatchRule::Sprt` carries an optional `(depth, threads)`;
  `spec_line` includes the depth; `run_match` passes `Control::FixedDepth(depth)`, `threads` and
  `print_moves: false` to `gate::play_sprt`. Usage text. Tests for the parsing and the spec line.

## Progress

- 2026-09-28: plan written; approved and implemented, not yet built. `spec.txt` names the depth but
  not the thread count, so a match can resume with another number of threads.
- 2026-09-28: the first run panicked: half pairs need one pair in flight. Fixed by recording half
  pairs only when `threads == 1`; with several, whole pairs only, as the gate. dorothy-0001 vs
  caroline-0001 at depth 9, 8 threads: stopped at 552 games, +223 =133 -196 (52.4%, about +17 Elo
  +/- 25), no verdict. The two are about the same strength.
