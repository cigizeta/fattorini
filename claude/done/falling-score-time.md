# Time from a falling score

**Status (2026-10-08):** done and kept. Implemented (`falling_score_factor`, `TimeManager`,
`TimeLimit::Clock`'s `previous_score`; `uci.rs`'s `last_score`; the arena's in-process player),
tests passing. Duel on idle the Lichess machine, generation 2's replay stopped for it: **+66 -64 =107 for the
new build, 0.504, +2.9 +/- 32.8 Elo over 237 games** (8 more aborted when the developer stopped
it), neither side near its bound (0.600 at 237). Drawn, so kept under the harm-check rule; stopped
at 237 games it rules out only a loss of about 70 Elo or more, not the 35 a full run would.

The cutechess command that worked: `EvalFile` as an absolute path (`$(pwd -W)/...`), since
cutechess-cli starts each engine in the engine's own folder and Rotor exits when its `EvalFile`
fails to load; with a relative path both engines died right after `isready`.

What time-management.md left: score instability. The clock target grows with best-move
instability `C` only; a search finding the position worse than the game thought gets no more time.

## Decisions (2026-10-08, the developer)

- **A separate factor, not folded into `C`:** a move flipping between two equal moves and a score
  falling under one best move are different events, each keeps its own constant, and Stockfish
  multiplies separate factors too.
- **Against the previous move's final score, as Stockfish** (`bestPreviousAverageScore`, fetched
  2026-10-08 from `search.cpp`), not against the previous depth: a drop between depths of one
  search is noisy (odd-even), and a drop the opponent's move caused is already there at depth 1.
- **Both ways, as Stockfish** (its factor is clamped to [0.576, 1.728]): a rising score saves time
  for the moves where it falls.
- **One reference only:** Stockfish's second term (`iterValue`, the last four moves' scores,
  weight 1.1 against 2.30) is a refinement of its tuning, a weight we could only guess.
- **The scale follows the evaluator's pawn** (`pawn_percent`, as the pruning margin), so a drop of
  one real pawn buys the same time under every network.

## The formula

```text
F = clamp(1 + (previous - current) / (200 cp * pawn_percent / 100), 0.6, 1.7)
target = min(((alpha + 1/M) * T + beta * I) * (1 + r * C) * F, 0.5 * T)
```

- `previous`: the final score of this engine's previous search in the game, from its own side
  (the same side moves at every one of its searches). `current`: the score of the depth just
  completed, so `F`, like `C`, is recomputed after every depth and the running depth keeps its
  deadline.
- `F = 1` when there is no previous score (the first search of a game, after `ucinewgame`), and
  when either score is a mate or a tablebase score: those bands are not evaluations.
- Bounds 0.6 and 1.7 (Stockfish's, rounded) and 200 cp are defaults; the duel judges the term as a
  whole. Fixed `movetime`, fixed depth and no clock are untouched, so self-play and replay are too.

## Where the score lives

- `uci.rs`'s engine keeps the last search's final score, cleared by `ucinewgame`, and passes it
  into the search with the time limit; a ponder search counts as the move's search.
- The arena's in-process player under a clock (`arena/in_process.rs`, which the duel uses) keeps
  its own the same way, cleared per game.
- `TimeManager` takes `previous: Option<i32>` and the scale; `clock_target_ms` takes `F`.

## Tests

- `F`: 1 without a previous score, with a mate or tablebase score on either side, and for an equal
  score; a drop of a third of the scale gives about 1.17, a rise of a full scale 0.6 (clamped
  from 0), a drop of two scales 1.7.
- The same real drop gives the same `F` at `pawn_percent` 70 as at 100.
- `clock_target_ms` with `F = 1` equals today's target.
- `ucinewgame` clears the previous score; a new in-process game starts without one.

## Duel: a harm check (decided 2026-10-08, the developer)

One duel at 10+0.1, the new build against the one before it, both on amanda-0001 (the champion
labelling generation 2, with her `margin.txt`), through cutechess-cli with two binaries as B1's:
in-process players run the same code, so `rotor-arena duel` cannot compare builds. Stopped by
Rotor's duel rule (0.5 + 3.09 * 0.5 / sqrt(N)) or at 1000 games. **A drawn duel keeps the term; only a clear loss removes
it.** This reads "drawn" differently from B2, B4 and B6, which a drawn duel shelved: those were
speculative refinements, while a falling-score term is a standard component of engine time
management (as the TT mate-distance fix was adopted on its own standing), whose gain, a few Elo,
the duel rule cannot resolve; the duel only guards against a badly scaled `F` losing clearly.

## Not included: more time when pondering

Stockfish adds a quarter to every search's optimum while `Ponder` is on (`timeman.cpp`, fetched
2026-10-08: `if (options["Ponder"]) optimumTime += optimumTime / 4;`), on every move, hit or not.
Rotor tried exactly that and removed it (2026-10-03, TODO.md: inconclusive after 620 games at +5
Elo), and `rotor-arena` never ponders, so no duel could measure it.
