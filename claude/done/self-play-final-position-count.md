# Self-play counts the final position

## The defect

`bin/rotor-train prepare amanda` (generation 1, source `hand-crafted`) panicked at the end of its
build:

```
assertion `left == right` failed: the segments hold 54472036 positions but 54377420 were expected
```

A game of `p` plies passes through `p + 1` positions; with `b` book plies, its training positions
are positions `b` to `p` inclusive, `p - b + 1` of them: the `p - b` searched positions, each with
its `{score}` after the ply played from it, and the final position, which no ply leaves, labelled by
the game's result. The final position is given to the network and counted.

- The build counts `moves - book + 1` per game (`CollectedSelfPlayGame::position_count`).
- The replay counts the same way, from the moves: `book_and_positions` in `chunk.rs` writes it to
  `sample.txt`, `game_positions` in `replay.rs` recomputes it, `seal` refuses a mismatch.
- The self-play counts the non-empty `{...}` comments of the PGN text (`trainable_positions` in
  `self_play.rs`): one per searched position, so the final position is missed, one per game.

The 100 self-play segments of `hand-crafted` are correct; only their `.count` files are one per game
low. 94,616 missing positions over 10,156,540 counted, about 107 per game: plausibly 94,616 games.
Nothing compared the two conventions before the build, the first place both sources meet one
formula.

## The change

1. **`self_play.rs`: count plies, not comments.** The worker has the game's moves at hand
   (`game.moves`), and self-play writes no book plies (it starts from `starting_fen`), so a game's
   positions are `game.moves.len() + 1`. Replace `trainable_positions(&pgn_text)` with that,
   delete `trainable_positions` and its two tests, and add a test that a game of `n` moves sends
   `n + 1` positions (or test a small `game_positions(&game)` helper, mirroring `replay.rs`).
   Fix the doc comments that say the final position is not counted.
2. **`dataset_cache.rs`**: its doc of `build_dataset_cache` already states the right rule; no change.
3. **`rotor-arena self`'s stopping rule** (more than `self_play` positions) now counts final
   positions: future runs stop about 1% sooner. Accepted; `configs/corpus.yaml` sizes the corpus
   in training positions, which is what is now counted.

## Repairing the existing `.count` files

The 100 `.count` files on Drive under `self/hand-crafted` each need their segment's game count
added. A one-off, done by the developer from a shell rather than with new code: for each segment,
count its games (`[Event ` lines) and rewrite the `.count` as old plus games. The commands are
given in the conversation that wrote this plan, not stored here. Local `.count` copies left on
the self-play machines are stale too; they only feed `rotor-arena self`'s stopping rule for
`hand-crafted`, which is complete, so they can be deleted or left.

Then `bin/rotor-train prepare amanda` again: the partial files in `data/datasets/hand-crafted/`
have no manifest, so the build starts over. Nothing was deleted by the failed run
(`delete_sampled_chunks` runs only after a successful build).

## Verification

- `cargo test` for the self-play module.
- The repaired `.count` files sum to 10,251,156 (10,156,540 + 94,616); the rerun of `prepare`
  passes its assertion at 54,472,036 positions.
