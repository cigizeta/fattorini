# Freezing the test set

Design only: nothing here is implemented yet. Written 2026-10-17 with the developer. Companion to
pgn-stream-training.md, which trains from PGN files without `smithy prepare`; together they replace
prepare's dataset cache.

## Goal

A test set built once, then never changed: every run on a corpus, whatever its architecture, is
judged on the same positions, and no test game ever feeds training.

## Test games live in their own directory

A corpus has two directories of `.pgn.zst` files, each file beside its `.count` of trainable
positions, every game in the shape `arena self` writes (`arena`'s `pgn.rs`): a `[FEN]` header, a
`[Result]`, and a `{<score>}` comment after every move.

- **The training directory**, read by training alone.
- **The test directory**, read by the freezer alone.

No game is in both, by construction. Training skips nothing and needs no criterion: no hash, no
end point, no corpus order to keep stable as the corpus grows. How the test directory is filled
depends on the producer.

### Self-play: generated with `arena self --test`

Test games are played on purpose, by a self-play run of their own (the training machine is fast enough), with the
same teacher and settings as the training games, written to the test directory instead of the
training one. Training loses no game to the test set.

- **Settings:** self-play is depth 9 on every machine today, so which machine plays the test games
  does not matter. When self-play moves to MCTS, likely searched by time or visits, the test run must
  keep the training run's settings, or its scores come from a different distribution.
- **Independence from training games:** each worker draws openings from `RandomBook` with an
  OS-seeded RNG (`self_play.rs`'s `rand::rng()`), so the test run does not replay the training run's
  sequence. But both draw from the same book of about 2.5M openings: with tens of thousands of test
  games and many more training games, many test games start from an opening some training game also started from. Only the
  start and the first plies are shared, and the scores of a depth-9 search from there differ little
  between games. Accepted as is (the developer, 2026-10-17): no split book.

### Lc0: whole files held out

Lc0's games cannot be generated again, so some of the converted files are held out: the converter
(lc0-data.md) moves whole files to the test directory until it holds G games, and the rest goes to
the training directory. Which files, fixed once: the first by tar date, so later downloads only ever
add training files. Holding out costs those games' positions, G x L, which for Lc0's 2.5 to 3
billion positions is a few percent at most.

## How many test games: G

L is the mean positions per game (around 100, measured by the first freeze). The test set is
4 x 2^20 positions sampled from G games, about 4 x 2^20 / G per game.

**Decided: G = 2^16 = 65,536 games** for both producers (the developer, 2026-10-17), of which the
reservoir keeps 4,194,304 positions, about 64 a game. Hand-crafted self-play on another machine (2026-10-17,
12 games) averaged about 175 positions a game, not the 100 first assumed: about 11M positions in
11 or 12 segments, the reservoir keeping a smaller share of each game, which costs nothing.

**Why so few games is enough:** test games share nothing with training games, so positions from the
same test game no longer leak anything; their correlation only makes the test loss's estimate a
little noisier, never biased. Tens of thousands of independent games already estimate it far more
finely than the round-to-round changes patience reacts to. Spreading 4 x 2^20 positions over a
million games would play about 105M positions to keep 4%.

**Cost:** for self-play, a short run of the training machine's, a small fraction of a generation's self-play. For Lc0, about 6.5M held-out positions, under
0.3% of the corpus.

## The freezer

`smithy freeze`, run through `bin/smithy`.

**Input:** the test directory, the list of lambdas, and a seed.

**One pass** over every file of the test directory, sorted by name (only for reproducibility: any
fixed order would do):

1. Read each game.
2. Label every position of the game at every lambda (`labels.rs`'s `compute_labels`; the backward
   recursion needs the whole game).
3. Feed the positions, with their labels, to a reservoir (Algorithm R, Vitter 1985) of exactly
   **4 x 2^20 slots** (`TEST_POSITIONS_CAP` today), seeded by the seed: running it twice on the
   same directory gives the same set.

**Stops with an error** if the directory holds fewer than 4 x 2^20 positions.

**Memory:** 4 x 2^20 slots of a packed position (about 24 bytes) and one 4-byte label per lambda:
about 150 to 300 MB, fine on the Lichess machine and the training machine.

## The output: one frozen file

Written once into the test directory, backed up on Drive (it is the most precious file of a corpus:
every run is judged on it), in the seekable zstd-frame format of `smithy forge`'s caches
(`framed.rs`). A header records:

- the format version;
- the lambdas, in order;
- the seed, the number of games read, and the names of the files read;
- the position count, always 4 x 2^20.

Then the 4 x 2^20 rows, each a position followed by one label per lambda, in reservoir order (the
test pass reads them whole, so their order does not matter).

**Frozen means frozen:** test files added to the directory later are not part of it. The freezer
refuses to overwrite an existing frozen file unless it is adding lambdas (below).

## What training does with it

- Loads the frozen file at startup and keeps it in memory for every round's evaluation.
- Stops with an error if the run's lambda is not among the file's lambdas.
- Reads the training directory only.

## Adding a lambda

Freeze again with the longer list, over the files the header names, with the same seed: the same
positions come out, so only the labels are added. The pass reads only the test games, so this is
cheap.

## Against prepare's test half

Today (`dataset_cache.rs`'s `is_test_position`) every position flips its own coin, of probability
`min(5%, TEST_POSITIONS_CAP / total)`, inside prepare's pass. What changes:

1. **Separate games, not separate positions.** Today a test position's neighbours from the same game
   are usually in training, so the test loss partly measures memorization, and patience and the
   learning-rate steps run on it.
2. **Exactly 4 x 2^20**, not on average.
3. **Frozen and separate:** it does not change as the corpus grows.
4. **For self-play, no training data spent on it.**

Unchanged: the size, one label per lambda, a fixed seed.

## Open points

- **L**, the mean positions per game, for each producer, measured by the first freeze; G revisited
  with it.
- **Where `--test` writes:** the test directory's path beside the teacher's training segments.
- **lc0-data.md's Validation section** (held out by `hash(game-id) mod 64`) is superseded by this
  document once accepted.
