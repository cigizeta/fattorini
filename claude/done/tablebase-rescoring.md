# Tablebase rescoring: design and implementation plan

Status: designed 2026-11-03 with the developer; steps 1 to 6 applied the same day, to test. As
built, step 5's first test also checks a middlegame game is never probed. Answers the TODO's "Tablebase
rescoring of the Lc0 corpus, up to 6 pieces".

## Why

Lc0's labels in positions of 6 pieces or fewer are its own values, never rescored; Stockfish
rescores Leela's data with Syzygy before training. A wrong label there still trains the shared
transform, and quiescence leaves fall into the tables. Self-play's labels are no better there: the
search probes the tables only right after a zeroing move, so an ending reached by quiet moves is
labelled by the network.

## Decisions (the developer, 2026-11-03)

- **Always, no config key:** a correct label is never worth switching off.
- **Rescored, not dropped**, as Stockfish does.
- **Both ends replaced:** the label at lambda 0 (the search's score) and at lambda 1 (the game's
  result) both become the tables' result, so the label is the same at every lambda. Within the
  tables the true value is known; a won ending in a drawn game is a later blunder, not the
  position's value.
- **The ends of the label:** a win 1, a draw 0, a loss -1, from the side to move, as the replay's
  tablebase labels (replay-tablebase-labels.md) and a decided game's final position.
- **Under the fifty-move rule, from DTZ:** `SyzygyTablebase::root_result`, given the game's
  positions before the one probed, as the replay and the arena's early stop use it. A position it
  cannot answer (castling rights, a missing table) keeps its label.
- **No tables, no scatter:** the four sets of `bin/syzygy.sh` (WDL and DTZ, up to 6 pieces) are
  required, as the arena requires them; their absence is an error naming `bin/syzygy.sh`, never a
  silent scatter without them.
- **Step 0 restarted on it (the developer, 2026-11-03):** a safe network, not a test, so nothing is
  confounded. The developer deletes the run's directory by hand; its buckets are scattered again by
  themselves (below).

## Scope

- **The NNUE forges' buckets (`buckets.rs`):** rescored at the scatter, which is bound by Drive's
  download (the CPU about 8%), so the probes cost almost nothing.
- **Old buckets invalidated:** `buckets.tsv` gains a line `labels	tablebase`; buckets without it,
  or with another value, are scattered again from the start, as a `keep` line of another value
  already makes them.
- **Not in scope:** the frozen test set (`freeze.rs`), kept as it is so that every network's test
  loss still compares with every earlier one's (lc0-counts-forges.md checked the new corpus draws the
  old set exactly); the transformer's stream (`pgn_train_stream.rs`), no transformer trained now.
  The test loss therefore measures, in those few positions, distance from Lc0's labels, not the
  tables'; a small share, the comparison worth more.

## Steps

### 1. The tables' directories (`rotor::pipeline::data_root`)

`syzygy_directories` moved there from `arena::data_root`, which re-exports it, so the smithy finds
the same four sets.

### 2. The probe (`crates/smithy/src/game_labels.rs`)

`self_play_game_tablebase(collected, tablebase, game_index) -> Vec<Option<f32>>`, one per training
position as `self_play_game_labels`: the game replayed from its starting FEN, every earlier
position's hash kept, and each position of at most 6 pieces probed by `root_result` with them;
`Some(1.0 | 0.0 | -1.0)` from the side to move, `None` where the tables are silent or the position
has more pieces.

### 3. The stream (`pgn_stream.rs`)

`StreamOptions::tablebase: Option<Arc<SyzygyTablebase>>`: when set, a worker replaces every
lambda's label of each position the probe answers. `None` for the training stream of transformers
and the tests; `SyzygyTablebase` gains a plain `Debug`.

### 4. The scatter (`buckets.rs`)

- `prepare` loads the tables (`SyzygyTablebase::from_directories`), an error without them, and
  passes them to `prepare_in_batches`, which passes them to each batch's stream.
- `State::labels`, written `labels	tablebase` (with tables) or `labels	plain` (the tests, without);
  a mismatch scatters again from the start, said as the `keep` mismatch is.
- The tests call `prepare_in_batches` without tables through a local `prepare`.

### 5. Tests

- `self_play_game_tablebase`: a KQ v K game, its positions labelled 1 and -1 by side to move; skipped
  without tables, as the tablebase tests are.
- Buckets made without the `labels` line, or with `plain` when tables are given, are scattered
  again.

### 6. TODO.md

The entry marked done, pointing here; step 0's restart noted.

## Commands for the developer

```
cargo test -p smithy
```

```
cargo test -p arena
```

```
cargo clippy --workspace --all-targets
```
