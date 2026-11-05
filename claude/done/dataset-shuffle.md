# Shuffle the dataset at build time

**Done:** datasets are shuffled at build time into 256 buckets, the manifest marked `order\tshuffled-256` (unmarked datasets are refused, `dataset_cache.rs`). The amanda run that exposed the defect is historical.

## The defect

`rotor-train prepare` writes the train rows in segment order (`dataset_cache::run_pass`): TCEC,
then CCRL's chunks (the archive's months, oldest first), then the self-play segments. `rotor-train
train` reads the train file cyclically from row 0 (`read_records_cyclic`) through a shuffle buffer
of 2^19 positions, about 1% of a 50M dataset: it mixes neighbouring games, not the corpus. Every
epoch is a walk through the corpus in stored order.

Seen in amanda's generation 1 on the 50M corpus (lambda 0.65, stopped): in the last ~19% of every
epoch, the self-play stretch, the train loss drops and the test loss jumps (0.068 -> 0.078 in epoch
1, 0.057 -> 0.068 in epoch 2, 0.053 -> 0.061 in epoch 3), and the learning rate stepped down right
after each jump, once per epoch: patience was measuring the data order, not convergence. The CCRL
months drift too, less visibly. The run is not accepted.

## The fix: a bucketed shuffle in the build

The train half is written in a uniformly random order, so a cyclic read is a random pass. The test
half is evaluated whole every round, so its order does not matter and it stays as it is.

1. **Scatter.** During the one pass over the segments, each train position goes to one of
   `SHUFFLE_BUCKETS = 256` temporary bucket files, chosen by a hash of its global position index
   under a seed of its own (`DATASET_SHUFFLE_SEED`, distinct from `DATASET_SPLIT_SEED`): a pure
   function of the index, like the train/test coin. A bucket file holds each row whole, its
   position followed by one label per label set, uncompressed: 256 open files, under Linux's
   default limit of 1024, where one file per label set would exceed it. A label-only pass writes
   rows of labels alone, to the same buckets.
2. **Gather.** Then, bucket by bucket in order, read the bucket's rows into memory, permute them
   with Fisher-Yates under an RNG seeded by `(DATASET_SHUFFLE_SEED, bucket)`, append them to the
   final `train.positions` (and each `train.labels`), and delete the bucket. A label set reads its
   bucket and applies the same permutation (it depends only on the seed, the bucket and its row
   count), so row `r` of every file is still one position.
3. A random bucket per row followed by a uniform permutation within each bucket is a uniformly
   random permutation of the whole.

**Costs.** One extra local pass over the train rows, and temporary disk about the size of the train
files. At 100M positions a bucket holds about 370k rows: a few tens of MB in memory. Negligible
against reading the segments from Drive.

**A label-only pass (`add_label_sets`)** scatters and gathers its labels the same way, with no
positions, and lands each label beside its position by construction. The existing test that a later
label pass writes what a first build would have still holds, and the alignment test changes from
"the label of the i-th position read" to "the label at the row where the shuffle put it".

**Marking shuffled datasets.** The manifest gains a line `order\tshuffled-256`. A dataset whose
manifest lacks it was built before this change: `prepare` and `train` refuse it, telling the
developer to delete `data/datasets/<source>` and prepare again, as `prepare` already does for a
dataset built before self-play joined the corpus.

**Unchanged.** The shuffle buffer stays: the order is now the same in every epoch, and the buffer
varies it from one epoch to the next. The train/test split, the segment order in the manifest, and
the `.count` check are untouched.

## Recovering

- Delete `data/datasets/hand-crafted` on the GPU machine, and `data/networks/amanda/gen-0001`
  (the stopped lambda 0.65 would otherwise resume on reordered data with weights from the old).
- `bin/rotor-train prepare amanda` rebuilds the 50M dataset, shuffled (Drive's cache may still hold
  the segments), then train amanda and betty again.

## Verification

- `cargo test` for `dataset_cache`: a build's train rows are a permutation of the unshuffled rows
  (same multiset), labels stay aligned with their positions, a label-only pass equals a first
  build's, and the order differs from file order.
- In the rebuilt 50M run's `history.tsv`: no test-loss jump at the end of each epoch.
