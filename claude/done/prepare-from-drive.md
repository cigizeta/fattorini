# rotor-prepare: one pass, straight from Drive

Builds on corpus-positions.md and move-segments.md. rotor-self now moves every finished segment to
Drive and keeps only its `.count` locally, and a generation is complete once its segments' `.count`
files add up past `corpus_positions`.

## What the developer wants

1. **Read directly from `/mnt/g`.** No copy of the segments into `data/`.
2. **Read every segment of the generation**, even past `corpus_positions`.
3. **One pass** over the segments, not two.
4. **The split is a coin, not blocks of 33**: each position goes to test with probability 1/33,
   to train otherwise (first written as 0.05, changed by the developer to keep the 32:1 ratio, so
   a generation of at least `33 * 1024 * 512` positions expects at least 2^24 train rows).
5. **No SHA-256.**

rotor-prepare is started by hand once every machine is idle, so no segment should arrive during
a build; the edge case is still handled.

## 1. Which segments, and when

- Online: rotor-prepare lists the remote games directory and sums the remote `.count` of every
  `.pgn.zst`. Until the sum exceeds `corpus_positions` (the same rule as rotor-self) it prints
  `N of M positions (P%)` and checks again every `GENERATION_WAIT_CHECK`, as today. A `.count`
  that cannot be read is a warning and counts 0.
- Once complete, **that listing is the dataset**: every segment in it, in `choose_segments` order
  (timestamp, then machine), which the manifest records as today. `choose_segments` loses its
  `files` argument and takes everything.
- After the build, the remote is listed again. Segments not in the dataset are reported:
  `warning: N segment(s) arrived during the build and were not used`. The build still succeeds;
  a built dataset stays final.
- `--offline`: the same, over the local `data/self/{source}` only (segments of an offline
  rotor-self run, not yet moved). Refused unless the local `.count` files add up past
  `corpus_positions`.
- `download_missing` goes. The self-play config's `files` stopgap goes.

## 2. Reading from Drive

- Each segment is opened at `remote::remote_path(self_play_dir(source))/{segment}.pgn.zst`
  (offline: the local path). `build_dataset_cache` takes the directory to read from rather than
  deriving it from `self_play_pgn_path`.
- A read error mid-build is fatal, as any read error is today: no manifest is written, so a rerun
  starts over.

## 3. One pass

Two things forced the count pre-pass (`count_positions`): the cache header carries `row_count`
before the first record, inside the zstd stream; and the block-of-33 split needed the total to
send the trailing partial block to train. Both go:

- **The split** (item 4) needs no total at all.
- **`row_count` leaves the header** for a sidecar `{cache}.count` (one decimal number, written via
  `.tmp` and rename once the cache is finished), the same pattern `length_cache.rs` already uses
  for the same reason. The header keeps its magic, version and `MAX_NON_KING_PIECES` checks;
  `FORMAT_VERSION` goes 3 -> 4, so an old cache fails loudly instead of being misread.
- `CacheFile::open(path)` reads `row_count` from `{path}.count` and panics if it is missing. Its
  signature is unchanged, so no reader call site changes.
- `count_positions` goes. The manifest still records `positions`, `train_rows`, `test_rows`,
  counted while writing, and is still written last, after both sidecars.
- The sum of the segments' `.count` files (already read for step 1) sizes the progress bar, and
  at the end the build asserts that it equals the positions written. They must agree:
  `label_and_featurize_self_play_game` requires one score per move and yields one record per
  move, and `.count` counts scored moves.

Every other writer of `CacheHeader` writes the sidecar too:

- `builder.rs` `build_caches`: dead code today (only `is_train_game` is used from `builder.rs`);
  adapted minimally so it stays correct, not deleted in this change.
- Test helpers in `reader.rs`, `window.rs`, `shuffle_buffer.rs`, and `builder.rs`' tests: a
  shared test helper in `cache.rs` writes header, records and sidecar together.

## 4. The split

- `is_test_position(position_index) -> bool`: `splitmix64(position_index ^ DATASET_SPLIT_SEED)`
  mapped to `[0, 1)`, compared with `TEST_FRACTION = 1/33`. A pure function of the global index,
  so a rebuild over the same segments reproduces the same split, as today. Not `StdRng` per
  position: seeding a ChaCha generator 17 million times is needless cost.
- `SPLIT_BLOCK` and `test_offset_in_block` go; the doc comment about stratified blocks is replaced.
  At ~17M positions, 5% is ~865k test rows, and an independent coin's clumping is negligible.
- `net_config.rs` (the `training_batch_size` doc on the 32:1 ratio) and `configs/amanda.yaml` (the
  2^24 comment): an epoch of `rounds_per_epoch * steps_per_round * training_batch_size` is now
  about one pass over the ~95% train rows, not exactly one; the cursor carries over between
  epochs, as it already does.

## 5. No SHA-256

- rotor-self: `write_companions` writes only `.count`; the segment encoder enables zstd's own
  frame checksum (`include_checksum(true)`), verified automatically on every decompression, at no
  extra read. Segments already on Drive have none; they keep the protection of zstd's truncation
  detection and the PGN parser.
- `remote::is_segment_file`, `is_moved_file`, `recover_sealed`'s leftovers: no `.sha256`.
- rotor-prepare: `verify_segment` and its tests go.
- `sha2` leaves `Cargo.toml` if nothing else uses it.
- The `.sha256` files already on Drive stay; nothing reads them.

## Docs

`prepare.rs` doc comments (`run_prepare`, `prepare_self_play_dataset`), `dataset_cache.rs`
module and function docs, `README.md` (the rotor-prepare line), `self_play_config.rs` module doc.

## Tests

- `is_test_position`: deterministic; the test fraction over 1M indices is within 0.05 +- 0.002.
- Cache round trip with the sidecar; `CacheFile::open` panics without it; version 3 refused.
- `choose_segments` takes everything, in timestamp-then-machine order.
- A small end-to-end build from a temporary directory: one pass, row counts in the sidecars equal
  the rows read back, manifest written last; a `.count` sum that disagrees with the records
  panics and writes no manifest.
- rotor-self: a new segment's `.sha256` is not written; its zstd frame carries a checksum (a
  flipped byte fails to decompress).

## Order

1. SHA-256 removal and the zstd checksum (rotor-self, remote, prepare).
2. Cache sidecar and version 4 (`cache.rs`, `reader.rs`, every writer, test helpers).
3. The split.
4. `build_dataset_cache` single pass, reading from a given directory.
5. rotor-prepare: listing, completion by positions, the after-build check, no download.
6. Docs.
