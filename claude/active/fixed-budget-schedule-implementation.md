# A fixed budget and a falling schedule: implementation

**Status:** plan written 2026-11-17. Steps 1 to 9 written 2026-11-18, not yet compiled nor tested
(step 10, by the developer); then the scaled smoke run (step 11). Implements fixed-budget-schedule.md.

**As written (2026-11-18), where it differs from or settles the plan:**

- The open points: the budget's rate replaces the fixed rate inside `StackedAdamW` (`set_budget`),
  the warmup factor unchanged, both from the arm's count, which a budget never rolls back. The round
  robin record needed no change, only the view (`StandingView::label`). `scale` needs nothing for
  training (`train_rows` shrinks); a scaled run's round robin pairs shrink by `scale::games`.
- `stages::budget_after` replays a budget's rounds as `Stages` (one stage, no switch, finished at
  `total_steps`), so the round loop's patience branches never fire under a budget: the loop differs
  only in its pruning (`keep_steps`), no `best.opt`, and the rate a history row records
  (`Budget::rate`). `lockstep::Rule` carries either schedule.
- `run_config.txt` of a patience run is byte-identical to before; a budget adds `schedule` and
  `epochs` and has no `patience_rounds`, so neither resumes as the other.
- `lambda: polyline` is checked in the smithy's `load_config`, the forge key's reader, not in
  `net_config`.
- The page: `Budget::checked` (no panic on a poll), the budget's table without stage and patience,
  its rate in three digits, its progress `round r of R`, no stage lines on its loss chart.

## What the code does today (read 2026-11-17)

- **Training** runs through `lockstep::train_in_lockstep` for every track, a polyline run being one
  arm. `open_arm` freezes `run_config.txt` (rates, warmup, patience among its fields) and replays
  `stages::stage_after` from `history.tsv`; `open_stack` sets each slot's rate from its stage and the
  warmup on `StackedAdamW` (a factor derived from the step). After every round: the history row, the
  round's checkpoint, the stages replayed, `checkpoint_store::keep_only(best_step)`, `best.opt` at a
  new best, the rate set at a switch, `roll_back` at a switch, and `finish` when the last stage runs
  out.
- **The candidate:** `finish` is `candidates::export_best`, which exports the best checkpoint to the
  arm's `candidate.nnue` and writes `candidate.txt` (its `step`), whose presence marks the run
  finished.
- **The generation** (`smithy.rs`, about line 5454): `Train`, then `RoundRobin` only with two arms or
  more (a grid's; a polyline arm goes straight to the gate), then `Gate`, then promotion from
  `candidate.txt`'s step. `round_robin` takes the arms' lambdas (`&[f64]`), plays every pairing
  `round_robin_pairs` pairs at the defaults (`play_pairs`), ranks by points; its record and its page
  view (`round_robin_view`, `StandingView`) are keyed by lambda.
- **The page's training table** reads `Schedule` (`smithy.rs` about line 2812: rates, warmup,
  patience) for its stage and patience columns, and `lockstep::latest_round` for the stages.

## The design, in code terms

- **The switch:** `training.schedule: patience | budget` in `net_config.rs`, `patience` when absent.
  `budget` requires `epochs` (a positive number), a `learning_rate` of exactly two rates, start then
  end, decreasing, `warmup_rounds`, and `lambda: polyline` (one arm: the grid is deprecated, and nine
  arms times nine candidates is not wanted); it refuses `patience_rounds` and a transformer. `patience`
  is validated exactly as today. All four refusals tested.
- **The budget in steps,** from `train_rows`, which `train_in_lockstep` already has:
  `steps_per_epoch = train_rows / training_batch_size`, `total_steps = ceil(epochs * steps_per_epoch
  / steps_per_round) * steps_per_round`, whole rounds. A pure function, `budget::total_steps`, tested.
- **The rate,** a pure function of the run step, `budget::rate(step)`: the warmup ramp as today over
  `warmup_rounds`, then `start * (end / start) ^ ((step - warmup) / (total - warmup))`, `end` at the
  last step. Set per step, inside `StackedAdamW`, as the warmup factor already is: a schedule given to
  the optimizer once (`set_budget(total_steps, warmup_steps, start, end)`), its rate derived from each
  slot's run step, so a resume needs nothing but the step count. Tested: start after the warmup, end
  at the last step, geometric halfway (the geometric mean of start and end).
- **The candidates:** the checkpoints at epochs 2, 2.25, ..., 4, each at the first round end at or
  past `e * steps_per_epoch` (checkpoints exist at round ends only). A pure function,
  `budget::candidate_steps`, tested, among them that the last is `total_steps`. With `epochs` other
  than 4: every quarter epoch from half the budget to its end.
- **Checkpoints kept:** under `budget`, `keep_only` keeps every candidate step and the latest round
  (for a resume), deleting the rest; under `patience`, as today. A new `checkpoint_store::keep_steps`.
- **No rollback, no `best.opt`, no stages** under `budget`: the round loop's patience branch untouched,
  a budget branch beside it. The run finishes when its run step reaches `total_steps`. Step 0 is still
  measured and recorded, for the history, never a candidate.
- **Finish, under `budget`:** exports every candidate checkpoint to
  `gen-<g>/lambda-polyline/candidates/epoch-<e>.nnue` (`e` as `2.00`, `2.25`, ...), through the same
  `write_engine_network` and quantization, staged and renamed; then a `candidates.txt` listing
  `epoch<TAB>step<TAB>test_loss` per candidate, written last, its presence marking the training done.
  No `candidate.nnue` yet: the round robin chooses it.
