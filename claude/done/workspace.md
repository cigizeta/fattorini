# Workspace: rotor, arena, smithy

**Status:** done 2026-10-13, both commits checked on the Lichess machine. Still open: at the training machine's next training
run, a GPU subcommand must print `device = Cuda(0)` (see Checks).

Found while applying commit 2:

- `nnue_bench` `#[path]`-includes `pgn_replay` and the arena's `compression.rs`: both paths now
  reach into `crates/pgn-replay/` and `crates/arena/`, and `rotor` takes `pgn-reader` and `zstd`
  as dev-dependencies. A real dependency of `rotor` on `pgn-replay` would be a cycle.
- The tests that read a committed file (`configs/`, `examples/`, the end-to-end `zulu` run in
  `data/`) first call `rotor::pipeline::repository_root::enter_for_tests`, which moves the test
  process to the repository root once.
- The `runner` finds the venv by walking up from `$PWD` to the first directory holding `.rotor`.
- Active docs that name old `src/...` paths of finished work are left as they are.

## Goal

An intermediate step towards a multi-game repository (cigizeta-games): split today's single
`rotor` package into a Cargo workspace, and rename the two tools after what they will become.

- `rotor-arena` becomes `arena`.
- `rotor-train` becomes `smithy`, and its `train` subcommand becomes `forge`: `smithy forge
  <name>`.
- `turf` (go) is out of scope. So are a `games` trait crate, a separate `chess` rules crate and a
  shared `search` crate: each is extracted when its second user exists, not before.

## Where things stand

- One package, `rotor`, with a library (`src/lib.rs`) and three `[[bin]]` targets.
- The library holds the chess rules (`board`, `movegen`, `perft`), the engine (`search`, `eval`,
  `nnue`, `tt`, `uci`, `tablebase`), what is shared with training (`features`, `labels`), and
  `pipeline`, the file contract between the two tools.
- `rotor-arena` and `rotor-train` are module trees, `src/arena/` and `src/train/`, pulled into
  `src/bin/rotor_arena.rs` and `src/bin/rotor_train.rs` by `#[path]`. They are not in the library.
- `src/train/pgn_replay.rs` is `#[path]`-included into both tools. It uses `shakmaty`, which must
  never link into the engine, so it cannot move into the library.
- Both tools depend on the whole engine library, not just on the rules: the arena plays in-process
  players with `rotor::search`, and the trainer labels by searching. This is why a `chess` crate
  apart from `rotor` would be cosmetic today.
- The engine itself uses `rotor::pipeline::data_root` (`tablebase.rs`), so `pipeline` stays in the
  library for now.
- `tch` is optional, behind the `training` feature, required by `rotor-train` alone.

## Target layout

```
cigizeta-games/
  Cargo.toml              # [workspace] only: members, package defaults, lints, dependencies
  Cargo.lock              # stays here, one for the workspace
  .cargo/config.toml      # stays here
  crates/
    rotor/                # lib + bin `rotor`, benches/
    arena/                # bin `arena`
    smithy/               # bin `smithy`, build.rs, the only crate with tch
    pgn-replay/           # lib: SAN to Rotor move, used by arena and smithy
  bin/  configs/  data/  companion/  claude/   # unchanged, at the root
```

Dependencies: `arena -> rotor, pgn-replay`; `smithy -> rotor, pgn-replay, tch`;
`pgn-replay -> rotor, shakmaty`. `rotor` depends on no other crate of the workspace.

## Decisions

- **Two commits.** The rename first, in the current single package; then the split.
- **`pgn_replay` becomes its own crate, `pgn-replay`**, since `shakmaty` must stay out of `rotor`.
- **`tch` becomes a plain dependency of `smithy`.** The `training` feature, `optional = true` and
  `required-features` disappear. `move-stats` and `node-stats` stay features of `rotor`.
- **Names that say "rotor" but are not the engine stay as they are:** the data root
  `%LOCALAPPDATA%\rotor`, the Drive folder `G:/My Drive/rotor`, and the `.rotor` venv. Renaming the
  first two means migrating every machine's data; it is its own operation, if ever.
- **`target/` stays at the root**, so `target/release/rotor.exe`, which the arena spawns as a UCI
  engine, keeps its path.
- **Historical docs are not rewritten.** `claude/done/` and `claude/archived/` keep the old names.

## Commit 1: rename the binaries

No file moves except the two wrappers. The developer runs:

```bash
git mv bin/rotor-arena bin/arena
```

```bash
git mv bin/rotor-train bin/smithy
```

Then Claude edits:

- `Cargo.toml`: `[[bin]] name = "arena"` and `name = "smithy"`. The paths
  `src/bin/rotor_arena.rs` and `src/bin/rotor_train.rs` stay, since commit 2 removes them anyway.
  The comments naming the binaries.
- `build.rs`: `cargo:rustc-link-arg-bin=smithy=...`, and its comments.
- `bin/arena`, `bin/smithy`: the `--bin` names, the `.exe` paths, the messages. `bin/rotor` and
  `bin/lichess-book.sh`: their mentions.
- `src/train/mod.rs`: the subcommand `train` dispatched as `forge`, and its usage text; every
  mention of `rotor-train train` becomes `smithy forge`. The module and type names inside
  (`train_stream`, `round`, ...) stay: they name what the code does, not the command.
- `src/`: every user-facing string (usage, errors, progress) and every comment naming
  `rotor-arena` or `rotor-train`, about 300 occurrences. Temporary file prefixes in tests
  (`rotor-train-adamw-test-...`) become `smithy-...`.
- `configs/*.yaml`, `.gitattributes`: their mentions.
- `README.md`, `CLAUDE.md`, `claude/TODO.md`, and the docs in `claude/active/`.

Check, on the Lichess machine, with the venv active:

