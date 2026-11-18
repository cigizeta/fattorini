# Reckless's time management alone

Status: **implemented 2026-11-13**; `cargo test --workspace --release` and clippy clean; smoke
tests through `bin/arena match` passed the same day, no forfeit: 40/20 (18 games, 3 min, the
clock refilled) and 10+0.1 (20 games, 82 s). **Lichess, 2026-11-13:** the `@tuned` snapshot of
this build taken; lichess-bot's `config.yml` pointed at it, `move_overhead` 2000 down to 385, and
its `SyzygyPath` fixed: it named a tree of the project's old name that no longer exists, so a bot
run from it played with no tables, and its engine was an old pre-rename binary outside the
repository. Whether that file was the one the bot ran is to check in lichess-bot's log; if it
was, the past week's Lichess games (the 2612, the +1.59 in KQ vs KN) were not the build TODO.md
says. Left: every arena updated, the first Lichess games read, the cutechess-cli check at the
next release. As built,
three departures from the plan below:

- **`TimeLimit::Fixed` and `SINGLE_MOVE_CAP_MS` stay,** narrowed: `Fixed` is made only by
  `single_move_limit`, a forced move under a clock or `movetime` at 0 ms (depth 1), and a forced
  move under `go depth` (the arena's fixed-depth games) still capped at 500 ms beside the depth.
  `TimeLimit::Clock` lost its `single_move` field.
- **Names:** `TimeParameters`, `TIME_PARAMETERS`, `time_bounds_ms`, `EXTRA_MARGIN_MS`,
  `TimeFactors`, `TimeCounters`, `TimeManager::{bounds, factors, multiplier}`; the credit to
  Reckless in `TimeParameters`' doc comment.
- **The old tests' lines (5 to 8)** read back with both sides the same player, their managements
  dropped: still listed, but Resume on 5 to 7 would duel a player against itself. Not to be
  resumed.

README's option table says 100; `RELEASE-NOTES.md` stays as released until the next release.
**Every arena must run this build before the next gate or tune:** an arena of an older build reads
a chore without the management field as `Split`, and would play the old management unnoticed
(the page shows each arena's commit).

## Why

Test 8 (reckless-time-management.md): Reckless's hand-picked set beat Classic at 180+2, 53.4% over
654 games, and its clock profile ends the Lichess complaint of a minute left unused. Split's own
180+2 duel was suspended undecided (blitz-time-management-v2.md); it beat neither. Reckless ships
untuned; its SPSA comes later, after the search's, as an improvement measured on its own.

The `TimeManagement` enum and its UCI option were temporary from the start, "removed with this enum
once the duels decide" (search.rs).

## What goes

- **`TimeManagement`**, its three variants, `name` and `parse`; the `TimeManagement` UCI option, its
  `setoption`, its line in the `uci` reply, and its tests.
- **Classic:** `clock_target_ms`, `TimeParameters`, `MAX_CLOCK_FRACTION`, its "no depth past half"
  rule, and `TimeLimit::Fixed` with them (Reckless's `Movetime` replaces it).
- **Split:** `split_times_ms`, `SplitTimeParameters`, `SPLIT_TIME_PARAMETERS`, `TimeManager::split_times`,
  `falling_score_factor` unless Reckless's trend reads it, and the `split_*` tests.
- **`SINGLE_MOVE_CAP_MS`** and `single_move_limit`: Reckless's forced move searches depth 1 alone.
- **`DEFAULT_MOVE_OVERHEAD_MS` (20)** and `default_move_overhead_ms`: `Move Overhead` defaults to
  100, and `move_overhead_ms` goes back from `Option<u64>` to `u64`.
- **`TimeLimit::Clock`'s `management` field.**

## What changes

1. **search.rs:** one time manager, the Reckless path without its `match`. `TimeLimit` keeps
   `Clock` and `Movetime`. The doc comments credit Reckless as now (its structure, its constants as
   starting values), and the stale ones go: "no safety margin is ever subtracted" is false under
   Reckless.
2. **uci.rs:** the option and its command gone; `Move Overhead` advertised with default 100, as
   already. An old GUI or lichess-bot config still sending `setoption name TimeManagement` gets
   the usual unknown-option treatment (ignored), so no config breaks.
3. **The wire format (protocol.rs `Player`):** the third field, the management, is no longer
   written. Read back, any management named is accepted and ignored, so a chore from an arena or
   smithy one commit behind still parses. The tests changed to say so.
4. **The arena (in_process.rs, pairing.rs, chores.rs):** `InProcessPlayer::new` loses its
   `TimeManagement` argument; every in-process game, tune or gate, plays Reckless. This is the
   change of substance for the forge: **gates and tunes until now played Split**, so from here
   their games are timed by Reckless. The fullmove it needs is already passed (step 2 of
   reckless-time-management.md).
5. **The smithy (matches.rs, smithy.rs, tune.rs, page.html):** a test side loses its management.
   `Side::parse` still reads `@classic`, `@split` and `@reckless` and drops them, so the kept tests'
   lines (5 to 8) still show; `Side::text` never writes one. The page's per-side "time" select goes.
   `smithy.rs:4557`'s check (a non-Split side refused somewhere) goes with it.
6. **Names:** the `reckless_` prefix kept where it names the origin of the constants
   (`RecklessTimeParameters`, `reckless_bounds_ms`), or plain names (`TimeParameters`,
   `time_bounds_ms`) with the credit in their doc comment. **Decided (the developer, 2026-11-13):
   plain names**, the credit kept in the doc comments.
7. **`tm-probe`** stays: it measures the factors the tune will move.

## Checks

1. `cargo test --workspace --release` and `cargo clippy --workspace --all-targets`, both clean.
2. A smoke test at a repeating control (40/20) and at 10+0.1 through `bin/arena`, the default build,
   no option set: no forfeit. Then a smoke test through cutechess-cli with no `Move Overhead` set,
   as a CCRL tester runs it.
3. No duel: the binary is the one test 8 measured, less dead code.

## Lichess, the same session

1. A new `@tuned` snapshot of the commit, in Drive's `data/snapshots/`.
2. lichess-bot pointed at it, its `move_overhead` 500 down to **385** (500 - 100 - 15: the total kept
   back stays 500 ms, the engine now keeping 100 + 15). Only together: on the old snapshot, 385
   would leave 405 in all.
3. If lichess-bot's config sets the engine's `Move Overhead` among its UCI options, that replaces
   the 100; the sum is then 385 + that + 15. Check, and remove it or count it.
4. Its first games read for flags and for the clock left at the end.

## Afterwards

- TODO.md: "Clock safety at default settings" rewritten for Reckless (its `MAX_CLOCK_FRACTION`
  points are Classic's) or dropped; the `Control::Fischer` rename unaffected.
- reckless-time-management.md, blitz-time-management*.md and smithy-time-management-tests.md to
  `done/` or `archived/` as their state says, Split's v2 docs "superseded, see reckless-only.md".
- The next release's notes say the time management changed, crediting Reckless.
