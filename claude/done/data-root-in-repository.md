# The data root in the repository

**Status:** approved 2026-11-06, edits applied; the developer's steps and the verification remain.

`%LOCALAPPDATA%` belongs to a release only (the developer, 2026-11-06). Today `data_root()` serves
development too, so the arena, the smithy, the tests and the benches all exit when the variable is
unset, and development data sits outside the repository. A release ships no tablebases and no book.

## Decisions

- **Development:** the book and the tablebases live in the repository's gitignored `data/`:
  `data/chess/books/` and `data/chess/syzygy/`. Nothing in development reads `LOCALAPPDATA`.
- **Release:** the engine `install` sets up stays in `%LOCALAPPDATA%\kalo-games\bin`, the only use
  left. An install creates no folder for lichess-bot's PGN: that is lichess-bot's own setup.
- **No books in an install:** the Install screen offers a machine and a player, nothing else.
- **The Books screen stays, unchanged, on Drive:** it reads its sources from and writes its books
  to `G:/My Drive/kalo-games/data/chess/books/`, and an install no longer takes one (the developer,
  2026-11-06). The tablebases and the arena's opening positions are local, in `data/`.
- **An existing `temper-chess.bin`:** left where it is, no longer written, removed or reported.
- **Finding the repository:** `data_root()` walks up from the current directory to the first one
  holding `Cargo.lock`, as `.cargo/config.toml` already walks up for the venv. The wrappers `cd` to
  the root, while `cargo test` runs in the crate's directory, so a plain relative `data` would make
  the tests that need real tables skip silently. No path is compiled in: `CARGO_MANIFEST_DIR` would
  put a build machine's path into the binary. No `Cargo.lock` above is an error and exits.
- **No fallback to the old place:** a clean cut, as in kalo-games-rename.md.

## Edits

- `crates/temper-chess/src/pipeline/data_root.rs`: `data_root()` as above, with a pure function
  taking the starting directory and the test for the repository, so its tests need no process state.
  Drop `BASE_VARIABLE`.
- `crates/arena/src/install.rs`: a new `release_root()` is the one reader of `LOCALAPPDATA`, used by
  `run` and `installed`, its test moved from `data_root.rs`. Remove `games_dir`, `BookChoice`,
  `Install.book`, `install_book`, `kept_book_fields`, `with_book_fields` and their tests.
- `crates/temper-chess/src/pipeline/protocol.rs` and its callers: drop `book` from `InstallRequest`.
- `crates/smithy/src/page.html`: remove the Book row and its value from the request, and the book
  column of the Installed table.
- `crates/smithy/src/main.rs`: the help text no longer offers a book in an install.
- `crates/arena/src/data_root.rs`: its header comment.
- `bin/syzygy.sh`, `bin/lichess-book.sh`: `DATA_ROOT` from `REPO_ROOT`, comments updated.
- `DEV.md`: the setup section, the Defender exclusion, "the same tree as the local data root".
- `CLAUDE.md`: the data root sentence.
- `kalo-games-rename.md`: its data root decision, pointing here.

## The developer's steps

On every machine, with nothing running and enough free space on the repository's drive: move
`books/` and `syzygy/` from `%LOCALAPPDATA%\kalo-games\chess\` to `data/chess/`, keep `bin/` and
`lichess/` where they are, and update any local config that names the moved paths.

## Verification

`cargo test` passes and the real-table tests run rather than skip, a grep for `LOCALAPPDATA` in
`crates/` and `bin/` finds only `release_root()`, `bin/arena start` works with the variable unset,
and an install from the page places the engine under `%LOCALAPPDATA%\kalo-games\bin`.
