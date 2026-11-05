# smithy lc0: the converter

Implementation plan, 2026-10-17, drafted for the developer to review. Implements decision 3 of
lc0-data.md: test91's tars become self-play-shaped PGN segments in `data/lc0/{test,train}/`, so
`smithy freeze` and `smithy forge` read them as they read `data/self`.

## Command

```
bin/smithy lc0
```

No arguments: everything comes from `configs/lc0.yaml`. Run on one machine at a time (a
machine-wide lock, as `arena self`). Ctrl+C stops it after the tar in progress is abandoned
cleanly; the next run starts that tar over.

## Config: `configs/lc0.yaml`

```yaml
url: https://storage.lczero.org/files/training_data/test91/
# The first tar of the window, by the date in its name.
from: 20260101-0000
# data/lc0 on Drive, test and train together, is never taken past this.
cap_bytes: 5000000000
test:
  games: 65536
segment: 1000000
```

## The loop

1. **List** `url` (`curl.exe -s`), keep `training-run2-test91-YYYYMMDD-HHMM.tar` names from `from`
   on, with their sizes from the listing, sorted by name (date order).
2. **Skip** every tar with a row in `data/lc0/tars.tsv`, and every empty one (10,240 bytes; a row
   with zero games is written for it, so it is never listed again).
3. **Predict:** the tar's listed size times the largest `pgn_bytes / tar_bytes` in `tars.tsv`. If
   `data/lc0`'s bytes plus the prediction pass `cap_bytes`, stop, printing the bytes used and the
   next tar. With no row yet, no prediction: the first tar always converts.
4. **Where it goes:** `test/` while the test directory's games (summed from `tars.tsv`) are below
   `test.games`, `train/` afterwards. Decided per tar, so a tar is never split between the two.
