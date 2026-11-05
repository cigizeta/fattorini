# No text bars on the page

Status: implemented 2026-10-23, not compiled when written; to be judged by the developer as each
feature is next used.

## What (the developer, 2026-10-23)

Every text progress bar left on the page goes, as training's did (round-in-flight.md), each feature
judged later, when used.

## What was left

- **Self-play and the tune:** a live text bar each, duplicating what their own boxes already draw as
  graphical bars (`WorkView::SelfPlay`, `WorkView::Tune`).
- **The rest, text alone:** lc0's conversion (downloading and converting, per file, mirrored from
  its child process), and the forge's preparation steps (`buckets.rs` scattering, `freeze.rs`
  reading test games and writing positions). Each reaches the page as a `progress::LiveBar`: label,
  position, length, counts, percent, ETA and rate.

## Decisions

1. **Self-play's and the tune's live bars go** (`live`, `end_live`, `LIVE` in `smithy.rs`): their
   boxes say it already.
2. **The page draws every other live bar graphically,** as the training box's lines: `label: counts
   (percent%), rate, ETA about 58s` over a graphical bar, a counter (no length) with its line alone.
   One place serves them all: `liveLine` in `page.html`, the `<pre>` terminal look gone.
3. **The ETA in the page's wording:** the `LiveBar` ETA, `HH:MM:SS`, read back into seconds and
   written by `duration`, as every other ETA on the page.
4. **indicatif stays underneath for now:** the bars still feed `progress::live` (the arena's book
   download and the checkpoint inspections draw theirs on a terminal). Its removal from the
   workspace is the next step, `LiveBar` already the page's only view of a bar.

## The work

- `smithy.rs`: `live`, `end_live`, `LIVE` and their indicatif imports removed; self-play's and the
  tune's loops without a bar.
- `page.html`: `liveLine` graphical; `terminalBar` and the `<pre class="terminal">` look removed if
  nothing else uses them.
