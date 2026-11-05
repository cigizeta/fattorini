# Learning-rate schedule for NNUE training (dorothy)

Superseded, see one-learning-rate.md: set aside as too complex; one rate per generation, trained to
patience, then one gate.

Status: design agreed with the developer on 2026-09-28; implementation plan below, not yet applied.
Context: train-until-patience.md (caroline's grid of fixed rates drifted to its lowest rate and hit
the 4-epoch cap unconverged).

## Why

With AdamW every step moves each weight by about the learning rate, whatever the gradient's size, so
a fixed rate is wrong for part of training: a high one learns fast but jitters on a high plateau, a
low one settles lower but starts slowly. Caroline's winner drifted from 1e-3 (mid-epoch 1) to 3.5e-4
(epoch 4, the bottom of its grid). A schedule replaces the question "which fixed rate" with a rate
that falls as training progresses, and one arm replaces the grid.

## The schedule, as agreed

`configs/dorothy.yaml`:

```yaml
schedule:
  learning_rates: [1.0e-3, 1.0e-4, 1.0e-5]
  warmup_rounds: 2
  transition_rounds: 16
  patience_rounds: 8
```

1. Warm up linearly over `warmup_rounds` to the first rate.
2. At every rate but the last: stay until `patience_rounds` rounds pass without a new best test loss,
   then decay geometrically over `transition_rounds` to the next rate: at step `s` of a transition of
   `n` steps from `a` to `b`, the rate is `a * (b / a)^(s / n)`. Geometric rather than linear because
   a learning rate acts on a ratio scale: linear decay from 1e-3 to 1e-4 is still at 5.5e-4 halfway,
   and does most of the last factor of three in its last rounds.
3. On reaching the last rate, stop. There is no stable phase at the last rate.
4. No minimum length and no cap: the same rule everywhere. A first rate that runs out of patience
   inside epoch 1 is not prevented; it should not happen, and if it does something is wrong and is
   to be investigated. Every phase change is printed with its step and epoch, so it shows.
5. The challenger is the best checkpoint of the whole run, as today, then one gate.

Patience at a later rate counts from the end of the transition into it: rounds without a new best
since `max(best_step, end of transition)`. The best test loss itself is over the whole run.

## Candidates and the gauntlet (agreed; implementation plan to follow separately)

Loss chooses the checkpoint within a phase; games choose between phases.

1. **Candidates**: one per phase of the schedule, the phase's best checkpoint by test loss. The
   phases are each stable rate and each transition: `n` rates give `2 (n - 1)` candidates, 4 for
   dorothy (stable 1e-3 with its warmup, transition to 1e-4, stable 1e-4, transition to 1e-5).
2. **Gauntlet**: each candidate plays 200 games (100 opening pairs) against the lineage's champion,
   at the gate's fixed depth. The games only rank; they decide nothing about promotion.
3. **Choice**: the highest score. A tie goes to the lower best test loss (the next best evidence
   when games cannot separate two candidates); a tie in loss too, to the later phase.
4. **Gate**: one fresh SPRT of the chosen candidate against the champion, the gauntlet's games not
   counted. Pass: promote. Fail: the champion stays, and no other candidate gets a turn (that would
   be several gates, with their false promotions adding up).

Consequence for checkpoint retention: every phase's best checkpoint must survive pruning, plus the
latest for a resume.

## Decisions

- **`schedule` and `learning_rate` are mutually exclusive** for the NNUE kinds; the transformer keeps
  requiring `learning_rate`. Amanda, betty and caroline keep their grids and the patience-12 rule of
  train-until-patience.md unchanged.
- **One arm, named by its first rate**: `gen-NNNN/lr-1e-3/`, and `challenger.txt` records
  `learning_rate 1e-3`. `promote`, `check --lr` and the arena need no change.
- **The rate is recomputed every step, not every round.** `AdamW` gets a segment
  `(start_step, end_step, from_rate, to_rate, shape)` in place of its fixed `lr` and warmup, the shape
  linear for warmup (a geometric ramp cannot start from zero) and geometric for transitions, and computes
  the rate from its own step count, as its warmup already does. The training loop sets the segment at
  each phase change. Warmup becomes the first segment (`0 -> first rate` over `warmup_rounds`);
  the grid kinds keep `WARMUP_STEPS` as a segment of their own fixed rate.
