# Move Overhead

**Status:** design approved by the developer 2026-11-09; implementation plan in
move-overhead-implementation.md. To be implemented only once the current SPSA run has finished,
so that a stop and restart of that run never meets a changed time management.

## The problem

Fattorini keeps no margin for lag. A move's target is computed from the whole clock the GUI sends
(`clock_target_ms`, capped at `MAX_CLOCK_FRACTION * T`), and `movetime` is used as given. Nothing
covers what happens outside the search's own timing:

- the deadline is read only every `CHECK_INTERVAL` (1024 nodes, about 10 ms at an old 99,000
  nodes/second; longer with a network evaluation), then the search unwinds and writes `bestmove`;
- the time between `go` arriving and the search starting is not counted;
- the GUI's own handling, and the operating system taking the core away when a tester runs several
  games on one machine.

On Lichess this never showed, because lichess-bot's `move_overhead` (2000 ms) already takes 2 s off
the clock it sends. CCRL testers run the engines under a GUI on one machine, with no lichess-bot, and
are assumed to set no option: at default settings, Fattorini depends on whatever the GUI forgives.
The cap being relative makes it worst near zero: at 20 ms left a move may plan 10 ms, while lag does
not shrink.

## Decisions

1. **A fixed overhead per move, the Stockfish way:** a `Move Overhead` UCI option, in milliseconds,
   charged as a fixed amount per move, not a fraction of the clock.
2. **Default 30 ms.** Stockfish's 10 ms covers mostly the GUI side; Fattorini overshoots its own
   deadline more (fewer nodes per second, the same 1024-node check), and CCRL testers often run
   games concurrently. The cost is negligible: about 1.3 s set aside at the start of a 40/15
   control, 60 ms under Fischer. To be revised once measured (below).
3. **Range 0 to 5000.** 0 for a caller that keeps its own margin, as lichess-bot does.
4. **The clock is reduced before the formula:** `T' = max(1, T - overhead * k)`. The formula, the
   `MAX_CLOCK_FRACTION` cap, the instability, the falling-score factor and the stop fraction `s`
   are unchanged; they read `T'`. A move therefore never plans to spend the reserve.
5. **`k`, the moves the reserve covers:** `M + 2` under `movestogo` (Stockfish's `2 + mtg`), one
   overhead kept for each move left before the control plus two spare; `2` under Fischer, where the
   increment refills the clock and a stable move spends half of it.
6. **`movetime` is reduced too:** `max(1, movetime - overhead)`. Unlike Stockfish, since the search
   stops at exactly the target and always runs past it by the unwinding and the `bestmove` write.
7. **The arena's in-process player uses the default**, so a timed match plays as the release does.
8. **lichess-bot keeps its own `move_overhead`** for now; the engine's 30 ms adds to it. Lowering
   it, with `Move Overhead` passed through `uci_options`, is a later choice for the developer.

## The measurement that may revise the default

On an idle machine, with a network: over a few hundred `go movetime` moves and near-empty clocks,
the gap from the deadline to `bestmove`, the worst one included. The default becomes about twice
the worst gap, rounded. A worst gap above 15 ms points at `CHECK_INTERVAL` first. The
repeating-control check of TODO.md (`tc=40/60`) runs after this change, and judges it.
