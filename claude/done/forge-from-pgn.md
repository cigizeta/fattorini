# smithy forge from PGN: implementation plan

Implementation plan, 2026-10-17, drafted for the developer to review. The training half of
pgn-stream-training.md: `smithy forge` trains the NNUE track from a teacher's training segments
through `pgn_stream` (pgn-reader-implementation.md, 4.7M positions a second on another machine) and is judged on
the teacher's frozen test set (freeze-implementation.md). No dataset, no `smithy prepare`.

## Where it plugs in

`train_lambdas_in_lockstep` (`main.rs`) already isolates both ends:

- **The training stream** comes from one closure, `stream_from(steps_done, _)`, returning a
  `train_stream::TrainStream`: today a `ShuffledStream` over the dataset's `train.positions`.
- **The test set** is `CacheFile::open_lockstep(test.positions, [lambda-x/test.labels, ...])`, a
  `BatchSource`. The frozen set is the same files, so it is opened the same way from
  `data/self/<teacher>/frozen/`.

Everything else (the stacked network, the rounds, patience, backups, history, candidates) is
unchanged.

## Changes

1. **`pgn_stream` carries every lambda.** Lockstep trains every pending lambda on each batch, so a
   streamed position carries one label per lambda, in the arms' order: `StreamOptions::lambdas`
   replaces `lambda`, and the stream yields `CacheRecord`s directly (the type batches are built from).
2. **`PgnTrainStream: TrainStream`**, new, in `pgn_train_stream.rs`:
   - epochs without end: when a `PgnStream` drains, the next starts over the same files, shuffled
     with a seed from the epoch number;
   - its own shuffle buffer of `training.shuffle_buffer` records (the existing `ShuffleBuffer` reads
     a `CacheFile`; this one takes records from the stream), filled up front, each batch drawn from it
     at random and refilled from the stream;
   - `cursors()` empty, as `ShuffledStream`'s: the position is derived from the step count.
3. **Resume, retraining allowed** (decided 2026-10-17): `steps_done * batch_size` gives the epoch
   (`/ epoch_positions`) and how far into it; the resumed stream starts a fresh shuffle of that
   epoch, seeded by the epoch and `steps_done`, and the epoch ends early by the positions already
   trained. Some positions are trained twice that epoch and some not at all; the next epoch is
   whole again. No replay, nothing persisted beyond today's files.
4. **The source:** the teacher of the generation's `teacher.txt` (prepare's `fetch_teacher`, moved
   into forge), its training segments `data/self/<teacher>/train/` on the remote, read in place.
5. **Checks before the first round,** replacing prepare's: the training segments hold at least
   `self.yaml`'s `train.positions` (otherwise stop, saying how many there are: forge does not wait);
   the frozen test set exists and has every pending lambda (otherwise stop, naming the
   `smithy freeze` command that adds them).
6. **`run_config.txt`:** `parent` is the teacher's source as today; `train_rows` becomes the epoch's
   positions (the segments' `.count` summed) and `test_rows` the frozen rows, 4 x 2^20. A run started
   from a dataset therefore refuses to resume from the stream, and the other way round: a generation
   switches at its start, never mid-run.

## The reader's threads

The batch builder already runs its CPU half on every core (`round::BatchBuilder`), so the reader must
not take them all. Proposed: `available_parallelism / 2` reader threads, at least 1, 64 open files,
keep probability 1. Another machine's numbers say 4 threads already give 3.4M positions a second.

## Not in this plan

- `check`, `calibrate` and `correlate` on the frozen set: they read the dataset's test half today.
- The transformer track (`length_cache`), and Lc0.
- Removing `prepare` and `dataset_cache.rs`: after a generation has trained this way.

## Tests

- `PgnTrainStream` never ends: more batches than an epoch holds come out, every batch full.
- An epoch yields each position once (keep probability 1), the buffer's residue included.
- A resume at `steps_done` starts the right epoch and ends it early by the right count.
- Each record carries one label per lambda, in order, equal to `self_play_game_labels` at each.

## Decided (the developer, 2026-10-17)

- **The switch:** `labels: self` in `net.yaml` replaces `labels: alone` (beside `archives` and
  `lc0`): the teacher's games in `data/self`, streamed. No network used `alone`; prepare's `alone`
  branch goes in the same change, leaving prepare `archives` alone until it is removed.
- **Reader threads:** half the logical cores, at least 1, fixed in code.
- **Keep probability:** 1, every position of every epoch trained, so the first streamed
  generation is comparable with the earlier ones. Random skipping is a later experiment.
