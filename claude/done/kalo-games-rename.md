# The rename to kalo-games

**Status:** done (the developer, 2026-11-06). Its one decision to keep the engine's name,
superseded the same day by kalo-chess-rename.md.

The repository is `kalo-games` on GitHub, and its folder too. This finishes the rename everywhere the
old name still lives. It supersedes the names in temper-games-restart.md, which stays as written.

## Decisions

- **Project:** Kalo Games. **Engine:** Temper Chess, unchanged: the crate, binary, UCI name, `bin/`
  wrappers stay, and plain "Temper" stays a shorthand for the engine.
- **Firewall rules:** `kalo-games smithy` and `kalo-games arena`, replacing the `temper-chess` ones
  (the developer, 2026-11-06).
- **Venv:** `.kalo-games`.
- **Data root:** moved into the repository, see data-root-in-repository.md; `%LOCALAPPDATA%\kalo-games`
  is now the release's. **Drive root:** `G:/My Drive/kalo-games`.
- **No fallback to the old names:** a clean cut, one fixed place each, as before.
- **History stays:** `claude/done/`, `claude/archived/`, temper-games-restart.md, temper-chess-rename.md
  and dated TODO entries keep the name they had.

## Claude's edits

Every current use of the name: `.gitignore`, `.vscode/settings.json`, `.cargo/config.toml`,
`CLAUDE.md`, `README.md`, `DEV.md`, `bin/smithy`, `bin/syzygy.sh`, `bin/lichess-book.sh`,
`crates/smithy/build.rs`, the `data_root.rs` and `remote.rs` constants, the paths in tests and
comments, and the note at the top of `TODO.md`.

## The developer's steps

On every machine, with no build, training, self-play, replay or Lichess play running, and Drive
fully synced. Drive first, on one machine: renaming the folder in Drive carries over to the others.
Then, per machine: move `books/` and `syzygy/` into `data/chess/` (data-root-in-repository.md),
keep the installed engine under `%LOCALAPPDATA%\kalo-games\bin`, recreate the venv, update the firewall rules, fix any
gitignored local file and the lichess-bot config if they name an old path, then rebuild. Machines
other than this one also update their remote and rename their folder.

## Verification

`cargo test` passes, a grep for the old name finds only the history above, `bin/smithy` starts with
the new venv active, and `bin/arena` sees the moved data and the Drive store.
