# Continue the ponder search on ponderhit

Status: implemented 2026-10-03 (`PonderState` in `search.rs`), tests not yet run; the Lichess
check below is left. On the Lichess machine's branch only: the training machine does not pull
before betty's generation 1 is selected and promoted (TODO.md, Search).

Supersedes pondering.md's "a live switch can come later if Lichess games show it matters".

## Today

`go ponder` runs a search with no limit (`infinite`), keeping the `go`'s time fields aside. On
`ponderhit`, `uci.rs` stops it, joins its thread and starts a fresh timed search from depth 1 with
the full budget. Only the transposition table carries over; the time manager's instability starts
over at 1, and the time spent pondering is not counted.

## Stockfish (fetched 2026-10-03, master)

- `go ponder` sets `limits.ponderMode`; the time manager is initialized as for any timed `go`, and
  `startTime` is the `go`'s arrival, so the elapsed time includes the ponder time.
- `ponderhit` only does `threads.main_manager()->ponder = false`. The search goes on.
- Between iterations, when the time is up:
  `if (mainThread->ponder) mainThread->stopOnPonderhit = true; else threads.stop = true;`
- `check_time()` (in the tree): `if (ponder) return;`, then stops when
  `elapsed > tm.maximum() || stopOnPonderhit`.
- `timeman.cpp`: `if (options["Ponder"]) optimumTime += optimumTime / 4;`

## Design

The ponder search is the game move's search from the start, with the real clock, only told not to
stop while pondering.

1. **A shared `pondering: AtomicBool`**, true from `go ponder` until `ponderhit`, created by
   `go_search` and read by thread 0 only. Helpers stay as they are: they only follow `stop_signal`.
2. **The ponder search gets the real `TimeLimit`** from the `go ponder`'s fields, through
   `single_move_limit` like any game move, and its `TimeManager` starts at the `go ponder`'s
   arrival, as Stockfish's `startTime`: the whole ponder time counts as spent.
3. **Between depths** (`choose_best_move_with_progress` and the MultiPV one): when
   `should_start_next` says no while pondering, it sets `stop_on_ponderhit` and goes on deepening
   instead of stopping. Not pondering, unchanged.
4. **In the tree**: the deadline check at `CHECK_INTERVAL` is skipped while pondering, and also
   stops when `stop_on_ponderhit` is set and pondering has ended. A depth's deadline is still fixed
   when it starts, so after `ponderhit` a depth begun while pondering usually finds its deadline
   already past and stops at the next check; it is discarded as any stopped depth is, and the last
   completed depth's move is played.
5. **`ponderhit`**: clears `pondering` and sets the outcome to `PONDER_HIT`; the thread is neither
   stopped nor joined. The search thread prints `bestmove` itself when it ends, as a timed search
   does. A ponder search that already ended on its own (a `depth`, a mate, `MAX_PLY`) still waits,
   and on `ponderhit` prints its `bestmove` at once.
6. **`stop` while pondering**: unchanged, prints `bestmove`, which the GUI ignores.
7. **The instability carries over**: same `TimeManager`, nothing to do.

Not adopted here: Stockfish's `+25%` of the optimum when `Ponder` is on. Only an SPRT can adopt it,
run with cutechess-cli, which ponders (`-each ponder`) where `rotor-arena` does not, after this
design is in. The `+25%` goes behind a temporary UCI option for that SPRT only, removed once it is
decided, as the move picker's switches were.

## What changes

- `search.rs`: `Search` gains `pondering` and `stop_on_ponderhit` (both `Option<&AtomicBool>`,
  `None` outside `go_search`); the deadline checks in `alpha_beta_with_buffer` and
  `quiescence_from` read them, only at `CHECK_INTERVAL`, so the hot path is untouched;
  `choose_best_move_with_progress` and the MultiPV variant take `pondering: Option<&AtomicBool>`;
  `choose_best_move` passes `None`, so self-play, replay and the arena are unchanged.
- `uci.rs`: `go ponder` builds the real limit; `RunningSearch.ponder` no longer holds a `go` to
  restart; `PonderHit` flips the flags instead of stop, join and `handle`; the thread no longer
  returns silently on `PONDER_HIT`.

## Tests

- After a ponder longer than the target, `ponderhit` gets `bestmove` well under the target (say
  `go ponder wtime 2000 btime 2000`, sleep 1 s, `ponderhit`, `bestmove` within 200 ms).
- An immediate `ponderhit` gives a normal timed search: `bestmove` within the target and not at
  once.
- `info depth` keeps rising across `ponderhit`, never restarting from 1.
- `stop` while pondering still prints `bestmove`; a finished ponder search still waits for
  `ponderhit` or `stop`.
- A single legal move: `bestmove` within 500 ms of `ponderhit`.
- The existing `ponderhit_answers_once_from_the_timed_search` and
  `ponderhit_with_nothing_running_is_a_silent_no_op` still pass, one `bestmove` per `go`.
- Fixed-depth test unchanged.

## Check

One Lichess game with `ponder: true`: its log shows `ponderhit` answered at once after long ponders
and no loss on time; then this and pondering.md move to `done/`.
