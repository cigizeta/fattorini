# The rename to temper-chess

**Status:** superseded, see temper-games-restart.md (2026-11-05). Steps 1 to 4 were done.
Designed and agreed 2026-11-04 (the developer).
`main` holds a squash commit and, merged on top of it, all of `fabio`'s commits.

## Why

The name Rotor was already taken: Jan Brouwer's UCI engine, in rating lists since 2007, eight
versions on CCRL. This project has no public release yet, so renaming is at its cheapest now.

The new name is **temper-chess**: the verb of the smithy, tempering steel. No engine named Temper on
chessprogramming.org or CCRL's 40/15 list (checked 2026-11-04), and none found on GitHub, where
plain `temper` is crowded by projects outside chess.

## Decisions

- The repository and the binary are both `temper-chess`; the display name is Temper.
- Everything renamed, the Drive root included.
- `fabio` is merged into `main` after the rename, not squashed: its history is kept (the developer,
  2026-11-04, replacing the earlier "its history does not matter").
- The Lichess account keeps its name: a Lichess name cannot be changed, and it need not match.
- `claude/` keeps its history as written: those records describe what happened under the old name.
  `TODO.md` gets a note at the top. CLAUDE.md is renamed like the code.
- Networks and configs keep their names.

## The names

| Old | New |
|---|---|
| crate `rotor`, `crates/rotor/` | crate `temper-chess`, `crates/temper-chess/` |
| `rotor::` in Rust | `temper_chess::` |
| binary `rotor.exe`, `bin/rotor` | `temper-chess.exe`, `bin/temper-chess` |
| `ROTOR_*` variables | `TEMPER_*` |
| venv `.rotor` | `.temper-chess` |
| `%LOCALAPPDATA%\rotor` | `%LOCALAPPDATA%\temper-chess` |
| `G:/My Drive/rotor` | `G:/My Drive/temper-chess` |
| installed `rotor.exe`, `.nnue`, `.bin`, `.txt` | `temper-chess.*` |
| `Local\rotor-lichess-group-<n>` | `Local\temper-chess-lichess-group-<n>` |
| events `rotor self-play`, `rotor arena` | `temper-chess self-play`, `temper-chess arena` |
| UCI `id name Rotor` | `id name Temper` |

Nothing parses the PGN events: they are only written, plus one test fixture. Games already played
keep their old event text.

Kept on purpose: the network file magic `ROTORNNU` (`nnue.rs`), or every existing network stops
loading; the Lichess account `rotor-bot`; the names of `claude/` documents and old tools cited in
comments (`rotor-evolve`, `rotor-transformer`, `rotor-network-gate`, `rotor-prepare`).

The text is changed by one reviewed `sed` the developer runs, not file by file, then checked by a
search for what remains.

## Constraints

- The Drive root and the arena's binary names are compiled in, so builds of the old and new name
  never run together: every smithy and arena stopped, every machine switched in one go.
- A venv stores absolute paths, so it is created anew, not renamed.
- lichess-bot's configuration is not visible to a search (no match in what is tracked), so the
  developer points it at the new installed names by hand.
- The Lc0 scatter was stopped for this. It did not resume (2026-11-04): `buckets.tsv` records each
  file by its full path, Drive root included, so the renamed root read as a changed training set
  and the committed buckets were truncated at the restart. It scattered from the start, 24 batches.
  The first real resume followed after a reboot (2026-11-04): 119 files kept, 1278 left, 22 batches.

## Steps

1. All machines: stop every smithy and arena. The rewriting machine shows nothing left to upload.
2. Claude, on `fabio`: code, wrappers and docs. No Bash, so file by file, the crate's directory and
   `bin/temper-chess` created by the developer's `git mv` first.
3. The developer: tests and clippy, then commit.
4. The developer: GitHub repository renamed, the remote URL updated, `fabio` merged into `main`.
5. On each machine: repository folder and data root renamed, the venv created anew, DEV.md's setup,
   then `cargo clean -p torch-sys -p smithy` and the same with `--release`: `target/` moves with the
   folder, and torch-sys's cached build output, one per profile, keeps the old venv's path, so the link fails on `torch_cuda.lib` (seen 2026-11-04).
6. The developer: the Drive folder renamed once, in the browser, then each machine seen to show it.
7. lichess-bot's configuration updated, and an install made so the new names exist.
8. The scatter restarted from the page.

## Open

A clear loss in the experiment would have to undo the corrections and keep the rename. With the
history kept, their commits can be reverted one by one; the merge itself is not reverted.
