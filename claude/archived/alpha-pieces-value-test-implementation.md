# alpha_pieces value test: implementation plan

**Superseded (2026-10-18), see transformer-streaming.md:** zelda never ran; experiment 5 replaces
her. The code this plan built is what the streaming transformer reuses.

**Status:** implemented 2026-10-15, tested. Revised by zelda-lockstep.md: every lambda in lockstep,
stacked, through amanda's loop. Departures, decided while implementing:

- `regroup` marks a finished half with `lengths/{train,test}.done`, written last and naming the
  lambdas regrouped (`dataset_paths::lengths_done_path`), instead of reading every bucket's `.count`:
  a label file has no sidecar, so only a marker written after everything covers the labels too. A
  `.done` naming other lambdas than the dataset now has is refused, asking to delete `lengths/`.
- The regrouped layout has no split level of its own: `lengths/positions` and `lengths/lambda-<l>`,
  each holding `{split}/{pieces}.*`, the layout `open_readers` already adds the split to.
- A new `length_cache::SplitBucketWriter` writes one split's buckets with every lambda's labels in one
  pass; the test-only `PositionCacheBuilder` and `LabelCacheWriter` stay for the fixtures.
- Step 5's averaging needed no fix: `run_test_pass` already weighs each batch by its rows and divides
  by the total, so the test loss is the per-position mean over every bucket.
- The stem-keyed paths (`dataset_paths::positions_dir`, `labels_dir`) and `open_readers_for_stems`
  are removed with `stem_dirs`; arena's test of the transformer example's `preprocessing` now expects
  it empty.
- zelda trains at 1.0e-3, its value head one hidden layer of 32.

## What stands and what is stale

Stands, reused unchanged: the model (`transformer_network.rs`: embeddings, blocks, value head in
pawns), the batch builder (`transformer_batch.rs`, which refuses mixed lengths), the vocabulary
(`transformer_vocab.rs`), the length-cache readers and writers (`length_cache.rs`:
`LengthCacheReader`, `open_readers`, `row_counts_of`, `pick_length_index`, `PositionCacheBuilder`,
`LabelCacheWriter`), `BucketStream` (`train_stream.rs`), and the resumable round loop
(`run_train_transformer`, `round::run_round`).

Stale, changed here:

- **The loss.** `run_train_transformer` trains with `mse_loss` on the output against the label;
  every NNUE trains, and reports test loss, with `nnue_network::matching_loss` (architectures.md,
  "Output units and training loss"). With a different loss zelda's test loss would not compare with
  amanda's.
- **The data source.** `forge`, `check`, `calibrate` and `correlate` read through `stem_dirs`
  (`preprocessing.pgn` and `depth`) and `open_readers_for_stems`, at caches nothing builds.
  `net_config::load` requires a transformer to name `preprocessing.pgn`.

## Step 1: zelda's config names its data (`net_config.rs`)

- A transformer's config gains `data: { teacher: <network-NNNN>, lambda: <l> }`, required for a
  transformer and refused for an NNUE, both directions like `training.attention`. `teacher` parses
  as `generation::Teacher`, so `hand-crafted` or `network-NNNN`, the names datasets are keyed by.
  `lambda` is validated as `label.lambda` values are (`preprocessing_config::lambda_text` round trip).
- The transformer's `preprocessing.pgn` requirement goes: `preprocessing` must be empty for a
  transformer too (`check_removed_fields` keeps refusing the retired fields).
- Tests: a transformer config with `data` loads; without it, refused; an NNUE with it, refused; a bad
  lambda, refused.

## Step 2: where regrouped buckets live (`dataset_paths.rs`)

- `lengths_dir(source)`: `data/datasets/<source>/lengths`.
- `length_positions_dir(source, split)` and `length_labels_dir(source, split, lambda)`: the layout
  `length_cache::open_readers(positions_dir, labels_dir, split)` already expects, one positions file
  and one label file per piece count, the label directory named by the lambda
  (`preprocessing_config::lambda_dir_name`).
