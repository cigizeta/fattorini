# The transformer on the scattered buckets: implementation plan

Written 2026-11-18, the plan of transformer-on-buckets.md (its decisions confirmed the same day).
Not yet applied.

## What the code already does

Read on 2026-11-18, to place each step:

- **The frozen test set is already shared.** `smithy.rs`'s `train` freezes one `TestSet`, labelled
  by the run's `Labelling` (polyline included) and the source's curve, and loads it as one
  `GroupedTestSet` for both architectures: "every architecture is judged alike". Nothing to do.
- **The curve is already right on the buckets.** `buckets::prepare` scatters under the source's
  curve (`on.curve()`); the Atan curve is hardcoded only in `pgn_train_stream.rs`'s `start_epoch`,
  which this plan takes out of the transformer's path.
- **The polyline is already computed per record as a bucket loads** (`Labelling`, `Progress`).
- **What differs is only the transformer's branch of `train`:** it skips `buckets::prepare`
  (`train_rows` summed from the files), refuses the polyline, keeps only the constant lambdas, and
  streams through `BucketedTrainStream` over `EpochRecords`, sized by `shuffle_buffer`.
- `BucketStream` emits records across bucket and epoch boundaries without marking them; each epoch
  is every committed record once, `total` records (`buckets.tsv`).

## A consequence of dropping the partial groups

Every epoch holds the same records, only reordered, so each piece count `g` with `n_g` records
gives exactly `floor(n_g / B)` batches every epoch, `B` the batch size. **The batches of an epoch are
a constant:** `batches_per_epoch = sum over g of floor(n_g / B)`. It gives the resume its epoch
without replaying earlier epochs, and the budget its training rows: `batches_per_epoch * B`, not the
buckets' total. A piece count with fewer than `B` records never trains; negligible on any real
corpus, and said in the log.

The `n_g` need one pass over the buckets: measured once and kept beside them, as the polyline is.

## Steps

1. **The piece counts beside the buckets.** A `piece-counts.txt` in the bucket directory: the
   records of each piece count, 1 to 30 beside the kings (K vs K counted apart, never trained),
   measured by one pass over the committed buckets when missing, with a bar and the job's stop, and
   kept with the count of records it measured; measured again when that count differs from
   `buckets.tsv`'s (a later prepare appended). Where: `buckets.rs` or a small module of its own, as
   `polyline::ensure`. Tests: a fixture's counts, a stale file measured again.

2. **`BucketStream`, record by record.** A `next_record()` beside `next_batch`, and
   `records_per_epoch()` (`total`). `next_batch` stays as the NNUE reads it. Test: records read one
   at a time equal the batches' records, in order.

3. **The grouped stream** (`bucketed_train_stream.rs` rewritten, the name kept): over a
   `BucketStream`, 30 groups; each record read goes to its piece count's group, K vs K skipped; a
   group reaching `B` is emitted as one batch, in arrival order (no random draw: the input is fully
   shuffled), and emptied. After `records_per_epoch` records the epoch ends: every group cleared.
   **Resume:** from `steps_done`, the epoch is `steps_done / batches_per_epoch` and `k` its batches
   done; the `BucketStream` opens at `epoch * records_per_epoch`, and `k` batches are grouped and
   thrown away before the first is returned, with a log line saying so. `cursors()` stays empty.
   Tests: every batch full and of one piece count; exactly `batches_per_epoch` batches an epoch,
   the partial groups dropped; a resume at step `s` returns the batch an unbroken stream returned
   at `s`; no K vs K.

4. **`smithy.rs`'s `train`, one path for both architectures:** the buckets prepared for a
   transformer too; the polyline refusals removed (`train`'s and the load's, lines 314 to 316);
   the arms passed as for an NNUE, through `Labelling`, so the `lambdas` filter goes; for a
   transformer, `piece-counts.txt` ensured after the polyline, and `train_rows` set to
   `batches_per_epoch * B`; `stream_from` opening the grouped stream. The comment on
   `pgn_train_stream::reader_threads` checked for what still reads it.

5. **`net_config.rs`:** `shuffle_buffer` read by nothing any more: refused for every architecture,
   with a message saying the transformer reads the scattered buckets; its tests changed to match.

6. **The old stream removed:** `EpochRecords`, `PgnTrainStream` and the old `BucketedTrainStream`,
   and `pgn_train_stream.rs` down to the segment listing (`training_files`, `test_files`,
   `reader_threads`), which census, freeze, Lc0's rewrite and the test set still use. `lockstep.rs`'s
   tests that open a `PgnTrainStream` or the old stream move onto a bucket fixture. A separate step,
   so 1 to 5 can be seen working first.

7. **The config:** `configs/lc0-alpha-pieces.yaml`, new: a small shape to start (width 64, 4
   layers), `lambda: polyline`, the fixed budget's fields as
   `lc0-halfk-threats-screlu-hidden-budget`'s, a `scale` for the scaled run, no `shuffle_buffer`.
   Earlier transformer runs are too far in the past to build on (the developer, 2026-11-18).

   **The budget on a transformer** (the developer, 2026-11-18, found at step 4): `net_config.rs`
   refused it; now allowed, the round robin skipped as for every transformer run (MCTS still
   undecided), `candidates::export_budget_candidates` recording the candidates' test losses in
   `candidates.txt` without writing engine networks.

8. **Seen working:** `cargo test -p smithy`; then a scaled smoke run of the config on the training
   machine, end to end: its piece counts measured, its rounds, a stop and a resume (the log's
   replay line, the loss continuing), its test loss on the page.

## Then, the experiment (transformer-on-buckets.md)

1. **Throughput:** the first rounds of step 8's run on the GPU give positions per second, at widths
   64 and 128. If the round's log does not say positions per second, it is added there. **The CPU
   is for development only** (the developer, 2026-11-18): tests and short smoke runs; if it is too
   slow even for those, the GPU.
2. **The scaled run and its NNUE baseline:** the same `scale`, the same budget and the same frozen
   test set; the halfk winner's config at that scale beside it. Compared by test loss only.

## Not in this plan

- The existing value head's own soundness beyond its stream: step 8 shows whether it trains.
- Mixed precision, the policy head, playing a transformer.
