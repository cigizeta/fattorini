# The training box's round in flight

Status: done, verified on the page 2026-10-23 (the developer, upon a restart mid-run).

## What (the developer, 2026-10-23)

A try, judged when seen: the training box shows the round under way, with what the text bar shows
(the half, its steps or rows, rate and ETA), so the text bar may later go. The text bar stays for
now. The general direction is indicatif gone entirely (TODO.md), so the page reads none of its bars:
the round reports itself.

## Decisions

1. **The round reports itself:** `rounds_display::print_progress`, which every round's two halves
   already call, also keeps a plain record of the pass under way (half, round, done, total, when it
   began and from where), with no indicatif in it; `clear_progress_line` forgets it. Only a round's
   passes (a non-empty suffix) are kept, not the checkpoint inspections'.
2. **In memory, not from a file:** no record holds a round until it ends; a restart loses only the
   round in flight, which training redoes. The one exception to "records first", and why.
3. **Carried by `state.json`:** `ForgeView.round`, set on the running forge's own view only, its
   rate and ETA computed by the smithy from the pass's own start, as the bar's are.
4. **The bar:** past the solid patience rounds, a lighter segment as wide as the lambdas still
   training, filled by the share of the training half done. Removed (the developer, 2026-10-23):
   hard to see, harder as the patience rounds grow, its meaning unclear to a viewer; the round's
   line says it all. The bar is the patience rounds alone, as before.
5. **The line:** the box's first line, above the patience rounds' line, worded and styled as it
   (the developer, 2026-10-23): `round 68, training: 1,435 of 4,096 steps (35.0%), 45.4 steps/s,
   ETA about 58s`, only the half's own ETA (two ETAs confused); in the test half, `round 68,
   testing: 1,572,864 of 4,194,304 rows (37.5%), ..., ETA about 3s`, in the rows its pass counts.
   The line follows `state.json`, polled every 2 s. The grey line under the bar says `training`,
   not `train`, as the terminal's round event does.
6. **Elapsed stays per round,** from the histories, as today.
7. **The frozen test set's read (the developer, 2026-10-23):** in the round's line's place, worded
   alike: `reading the frozen test set: 1,234,567 of 4,194,304 positions (29.4%), 95,000
   positions/s, ETA about 31s`, noted by `freeze::read` every 4,096 positions through the same pass
   record (`note_pass`, half `reading`). The bar stays still during it; a fresh run, no round yet,
   shows the training box with this line alone.
8. **The text bars go (the developer, 2026-10-23, once 1 to 7 were seen):** a round's passes and
   the read make no indicatif bar, so the page's live section shows none during training. The
   checkpoint inspections' sweeps keep theirs, on the terminal.

## The work

- `rounds_display.rs`: `RoundPass`, set by `note_pass` (from `print_progress` and `freeze::read`),
  forgotten by `end_pass`; `current_pass()` gives its view.
- `smithy.rs`: `ForgeView.round`, set in the snapshot's forge view during training.
- `page.html`: `bar` with an in-flight segment; the training box's first line (`roundLine`).
