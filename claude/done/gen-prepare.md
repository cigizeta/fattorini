# gen-server's prepare phase: shuffled buckets instead of a shuffle buffer

**Status: design agreed with the developer, 2026-10-21; implementation plan below, not to be
implemented before the `lc0-psq-direct` forge has finished and been read.** Belongs to
self-training-simpler-attempt.md and self-training-simpler-implementation.md.

**Revised 2026-10-22 (the developer): to be implemented now,** before the `lc0-psq-direct-fine`
forge, which waits for it. Since then `gen-server` became the smithy's **forge** job
(smithy-and-arenas.md), and the old `forge` command is gone: below, "gen-server" reads "a forge".
The revision, settling the open points:

- **Every NNUE forge, Lc0's included,** trains on buckets; the buffer stream (`PgnTrainStream`) is
  kept for transformer forges alone, whose batches hold one piece count each (transformer-streaming.md),
  until that is settled separately.
- **Where the buckets live:** a self-play generation's in `data/buckets/<run>/gen-NNNN/`, shared by
  its attempts, a run's older generations' deleted as a new one trains; Lc0's in `data/buckets/lc0/`,
  shared by every Lc0 forge, since they all read the same converted corpus, scattered once and again
  only when the corpus changes (its files and sizes recorded beside the buckets, `buckets.tsv`).
  Outside the run's directory, so the archive never moves them.
- **No new phase:** the scatter runs at the start of Train, and does nothing when the buckets already
  hold the corpus; `state.txt` and the page keep their phases.
- **B = 1024**, fixed: about 100k records a bucket for a self-play attempt of 100M positions, about
  1M for Lc0's capped corpus of about 1B.
- **Records:** the test set's compact position (`cache.rs`, 5 + 2n bytes, at most 65), then score and
  result, about 50 bytes on average; **compressed with zstd** at a low level, each Prepare appending
  one frame per bucket (concatenated frames read back as one stream). About 15 GB for Lc0 instead of
  50, 1.5 GB for a self-play attempt instead of 5; reading and decompressing a bucket costs tens of
  milliseconds, a bucket ahead of the GPU, so training is no slower. The training machine can give 128 GB.
- **No extra fields** for filters: a new filter re-prepares, which is affordable.
- **A record's labels:** its labels at lambda 0 (`f32`) and at lambda 1 (`i8`); the direct blend is
  linear between them, decisive scores and the final position included, so any lambda's label is
  computed at load.

Implemented 2026-10-22 (`buckets.rs`; `PgnTrainStream` kept for tests only), tests passing; the
smoke forges on the Lichess machine next.

## Why

`smithy gen-server` trains through `PgnTrainStream::open`, forge's refilling buffer of
`shuffle_buffer` positions (2M). Whole games enter it together, so a game's 100 to 200 siblings are
drawn within one buffer turnover (about 512 steps of 4096): in any such window the network trains on
the same 10 to 20k games.

**The symptom (lc0-psq-direct, 2026-10-21):** at every rate drop the test loss falls while the train
loss rises (lambda 0.05: 0.0688 to 0.0722 at the switch to 3e-4), and 3e-3 plateaus for dozens of
rounds. The hypothesis: at a high rate the network tracks the buffer's current games, which lowers
the train loss (measured on siblings of positions just trained) without helping unseen games.
**Unproven:** the runs from the prepare days (amanda, betty, caroline, dorothy), trained on fully shuffled
caches, are to be read first; if their train loss rises at rate drops too, this design is optional.

`smithy prepare` was removed (pgn-stream-training.md) mostly because training was going to be
continuous, where a growing corpus cannot be prepared once. gen-server is generational again: each
attempt's corpus is fixed once played, so a prepare phase is affordable.

How others do it (fetched 2026-10-21): nnue-pytorch skips positions i.i.d. at random
(`random_fen_skipping`, P(skip) = N/(N+1), 3 in its example config) through a 1M-entry window;
bullet does no random skipping and relies on Fisher-Yates over its buffer and an offline shuffle of
the whole dataset (`bullet-utils shuffle`).

## Decisions (the developer and Claude, 2026-10-21)

1. **A phase between Play and Train:** Play, **Prepare**, Train, Round robin, Gate, Decide. After a
   failed gate, the next attempt's Play adds its positions and Prepare scatters only those.
