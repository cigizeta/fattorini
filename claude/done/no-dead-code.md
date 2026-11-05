# No dead code

Decided with the developer (2026-10-04): dead code is no longer tolerated. Every build fails on it,
and what is dead today goes.

## Constraint: caroline keeps running

Caroline's replay and self-play run from the main tree (`bartolomeo`). A restart, the next round,
or another machine pulling the branch rebuilds through `bin/`. So the work happens on branch
`carlo`, in the worktree `../rotor-carlo` with its own `target/`, and is merged into `bartolomeo`
only once it builds with no warnings and its tests pass (see TODO.md).

Every change below either deletes code the compiler proves unused, or moves code between files
unchanged. Neither changes what self-play and replay do.

## The lint

In `Cargo.toml`:

```toml
[lints.rust]
dead_code = "deny"
unused = "deny"
non_shorthand_field_patterns = "deny"
```

`unused` also covers unused imports, variables and `mut`. Not `warnings = "deny"`: a new
toolchain's lint would then break the build. The lint is added last, once the tree is clean, so no
intermediate commit fails to build.

## What is dead today

The full `--features training --all-targets` build (2026-10-04): 25 warnings in `rotor-arena`, 46 in
`rotor-train`, 4 in its test build (3 duplicates), none in the library or the benches. The library
reports nothing because its items are `pub`, never reported dead.

1. **Shared pipeline files, each binary blind to the other's half** (about 38).
   `pipeline/generation.rs`, `pipeline/preprocessing_config.rs`, `pipeline/corpus_config.rs` are
   `#[path]`-included by both binaries. Arena never uses the training side (weights and candidate
   paths, `read_verdict`, `promotable_challenger`, `training_state_check`, the dataset paths,
   `Split`, `Preprocessing::depth`); train never uses the arena side (selection, standings, duel
   and verdict writers, `read_champion_lambda`, `load_preprocessing`).
2. **Dead in both binaries**: `opponent_network_path`, `self_play_segments`, `self_play_pgn_path`.
3. **The old labelling pipeline** in `train/length_cache.rs` (`PositionCacheBuilder`,
   `LabelCacheWriter`, `PgnWorkerPool`, `DispatchedGame`, `GameResult`,
   `label_and_featurize_self_play_game`, `write_row_count`, `skip_to`, `piece_count`,
   `create_split_directories`, `create_for_write`, `COMPRESSION_LEVEL`,
   `PER_WORKER_DISPATCH_CAPACITY`), with `train/cache.rs`'s `write_record` and `HEADER_SIZE`:
   what the replay's labelling left behind.
4. **Scattered**: `feature_set.rs`'s marker machinery (`MAGIC`, `FORMAT_VERSION`, `write_marker`,
   `read_marker`, `MarkerDecision`, `decide_marker`) and its CLI and discriminant conversions;
   `chunk.rs`'s `parse`; `early_stopping.rs`'s `replay`; `reader.rs`'s `open`; `batch.rs`'s
   `to_dense_batch`; `Sprt::opponent_score`; `CorpusConfig::window_positions`.
5. **Not dead code**: `round.rs:182`, two redundant field patterns (`rows_done: rows_done`).

## Decisions

- **Group 1: the contract into the library, the rest split (revised 2026-10-04, after step 2).**
  Splitting everything fails for the writer/reader pairs: the gate verdict (arena writes, train
  reads), the selection (arena writes, train's `promotable_challenger` reads), the champion's lambda
  (train writes `champion.txt`, arena reads). Split across binaries, each format would have two
  definitions free to drift; kept in a `#[path]` file, each binary sees the other's half as dead.
  So:
  - **`src/pipeline/` becomes the library module `rotor::pipeline`**, `#[path]` inclusions gone,
    holding the file contract: the pairs, with their round-trip tests, and the paths and helpers
    both binaries use (`data_root`, `remote`, `corpus_config`, the shared part of `generation` and
    `preprocessing_config`). Its `pub` items are invisible to the lint, the accepted cost: the
    module is kept to the contract, so review covers it.
  - **One-sided helpers move into their binary**, where the compiler checks them: train's weights
    and candidate paths, `training_state_check`, the dataset paths and `Split`, `positions_dir`,
    `labels_dir` and `Preprocessing::depth`'s readers under `src/train/`; arena's standings and
    duel paths and writers, `load_preprocessing` and `window_positions` under `src/arena/`. Their
    tests move with them.
  - **`train/pgn_replay.rs`**, `#[path]`-included by the arena and `nnue_bench` under
    `#[allow(dead_code)]`, is checked the same way: one-sided parts to their user, shared parts to
    the library.
- **Groups 2 to 4: delete**, except an item used only by tests, which becomes `#[cfg(test)]`
  (`to_dense_batch` likely is).
- **Retired code the lint cannot see: the big five goes.** It is still reachable through the
  feature-set dispatch, so the compiler does not report it. It goes as a decision: betty is
  retired.
- **The transformer code stays**: it is alpha_pieces work half done, not retired. If it warns, its
  items get a narrow, commented `#[allow(dead_code)]` until the work resumes, the one exception.

## The big five's footprint

18 files mention it. To remove: `src/big_five.rs`, `src/train/big_five_network.rs`, its
`FeatureSet` variant and dispatch (`feature_set.rs`, `mod.rs`, `net_config.rs`, `batch.rs`,
`nnue_network.rs`), its `.nnue` loader in `nnue.rs` (the tag stays reserved, as tag 0 is), its
mentions in `lib.rs`, `features.rs`, `adamw.rs`, `test_cache.rs`, the benches, and
`configs/betty.yaml`.

To check before deleting, not assumed: `eval.rs` and `eval_terms.rs` mention it, and the big five
computed attack sets (mobility, king danger). Anything there the hand-crafted evaluation or
psq_threats uses stays, moved if need be.

`data/networks/betty/` is not touched: data, not code.

## Steps

Each its own commit, building and tests passing.

1. **Big five out**: the footprint above.
2. **Groups 2 to 4**: deletions, test-only items under `#[cfg(test)]`; the `round.rs` patterns.
3. **Group 1**, as decided above, in two commits: first `src/pipeline/` moved into the library
   unchanged (mechanical: `#[path]` inclusions become `use rotor::pipeline::...`), then the
   one-sided helpers moved out into their binaries. Every `#[allow(dead_code)]` goes, including
   `SEARCH_DEPTH`'s and `nnue_bench.rs`'s.
4. **The lint**, and a full `--features training --all-targets` build with zero warnings.
5. **Merge** into `bartolomeo`, worktree removed.

Step 1 goes first because it may turn more code dead, which steps 2 and 3 then catch in one pass.
