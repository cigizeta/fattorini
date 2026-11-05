# Network configs move from `data/networks/` to `configs/`

## Status

Implemented. Not yet built/tested by the developer (no Bash access from this session).

## Decision

`net.yaml` is a hand-written, checked-in configuration, not generated data -- but it currently
resolves to `data/networks/{name}/net.yaml`, and `/data/` is entirely gitignored (`.gitignore`),
so it could never actually be committed. The developer's instruction: "configurations should be
source code" -- `net.yaml` belongs under a real, tracked source directory. `configs/` (a plain
repo-root directory, not covered by any `.gitignore` rule) already exists with one real file,
`configs/amanda.yaml`, dropped there ahead of this change, in exactly the shape this plan produces:
`configs/{name}.yaml`, flat, no per-network subdirectory.

`preprocessing_config::net_config_path(name)` is the single resolution point both `rotor-curate`
and every `rotor-train` subcommand (`prepare`/`train`/`check`/`calibrate`/`correlate`) already go
through for "a configuration name passed as an argument" -- so this is a one-function change at
the resolution layer, not a new mechanism.

## A real side effect this surfaces: `run_dir` must stop deriving from the config path

`rotor_train/main.rs::run_dir(config_path)` today just strips `net.yaml`'s own filename to get
"this network's own run directory" -- where checkpoints, `history.tsv`, and round-progress state
get written (`data/networks/{name}/checkpoints/...` today, since that's `net.yaml`'s own parent).

Once `net_config_path` returns a **flat** `configs/{name}.yaml` with no per-network subdirectory,
naively deriving `run_dir` the same way would make it resolve to plain `configs/` for *every*
network -- every network's checkpoints would collide in one shared, git-tracked directory. That's
wrong on two counts: checkpoints are generated data (belongs under gitignored `data/`, not tracked
`configs/`), and each network must keep its own separate run directory.

**Fix**: `run_dir` stops taking "the config path's own parent" and instead extracts the config
file's own stem (`configs/amanda.yaml` -> `amanda`) and rebuilds `data/networks/{name}` from it --
decoupling "where this network's config source lives" from "where this network's generated run
data lives," tied together only by the network's own name. No caller-side signature changes needed
anywhere else: `run_dir` still takes the same `config_path: &str` it always has, only its internal
derivation changes 13 call sites are unaffected.

`data/networks/` keeps existing as a purely generated-data directory (checkpoints/history/state),
now holding no `net.yaml` of its own at all.

## File-by-file plan

### `src/bin/preprocessing_config.rs`
- `net_config_path(name)`: `data/networks/{name}/net.yaml` -> `configs/{name}.yaml`.
- Update its own doc comment and the module-level doc comment's directory-role list (added in the
  last change) to mention `configs/` as the fourth role: hand-written network configs, tracked.
- Test `net_config_path_resolves_a_bare_network_name_to_its_own_net_yaml`: update the expected path.

### `src/bin/rotor_train/main.rs`
- `run_dir(config_path)`: derive the network name from `config_path`'s own file stem
  (`std::path::Path::file_stem`) instead of its parent directory, and return
  `format!("data/networks/{name}")`.
- Update `run_dir`'s own doc comment.
- Update the dispatch comment near the top of `main` (currently says configs resolve to
  `data/networks/<name>/net.yaml`).
- Tests `run_dir_strips_the_net_yaml_filename`/`run_dir_defaults_to_the_current_directory_when_
  there_is_no_parent`: rewritten against the new stem-based derivation and its own expected output
  (`data/networks/{name}`, not the input path's own parent).

### `src/bin/rotor_train/net_config.rs`
- Module doc comment's "One per network run directory (`data/networks/<name>/`)" line updated to
  describe the split: config source at `configs/<name>.yaml`, generated run data at
  `data/networks/<name>/`.

### Not in scope
- `src/bin/rotor_train/transformer_config.rs`'s own stale `data/teachers/<teacher>/teacher.yaml`
  references are pre-existing staleness in the old, unrelated "teacher" config remnants (not
  `net_config.rs`'s own `Preprocessing`) -- left untouched, per this project's own "fix a
  claude/-adjacent or stale reference opportunistically, only when already touching that exact
  code" convention.
- `configs/amanda.yaml` itself needs no change -- already in the exact shape this plan expects.
- `.gitignore` needs no change -- `configs/` was never covered by the `/data/` rule.

## Verification plan
- `cargo build --release` and `cargo test` both pass.
- Confirm `rotor-curate generate --threads 1 amanda` and `rotor-train prepare amanda` both resolve
  `configs/amanda.yaml` correctly (once real PGN data exists to run them against).

## Progress log

2026-09-22: Implemented as planned.
- `preprocessing_config.rs`: `net_config_path` now returns `configs/{name}.yaml`; module doc
  comment's directory-role list gained `configs/` as a fourth entry; test updated.
- `rotor_train/main.rs`: `run_dir` now derives the network name from `config_path`'s own file stem
  (via `Path::file_stem`) instead of its parent directory, and returns `data/networks/{name}` --
  no signature change, so none of its 13 call sites needed touching. Its doc comment, the dispatch
  comment near the top of `main`, and its own tests (renamed, rewritten against the new
  derivation) updated.
- `rotor_train/net_config.rs`: module doc comment updated to describe the config/run-data split.
- `transformer_config.rs`'s own pre-existing, unrelated old `data/teachers/` references left
  untouched, as planned (out of scope, not touched for this change).

Verified by repo-wide grep that no other source file references the old `data/networks/<name>/
net.yaml` path; the only remaining `data/networks` hits are `run_dir`'s own new, correct output and
the pre-existing, out-of-scope `transformer_config.rs` comments noted above.

**Not done in this session** (no Bash/build access): running `cargo build --release` and
`cargo test`.
