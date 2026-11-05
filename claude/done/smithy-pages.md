# The smithy's page: a box per config

**Status: plan and implementation, 2026-10-22, from the developer's redesign of the page after
configs-with-tunes.md ("try (1), then I will judge"); tests passed; visually good on the Lichess machine
(the developer, 2026-10-22), judged further day by day in use.**

## Decisions (the developer, 2026-10-22)

1. **The main page** shows a box per config, `smoke`, `pesto`, `self-psq`, `lc0-psq` in that order,
   any other forge config after them by name, and the arenas' box, which stays on the main page.
2. **A box opens its config's page,** with Back; everything about that config is done there.
3. **The running job** (option 1): its config's box is marked running with a one-line status; its
   page shows the live sections and Stop.

## Settled by Claude, to judge

- A box's line: the running job when it is the config's; else its latest run's state ("no run yet"
  when none); `pesto`'s, its tune.
- A config's page: Back and its name; the job when it is the config's (running, with Stop on the
  smithy's own machine; or how the last one ended); its live sections; its runs with View, Resume,
  Tune, and New run; the View section. `pesto`'s page: its tune and Tune, the live tune while it
  runs.
- Each page has an address, `#<config>`, so the browser's back and a reload keep it.
- The state names the job's config (`Snapshot::config`, `job_config`): a forge's own, `pesto` for
  pesto's tune, a network's run's config for its tune.
