# Time management from scratch

Status: **design in progress** (the developer, 2026-11-12). No plan, no code yet.

## Why

The developer, 2026-11-12, during the 180+2 duel of Split v2 against Classic
(blitz-time-management-v2.md), with 110 pairs played and the duel a dead heat: v2's implementation
does not convince. The duel is left to finish, and its verdict is recorded in
blitz-time-management-v2.md as decided. In parallel, a redesign from scratch, inspired by Stockfish
and Reckless but not copied from them (their sources to be checked before any detail is fixed),
built so that SPSA does the tuning.

What v2 has that this design drops:

- **The reserve and settle-point machinery** (`R`, the horizon `H`, `L`, `plan`), which exists only
  to steer where the clock settles.
- **The `s` rule** (no new depth past a fraction of `soft`): the stop is checked after each depth
  against the scaled budget, and the tuned coefficients absorb the overshoot.
- **Rules correcting each other** (`s`, the reserve, the cap): each part here has one job.

The constant count is about the same as v2's (8 to 10), but each one has a single meaning, so SPSA
can tune it. More is left to measurement than to arithmetic: the starting values are rough, and the
design is only as good as its tune.

## Shape

1. **Base budgets,** computed once per move, from the time left `T`, the increment `I` and the move
   overhead.
2. **Factors,** applied to `soft` after each completed depth, from the search itself:
   - **Node fraction:** the share of the last depth's nodes spent on the best move. Large, stop
     early; small, keep going.
   - **Best-move stability:** how many depths in a row the best move has stayed the same.
   - **Score trend:** a score falling against the previous move's means think longer.
3. **Stop rules:** after a depth, stop once the time spent exceeds the scaled `soft`; `hard` aborts
   the running depth regardless.

## Two models: the base differs, the rest is shared

Fischer clocks (Lichess) and repeating clocks with `movestogo` (CCRL's 40/15) need different bases:
under `movestogo` the engine knows how many moves `T` must cover, so a fixed fraction of `T` would
spend too fast with 40 moves to go and leave the clock unused with 2.

| | Fischer (Lichess) | `movestogo` (CCRL 40/15) |
|---|---|---|
| Base `soft` | `a * T + b * I` | `a' * T / movestogo` (+ `b * I` if there is an increment) |
| Base `hard` | `c * T + d * I`, capped | `c' * T / movestogo`, capped |
| Node fraction, stability, score trend | same | same |
| Stop rules, overhead, cap | same | same |

- `a'` near 1 is an even share; a little above 1 spends more early.
- Example: 40/15 with 10 minutes left and 20 moves to go, an even share is 30 s a move.
- The factors measure how hard the position is, which does not depend on the clock type: tuned once,
  under Fischer, and reused under `movestogo`.

## Tuning

- **Tuned by SPSA, at 10+0.1:** `a`, `b`, `c`, `d` and the factors' coefficients. Before it: the
  smithy's bench on an idle machine (TODO.md), or the tune plays at the wrong control.
- **Never tuned:** the move overhead and the cap on one move, the safety margins a lag-free tune
  would drive to zero.
- **Proposed, not decided:** `a'` and `c'` hand-picked near an even share, kept conservative,
  checked by a smoke test and a short duel at a repeating control rather than tuned.

## Open questions

One at a time, in this order:

1. The `movestogo` coefficients hand-picked and checked, not tuned (proposed above).
2. **Node fraction in the first version?** It needs nodes counted per root move; whether the root
   loop does so today is not yet checked. Recommended: yes, if counting is cheap; otherwise first
   stability and score trend alone, node fraction a step of its own.
3. The factors' exact forms and starting values, after reading Stockfish's and Reckless's sources.
4. The verdict: which duels, at which controls, against whatever ships after v2's verdict.
