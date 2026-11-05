# zelda in lockstep: every lambda at once, stacked

**Superseded (2026-10-18), see transformer-streaming.md:** zelda never ran; experiment 5 replaces
her, its lambdas stacked in lockstep as planned here.

**Status:** implemented 2026-10-15, not yet compiled or tested. Revises
alpha-pieces-value-test-implementation.md, whose steps 1 to 7 stand except where this plan says
otherwise. Departures, decided while implementing:

- Step 5's stack trait became a `StackedModel` trait over the network (`StackedNetwork`,
  `StackedTransformer`), and `Stack` generic over it: one `Stack`, one `Arms` impl, one `open_stack`,
  no second stack type. `train_lambdas_in_lockstep` keeps its signature as the NNUE's wrapper around
  the generic `train_in_lockstep`, so amanda's lockstep test is unchanged.
- The regrouped lambdas are read from both `.done` files by `regroup::regrouped_lambdas`, which refuses
  halves regrouped at different lambdas.
- With `run_train_transformer`'s one-network loop gone, `round::Trainee` and `adamw::AdamW` are
  test-only (the round's tests and `StackedAdamW`'s reference), `nnue_network::matching_loss` is
  test-only again, and `early_stopping.rs` is unused, to be removed with `git rm`.
- `transformer_network.rs` allows dead code outside the tests: the binary builds it only for its
  parameters, and its forward passes are the stack's test reference and inference's future entry.
- zelda takes amanda's rates without the first, from 1e-3: 3e-3 is high even for an NNUE starting from
  its champion. Her batch, round, warmup and patience stay amanda's.

## Revision: zelda is generational (2026-10-15, implemented, not yet tested)

Departures: a leftover `data:` is refused rather than ignored (the file's unknown sections are
otherwise ignored); `run_train_nnue` became `run_train_generation` for both kinds, choosing
`train_lambdas_in_lockstep` or `train_transformer_lambdas`; the test runs zelda's fixture network at
generation 2 from a generation-1 champion, as amanda's lockstep test does, and checks that both
lambdas reach `candidate.txt` without a `.nnue` (a refused second `forge` exits the process, so it is
not tested).

zelda trains as generation 1 from amanda-0002, as caroline and dorothy did: a teacher in
`gen-0001/teacher.txt`, not a `data:` section. Only her network and her data path differ from an NNUE's.

- **Config:** `data:` goes, from `net_config.rs` too (`Data`, its validation, its tests). zelda gets
  `label.lambda` with amanda's grid, 0.90 to 1.00, so `prepare` finds every label set built and does
  nothing.
