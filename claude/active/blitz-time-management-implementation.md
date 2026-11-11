# Blitz time management: implementation plan

Implements blitz-time-management.md (design settled 2026-11-11). Status: steps 1 to 8 done (2026-11-11), `cargo test` and clippy passing. The binary is copied to
`data/duel-bin/fattorini-tm.exe` and registered as `fattorini-split` and `fattorini-classic` in
`data/engines.yaml`. Next: the 10+0.1 duel, then the 180+2 duel; results go here.

## The formula

Under a clock (`TimeLimit::Clock`), with `T` the clock, `I` the increment, `M` the moves to go
(`None` under Fischer), `o` the move overhead, `C` the instability and `F` the falling-score factor:

```
H     = min(M, 50), or 50 under Fischer
L     = max(0, T + (H - 1) * I - o * (H + 2))       // the time left for the next H moves
base  = L / min(H', H)                              // H' = 25
soft  = base * (1 + r * C) * F                      // r = 2.0
hard  = min(k * base, 0.8 * T - o)                  // k = 5
soft  = min(soft, hard)
```

- Under `movestogo`, the divisor is `min(H', H)`: with fewer than 25 moves to go, the clock is
  shared among the moves actually left (`M = 1` gets all of `L`, then `hard` caps it at `0.8 * T`).
  The duel and Lichess are Fischer; this only has to stay sane for CCRL's 40/15, which has no
  increment: at `M = 40` a stable move plans `T / 25`, against today's target of `0.075 * T`
  (`T / 13`) halved by the `s` rule. The unit tests pin it.
- `o` is applied once, here, in place of `time_limit`'s `move_overhead_ms * reserves`: `time_limit`
  passes `T` and `o` through instead of subtracting.
- The single-move cap (`SINGLE_MOVE_CAP_MS`) lowers both `soft` and `hard`.

## Steps

1. **`TimeLimit::Clock` carries the overhead.** A field `overhead_ms: u64`; `time_limit` stores
   `move_overhead_ms` there and leaves `time_ms` as given. Every constructor and test of `Clock`
   updated (search.rs, in_process.rs). `TimeLimit::Fixed` unchanged.
2. **A `TimeManagement` selector, temporary.** `pub enum TimeManagement { Classic, Split }`, a field
   `management` on `TimeLimit::Clock`, `time_limit` taking it as a parameter. `Classic` is today's
   code, untouched. The doc comment says it exists only until the duel's verdict.
3. **The new formula.** `fn split_times_ms(time_ms, inc_ms, moves_to_go, overhead_ms, instability,
   falling) -> (soft, hard)` beside `clock_target_ms`, with its constants in a
   `SplitTimeParameters { horizon: 50, divisor: 25.0, r: 2.0, k: 5.0, hard_fraction: 0.8 }` whose doc
   comments carry the why (the Stockfish quotes of the design doc, fetched 2026-11-11, and the
   2026-11-11 Lichess games), as `TimeParameters` does today.
4. **`TimeManager` under `Split`.** `should_start_next`: `elapsed < soft`, no `s` fraction.
   `deadline`: `started + hard`, fixed for the move: `hard` depends on neither `C` nor `F`, so the
   deadline inside the tree never moves, as the existing doc comment requires. Under `Classic` and
   `Fixed`, both behave as today. `may_start_next_depth` and the ponder handling need no change.
5. **The UCI option.** `option name TimeManagement type combo default Split var Split var Classic`
   in uci.rs, beside `Move Overhead`: a parse arm, a test, a field on the engine passed to
   `time_limit`. Default `Split`, so the duel's new side needs no option and lichess-bot needs no
   change after the verdict.
6. **The arena's in-process player** (in_process.rs) passes `TimeManagement::Split`. The duel itself
   runs over UCI (below), so the in-process player needs no switch.
7. **Unit tests** (search.rs), replacing nothing of `Classic`'s until it is removed:
   - stable soft time at 180+2 and at 240+3, by hand from the formula;
   - `C = 1` triples `soft` up to `hard`; `F = 1.7` raises it, still under `hard`;
   - `hard` at a low clock is `0.8 * T - o`, and `soft <= hard` always;
   - `movestogo 1` spends at most `hard`, `movestogo 40` is pinned;
   - the single-move cap lowers both;
   - a clock below the overhead gives 0 for both (depth 1 still answers, as today);
   - a `TimeManager` under `Split` starts a new depth until `soft` and its deadline is `hard`.
8. **`cargo test`** and `cargo clippy`, both clean.

## The duel

Over UCI, so both sides pay the protocol latency Lichess does. The developer copies the release
binary (built by `bin/fattorini`) out of `target/`, so a later build cannot change it mid-run, and
registers it twice in the gitignored `data/engines.yaml`:

```yaml
  fattorini-split:
    path: "<the copied fattorini.exe>"
  fattorini-classic:
    path: "<the copied fattorini.exe>"
    options:
      TimeManagement: Classic
```

Then, on one idle machine, plugged in, nothing else running:

```
bin/arena duel fattorini-split fattorini-classic --time 180+2
```

```
bin/arena duel fattorini-split fattorini-classic --time 10+0.1
```

- **Ships** if 180+2 is clearly positive and 10+0.1 does not regress beyond its error bar.
- Games per duel: as many as the arena's default run, extended if the error bar still spans zero.
- The arena-side check that two entries may share one executable is done before the run; if the
  arena refuses it, two copies of the binary.

## After the verdict

- **It ships:** remove `Classic`, the selector, the option, `TimeParameters`, `clock_target_ms`,
  `MAX_CLOCK_FRACTION` and their tests; correct the comments that cite them (search.rs:268,
  search.rs:3117); a release for Lichess; watch the clock left at the end of blitz games.
- **It does not:** record the result in the design doc, remove `Split`, and archive both docs.

Both docs move to `done/` once Lichess shows the clock used with no losses on time.
