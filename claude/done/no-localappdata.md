# No LOCALAPPDATA, the install a snapshot

**Status:** design, to discuss (the developer, 2026-11-06). Nothing applied yet.

## Why

The developer, 2026-11-06: this repository **never** uses `%LOCALAPPDATA%`. A release is a plain
`.exe` (its network embedded, `KALO_CHESS_EVALFILE`) that writes nothing there, nor anywhere. Until
now `%LOCALAPPDATA%\kalo-games\bin` held the engine the arena installs (data-root-in-repository.md,
kalo-games-rename.md); that is the one use left.

And what the arena makes is no install: it is a **snapshot**, one engine build, one network and its
parameters, taken at one commit, which anything may then run (lichess-bot today). Not named for
Lichess, and not "freeze", which already names the frozen test sets (`smithy/src/freeze.rs`,
`data/lc0/frozen`) and a run's frozen config (the developer, 2026-11-06).

## What uses it today

- `crates/arena/src/install.rs`: `RELEASE_BASE_VARIABLE`, `release_root()`, the install's `bin/`
  and `installed()`'s `kalo-chess.txt`; two tests (`the_release_root_is_under_local_app_data`,
  `no_base_is_no_release_root`). The only code that reads the variable.
- Comments: `install.rs`'s header, `pipeline/data_root.rs`'s header.
- `uci.rs`'s test `the_installed_eval_file_sits_beside_the_executable`, an example path only.
- CLAUDE.md's Windows section.
- The word "install" itself: the protocol (`protocol::INSTALL_PATH`, `InstallRequest`), the arena's
  chores and `Alive`, the smithy's page (its Install button and installed engine's box), and DEV.md.
  Every one to be listed by a search before the plan.

The engine itself writes nothing: it only reads `kalo-chess.nnue` and `parameters.txt` beside its
own binary, when they are there (`uci::eval_file_beside`, `nnue::parameters_file_beside`). A release
`.exe` alone, with neither beside it, plays its embedded network at its defaults. Nothing to change.

## Proposed

1. **The snapshot lives in the data root:** `data/chess/snapshot/`, holding `kalo-chess.exe`,
   `kalo-chess.nnue`, `parameters.txt` and `kalo-chess.txt` as `bin/` does today. Gitignored with
   the rest of `data/`, never copied to Drive (nothing uploads it). One snapshot a machine, each new
   one replacing the files (`move_aside` unchanged, for a running engine).
2. **`release_root` and its variable deleted;** a constant `SNAPSHOT_DIR = "data/chess/snapshot"`,
   relative to the repository root every binary runs from, as `engine_build_path` already is. The
   two release-root tests go.
3. **Install renamed snapshot throughout:** `install.rs` to `snapshot.rs`, `Install` to `Snapshot`,
   the protocol's path and request, the page's button ("Snapshot") and box, DEV.md. The record
   `kalo-chess.txt` keeps its fields, its `installed` field becoming `taken`. A smithy and an arena
   then speak the new protocol together: every machine switches at once.
4. **"Release" means the `.exe` on GitHub alone,** never the snapshot.
5. **The rule written into CLAUDE.md:** nothing in this repository reads or writes
   `%LOCALAPPDATA%`, or anywhere outside the repository and Drive; a release writes nothing.
6. **On the Lichess machine, by hand:** lichess-bot's configuration points at
   `data/chess/snapshot/kalo-chess.exe` (absolute, with `/`), together with the `kalo-chess`
   account (kalo-chess-rename.md); then `%LOCALAPPDATA%\kalo-games` deleted, on every machine that
   has one.

## Risks

- `git clean -fdx` would delete the snapshot with the rest of `data/`; lichess-bot then fails to
  start the engine until the next snapshot. Accepted: `data/` holds far more valuable files already.
- lichess-bot runs the engine from inside the repository: a `git pull` never touches it (`data/` is
  ignored), and the arena's `cargo build` writes to `target/`, not to the snapshot.
