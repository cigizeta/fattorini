# The terminal as a log: events only, the page for everything else

Status: implemented 2026-10-23, to build and test; not compiled when written. Built as below, but:
the bars are kept, drawn nowhere (the developer, 2026-10-23), since they are what the page's live
bars read (`progress::register`, `progress::live`): `progress::log_events`, called first by
`smithy start` and `arena start`, sets up `env_logger` and makes every bar made after it undrawn.
Each round is one line, `round N took Xs: Ys train, Zs test; best lambda L, loss B; a new best for
lambda ...`; the round robin's and the gate's ends one line each. Gone: the redrawn round block,
the round robin's live standings, the gate's and the tests' text gauges, the tune's theta table, the
arena's chore bar (its chore is said once, when it arrives). Timestamps are UTC (`env_logger`'s own,
no local-time dependency).

## What

The smithy's page now shows all a job's state: training tables, the round robin, the gate's gauge,
tests, and every arena's chore, games and rate. The terminal duplicates it with live bars, tables and
text gauges, and every change to one must be repeated in the other (the "train" against "training"
fixes of 2026-10-23). The terminal becomes a plain log of events instead.

## Decisions (the developer, 2026-10-23)

1. **`log` with `env_logger`, to standard output only.** No log file, so no detach mode yet: the
   smithy stays attached to its terminal, the one record of its events. `env_logger` writes to
   stderr by default; its target is set to stdout. `RUST_LOG` is read, `info` when unset.
2. **Events only, no progress.** No bars, no gauges, no periodic progress lines: the page is the one
   place progress is watched.
3. **The arenas too:** their progress is on the smithy's page (the Arenas table: session, workers,
   chore, in play, done, rate, seen, commit), so their terminal logs events only.
4. **Only `smithy start` and `arena start` change.** The direction is that the page does everything,
   install included, and every other command is deprecated: left as it is, bars included, until the
   page replaces it and it is removed. Nothing marks it deprecated when run (a personal project).
   indicatif stays in the workspace for those commands until then.
5. **The bootstrap stays:** `bin/smithy` and `bin/arena` start the two servers before any page exists.

## The events

The smithy:

- started, listening on its port; stopped.
- a job started, stopped, ended; a phase entered (worded as the page words it: training, round
  robin, gate).
- a round done: its number, the best lambda and its loss, a new best marked, its time
  (`round 14 took 89.5s: 84.0s train, 5.5s test`); a stage changed; a lambda stopped by patience.
- the round robin finished, its winner; the gate's verdict.
- an arena claimed, lost, back; a machine unreachable.
- a test started, resumed, stopped, its verdict.
- errors and warnings, as they are now.

An arena:

- started, listening; claimed or released by a smithy.
- a chore received, finished, dropped by a new epoch.
- the smithy lost, back.
- errors and warnings.

## The work

- `env_logger` initialized at the start of `smithy start` and `arena start`, target stdout, `info`
  by default.
- Their prints become `info!` / `warn!` / `error!`; detail worth keeping only for debugging becomes
  `debug!`.
- Under those two commands, the live bars (`rounds_display::print_progress`, the round robin's, the
  gate's, the tests'), the per-round lambda table (`lockstep::round_block_lines`), the gate's text
  gauge (`gate_text`'s `llr_gauge`) and the arena's live display go. What only they used is
  removed; what a deprecated command still uses stays.
- The page is untouched.

## To check when implementing

- Which code paths `smithy start` and `arena start` share with deprecated commands, so a bar is not
  removed from under one of them.
- That nothing on the page reads text the terminal produced.
