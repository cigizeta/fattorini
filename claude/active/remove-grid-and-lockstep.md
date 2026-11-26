# Remove the grid and lockstep (design agreed 2026-11-26)

## Why

No config trains a grid any more: every one in `configs/` says `lambda: polyline`, and every recent
experiment is a budget. Yet the only training loop is `train_in_lockstep` over a stacked network,
one slot wide for a single arm, and the grid's constants, coarse and fine, its round robin and its
fallback still shape the forge, the caches, the page and about fifteen tests. The developer,
2026-11-26: "time has arrived to remove the grid, and therefore the training in lockstep as well".

## Decisions (the developer, 2026-11-26, one question at a time)

1. **One arm trains an unstacked network.** A plain loop over `Network` (NNUE) and
   `TransformerNetwork`: their losses out of `#[cfg(test)]`, plain AdamW. `stacked_network.rs`,
   `stacked_transformer.rs`, `stacked_adamw.rs` and `arm_slice.rs` are deleted.
2. **The patience schedule goes too:** the budget alone. Stages, rollback, `Rule::Patience`,
   `ScheduleRow::Patience` and the page's `stageProgress` and patience-rounds bars are deleted. The
   patience configs move to `archived/configs/`, the eight found by the survey:
   lc0-halfk-threats-pairwise-hidden, -pairwise-hidden-gentle, lc0-halfk-threats-screlu,
   -screlu-hidden, -screlu-hidden-512, lc0-psq-threats-screlu, pesto-halfk-threats-screlu,
   pesto-psq-threats-screlu (each checked for `patience_rounds` when moved).
3. **Grid-era runs are dropped, not read:** `ArmLambda::Constant`, `lambda-<constant>` directories,
   `-l<lambda>` candidate names and their arena players are no longer parsed. The files stay on
   Drive, unread. Old candidates leave the trial and Tests forms.
4. **The `lambda:` key is removed** from every config, and a config carrying it is refused at load,
   naming the key. The attempt directory keeps its `lambda-polyline` folder, so current budget
   attempts resume and read unchanged. A `state.txt` written with the grid's `center` and `step`
   still parses (the fields ignored), for the same reason.
5. **Checked by equality before deletion,** in two commits:
   - **Commit 1:** the unstacked loop beside the stacked one, and a test that from the same seed
     both give the same losses and the same weights (within float tolerance) over a few budget
     steps, NNUE and transformer, with the polyline labels.
   - **Commit 2:** once it passes, the production path switched to the unstacked loop, then the
     stack, the equality test, the grid and patience deleted.
   - Then a short budget attempt on the training machine, its first rounds' test loss against an
     earlier attempt of the same config.

## What stays

The budget schedule (`Rule::Budget`, its kept checkpoints, which are one arm's rounds, not arms),
`export_budget_candidates`, the budget round robin among the checkpoints and the general
`round_robin.rs`, the gate, the history (`history.tsv`, its replay and `latest_of` for the page),
`round.rs`'s `run_round` and `run_test_round` (on one network), `Dirs::arm`.

## What goes or shrinks

| Place | Change |
|---|---|
| `smithy.rs` | `LambdaMode`, `grid_center`, `GRID`, `STEP_HUNDREDTHS`, `grid()`, `next_grid()`, `lambda_of()`, `GenState`'s center and step (ignored on parse), `arms_of`, `generation_arms`, the grid round robin, `fell_back`, `learned_lambdas`, `lambda_label`, the forge view's grid branch; `train` builds one network; the module doc rewritten |
| `lockstep.rs` | Becomes the one-arm loop (renamed `training.rs`): `Standing` and `Rule` budget only; `open_arm`, resume and seed, the round loop, history, checkpoints, the budget's keep; no slots, no retain, no skipping arm, no best-lambda text, no stage switch or rollback |
| `round.rs` | The `Arms` trait and its test `Trainee` go; `run_round` on one network, its labels one column |
| `cache.rs`, `buckets.rs`, `batch.rs`, `transformer_batch.rs` | Labels a scalar per position, not a `[rows, arms]` matrix |
| `freeze.rs` | One label set: no lambdas added to a frozen set |
| `preprocessing_config.rs` (fattorini) | `ArmLambda` reduced to polyline or removed with its `lambda_text`, `lambdas_text`, `lambda_dir_name` (the directory name a constant) |
| `page.html` | `trainingProgress`'s grid and stage branches, the lambda and rank columns, the lambda sort |
| `DEV.md` | `grid_center`, per-lambda candidates, "trains 9 candidates at once", patience |
| Tests | The grid's and lockstep's (`lockstep.rs` two-lambda and transformer-lambdas tests; `round.rs`'s four lockstep tests and helpers; `smithy.rs`'s five grid tests and the config test's grid half; `stacked_adamw.rs`'s; `freeze.rs`'s growing lambdas; `transformer_batch.rs`'s label columns; `buckets.rs`'s four-lambda stream), and patience's |
| `configs/` | `lambda: polyline` lines removed; patience configs archived |

## Order of work

1. **Commit 1, the unstacked loop beside the stack:** `Network::training_loss` and
   `test_loss` out of `cfg(test)` (and the transformer's); a one-network AdamW step matching
   `stacked_adamw`'s (decay, bounds such as `TRANSFORM_BOUND`, the rate schedule); the equality test.
   The developer runs tests and clippy.
2. **Commit 2, the switch and the deletion:** `train` on the unstacked loop; lockstep reduced to
   `training.rs`; the stack files, grid, patience, grid-era readers and their tests deleted; labels a
   scalar; configs and DEV.md. Tests and clippy.
3. **A short budget attempt** on the training machine (after `git pull`, the smithy and arenas
   restarted), compared with the earlier attempt's first rounds.

Each step's details are read from the code when it is written; the survey's line numbers drift.
