# Training loss chart

Status: designed 2026-11-16 (the developer), implemented the same day, to check by eye after the
smithy's next restart. Colors (the developer, "I will judge only after I see them"): test the
accent blue, train a new `--train` amber (#d97706, dark #fbbf24), best dot `--best` green, stage
lines `--line`, axes and text `--muted`. As built: `Status::arm_rounds` replaces `last_row`, returning
the row and the curve from one read; the curve is cached for every arm and sent for the polyline's
alone. For the polyline case only: one arm,
lambda `polyline`. A grid keeps its table and has no chart.

## The page today

The training box (`training`, `page.html`) gets each lambda's latest round alone
(`LambdaView.last`, `smithy.rs`), drawn as a one-row table: lambda, rank, learning rate, train loss,
test loss, best loss, round, stage, patience. With one arm, lambda says `polyline` and rank says
`1`, both always.

## The change

1. **Table:** for the polyline case, no `lambda` and `rank` columns.
2. **Data:** `LambdaView` gains `curve: Vec<[f64; 4]>`, one `[round, train loss, test loss,
   learning rate]` per round of the arm's `history.tsv`, filled for a polyline arm only (empty
   otherwise, so a grid's polls stay small). `last_row` already reads every record to sum the
   elapsed time; the curve comes from the same read and is cached with the row by the file's
   modification time. About 300 rounds, under 10 KB a poll.
3. **Chart:** below the table, inline SVG, no library, the box's full width, about 240 px tall.
   - x: the round; y: the loss. Two lines, train and test, with a legend.
   - **Round 0 left out:** it only tests (no training loss), and its test loss, far above the
     rest, would flatten the curve. y scales to the rounds from 1 on, with a few ticks each axis.
     **Since 2026-11-26 (the developer): always four loss ticks,** the axis running from the first
     to the last, the step 1, 2 or 5 times a power of ten; seen working the same day.
     **Revised 2026-11-16 (the developer, after seeing it):** rounds 1 to 3 still took the height;
     y now scales to the rounds after the first tenth alone, linear, the plot clipped as a
     window: earlier lines leave it through its top, never pinned to the edge (the developer). A log of the gap above the lowest loss was weighed and not taken: its shape
     moves with every new best.
   - **Stage boundaries:** a faint vertical line at the last round of each learning rate before a
     switch, where the drop begins: a round's losses are measured after it trains, so the new
     rate's first round already shows its effect (the developer, 2026-11-16, after seeing the line
     one round late).
   - **Best round:** a dot on the test line at `last.best_round`.
   - Colors as tokens on `:root`, a dark variant beside the page's own.
4. **Nothing before round 1:** no chart until a trained round exists.

## Tests

- Rust: `curve` filled from a history fixture for a polyline arm, empty for a constant lambda.
- The page has no test harness: checked by eye on the running smithy, on a finished run's
  history (step 2's, 323 rounds) and on a live one.

## Commands for the developer

```bash
cargo test --release -p smithy
```

```bash
cargo clippy --workspace --all-targets --release -- -D warnings
```

The page is compiled into the smithy: it shows on the smithy's next restart, never during a
training.
