# The smithy's page in three screens: Home, Config, Run

**Status: design agreed with the developer, 2026-10-23; plan below, being implemented.**

## Why

A config's page held its runs, the job's live phases, the phases kept after the job ended, and a
View of any attempt, all on one screen: the same attempt could show twice (the kept phases and a
View of it), and a View of another run could sit beside a running job. The developer, 2026-10-23:
clicking a config box shows its runs; clicking to run or to view goes to a third screen, with its
own Back. While a job runs, no other run is viewed.

## Decisions (the developer, 2026-10-23)

1. **Three screens, each its own address:** Home `#`, Config `#<config>`, Run `#<config>/<run>`.
   F5, the browser's Back and a bookmark keep working. Back goes up one level: Run to its Config,
   Config to Home.
2. **The Run screen** shows that run alone. While its job runs: Running now with Stop, the
   Progress block, the live phases. Otherwise: the View, its generation and attempt choices,
   opened on the latest attempt (the one a job just ended on), with Resume. The phases kept after a
   job ends are gone: the View shows them, in one place only.
3. **New run and Resume** go straight to the Run screen of the run they start.
4. **On the Config screen during a job:** the running run's row says `running`, its button Open;
   every other View is hidden. The View of the running run's screen is its live phases.
5. **No jump from Home** to the Run screen: the running config's box opens its Config screen, as
   every box does.
6. **Lc0 and PeSTO keep one screen** (their corpus box). **PeSTO's tune** gets a Run screen,
   `#pesto/tune`: a job with its own work, as a forge.
7. **Only the smithy's own job** shows as running: one smithy at a time is the developer's
   responsibility, as for claims.

## Plan

1. **Routing:** `route()` parses `#<config>[/<run>]` into `current` and `currentRun`; Back sets the
   hash one level up.
2. **Config screen:** the runs list and New run only; each row Open (the running run), or View,
   Resume (no job running), both to `#<config>/<run>`. The job section, the live and phase sections
   leave it.
3. **Run screen:** the job section and the Progress block when the job is this run's; else the View,
   on the latest attempt, with Resume. New run and Resume set the hash to the run's screen; New
   run's name is known once the job starts (`state.job`), so the page goes there on the poll that
   shows it.
4. **PeSTO's tune:** the Tune row's Start goes to `#pesto/tune`, its work drawn there.
5. **Checks:** a forge started from New run lands on its Run screen; Stop, then the View on the
   attempt it ended on; Resume from there; F5 on each screen; the browser's Back; a tune.
