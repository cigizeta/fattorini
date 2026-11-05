# Training straight from PGN files

Design only: nothing here is implemented yet. Written 2026-10-17 with the developer. It removes
`smithy prepare` and its dataset cache (`dataset_cache.rs`), which cost time and above all disk:
`smithy forge` reads the corpus's `.pgn.zst` files directly, every epoch. The test set comes from
test-set-freeze.md.

## Why again

rotor-prepare-deprecation.md tried this once and was deleted for a deadlock at the round boundary.
The idea was not at fault; its shape was. The test set was produced while streaming (a mod-33 counter
writing one file per round), and the checkpoint had two owners (the GPU thread the model, the main
thread the progress through the files), so the threads needed messages both ways: a round-complete
signal, a checkpoint handshake, a completion watermark. Two threads each waiting on the other
deadlocked.

Here the test set is frozen beforehand and the checkpoint has one owner, so data flows one way only:
from the readers to the GPU.

dataset-shuffle.md is the other lesson: a shuffle buffer mixes only neighbouring games. With 2^19
positions over a corpus read in file order, the test loss jumped every epoch at the self-play
stretch, and patience reacted to the order, not to convergence. So mixing across the corpus has to
happen in the reader, not the buffer.

## How the others do it

Neither nnue-pytorch nor bullet trains in game order, and neither shuffles on disk for game-ordered
formats. nnue-pytorch's loader reads `.binpack` (game order, needed by its compression) with several
threads at once, skips positions at random (`random_fen_skipping`), and shuffles a buffer. bullet's
`SfBinpackLoader` and `ViriBinpackLoader` decode game by game into a shuffled buffer. Rotor does the
same over its own compressed PGN, with no external format or library.

## The input

A corpus as test-set-freeze.md defines it: a training directory of `.pgn.zst` files with their
`.count`, from self-play or Lc0, and the frozen test file built from its separate test directory.
The reader reads the training directory only, skips no game, and does not know which producer wrote
the files.

## The flow

```
worker 1 --+
worker 2 --+--> bounded channel --> buffer --> batch --> GPU step
worker N --+    (labelled positions)  (owned by the training thread)
```

### 1. Startup

- Lists the training directory's files and reads their `.count`.
- Loads the frozen test file (test-set-freeze.md) and checks the run's lambda is in it.

### 2. Each epoch

- Shuffles the training file list with a seed made from the epoch number.
- Each of the N workers opens K / N files of it (K around 32 to 64 in total), each with a zstd
  decoder at its first game.

### 3. A worker

- Picks one of its open files at random, weighted by the positions it has left (its `.count` minus
  what it has read).
- Reads that file's next game, sequentially: a `.pgn.zst` cannot be seeked, and is never asked to.
- Labels its positions at the run's lambda (`compute_labels`).
- Keeps each position with probability p (random skipping), dropping the rest.
- Sends the kept positions down the channel; blocks while it is full.
- When a file runs out, opens the next of the epoch's list, shared by an atomic index: the only
  state the workers share. When the list is used up and its files drained, the epoch ends.

Weighting by positions left makes every open file run out at about the same time, so an epoch
stays a proportional sample of the corpus from start to end. Many files open at once is what spreads
the corpus over every stretch of training; the buffer only spreads one game's positions over many
batches.

### 4. The buffer

Owned by the training thread, so no lock.

- Positions from the channel go into the buffer (2^19 or more).
- Once full, each batch is drawn at random from it.

Behind a small interface (insert a position; a batch if one is ready), so that the transformer track
later swaps in one buffer per piece count (2 to 32): a batch is drawn from whichever bucket fills
first, which keeps piece counts in the corpus's proportion with no padding. Not built in this plan.

### 5. Training

- The training thread receives positions, fills batches, runs the GPU steps.
- At every round boundary (`steps_per_eval` steps) it evaluates the frozen test set, held in memory,
  and appends a row to `history.tsv`, as today.

If building batch tensors on this thread turns out to starve the GPU, a batcher thread can own the
buffer and pass finished batches through a second channel: still one way only. Not built unless the
measurement asks for it.

## Resume: some positions may be trained twice

Decided with the developer: exact replay is not required.

- **The checkpoint**, written by the training thread alone at a step boundary: the model, the
  optimizer, the epoch, and the positions consumed so far in it.
- **On resume**, a fresh random stream for the rest of the epoch, seeded by the epoch and the
  resume count, trains only the positions the epoch still needs. In that epoch some positions are
  trained twice and some not at all; every pick is random either way, so the epoch is still a
  proportional sample. The next epoch bears no trace of it.
- **What this buys:** no replay of consumed positions at resume, and readers that need not be
  deterministic: N workers may finish games in any order, with no reordering and no saved state.
- **What it costs:** runs are not bit-for-bit reproducible, as any multi-threaded shuffle buffer
  already makes them.

## Stopping

On Ctrl+C or when patience runs out, the training thread finishes its step, writes the checkpoint,
and drops the channel's receiver. Workers blocked on a full channel get a send error and exit. No
thread ever waits on one that may be waiting on it.

## What goes away

- `smithy prepare` and `dataset_cache.rs`: the flat train/test caches, the label sets precomputed
  per lambda, the 256-bucket shuffle, and their disk.
- Prepare's checks that still matter (`teacher.txt`, waiting for enough self-play) move to the start
  of `smithy forge`.
- `labels: archives` (deprecated: CCRL and TCEC are no longer training data), unless the developer
  decides otherwise.

The transformer track moves to the bucketed buffer, about 32 buffers, one per piece count (the
developer, 2026-10-17): `smithy regroup` and its length buckets are not kept, and go with prepare.

## Order of the work

1. **Measure first:** a reader alone, N threads on the training machine over existing segments, in positions
   parsed and labelled per second, against the positions per second the GPU consumes. If the reader
   cannot keep up, nothing else matters.
2. `arena self --test` and the freezer (test-set-freeze.md), on a self-play corpus.
3. The reader and the buffer, with the self-play corpus.
4. `smithy forge` on the stream; prepare and `dataset_cache.rs` removed.
5. Lc0, reusing the same reader with its own file list.

## Open points

- **Epochs:** does a self-play generation train several passes, or one? And the epoch size: lc0-data.md
  fixes 96 x 2^20 positions out of 2.5 to 3.3 billion, which here becomes random skipping at about
  1 in 30.
- **p, K, N and the buffer size**, from the measurement.
- **Lc0's cost:** an epoch parses and labels every game of the corpus, billions of positions read from
  Drive, to keep about 1 in 30. If the measurement says that is too slow, lc0-data.md's epochs built
  ahead on the Lichess machine stay; otherwise its "Building an epoch" and "Transfer" sections are superseded.
  That document is left alone until the number exists.