- **No new state file.** Where the schedule is (which rate, stable or transition, since which step)
  is replayed from `history.tsv` by a pure function over the recorded test losses: the same losses
  give the same phase changes, so a resume lands exactly where the run was. Unit-tested on its own.
- **No epoch for a scheduled run.** Nothing in the schedule needs one, so `dorothy.yaml` has no
  `rounds_per_epoch`: a schedule config must not have it, a grid config still requires it. Logs count
  rounds only. The one challenger goes to `epoch-0001/`, so the gate and `promote` are unchanged;
  renaming that directory (vestigial for every lineage since each generation has one challenger) is a
  separate cleanup.
- **Checkpoint retention is unchanged here.** Keeping more checkpoints for a tournament of
  candidates is a separate change.

## Implementation plan

### `src/train/adamw.rs`

- Replace `lr` + `warmup_steps` with a `RateSegment { start_step, end_step, from, to, shape }`,
  `shape` being `Linear` or `Geometric`: the rate at step `t` is `from` before `start_step`,
  interpolated between (`from + (to - from) * f` or `from * (to / from)^f`, `f` the fraction of the
  segment done), `to` after. `set_segment`, `rate()`.
- `set_warmup_steps` kept as a wrapper building the segment `(0, steps, lr / steps, lr)`, so the grid
  path is unchanged. Tests for the segment, and the existing warmup tests unchanged.

### `src/train/net_config.rs`

- `Training.learning_rate` becomes optional; new `Option<ScheduleConfig { learning_rates,
  warmup_rounds, transition_rounds, patience_rounds }>`.
- `rounds_per_epoch` stays optional, as it already is for the transformer.
- `load` validates: an NNUE kind has exactly one of `learning_rate` and `schedule`; with
  `learning_rate` it needs `rounds_per_epoch`, with `schedule` it must not have it; the transformer
  has `learning_rate` and no `schedule`; `learning_rates` has at least two strictly decreasing positive rates;
  `transition_rounds >= 1`, `patience_rounds >= 1`. Tests for each refusal.

### `src/train/schedule.rs` (new)

- `Phase { Warmup, Stable { rate_index, since_step }, Transition { rate_index, start_step },
  Done }` and `replay(schedule, steps_per_round, rounds: &[(step, test_loss)]) -> Phase`: walks the recorded rounds applying the rules above. `segment(phase, ...) -> RateSegment`.
- Unit tests: warmup then stable; patience ends a rate after `patience_rounds` rounds without a new
  best; patience at a later rate counts from the end of its transition; the last transition ends in
  `Done`; a resume mid-transition continues the same segment.

### `src/train/mod.rs`

- `run_train_nnue`: with a `schedule`, build one arm at the first rate; after every round, replay
  the schedule from the arm's history, set the optimizer's segment for the next round, and when the
  phase is `Done`, call `finish_training` (a new `StopReason::ScheduleDone`). The patience-12 rule
  and the epoch cap apply only to grid configs.
- Messages: each phase change is printed (`rate 1e-3 stable since step N`, `decaying to 1e-4 over 16
  rounds`), and the round line shows the current rate.
- `run_config.txt` freezes the schedule in place of `rounds_per_epoch`, so a resumed run cannot
  silently change it.
- Everything epoch-based in the loop (the "epoch N round r/32" line, `placement`, the ratio of an
  epoch to the cache) is printed only for grid configs; a scheduled run's challenger goes to
  `epoch-0001/`.

### `configs/dorothy.yaml`

Committed with the code that reads it.

## Progress

- 2026-09-28: plan written. Revised the same day: no minimum of one epoch, the same rule everywhere;
  an early end is a signal to investigate, not something to prevent. Then: no epoch for a scheduled
  run, `rounds_per_epoch` removed from `dorothy.yaml` by the developer. Then: transitions geometric,
  not linear; `transition_rounds` stays 16.
