# smithy freeze: implementation plan

Implementation plan, 2026-10-17, drafted for the developer to review. Implements
test-set-freeze.md for self-play (Lc0 later, same command over `data/lc0/test`). Follows
self-play-train-test-dirs.md, which fills `data/self/<teacher>/test/`.

## Command

```
bin/smithy freeze <teacher> <network>
```

- `<teacher>`: `hand-crafted` or `<network>-NNNN`, as `smithy teacher` and `arena self` name it.
- `<network>`: whose `label.lambda` list (`preprocessing_config::load_lambdas`) the labels are
  computed at. A teacher's test set serves every student; each student brings its own lambdas.

## Output: prepare's test half, frozen

Under `data/self/<teacher>/frozen/`, local, then uploaded to the same path on Drive:

```
test.positions                  cache.rs position records, framed.rs (FRAME_ROWS), + .frames, .count
lambda-0.95/test.labels         one f32 per position, same rows, + .frames, .count
lambda-0.95/manifest.txt        written last: this label set is complete
manifest.txt                    written last: the positions are complete
```

The same files, names and formats as prepare's test half (`dataset_cache.rs`,
`dataset_paths.rs`), so forge's test pass reads them unchanged, only from another directory.

`manifest.txt` records: the seed, the test segments read (bare names, in order), the games and
positions read, and the rows written (always 4 x 2^20).

## The pass

1. List `data/self/<teacher>/test/*.pgn.zst` on the remote, sorted by name. Sum their `.games`;
   stop with an error below `self.yaml`'s `test.games`, so a freeze never runs on a test directory
   still being played.
2. Read every game in order (`SelfPlaySegmentReader`), each game's positions
   (`self_play_game_positions`) and labels at each lambda (`self_play_game_labels`).
3. Algorithm R over the stream of positions, 4 x 2^20 slots, seeded by a fixed
   `FREEZE_SEED`: position i (from 0) fills slot i while i < k; afterwards it replaces slot
   `r = uniform(0..=i)` if r < k. Each slot holds the position and one label per lambda.
4. Assert the positions read equal the sum of the segments' `.count`, and that there are at least
   4 x 2^20 of them.
5. Write the slots in slot order: the positions file, then each label file, then the label
   manifests, then `manifest.txt`. Upload the directory.

**Memory:** a slot is a `CanonicalPosition` plus 4 bytes per lambda: a few hundred MB at 4 x 2^20
slots for six lambdas.

## Adding lambdas

`freeze` on a teacher already frozen, for a network whose lambdas it lacks: replays the pass over
the segments its manifest names (not the directory's current list), with the same seed. Which
position fills which slot depends only on the stream length and the seed, never on the content,
so the same slots come out: positions are not rebuilt, only the missing label files are written,
and each new label manifest must equal the positions manifest. With nothing missing, it prints so
and exits.

Segments added to the test directory after the freeze are never read: frozen means frozen.

## Tests

- The reservoir: fewer than k positions keeps them all in order; a fixed seed gives the same slots
  twice; over many draws every position is kept with probability about k / n.
- A label-only pass over the manifest's segments puts each label beside its own position.
- The refusals: too few games, too few positions, a `.count` that disagrees with the games read.

## Not in this plan

- Forge reading the frozen test set instead of the dataset's test half: comes with
  pgn-stream-training.md.
- Lc0.

## Decided (the developer, 2026-10-17)

- **Where:** `data/self/<teacher>/frozen/`, beside `train/` and `test/`.
- **The lambdas:** inferred from the network named on the command line; the freezer adds the
  label sets a frozen test set lacks.

## Implemented

`crates/smithy/src/freeze.rs`, `smithy freeze` in `main.rs`. The reservoir draws by a SplitMix64
hash of the index under the seed rather than a library RNG, so no dependency update can change the
slots a later label pass draws.

## First freeze (2026-10-17, the training machine)

`bin/smithy freeze hand-crafted caroline`: 4,194,304 positions drawn from 10 segments, 70,039 games
and 10,021,146 positions, labelled at lambda 0.95 to 1.00, in `data/self/hand-crafted/frozen/` and
uploaded to Drive. Sampling took 11 s (about 870,000 positions/s); writing and uploading ran silent
after it (freeze-progress.md).
