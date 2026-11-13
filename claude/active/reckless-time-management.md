# Reckless time management

Status: **design settled** with the developer 2026-11-13. Steps 1 to 9 done the same day
(search.rs, uci.rs): `cargo test -p fattorini --release` and `cargo clippy --workspace
--all-targets` clean. Step 10 done 2026-11-13 at 40/20 against Classic
(`data/matches/fattorini-reckless-vs-fattorini-classic-ref/20261113-0833/`), once the arena could
play repeating controls (arena-repeating-controls.md), stopped by hand at 23 pairs with a
`result.txt` of its own so the pairing's next run starts a new folder: no forfeit in 48 games; the clock refilled
after each 40th move; Reckless spends about the whole period (about 19 of 20 s over its first 40
moves, longer thinks of 2.5 s on hard moves, 0.1 to 0.2 s in the period's last moves), as its
cyclic bounds (1.0 and 5.0 shares) make it; Classic likewise. 

**Step 11, the 10+0.1 duel against Classic: won by Reckless, 2026-11-13.** +15 -3 =14 for
Reckless, 22.0 - 10.0, S 0.6875, resolved at 32 games in 2 minutes
(`data/matches/fattorini-reckless-vs-fattorini-classic-ref/20261113-0841/`). Classic, not Split,
as the opponent: Split's 180+2 duel is suspended undecided (blitz-time-management-v2.md). No
forfeit; every decisive game a checkmate. About +136 Elo taken at face value, an upper estimate: a
duel stopped at its first crossing overstates the gap. In a game read, Reckless spends more in the
middlegame (0.8 to 0.9 s a move around move 10 against Classic's 0.4 to 0.7 s) and on hard
endgame moves (1.0 to 1.4 s), while Classic stays at 0.1 to 0.3 s for most of the game: its
"no depth past half the target" keeps a large reserve unspent.

**Next: a 180+2 duel of untuned Reckless against Classic (the developer, 2026-11-13),** before the
tune, at the control that matters for Lichess; if Reckless wins it, it can ship untuned and the
tune becomes an improvement measured later. The tune needs groundwork first anyway: the arena's
in-process engine plays `Split` hard-coded, and Reckless's constants are not yet tunable
parameters. Checked beside the score, from the duel's `games.pgn` move times: the clock each side
has left at moves 20, 40 and 60. The Lichess pain (the developer, 2026-11-13) is unused time: a
Reckless that ends games with seconds left rather than a minute fixes it. Two departures from
the Steps as planned: the arena's in-process engine is left alone, since it plays `Split`
hard-coded and never reads `fullmove`; and the `uci` reply advertises the selected management's
own default overhead (20 under the default `Split`), not 100, so it never states a default the
engine does not use.

## Why

The developer, 2026-11-13, while the 180+2 duel of Split v2 against Classic runs: a complete
redesign of time management, modelled on Reckless's. Its shape (two base bounds, then a soft bound
scaled after each depth by a product of clamped linear factors) is the one time-management-from-scratch.md
was heading for, and every one of its constants is a single, SPSA-tunable number.

## Source

Read 2026-11-13 from Reckless's `main` branch, files `src/time.rs`, `src/search.rs`, `src/uci.rs`,
`src/thread.rs`, `src/threadpool.rs` (no commit hash visible in the raw files). Every formula and
constant below matches that code. The first draft's notes are in
reckless-time-management-superseded.md.

## What is taken, what is changed

| Reckless | Fattorini | Why |
|---|---|---|
| Soft-stop vote, 65% of threads | dropped | under Fattorini's Lazy SMP, thread 0 alone makes every time decision and broadcasts the stop, as today |
| Thread-vote choice of the final move | dropped | not time management; `select_best_result` stays |
| `MoveOverhead` default 100 ms, plus a fixed 15 ms | the same: `Move Overhead` default 100 ms under the Reckless variant (Classic keeps 20), plus `EXTRA_MARGIN_MS = 15` | Reckless's constants are followed (the developer, 2026-11-13) |
| Scores in Reckless's centipawns (12, 0.048 per cp) | scaled by `pawn_percent` | as `falling_score_factor` already does, so one pawn buys the same time under every evaluator |
| `previous_best_score` 0 on a new game | `None`: score trend 1.0 | as `falling_score_factor` |
| Decisive scores enter the trend | trend 1.0 on a decisive score | as `falling_score_factor` |
| Hard check every 2048 nodes | Fattorini's existing check interval | not a time-management choice |
| Constants | Reckless's, as starting values | to be re-tuned by SPSA for Fattorini's search |

## Specification

All times in milliseconds, `f64` where fractional. `T`, `I`: own `wtime`/`btime`, `winc`/`binc`.
`mtg`: `movestogo`. `fullmove`: the FEN's field 6 of the root position, advanced by the moves
played since (one per Black move).

### Constants

Two kinds (the developer, 2026-11-13). **Fixed:** Reckless's hand-picked constants, recognisable
by being round, followed as they are and never tuned. **Tuned:** Reckless's SPSA output, the
starting values of Fattorini's own SPSA, run after the search's.

```
// fixed: hand-picked by Reckless
MOVE_OVERHEAD_MS    = 100        // the option's default
EXTRA_MARGIN_MS     = 15
INC_FRACTION        = 0.75       // share of I added to both bounds
CYCLIC_SOFT         = 1.0        // times the even share; never exercised by a Fischer tune
CYCLIC_HARD         = 5.0
EVAL_WINDOW_CP      = 12         // since its introduction (PR #145), untouched by every SPSA
                                 // session checked

// tuned: Reckless's SPSA output as starting values
SOFT_MAX            = 0.0594     // soft fraction of T, approached late in the game
SOFT_RISE           = 0.0492     // how far below SOFT_MAX it starts
SOFT_RATE           = 0.0386     // how fast, per full move
HARD_FRACTION       = 0.7281
NODES_A = 3.1838   NODES_B = 2.6554   NODES_MIN = 0.5460
TREND_A = 0.7426   TREND_B = 0.0480   TREND_MIN = 0.7214   TREND_MAX = 1.4031
PV_A    = 1.2881   PV_B    = 0.0440   PV_MIN    = 0.7160
EVAL_A  = 1.2664   EVAL_B  = 0.0416   EVAL_MIN  = 0.8642
CHANGES_A = 1.1500 CHANGES_B = 0.2526   // both from SPSA session #1072, as most of this group
```

The split was checked against Reckless's history (2026-11-13): the tuned group's current values
come from its "SMP SPSA session 200k games (#1072)" or earlier sessions. The other fixed constants
were classed by roundness alone.

### Base bounds, once per `go`

```
fn base_bounds(limits, fullmove, overhead) -> (soft, hard):
    match limits:
        movetime(ms):
            soft = hard = ms
        clock(T, I, mtg = none):                         // Fischer
            avail = max(T - overhead, 0)
            scale = SOFT_MAX - SOFT_RISE * exp(-SOFT_RATE * fullmove)
            soft  = min(scale * avail + INC_FRACTION * I, avail)
            hard  = min(HARD_FRACTION * avail + INC_FRACTION * I, avail)
        clock(T, I, mtg = some(m)), m >= 1:              // repeating
            avail = max(T - overhead, 0)
            base  = avail / m + INC_FRACTION * I
            soft  = min(CYCLIC_SOFT * base, avail + I)
            hard  = min(CYCLIC_HARD * base, avail + I)
        depth, nodes, infinite, no clock:
            return (none, none)                          // no time management
    soft = max(soft - EXTRA_MARGIN_MS, 0)
    hard = max(hard - EXTRA_MARGIN_MS, 0)
    return (soft, hard)
```

Under `movetime`, Reckless's way (the developer, 2026-11-13), under the Reckless variant only: no
move overhead kept back, `hard = ms - EXTRA_MARGIN_MS`, and a new depth starts while the elapsed time
is under `ms` itself, unscaled (the multiplier applies only to clock limits). Classic and Split
keep today's `movetime` (the overhead kept back, no new depth past half).

### State kept during one search

```
previous_score : Option<i32>   // final best score of this engine's previous go in the game,
                               // from its own side; None on the first move or after ucinewgame;
                               // never written by a ponder search
average        : i32           // running average of the root score, seeded with previous_score
                               // (0 if None)
eval_stable    : u32 = 0       // depths in a row with the score near `average`
pv_stable      : u32 = 0       // depths in a row with the same best move
last_best      : Option<Move> = None
changes        : u32           // best-move changes within the current depth
root_nodes[m]  : u64           // nodes spent under each root move, summed over all depths
total_nodes    : u64           // nodes of the whole search
```

### Inside each depth

```
at the start of each depth:
    changes = 0

in the root move loop, after searching root move m (index k, 1-based, in this iteration):
    root_nodes[m] += nodes spent under m
    if score > alpha or k == 1:
        m becomes best for now
        if k > 1:
            changes += 1                 // counts every new leader, aspiration re-searches included

after each aspiration search returns score s for the best move:
    average = (average + s) / 2
```

### After each completed depth

A depth counts as completed only if the hard bound did not abort it.

```
best, score = best move and score of the completed depth

if |score - average| < EVAL_WINDOW_CP * pawn_percent / 100:  eval_stable += 1
else:                                                       eval_stable  = 0

if last_best == some(best):  pv_stable += 1
else:                        pv_stable  = 0
last_best = some(best)

fn multiplier() -> f64:
    f      = root_nodes[best] / total_nodes
    nodes  = max(NODES_A - NODES_B * f, NODES_MIN)

    trend  = if previous_score is none or decisive(previous_score) or decisive(score):
                 1.0
             else:
                 d = (previous_score - score) * 100 / pawn_percent  // in Reckless's centipawns
                 clamp(TREND_A + TREND_B * d, TREND_MIN, TREND_MAX)

    pv     = max(PV_A   - PV_B   * pv_stable,   PV_MIN)
    eval   = max(EVAL_A - EVAL_B * eval_stable, EVAL_MIN)
    churn  = CHANGES_A + ln(1 + CHANGES_B * changes)

    return nodes * trend * pv * eval * churn

if clock limit and elapsed >= soft * multiplier():  stop
if movetime    and elapsed >= ms:                   stop   // the raw movetime, not soft
```

Note `trend` has a floor of `TREND_A = 0.74` when the score is unchanged, not 1.0: Reckless's
neutral point is a drop of about 5 cp. The fallback to 1.0 above (no previous score, decisive
score) is Fattorini's and is a deliberate exception.

### During search

```
every check interval (Fattorini's existing node-count check):
    if completed_depth >= 1 and hard is some and elapsed >= hard:
        abort the search; play the best move of the last completed depth
```

### At the end of `go`

```
previous_score = score of the move played    // unless this was a ponder search
```

## Steps of phase 1

Code read 2026-11-13: `TimeLimit`, `TimeManagement`, `time_limit`, `TimeManager`,
`choose_best_move_with_progress` and its MultiPV twin, `search_root_pondering` (search.rs);
`set_position` and `last_score` (uci.rs).

1. **`TimeManagement::Reckless`,** a third variant of the temporary enum and of the UCI option, so
   one binary can duel it. `time_limit` passes the clock as given under it, as under `Split`, the
   overhead beside it. A forced move under it (`single_move`) starts no depth past 1.
   `movetime` under it as in "Base bounds": `time_limit` returns `Fixed` before it reads the
   variant today, so the variant must reach the `Fixed` case too.
   **The overhead per variant:** the engine's `move_overhead_ms` becomes `Option<u64>`, `None`
   until a `setoption` sets it; `None` resolves to `DEFAULT_MOVE_OVERHEAD_MS` (20) under Classic
   and Split, to `RECKLESS_MOVE_OVERHEAD_MS` (100) under Reckless. The `uci` reply advertises 100,
   the shipping default. The arena's in-process engine (in_process.rs) resolves it the same way.
2. **`fullmove` reaches `TimeLimit::Clock`:** `set_position` keeps `Board::from_fen`'s second value
   (discarded today) and adds one per Black move played; `ucinewgame` resets it with the board. The
   arena's in-process engine (in_process.rs) passes its own, from game.rs's `fullmove_number`.
3. **`reckless_bounds_ms(time, inc, moves_to_go, fullmove, overhead) -> (soft, hard)`,** the one
   place the base formula lives, with the constants of "Constants" in a `RecklessTimeParameters`
   struct, its doc comment crediting Reckless (decided above).
4. **Search counters, at `ply == 0` in `alpha_beta_with_buffer`:**
   - `root_nodes`: `self.nodes` before and after each root move, added to that move's entry.
   - `best_move_changes`: `+1` when a move other than the first legal one searched at this root
     call scores above `alpha`, before `alpha` rises. A fresh `Search` per depth resets it; every
     aspiration attempt of the depth adds to it.
   Both returned in `RootResult`. `choose_best_move_with_progress` sums `root_nodes` over the
   depths; the nodes total is the one it already keeps.
5. **`average`,** in `search_root_pondering`'s aspiration loop: `(average + attempt) / 2` after
   every attempt that was not stopped; seeded from the previous move's score (0 without one),
   carried across depths, returned in `RootResult`.
6. **`TimeManager` under `Reckless`:** `eval_stable` and `pv_stable` updated in
   `record_iteration`, which now also takes the depth's `root_nodes`, nodes, changes and `average`;
   `should_start_next` is `elapsed < soft * multiplier()`; `deadline` is `started + hard`, fixed
   for the move. Classic's and Split's paths untouched.
7. **MultiPV:** rank 1's search alone feeds the counters; the extra ranks' nodes stay in the total.
8. **Unit tests:** the bounds by hand at 10+0.1 and 180+2 (fullmove 1, 20, 60), 40/15 with 40 and
   1 to go, a clock smaller than the overhead (both 0); each factor at its floor, its neutral point
   and its ceiling; the trend 1.0 without a previous score or with a decisive one; `deadline`
   unchanged across `record_iteration`.
9. **`cargo test` and `cargo clippy`,** both clean.
10. **Smoke test** at a short repeating control: no game flags.
11. **The 10+0.1 duel** against the incumbent, the arena's per-move clock read for how the time is
    spent.


## Integration

- Whichever variant loses is removed afterwards, with the option.
- Ponder: bounds are computed at `go ponder` as for any `go`, and the clock starts at `ponderhit`,
  as today.

## Decided

- **This document supersedes time-management-from-scratch.md (the developer, 2026-11-13),** now
  archived.
- **All five factors from the start (the developer, 2026-11-13):** Reckless's constants were
  tuned with the five together, so a subset would be an untuned combination. If the whole loses,
  factors are removed one at a time to find the culprit.
- **Reckless's constants are followed (the developer, 2026-11-13),** the 100 ms overhead and the
  15 ms margin included; the 100 ms as the Reckless variant's own default (below).
- **Reckless's hand-picked constants are followed; the rest is tuned, after the search's SPSA
  (the developer, 2026-11-13).** The split is in "Constants". Until that tune, the tuned kind
  holds Reckless's values. The tune follows TODO.md's time-management SPSA: 10+0.1 scaled by the
  bench, every idle arena, then a 180+2 duel of tuned against untuned.
- `CYCLIC_SOFT` and `CYCLIC_HARD` are checked by a smoke test at a repeating control; the Fischer
  tune never exercises them anyway.

- **The tune's budget stays 32k pairs (the developer, 2026-11-13),** about 12 hours over every
  arena, though sized for 4 parameters and now covering 18: noisier values accepted.

- **Order (the developer, 2026-11-13):**
  1. **A feeling for Reckless's values:** a smoke test at a repeating control (no game flags),
     then a 10+0.1 duel against the incumbent, and the arena's per-move clock read for how the time
     is spent. Not a ship decision.
  2. **Starting values for the tune,** chosen from what step 1 shows: Reckless's, or adjusted where
     the clock use looks wrong for Fattorini's search.
  3. **The tune** (32k pairs at 10+0.1), then **the verdict** at 180+2, tuned against the
     incumbent.

- **Inspired, not copied (the developer, 2026-11-13):** Fattorini's code is written from this
  specification, never from Reckless's source text. The code credits the idea and the constants'
  origin, e.g. "modelled on Reckless's time management (codedeliveryservice/Reckless): its
  structure, and its constants as starting values". Both projects are AGPL v3.
- **A forced move searches depth 1 only (the developer, 2026-11-13),** in place of the 500 ms
  cap (`SINGLE_MOVE_CAP_MS`) and of Reckless's uncapped search (about a third of a normal move,
  seconds at 180+2, about 7 s at 40/15). No clock spent; the score is shallow and is used as it
  is, by the next move's trend and by the arena's adjudicator alike. The ponder move comes from the
  transposition table (`ponder_move`), since a depth-1 PV holds the forced move alone. **Under
  the Reckless variant only:** Classic and Split keep the 500 ms cap, which goes with them once
  Reckless wins, so the duel changes time management alone.
- **Each variant its own default overhead (the developer, 2026-11-13):** Classic keeps 20 ms,
  untouched, as the reference it is kept for; Reckless 100 ms, as it would ship. A `Move Overhead`
  a GUI sets explicitly applies to every variant. The duels set none, so each side plays at its
  own default, and they measure Reckless as shipped against Classic as it is. Classic is to
  disappear; the 20 ms default goes with it.
- **The verdict (the developer, 2026-11-13):** the 180+2 duel, capped at 800 games, runs first;
  the decision is taken on its result, with no rule fixed beforehand. The incumbent is whatever wins Classic against Split v2.
