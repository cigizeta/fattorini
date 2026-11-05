# A single legal move gets a capped search

Planned 2026-10-03. Status: done. Implemented 2026-10-03 on the Lichess machine (`single_move_limit`,
`SINGLE_MOVE_CAP_MS`, the `single_move` flag on `TimeLimit::Clock`); reviewed against this plan and
all tests passed 2026-10-04 on `bartolomeo`. Not pulled onto the Lichess machine or another machine while caroline's replay
and self-play run, so the label path's binary stays the one that started them.

## Problem

Nothing in `search.rs` or `uci.rs` looks at the root move count, so under a clock a forced move gets
the same target as any other (`TimeManager`, `clock_target_ms`): time spent learning nothing.

## Stockfish's rule (fetched from `src/search.cpp` on master, 2026-10-03, not recalled)

Inside its time-managed branch only, `if (limits.use_time_management() && !threads.stop &&
!mainThread->stopOnPonderhit)`, after every completed depth:

```cpp
double totalTime = mainThread->tm.optimum() * fallingEval * reduction
                 * bestMoveInstability * highBestMoveEffort;

if (rootMoves.size() == 1)
    // Cap used time to 0.5s for a better viewer experience
    totalTime = std::min(500.0, totalTime);
```

then stops once `elapsedTime > std::min(totalTime, double(mainThread->tm.maximum()))`. It still
searches, briefly, so that it reports a score and a ponder move; it does not move instantly. With
zero root moves it reports mate or draw and `bestmove (none)` at once, which Rotor already does.

## Rotor's version

Wider than Stockfish's (the developer, 2026-10-03): **every move played in a game** gets the cap,
whatever limits its search, a clock or a fixed depth; **no search whose score is kept** does.

- **Games, capped:** UCI `go` with a clock, `go depth N`, and `go movetime` (lichess, timed matches,
  a GUI), and `rotor-arena`'s in-process player at a fixed depth (the selection's depth-9 round
  robin, `in_process.rs`). In a game only the move matters, and a forced move has one.
- **Self-play and replay, never capped:** they search at depth 9 for the **score**, which becomes the
  training label, and a forced position needs its full-depth evaluation as much as any other. They
  call `choose_best_move` directly (`self_play.rs`, `replay.rs`), not through `go`, so the cap lives
  in the game callers and `choose_best_move` itself is unchanged: neither can reach it.
- **Not `go infinite`, not a ponder search before its `ponderhit`:** both run until told to stop; the
  search after a `ponderhit` is a game move and is capped.
- **What the cap is:** `SINGLE_MOVE_CAP_MS = 500`, a named constant with Stockfish's value and reason,
  applied to the limit by one helper in `search.rs`, called by `uci.rs`'s `go_search` and
  `in_process.rs`'s `choose_move` when the position has exactly one legal move (`legal_moves`, once per
  move, off the hot path):
  - under a clock, `TimeLimit::Clock` carries `single_move: true` and `clock_target_ms` caps the
    target after its own formula and `MAX_CLOCK_FRACTION`, so the cap only ever lowers it;
  - under `movetime`, `Fixed { min(movetime, 500) }`;
  - under no time limit (`go depth N`, the in-process fixed depth), `Fixed { 500 }`, alongside the
    depth: whichever comes first ends the search.
- Rotor's target drives both its gates (`TimeManager`): no new depth after half the target (250 ms),
  and the running depth aborted at the target. Stockfish only checks after completed depths, so Rotor
  can stop slightly sooner; with one legal move, nothing is lost.

## Tests

- `time_limit` on a position with one legal move gives `single_move: true`; on the start position,
  `false`.
- The clock target with `single_move` is at most 500 ms on a long clock, and unchanged (already
  smaller) on a clock below it.
- The helper: on a one-move position, a clock gets `single_move`, `movetime 2000` becomes 500, no
  limit becomes `Fixed { 500 }`, and `infinite` stays unlimited; on the start position, every limit
  is unchanged.
- `go depth 20` on a one-move position returns its move within about 500 ms.
- `choose_best_move` at depth 9 with no limit, as self-play and replay call it, on a one-move
  position, reaches depth 9 and reports that depth's score: the label is never cut short.
- A real `go wtime ... btime ...` on a one-move position returns its single move as `bestmove`
  within a bound well under the uncapped target.

## Measuring

None: it changes nothing but the time of forced moves, which only saves clock (and, in the
fixed-depth round robin, wall clock). No match needed.
