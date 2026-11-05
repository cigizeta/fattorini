# rotor-train: one directory per generation, and an optimizer reset at promotion

**Superseded in part, see one-learning-rate.md and lockstep-lambdas.md.** Still true and in the
code: generation numbering (N_0 is `hand-crafted`, `gen-k` trains on N_{k-1}'s games),
`self_play_source`, `LINEAGE_ROOT`, `promote`, and the weights-only reset with a warmup. Gone: the
`lr-*/`, `grid.tsv` and `epoch-NNNN/` levels (a flat `gen-NNNN/` now), lambda fixed at 0.7 (a lambda
grid in lockstep now), the migrations, and the binaries `rotor-self`, `rotor-prepare` and
`rotor-evolve` (now `rotor-arena self`, `rotor-train prepare`, `rotor-arena gate`/`select`).

## Status

Implemented 2026-09-25; the developer ran the `rotor-train`, `rotor-self` and `rotor-prepare` test
suites and all pass. The migration of amanda's existing `lr-*` directories has not been run yet. Settled with the developer the same day. First step of cleaning up the `data/` layout;
the rest of `data/` (self-play games, datasets, caches) comes later and is out of scope here.

Code: `src/train/generation.rs` (paths, current generation, staged creation, migration),
`src/train/adamw.rs` (`set_warmup_steps`), `src/train/main.rs` (`promote`, `seed_new_arms` in place
of `share_one_initialisation`, `WARMUP_STEPS`, every NNUE subcommand on the current generation).

## Decisions

- **Layout.** `data/networks/{name}/gen-{NNNN}/lr-{rate}/`, four digits, `grid.tsv` at
  `gen-{NNNN}/grid.tsv`. The directory boundary is the promotion reset, so every rule that reads
  "since the last reset" (challenger selection, loss comparability, checkpoint retention) is
  scoped by the directory, and `grid.tsv` stops ranking arms across generations whose losses are
  measured against different targets.
- **Numbering.** N_0 is `hand-crafted`, which is not a directory. `gen-k` trains on games from
  N_{k-1} and produces N_k. The first trained generation is `gen-0001`.
- **All six arms start each generation from the champion's weights** (PBT's exploit step).
- **The optimizer is reset at every promotion.** Weights only carry over; every arm starts at step
  0 with zero moments. Reasons:
  - Once all arms hold the champion's weights, each arm's own moments describe weights it no
    longer has.
  - The champion's own moments generally do not exist: checkpoints are `step_*.vs`, weights only,
    and `optimizer.opt` holds only an arm's latest step. Keeping them would mean an `.opt` per
    checkpoint, about three times the storage, to carry about 1000 steps' worth of `v`.
  - Step 0 keeps the cursor, round and epoch boundaries derived from `step_count()` alone, with no
    per-generation base step.
  - `m` forgets in about 10 steps and `v` in about 1000 (`beta1 = 0.9`, `beta2 = 0.999`), so the
    choice only affects the start of a generation's first epoch.
- **A linear learning-rate warmup** over the first `WARMUP_STEPS = 1000` steps of a fresh optimizer
  (about `1 / (1 - beta2)`). A zero-moment Adam's first updates are close to `lr * sign(g)` on
  every parameter, which jolts a trained network; the warmup absorbs that. It applies whenever the
  optimizer starts at `t = 0`, which includes a from-scratch `gen-0001`; that is harmless, and the
  existing `gen-0001` run is already far past it.
- **A failed gate does not reset anything.** The next epoch resumes in the same `gen-{NNNN}/lr-*/`
  directories, as today.

## Mechanism

- **No manual file operations.** Every transition is a `rotor-train` command; the developer never
  moves, copies or deletes anything under `data/networks/` by hand.
- **`rotor-train promote <network-name> <step> --lr <rate>` moves the network to its next
  generation**, same argument shape as `check`. It reads `gen-k/lr-{rate}/checkpoints/step_{step}.vs`
  from the current generation `gen-k` and creates `gen-(k+1)/` holding:
  - `initial.vs`: those weights, which every arm of `gen-(k+1)` starts from.
  - `initial.nnue`: the same weights exported, for naive networks (the only kind the engine
    can run). This is N_k: the network whose games `gen-(k+1)` trains on and the champion its
    gate has to beat, so `data/engines.yaml` points at it.
  - `initial.txt`: where the weights came from (generation, rate, step, test loss).

  `gen-(k+1)/` is written under a temporary name and renamed into place last, so a crash never
  leaves a half-built generation that would count as the current one. `promote` refuses a step
  that is not a retained checkpoint of the current generation, and does not touch `gen-k`, which is
  frozen from then on.
