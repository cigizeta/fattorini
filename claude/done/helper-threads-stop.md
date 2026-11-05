# Helper threads stop when thread 0 ends

**Status:** done; the fix and its test are in `uci.rs` (checked 2026-10-14).

Agreed with the developer on 2026-10-06, after rotor-bot hung on Lichess (game 9qvsokTS): lichess-bot
sent `go wtime 97480 btime 166440` and no `bestmove` came for about 5 minutes, until the developer
quit. At least two games are believed lost on time this way, one of them in KR vs K.

## The bug

`go_search` (`uci.rs`) runs thread 0 plus `Threads - 1` helpers, then joins all of them before
printing `bestmove`. Helpers have no clock: they stop only when `stop_signal` is set. Thread 0 sets
it only when the time manager refuses the next depth (`choose_best_move_with_progress`,
`may_start_next_depth`) or its deadline passes mid-search. Its loop also ends without setting it:

- at the depth cap, `max_depth.min(MAX_PLY)` (depth 100 under a clock: the PGN shows many depth-100
  searches in won and lost endings);
- on `pv.is_empty()`.

Then the join waits for helpers still deep in an iteration with no clock, and `bestmove` never
comes in time. Endings make it likely: thread 0 reaches depth 100 fast on mate and tablebase scores,
while the WDL probe only fires right after a zeroing move, so a helper's deep iteration in KR vs K
gets no cutoff from it.

## The fix

In `go_search`, thread 0's closure sets `stop_signal` once its search returns, however it ended,
before the join. Stockfish does the same: its main thread sets `threads.stop` when its iterative
deepening ends, then waits for the workers.

Not in `search.rs`: `choose_best_move` with `thread_idx` 0 is also called by `in_process.rs`,
`self_play.rs` and `replay.rs` with a `stop` flag that outlives one move (the game's or the batch's
stop request). Setting it there would make every later search of that game stop at depth 1. The fix
touches `uci.rs` only, so the label path (self-play and replay, single-threaded through
`choose_best_move`) is unchanged and the freeze holds.

Ponder is unaffected: the ponder wait follows the join, so a ponder search that ends on its own
stops its helpers, then still waits for `ponderhit` or `stop` before answering.

Done through a small function, `run_main_thread(stop_signal, search)`, which runs thread 0's search
and then sets the signal, so the property has a test of its own: a race between real threads would
not reproduce it reliably.

## Steps

1. `run_main_thread` in `uci.rs`, used by thread 0's closure for both the MultiPV and the single-line
   search.
2. Tests: `run_main_thread` returns the search's result and leaves `stop_signal` set; the existing
   `uci.rs` tests (ponder, stop, MultiPV) still pass.

## Verify (Windows, the Lichess machine)

```
cargo test --release --lib uci
```

Then lichess-bot on the new binary (`smithy install`), `Threads: 4`; no game hangs waiting for
`bestmove`.
