# A dataset cache on disk for the self-play NNUE track

**Archived: implemented, then overtaken in its details.** Still in the code: one flat train and
one test cache per dataset, built once by `prepare`; the shuffle buffer (`shuffle_buffer.rs`) and
`ShuffledStream` (`train_stream.rs`); the read position derived from the step count; `run_config.txt`
frozen at first use. Gone: the block-of-33 split (`SPLIT_BLOCK`), `lambda-{L}/` paths (one dataset,
six label sets now), `rounds_per_epoch` and epochs (one-learning-rate.md), `preprocessing.parent`,
and the streaming path and dead code it names. The sections on existing games and run state
describe data that no longer exists.

## Status

**Implemented.** Replaces the streaming self-play pipeline as `rotor-train train`'s NNUE read side,
and implements the cache evolutionary-training.md describes: one train cache and one test cache per
dataset, written once by `prepare`, read cyclically through a shuffle buffer.

The streaming pipeline was deleted rather than repaired. Its round-boundary deadlock is diagnosed in
rotor-train-round-boundary-deadlock.md, which is kept for the diagnosis, not as work to do.

Two decisions were taken as recommended rather than deferred: the flat cache over keeping the
length buckets, and deleting the streaming path over deprecating it in place.

What landed, beyond the design below:

- `--threads` is gone from `rotor-train train`. It sized the streaming pipeline's PGN worker pool,
  and nothing on either track has threads to size any more. An invocation still passing it is
  rejected rather than ignored.
- **`net.yaml`'s `training` section gained two fields and renamed one.** `shuffle_buffer` and
  `rounds_per_epoch` are both mandatory for the NNUE kinds and forbidden for the transformer,
  validated in `net_config::load` exactly as `training.attention` already was in the other direction;
  `steps_per_eval` became `steps_per_round`, because `rounds_per_epoch` counts in the same unit and
  two names for one thing invite exactly the off-by-a-factor that pair would otherwise hide. amanda
  is `steps_per_round: 512`, `rounds_per_epoch: 32`, `shuffle_buffer: 524288`.

  The test set's size is deliberately *not* a field: the split stays exactly 32 train rows per test
  row, hard-coded as `dataset_cache::SPLIT_BLOCK = 33`. So `M` train rows carry `M / 32` test rows,
  and one round's test pass is `1/32` of its train volume. That 33 is inherited from the retired
  streaming path's ratio rather than chosen, and remains a value awaiting measurement.
- **The NNUE track now requires `preprocessing.parent`.** It trains on a self-play dataset, so a
  config naming only curated `pgn` stems has nothing to point at. This is a real narrowing of what
  `train` accepts, taken deliberately: the curated corpus was the bootstrap, and the loop this feeds
  is defined over generations of self-play. `check`/`calibrate`/`correlate` narrow the same way.
- `run_train_nnue` now prunes `checkpoints/` by test loss, which the corpus path never did and the
  streaming path did. History is appended before the checkpoint it describes, so retention always
  has a row to rank by.
- **One invocation is one epoch, and there is no early stopping on the NNUE track.** The run trains
  `rounds_per_epoch` rounds and exits, so the gate can play the candidate; a failed gate re-invokes
  it. `--patience`/`--min-steps` are rejected for an NNUE network rather than ignored, and
  `early_stopping.state` is no longer written. Both flags still apply to the transformer track, which
  is why `parse_job_flags` returns them as `Option` -- that is what lets the NNUE arm tell a flag that
  was passed from one that was merely defaulted.

  The boundary is a condition on `optimizer.step_count()`, not on the stream's cursor: the cursor is
  the read position, which the shuffle buffer keeps ahead of what has been stepped on, so it would
  name the boundary a buffer's worth of rows early. Using the step count also means no new persisted
  state -- it already survives a resume and already counts exactly `batch_size` rows per unit, so a
  second counter could only ever disagree with it. The console's "lowest test loss so far" is derived
  from `history.tsv` rather than a state file, for the same no-second-copy reason.

  The epoch length is `rounds_per_epoch * steps_per_round * training_batch_size` positions, declared
  rather than derived. An epoch is *meant* to be one pass over `M`, and is exactly that for a dataset
  built to `33 * 1024 * 512`. Against today's 13.54M-row cache, 32 rounds is 1.239 passes; that is
  printed, not asserted, because a differently-sized dataset is perfectly usable to train on and
  refusing it would force the epoch to be re-derived per dataset -- the thing declaring it avoids.