- **The current generation is the highest `gen-{NNNN}` on disk**, or `gen-0001` if there is none.
  No state file: `promote` creating the next directory is what closes the previous one.
- **Every generation's starting weights are `gen-{NNNN}/initial.vs`.** `train` on a generation
  with no arms yet loads it into every arm, with a fresh `AdamW`. `gen-0001` has no `initial.vs`
  until its first `train`, which writes the shared random initialisation there and keeps it,
  replacing `share_one_initialisation`'s temporary file. So one rule serves every generation, and
  every generation records what it started from. The existing all-fresh-or-all-resumed check is
  unchanged: the first backup writes `model.vs` and `optimizer.opt` as usual, and a crash before
  that simply seeds again from `initial.vs`.
- **`check`, `calibrate`, `correlate`** read the current generation.
- **The challenger is exported at every epoch boundary, and there is no `export` subcommand.**
  (Added the same day, revised 2026-09-26.) When `train` completes an epoch it writes the
  generation's one challenger, the lowest-test-loss checkpoint of any arm over the whole generation,
  to `gen-NNNN/epoch-NNNN/challenger.nnue`, with `challenger.txt` (rate, step, test loss,
  checkpoint), renamed into place. Naive networks only; the others are skipped with a note.
- **If that challenger comes from an earlier epoch, `train` stops and exports nothing.** The epoch
  trained and none of its rounds beat a network already gated: something is wrong with training, and
  it is the developer's to understand, not a candidate to gate a second time.
- **One gate per epoch, not a tournament.** `rotor-evolve gate <network> --threads <pairs>` plays
  the latest epoch's `challenger.nnue` against the generation's `initial.nnue`, or the hand-crafted
  evaluation in `gen-0001`, and writes `gate.db` and `gate.pgn` into the same epoch directory.
  Rerunning it resumes. The two players are built in memory, with no `data/engines.yaml` entry; the
  challenger is recorded as `{network}/gen-NNNN/epoch-NNNN` and the champion under its self-play
  source label. The SPRT constants are the `sprt:` section of `net.yaml`, all three required; the
  depth is `generation::SEARCH_DEPTH = 9`, shared with self-play; `--threads` is required.
  Tournament configs lose `decision`, `match.sprt` and `match.threads` and refuse unknown keys, so
  `data/tournaments/gate-0/` no longer loads.

  ```
  data/networks/amanda/gen-0001/
    initial.vs, grid.tsv, lr-*/
    epoch-0001/
      challenger.nnue, challenger.txt     exported by train at the end of epoch 1
      gate.db, gate.pgn                   written by rotor-evolve gate
    epoch-0002/
  ```
- **Migration from the old layout is automatic.** Every NNUE subcommand first checks the network
  directory for the old layout (`lr-*` directories and `grid.tsv` directly under it, no `gen-*`).
  If it finds one, it renames them into `gen-0001/`, prints what it moved, and continues. A
  directory holding both layouts is refused rather than guessed at. This is a one-time
  transition, and the check can be deleted once no old layout remains.
- **`AdamW`** gains a warmup: the effective rate at step `t` (after incrementing) is
  `lr * min(1, t / warmup_steps)`. `warmup_steps` defaults to 0 in `AdamW::new`, so the
  cross-check test against `tch`'s built-in AdamW is unaffected, and the NNUE track sets
  `WARMUP_STEPS`. It is derived from `t`, so a resume mid-warmup continues it correctly with no
  new persisted state.
- **The transformer track keeps `data/networks/{name}/lr-{rate}/`.** It is not in the generational
  loop, so it gets no generation level.

## Self-play games and datasets

Second step, settled and implemented the same day; its tests pass too.

- **Keyed by the generator that played the games**, the same key for `data/self/{source}/` and
  `data/datasets/{source}/`:

  ```
  data/self/hand-crafted/                    games by N_0, the lineage root
  data/self/amanda/gen-0001/                 games by N_1, the champion promoted out of networks/amanda/gen-0001
  data/datasets/hand-crafted/
  data/datasets/amanda/gen-0001/
  ```

- **`networks/{name}/gen-k` trains on `self_play_source(name, k)`**: `hand-crafted` for `k = 1`,
  `{name}/gen-{k-1}` otherwise. No mapping is stored; it follows from the generation number alone.
- **Not keyed by the consuming generation**, which would remove the off-by-one: that would stop the
  root's games being shared between every network rooted there, and games would be named by who
  consumes them while `rotor-self` only knows who plays them.
