# The smithy's page as its terminal: live progress and the terminal's blocks

**Archived 2026-11-04: superseded, see no-text-bars.md and indicatif-removal.md. The terminal
draws no bars any more, so there is nothing to mirror.**

**Status: design agreed with the developer, 2026-10-23; plan below, not implemented.**

## Why

The terminal shows a bar that moves at every training step and every test batch (`print_progress`,
rounds_display.rs, fed by `round.rs`'s `on_progress`), and others for the scatter, the round robin,
the arenas' chores. The page shows none of them: its Training table changes only when a round ends,
and it polls every 2 s. A long round looks frozen. The developer wants the page to resemble the
terminal, with progress whenever a step is done; and, beyond the bars, **the page as close as
possible in spirit to the terminal** (the developer, 2026-10-23): its blocks shown as the terminal
prints them, the lambdas' table first.

## Decisions (the developer, 2026-10-23)

1. **The web page stays,** not a native GUI (egui was weighed): any machine on the LAN can open it,
   and it will serve whoever runs Rotor once it is public. The cost of a browser polling a few
   hundred bytes is negligible beside training; efficiency decided nothing.
2. **Every terminal bar is mirrored,** hooked once in the `progress` crate, not wired bar by bar: a
   bar added later appears on the page by itself.
3. **The page polls a small `progress.json` every 250 ms,** the terminal's own redraw pace (the live
   block's `REDRAWS_PER_SECOND` is 4). Not server push: the listener answers one request at a time,
   and a held stream would need a thread per viewer, for nothing visible at 4 updates a second.
4. **Every terminal block is shown as the terminal prints it:** the lambdas' round block, the round
   robin's live table, the gate's SPRT line, the tune's table. Other parts of the page (the catalog,
   the job controls, the Machines and Arenas tables) stay as they are.
5. **One source of truth, Claude's proposal accepted:** the page never imitates the terminal in
   JavaScript, which would drift. Each block's lines are built by one Rust function that the terminal
   and `state.json` both call; colours travel as named styles, not ANSI codes.

## Design

### The terminal's blocks, built once

- Each block's line-building is pulled out into a function of the data the page already has, so it
  works from memory during a phase and from the files for a past attempt (the page's View):
  - **The round block** (lockstep.rs's `round_line`, `round_block_lines`; rounds_display.rs's
    `merge_rounds`, `progress_gauges`, `colour_round_line`, `timing_line`): from each lambda's latest
    round, the `RoundRow` the page reads from `history.tsv` (`Status::last_row`), which needs the
    round's train and test seconds added for the timing line. The redrawn form is the one shown: best
    loss first, `@ round b/n`, the stage and patience gauges, a finished lambda greyed without
    gauges, a new best in bold green, the timing line under it.
  - **The round robin's table, the gate's line, the tune's table:** the same, each from what the
    page's `WorkView` already carries; step 1 lists where each is printed today.
- A block is `Vec<StyledLine>`, each line a list of spans `(text, style)`, style one of `plain`,
  `best` (bold green), `finished` (grey). The terminal turns styles into ANSI codes (`highlight` and
  the grey of `colour_round_line`, under the same `NO_COLOR` and terminal checks); `state.json`
  sends them as they are.
- The page renders a block in a monospace `<pre>`, each style a CSS class whose colours match the
  terminal's, in the page's light and dark themes.

### The registry, in the `progress` crate

- Every bar the crate makes (`resumed`, `bar`, `counter`, `bar_in`) is also registered in a
  process-wide list, as an `indicatif::WeakProgressBar` with what the terminal line needs beyond the
  bar itself: its unit, its start (the baseline of this run's rate and ETA), and whether it is a
  counter.
- **Nothing is done per step:** the callers keep calling `inc` and `set_position` on their bar, and
  the registry reads the bar only when asked, `progress::live()`. A dropped bar no longer upgrades and
  is pruned then; a finished one (`is_finished`) is left out.
- `live()` returns, for each bar, in creation order: label, position, length (none for a counter),
  unit, elapsed seconds, and the rate and ETA as `rate_text` and `eta_text` already print them, so the
  page shows exactly the terminal's words.
- Hidden bars (`cfg!(test)`) are registered too; nothing serves them under a test.

### The smithy

- `GET /progress.json` on every listener, read-only like `state.json`: `progress::live()` as JSON.
  Its own endpoint, so the 250 ms poll never builds the full snapshot (`state.json` reads run files).
- Bars made outside the crate are moved into it, or registered: the live block (`LIVE` in
  smithy.rs, `ProgressStyle` built locally) at least; step 1 below lists them all.

### The page

- A **Progress** block at the top, one line a live bar, as the terminal draws it: label, elapsed,
  the bar (`bar()`), `pos/len unit`, percent, ETA and rate; a counter as `pos unit (rate)`. Gone
  when its bar finishes. In monospace, as the blocks.
- Its own `setInterval` at 250 ms on `progress.json`; `state.json` stays at 2 s.
- The forge's phases and a tune's work show their terminal blocks (above) in place of today's
  tables: Training's lambda table, the round robin's standings, the gate's numbers, the tune's
  parameters.

## Plan

1. **Inventory:** every `ProgressBar` the smithy makes outside the `progress` crate, each either
   moved to a crate constructor or registered through a new `progress::register`; and where each
   terminal block (round robin, gate, tune) is printed today.
2. **`progress` crate:** the registry, `register`, `live()` and its `LiveBar` (serde `Serialize`
   behind no feature: the crate gains `serde`); tests: a bar registered is listed with its rate and
   ETA, a finished one left out, a dropped one pruned.
3. **Smithy, progress:** `PROGRESS_PATH` served by `page()`; a test that a bar made in the process
   shows in `progress.json`.
4. **Smithy, blocks:** `StyledLine` and its ANSI rendering; the round block built from `RoundRow`s
   (train and test seconds added to it), the terminal calling it in place of today's line-building,
   its output byte for byte as today (a test pinning a block's text); then the round robin, the gate,
   the tune likewise; each block in `state.json` and the View.
5. **Page:** the Progress block and its 250 ms poll; the blocks in `<pre>`, the style classes in both
   themes; today's Training, round robin, gate and tune tables removed.
6. **Docs:** DEV.md (the page mirrors the terminal), TODO.md (the "Training bar" and "Prepare box"
   items: the scatter and the steps now show through this, their boxes still to judge).
7. **Checks:** `smithy start --dev` on a scaled forge, the page beside the terminal: the training bar
   moving with the terminal's through train and test passes; the round block the same text after
   each round, colours and gauges included; the round robin and gate the same; a tune; a past
   attempt's View; then a page opened on another machine.

### Lc0's Convert, a child process (the developer, 2026-10-23)

Convert runs as the smithy's child, its output a pipe the smithy prefixes into its terminal, so it
draws hidden bars and its progress showed nowhere: a dev Convert's first tar downloaded for minutes
in silence. Now the child prints its registry (`progress::live()`) as one `@progress <json>` line
four times a second; the smithy takes those lines out of what it prints and keeps them as mirrored
bars (`progress::mirror`), which `live()` lists after its own, texts as the child computed them. An
empty list, or the child's end, clears them. On the page only: drawn in the smithy's terminal, they
would draw over the arena job's bar, which runs beside Convert.

## Open, to decide while implementing
- **The arenas' bars** are in the arenas' processes: out of scope; the Machines table and the work
  views say what they do.
