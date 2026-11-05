# The smithy as a window

**Status: idea, decisions below agreed 2026-10-22; not urgent, not designed further, not
implemented.** To take up with the GUI pass (TODO.md: the scatter's Prepare box, the Training bar;
browser-gui.md).

## Why

The page is a browser tab to open by hand, easily lost among others, and says nothing while a phase
it has no box for runs. Rotor is a personal project: the page need not be seen from other machines.

## Decided (the developer, 2026-10-22)

- **`smithy` alone** starts the smithy and opens its window; `smithy start` goes. The window shows
  what the page shows today.
- **Closing the window stops the job** (and the smithy with it).
- **`smithy --local` replaces `smithy smoke`** (the developer, 2026-10-22): the same window, every
  config scaled down and stored locally only, on any machine (TODO.md). **Superseded on 2026-10-23
  by `smithy start --dev` on today's page (discovery-and-heartbeats.md),** which the window inherits
  when built.
- The smithy's HTTP server stays: the arenas it claims take their chores from it over the LAN
  (discovery-and-heartbeats.md). Only the viewer changes.

## Proposed, to decide when designed

- **The window: WebView2, embedded** (`wry` with `tao`), showing `page.html`, so the page is kept and
  built on once. Against Edge's app mode (`msedge --app=...`: no dependency, but a separate process
  that can outlive the smithy or close without it knowing) and native widgets (`egui`: the page
  rewritten, a graphics stack added).
- **A confirmation** before closing while a job runs, since a close stops it.
- **`smithy stop` and `smithy status`** from another terminal: kept or dropped.
- **The LAN page** (`http://the training machine:47821/`): dropped, or kept read-only, now that nothing needs it.
