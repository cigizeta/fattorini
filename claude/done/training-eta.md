# Training ETA by stages

Status: designed and implemented 2026-11-16 (the developer), to test. As built: no Rust test for
`stage_round`, which is `Stages::switch_step` (already tested in `stages.rs`) divided by the round's
steps; the page's `stageProgress` counts the switch round's stage as done (step 1 below).

## The problem

The training box's ETA (`trainingProgress`, `page.html`) counts patience rounds: a run of 5 stages
and patience 6 is "30 patience rounds", and its ETA scales the elapsed time by the share of them
used. That worked with the grid, whose lambdas average each other out; for one arm on the
polyline it does not: a new best resets the stage's patience, so the count stalls for most of a
stage, and the ETA keeps saying "about an hour" until the last rounds (halfk run 2: 18 of 30 with
1h 16m left at round 201 of 271).

## The estimate (the developer, 2026-11-16)

Known at the latest round: `S` stages, patience `P`, the current stage `s` (from 1), the round `R`
it began at, the current round `r`, and `p` rounds without a new best in it.

- **The developer's estimate:** the stages done took `R` rounds, so each about `R / (s - 1)`, the
  whole run about `S * R / (s - 1)`: `estimate_left = S * R / (s - 1) - r`. None in stage 1.
- **The minimum:** no round ever improving again, the current stage ends in `P - p` rounds and
  each later one takes exactly `P`: `minimum_left = (P - p) + (S - s) * P`. Never an overestimate.
- **Rounds left:** `max(estimate_left, minimum_left)`; in stage 1 none, only `minimum_left` shown
  as a bound.
- **ETA:** rounds left times the latest round's seconds (training and test), the pace the next
  rounds will have.

Checked on `lc0-psq-threats-screlu` run 1 (stages began at rounds 38, 72, 100, 146; ended at 180):
the total estimated at each stage's start is 190, 180, 167, 183.

## Where it applies

**One arm only** (the developer, 2026-11-16): a lockstep grid keeps today's patience-round progress
and ETA unchanged, until the grid's code is removed (TODO.md).

## The page

For one arm, the line under the round's own becomes:

- stage 2 on: `151 of about 230 rounds (65.7%), elapsed 1h 48m, ETA about 56m; at least 13 rounds left (9m)`;
- stage 1: `20 rounds, elapsed 12m; at least 30 rounds left (21m)`: no ETA and no bar, no stage
  having ended to measure one by, only the minimum (the developer, 2026-11-16);
- finished: `271 of 271 rounds (100.0%), elapsed 2h 35m`.

Worded as the round's own line above it (`878 of 4,096 steps (21.4%)`) and the patience line it
replaces (`18 of 30 patience rounds (60.0%)`).

The bar: rounds played over played plus rounds left, from stage 2 on.
The patience line goes: the table's `patience` column still shows `p of P`.

## Implementation

1. **`RoundRow` (`smithy.rs`) gains `stage_round: u64`**, the round the current stage began at:
   `stages.switch_step / per_round`, 0 in stage 1. At the round of a switch the row is the old
   stage's (its rate), as today; its `waiting` reads `P`, so `minimum_left` is then the later stages'
   alone, which is right: the next round is the next stage's first.
2. **`page.html`:** `trainingProgress` branches on `lambdas.length === 1`; the one-arm branch in a
   function of its own, `stageProgress(row, round)`, the formulas above, rounded up to whole rounds.
   The grid's branch untouched.
3. **Tests:** `RoundRow`'s `stage_round` on a history with two switches (`stages.rs`' fixtures); the
   page's arithmetic has no test harness, so its formulas are kept to the few lines above, checked
   by hand on psq run 1's history.

## Commands for the developer

```bash
cargo test --release -p smithy
```

```bash
cargo clippy --workspace --all-targets --release -- -D warnings
```
