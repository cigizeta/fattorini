# One NNUE run per generation, stepping down a list of rates on patience

Status: agreed with the developer on 2026-09-28; done (implemented, built, all tests pass the same
day). Supersedes learning-rate-schedule.md (set aside as too complex) and the stopping rule of
train-until-patience.md.

**The lineage names here are those of 2026-09-28** (amanda, betty, caroline, dorothy as they
were then), not today's: the names were later reused for new lineages, and `configs/dorothy.yaml`
no longer exists. Today's lineages train under this same rule with their own rate lists (e.g.
caroline's, caroline-generation-1.md).

## Why

- A grid of fixed rates costs a full training run per rate, and near the optimum the rate barely
  matters: at 4 epochs caroline's 3.5e-4, 5e-4 and 7e-4 arms were within 3e-4 in test loss, and
  betty-0001 and caroline-0001, trained very differently, are about equal on the board.
- The best fixed rate depends on how long training runs (caroline's winner drifted from 1e-3 to
  3.5e-4). Stepping the rate down when progress stalls follows that drift inside one run.
- The grid made the stopping rule complicated (per-arm patience, every arm exhausted, slow arms
  stalling the run). One run has one best and one patience count.
- One gate per generation already guards against a bad challenger: no tournament of candidates.

## The rule

`configs/dorothy.yaml`:

```yaml
learning_rate: [3.0e-3, 1.0e-3, 3.0e-4, 1.0e-4, 3.0e-5, 1.0e-5]
warmup_rounds: 2
patience_rounds: 8
```

1. Warm up linearly over `warmup_rounds` rounds to the first rate.
2. Train at the current rate. When `patience_rounds` rounds pass without a new best test loss,
   switch abruptly to the next rate. The weights and the optimizer's moments carry over; only the
   rate changes.
3. Patience counts from the latest of: the last new best, the last switch, the end of warmup. So
   warmup rounds never count against patience, and every rate gets its full `patience_rounds`.
   The best test loss itself is tracked over the whole run, warmup included.
4. When patience runs out at the last rate, stop. No minimum length and no cap. With no improvement
   at all the run would spend `patience_rounds` at every rate (48 rounds for dorothy); training is
   cheap enough to afford that.
5. The challenger is the best checkpoint of the whole run, then one gate.

## Decisions

- **Config**, `training:` of an NNUE kind: `learning_rate` is a scalar or a list, strictly
  decreasing, every rate finite and positive; a scalar is a one-stage run. `warmup_rounds` (may be 0)
  and `patience_rounds` (at least 1) are required, NNUE-only, checked in both directions like
  `shuffle_buffer`. `rounds_per_epoch` is gone. The transformer takes exactly one rate and keeps its
  `--patience`/`--min-steps` flags.
- **Code constants removed**: `WARMUP_STEPS`, `PATIENCE_ROUNDS`, `MAX_EPOCHS`, `StopReason`.
- **No state file.** The current stage is replayed from `history.tsv`'s test losses by a pure,
  unit-tested function (`stage_after`), so a resume lands exactly where the run was. `history.tsv`
  gains a `learning_rate` column after `step`, for reading; the replay does not use it.
- **`AdamW::set_learning_rate`**, called before every round with the replayed stage's rate. The
  warmup factor still multiplies it, and warmup always ends before the first possible switch.
- **No epoch anywhere on the NNUE track.** Logs count rounds and steps. One line reports how many
  passes over the train cache one round is.
- **Flat generation directory.** Everything a generation holds sits in `gen-NNNN/`:
  `run_config.txt`, `model.vs`, `optimizer.opt`, `history.tsv`, `round_progress.state`,
  `checkpoints/`, `initial.vs` (gen-0001 only), `challenger.nnue`, `challenger.txt`, `gate.db`,
  `gate.pgn`, `verdict.txt`, `champion.*`. No `lr-*/`, no `epoch-NNNN/`, no `grid.tsv`.
- **`check`/`calibrate`/`correlate` take `<network> <step>`**, no `--lr`. The transformer's run
  directory takes its rate from its config and stays `data/networks/<name>/lr-<rate>/`.
- **Records.** `challenger.txt` and `champion.txt` record the rate the checkpoint was trained at,
  for provenance; nothing parses it. `promote` reads the step only. `champion.txt` loses `epoch`.
- **Frozen fields** in `run_config.txt`: `rounds_per_epoch` out; `learning_rate` (the whole list),
  `warmup_rounds` and `patience_rounds` in.
- **No compatibility code.** The layout migrations are deleted. Old generations are not converted:
  their `champion.*` files stay where they are, which is all matches, promotion and the next
  generation read.
- **Existing lineages.** amanda, betty and caroline get one rate, `3.0e-4`, `warmup_rounds: 2`,
  `patience_rounds: 12`.
- **Checkpoint retention** is unchanged: the best few by test loss plus the latest.

## Implementation plan

- `src/train/net_config.rs`: as above, with tests (a list that is not strictly decreasing, a zero
  rate, a missing `patience_rounds`, a stray `warmup_rounds` on the transformer, a list on the
  transformer).
- `src/train/adamw.rs`: `set_learning_rate`, `learning_rate()`, with a test.
- `src/train/history.rs`: the `learning_rate` column.
- `src/train/stages.rs` (new): `stage_after(records, steps_per_round, warmup_rounds,
  patience_rounds, stage_count) -> Stages { stage, finished, best_step, best_loss }`, with tests:
  warmup never counts; a switch after `patience_rounds` flat rounds; patience restarts at a switch;
  a new best restarts it; the last stage's exhaustion finishes the run; no records is stage 0.
- `src/train/round.rs`: one `Trainee` per round, scalar losses; tests adjusted.
- `src/train/mod.rs`: one network in `gen-NNNN/`; after every round, append history, replay the
  stages, print a switch, set the rate, finish when done (also checked at startup). `NnueArm`,
  `seed_new_arms`, `shared_step_count`, `lowest_index`, `placement`, `grid` go. `check`,
  `calibrate`, `correlate`, `promote`, `USAGE` and docs updated. Tests updated.
- `src/train/grid.rs`: deleted.
- `src/train/run_config.rs`: docs, and a test that names `rounds_per_epoch`.
- `src/pipeline/generation.rs`: flat layout, migrations removed (done).
- `src/arena/main.rs`, `gate.rs`, `match_player.rs`: gate in `gen-NNNN/`, no migration (done).
- Configs and examples (done, dorothy by the developer).

## Progress

- 2026-09-28: plan written for one rate; revised the same day to a list of rates stepped down on
  patience, from the latest of last best, last switch and end of warmup.
- 2026-09-28: implemented, not yet built: `net_config.rs`, `adamw.rs` (`set_learning_rate`),
  `history.rs` (`learning_rate` column), `stages.rs` (new), `round.rs` (`Trainee`), `mod.rs` (one
  run per generation, stages replayed after every round, `check`/`calibrate`/`correlate` without
  `--lr`), `run_config.rs`, `generation.rs` (flat layout, no migrations), the arena's gate, configs
  and examples. `grid.rs` is unreferenced and to be deleted with git.
- 2026-09-28: `grid.rs` removed; built; all tests pass. Next: train dorothy's generation 1.
