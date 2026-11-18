# The transformer on the scattered buckets

Written 2026-11-18. A design note, not yet a plan: its decisions confirmed by the developer the
same day; an implementation plan follows. Part of the transformer's redesign
(rotor-transformer.md, architectures.md), after the halfk line.

## Goal

A first experiment, abstract and cheap: train the transformer's value head on Lc0's data and
compare its test loss with the halfk winner's. **No games:** the engine cannot play a transformer,
and nothing here changes that. A scaled run first.

For the loss comparison to be fair, the transformer must train exactly as the NNUEs do: the same
records, the same full shuffle, the same labels (polyline, curve) and the same frozen test set.
Today it does none of these.

## Why the current stream cannot stay

The transformer trains through `bucketed_train_stream.rs` over `pgn_train_stream.rs`, reading the
PGN segments directly. It was never moved onto the scatter when the NNUEs were, only because no
transformer was trained then. As a result:

- **A streaming shuffle.** Shuffled segment order, a few files interleaved, then one shuffle buffer
  per piece count. Positions of one game stay close together: the failure `buckets.rs` records
  (the network tracking the buffer's current games, the test loss falling while the train loss rose
  at every rate drop). Known to undermine training quality.
- **No polyline.** The polyline is measured on the buckets, which a transformer run does not
  scatter, so the smithy refuses `lambda: polyline` for a transformer. The stream takes constant
  lambdas only, from the grid and the lockstep arms meant to go.
- **The Atan curve, hardcoded** in `pgn_train_stream.rs`'s `start_epoch`. Atan is deprecated; the
  NNUEs no longer train on it.

## Design

**No padding.** Every batch holds positions of one length, as today.

**Gather, then group online by piece count:**

1. The transformer reads the scattered buckets exactly as an NNUE forge does (`BucketStream`):
   buckets in an epoch's random order, each shuffled as it loads. The full shuffle comes from here.
2. Each record goes to the group of its piece count, 3 to 32 with the kings: 30 groups. K vs K is
   dropped, as today.
3. A group that reaches the batch size is sent to training as one batch, and starts empty again.

No random draw within a group and no capacities from a first fill: the input is already fully
shuffled, so grouping only reorders it. Memory is at most 30 partial batches.

**Labels as the NNUE's:** each record's lambda from the polyline at its progress
(`Polyline::at(i, n)`), computed as the batch loads, as the buckets already allow; the NNUEs' label
curve, not Atan. The polyline is the one measured on the same corpus's buckets (`polyline.txt`).

**The test pass:** the frozen test set, grouped by piece count. `grouped_test_set.rs` already
groups a test set by length; to check that it reads the frozen one.

## Decisions (confirmed by the developer, 2026-11-18)

1. **The end of an epoch: the partial groups are dropped** (the developer: drop, as it is the
   simpler). At most 30 times the batch size less
   one positions an epoch, negligible. Epochs stay independent, which keeps the resume within one
   epoch. Rejected: carrying them into the next epoch, as the current stream does.
2. **Resume: the epoch's grouping replayed, without training.** The step count no longer says how
   many records were read, and partial groups are lost at a stop. But the grouping is
   deterministic under the epoch's seed: a resume reads the epoch from its start, grouping without
   training, until the batches emitted reach the step count, then trains. Nothing persisted beyond
   the step count, as the NNUE's rule. Its cost: one read up to the point of the stop.
3. **Rare piece counts: nothing special.** 3 or 4 pieces fill a batch slowly and arrive as a rare
   batch rather than spread out; harmless. No minimum rate, no merging of groups.

## What goes, once this works

- The transformer's PGN training stream: `bucketed_train_stream.rs`'s use of `EpochRecords`, and
  `pgn_train_stream.rs` if nothing else reads it (its tests' `PgnTrainStream` to check).
- The polyline refusals for a transformer in `smithy.rs`.
- The constant-lambda path, with the lambda grid and the lockstep arms (TODO, "Code to clean up"):
  the transformer no longer holds it back.

## The experiment, once this is built

1. **Throughput first:** a few thousand training steps at width 64 and 128, on CPU and GPU, giving
   positions per second. It fixes the scale and the model size; the estimates of the discussion
   (about 150 MFLOP per position for a training step at width 128, 4 layers) are too loose to plan
   on.
2. **A scaled run** of the transformer, and the NNUE baseline at the same scale: the same share of
   the corpus, the same budget (the fixed budget of fixed-budget-schedule.md once proven), the same
   frozen test set. Never the full-scale halfk winner against a scaled transformer.
3. **Reading the result:** a transformer ahead at a small scale is a strong signal; behind by a
   little is not conclusive, since a transformer likely wants more data than an NNUE. Equal loss
   would show the training works, not that it would play better: it costs far more per evaluation.

## Not settled here

- Whether the existing value head (rotor-transformer-implementation.md) trains as it is, or needs
  rework beyond its stream.
- Mixed precision (bf16) in the trainer: about halves a GPU run's time, not needed for the
  scaled run.
- The policy head: Lc0's data carries policy targets, so it could train beside the value head
  later; out of scope for the first experiment.
