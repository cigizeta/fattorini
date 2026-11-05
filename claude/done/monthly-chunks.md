# Monthly chunks

Design, settled with the developer on 2026-10-03. Branch: `alessio`. Not yet implemented.
Follows corpus-windows.md (done), whose chunking it replaces; the windows, the monthly archives and
the self-play share stay as they are.

## Why

corpus-windows.md chunks a window as one stream: chunks cross month boundaries, so a window can only
grow (a month dropped would leave chunks half in, half out), and the games after the last full
chunk wait in a tail. With chunks that never cross a month, a window is just the set of months
whose chunks are used: shrinking it (say CCRL from 2016, to shorten each generation's 28-35 h of
labelling) becomes a configuration change, as does growing it. Nothing has been replayed yet, so
re-chunking now costs only a few hours of CPU.

## Decisions

- **A chunk never crosses a month.** Each month is cut on its own, in file order.
- **Fixed limit, as today:** a chunk closes at the first game boundary at or past `file` (100,000)
  positions, so a replay claims about 100,000 positions, never a whole month. A month's last chunk
  is whatever is left, possibly small. That is final, not a defect: a month never changes once on
  Drive, so its last chunk never needs completing.
- **TCEC the same, months not grouped.** Its months hold 2,000 to 19,000 positions, so nearly every
  TCEC month is one small chunk (about 136 of them); claims and uploads cost nothing next to the
  search. An empty month has no chunk.
- **Shrinking the windows is postponed**: the windows stay as they are; it is a configuration change
  for later.

## Layout

```
data/games/chunked/<archive>/<YYYY-MM>/chunk-NNN.pgn.zst(.count)    a month's chunks
data/games/chunked/<archive>/<YYYY-MM>/chunked.txt                  its record, written last
data/games/replayed/<archive>/<player>/<YYYY-MM>/chunk-NNN.pgn.zst  a chunk searched by the player
```

Three digits: the largest CCRL month (2025-01, 4.2M positions) has about 42 chunks.

`chunked.txt` per month: a header (`archive`, `month`, `file_positions`, `chunks`, `games`,
`positions`, `thrown`), then the `chunk` and `thrown` lines as today. Its presence makes the month
complete; an empty month has one with `chunks: 0`.

## Chunking: `rotor-train chunk`

- For each archive, every month of the window without a `chunked.txt` on the remote: read the
  month whole, check it against its `.count`, write its chunks locally, upload them, then the record
  last. A month already chunked is skipped. Months outside the window are left as they are.
- **Resumes by month.** A month stopped halfway is chunked again from its start (its local
  directory cleared first): a CCRL month is at most about 42 chunks, minutes of work. So the
  progress record, `consumed`, the pending-months file, the tail, and the "a window only grows"
  rule all go.
- Every other rule stays: games with no trainable position left out, corrupt games thrown and
  listed, a `[FEN]` stops the command, `[Source]` and `{ Book exit }` as today.

## Replay and prepare: the window decides

- `rotor-arena replay` and `rotor-train prepare` read `corpus.yaml` again: the chunks are those of
  the windows' months, each month's from its `chunked.txt`. A month of the window without one stops
  them, naming it (`chunk` was not run since the window grew).
- A chunk is named by archive, month and number (`ccrl 2009-04 chunk-003`). The replay's division
  among machines works on the list in (archive, month, number) order, as today on (archive, number).
- `prepare` takes the windows' replayed chunks, then self-play at `S` from their positions, as today.
  Replayed months outside the window are ignored, not deleted: widening a window back reuses them.
- `rotor-arena self` is unchanged: `S` from the windows' `.count`s, now with no tail, so equal to
  the replay's positions plus the thrown games' (none so far).

## Migration

Delete the chunks of corpus-windows.md, locally and on Drive (nothing was replayed from them; the
test replay's outputs are already deleted), then run `chunk`.

## Implementation plan

One commit: the replay and `prepare` read what `chunk` writes.

1. `corpus_config.rs`: `chunked_dir(source, month)`, `data/games/chunked/<source>/<YYYY-MM>`.
2. `chunk.rs`: `chunk_month(archive, month, file_positions)` replaces `chunk_archive`; `ChunkWriter`
   keeps `seal` and the thrown lines, loses the resume (`consumed`, `skip_entries`, `all_sealed`,
   the progress file); `months_to_chunk`, `months_to_read`, `recorded_tail`, `PENDING_MONTHS` go.
   Tests: a month's chunks never mix months (two fixture months), the last chunk of a month is
   short, an empty month has a record and no chunk.
3. `replay.rs`: `Chunk` gains `month`; `list_chunks` reads each window month's record; output,
   claim and staging paths gain the month directory.
4. `prepare.rs`: `RecordedChunk` gains `month`; `recorded_chunks(config)` per window month;
   segments `games/replayed/<archive>/<player>/<month>/chunk-NNN`.
5. Doc comments, `rotor-train`'s usage, `README.md`.

### Verification

```
cargo test --release
```

```
cargo test --release --features training --bin rotor-train
```

Then: delete the old chunks, `bin/rotor-train chunk` (each archive's chunk positions summing to its
window's `.count`s minus thrown games), and a test replay of one chunk, deleted afterwards.

## Progress

- 2026-10-03: implemented as planned, not yet compiled (no shell here). Details:
  - `chunk_month` returns the month's totals, `None` for a month already chunked; `run` checks every
    window month is on the remote first, then prints each archive's totals for the months chunked
    now.
  - `ChunkWriter::open` empties the month's local directory; `complete` uploads the chunks, writes
    `chunked.txt` straight to the remote, then removes the local directory (the remote is where
    every replay reads).
  - `replay.rs`: a chunk is keyed `(origin, month, number)`; `remote_state` lists only the month
    directories that exist under the player's output folder, so early on there are few to list.
  - Record lines keep their format with three-digit numbers (`chunk 003 entries ...`).
  Next: the developer runs the tests, deletes the old chunks, and runs `chunk`.
- 2026-10-03: tests passed, committed, old chunks deleted, `chunk` run. CCRL: 249 months, 2,986
  chunks, 285,615,946 positions (exactly the months' `.count`s), 0 thrown; largest month 2025-01,
  42 chunks. Same speed as the old chunking, reading from local disk: the work is CPU-bound on one
  core (parsing, legality, zstd 19); months are independent now, so chunking them in parallel is
  the change to make if it ever has to be repeated.
- 2026-10-03: a test replay of one chunk passed, its outputs deleted. Done.
