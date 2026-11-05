# Three binaries: rotor, rotor-train, rotor-arena

Status: plan, approved by the developer 2026-09-27. Supersedes the "merge `rotor-train` and
`rotor-evolve`" item in TODO.md, which merged along the wrong line.

## Goal

Split the binaries along the one boundary that matters, what they depend on:

| Binary | Does | Depends on |
|---|---|---|
| `rotor` | the UCI engine | nothing extra |
| `rotor-train` | `prepare`, `train`, `check`, `calibrate`, `correlate`, `promote`, `publish` | libtorch |
| `rotor-arena` | `self` (self-play), `match`, `gate`, tournaments, later `sprt` | SQLite, book, Syzygy; no libtorch |

`rotor-self`, `rotor-prepare` and `rotor-evolve` disappear. `rotor-curate` is tolerated dead code:
it keeps building, untouched but for the paths of the shared files it includes, and is removed
some other time.

Why the gate is in the arena and not in training: it plays games, and every piece of game-playing
code (the game loop, players, the book, PGN writing, the results database, the SPRT) is the
arena's. The gate and training already talk only through files (`challenger.nnue`,
`verdict.txt`), so no code needs to cross. The loop reads `rotor-train train`, `rotor-arena gate`,
`rotor-train promote`.

## The rule

**A binary's directory holds only its own code. `src/pipeline/` holds the pipeline contract: the
files that define something on disk or on Drive which both `rotor-train` and `rotor-arena` read
or write.** The two halves of the loop share no code at runtime and talk only through those files,
so this is what they agree on. Being used by two binaries is not enough to belong there; being part
of that contract is. `src/` itself holds the engine library (`lib.rs` and its modules), `main.rs`,
and `rotor-curate`'s two files.

No `#[path]` points into another binary's directory, with one tolerated exception: `rotor-curate`
includes `train/pgn_replay.rs`, and that goes when `rotor-curate` does.

## Layout

```
src/                 engine library, main.rs, curate.rs, polyglot_book.rs
src/pipeline/        generation, preprocessing_config, remote, self_play_config
src/train/           rotor-train: everything it had, plus prepare, builder, cache, dataset,
                     dataset_cache, length_cache, pgn_replay
src/arena/           rotor-arena: everything src/evolve/ had, plus self_play, self_play_command,
                     ctrlc_handler, machine
src/bin/             rotor_arena.rs, rotor_train.rs, rotor_curate.rs
bin/                 rotor, rotor-arena, rotor-train, rotor-curate, lichess-book.sh, syzygy.sh
```

What each pipeline file defines, and who reads or writes it:

- `generation`: the network tree, `data/networks/<name>/gen-NNNN/epoch-NNNN/`. Training writes the
  challenger and promotes; the arena's gate writes the verdict and self-play plays the champion.
- `preprocessing_config`: where configs and datasets live, `configs/<name>.yaml`,
  `data/self/<source>/`.
- `self_play_config`: `configs/self_play.yaml`, the corpus size self-play stops at and `prepare`
  waits for.
- `remote`: the Drive side of the same layout: segments, generations, the pointer.

## Code changes after the moves

### rotor-train

- `train/mod.rs` (was `main.rs`) declares every module it and `prepare` need, once:
  pipeline files by `#[path = "../pipeline/<file>.rs"]`, the rest by their own name. It gains
  `prepare`, `dataset_cache` and `self_play_config`.
- `prepare.rs` loses its own module declarations and imports its siblings through `super::`. Its
  `main` becomes `run(rest: &[String])`, and `train/mod.rs` dispatches `prepare` to it.
- The binary reads `argv` in one place: `train/mod.rs`'s `main`.

### rotor-arena

- `arena/main.rs` (was `evolve/main.rs`) declares everything once, including `self_play`,
  `self_play_command`, `ctrlc_handler`, `machine`, and the shared files. Its own copies of
  `generation` and `preprocessing_config` now come from `src/pipeline/`.
- `self_play_command.rs` (was `self_play/main.rs`) loses its module declarations, imports through
  `super::`, and its `main` becomes `run(args)` taking the arguments after `self`. Dispatched from
  `arena/main.rs` as `rotor-arena self ...`.
- The subcommands, `self`, `match`, `gate` and the tournament id, are unchanged in behaviour.

### Cargo, wrappers

- `Cargo.toml`: `[[bin]]` entries `rotor`, `rotor-curate`, `rotor-train` (with
  `required-features = ["training"]`), `rotor-arena`. The comments naming the old binaries are
  updated.
- `bin/rotor-arena`: the old `bin/rotor-evolve`, building and running `rotor-arena`, no venv.
- `bin/rotor-train`: unchanged in shape. `prepare` now needs the venv too, which costs nothing:
  it runs on the training machine, right before `train`.

### Text

Every message a user reads that names a command is updated: `rotor-self` becomes
`rotor-arena self`, `rotor-prepare` becomes `rotor-train prepare`, `rotor-evolve` becomes
`rotor-arena`. So are the usage texts, the module doc comments of the files that change, and
README.md's commands and pipeline diagram. Doc comments elsewhere that mention an old binary by
name are historical and left alone, fixed opportunistically when their file is next edited.
Temporary file names in tests (`rotor-train-builder-test-...`) are left alone: they only need to be
unique.

## Verification

```
cargo build --release --bin rotor-arena
cargo build --release --features training --bin rotor-train
cargo build --release --bin rotor-curate
cargo test --release --features training
```

Then one real run of each: `rotor-arena self --offline` for a batch, `rotor-train prepare` on an
existing dataset (idempotent, returns at once), `rotor-arena match` for one game.

## Follow-ups, not in this change

- The SPRT match (rotor-evolve-sprt-match.md) is built in `rotor-arena`, revised against TODO.md's
  decisions.
- Removing `rotor-curate`.
