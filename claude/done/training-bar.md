# The training box's progress bar

Status: done 2026-10-23, seen on the page by the developer. Durations, the tests' ETA's too, show
whole minutes from 90 s, seconds below it, so never `1m`.

## What (the developer, 2026-10-23)

One bar in the page's Training box, for the whole training, of resolution stages x patience x
lambdas: 5 x 6 x 9 = 270 for today's forges.

## How it counts

A lambda's run ends exactly when patience runs out at its last stage, so its distance from the end is
counted in patience rounds:

- a lambda in stage `s` (from 1) with `w` rounds without a new best: `(s - 1) x patience + w`;
- a finished lambda: `stages x patience`, all of its share;
- a lambda yet to record a round, or at a rate the schedule does not name: 0.

The bar is the sum over the grid, of `stages x patience x lambdas`. It is not monotonic: a new best
resets a lambda's patience, so the bar steps back, never below the start of that lambda's stage. That
is honest: training is then still improving. Beside it, its count: `87 of 270`.

## Its ETA (the developer, 2026-10-23)

Approximate: the time left at the training's average pace so far, `elapsed x (total - done) / done`.
The elapsed time is exact, as the files record it across stops and restarts: each row's
`elapsed_seconds`, the sum of its `history.tsv`'s train and test seconds, round 0's test included;
the longest of the lambdas', since they train in lockstep and the last to finish has every round.
Approximate because the bar does not move at an even pace: a new best steps it back, and a stage of
a lower rate often ends sooner.

## The work

`training()` in `page.html` alone, from the rows it already gets (`stage`, `stages`, `waiting`,
`patience`, `finished`), and `RoundRow`'s `elapsed_seconds` in `smithy.rs` for the ETA.
