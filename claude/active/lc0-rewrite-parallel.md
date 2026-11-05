# The Lc0 rewrite in parallel

**Status: agreed with the developer 2026-11-04, implemented the same day; to build, test and see
running.**

## Why

`smithy lc0-rewrite` (training-corrections-implementation.md) rewrote one segment at a time: about
89 s a segment with PeSTO's play beside it, CPU at 16% with it stopped, about 34 h for the 1,405
segments. Decompression and the text rewrite are single-threaded, and zstd's multithreading at level
19 cuts a segment into few large jobs, so most cores idled.

## The change

- Workers, the available cores less one (at least one), take segments from a shared index.
- Each worker rewrites a segment as before, with zstd single-threaded: write `.rewritten`, sync,
  record it in `rewrite-done.txt`, rename over the original.
- The done-file's appends go through a mutex, one whole line each, so lines never interleave. It is
  read as a set, so the order of its lines does not matter to a resume.
- A segment is still recorded before its rename: no segment is ever converted twice.
- `curve.txt` is written only after every worker has finished.
- Progress prints a count of segments finished, not an index, since they finish out of order.

## Unchanged

The level (19), the file format, the resume rules, the command.

## Check

Tests and clippy; the rewrite restarted, its pace measured again from the segments' timestamps.
