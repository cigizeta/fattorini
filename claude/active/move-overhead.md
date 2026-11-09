# Move Overhead

**Status:** design approved by the developer 2026-11-09; implementation plan in
move-overhead-implementation.md. To be implemented only once the current SPSA run has finished,
so that a stop and restart of that run never meets a changed time management.

## The problem

Fattorini keeps no margin for lag. A move's target is computed from the whole clock the GUI sends
(`clock_target_ms`, capped at `MAX_CLOCK_FRACTION * T`), and `movetime` is used as given. Nothing
covers what happens outside the search's own timing:

- the deadline is read only every `CHECK_INTERVAL` (1024 nodes: about 1.2 ms at the 841,150
  nodes/second per thread benched with a network on 2026-11-09), then the search unwinds and writes
  `bestmove`;
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
2. **Default 20 ms** (the developer, 2026-11-09: a modern machine has no real lag problem). The
   search overshoots its deadline by about a millisecond (`CHECK_INTERVAL` at the benched speed), so
   20 ms covers that, the GUI's side and a tester running games concurrently, with room above
   Stockfish's 10 ms. The cost is negligible: about 0.8 s set aside at the start of a 40/15
   control, 40 ms under Fischer. To be revised only if the measurement (below) asks for it.
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
8. **lichess-bot keeps its own `move_overhead`** for now; the engine's 20 ms adds to it. Lowering
   it, with `Move Overhead` passed through `uci_options`, is a later choice for the developer.

## The measurement that may revise the default

On an idle machine, with a network: over a few hundred `go movetime` moves and near-empty clocks,
the gap from the deadline to `bestmove`, the worst one included. The default becomes about twice
the worst gap, rounded, if that exceeds 20 ms. A worst gap above 10 ms points at `CHECK_INTERVAL`
first.

The SPSA tune's games cannot settle this: under Fischer the clock settles at about twice the
increment, so a move would have to overshoot by about the increment to flag, far beyond any lag. The
repeating-control check of TODO.md (`tc=40/60`) runs after this change, and judges it.
