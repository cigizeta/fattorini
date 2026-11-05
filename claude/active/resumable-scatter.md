# The resumable scatter: design and implementation plan

Status: designed 2026-11-03 with the developer; steps 1 to 3 applied the same day, tests and
clippy passing (a third test beside the plan's, `batches` itself on a hand-made list). Left: a
real scatter seen resuming, then this document to `done/`. Answers the TODO's "The scatter,
possible optimizations", its first idea ("Resumable, a must").

## The problem

`buckets::prepare` writes `buckets.tsv` only once every new file is scattered, so a scatter
stopped partway, by Stop on the page or by a failed read (Lc0's first scatter on `data/lc0`, lost
after 79 minutes to one failed Drive read), starts over from the last state on the next try.

## Why not record the state every N segments

The scatter reads up to `OPEN_FILES` files at once over several threads, so every bucket's frames
mix records of many segments. At a given moment the written bytes hold some segments whole and
others in part, and truncating a bucket cannot tell them apart. A commit is only possible where
every file is either wholly written or not at all.

## The design: the scatter in batches, a commit after each

- The new files are split, in their given order, into batches of about `BATCH_POSITIONS` =
  50,000,000 positions (a fixed constant, the developer, 2026-11-03): files are added to a batch
  until it reaches the constant, so the last file may carry it over and a file larger than the
  constant is a batch alone. Lc0's corpus: about 24 batches, about 4 minutes each at Drive's rate.
- Each batch is today's loop with its own `PgnStream`, then every bucket flushed and `buckets.tsv`
  written with the batch's files added (temporary file renamed over the old: a crash leaves either).
- A restart needs nothing new: the existing path truncates every bucket to its committed length and
  scatters only the files not yet listed.
- The arrival index goes on from `state.records()`, so each batch draws its buckets afresh, as an
  appended file does today.
- One progress bar over all the new positions, not one a batch; Stop checked as today, losing at
  most the batch in flight.
- Cost: each commit flushes about 128 MB of partial frames (a half-full `FLUSH_BYTES` per bucket),
  about 24 extra small frames a bucket for Lc0, no effect on loading and almost none on the size;
  and the readers idle at each batch's end while its slowest file finishes, seconds against
  Drive's download.

**Out of scope (the developer, 2026-11-03):** retrying a failed read inside `PgnStream`. It stays a
TODO item of its own; with batches, a failed read costs one batch and a Resume goes on.

## Steps

### 1. `prepare` in batches (`crates/smithy/src/buckets.rs`)

- `const BATCH_POSITIONS: u64 = 50_000_000`, its comment saying why (a commit at each batch's end,
  the only points where the buckets hold whole files).
- `prepare` keeps its signature and calls `prepare_in_batches(dir, files, threads, stop, say,
  BATCH_POSITIONS)`, which the tests call with a small batch.
- The loop body (stream, push, flush, state written) moved into a function scattering one batch
  into the open `BucketWriter`s and the `State`, returning `Stopped` or the records.
- The bucket writers opened and truncated once, before the first batch, as today.
- The module's doc: "The state" paragraph says it is written after each batch.
- The end message unchanged (minutes, records, GB), the start one adding the batch count.

### 2. Tests

- **A stopped scatter keeps its finished batches:** a training set of several files, batch size of
  one file; stop after the first batch (a `stop` set by `say`'s commit message or a counter); the
  state lists the first batch's files; the next prepare adds only the rest, and its epoch equals
  `direct(files)`.
- **Batches change nothing but frames:** the same files in one batch and in several give the same
  sorted records.
- The existing tests pass unchanged (`a_stopped_scatter_is_undone_by_the_next` included).

### 3. TODO.md

The idea marked done, pointing here; the retry left as its own item.

## Commands for the developer

```
cargo test -p smithy buckets
```

```
cargo clippy -p smithy --all-targets
```