```bash
cargo test --release --features training
```

```bash
./bin/arena
```

```bash
./bin/smithy
```

Both should print their usage. Commit message:

```bash
git commit -am "rotor-arena renamed arena, rotor-train renamed smithy, its train subcommand forge"
```

## Commit 2: the workspace

### Moves (the developer)

The order matters: `pgn_replay.rs` and `build.rs` leave before their directories move.

```bash
mkdir -p crates/pgn-replay/src crates/arena crates/smithy crates/rotor
```

```bash
git mv src/train/pgn_replay.rs crates/pgn-replay/src/lib.rs
```

```bash
git mv src/arena crates/arena/src
```

```bash
git mv src/train crates/smithy/src
```

```bash
git mv crates/smithy/src/mod.rs crates/smithy/src/main.rs
```

```bash
git mv build.rs crates/smithy/build.rs
```

```bash
git rm src/bin/rotor_arena.rs src/bin/rotor_train.rs
```

```bash
git mv src crates/rotor/src
```

```bash
git mv benches crates/rotor/benches
```

Nothing builds between these moves and Claude's edits; nothing is committed in between.

### Edits (Claude)

- **Root `Cargo.toml`** becomes a workspace manifest: `members = ["crates/*"]`,
  `resolver = "3"`, `[workspace.package]` (edition, `rust-version`, license, version),
  `[workspace.lints.rust]` (the current `deny` set), and `[workspace.dependencies]` for those used
  by more than one crate. The long dependency comments move to the crate that uses each one.
- **`crates/rotor/Cargo.toml`**: the library, the `rotor` binary, the three benches, the features
  `move-stats` and `node-stats`, `[lints] workspace = true`. Only the engine's dependencies.
- **`crates/arena/Cargo.toml`**: binary `arena`; `rotor`, `pgn-replay`, `rusqlite`, `mimalloc` and
  the rest of what `src/arena/` uses.
- **`crates/smithy/Cargo.toml`**: binary `smithy`; `rotor`, `pgn-replay`, `tch` (not optional).
- **`crates/pgn-replay/Cargo.toml`**: library; `rotor`, `shakmaty`, `pgn-reader` if it uses it.
- **`crates/arena/src/main.rs`**: the `#[global_allocator]` from the deleted
  `src/bin/rotor_arena.rs`, the arena's `main` as the crate's `fn main`, `#[path]` attributes
  removed, `mod pgn_replay` removed in favour of the `pgn_replay` crate.
- **`crates/smithy/src/main.rs`**: the same, without the allocator.
- **`crates/pgn-replay/src/lib.rs`**: its header comment rewritten (no more `#[path]`).
- **`crates/smithy/build.rs`**: no more `CARGO_FEATURE_TRAINING` test, since the script now runs
  only for `smithy`. The venv is two levels above `CARGO_MANIFEST_DIR`.
- **`crates/rotor/benches/search_bench.rs`**: `target/release/rotor` is two levels above
  `CARGO_MANIFEST_DIR`.
- **Tests reading `configs/` or `data/`.** `cargo test` runs each crate's tests from that crate's
  directory, no longer the repository root. The tests that open real files
  (`preprocessing_config.rs` loading `configs/amanda.yaml` and siblings, `arena/config.rs`, and any
  other found by running the tests) resolve them from `CARGO_MANIFEST_DIR`'s grandparent. Tests
  that only compare path strings are unaffected.
- **`.cargo/config.toml`**: the `runner` puts `$PWD/.rotor/...` on `PATH`, and under `cargo test`
  `$PWD` is now the crate's directory. It must find the repository root another way; checked on
  the Lichess machine, whose CPU torch needs it just the same, before settling the form.
- **`bin/smithy`**: `cargo build --release --bin smithy`, no `--features training`. `bin/rotor`
  and `bin/arena` need no change: binary names are unique across the workspace.
- **Docs**: `README.md` (the introduction's crate list becomes true; the `training` feature,
  `required-features` and `build.rs` mentions), `CLAUDE.md` if it names a moved path. Active docs
  that name `src/...` paths or the `training` feature, notably `cpu-only-rotor-train.md`, which
  plans a change to `build.rs`.

### Checks

Everything on the Lichess machine, with the venv active (its torch is CPU-only, which `build.rs` handles).
`torch-sys` remembers where it found libtorch, so clear it once:

```bash
cargo clean --release -p torch-sys
```

```bash
cargo test --release --workspace
```

```bash
./bin/rotor
```

```bash
./bin/arena
```

```bash
./bin/smithy
```

Then one short run of each tool's everyday command, to see the relative paths still resolve from
the root: a `./bin/arena match` of a few games, and a `./bin/smithy` subcommand that reads
`configs/`. The lichess-bot install is unaffected (`smithy install` copies the same `rotor.exe`).

Commit message:

```bash
git commit -m "Workspace: crates rotor, arena, smithy and pgn-replay"
```

The training machine is needed for one check only, at its next training run: a GPU subcommand still prints
`device = Cuda(0)`, which `build.rs`'s `torch_cuda.dll` link argument decides and only a CUDA torch
exercises.

## Risks

- **Dead code.** `dead_code = "deny"` never reports a `pub` item of a library. That is already the
  case for `rotor`; `pgn-replay` joins it. Nothing new is hidden in `arena` or `smithy`, which stay
  binaries.
- **Build times.** Four crates compile in parallel where they can; the engine library is still one
  unit. No change expected; noticed if not.
- **Benches and features.** `cargo bench --features move-stats` from the root needs `-p rotor`;
  the docs giving bench commands are updated.

## Later, not in this plan

- `pipeline` out of the engine library, once `data_root` has a home the engine can share.
- `games` trait, `chess` rules crate, shared `search`: with `turf`.
