# A seekable train cache

**Status (2026-10-08):** implemented (`src/train/framed.rs`, the reader, `dataset_cache.rs`'s
writers, every test fixture); tests to run. `.frames` starts with a `frame_rows` line, so tests
write small frames with no extra parameter on the reader. To `done/` once generation 2's cache is
built and a resume is seen to be fast. Wanted before generation 2's
`prepare`, which builds a fresh cache anyway, so the new format costs no extra rebuild.

## The problem

Every cache file (`train.positions`, `test.positions`, each lambda's `train.labels` and
`test.labels`) is one zstd stream of variable-length records. `reader.rs::CacheFile::seek_to`
reaches row `target` by reopening every file of the cache and decoding `target` records of each:
resuming betty at step 192,895 sat silent before the shuffle buffer's fill while 9 streams decoded
in lockstep up to the cursor (TODO.md, 2026-10-06). The cost grows linearly with the cache.

Only two callers seek: a resumed run (`train_stream.rs`, from the persisted cursor) and the
cyclic wraparound (to row 0, already cheap). Steady-state reading never seeks.

## Design

- **Frames of a fixed row count.** Each cache file is written as consecutive independent zstd
  frames: the header alone in frame 0, then `FRAME_ROWS = 2^20` records per frame, the last frame
  short. Concatenated frames are a valid zstd stream, so a sequential read is unchanged.
- **A sidecar index, `<file>.frames`:** one line per row frame, its byte offset in the file, written
  beside `<file>.count` once the file is finished. Rows are fixed per frame, so row `r` is in frame
  `r / FRAME_ROWS`, at record `r % FRAME_ROWS` within it; no row numbers need storing.
- **Seek:** open the file, seek to the frame's offset, start a fresh decoder there, discard at most
  `FRAME_ROWS - 1` records. Every file of a cache shares the same row boundaries, so the lockstep
  positions and labels stay aligned by construction.
- **Write:** a framed writer in place of `open_writer`/`finish_writer` (`dataset_cache.rs`): counts
  records, and every `FRAME_ROWS` finishes the current encoder, records the file offset, and starts
  a new one on the same file. Used for both halves and every file, so there is one format. The
  `length_cache.rs` layout (transformer only) is untouched.
- **A line before the fill**, so the wait is never silent: `resuming at row R: seeking every cache
  file to frame F` (or the shuffle buffer's own `filling ...`).
- **Validation:** the reader checks the index's frame count against `<file>.count`
  (`ceil(rows / FRAME_ROWS)` lines), and that offsets increase; anything else is refused.

## Cost

Compression barely changes: a frame of 2^20 records is tens of MB, far above zstd's window. A seek
decodes at most one frame per file, about a second, against up to a whole pass today.

## Old caches (decided 2026-10-08, the developer)

A cache without `.frames` is refused, naming the missing index: caches already built belong to the
previous generation and are to be purged, so there is one format and one read path.

## Tests

- A framed file reads back the same records as today's stream, across frame boundaries, with a
  short last frame and with exactly `FRAME_ROWS` rows (no empty frame).
- `seek_to` lands on the right row at a frame start, mid-frame and in the last frame; in lockstep
  across a positions file and two label files.
- A wrong or missing index is refused.
- Small `FRAME_ROWS` in tests, through a parameter, so fixtures stay tiny.