- **The round robin, generalized:** its players become named candidates (a label and a network path)
  instead of lambdas: `round_robin(state, players: &[(String, String)])`, the view's `StandingView`
  carrying a label (`lambda 0.05` or `epoch 3.25`) instead of a lambda. The grid's caller builds its
  players from its lambdas as today; the budget's from `candidates.txt`. Same `play_pairs`, defaults,
  depth and record. **Pairs per pairing under `budget`:** about 2,000 games per candidate,
  `ceil(1000 / (n - 1))` pairs a pairing (two games a pair, `n - 1` opponents), 125 for nine, 36
  pairings, 9,000 games; not `round_robin_pairs`, which stays the grid's.
- **After the round robin, under `budget`:** the winner's checkpoint exported as the arm's
  `candidate.nnue` and `candidate.txt` (its `step`), as `export_candidate` does today, so the gate and
  the promotion run unchanged. The generation's flow: under `budget`, `Train` always goes to
  `RoundRobin`, whatever the arm count; `learned_arms`' fallback to step 0 does not apply (step 0 is
  never a candidate).
- **`run_config.txt` under `budget`:** freezes `schedule`, `epochs`, `learning_rate` (both rates),
  `warmup_rounds`, the batch and round sizes and `train_rows`, `test_rows`; no `patience_rounds`.
  A run frozen under one schedule refuses to resume under the other, by the existing check.
- **The page:** under `budget`, the training table drops its stage and patience columns, and its
  progress line reads `round r of R (p%), elapsed, ETA` from `total_steps` and the measured time per
  round; the round robin section shows epochs. Patience runs' table and estimate unchanged
  (deprecated). The exact `page.html` changes to settle on reading it.
- **Configs:** `configs/lc0-halfk-threats-screlu-hidden-budget.yaml` and
  `configs/lc0-halfk-threats-screlu-hidden-512-budget.yaml`, each its patience config with
  `schedule: budget`, `epochs: 4`, `learning_rate: [1.0e-3, 1.0e-5]`, no `patience_rounds`, its
  header saying it differs by the schedule alone. **From 1e-3 to 1e-5** (the developer, 2026-11-17):
  3e-3 was too high (the 512 run's loss rose after 9 rounds at it; at 256 every switch to 1e-3 gained
  about 0.002 at once), and bullet starts at 1e-3, nnue-pytorch at 8.75e-4; a hundredfold range, as
  bullet's example and our old one. The end untested below 3e-5, where every patience run still
  gained: the round robin's candidates span about 1e-4 (epoch 2) to 1e-5 (epoch 4), so it shows
  whether the low end pays; if epoch 3 (about 3e-5) beats epoch 4, the next configs end at 3e-5.

## Steps

1. `net_config.rs`: the `schedule` and `epochs` fields, the validation, the tests. Every existing
   config still loads (the test that loads every config in `configs/`).
2. `budget.rs` (new): `total_steps`, `rate`, `candidate_steps`, with their tests.
3. `stacked_adamw.rs`: the budget schedule beside the warmup, the rate per step from the run step;
   tests against `budget::rate`.
4. `checkpoint_store.rs`: `keep_steps`, tested.
5. `lockstep.rs`: `open_arm` freezes the budget's fields; `open_stack` sets the schedule; the round
   loop's budget branch (no stages, no rollback, finish at `total_steps`); the startup lines saying
   the budget (`R rounds, 4 epochs, rate 3e-3 to 3e-5 geometric, candidates at ...`).
6. `candidates.rs`: the budget's finish (every candidate exported, `candidates.txt`), and the export
   of the round robin's winner as `candidate.nnue`.
7. `smithy.rs`: the round robin over named players; the budget's flow, `Train` to `RoundRobin` to
   `Gate`; the training table's `Schedule` for both kinds; the ETA.
8. `page.html`: the table's columns and the round robin's labels.
9. The two configs.
10. Checks, by the developer:

```
cargo test -p smithy
```

```
cargo clippy --workspace --all-targets
```

11. **A scaled smoke run** of `lc0-halfk-threats-screlu-hidden-budget` on a development tree, end to
    end: training to its budget, nine candidates exported, the round robin, the winner's export, the
    gate. Then a resume check: stopped mid-training and mid-round-robin, resumed.
12. **The validation run:** `lc0-halfk-threats-screlu-hidden-budget` on the training machine (about 3.6
    hours), against screlu-hidden at 256 under patience: final losses, the round robin's curve over
    epochs 2 to 4, a depth-9 duel of the two winners. Then the 512 pair, once the patience 512 run is
    in.
13. **Docs:** this plan's progress; fixed-budget-schedule.md's open points closed; TODO.md.

## Open, to settle while implementing

- Whether `StackedAdamW`'s per-slot rate (`set_learning_rate`) and its warmup factor combine as the
  plan assumes; if the warmup factor multiplies a fixed rate, the budget's rate replaces the fixed
  rate per step, the warmup factor unchanged.
- The round robin record's format: keyed by pairing index today, so named players may need no change
  to it, only to the view.
- Whether `scale` (scaled runs) needs anything: `train_rows` already shrinks with it, so the budget
  scales by itself.