- Tests: the paths, and that `open_readers` finds what they name (with step 3's fixture).

## Step 3: `smithy regroup <teacher>` (`regroup.rs`, new)

- Usage: `smithy regroup <hand-crafted|network-NNNN>`; the source is the teacher's dataset,
  `data/datasets/<source>`, which must be built (`dataset_cache::built_manifest`) and shuffled
  (`Manifest::is_shuffled`), else refused naming `smithy prepare`.
- For each split, one pass: read every row's position and its label at every lambda the dataset has
  (`dataset_cache`'s label sets), compute its piece count (`2 + pieces.len()`), and append the
  position to that count's `PositionCacheBuilder` and each label to that count's and lambda's
  `LabelCacheWriter`. Row order within a bucket is the dataset's.
- **Resumability:** none inside a pass. A split whose every bucket has its `.count` sidecar is done
  and skipped; a split with any missing is deleted and rebuilt from its first row. The test half
  (about 4.2M rows) first, then the train half.
- **Checks at the end of a split:** the buckets' rows sum to the split's rows in the manifest, and
  every lambda's label files have the positions files' row counts.
- Progress: one `progress` bar per split, with the uniform style.
- Refuses a dataset already regrouped whole, saying so, and does nothing.
- Tests: a fixture dataset of known positions of several piece counts and two lambdas, regrouped:
  each bucket holds exactly its count's rows, in order, with their labels at both lambdas; the sums
  check; a split interrupted (a missing `.count`) is rebuilt; an unshuffled or unbuilt dataset is
  refused.

## Step 4: the transformer track reads them (`main.rs`)

- `stem_dirs` and `open_readers_for_stems`' callers go: `forge`, `check`, `calibrate` and
  `correlate` open `length_cache::open_readers(length_positions_dir(teacher, split),
  length_labels_dir(teacher, split, lambda), split)` from `data`. A missing regrouped dataset is
  refused naming `smithy regroup <teacher>`.
- `open_readers_for_stems` and `stem_dirs` are removed if nothing else calls them (no dead code).
- The run directory stays `data/networks/zelda/lr-<rate>`.

## Step 5: the shared loss

- `run_train_transformer`'s loss becomes `nnue_network::matching_loss(&prediction, &batch.label)`,
  for training and for the test pass `run_round` runs with the same closure. Moved to a module both
  tracks use if `nnue_network` is the wrong home (`value_head.rs`, beside the head it serves).
- **The test loss is the per-position mean** over the whole test half: check how `run_round`
  averages over a bucketed test read (per batch or per row); per row is required, so a bucket of a
  few rows does not weigh as much as a full batch. Fixed if it is per batch.
- Tests: the transformer's loss equals `matching_loss` on a hand-built batch; the bucketed test mean
  equals the mean over all rows on a fixture with buckets of unequal sizes.

## Step 6: padding is harmless (`transformer_network.rs`, test only)

- A test: positions of several lengths, each evaluated alone, then padded to one length (padding
  tokens appended after the real ones) with a mask that lets no token attend to a padding token. The
  `[CLS]` outputs agree within float tolerance. LayerNorm and the feed-forward act per token, and the
  real tokens keep their positional embeddings, so only attention can leak, and the mask closes it.
  Builds the mask through the existing `attention_mask` argument; no change to the model.

## Step 7: zelda (`configs/zelda.yaml`)

- `architecture: kind: transformer`, `embedding_dim: 64`, `num_layers: 4`, `num_heads: 4`,
  `feedforward_dim: 256`, `dropout: 0.0`; the value head's hidden widths as the model expects (one
  layer of 32, unless the code's default says otherwise).
- `training`: `attention: full`, the batch size and round length of the NNUE configs (4096 and 1024
  steps a round), the test batch 16384.
- `data: teacher: amanda-0002, lambda: 0.95`, generation 2's winning lambda, until generation 3's
  selection names amanda's: `regroup` keeps every lambda, so changing it later needs no new pass.
- Comments in the file say why each value is what it is, as the other configs do.
- Test: the committed config loads (as every config's).

## Running it, for the developer

    cargo test --release -p smithy

    bin/smithy regroup amanda-0002

    bin/smithy forge zelda

On the Lichess machine the device is the CPU (`cuda_if_available`). Then zelda's `history.tsv` against amanda's
generation-3 runs, test loss against positions seen (steps x 4096).

## Out of scope

The policy head, MCTS, engine inference, padded batches in training, the medium network.