2. **Prepare only scatters; every epoch gathers.** Prepare writes each new position to one of B
   bucket files, chosen by a hash of its global index. No shuffle at prepare time.
3. **Shared buckets.** Every attempt appends to the same B buckets, so each bucket holds every
   attempt in proportion. Within a file old positions precede new ones; that order never reaches
   training, being erased by the shuffle at load.
4. **Gather per epoch:** the epoch's seed fixes the bucket order and a Fisher-Yates permutation
   within each bucket, applied in memory as the bucket loads. Batches are read sequentially: a
   uniformly random order every epoch, a fresh one each epoch.
5. **No shuffle buffer.** gen-server's training ignores `shuffle_buffer`. A prefetch thread loads and
   shuffles the next bucket while the GPU trains on the current one: one bucket ahead, not a buffer.
6. **Records carry no labels:** position, score and result. The direct blend,
   `(1 - lambda) * score + lambda * result`, is computed per lambda at batch time, so moving the grid
   never invalidates the buckets.
7. **The PGN stays the record of truth;** the buckets are a disposable local cache on the server's
   disk, never on Drive.
8. **Exact resume:** `steps_done * batch_size` gives the epoch, the bucket and the offset within it;
   the epoch seed reproduces the order. Load that bucket, shuffle, skip ahead in memory.
9. **gen-server only;** forge and its stream stay as they are ("fresh code, old code untouched").

## Open, to settle in the plan

- **B:** 256 as the old prepare, or grown with the corpus so a bucket keeps a bounded size (each
  attempt adds about 375k records a bucket at B = 256).
- **Extra fields** for later filters (ply, whether the move is a capture or the side is in check),
  so a Stockfish-style filter needs no new prepare.
- **Record format:** fixed width, raw or zstd-compressed per bucket; disk guessed at 4 to 8 GB an
  attempt raw, to measure.
- **The test set** stays loaded whole by `open_one_pass` (4M positions, held back by game already).
- **Lc0 for generation 1:** the same scatter over about 1B positions, once (a few tens of GB local).

## Implementation plan

1. **Record and bucket file** (`gen_buckets.rs`): the fixed-width record (canonical position, score,
   result, the extra fields if adopted), writer and reader, round-trip tests.
2. **Scatter:** read an attempt's new corpus files through the existing PGN reader, hash each
   position's global index to a bucket, append. Before appending, store every bucket's length in
   `state.txt`; a restart truncates each bucket back to it and scatters again. On completion,
   `state.txt` records the phase done and the new lengths.
3. **The phase in gen-server:** `Phase::Prepare` between Play and Train, resumed as above, with its
   log line (positions scattered, bytes, minutes).
4. **Gather stream** (`BucketStream`): epoch seed to bucket order and per-bucket permutation; a
   prefetch thread; `next_batch` yielding `CacheRecord`s with the grid's labels blended at load;
   `open_at(steps_done)` for resume.
5. **Train on it:** gen-server's `stream_from` builds a `BucketStream` instead of
   `PgnTrainStream::open`; `train_rows` from the bucket lengths.
6. **Tests:** every position comes out exactly once per epoch; two epochs differ in order; a resume
   at step s yields the same records as an uninterrupted run from s; truncation after an interrupted
   scatter restores the lengths; the blend at load equals `Labelling::Direct`'s.
7. **Measure:** disk per attempt, scatter minutes, that the prefetch keeps the GPU fed; then the
   medium experiment's train/test curves against the buffer's.

Revised 2026-10-22: the bucket module is `buckets.rs`, its file a sequence of zstd frames; step 2
truncates by frames (each bucket's byte length stored before appending); it scatters Lc0's corpus
into `data/buckets/lc0/` when its recorded corpus differs from `data/self/lc0/train`'s files, else
skips; step 5 covers every NNUE forge.

## Steps for the developer

1. Approve the revision above (2026-10-22).
2. Implement, `cargo test -p smithy`, then the smoke configs on the Lichess machine (`gen-psq-smoke`,
   `gen-lc0-psq-smoke`) from the smithy's page.
3. On the training machine, `lc0-psq-direct-fine`: its first Prepare scatters Lc0 once; its curves against
   `lc0-psq-direct`'s, at the rate drops above.