- **`rotor-self --network <name> --stem <stem> --threads <n>`**, `--network` required. It plays
  whatever generates the network's current generation's games: the hand-crafted evaluation at
  `gen-0001` (the root must be `hand-crafted`), `gen-k/initial.nnue` after that. Both player headers
  are `rotor {source}`, so provenance survives independently of paths. The one-process lock moves
  to `data/self/rotor-self.lock`, since generators now write into different directories.
- **`rotor-prepare <name>`** builds the current generation's dataset. **`rotor-train`** reads it,
  for `train`, `check`, `calibrate` and `correlate`; a missing one names the two commands that
  produce it. `run_config.txt`'s frozen `parent` field records the source actually trained on,
  which for `gen-0001` is the same `hand-crafted` it recorded before.
- **No migration**: `data/self/hand-crafted/` and `data/datasets/hand-crafted/` are already where
  the layout wants them. A `rotor-self.lock` left in `data/self/hand-crafted/` is harmless.
- **Not handled**: a machine that stopped mid-batch under the previous champion leaves that
  unfinished batch in the old directory after a promotion. Self-play idles during a generation by
  design, so the batch belongs to a dataset that is already complete. Stopping self-play at `M`
  positions is still evolutionary-training.md's prerequisite 7.

## Lambda fixed at 0.7

Third step, the same day: the budget buys one lambda, not a sweep.

- `preprocessing_config::LAMBDA = 0.7`, for every network, dataset and label file.
  `preprocessing.lambda` is gone from `net.yaml`, and a config that still sets it is refused rather
  than silently trained at the constant.
- No `lambda-{L}` directory level anywhere: `data/datasets/{source}/` and
  `data/caches/{stem}.depth{N}/labels/`.
- `labels::compute_labels` keeps lambda as a parameter: it is the recursion itself, tested at 0, 0.5
  and 1.
- `run_config.txt` still freezes `lambda`, written from the constant, so existing arms keep
  resuming.
- The existing `data/datasets/hand-crafted/lambda-0.70/` is moved up one level by hand, by the
  developer's choice.
- The transformer depth-0 experiment, which labelled at lambda 0, can no longer be rerun as it was
  without editing the constant.

## Lineage root fixed at hand-crafted

Fourth step, the same day: `preprocessing.parent` only ever held `hand-crafted`, by design.

- `generation::LINEAGE_ROOT = "hand-crafted"`, and `self_play_source(network, generation)` has no
  root argument.
- `preprocessing` is optional in `net.yaml`, and is the transformer track's alone (`pgn`, `depth`).
  `net_config::load` refuses an NNUE config that has it and a transformer config with no `pgn`.
  `configs/amanda.yaml`, `examples/net_naive.yaml` and `examples/net_nnue.yaml` have no section.
- `preprocessing.parent` is refused wherever it still appears, like `lambda`.
- `rotor-prepare` builds the self-play dataset exactly when `pgn` is empty, since it reads no
  architecture; the `net_config::load` checks make that the same as "an NNUE network".
- `run_config.txt` still freezes `parent`, as the source actually trained on: `hand-crafted` for
  `gen-0001`, as before.

## Open

- **Round-0 test pass** (the promoted weights measured on the new test cache before any step, to
  see whether the cold start hurts). Deferred: it needs a history row with no train loss and a rule
  keeping step 0 out of challenger selection, since the champion cannot be its own challenger.

## Steps

1. `AdamW` warmup, with a test of the ramp and one showing `warmup_steps = 0` matches today.
2. `network_dir`, `generation_dir`, `run_dir` split into the generation-aware NNUE path and the
   transformer's unchanged one; current-generation discovery; tests.
3. Automatic migration of the old layout, with a test on a temporary directory.
4. Seeding every generation from `gen-{NNNN}/initial.vs` in `run_train_nnue`, replacing
   `share_one_initialisation`'s temporary file.
5. `promote` subcommand and `USAGE`, with tests for the atomic creation and the refusals.
6. Doc comments and configs that name the old path: `configs/amanda.yaml`,
   `examples/net_naive.yaml`, `grid.rs`, `checkpoint_store.rs`, `net_config.rs`,
   `benches/nnue_bench.rs`, and `data/engines.yaml`'s `amanda-e5` entry, repointed at
   `gen-0001/lr-1e-3/checkpoints/step_000080896.nnue` and renamed `amanda-gen-0001`. That entry
   works only once any NNUE subcommand has run the migration.
7. Amend evolutionary-training.md and rotor-train-learning-rate-grid.md: the reset is weights
   only, and the generator identity is the generation number.
