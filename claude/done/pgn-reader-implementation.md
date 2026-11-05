# The PGN stream reader: implementation plan

Implementation plan, 2026-10-17, drafted for the developer to review. The reader of
pgn-stream-training.md (steps 3 to 4 of its flow: workers, channel, buffer), built first with a
benchmark, since its rate decides whether training can read compressed PGN directly. If it cannot
keep the GPU fed, a binary format of our own replaces the PGN (the developer, 2026-10-17); the
reader's shape stays.

## Module: `crates/smithy/src/pgn_stream.rs`

```
PgnStream::start(files, lambda, threads, open_files, keep_probability, seed) -> PgnStream
PgnStream::next() -> Option<Labelled>     // None once the epoch's files are drained
drop(PgnStream)                            // workers exit on their next send
```

- **`files`:** `.pgn.zst` paths with their `.count`, in a list the caller shuffles per epoch.
- **Workers:** `threads` threads, each owning `open_files / threads` open files. Each picks one of
  its files at random weighted by the positions it has left, reads that file's next game
  (`SelfPlaySegmentReader`), labels it at `lambda` (`self_play_game_labels`), replays its
  positions (`self_play_game_positions`), keeps each with probability `keep_probability`, and sends
  the kept ones down one bounded channel. A file that runs out is replaced by the next of the list,
  shared through an atomic index.
- **One way only:** nothing flows back to the workers. When the stream is dropped, the receiver goes,
  and every worker exits on its next failed send.
- **`Labelled`:** a `CanonicalPosition` and its `f32` label.
- **Randomness:** each worker seeded from `seed` and its index. Resume does not need it
  reproducible (pgn-stream-training.md).

The shuffle buffer and batches stay the training thread's, outside this module.

## Benchmark: `smithy stream-bench <teacher> <threads> [--seconds S]`

Opens the teacher's segments on the remote, old tree (`legacy_self_play_dir`) or new
(`self_play_train_dir`), whichever holds segments, starts a `PgnStream` with `keep_probability`
1 and `open_files` 64, drains it for S seconds (default 120), discards everything, and prints:

- positions per second, total and per thread;
- games per second;
- compressed megabytes read per second, against Drive's read speed.

Run once per thread count (1, 4, 8, all) to see whether it scales.

Drive is in streaming mode but the `rotor` folder is kept offline (the developer, 2026-10-17), so
the segments are read from local disk and the benchmark measures the CPU, not the network.

## Measured on another machine (2026-10-17)

amanda-0002's 717 legacy segments, 73,613,518 positions, read whole from local disk (Drive's
`rotor` folder offline), self-play paused:

| threads | positions/s | per thread | time for the corpus |
|---|---|---|---|
| 1 | 796,889 | 796,889 | 92.4 s |
| 4 | 3,386,097 | 846,524 | 21.7 s |
| 8 | 4,575,512 | 571,939 | 16.1 s |
| 12 | 4,748,963 | 395,747 | 15.5 s |

Linear up to 4 threads, flattening near 4.7M beyond: about what another machine's physical cores give, with the
runs at 8 and 12 too short (16 s) to separate that from the ramp and the tail of a drained list.
The mean label is -0.002 and amanda's games average about 130 positions. Compressed PGN is fast
enough unless forge trains well above 4M positions a second; no binary format is needed so far.

## What the number is compared with

The positions per second forge trains on, on the training machine: rows per step times steps per second, from its
progress bar. Measured when the training machine is available.

## Tests

- Every position of every file comes out exactly once with `keep_probability` 1, in some order,
  across several workers and more files than open slots.
- With `keep_probability` p, about p of them come out.
- Dropping the stream mid-epoch ends every worker (the test joins them).
- Labels match `self_play_game_labels` for the same game.

## Not in this plan

Forge reading the stream (the training loop, the test pass on the frozen set, resume), until the
benchmark says the PGN is fast enough.
