# Replay and self-play: one zstd encoder, level 19, on every core

Decided with the developer 2026-10-05. **Status:** done; `compression.rs` is the one encoder of
the replay and self-play (checked 2026-10-14).

## Why

Replay wrote its chunks at zstd level 19, self-play its segments at level 3. The PGN is the same
format; the level is most of why a segment is larger than a chunk on Drive, whose 15 GB is the
project's tightest storage. Both now go through one encoder, `src/arena/compression.rs`: level 19,
a content checksum (as both had), one worker per logical core.

## The cost, and why every core

Both compress while every search thread waits: replay's `seal` after a chunk, self-play's
`compress_sealed` after a segment. A chunk or segment is about 1 MB of text (replay's own note),
about 100k positions, searched in 25 to 30 s on the training machine now that replay's rate is presumably close to
self-play's (not re-measured since the throughput work). Level 19 on one thread runs at a few MB/s
(zstd's typical figure, not measured here), so a fraction of a second: a percent or two, smaller
than first estimated (10 MB a segment was wrong). Multithreading takes most of that back for a few
lines.

**The job size must be set.** zstd's multithreaded mode splits its input into jobs, by default
several times the window, 8 MB at level 19: larger than a whole chunk, so one job on one thread.
The job size is set to zstd's minimum, 512 KB (it raises anything smaller to that), so a chunk is
two or three jobs. A modest gain: the real cost was small to begin with.

## Changes

1. `Cargo.toml`: `zstd = { version = "0.13", features = ["zstdmt"] }`.
2. `src/arena/compression.rs`, new: `encoder(writer)`, level 19, checksum, workers, job size.
3. `replay.rs`: `compress` uses it; `OUTPUT_COMPRESSION_LEVEL` removed.
4. `self_play.rs`: `compress_sealed` uses it; `COMPRESSION_LEVEL` removed.

Readers are unchanged: a multithreaded stream is an ordinary zstd stream.

## Verification (on Windows)

- `cargo test --release` passes, the compression round trips of `self_play.rs` included.
- At the next replay and self-play: a chunk's and a segment's compression stays well under a
  second, and segments shrink on Drive to about a chunk's size per position.

Not on the label path: compression changes no search, evaluation or probe.
