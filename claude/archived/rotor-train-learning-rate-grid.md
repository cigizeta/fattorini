# A learning-rate grid in net.yaml

**Superseded 2026-09-28, see one-learning-rate.md:** the grid is gone (`grid.rs`, arms, `lr-*/`,
`grid.tsv`, `--lr` on the inspect commands); one run steps down a list of rates. Its lockstep idea
returned as lockstep-lambdas.md (lambdas in place of rates). Kept for the GPU measurement below,
which TODO.md cites.

## Status

**Implemented**, in the shape below: `training.learning_rate` accepts a list, `--lr` is gone from
`train`, and one `rotor-train train` invocation runs every rate in lockstep until all of them have
completed one epoch.

What landed, beyond or against the design below:

- **`--lr` is removed from `train` and kept on `check`/`calibrate`/`correlate`.** The grid is the whole
  job now, so a flag naming one rate could only describe one arm of it; the three inspect commands each
  read one arm's checkpoint, so they still need it.
- **The transformer track takes exactly one rate**, asserted in `net_config::load` rather than silently
  using the first. It has no epoch, no gate and no population to select from, and whether six
  transformers fit on the GPU at once is a question worth not answering yet. It runs the same round loop
  with a one-element arm slice.
- **`round.rs` grew `Arm` and `RoundOutcome`.** An arm is an optimizer borrow plus two closures (the loss
  over an already-built batch, and its own backup); the batch itself is built once per step by
  `run_round`'s `to_batch` and handed to every arm, which is what makes the shared decode real rather
  than nominal. The test pass works the same way: one read of the test cache, every arm evaluated on
  each batch, one mean loss per arm.
- **The arms' shared step count is asserted, not assumed** (`shared_step_count`). Lockstep means all
  arms are always at the same count, so the round and epoch boundaries are conditions on that one
  number; a disagreement means a hand-assembled run directory and is fatal.
- **A grid is entirely fresh or entirely resumed**, also asserted. Lockstep has no meaning for a grid
  where one arm has a generation of training the others do not, and silently starting the missing ones
  from scratch would produce exactly that.
- **Identical initial weights come from saving the first arm's store and loading it into the others**,
  through a temporary `initial.vs` that is removed immediately, rather than from re-seeding each arm's
  initialiser. It needs nothing from the initialiser and cannot drift from it.
- **`grid.tsv` is rewritten after every round**, not only at the end, so an interrupted run still leaves
  readable standings. Columns: rate, step, rounds, final test loss, best test loss, **best step**. That
  last one is what makes the file actionable rather than merely informative: an arm's candidate is the
  checkpoint at its best step, `checkpoints/step_{best_step:09}.vs`, so the pair names a file the gate
  can load, and without it recovering that step means re-reading the arm's whole `history.tsv`. There is
  no `diverged` column, because -- see below -- there is no divergence guard yet.
- **No divergence guard.** The design's "abandon an arm whose loss goes non-finite" is *not*
  implemented. It would need a per-arm active flag, and a diverged arm that stops stepping breaks the
  single shared step count that the boundaries and the resume all rest on. A `NaN` arm currently wastes
  its share of the GPU for the rest of the epoch and records `NaN` in its own history, which is visible
  in `grid.tsv` and costs nothing else. Worth doing when the top rung of a bracketed grid actually
  starts failing; it needs a rule for "rises far above its own best" anyway, which is a policy decision
  and not an implementation one.
- **No promotion reset.** "On a promotion, every arm adopts the promoted checkpoint's weights and
  optimizer state" belongs to the gate, which does not exist yet. Nothing here decides a promotion.
  (Since built as `rotor-train promote`, weights only, one directory per generation: see
  rotor-train-generations.md. The temporary `initial.vs` above is now kept, as the generation's
  record of what it started from.)
- **Both durations in an arm's `history.tsv` are the round's, not the arm's.** The arms are trained and
  tested together over one decode, so there is no per-arm time to record, and a per-arm figure would be
  a sixth of one number rather than a measurement.

## Why

The learning rate is the one training parameter that cannot be reasoned into place. amanda's was
set against batch 4096 and the batch is now 1024; rescaling rules say roughly where to look, not
what to use.

Rather than settle it in a separate experiment and commit, evolutionary-training.md's loop carries
the whole grid as a population: every generation trains all six rates, the gate picks a winner, and
on a promotion every arm resets to the winner's weights and optimizer state. That is Population
Based Training with a fixed grid standing in for mutation. See "The learning-rate population" there
for the selection rules and for when the grid retires; this document covers only how `rotor-train`
expresses and runs the arms.

Two consequences shape everything below. **The grid is permanent infrastructure, not an experiment
harness** -- it runs every generation until it retires, so its resume and retention behaviour is
load-bearing rather than incidental. And **arms run in lockstep, not sequentially**, which reverses
an earlier decision here; the reasoning is under "What an arm is".