5. **Download** to `%LOCALAPPDATA%\rotor\lc0\staging\` (`curl.exe -f -o <name>.tmp`, renamed when
   complete).
6. **Convert** (below) into local segments of the self-play shape, closing each at the first game
   boundary past `segment` positions; the tar's last segment closes at its end, however short.
7. **Upload** each segment with its `.games` and `.count`, `.count` last (`remote::copy_up`).
8. **Append the tar's row** to `tars.tsv` on Drive, via a `.tmp` and a rename: the tar is done.
9. **Delete** the tar and its local segments.

Startup clears the staging directory: anything there belongs to an unfinished tar. A segment of
that tar already uploaded is overwritten when the tar converts again (same name, same content).

## `tars.tsv`

```
tar	dir	tar_bytes	pgn_bytes	games	positions	segments
training-run2-test91-20260101-0017	test	411152384	...	...	...	3
```

`pgn_bytes` is the tar's segments compressed, the figure the cap and the prediction use.

## Segment names

`{tar-stem}-{NN}.pgn.zst`, `NN` from 00: a segment names its tar, and sorting by name keeps date
order, which freeze's manifest relies on for reproducibility.

## Conversion: one game

A member `training.<game-id>.gz`, gunzipped, is a flat array of 8356-byte V6 records, one per ply.

- **Checks, each a hard stop naming the tar and member:** size a multiple of 8356, at least one
  record; every record `version` 6, `input_format` 1.
- **Planes:** 104 u64 bitboards, 8 positions of 13 planes, the first 13 the current position from
  the side to move's view (ours P N B R Q K, theirs P N B R Q K, repetition), flipped vertically when
  Black is to move. The other 91 (history) are read only for the start position's en-passant square.
- **The start position** from the first record: pieces from planes 0 to 11, side to move from
  `side_to_move_or_enpassant`, castling from the four `castling_*` bytes, en passant from history
  plane set 1 (a pawn that just moved two squares), rule 50 from `rule50_count`, fullmove 1. Must
  be legal in shakmaty, otherwise a hard stop.
- **The moves:** record i's `played_idx`, decoded through Lc0's 1858-move table (generated in code,
  from the side to move's view). For every record but the last, the move is also checked by playing
  it and comparing the board with record i+1's planes: a mismatch is a hard stop, which catches a
  wrong table at the first game. The last record's move has no next board; it is still checked for
  legality.
- **The score after each move:** record i's `best_q` (W - L for its side to move, in [-1, +1]),
  converted with `labels.rs`'s inverse to centipawns and written in self-play's comment, under
  self-play's sign convention (to be confirmed against `self_play_game_labels` in the first test).
- **The result:** the first record's `result_q`, +1, 0 or -1 for its side to move, turned to White's
  `1-0`, `1/2-1/2` or `0-1`. Any other value is a hard stop.
- **Positions:** records + 1, the final position included, as self-play counts them.
- **Headers:** `[Event "lc0 test91"]`, `[Round "<game-id>"]`, `[White "lc0"]`, `[Black "lc0"]`,
  `[Result]`, `[FEN]`, `[SetUp "1"]`. No `[Termination]`: the record does not say (most games end by
  resignation).

`invariance_info` is printed for the first game of each run and otherwise ignored: if it shows
transformed boards, the board check above fails loudly and the plan is revisited.

## Dependencies

- **tar:** parsed by hand, a few dozen lines: 512-byte ustar headers, name and octal size, data
  padded to 512. Only regular files ending `.gz` are read; anything else is skipped.
- **gzip:** `flate2`, a new dependency of `smithy`: gzip is too much to write by hand.
- **HTTP:** `curl.exe`, part of Windows 10 and 11, launched by its full name (`Command`), so no HTTP
  crate.

## Code

- `crates/smithy/src/lc0/mod.rs`: the command, the loop, `tars.tsv`.
- `lc0/tar.rs`: the ustar reader over a `Read`.
- `lc0/record.rs`: the V6 record, its checks, the planes to a shakmaty position.
- `lc0/moves.rs`: the 1858-move table.
- `lc0/convert.rs`: a game to PGN text, segments.
- `rotor::pipeline::preprocessing_config`: `lc0_test_dir`, `lc0_train_dir` beside the self-play ones.

## Tests

- The tar reader on a tar built in the test: two members, one padded, one skipped.
- The 1858 table: 1858 entries, no duplicates, and known entries (index 0, `e2e4`, a promotion,
  an underpromotion) from both sides' views.
- A record built in the test, Black to move, decodes to the expected FEN.
- One real game: the first member of the October tar, copied into `crates/smithy/tests/data/` (about
  15 KB gzipped), converts with every board check passing, and its PGN replays through
  `self_play_game_positions` to records + 1 positions.
- The prediction: no row converts; a row with ratio r stops before a tar of size s when used + r s
  passes the cap.
- The test/train split: a tar goes to `test/` while the games are below `test.games`, wholly.

```bash
cargo test -p smithy lc0
```

## Not in this plan

- `labels: lc0` in forge and the `amanda-lc0`, `caroline-lc0` configs: a plan of their own, after
  the first tars are converted.
- The `.meta` sidecar (TODO): the converter writes `.count` and `.games` like self-play, and switches
  with it.
- Policy, `d` and `m`: not kept (lc0-data.md).


## Implemented (2026-10-17), `cargo test -p smithy lc0` passing on the training machine

The real game converted with every one of its 69 boards matching the moves played, so the plane bit
order, the castling bytes and the 1858-move table's order are as remembered, at least on that game.

`crates/smithy/src/lc0/` (`mod.rs`, `tar.rs`, `record.rs`, `moves.rs`, `convert.rs`), `smithy lc0`
in `main.rs`, `configs/lc0.yaml`, `flate2` and `shakmaty` in smithy. Where it departs from the plan:

- **The best move is kept:** each comment is `{<score> <best>}`, `best_idx` decoded through the same
  table, the generation-4 shape the reader already takes. It costs nothing and feeds the move head.
- **The score is not negated:** the comment is for the side to move (`self_play_game_labels` turns it
  White-relative), so it is `best_q` as the record has it.
- **The board check** compares every record's pieces with the board the previous moves reach, the
  first record included, rather than diffing two records' planes.
- **Staging** is `data/lc0/staging/`, and segments are sealed in `data/lc0/{test,train}/` and
  uploaded from there, as `remote` mirrors `data/`. Startup clears the two segment directories and
  staging's `.tmp` downloads, but keeps a complete `.tar`, which is converted without downloading
  it again (a January tar is 824 MB, 7 minutes at the training machine's 2 MB/s).
- **Two downloads ahead (the developer, 2026-10-17):** the first tar took 7 min to download at
  about 2 MB/s and 2.5 min to convert, about 40 hours for the cap, 217 tar bytes downloaded per
  position kept. One connection gets about 2 MB/s and the home line up to 5 MB/s, so tars N+1 and N+2
  download in background threads (`curl.exe -s`) while N converts: about 3.5 min a tar, about
  15 hours. A tar is fetched ahead only if it fits under the cap beside what `tars.tsv` holds plus
  the predicted output of every tar before it in the queue (the rows alone would undercount: the
  tars ahead have none yet); when the queue is empty, the decision on `tars.tsv` alone decides, as
  before. A finished download left at a stop stays in staging for the next run; an unfinished
  one's `.tmp` is cleared.
- **Standard chess only (the developer, 2026-10-17):** test91's book also opens Chess960 and
  Chess324 games, which the first real run hit. A game is converted only if every castling right has
  its king and rook home, and every knight, bishop or queen on a back rank stands on its standard
  home square (`is_standard_start`); others are counted in `tars.tsv`'s `skipped` column.
- **Sizes** come from `curl.exe -I` per tar (its `Content-Length`), not from parsing the listing.
- **No lock and no Ctrl+C handler:** Ctrl+C kills the run, and the next one clears the staging and
  converts the unfinished tar again. An uploaded segment of that tar is rewritten identically, so
  `remote::copy_up` skips it.
- **No `preprocessing_config` paths yet:** the converter's own constants; they move there with
  `labels: lc0` in forge, which needs them too.

## Decided (the developer, 2026-10-17)

- **`from: 20260101-0000`**, as lc0-data.md: the window starts with January's larger tars and runs
  forward until the cap.
- **The real game is copied into the repository,** not read from `~/lc0-sample`, which is to be
  deleted: `crates/smithy/tests/data/lc0-test91-229828019.gz`, the first member of
  `training-run2-test91-20261017-0417.tar`, with `crates/smithy/tests/data/LC0-LICENSE.txt` beside
  it (test91's `LICENSE.txt`, ODbL 1.0 and DbCL 1.0, with the tar and member it came from).
