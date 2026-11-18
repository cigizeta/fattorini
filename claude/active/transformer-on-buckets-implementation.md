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

   **Sides in the vocabulary** (the developer, 2026-11-18, found by step 8's first run): the
   scaled run stayed at test loss ln 2, a constant prediction, while the scaled NNUEs fell to about
   0.65 on the same share. Its tokens did not say whose piece each was. `transformer_vocab.rs` now
   has an own and an opponent block, 1137 tokens (rotor-transformer.md, Vocabulary).

8. **Seen working:** `cargo test -p smithy`; then a scaled smoke run of the config on the training
   machine, end to end: its piece counts measured, its rounds, a stop and a resume (the log's
   replay line, the loss continuing), its test loss on the page.

## Step 8, seen (2026-11-18)

Scaled runs of `lc0-alpha-pieces` at 1/1024 on a CPU, 1.98M training positions, 484 rounds of 4
steps of 4096, about 4 to 5 minutes each:

- **r1:** the old vocabulary and network: stuck at test loss ln 2 (a constant prediction).
- **r2:** sides in the vocabulary: still ln 2. Ended in error: training published every budget
  candidate's `.nnue`, which a transformer has none of; fixed (only existing files published).
- **Diagnosis:** a toy batch (a queen up for one side or the other) trained at once; the real
  buckets, trained alone without the round loop, barely moved (0.6930 to 0.6900 at best in 400
  batches of 1024; flat with dropout 0.1). Hence pre-norm, 0.02 initialisation and no dropout
  (rotor-transformer.md); in the same harness then 0.6762 at rate 3e-4, 0.6833 at 1e-3, which
  jumped. The budget's start set to 3e-4.
- **r3:** pre-norm, rate 3e-4 to 1e-5: test loss 0.6534 at the end (best 0.6530), against the
  scaled NNUE baselines' 0.6507 and 0.6536 under the same budget: level. Behind early (by 0.014 at
  step 200), the gap closing until the rate fell below about 3e-5. Ended cleanly, no round robin.
- **r4:** the same, down to 3e-6 (a fall of 100, as the NNUEs'; Lc0's own transformers fall about
  67, in steps, Chessformer's appendix): test loss 0.6584 at the end, behind r3 from step 200 on
  (by 0.003 to 0.005): a geometric fall to a lower end lowers the rate throughout, and at this scale
  the middle of the run matters more than the end. Back to 1e-5. A rate held high for longer, as
  Lc0's step schedule, may beat r3: the budget has no step schedule yet.
- **Width 128** (`configs/lc0-alpha-pieces-128.yaml`): r3's schedule, width doubled. Next.

Lc0's settings, for reference (Chessformer, ICLR 2026, appendix A.2): Nadam, its 191M network at
2e-3, then 3e-4 at 4.49M steps and 3e-5 at 5.47M of 6M; its small ablations (width 256, 8 layers,
batch 2048) at a constant 5e-4 for 1.4M steps. Its earlier transformers were post-norm with DeepNet
initialisation (Lc0's blog, 2024-02).

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