amanda's grid today:

```yaml
training:
  learning_rate: [3.0e-3, 1.0e-3, 3.0e-4, 1.0e-4, 3.0e-5, 1.0e-5]
```

## Schema

- **`learning_rate` accepts a scalar or a sequence, and both become `Vec<f64>`.** A scalar
  deserializes to a one-element vector, so there is one code path rather than two and every
  existing config keeps working unchanged. `net_config.rs`'s `Training.learning_rate` changes type
  from `f64` to `Vec<f64>`, with an untagged one-or-many helper doing the widening.
- **The four call sites that read it become arm-aware.** `main.rs` passes
  `config.training.learning_rate` straight to `AdamW::new` in four places; each takes the current
  arm's rate instead.
- **An empty list is a config error**, asserted at load beside the existing
  `training.attention`/`architecture.kind` check, since a run with no rate to train at has nothing
  to do.
- **Duplicate rates are a config error too.** Two arms at the same rate would collide on their run
  directory, so this is caught at load rather than discovered as a corrupted resume.
- **Order is preserved, not sorted.** Arms run in the order written, so the developer decides which
  runs first, and a grid can be front-loaded with the rates it most wants an answer about.

## Run layout

`run_dir(config_path)` is `data/networks/{name}` today, holding `checkpoints/`, `history.tsv` and
the model, optimizer and round-progress state. An arm level goes underneath it:

```
data/networks/amanda/
  grid.tsv
  lr-3e-3/
    checkpoints/
    history.tsv
    ...
  lr-1e-3/
    ...
```

The arm level already exists: `run_dir` is `data/networks/{name}/lr-{rate:e}` today, one directory
per `--lr` value, formatted with `{:e}` so `--lr 3e-4` and `--lr 0.0003` name the same one. What is
missing is the grid running them together rather than one invocation per rate.

- **The directory is a function of the rate alone**, under one canonical formatter used for both
  writing and lookup. Not of the arm's index: reordering the list in the config would otherwise
  shift every arm onto another arm's state.
- **The arm level exists even for a single rate.** One layout, not two. This moves where an
  existing single-rate run keeps its files, which is worth doing once rather than carrying a
  special case forever.

## What an arm is

- **Its own `VarStore` and `AdamW`** at that arm's rate.
- **Identical starting weights across arms**, which the loop now supplies for free: on the first
  generation every arm starts from the same initialisation, and on every later one from the same
  promoted checkpoint. A difference between two arms is then attributable to the rate and not to
  where they started.
- **Arms run in lockstep over one shared batch stream.** Every arm consumes the same rows in the
  same order from a single decode of the cache, stepping all six networks on each batch before
  moving to the next. This reverses the earlier sequential design, for two reasons that only
  appeared once the grid became part of the loop. Decompressing and dense-expanding the cache is
  the dominant per-epoch CPU cost, and running arms one after another would pay it six times
  instead of once. And identical batch order makes the comparison between arms exact rather than
  merely fair, which matters now that the comparison decides a promotion rather than settling a
  parameter once.
- **One set of caches, opened once.** `prepare` does not read `learning_rate`, so the expensive
  part is shared by construction.
- **The same budget for every arm**, which lockstep enforces by construction.

## Divergence guard

An arm whose test loss goes non-finite, or rises far above its own best, is abandoned and recorded
as diverged rather than spending its remaining budget. A grid bracketed on purpose contains rates
expected to fail, so this is the normal path for the top rung, not an error case.

## The reset

**Superseded 2026-09-25, see rotor-train-generations.md: the reset is weights only.** Every arm of
the new generation starts from the champion's weights with a fresh optimizer and a linear warmup over
its first 1000 steps. Once all arms hold the champion's weights, each arm's own moments describe
weights it no longer has. The champion's moments generally do not exist either: `checkpoints/` holds
weights only, and `optimizer.opt` only an arm's latest step. And a fresh optimizer restarts the step
count, which keeps the cursor and both boundaries derivable from it alone. The transient the text
below worries about is confined to about `1 / (1 - beta2) = 1000` steps, which the warmup absorbs.
The original reasoning is kept below for the record.

On a promotion, every arm adopts the promoted checkpoint's **weights and optimizer state
together**, then continues at its own rate. `training.md` records that full
weights-plus-optimizer-state checkpointing already exists via `Tensor::save_multi`/`load_multi`,
which is exactly what this needs.

Carrying the optimizer state rather than reinitialising it is deliberate. AdamW's moment estimates
track gradient magnitudes, which are a property of the loss surface at that point in training and
not of the learning rate that got there, so they transfer coherently to an arm at a different rate.
Resetting them instead would give every arm a transient at the start of every generation, and those
transients would be the loudest signal in the first rounds after a reset -- precisely the rounds
the next comparison reads.

## Retention and resume

