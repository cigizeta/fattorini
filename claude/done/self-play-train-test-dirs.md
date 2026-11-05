# Self-play into data/self, with a test directory

Implementation plan, 2026-10-17, with the developer. First step of pgn-stream-training.md and
test-set-freeze.md: `arena self` writes its games into a new tree on Drive,
`G:/My Drive/rotor/data/self`, split into training and test directories, as Lc0's corpus will be
(`data/lc0`, decided after this).

## Layout

```
data/self/<teacher>/train/{machine}-{YYYYMMDD-HHMMSS}.pgn.zst   (+ .count, + .games)
data/self/<teacher>/test/{machine}-{YYYYMMDD-HHMMSS}.pgn.zst    (+ .count, + .games)
```

`<teacher>` is `Teacher::source()`: `hand-crafted`, or `<network>/gen-NNNN`.

## Imitating Lc0: the game is the unit, the file is sized for compression

Lc0 publishes an hour of games per file, a varying number. Rotor's segments likewise close at the
first game boundary past `segment` positions (1,000,000, about 10,000 games): big enough for zstd
to be worth it, never splitting a game, its game count varying. The test set is a number of
**games**: `arena self --test` plays whole segments into the test directory until it holds at
least `test.games` (65,536, test-set-freeze.md), about 7 segments.

## Drive stays intact

Nothing under `data/games/` is moved, renamed or deleted. `smithy prepare` keeps reading
`data/games/self/<teacher>` and `corpus.yaml`; the old segments are deleted later by hand, to free
space. New self-play starts from zero in the new tree.

## Changes

1. **`configs/self.yaml`** (written by the developer), `arena self`'s config:
   ```yaml
   train:
     positions: 1000000000
   test:
     games: 65536
   segment: 1000000
   ```
   Read by a new `rotor::pipeline::self_config`, which refuses `segment: 0`, `train.positions`
   below `segment`, and `test.games: 0`. `corpus.yaml` is untouched: legacy, for `arena replay`'s
   archive months and `prepare`, until `arena replay` replays a generic PGN and prepare is gone.
2. **A `.games` companion** per segment, `{segment}.pgn.zst.games`: its number of games, counted
   from the sealed file's `[Event` headers at compression (which reads the whole file anyway), so
   the batch's staging and resume are untouched. Written before `.count`, both before the rename
   that makes the segment visible; kept locally after upload, like `.count`; uploaded with it
   (`remote::is_segment_file`). Crash recovery removes a `.games` left by an interrupted
   compression together with its `.count`.
3. **`preprocessing_config.rs`:** `self_play_dir` becomes `legacy_self_play_dir` (`prepare`,
   `buckets`), beside the new `self_play_train_dir` and `self_play_test_dir`.
4. **`arena self <player> [--test]`:** without `--test`, plays into the training directory until
   its segments hold `train.positions` (summing `.count`); with it, into the test directory until
   they hold `test.games` (summing `.games`). Same teacher, depth, book, batches, resume and upload.
5. **The machine-wide lock** moves to `data/self`. **The startup upload** covers `data/self` and
   still `data/games/self`, so a segment finished there by the old binary still reaches Drive.

## Before switching a machine

An unfinished batch (`{stem}.pgn` staging file) under `data/games/self` is not resumed by the new
binary: finish it with the old one first, or delete it.

## Verification

- `cargo test -p rotor self_config`, `cargo test -p rotor remote`, `cargo test -p arena`.
- `bin/arena self hand-crafted --test` writes its first segment with `.count` and `.games` to
  `G:/My Drive/rotor/data/self/hand-crafted/test/`; Ctrl+C then a restart resumes it.

## Results

- **2026-10-17: hand-crafted's test games are played** (`bin/arena self hand-crafted --test`,
  about 2 h). 10 segments in `G:/My Drive/rotor/data/self/hand-crafted/test/`, each with
  its `.count` and `.games`: 70,039 games (at least 65,536), 10,021,146 positions, about 143 a
  game. That is fewer than the about 175 Another machine's 12-game sample suggested (test-set-freeze.md), and
  enough for the freezer's 4 x 2^20.
