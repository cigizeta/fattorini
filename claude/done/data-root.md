# One data root for third-party assets: ~/.local/share/rotor

**Done:** third-party assets (the EPD book, the Syzygy tables, and lichess-bot's `bin/`) live in `$XDG_DATA_HOME/rotor`, else `~/.local/share/rotor` (`src/arena/data_root.rs`). The "awaiting first run" status below is stale. **Superseded in part (2026-10-04):** `XDG_DATA_HOME` is no longer read; the root is always `~/.local/share/rotor`, in `src/pipeline/data_root.rs` and both scripts.

Status: implemented 2026-09-27, awaiting the first real `self`/`gate` run to confirm the tables load. Prompted by preparing a Lichess deployment: the developer moved the
opening book and the Syzygy tables out of the repository into `~/.local/share/rotor/`, and wants
development and deployment to share that one copy, with no symbolic links.

## The rule

Third-party downloads live in one data root, shared by development and deployment. The project's
own data stays in the repository: `configs/`, `data/networks/`, `data/self/`, `data/matches/`,
`data/machine.yaml`.

The root is `$XDG_DATA_HOME/rotor`, and `$HOME/.local/share/rotor` when `XDG_DATA_HOME` is unset:
the standard XDG rule, with no Rotor-specific variable. Fixed rather than a flag, like
`data/machine.yaml`, so no command can name a different one by mistake. `HOME` unset is an error
naming both variables.

Its layout, as it stands on disk today:

```
~/.local/share/rotor/
  books/UHO_Lichess_4852_v1.epd
  syzygy/3-4-5-wdl/
  syzygy/6-wdl/
```

The Lichess deployment adds `bin/` and `networks/` beside these later; not part of this change.

## What changes

### 1. `src/arena/data_root.rs` (new)

The one place that knows the root and what is in it:

- `data_root() -> String`: the XDG rule above.
- `book_path() -> String`: `<root>/books/UHO_Lichess_4852_v1.epd`.
- `syzygy_directories() -> [String; 2]`: `<root>/syzygy/3-4-5-wdl`, `<root>/syzygy/6-wdl`.

Unit-tested with `XDG_DATA_HOME` set and unset. The functions take the environment's two values as
parameters under a thin public wrapper, so the tests do not mutate the process environment, which
other tests running in parallel would see.

### 2. The engine library loses its repository-relative defaults

`tablebase::DEFAULT_DIRECTORIES` and `SyzygyTablebase::with_default_directories` are a fact about a
checkout sitting inside the engine's library, and the engine itself never uses them: it takes its
tables from `SyzygyPath`. They go. The library gains one loader that says what it found:

```rust
/// Loads every table in `directories`, refusing a directory that is missing or holds no tables.
pub fn from_directories(directories: &[String]) -> Result<SyzygyTablebase, String>
```

`add_directory` stays for `SyzygyPath`, whose handling in `uci.rs` is unchanged: an engine told a
bad path by a GUI keeps playing, as today.

### 3. Missing tables stop being silent

`rotor-arena self` and `gate` load tables in-process today through `with_default_directories`, which
discards every error. With the files moved, both would now play without tablebases and say nothing:
self-play games labelled differently, a gate judged differently, invisibly. Through
`from_directories` a missing or empty directory ends the run, naming the path and
`bin/syzygy.sh`.

Callers: `pairing::load_tablebase_if_needed` (only when an in-process player takes part; a match of
UCI engines still loads none), `self_play.rs`, and `rotor-curate` (below).

### 4. The book

`book::DEFAULT_BOOK_PATH` goes; `RandomBook::open` is called with `data_root::book_path()` from its
three callers (`pairing.rs`, `gate.rs`, `self_play.rs`). Its missing-file message already names
`bin/lichess-book.sh`.

### 5. The download scripts

`bin/syzygy.sh` and `bin/lichess-book.sh` compute the same root in shell,
`${XDG_DATA_HOME:-$HOME/.local/share}/rotor`, and download there: `syzygy/3-4-5-wdl`,
`syzygy/6-wdl`, `books/`. The shell and Rust copies of the rule are one line each; a comment in
each names the other.

### 6. rotor-curate

Tolerated dead code, but it builds, and it loads tables through `with_default_directories`. It
includes `arena/data_root.rs` by `#[path]`, a second tolerated reach into another binary's
directory beside `train/pgn_replay.rs`, marked the same way, and calls `from_directories`.

### 7. Text

README.md's setup steps for the book and the tables; the doc comments of the touched functions;
`.gitignore`'s `books/` and `syzygy-*` entries can stay, harmless, or go. I would remove them, so a
stray download into the repository shows up in `git status` instead of being hidden.

## Not changing

- The engine binary `rotor`: it reads nothing but what `EvalFile` and `SyzygyPath` name.
- The project's own relative paths (`configs/`, `data/`): still relative to the repository, still
  "run from the repo root", as every `bin/` wrapper already `cd`s there.

## Tests

- `data_root` with `XDG_DATA_HOME` set, unset, and `HOME` unset.
- `from_directories`: a missing directory refused, and an empty one (a temporary directory with no
  `.rtbw` files) refused. No test loads real tables: they are tens of gigabytes that a test machine
  need not have, and the first real `self` or `gate` run after the change is the check that they load.