- **Teacher and dataset:** `smithy teacher zelda amanda-0002`, then `smithy prepare zelda`, which copies
  `teacher.txt` into `gen-0001/` and, the dataset of amanda/gen-0002 being built and labelled at every
  lambda of the grid, returns without touching the corpus (`prepare_dataset`'s early return). Then
  `smithy regroup amanda-0002`.
- **`run_train_transformer`** follows `run_train_nnue`: `current_generation`, `training_state_check`,
  the lambdas of `label.lambda` with a candidate skipped, the source from `dataset_source`. The
  regrouped dataset must hold every pending lambda (`regroup::regrouped_lambdas`), else refused naming
  `smithy regroup <teacher>`; its readers open the pending lambdas' label folders, in their order.
- **Runs** in `gen-0001/lambda-<l>/`; the starting weights through `seed_network`, which saves
  `gen-0001/initial.vs` for a first generation, built through an unstacked `TransformerNetwork`.
  `transformer_starting_weights` goes.
- **The end of a run:** `finish_training`, whose `export_candidate` already writes `candidate.txt`
  without a `.nnue` for a transformer, so a finished lambda is skipped and the generation is never
  trained again, as an NNUE's. zelda's own `finish` goes. `export_candidate`'s last line no longer
  claims a `.nnue` it did not write.
- **`train_in_lockstep`** stays generic; the two callers differ only in the stack, the readers and the
  batch. `LockstepParts::grid` becomes `label.lambda` for both.
- **Unchanged:** `promote`, `upload` and `check`/`calibrate`/`correlate` refuse a transformer.
- **Test:** zelda's lockstep test runs a generation-1 transformer with a `teacher.txt` and a regrouped
  fixture: both lambdas reach `candidate.txt`, and a second `forge` is refused by
  `training_state_check`.

## Why

zelda is compared with amanda, and amanda's generation trains every lambda of its dataset at once and
then picks the best (`train_lambdas_in_lockstep`). zelda trained one guessed lambda through her own
loop (`run_train_transformer`), so a worse test loss could only mean the guess suited her badly. Now
zelda goes through amanda's loop, with her lambdas stacked into one network as amanda's arms are,
because speed comes first: one forward and one backward pass a step for every lambda, not one per
lambda.

What zelda gains from amanda's loop, unchanged: one shuffled stream; each lambda's own `history.tsv`,
checkpoints, backups and resume; warmup, then rates stepped down on patience, then a stop; a finished
lambda leaving the stack while the others go on; the round block, one line per lambda. And the same
schedule as amanda, so the two compare at equal positions seen under equal rules.

## Step 1: zelda's config (`net_config.rs`, `configs/zelda.yaml`)

- `data` keeps only `teacher`: the lambdas are every one the regrouped dataset has, named by its
  `lengths/{train,test}.done` markers (both must name the same lambdas, else refused).
- A transformer requires `training.learning_rate` as a list, `warmup_rounds` and `patience_rounds`, as
  an NNUE does; `shuffle_buffer` stays NNUE-only (the bucket stream needs none).
- `zelda.yaml` takes amanda's rates, warmup and patience, with a comment saying why, and dropout 0.1,
  BERT's rate: on while training, off in the test pass and in inference.
- Tests: zelda loads; a transformer with `data.lambda` or without the schedule is refused.

## Step 2: length buckets carry every lambda's label (`length_cache.rs`)

- `LengthCacheReader` holds one label file per lambda, read in step with the positions, and gives
  lockstep records (`CacheRecord` with one label per arm), as `CacheFile::open_lockstep` does for the
  flat cache. `open(positions_dir, label_sets: &[(String, f64)], split, piece_count)`; each label file's
  row count is checked against the bucket's.
- `open_readers` takes the label sets likewise. The one-label callers pass one set.
- Tests: a lockstep bucket read gives every lambda's label and matches each one-label read; a label file
  of another length is refused.

## Step 3: lockstep transformer batches (`transformer_batch.rs`)

- `TransformerBatch::label` becomes `[batch, lambdas]`, column `k` the `k`-th label of every record, as
  a `DenseBatch`'s labels are.
- The batch is split into a host half and an upload half, as `batch::host_batch` and `batch::upload`
  are, so `round::BatchBuilder` prepares batches on every core and the test cache can keep them.
- Tests: the labels' shape and columns; host then upload equals the direct build.

## Step 4: the stacked transformer (`stacked_transformer.rs`, new)

`TransformerNetwork`'s function arm by arm, every parameter one tensor with a leading arm dimension,
under the names an unstacked `TransformerNetwork`'s `VarStore` gives it, built once per arm and
stacked (as `StackedNetwork::with_head_shape` does), so names and shapes cannot drift.

- **Embeddings:** the token table `[arms, vocab, width]` viewed as `[vocab, arms * width]`: one lookup
  serves every arm, since the tokens are the same for all. Plus each arm's positional embedding.
  Hidden state `[arms, batch, length, width]`.
- **Linear layers** (`qkv`, `proj`, `fc1`, `fc2`, the value head): `baddbmm` over the arm dimension,
  the rows flattened to `[arms, batch * length, in]`.
- **Attention:** the arms folded into the batch dimension, `[arms * batch, heads, length, head_dim]`,
  through `scaled_dot_product_attention`, the same fused kernel as now. The kings-only mask is
  repeated over the arms.
- **LayerNorm:** `layer_norm` without affine over the width, then each arm's weight and bias.
- **The last block reads only `[CLS]`:** keys and values come from every token, but the query, the
  projection, the feed-forward part and both norms run on `[CLS]` alone, since the value head reads
  nothing else. That cuts the last block's cost by about the sequence length, for one of four blocks.
- **Dropout,** where `TransformerNetwork` has it: after the embeddings, on the attention
  probabilities (`scaled_dot_product_attention`'s `dropout_p`), after `proj` and after `fc2`, only
  while training. Every element draws its own mask, so each arm drops independently, as separate
  networks would. The test pass runs without it.
- **Losses:** `training_losses(batch, columns)` and `test_losses(batch, columns)`, `[arms]`, with
  `StackedNetwork`'s matching loss and label-space MSE, moved to a shared helper both stacks call so
  the loss cannot differ between amanda and zelda.
- `retain(keep)`, `parameters()`, as `StackedNetwork`'s.
- Tests:
  - arm `k` gives what an unstacked `TransformerNetwork` with arm `k`'s weights gives, full and
    kings-only, on positions of several lengths (one batch each), without dropout;
  - with dropout, training forwards differ between calls and between arms with equal weights, and
    evaluation forwards do not;
  - the last block read on `[CLS]` alone equals the full block's `[CLS]` row;
  - arm `k`'s gradient equals its unstacked loss's gradient;
  - `retain` keeps the right arms' weights.

## Step 5: amanda's loop takes its parts as parameters (`main.rs`)

`train_lambdas_in_lockstep` becomes generic over what differs between the two tracks, every other
line staying shared:

- **The stack:** a trait beside `round::Arms`, implemented by `Stack` (NNUE) and a new transformer
  stack over `StackedTransformer`: `save_slot`, `retain`, `set_learning_rate`, `learning_rate`,
  `step_count`, saving a slot's checkpoint. Both use `StackedAdamW` and `arm_slice` unchanged.
- **The runs:** the lambdas and each one's directory. amanda: `gen-NNNN/lambda-<l>/` and `label.lambda`.
  zelda: `data/networks/zelda/lambda-<l>/` and the dataset's lambdas, no generation.
- **The starting weights:** amanda's `seed_network`; zelda's `data/networks/zelda/initial.vs`, saved
  at the first start, every lambda from it.
- **The streams:** amanda's `ShuffledStream` and flat test reader; zelda's `BucketStream` and the
  length test readers. zelda's stream resumes from persisted cursors: a short bucket batch makes the
  position underivable from the step count, so her backups write the stream's cursors into
  `round_progress.state`, and a resume takes those of an arm at the lowest step.
- **The end of a run:** amanda exports a candidate for the gate; zelda's run ends at its best
  checkpoint, named in a line, with no export.
- `run_train_nnue` and a new `run_train_zelda` (the transformer's `forge`) each build their parts and
  call the loop.
- Tests: amanda's lockstep tests pass unchanged; two zelda lambdas on a fixture train to their stops,
  each with its own history, and a run interrupted and resumed reaches the same step counts.

## Step 6: what goes

- `run_train_transformer`, its `--patience`, `--min-steps` and learning-rate flags, `early_stopping.state`
  and the `lr-<rate>` run directory.
- `check`, `calibrate` and `correlate` refuse a transformer for now: the value test reads
  `history.tsv`, and they read one network, so they come back when zelda's winner is wanted.
- The unstacked `TransformerNetwork` stays: the stack is built from it, its tests are the stack's
  reference, and inference will use it.

## Step 7: measure before training

On the Lichess machine, after the tests: the stacked steps a second against one unstacked lambda's, from the round
lines' training seconds, so we know what the stack bought and what a round costs.

## Running it, for the developer

    cargo test --release -p smithy

    bin/smithy regroup amanda-0002

    bin/smithy forge zelda

## Out of scope

Mixed precision (bf16 on the training machine's GPU), worth measuring once zelda runs; the policy head; inference.
