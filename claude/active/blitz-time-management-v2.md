# Blitz time management, Split v2: design and implementation plan

Revises blitz-time-management.md after its implementation (blitz-time-management-implementation.md)
lost the 10+0.1 duel to `Classic` on 2026-11-11: +26 -52 =58 for Split, S 0.4044, about -67 Elo,
resolved at 136 games (`data/matches/fattorini-split-vs-fattorini-classic/20261111-1625/`).
Status: design settled with the developer 2026-11-11; steps 1 to 5 done the same day, and step 6's 10+0.1 duel.

**10+0.1 duel (2026-11-11): passed, drawn.** +209 -196 =347 for Split, 382.5 - 369.5 over 752
games, S 0.5086 (about +6 Elo, within noise), curtailed at 376 pairs since neither could pass its
bound by 400 (`data/matches/fattorini-split-vs-fattorini-classic/20261111-1640/`). v1's -67 Elo is
gone: no regression at the short control. Next: the 180+2 duel, the verdict.

## Why v1 lost

Two causes, compounding:

1. **The divisor was half the horizon.** `L` counted 49 future increments but was divided by 25,
   so a stable move planned about twice an even share (555 ms at 10+0.1). Stockfish's optimum is
   about 1.2 to 2% of its `timeLeft`, `L / 50` to `L / 80`.
2. **Classic's `s` rule was dropped.** v1 started a new depth until `soft`; a depth costs about as
   much as all the shallower ones together, so the last one started ran on to 1.5 to 2 times
   `soft`. Classic starts none past half its target, so it spends 0.5 to 1 times it.

Seen in the duel's PGN: Split spent 0.5 to 2 s a move in the opening and middlegame, then from about
move 25 the increment alone (0.1 s), while Classic still spent 0.2 to 0.4 s.

## The constraint

Where a Fischer clock settles depends on the ratio of real spend to plan, not on the plan alone.
`L / 50` with the `s` rule settles near 18 increments, where Classic already does (about 36 s at
3+2, the clock Lichess saw unused); without it, spend exceeds the increment at every clock and
drains to the hard cap. Counting the future increments only helps with an explicit settle point.

## The formula

Under a clock, with `T` the clock, `I` the increment, `M` the moves to go (`None` under Fischer),
`o` the move overhead, `C` the instability and `F` the falling-score factor:

```
H     = min(M, 50), or 50 under Fischer
R     = max(n * I, floor) + o * (H + 2)      // n = 5, floor = 300 ms
L     = max(0, T + (H - 1) * I - R)
plan  = L / H
soft  = s * plan * (1 + r * C) * F           // s = 0.5, r = 2.0
hard  = min(k * plan, 0.5 * T - o)           // k = 3
soft  = min(soft, hard)
```

A new depth starts while `soft` has not elapsed; `hard` aborts the running one and depends on
neither `C` nor `F`, so the deadline inside the tree stays fixed for the move. Real spend averages
about `0.75 * plan`, falling as the clock nears `R`, so the clock settles near five increments.

Decisions (the developer, 2026-11-11, one at a time):

- **Settle point: five increments** (`n = 5`), about 10 s at 3+2 and 0.5 s at 10+0.1.
- **Floor: 300 ms**, so a control with no or a tiny increment keeps a margin (CCRL's 40/15).
- **The slower start is accepted:** at 180+2 the first move starts no depth past 2.7 s against
  Classic's 5.5 s, spending more later instead. `s = 0.5`, `H = 50`.
- **Gate: 10+0.1, then 180+2,** as in v1.
- **The verdict at 180+2, decided before it runs** (the developer, 2026-11-11): Split ships if
  the duel resolves for it, or reaches its 800-game cap undecided with Split ahead (S above 0.5);
  Classic stays if it resolves for Classic, or the cap is reached with Split at or below 0.5. Why:
  longer controls compress Elo differences, so "clearly positive" may be out of reach in 800 games,
  while Split's purpose is the unused clock seen on Lichess, the duels only guarding against
  playing worse.

Other constants, chosen here: `k = 3` (an unstable move with a falling score, `3 * 1.7` times
`s * plan`, still fits under `3 * plan`); the cap `0.5 * T - o`, Classic's `MAX_CLOCK_FRACTION`, in
place of v1's 0.8, since this duel showed overspending, not underspending, is the risk at short
controls.

| | 10+0.1, move 1 | 180+2, move 1 |
|---|---|---|
| Classic, no depth started past | 275 ms | 5.5 s |
| Split v1, soft (spent about) | 555 ms (about 1 s) | 11 s (about 20 s) |
| Split v2, soft, stable move | about 135 ms | about 2.7 s |

## Steps

1. **`SplitTimeParameters`** (search.rs): `horizon: 50`, `r: 2.0`, `s: 0.5`, `k: 3.0`,
   `hard_fraction: 0.5`, `reserve_increments: 5.0`, `reserve_floor_ms: 300`; `divisor` removed. Doc
   comments carry the why above, citing v1's duel result, not this document.
2. **`split_times_ms`** computes the formula above; one place only, as today.
3. **`TimeManager` under `Split`** unchanged: `should_start_next` is `elapsed < soft`, the `s` rule
   now living inside `soft`; `deadline` is `started + hard`.
4. **Unit tests** rewritten for v2: stable `soft` and `hard` at 10+0.1, 180+2 and 240+3 by hand;
   the reserve's floor with no increment; `movestogo 1` and `movestogo 40` pinned; `C = 1` and
   `F = 1.7` still under `hard`; the single-move cap; a clock below the reserve gives 0 for both.
5. **`cargo test`** and `cargo clippy`, both clean.
6. **The duel binary** copied again to `data/duel-bin/fattorini-tm.exe` (the entries in
   `data/engines.yaml` stay), then `bin/arena duel fattorini-split fattorini-classic --time 10+0.1`
   on one idle machine; only if it does not regress beyond its error bar, `--time 180+2`.

## After the verdict

As in blitz-time-management-implementation.md: if it ships, `Classic` and the selector go; if not,
`Split` goes and the three documents are archived.
