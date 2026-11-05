# Move the curated database and prepared caches out of `data/games/`

## Status

Implemented. Not yet built/tested by the developer (no Bash access from this session).

## Decision

`data/games/` should hold only real source data (`.pgn`, `.7z`), not generated artifacts.
Confirmed with the developer:

1. `rotor-curate generate`'s own SQLite database moves from `data/games/` to `data/teachers/`,
   flat, same filename pattern as today (just a different parent directory).
2. `rotor-train prepare`'s own `positions.cache`/label files move from `data/games/` to
   `data/caches/`, same layout as today under that new parent.
3. `data/games/{stem}.pgn` itself is unaffected.

This does **not** revive the old "teacher" *config* concept `rotor-preprocessing-without-
teachers.md` deliberately removed (a named `teacher.yaml` resolving to a database) -- `data/
teachers/` and `data/caches/` are reused here purely as directory names for generated artifacts,
with no config format or CLI change attached. `net.yaml`'s `preprocessing` section still names
everything directly, unchanged.

## New paths

| artifact | old | new |
|---|---|---|
| PGN source | `data/games/{stem}.pgn` | unchanged |
| curated database | `data/games/{stem}.depth{D}.db` | `data/teachers/{stem}.depth{D}.db` |
| positions cache | `data/games/{stem}.depth{D}/positions` | `data/caches/{stem}.depth{D}/positions` |
| label files | `data/games/{stem}.depth{D}/labels/lambda-{L}` | `data/caches/{stem}.depth{D}/labels/lambda-{L}` |

## File-by-file plan

### `src/bin/preprocessing_config.rs`
- `database_path`: change its literal prefix from `data/games/` to `data/teachers/`.
- `positions_dir`/`labels_dir`: change their literal prefix from `data/games/` to `data/caches/`.
- `pgn_file_path`: unchanged.
- Update the module-level doc comment (currently says every database/cache is per-PGN-file under
  one shared naming convention) and each function's own doc comment to name the correct directory.
- Tests: update the expected path strings in `database_path_bakes_depth_into_the_filename` and
  `positions_dir_and_labels_dir_share_the_same_stem_depth_key`.

### `src/bin/rotor_curate.rs`
- `open_database`: `Connection::open` does not create missing parent directories (unlike
  `length_cache.rs`'s `create_split_directories`, which already `create_dir_all`s the cache tree).
  Since `data/teachers/` may not exist yet on a fresh checkout, add a `std::fs::create_dir_all` on
  the database path's own parent directory before opening it.

### `src/bin/rotor_train/main.rs`
- `stem_dirs_keys_every_pgn_stem_by_the_same_depth_lambda` test: update its expected
  `data/games/...` path strings to `data/caches/...`.

### `src/bin/rotor_train/length_cache.rs`
- The `#[ignore]`d diagnostic test `inspect_a_sample_of_real_test_rows_against_hand_computed_
  material` hardcodes `data/games/LumbrasGigaBase_OTB_ELO2400.depth0/{positions,labels}` -- update
  to `data/caches/...` to match where a real `prepare` run will actually have written them.

### Existing generated data
No generated `.db`/`positions.cache`/label files exist yet in this repo (`data/` currently holds
only the PGN source and its `.7z` archive), so there is nothing to move by hand.

## Verification plan
- `cargo build --release` and `cargo test` both pass.
- Spot-check `rotor-curate generate` against a small fixture network/PGN and confirm it creates
  `data/teachers/` on a checkout where it doesn't exist yet.

## Progress log

2026-09-22: Implemented as planned.
- `preprocessing_config.rs`: `database_path` now returns `data/teachers/{stem}.{suffix}.db`;
  `positions_dir`/`labels_dir` now return `data/caches/{stem}.{suffix}/...`; `pgn_file_path`
  unchanged. Module and function doc comments updated; tests updated to the new expected paths.
- `rotor_curate.rs::open_database`: added `std::fs::create_dir_all` on the database path's own
  parent directory before `Connection::open`, since SQLite (unlike the cache builders in
  `length_cache.rs`, which already `create_dir_all` their own tree) does not create a missing
  parent directory itself, and `data/teachers/` may not exist yet on a fresh checkout.
- `rotor_train/main.rs`: `stem_dirs_keys_every_pgn_stem_by_the_same_depth_lambda` test's expected
  paths updated to `data/caches/...`.
- `rotor_train/length_cache.rs`: the `#[ignore]`d diagnostic test's hardcoded paths updated to
  `data/caches/...`.

Verified by repo-wide grep that no other source file hardcodes a `data/games/{stem}.<db/depth...>`
path; the only remaining `data/games` references are the legitimate PGN-source path.

**Not done in this session** (no Bash/build access): running `cargo build --release` and
`cargo test`, and a real `rotor-curate generate` run to confirm `data/teachers/` gets created
correctly on a checkout where it doesn't exist yet.
