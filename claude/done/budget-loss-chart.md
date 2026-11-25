# A budget's loss chart: a fixed horizontal axis and its candidates

**Status:** design settled and plan written, 2026-11-18 (the developer). Implemented the same day:
`Budget::rounds_per_epoch` (with a test), `ScheduleRow::Budget`'s `rounds_per_epoch`, `lossChart`'s
fixed axis and epoch lines (as the stage lines are drawn, `class="stage"`). **Changed 2026-11-19 (the
developer):** the vertical lines mark the round robin's candidates, not the epochs ("The decision"
below). Steps 1 and 2 redone the same day: `Budget::candidate_rounds` (with a test) in place of
`rounds_per_epoch`, `ScheduleRow::Budget`'s `candidate_rounds`, `lossChart`'s candidate lines. Step 4
done (tests and clippy clean, committed). Step 3 left: the check by eye on the page.

## Why

The page's loss chart (`lossChart` in `page.html`) stretches its horizontal axis from round 1 to the
latest round, so every round redraws the whole curve at a new scale: a feature looks steep one visit
and gentle the next, and two screenshots of one run do not compare. A budgeted run knows its length
from the start (`schedule.total_rounds`, which its progress line already reads), so its axis can be
fixed at that length: the curve's end says how far the run is, the empty width what is left, and two
budgets of the same length share one scale (the 8 against 32 king buckets of halfk-king-buckets.md).

The candidates are known from the start too (`Budget::candidates`: nine, every sixteenth of the
budget from its half to its end, each at a round end). A line at each shows which points of the
curve the round robin will judge, and how the curve moves between them.

## The decision (the developer, 2026-11-18, the lines changed 2026-11-19)

- **For a budget only.** A patience run's length is an estimate that moves (that same run went from
  "about 88 rounds" to "about 220"), and an axis fixed on it would claim a length nobody knows: its
  chart stays as it is, stage lines included.
- **The axis:** from round 1 to `max(total_rounds, latest round)`, fixed from the first round drawn.
  The second term only guards against a round past the total, which a budget never has.
- **Candidate lines:** a faint vertical line at each candidate's round, all nine, **the last
  included** although it falls on the axis's end (the developer, 2026-11-19): the lines match the
  round robin's table one to one. Fewer when two candidates share a round end (a tiny run), as
  `Budget::candidates` already gives. A scaled run has its lines too. A budget has no stages, so
  these are the only vertical lines on its chart.
- **The epoch lines go** (replaced, 2026-11-19). For 4 epochs the candidates fall at epochs 2, 2.25,
  ..., 4, so the starts of passes 3 and 4 (where width 512 overfit, `lc0-halfk-threats-screlu-hidden-512`
  run 1) keep a line; the start of pass 2 loses its own.
- **Not done:** the vertical range's rule, which depends on the data alone and stays (the rounds
  after the first tenth of those drawn, padded 5%).

## Plan

1. **The snapshot** (`smithy.rs`): `ScheduleRow::Budget`'s `rounds_per_epoch: f64` replaced by
   `candidate_rounds: Vec<u64>`, each candidate's `step / steps_per_round` from the arm's
   `Budget::candidates`, filled where the row is built beside `total_rounds`. Whole rounds: a
   candidate is taken at a round end. `Budget::rounds_per_epoch` and its test removed if nothing else
   reads it.
2. **The chart** (`page.html`, `lossChart`): the axis as before. The epoch lines replaced by a line at
   each of `candidate_rounds`, drawn as the stage lines are (`class="stage"`), before the curves so the
   curves lie over them; the last at the axis's end, drawn too. A patience run's chart unchanged, the
   code path included.
3. **Checked by eye** on the smithy's page: a scaled budget run (nine lines, or fewer if two share a
   round), then the first full budget run (nine lines, from about round 135 to its last, about 270,
   every 17 rounds or so for today's corpus); and a patience run's chart unchanged.
4. `cargo test -p smithy` and clippy, for the snapshot's changed field.
