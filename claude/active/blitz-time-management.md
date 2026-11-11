# Blitz time management: a soft time, a hard maximum, the increment counted

Status: design settled (2026-11-11), implementation plan next. Nothing implemented.

## The problem

Two Lichess blitz games on 2026-11-11, both after `move_overhead` in lichess-bot's `config.yml`
went from 2000 to 500 ms:

- 3+3, a loss: Fattorini ended with about 1:00 on its clock, the opponent with 0:31, and the game
  was decided by a tactic Fattorini missed (39.Rxg4 Rxg4 40.Qf3+).
- 5+3, a draw by perpetual check: Fattorini ended with 2:28 of 5:00, the opponent with 1:18; its
  clock never fell below that.

Leaving the clock unused while missing tactics is the costly kind of caution. The comment on
`TimeParameters` (search.rs) claims Lichess games "showed sensible use of the clock" (2026-10-12);
these games contradict it.

## Why the clock settles high

Today, under Fischer (`movestogo` absent):

```
target = min((0.05 * T + 0.5 * I) * (1 + 2 * C) * F, 0.5 * T)
```

and the same `target` is both the plan and the hard deadline: no new depth starts once
`s * target` (`s = 0.5`) has elapsed, and the running depth aborts at `target`. A move therefore
ends between 50% and 100% of an already small target, usually near the low end. Measured in the
5+3 game (clock about 240 s): a stable target of 13.5 s, a depth cutoff at 6.75 s, and actual moves
of 3 to 7 s after the book.

The clock stops falling when a move spends the increment: roughly `0.5 * (0.05 * T + 0.5 * I) = I`,
so at `I = 3 s` the clock settles at `T` of about 60 to 90 s. **By construction, the last minute of
a blitz clock is never spent.** The increment's effective weight is about `0.3 * I`.

## What Stockfish does

Fetched from Stockfish master, 2026-11-11 (GPLv3, compatible with our AGPLv3).

`timeman.cpp`, `TimeManagement::init`: two times per move. Without `movestogo` it plans for 50
moves and counts their increments as available:

```cpp
int mtg = limits.movestogo ? std::min(limits.movestogo, 50) : 50;
TimePoint timeLeft = std::max(TimePoint(1), limits.time[us] + limits.inc[us] * (mtg - 1)
                     - moveOverhead * (2 + mtg));
```

```cpp
optimumTime = TimePoint(std::max(1.0, optScale * timeLeft));
maximumTime = TimePoint(std::max(double(optimumTime), std::min(0.8097 * limits.time[us] - moveOverhead,
                                                               maxScale * optimumTime)));
```

`optScale` is a few percent (it grows with the ply and is capped at `0.19404 * time / timeLeft`);
`maxScale` is at most 6.873.

`search.cpp`, `iterative_deepening`: after each depth the optimum is scaled and checked:

```cpp
double totalTime = mainThread->tm.optimum() * fallingEval * reduction
                 * bestMoveInstability * highBestMoveEffort;
```

```cpp
if (elapsedTime > std::min(totalTime, double(mainThread->tm.maximum())) || ...)
    threads.stop = true;
else
    threads.increaseDepth = mainThread->ponder || elapsedTime <= totalTime * 0.50;
```

Only `maximumTime` aborts a depth partway through (`check_time`). Half of `totalTime` only stops
the depth from being increased, never the search.

At the 5+3 game's middle game (T = 240 s, I = 3 s, about move 30), by our own arithmetic: optimum
about 21 s, a stable move about 8 s, an unstable one allowed up to the maximum of about 147 s. The
increment weighs about `1 * I` after the multipliers, and the clock settles near zero.

## The design

Keep our structure of factors (`C`, `F`), our round constants (the rule of 2026-10-12: reasonable
magnitudes, not tuned), and take Stockfish's two structural ideas.

1. **Count the future increments.** Under Fischer, plan from
   `L = max(0, T + (H - 1) * I - overhead * (H + 2))` with a horizon `H` of 50 moves, as Stockfish.
   Under `movestogo M`, `H = M` (at most 50), which leaves CCRL's 40/15 close to today.
2. **A soft time.** `soft = (L / H') * (1 + r * C) * F`, with `H'` a round divisor giving a stable
   move about the increment plus a small share of the clock. Iterative deepening starts no new depth
   once `soft` has elapsed; it is checked between depths only, so no running depth is wasted by it.
3. **A hard maximum.** `hard = min(k * soft_base, 0.8 * T - overhead)`, with `k` around 5. Only
   `hard` aborts a running depth. `C` and `F` change `soft` only, never `hard`, so the deadline
   inside the tree stays fixed for the move, as today.
4. **The single-move cap** (`SINGLE_MOVE_CAP_MS`) stays, applied to both times.
5. **`movetime`** keeps `TimeLimit::Fixed` and `FIXED_STOP_FRACTION`: out of scope.

Constants (settled, see Decisions): `H = 50`, `H' = 25` (a stable move about
`T / 25 + 2 * I` before factors, Stockfish-like after its stable multipliers of about 0.37),
`r = 2.0`, `k = 5`, hard cap `0.8 * T`.

Expected at T = 240 s, I = 3 s: `L` about 387 s, a stable soft time of about 15 s (with `F` at 0.6
when the score rises, about 9 s), an unstable one up to the hard maximum of about 75 s. The clock
then settles at a few times the increment instead of a minute.

## Decisions

- **Decided (2026-11-11): `H' = 25`, `k = 5`**, with `H = 50` and `r = 2.0` as proposed.
- **Decided (2026-11-11): the hard cap is `0.8 * T - overhead`**, Stockfish's 0.8097 rounded;
  `MAX_CLOCK_FRACTION` (0.5) goes.
- **Decided (2026-11-11): the `s` rule is dropped.** A new depth may start any time before
  `soft`; `hard` alone protects against a depth running too long. `TimeParameters::s` goes away
  under a clock (`FIXED_STOP_FRACTION` for `movetime` is out of scope and stays).
- **Decided (2026-11-11): a temporary UCI option** selects old or new time management in one
  binary; the arena registers the same engine twice with different options. The option and the old
  code are removed once the duel's verdict is in.

## Verification

- Unit tests in search.rs around the clock target (the `clock_target_*` tests) rewritten for the
  new formula: the stable soft time, the hard cap at low clock, `movestogo` near a refill, the
  single-move cap, and the clock never planned below zero.
- A duel of new against old at a blitz control (proposed `--time 180+2`, and `10+0.1` to check the
  short control does not regress), on one idle machine, enough games to tell a gain of tens of Elo.
- Then Lichess: the clock left at the end of blitz games should drop from about a minute to a few
  increments, with no losses on time.

The implementation plan follows in its own doc once these questions are settled.