- **Checkpoint retention needs no logic change.** `BEST_CHECKPOINTS_KEPT` pruning
  (rotor-train-checkpoint-retention.md) already operates on a `checkpoints_dir`; once that
  directory is per arm, retention is per arm.
- **Resume is per round, not per arm.** Lockstep means all six arms are always at the same round,
  so a run resumes at one round boundary with all six restored, rather than walking arms in order
  as the sequential design would have. The arms' directories still hold their own state; what
  changes is that they advance together.

## Reporting

One row per arm in `data/networks/{name}/grid.tsv`: rate, rounds completed, final test loss, best
test loss, and whether it diverged. Its job is to make the nomination mechanical rather than a
matter of reading six `history.tsv` files side by side, and to make the winner history readable
across generations, which is what the retire-and-re-centre rules in evolutionary-training.md are
read from.

## Out of scope

- **No promotion decision.** Test loss nominates the best two or three arms; the gate decides.
  evolutionary-training.md explains why test loss is not trustworthy enough to select on outright:
  the per-position split leaves each test position's own neighbours in the train cache.
- **No gate games.** Playing the nominees against the champion is `rotor-evolve`'s job.
- **`prepare` is untouched.**

## Open

- ~~The canonical formatting of a rate in a directory name~~ -- settled and implemented: `{:e}`, which
  prints the shortest representation that round-trips, so two spellings of one rate never make two
  directories, and `net_config::load` rejects a grid that names one rate twice under that spelling.
- ~~Whether an arm's per-generation state is kept per generation or overwritten in place~~ --
  settled: per generation, `data/networks/{name}/gen-NNNN/lr-{rate}/`, with `grid.tsv` in the
  generation directory (rotor-train-generations.md).
- ~~Whether moving existing single-rate runs under an arm directory needs a migration note~~ --
  settled: the move into `gen-0001/` is done automatically by the first NNUE subcommand run.
- How many generations of the same winner should trigger retiring the grid, or re-centring it when
  that winner is an edge rate. Both rules are the developer's call to make from `grid.tsv`, not
  something the loop should automate.
- ~~Whether the six arms fit on the GPU at once~~ -- settled by measurement, not by feel. amanda's
  naive net is **277,505 parameters** (`204 x 512` transform plus the 256/128/64 value head), so per arm
  that is 1.11 MB of weights, 1.11 MB of gradients and 2.22 MB of AdamW moments: **4.4 MB, or ~27 MB for
  six**, against a CUDA context an order of magnitude larger. Activations do not multiply -- only one
  arm's graph is alive at a time, since each arm forwards, backpropagates, steps and drops it before the
  next arm starts -- so the transient peak is one arm's (~4 MB at batch 1024) plus the one shared dense
  batch (0.84 MB training, 13.4 MB for a 16,384-row test batch). It becomes a real question at a larger
  architecture, not at this one.

  **Why the grid is cheap, measured on the first six-arm run**: **32% GPU utilization and 0.2 GB of GPU
  memory**, at batch 1024 and about 16 s per 512-step round. The 0.2 GB confirms the breakdown above --
  the ~27 MB of state is inside the CUDA context's own rounding. The 32% is the useful number:
  `nvidia-smi`'s utilization is the fraction of sampled time with at least one kernel running, so the
  card had *no work at all* for roughly two thirds of the wall clock. That rules out "kernels too small
  to fill the GPU" as the story and names the gaps between them instead -- per-op dispatch, launch
  latency, and the CPU-side data path. It fits the arithmetic: one arm-step is five linear layers
  forward, their backward passes and AdamW over ten parameter tensors, so fifty-plus mostly trivial
  kernels per arm and several hundred per batch across six, while the real arithmetic is about 1 GFLOP
  against a card that does tens of TFLOP/s.

  So arms cost launches, not compute, and a wider grid is nearly free. The speedup, when the wall clock
  starts to matter, is to stack the arms: six `[204, 512]` transforms over one shared input is a single
  `baddbmm` over a `[6, 204, 512]` weight stack, and the same per value-head layer, turning `6N` launches
  into `N` wider ones -- close to free on a launch-bound workload, so the grid would cost roughly what
  one arm costs. The price is one stacked model with a leading arm dimension instead of six `VarStore`s,
  plus a hand-rolled per-arm AdamW. Carried in `TODO.md`; not worth doing while an epoch is 8.5 minutes.

  The same measurement says something about the architecture rather than the grid: `embedding_dim: 512`
  over a 204-row input leaves the card idle two thirds of the time, so a wider transform or a deeper
  value head would cost far less than proportionally. A sizing question for `rotor-naive.md`, to be
  measured rather than assumed.

- **How much the shared decode actually saves is unmeasured** on this code path. The only single-arm
  round time on record (2.9 s) was measured under the retired streaming pipeline, whose data path was
  live PGN replay rather than a zstd cache read, so it cannot be subtracted from the grid's 14.8 s to
  attribute anything. One two-round run with a scalar `learning_rate` settles it.
