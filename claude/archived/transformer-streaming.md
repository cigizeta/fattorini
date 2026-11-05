# transformer-streaming

**Archived 2026-11-04: implemented 2026-10-17, never verified by a run; the transformer experiment
was abandoned and its configs archived.**

## Goal

Experiment 5 (TODO's "next network experiments"): the transformer, value head only, trained on
both labels, Lc0's (`data/lc0`) and Rotor's self-play (`labels: self`). The NNUEs already stream
their PGN segments (`pgn_train_stream.rs`). The transformer still reads the length-bucketed files
`smithy regroup` builds from a built dataset (`labels: archives`). This plan makes it stream as the
NNUEs do.

## Decisions (the developer, 2026-10-17)

- **Batching by length in memory.** The transformer cannot pad, so every batch comes from one
  piece count. Instead of length files on disk, the training stream keeps one shuffle buffer per
  piece count.
- **K vs K is skipped.** The search scores insufficient material as a draw before evaluating, so
  the network never sees it. The converter keeps every game's final position, so K vs K does occur
  in the corpus, rarely. A board holds 2 to 32 pieces, kings included, so that leaves 30 buckets: 3
  to 32 pieces (4 to 33 tokens with `[CLS]`). Every other draw by insufficient material, K+N vs K
  for instance, is kept. (First written as 31 buckets of 3 to 33 pieces, after archives-removal.md's
  "2 to 33", which was one too many: corrected 2026-10-17.)
- **A bucket emits when it is full.** Each bucket's capacity is set from the first fill, in
  proportion to its piece count's share, with a floor. A bucket that reaches its capacity emits one
  full batch drawn at random from its records. In rare buckets, records may wait a long time and
  carry over between epochs, but none is lost or duplicated.
- **The test set is grouped by piece count in memory, for both tracks.** The NNUE and transformer
  runs are then judged on the same batches.
- **K vs K leaves the frozen test sets too.** `smithy freeze` skips it while sampling, and the
  existing frozen sets, lc0's and self's, are frozen again.
- **The epoch handling is shared.** The two streams share one record source rather than each
  holding a copy of it.
- **The archives path is removed afterwards, in a separate change** (archives-removal.md, "Left"),
  once the streaming transformer has trained for a while.
- **zelda becomes `lc0-alpha-pieces` and `self-alpha-pieces`:** her shape, named by the
  experiments' rule (labels, then architecture). zelda's own config goes with the archives path.

## Design

### The training stream: `BucketedTrainStream`

A new `TrainStream`, beside `PgnTrainStream`, over the same `PgnStream` and the same epoch rules:
files reshuffled each epoch, a resume derived from the step count.

1. **Routing.** Each record read goes to the bucket of its piece count,
   `record.canonical.pieces.len() + 2`. K vs K (`pieces` empty) is dropped there, after
   `EpochRecords` has counted it, so the epoch's count stays exact.
2. **The first fill.** `training.shuffle_buffer` records (2,097,152 in the experiments' configs)
   are read into the buckets with nothing emitted. Each bucket's capacity is then set to
   `max(what it holds, 2 x training_batch_size)`. The total stays near `shuffle_buffer`, plus at
   most the floors of the rare buckets.
3. **Emission.** `next_batch` reads records into their buckets until one reaches its capacity,
   then draws `training_batch_size` records from that bucket uniformly at random without
   replacement, and returns them. Each piece count then gets batches at the rate its records
   arrive, so over a run the mix of lengths matches the data.
4. **Batches are always full.** A resume is therefore derived from the step count as the NNUE's
   is (`emitted = batch_size x steps`, `persists_position: false`, `cursors()` empty). After a
   restart, the first fill is read afresh. The records held in the buckets at the stop are not
   replayed: the same leeway `pgn_train_stream.rs` already documents for the NNUE ("Resume may
   retrain").
5. **Seeds.** The same epoch seed rule as `PgnTrainStream`, with one generator for the
   draws within buckets.

### The shared record source: `EpochRecords`

`PgnTrainStream` does two jobs today. It supplies records endlessly, epoch after epoch, and it
shuffles them through its buffer. The first job moves into a struct of its own, `EpochRecords`,
which both streams hold:

- `EpochRecords::open(files, lambdas, emitted, threads)` holds the files, the epoch and
  `left_in_epoch`, and the `PgnStream`. It covers the resume rules as they are today: the
  epoch and the offset into it derived from `emitted`, and a resumed epoch's seed of its own.
- `next_record()` returns the next record, starting the next epoch when this one is done.
- `start_epoch` and `epoch_seed` move with it.

`PgnTrainStream` keeps its buffer and `next_batch`, and reads from an `EpochRecords`. Its three
tests pass unchanged. `BucketedTrainStream` holds an `EpochRecords` too.

**K vs K is skipped after the source, in `BucketedTrainStream`.** The source counts every record
it reads against `left_in_epoch`, so an epoch's length stays exactly its `.meta` total.

### The test pass: `GroupedTestSet`

A `BatchSource` for both tracks:

- It reads the frozen test set (`freeze::frozen_test_set`, 4,194,304 positions) once per run into
  memory, grouped by piece count. That's about 0.6 GB with six lambdas' labels.
- `read_batch_stop_short` sweeps the groups in order of piece count, each group in batches of
  `test_batch_size`. A group's last batch is short, and the next batch starts the next group.
- The sweep ends, and the cursor returns to 0, after the last group. The cursor counts positions
  across all groups.
- It replaces `CacheFile::open_lockstep` as the test reader of `train_lambdas_in_lockstep`. The
  NNUEs' test loss is unchanged, since the pass already weights each batch by its rows
  (`round::run_test_pass`); only floating-point rounding can differ.
- **No K vs K to filter.** The frozen test sets no longer hold any (below).

### Freeze: no K vs K

- **The skip.** `freeze`'s reservoir sample (`freeze::sample`) skips K vs K, so no frozen test
  set holds it.
- **The `.meta` check.** `freeze` checks that the positions read equal what the `.meta` files say
  (`assert_eq!(sample.positions_read, positions, ...)`). The skipped positions are counted
  separately, so the check compares all the positions read, skipped ones included.
- **Freezing again.** The existing frozen sets are frozen again once: lc0's (`data/lc0/frozen`)
  and self's. A run judged on an old frozen set and one judged on a new set differ in their test
  loss by those few positions' share, so their losses are not comparable to the last digit.
  Whether `freeze` refreezes over an existing set, or the old one is deleted by hand first, is
  settled while implementing, from what `freeze` does today.

### Forge

- **A transformer under `labels: self` or `labels: lc0`** gets `streamed_inputs`, as the NNUEs
  do. Its parts of `train_in_lockstep` become `BucketedTrainStream`, `GroupedTestSet`,
  `transformer_batch`, `StackedTransformer`, and `persists_position: false`.
- **A transformer under `labels: archives`** (zelda) keeps today's path untouched until its
  removal.
- **`net_config`:** a transformer under `self` or `lc0` must have `shuffle_buffer`, as an NNUE
  must.

### Configs

- `lc0-alpha-pieces.yaml` and `self-alpha-pieces.yaml`: zelda's architecture (width 64, 4
  layers of 4 heads, feed-forward 256, dropout 0.1, value head `[32]`, full attention).
- Their `label`, `training` and selection settings follow `lc0-psq-threats.yaml` and
  `self-psq-threats.yaml`, field for field, apart from zelda's learning rates (no 3e-3, which is
  too high for a post-norm transformer).
- `self-alpha-pieces` is taught by caroline-0001, as every `self-` network is.

## Test

- `BucketedTrainStream`:
  - every batch is full and comes from a single piece count;
  - K vs K never comes out;
  - over many batches, each piece count's share of the batches matches its share of the input;
  - a resume skips what the step count says.
- `EpochRecords`: `PgnTrainStream`'s three tests, unchanged.
- `GroupedTestSet`: one sweep yields every position exactly once, each batch from a single piece
  count, and the cursor returns to 0 at the end.
- ~~A small NNUE fixture gives the same test loss through `GroupedTestSet` as through `CacheFile`.~~
  Implemented instead as: one sweep of `GroupedTestSet` gives every record of its flat source
  exactly once. With `run_test_pass` weighting each batch by its rows, that is the same test loss
  up to rounding. The NNUE end-to-end test in `main.rs` (`write_zulu_inputs`) now runs its test pass
  through `GroupedTestSet` too.
- **Not covered by a unit test:** a streamed transformer's whole `forge`. The archived
  transformer's end-to-end test stays as it is. The first real run is the check.
- `freeze`: a fixture with a K vs K final position freezes without it, and its `.meta` check still
  passes.
- `cargo test --release -p smithy` on the training machine.
- First real run: `smithy forge self-alpha-pieces` on the training machine, after experiments 1 to 4 or between
  them. It should train at roughly the speed of zelda's earlier runs.