- **`prepare` no longer builds a length-bucketed cache per self-play segment.** The plan said it
  would keep both, for the transformer's sake; `data/caches/` turned out to be empty, so building
  both would have meant replaying the whole corpus twice to produce a layout nothing reads.
  `stem_dirs` correspondingly ignores `preprocessing.parent` and is now purely `preprocessing.pgn`.
  `build_self_play_positions_and_labels` and the two `self_play_*_dir` helpers are left in place,
  unused: they are what a transformer trained on self-play games would need, and that is a decision
  to take when it comes up.
- **The training read position is derived from the step count, not persisted, and the three config
  fields that derivation depends on are frozen on a run's first invocation** (`run_config.txt`, beside
  `model.vs`). See "Resuming: a derived cursor, and a configuration frozen at first use" below for the
  reasoning and for what a resume gives up, which is unavoidable and bounded.
- Left behind, not cleaned up: `checkpoint_store`'s resume-generation half, `EarlyStopping::replay`
  and `length_cache`'s `PgnWorkerPool` existed only for the streaming path and are now unused. They
  produce dead-code warnings rather than errors, and removing them is a separate pass.

## What is actually on disk today, and what the design document gets wrong

evolutionary-training.md's prerequisites were written against code that is no longer the live path.
Corrected here, from the source:

- **`builder.rs`'s `build_caches` is dead.** Nothing calls it outside its own tests. It also reads
  scores from a `rotor-curate` SQLite database, which a self-play segment does not have.
- **`reader.rs` (`CacheFile`, `read_batch_cyclic`) is not compiled at all.** It is not in
  `train/main.rs`'s module list. Same for `nnue_round.rs`, `window.rs`, `round_progress.rs`,
  `window_progress.rs`, `transformer_round.rs` and `transformer_window.rs`.
- **The live corpus path is `round.rs`'s `run_round`**, reading `length_cache.rs`'s
  length-bucketed files: one file per piece count (`2..=32`), per split, per source.
