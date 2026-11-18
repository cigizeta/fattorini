# A budget's loss chart: a fixed horizontal axis and its epochs

**Status:** design settled and plan written, 2026-11-18 (the developer). Implemented the same day:
`Budget::rounds_per_epoch` (with a test), `ScheduleRow::Budget`'s `rounds_per_epoch`, `lossChart`'s
fixed axis and epoch lines (as the stage lines are drawn, `class="stage"`). Steps 3 and 4 left.

## Why

The page's loss chart (`lossChart` in `page.html`) stretches its horizontal axis from round 1 to the
latest round, so every round redraws the whole curve at a new scale: a feature looks steep one visit
and gentle the next, and two screenshots of one run do not compare. A budgeted run knows its length
from the start (`schedule.total_rounds`, which its progress line already reads), so its axis can be
fixed at that length: the curve's end says how far the run is, the empty width what is left, and two
budgets of the same length share one scale (the 8 against 32 king buckets of halfk-king-buckets.md).

The epochs are known from the start too. On 2026-11-18 `lc0-halfk-threats-screlu-hidden-512` run 1
began to overfit exactly where its third pass over the training set began (round 143: the train
loss fell 0.0010 in three rounds and went below the test loss, which rose), and width 256 showed the
same at its fourth and fifth passes. A faint line where each pass begins shows that as it happens.

## The decision (the developer, 2026-11-18)

- **For a budget only.** A patience run's length is an estimate that moves (that same run went from
  "about 88 rounds" to "about 220"), and an axis fixed on it would claim a length nobody knows: its
  chart stays as it is, stage lines included.
- **The axis:** from round 1 to `max(total_rounds, latest round)`, fixed from the first round drawn.
  The second term only guards against a round past the total, which a budget never has.
- **Epoch lines:** a faint vertical line where each pass after the first begins, at round
  `k * rounds_per_epoch` for each whole `k` with `1 <= k < epochs`: three for today's 4 epochs. No
  line at the end, which the axis already marks. A budget has no stages, so these are the only
  vertical lines on its chart.
- **Not done:** the round robin's nine candidates marked on the axis (the table names them by epoch
  already); the vertical range's rule, which depends on the data alone and stays (the rounds after the
  first tenth of those drawn, padded 5%).

## Plan

1. **The snapshot** (`smithy.rs`): `ScheduleRow::Budget` gains `rounds_per_epoch: f64`,
   `steps_per_epoch / steps_per_round` of the arm's `Budget`, filled where the row is built beside
   `total_rounds`. Fractional: a pass rarely ends at a round's end, and its line is drawn where it
   falls. The training stream's own epoch (`batches_per_epoch`, whole batches per piece count) is a
   few batches shorter than `train_rows / batch`, under a hundredth of a round: not worth a field of
   its own.
2. **The chart** (`page.html`, `lossChart`): given a budget's schedule (the `last` row's, as today's
   `budget` flag comes from), `first` is 1 and `latest` is `max(total_rounds, the latest row's
   round)`; the round ticks follow that range, so they too stay put. The epoch lines drawn as the
   stage lines are (`class="stage"`, or a class of their own if they must read apart), before the
   curves so the curves lie over them. A patience run's chart unchanged, the code path included.
3. **Checked by eye** on the smithy's page: a scaled budget run (its fraction of an epoch gives no
   whole pass, so no line: the axis alone), then the first full budget run (three lines, at about
   rounds 68, 135 and 203 for today's corpus); and a patience run's chart unchanged.
4. `cargo test -p smithy` and clippy, for the snapshot's new field.
