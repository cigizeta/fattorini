# Drop `pv` and `quiet` from `rotor-curate`

## Status

Implemented. Not yet built/tested by the developer (no Bash access from this session; see progress
log for what to verify).

## Decision

Simplify the curation algorithm: remove `preprocessing.pv` (the `net.yaml` config field) and the
`quiet` column from the `plies` table entirely. This reverts the `pv`-driven part of what
`multipv.md` (step 4, "Consumer: rotor-curate's `is_position_quiet`, corrected") and `training.md`
("Quiet-position filtering", "Generalizing `pv > 1`") built: `rotor-curate` goes back to scoring
every ply with a single plain search, and `rotor-train` goes back to filtering training data on
score alone, with no tactical-liveness check.

The general-purpose UCI `MultiPV` feature (`multipv.md` steps 1-3: `Search::excluded_root_moves`,
`choose_best_move_multipv_with_progress`, `src/uci.rs`'s `setoption name MultiPV`) is **not**
touched -- it stands on its own merits independent of curation and stays exactly as implemented.

Confirmed directly with the developer:
1. `play_game` reverts to one plain `choose_best_move` search per ply (no MultiPV root-breadth
   search at all), not MultiPV with a fixed breadth constant.
2. `is_position_quiet`/`is_quiet_across_root_candidates` (`rotor_curate.rs`) and the `quiet`
   parameter of `rotor-train`'s `is_trainable` are removed entirely, not just left uncalled.
3. Existing generated `.db`/`positions.cache`/label files are simply stale once path naming drops
   `pv`; full regenerate, no migration path. (In practice there is nothing to regenerate yet --
   `data/networks/` doesn't exist in this repo.)

## Design after this change

- `net.yaml`'s `preprocessing` section loses `pv`: `Preprocessing { lambda, depth, pgn }`.
- `rotor-curate generate`: `play_game` scores every ply with `choose_best_move` (the same plain,
  single-search entry point `play_self_play_game` already uses), exactly as it did before `pv`/
  MultiPV were introduced. `depth == 0` still means a bare `evaluate()` call, unchanged. No
  quietness is computed at all. The `plies` table becomes `(game_id, ply_index, score)`.
- Every database/cache path collapses to a `(stem, depth)` key: `depth9`, not `depth9.pv1`.
- `rotor-train prepare`: `is_trainable(cp: i32) -> bool` keeps only the decisive/score-window
  check; a ply's tactical liveness no longer has any bearing on whether it's trainable.

## File-by-file plan

### `src/bin/preprocessing_config.rs`
- `Preprocessing`: drop the `pv: Option<u32>` field.
- `generation_suffix`, `database_path`, `positions_dir`, `labels_dir`: drop the `pv` parameter;
  the suffix is always `depth{D}` (the `Some(p)` branch and its `.pv{p}` case go away).
- Update doc comments describing the `(stem, depth, pv[, lambda])` key to `(stem, depth[, lambda])`.
- Tests: drop `pv` arguments/assertions in `database_path_bakes_depth_and_pv_into_the_filename` (and
  rename it), `positions_dir_and_labels_dir_share_the_same_stem_depth_pv_key` (and rename it), and
  `load_preprocessing_parses_the_committed_transformer_example` (drop the `pv == Some(2)` assertion).

### `examples/net_naive.yaml`, `examples/net_nnue.yaml`, `examples/net_transformer.yaml`
- Remove the `pv:` line from each.

### `src/bin/rotor_curate.rs`
- Remove `is_position_quiet`, `is_quiet_across_root_candidates`, and their unit tests, plus the
  `quiet_then_capture_fixture` helper.
- `PlyRecord`: drop the `quiet` field.
- `open_database`: the `plies` table's `CREATE TABLE` drops the `quiet BOOLEAN NOT NULL` column.
- `drain_ready_results`: the `INSERT INTO plies` statement and its bound values drop `quiet`.
- `play_game`: drop the `pv: Option<u32>` parameter and the `pv_depth` resolution. In the
  `depth > 0` branch, replace the `choose_best_move_multipv_with_progress(..., pv_depth, |_report,
  extra_lines| ...)` call with plain `choose_best_move(&board, depth, None, &ancestor_hashes,
  tt.expect(...), tablebase, 0, stop)`. Drop the `quiets: Vec<bool>` local and the `quiet` half of
  every tuple; `PlyRecord` construction becomes score-only.
- `run_worker`: drop the `pv` parameter and its use in the `play_game` call.
- `run_generate_core`: drop the `pv` parameter and its use in the worker-thread spawn closure.
- `run_generate`: stop reading `preprocessing.pv`; drop the `pv` argument to `database_path` and
  `run_generate_core`.
- Remove now-unused imports: `choose_best_move_multipv_with_progress`, `MultiPvLine`, `is_quiet`
  (all three become unused once the functions above are gone).
- Trim doc comments throughout (`play_game`'s own, `PlyRecord`'s, the `pv_depth` block comment,
  etc.) that explain the removed `pv`/`quiet`/MultiPV-for-curation behavior.
- Tests: update every `run_generate_core` call site to drop its trailing `pv` argument
  (currently `Some(1)`/`None`).

### `src/bin/rotor_train/main.rs`
- `run_prepare`: drop `let pv = preprocessing.pv;` and the `pv` argument to `database_path`,
  `positions_dir`, `labels_dir`.
- `stem_dirs`: drop the `pv` argument to `positions_dir`/`labels_dir`.
- Update doc comments mentioning the `(stem, depth, pv, lambda)` key.
- Rename and update the `stem_dirs_keys_every_pgn_stem_by_the_same_depth_pv_lambda` test: drop
  `pv: Some(1)` from its `Preprocessing` fixture and update the expected path strings.

### `src/bin/rotor_train/net_config.rs`
- Drop the `pv: ~` line from both raw-YAML test fixture strings.

### `src/bin/rotor_train/dataset.rs`
- Delete `PlyRow`, `prepare_game_plies_statement`, and `read_game_plies` entirely. Once `quiet` is
  gone these are redundant with the already-existing `read_game_scores`/
  `prepare_game_scores_statement` (today used only by the superseded single-file `builder.rs`), so
  `length_cache.rs` switches to those instead of keeping two near-duplicate query paths.

### `src/bin/rotor_train/length_cache.rs`
- `is_trainable(quiet: bool, cp: i32) -> bool` becomes `is_trainable(cp: i32) -> bool`, dropping
  the `if !quiet { return false; }` line and trimming the doc comment's tactically-live rationale
  (only the decisive/score-window rationale remains).
- `build_positions_cache`: swap `dataset::prepare_game_plies_statement`/`read_game_plies` for
  `dataset::prepare_game_scores_statement`/`read_game_scores`; the per-ply loop becomes
  `for (i, score) in scores.iter().enumerate()`, calling `is_trainable(*score)`.
- `build_labels`: same swap; `compute_labels` is called directly on the `Vec<i32>` `read_game_scores`
  returns (no more `.map(|ply| ply.score)`); the per-ply loop zips `scores.iter().zip(&labels)` and
  calls `is_trainable(*score)`.
- Update doc comments referencing `plies.quiet`.
- Tests: drop the `quiet` argument from every `is_trainable(...)` call; trim/rename
  `is_trainable_rejects_a_non_quiet_ply_but_accepts_a_decisive_one` to just the
  decisive-vs-ordinary cases, dropping its quiet-specific assertions.

### `claude/` documentation
- `training.md`: mark the "Quiet-position filtering" section and the "Generalizing `pv > 1`"
  subsection as reverted, with a short note pointing at this document's bare filename
  (`curate-drop-pv-quiet.md`). Leave the rest of the historical prose intact.
- `multipv.md`: add a note to the Status paragraph and to "Consumer: rotor-curate's
  `is_position_quiet`, corrected" that `rotor-curate`'s own consumption of MultiPV was reverted (see
  `curate-drop-pv-quiet.md`). The general search/UCI feature (steps 1-3) is unaffected and stays
  described as implemented.

### Regenerating data
No production `data/networks/` exists yet in this repo, so there is nothing to actually regenerate
as part of landing this. Purely a heads-up for whenever real data does exist: any `.db`/
`positions.cache`/label files built under the old `depth{D}.pv{P}` naming become orphaned (new runs
write to the new `depth{D}`-only paths) and can be deleted whenever convenient; not automated here.

## Verification plan

- `cargo build --release` and `cargo test` both pass.
- Spot-check `rotor-curate generate` and `rotor-train prepare` against a small fixture PGN.

## Progress log

2026-09-22: Every file in the plan above edited as described:
- `preprocessing_config.rs`: `pv` dropped from `Preprocessing`/`generation_suffix`/`database_path`/
  `positions_dir`/`labels_dir`; tests and doc comments updated.
- `examples/net_{naive,nnue,transformer}.yaml`: `pv:` line removed.
- `rotor_curate.rs`: `is_position_quiet`/`is_quiet_across_root_candidates` and their tests deleted;
  `PlyRecord.quiet` and the `plies.quiet` column removed; `play_game` reverted to a plain
  `choose_best_move` call (no MultiPV); `pv` parameter dropped end to end (`play_game`/`run_worker`/
  `run_generate_core`/`run_generate`); unused imports (`choose_best_move_multipv_with_progress`,
  `MultiPvLine`, `is_quiet`, `Move`) removed.
- `rotor_train/main.rs`: `run_prepare`/`stem_dirs` drop `pv`; the `stem_dirs` test renamed and
  updated (no more `pv: Some(1)` fixture field).
- `rotor_train/net_config.rs`: `pv: ~`/`pv, Some(2)` removed from fixtures and assertions.
- `rotor_train/dataset.rs`: `PlyRow`/`prepare_game_plies_statement`/`read_game_plies` deleted
  outright (redundant with `read_game_scores`/`prepare_game_scores_statement` once `quiet` is gone).
- `rotor_train/length_cache.rs`: `is_trainable` now `(cp: i32) -> bool`, no `quiet` parameter;
  `build_positions_cache`/`build_labels` switched to `read_game_scores`; the two dedicated
  "skips plies that are not quiet" tests deleted (the behavior they tested no longer exists); every
  remaining `is_trainable(...)` call site and SQL fixture (`quiet BOOLEAN NOT NULL` columns/values)
  updated.
- `claude/active/training.md` and `claude/active/multipv.md`: the sections describing the now-
  reverted design marked "Reverted (see curate-drop-pv-quiet.md)", historical prose left intact.

Verified by repo-wide grep that no remaining source file references `preprocessing.pv`,
`plies.quiet`, `is_position_quiet`, `is_quiet_across_root_candidates`, or `PlyRow`/
`read_game_plies`/`prepare_game_plies_statement` (the `pv`/`quiet` hits still present in
`src/search.rs`/`src/uci.rs` are the unrelated principal-variation field and the general UCI
MultiPV feature, both untouched).

**Not done in this session** (no Bash/build access): running `cargo build --release` and
`cargo test` to confirm the whole workspace still compiles and every test passes. The developer
should run these before considering this landed.
