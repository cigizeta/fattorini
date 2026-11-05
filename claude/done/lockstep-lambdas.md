# Lockstep lambdas: every lambda of a generation trained over one shared batch stream

Agreed with the developer on 2026-10-03. Status: steps 1-6 implemented and tested on the Lichess machine
(2026-10-03), including an end-to-end test on a throwaway `zulu` network. Merged to `main`. Left: the first real run,
caroline's generation 1 (caroline-generation-1.md); then this doc moves to `done/`. The betty below
is the retired big five lineage, not today's.

## Why

`rotor-train train` trains the lambdas of `label.lambda` one after another (per-lambda-training.md):
six full passes of batch building over the same positions. Betty's generation 1 runs at about 6%
GPU, her wall clock nearly all batch building (the big five's `position_inputs`); generation 2's
corpus is 3.5x larger, about 90 h for five lambdas at today's speed (TODO.md).

The lambdas of a generation share their positions: the dataset stores them once, with one label file
per lambda in the same row order (`dataset_cache.rs`). So one batch, built once, can carry every
lambda's labels, and every lambda can step on it.

This is the learning-rate grid's lockstep (evolutionary-training.md, "All six arms train in lockstep
... from one shared decode of the cache"; rotor-train-learning-rate-grid.md), which ran in
production, with lambdas as the arms instead of rates. The developer judged it proven: no benchmark
comparison against the sequential code is planned, only the tests below.

Not this plan: stacking the arms into one model with batched matmuls (`baddbmm`). Implemented
later, once one network per arm turned out to be bound by kernel launches: see
stacked-lockstep-arms.md. The arms below are now slices of that stack while they train; their files
are unchanged.

## Design

### Arms

An arm is one lambda's run, exactly as today: its own `VarStore`, `Network`, `AdamW`, rate stage,
patience and stop, and its own `gen-NNNN/lambda-<l>/` files (`model.vs`, `optimizer.opt`,
`history.tsv`, `round_progress.state`, `run_config.txt`, `checkpoints/`, `candidate.*`). `select`,
`export`, `check`, `calibrate`, `correlate` and `promote` see nothing new.

Every arm starts from the same weights, as today. All arms' networks live on the GPU at once: a few
MB each (rotor-train-learning-rate-grid.md measured six naive arms at 0.2 GB in all).

### One batch, every label

- **Records carry one label per arm.** A reader that opens the positions file and every arm's label
  file of the split, read in lockstep (today's `CacheFile` reads one label file beside the
  positions), yields records with `labels: Vec<f32>` in arm order. `CacheRecord` itself keeps its
  one `label` for the other readers (the transformer's, `check` and the others); the multi-label
  record is a sibling type, or `CacheRecord` grows a label vector: settled when implementing, by
  whichever touches less.
- **The host batch carries a `[rows, arms]` label matrix** instead of `[rows]`; the features are
  built once. `DenseBatch::label()` becomes `label(arm)`, a column of the uploaded `[rows, arms]`
  tensor, and `Network::training_loss`/`test_loss` take the arm.
- **The test cache (test-features-once.md) is shared by every arm**: one prepared test batch holds
  every label column, 4 bytes a position per arm more.

### The shuffled stream

One `ShuffledStream` for the whole generation, as the grid had: every arm sees the same rows in the
same order, so the arms differ only in their labels. Seeded at random, as today.

### Lockstep, with arms that stop at different times

Each step: one batch, then each active arm in arm order does its forward pass, backward pass and
optimizer step on it, with its own label column and rate. A round ends for every arm at once (same
`steps_per_round`, same step count); then each active arm runs its test pass loss over the shared test
batches (one upload per batch, one forward pass per arm), appends its history row, saves its
checkpoint, prunes, and replays its stages. An arm whose stop rule now holds exports its candidate
(`finish_training`, as today) and leaves the active set; the rest go on. `train` ends when no arm is
active.

### Backup and resume

Backups save every active arm's files at the same moments, as today's backup does for one.

The stream's position is derived from the step count, as today. A resume finds the active arms
(those with no `candidate.txt`) and their step counts; these are normally equal, but a crash between
two arms' backups, or a generation begun under the sequential code, leaves them unequal. The rule:
the stream restarts at the **lowest** step count among active arms, and an arm further ahead skips
the batches it has already stepped on until the stream reaches its own count, then joins. Every arm
thus steps on each batch of its sequence exactly once (up to the shuffle buffer's own restart
inexactness, which today's resume already accepts).

A round's `round_progress.state` (loss sum, training seconds) stays per arm.

### Progress line and log

One progress line for the generation (round, step, active arms); after each round, one line per
active arm with its losses, best and stage, as today's per-run line, prefixed by its lambda.

### What goes

`train_lambda`'s per-lambda loop in `run_train_nnue`. The transformer track is unchanged: it trains
one run, and `run_round` keeps serving it with one arm.

## Implementation steps

Each step its own commit, tests passing.

1. **Multi-label reader**: positions plus every arm's label file, lockstep, refusing files of
   different lengths. Test: its records equal, arm by arm, today's single-label reader's.
2. **Label matrix in the host batch and `DenseBatch::label(arm)`**, for every NNUE kind; the
   single-lambda callers (`check`, `calibrate`, `correlate`, export) use arm 0. Test: column `k` equals
   the batch built today from lambda `k`'s labels.
3. **`run_round` over arms**: `Trainee`s as a slice, a training step per active arm per batch, a test
   pass per arm over shared batches. Test: two arms in lockstep end with the same weights and losses
   as two sequential runs over the same batch sequence and starting weights (CPU, a few steps; exact
   if achievable, else a tight tolerance).
4. **`run_train_nnue` in lockstep**: the arms, the shared stream and test cache, per-arm history,
   stages, export and leaving the active set. Tests: an arm that stops leaves its weights and files
   untouched while the others continue; `train` ends when every arm has its candidate.
5. **Resume**: from equal step counts, and from unequal ones (the ahead arm skips, then joins). Test:
   an interrupted and resumed run ends as an uninterrupted one.
6. Delete what step 4 replaced; update per-lambda-training.md's "trains every lambda in turn" with a
   pointer here, and the TODO's training-wall-clock items.

## When

Implemented and merged. Caroline's generation 1 is the first run trained in lockstep.