- **`rotor-prepare` already builds those files from self-play PGNs**, via
  `build_self_play_positions_and_labels`, one cache per segment, with a per-*game* 95/5 split
  (`builder.rs`'s `is_train_game`).
- **`run_train_nnue` still exists**, marked deprecated and `#[allow(dead_code)]`; `run_train`
  dispatches NNUE to `run_train_streaming` instead.
- **`split_counter.rs` and `round_router.rs` are the streaming path's**, so this design does not
  modify them -- but it does retire them, which is the opposite of what that prerequisite says.

So the only thing standing between today's tree and a cache-backed training run is the dispatch in
`run_train`. Everything below is about making the cache the one the design document actually asks
for, rather than the transformer's format inherited by accident.

## Why not just keep the length-bucketed cache

Bucketing by piece count exists for one reason: the transformer cannot pad, so every batch must hold
positions of exactly one length (`rotor-transformer-implementation.md`, design point B). NNUE has no
such constraint, and inherited the format only because the record layout happens to be shared.

The cost to NNUE is that **every training batch is materially homogeneous**: `pick_length_index`
picks one bucket, and the batch is 1024 positions that all have the same number of pieces. That is a
correlated gradient, batch after batch, on the one feature dimension the network most needs to
generalise across. It is not a correctness bug, and it may well train anyway, but it is the wrong
thing to leave in place under an experiment whose entire question is "does the naive NNUE converge
at all". A flat cache read through a shuffle buffer removes it, and is what
evolutionary-training.md specifies regardless.

## The dataset cache

One dataset, two files, built once per `(parent, lambda)`:

```
data/datasets/{parent}/lambda-{L}/train.cache
data/datasets/{parent}/lambda-{L}/test.cache
data/datasets/{parent}/lambda-{L}/manifest.txt
```

A new top level under `data/`, not a subdirectory of `data/caches/`: those are keyed per source
(`{parent}/{segment}` or `{stem}.depth{N}`) and stay the transformer's and the per-segment
convention's. A dataset is the union of every segment under one parent, which is a different thing
and deserves its own name.

- **Format: `cache.rs`'s existing flat record stream**, zstd-compressed, `CacheHeader` first. Nothing
  new to invent, and `read_record`/`write_record` already round-trip it.
- **`manifest.txt` is written last and doubles as the completion marker**, the same role
  `labels_dir`'s own `complete` file plays. It records the segments that went in, the total position
  count, and the two row counts. `prepare` rebuilds when it is missing, or when the segments
  currently on disk differ from the ones it lists; otherwise it prints and returns, as the existing
  builders do.
- **Whole dataset, not per segment.** A generation's dataset is complete before its `prepare` runs
  (evolutionary-training.md's refinement 1), so per-segment idempotency buys nothing and a single
  pair of files is what the cyclic reader and the epoch boundary both want.

### The block-of-33 split

`prepare` replays the games once and assigns each position a global index `i`, counted continuously
across game and segment boundaries. Within each block of 33 consecutive indices, one is drawn for
the test cache and the other 32 are appended to the train cache.

- **The draw is `StdRng::seed_from_u64(block_index ^ DATASET_SPLIT_SEED).random_range(0..33)`**, a
  pure function of the block index, matching `builder.rs`'s own `TRAIN_TEST_SPLIT_SEED` convention:
  a rerun reproduces the same split rather than reshuffling the dataset.
- **The trailing partial block goes entirely to train.**
- **Segments are visited in `self_play_segments`' sorted order**, which is already deterministic, so
  the global index is well defined.

This replaces `is_train_game`'s per-game 95/5 split for this path only. The per-game split stays
where it is for the curated and transformer paths.

### The row-count pre-pass

`CacheHeader` writes `row_count` before the first record, so both counts must be known up front.
They follow from the total position count alone, with no need to reproduce the draws: with `T`
positions, `test_rows = T / 33` and `train_rows = T - T / 33`, whatever each block drew.

`T` is one cheap scan: a self-play game contributes exactly one record per move
(`label_and_featurize_self_play_game` returns `labels.len() == moves.len()` records), so the pre-pass
counts SAN tokens and never touches a `Board`. All the expensive work -- replay, AB(lambda), feature
extraction -- happens once, in the writing pass.

## The read side

### A cyclic flat reader

Revive `reader.rs` by adding it to `train/main.rs`'s module list. `CacheFile` already does exactly
what is needed: a sequential zstd decode, `seek_to` reopening from the start for a wrap or a resume,
and `read_batch_cyclic`. Two changes:

- **Return records, not a `DenseBatch`.** `read_batch_cyclic` currently calls `to_dense_batch`
  itself. The shuffle buffer sits between the two, so the reader hands back `Vec<CacheRecord>` and
  the caller builds the batch. `read_range` goes the same way.
- **Add `read_batch_stop_short(cursor, n)`**, matching `LengthCacheReader`'s existing method: read
  up to `n` records, stopping at the end of the file and returning cursor `0`. This is what the
  test pass and the `check`/`calibrate`/`correlate` sweeps want, and giving it the same shape as the
  bucketed reader's is what lets those sweeps be shared rather than duplicated.

### The shuffle buffer

A new `shuffle_buffer.rs`: `B` resident records, filled from the cyclic reader; each emission picks a
uniform random slot, yields it, and refills that slot from the stream.

- **Positions, not games.** Plies of one game land in different batches, which is the point.
- **It never drains**, because the underlying read is cyclic and re-iteration is unbounded, so every
  part of every epoch is equally shuffled and there is no per-epoch reseed.
- **The buffer's contents are not persisted.** A resumed run refills and draws a different order, which
  evolutionary-training.md accepts explicitly in exchange for dropping the old "no RNG, no seed, just
  this one advancing integer" property. What exactly that costs, and where the resume gives in, is
  "Resuming: a derived cursor, and a configuration frozen at first use" below.
- **`B` is `training.shuffle_buffer` in `net.yaml`**, a real training parameter rather than a
  constant. `2^21` is the design document's value against `M = 2^24`; the dataset that exists today is
  13.54M train rows, so amanda gets `2^19` for now -- about 50 MB resident, and a 3.9% boundary cost
  against 15.5% at `2^21`.

### One round loop, two streams

`round.rs`'s `run_round` currently takes `(readers, row_counts, cursors, rng)` and picks a bucket
per batch. Generalise that to a trait rather than duplicating the loop:

```rust
pub trait TrainStream {
    fn next_batch(&mut self, batch_size: usize) -> Vec<CacheRecord>;
    fn cursors(&self) -> Vec<u64>;
    fn rows_per_epoch(&self) -> u64;
}
```

- **`BucketStream`** wraps today's readers/row counts/cursors/`pick_length_index`. The transformer
  track keeps exactly its current behaviour.
- **`ShuffledStream`** wraps a `CacheFile` plus a `ShuffleBuffer`, and reports a one-element cursor
  vector, so `RoundProgress` needs no change at all.

`rows_per_epoch` is not used by anything yet. It is there because the epoch boundary is what
eventually triggers the gate, and a stream that cannot say how long one pass is cannot report it.

The no-grad sweeps (`run_test_pass`, `run_cp_check`, `run_calibration_sweep`,
`run_correlation_sweep`) take `&mut [LengthCacheReader]` today and use only `row_count` and
`read_batch_stop_short`. A second small trait over those two members, implemented by both readers,
makes all four work for either format with no change to their bodies. The NNUE test pass then runs
over a one-element slice holding the flat test cache.

## Wiring

- **`run_train`'s NNUE arm calls a restored `run_train_nnue`**, built on `ShuffledStream` over the
  dataset cache. The transformer arm is untouched.
- **`run_check_nnue`/`run_calibrate_nnue`/`run_correlate_nnue`** read the dataset cache's test and
  train files instead of `stem_dirs`. The transformer's three keep `stem_dirs`.
- **`--threads` becomes optional** for `train`. It exists only for the streaming pipeline's PGN
  worker pool; the corpus path has no threads to size. Parsing it and ignoring it would be a trap,
  so it is accepted and documented as unused, or dropped -- one line either way.
- **`rotor-prepare` gains the dataset build**, after the existing per-segment loop: if
  `preprocessing.parent` is set, build `data/datasets/{parent}/lambda-{L}/` from every segment under
  it. The existing per-segment length-bucketed build stays, since the transformer track still reads
  it.

## Resuming: a derived cursor, and a configuration frozen at first use

The shuffle buffer makes exact resuming impossible, and the choice of where to give in is a design
decision rather than an implementation detail.

The invariant is `cursor = rows_emitted + buffer_capacity`: the cursor counts rows *read* from the
cache, and the buffer holds `capacity` of them that have been read and not yet stepped on. A state
file can carry one of the two numbers, never both, so a resumed run either re-reads rows that were
resident (emitting some of them twice) or walks past them (never emitting them in that pass).

This is not only a crash concern. One invocation is one epoch, so the *normal* generational flow
discards a full buffer at every epoch boundary. Whatever is given in on is paid once per generation,
guaranteed. At `shuffle_buffer: 524288` against a 13.54M-row cache that is 3.9% of a pass; at the
`2^21` evolutionary-training.md specifies it is 15.5%, which is the reason to raise that value and the
boundary cost together rather than tuning either alone.

**The decision: the read cursor is not persisted at all. It is derived.**

```
cursor = (training_batch_size * optimizer.step_count()) % train_rows
```

`optimizer.step_count()` is already in `optimizer.opt`, is already exact, and is already what the
round boundary, the epoch boundary, `history.tsv` and the gate are all indexed by. A persisted cursor
is exactly the "second counter that can only ever disagree" that this track already refuses for the
epoch and round numbers, and `RoundProgress`'s own doc comment already refuses for `steps_done`, for
the same reason and after the same kind of incident. Three consequences:

- **An inconsistency window closes.** `on_backup` writes `model.vs`, then `optimizer.opt`, then
  `round_progress.state`. A crash between the last two leaves a cursor from the previous backup beside
  a newer step count, with nothing to detect it. That state is no longer representable.
- **`rows_done` goes the same way**, as `(step_count % steps_per_round) * training_batch_size`. The
  flat stream's batches are never short, unlike a length bucket's, so there is nothing to track. What
  stays in `round_progress.state` for this track is only what is genuinely not derivable: `loss_sum`
  and `train_duration_secs`, the in-progress round's two accumulators. `ShuffledStream::cursors()`
  returns an empty vector so that no caller can persist a cursor even by accident.
- **The step-to-row map is fixed.** Epoch `k` starts at row `(k * rows_per_epoch) % train_rows`, which
  is row 0 for every epoch when the epoch is a whole number of passes, and cannot drift by a buffer per
  restart the way an advancing persisted cursor does.

What this does *not* do is make a resume exact, and nothing can. Refilling from `E = batch_size * S`
re-reads `[E, E + capacity)`. A row read `k` emissions ago is still resident with probability about
`e^{-k/capacity}`, so about 63% of that window genuinely was resident and is correctly recovered,
while the other ~37% had already been emitted and is emitted a second time, and the resident rows
drawn from further back than `capacity` are dropped. That is ~1.4% of a pass at a `2^19` buffer, paid
once per restart -- and since one invocation is one epoch, an epoch boundary is a restart, so it is
paid once per generation whether or not anything crashed. What a clean boundary does buy, when the
epoch really is one pass, is that `E` is a multiple of `train_rows`, so the next epoch begins its
sweep at row 0 rather than at an arbitrary offset.

One consequence worth naming, because pinning the phase creates it. Any scheme reads
`train_rows + capacity` rows per epoch -- the fill plus one row per step -- so `capacity` rows are read
twice per epoch no matter what. A drifting cursor moves that window around the file; a derived one
puts it on the same `[0, capacity)` every epoch, giving the first 3.9% of the file roughly double
exposure per epoch. It is benign here: within one generation the file's row order is game order from
one unchanging network, so a fixed random 3.9% slice weighted twice is a small effective-sample-size
cost and not a distribution shift.

### A PGN with less than one epoch in it

This is today's case, not a hypothetical: `rounds_per_epoch: 32` declares 16,777,216 rows and the cache
holds 13,542,107, so one epoch is 1.239 passes. It needs no special handling anywhere, and that is by
construction rather than by luck:

- **The read is cyclic**, so an epoch that outruns the cache wraps inside it. `ShuffledStream::open`
  reduces the derived position modulo `row_count`, which is a no-op for the first pass and handles any
  number of wraps after it.
- **The epoch boundary counts steps, not passes**, so the run still stops after exactly
  `rounds_per_epoch` rounds and the gate still gets a candidate on the declared cadence.
- **The shuffle buffer is clamped** to `row_count`, so a cache smaller than the configured buffer does
  not end up holding several cyclic copies of the same rows and emitting one position twice in a batch.

What it costs is exposure, not correctness: `rows_per_epoch % train_rows` of the cache is stepped on
one extra time within the epoch -- 23.9% of it today -- and which rows those are moves each epoch,
since 1.239 is not an integer. The run prints both the ratio and that percentage, so it is visible on
every invocation rather than buried in the arithmetic.

The one hard floor is a cache shorter than a single batch, which is rejected: the cyclic read would
wrap *inside* a batch and put the same position in it several times, which is a duplicated gradient
rather than a merely repeated one. Everything between one batch and one epoch is fine.

### A PGN with slightly more than one epoch in it

This is the steady-state case once self-play stops at `M` positions: it stops on a game boundary, and a
game is about 106 plies, so a purpose-built dataset overshoots one epoch by tens of rows rather than
landing on it exactly. Nothing needs to handle that either, and it is worth writing down why, because
"the epoch never finishes the pass" sounds like a leak.

With `train_rows = rows_per_epoch + d`, epoch `k` starts at `(k * rows_per_epoch) % train_rows`, which
is `train_rows - k * d` counting backwards. Each epoch therefore covers every row but the `d` that sit
immediately *before* its own start -- and the next epoch starts exactly at that window, so those rows
are the first ones it reads. The missed window walks backwards by `d` rows per epoch and no row is ever
starved; nothing accumulates. At `d` in the tens this is four orders of magnitude below the buffer
boundary's own cost, which discards a `capacity`-sized window at every epoch boundary regardless.

The run reports `d` as a row count rather than as a percentage, since at `d = 50` against 16.8M a
percentage rounds to "100.0% reached" and says nothing.

A cache longer than one epoch by a lot behaves the same way, just with a bigger `d`: the epoch does not
finish the pass, the next one continues from where it stopped, and the gate still fires on the declared
cadence rather than on the dataset's length.

### The configuration is read-only once a run starts

Deriving the cursor from the step count makes the derivation's inputs load-bearing across
invocations. If `training_batch_size` changed between two runs of one job, the same step count would
name a different row; if `steps_per_round` or `rounds_per_epoch` changed, the persisted step count
would be reinterpreted into a different position in a different round of a different epoch. That is
the same failure `RoundProgress`'s doc comment records for a lowered `steps_per_eval`, which once
completed a round with zero further training.

So the run freezes what it was started with, in `run_config.txt` beside `model.vs`, written on the
first invocation and verified field by field on every later one. A mismatch is fatal and names the
field, the recorded value and the current one: either restore the config, or delete the run directory
and start over. Frozen:

- `architecture` in full, plus `preprocessing.parent` and `preprocessing.lambda`, which name the
  dataset.
- `training_batch_size`, `steps_per_round`, `rounds_per_epoch`, the three the derivation and the two
  boundaries depend on.
- `train_rows` and `test_rows`. This is what turns a rebuilt dataset under a live run from a silent
  `cursor % row_count` into a refusal, and it keeps each round's test loss measured against the same
  test set, which is what makes the rows of one `history.tsv` comparable.

Deliberately *not* frozen: `shuffle_buffer`, which changes only how well a batch is mixed, and
`learning_rate`, which is already part of the run directory's own path and so cannot change within one
run. A plain text file rather than the magic-plus-version binary the other state files use, because
its whole purpose is to be read by a developer who has just been told which field disagrees.

## Reusing the existing self-play games

`data/self/hand-crafted/alpha-20260922-1702.pgn.zst` is the only dataset there is, and nothing has
to change to consume it.

- **It parses cleanly end to end.** The streaming run replayed it through `SelfPlaySegmentReader`
  and `label_and_featurize_self_play_game` -- the same collector and the same labelling function
  `prepare` uses -- and reached roughly 4.72M train positions before it hung. At the 32:1 streaming
  split that is about 4.87M positions total, which at the measured ~37 plies per game is about
  131.6k games, i.e. essentially the whole `128 * 1024`-game segment. The zstd frame terminates, the
  per-move score comments are all present (`label_and_featurize_self_play_game` asserts one comment
  per move and never fired), and no `Result` or `FEN` tag was malformed.
- **What is broken in it is the labels of the first six plies of every game**, not the file. Those
  plies were played with MultiPV-like exploration, and the score written into the comment was the
  score of the move actually played rather than the best move's -- biased low by up to 50cp. The
  fix is in `self_play.rs` now and applies to games generated from here on; these games keep the
  bias. Under AB(0.7) those six plies carry most of the weight of the opening labels, so this
  dataset is a poor teacher for openings and a fine one for the question actually being asked, which
  is whether the architecture fits its targets at all.
- **Nothing is filtered or repaired.** "We do not throw away anything" already governs
  `build_self_play_positions_and_labels`, and the same holds here.

Expect roughly 4.87M positions: about 4.72M train rows and about 147k test rows. The pre-pass is a
PGN scan; the writing pass is a single-threaded replay of 131k games, minutes rather than hours.
Parallelising it with the existing `PgnWorkerPool` is possible -- its results carry a `game_index`,
so a reorder buffer would restore the deterministic global position order the split needs -- but it
is not worth doing before the dataset is an order of magnitude larger.

## The streaming path

`run_train_streaming` and the modules that exist only for it (`split_counter.rs`,
`round_router.rs`, `stream_round.rs`, `stream_checkpoint.rs`, `completion_watermark.rs`, and
`length_cache.rs`'s `SelfPlaySegmentReader`/`PgnWorkerPool`) stop being reachable.

Recommendation: **delete them, in a commit of their own**, rather than carrying them the way
`run_train_nnue` was carried. `run_train_nnue` was kept as a working fallback; this is a path with a
diagnosed deadlock that is being abandoned in favour of the one it replaced, so keeping it invites
someone to reach for it. The diagnosis survives in
rotor-train-round-boundary-deadlock.md, which moves to `archived/` with a note saying the path was
retired instead of repaired.

This is a judgement call, not a technical requirement -- deprecating in place, as the project has
done before, also works and is the lower-risk option if the dataset cache turns out to need
iteration.

## Existing run state

`data/networks/amanda/lr-3e-4/` holds the streaming path's resume generations, `history.tsv` rows
and checkpoints. The corpus path looks for `model.vs`, which is not there, so it would start fresh
but append to the same `history.tsv` and mix checkpoints into the same directory. Delete the
directory before the first run rather than reasoning about the overlap.

## Tests

- **The split**: 100 synthetic positions produce exactly 3 test rows and 97 train rows; the draw is
  reproducible across two runs; the trailing partial block is entirely train.
- **The pre-pass agrees with the writing pass**: the header's `row_count` equals the number of
  records actually written, for both files, over a fixture spanning several blocks and several
  games.
- **Round trip**: a fixture dataset built from a small self-play PGN reads back with every label
  matching a hand-computed `compute_labels` result, and every position appearing exactly once across
  the two caches.
- **The shuffle buffer** emits exactly the multiset it consumed over a full cycle, and emits a
  different order than the file's with a fixed seed -- the two properties that matter.
- **`CacheFile::read_batch_stop_short`** stops at the end and returns cursor `0`, matching
  `LengthCacheReader`'s contract, so the shared sweeps behave identically over either.
- The existing `reader.rs` tests come back with it, updated for records instead of `DenseBatch`.

## Out of scope

The epoch boundary as a gate trigger, the learning-rate grid
(rotor-train-learning-rate-grid.md), Population Based Training, and the SPRT gate. This document
builds the dataset cache and the read side; those build on top of it.
