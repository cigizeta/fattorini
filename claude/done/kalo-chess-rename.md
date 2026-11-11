# The rename to kalo-chess

**Status:** design, to discuss (the developer, 2026-11-06). Nothing applied yet.

## Why

One name for the whole project: Kalo Games, its engines Kalo Chess and, later, Kalo Go. Temper Chess
was named on 2026-11-04 (temper-chess-rename.md) and has no public release yet, so renaming is
still cheap. kalo-games-rename.md kept the engine's name; this supersedes that one decision.

## The names

| Old | New |
|---|---|
| crate `temper-chess`, `crates/temper-chess/` | crate `kalo-chess`, `crates/kalo-chess/` |
| `temper_chess::` in Rust | `kalo_chess::` |
| binary `temper-chess.exe`, `bin/temper-chess` | `kalo-chess.exe`, `bin/kalo-chess` |
| `TEMPER_CHESS_*` variables (`EVALFILE`, `SELF_PLAYER`, `SELF_SECONDS`) | `KALO_CHESS_*` |
| installed `temper-chess.*` under `%LOCALAPPDATA%\kalo-games\bin` | `kalo-chess.*` |
| `Local\temper-chess-lichess-group-<n>` | `Local\kalo-chess-lichess-group-<n>` |
| PGN events `temper-chess self-play`, `temper-chess arena` | `kalo-chess self-play`, `kalo-chess arena` |
| UCI `id name Temper Chess <version>` | `id name Kalo Chess <version>` |
| display name "Temper" (README, DEV, CLAUDE.md, comments) | "Kalo Chess" |
| CLAUDE.md's "Temper Go" | "Kalo Go" |

Unchanged: the repository, venv, data root, Drive root and firewall rules (all already
`kalo-games`); networks and configs; `claude/`'s history (TODO.md gets a note at the top).

## Proposed decisions (to confirm)

1. **No trace of Temper outside `claude/`'s history (the developer, 2026-11-06: "TEMPER must go
   away").** The network magic `TEMPERCH` (`nnue.rs`, `&[u8; 8]`) becomes `KALO` (`&[u8; 4]`, like the
   smithy's `CHFC`, `CHFL`, `CHTR`; every use goes through `MAGIC.len()`), with no fallback that
   reads the old one. Existing network files are converted once instead, each into a new copy
   (`{ printf KALO; tail -c +9 old.nnue; } > new.nnue`), in the same switch as the new build: an old build cannot read
   a converted file, nor the new build an unconverted one. **The only network today (the developer,
   2026-11-06): `lc0-psq-threats-screlu`'s**, wherever a copy of it lives: Drive, the training
   machine's `data/networks/`, and any installed copy (installed again, not converted). emily's
   networks stay in the old Drive root, read by archive builds only, so they are left alone.
   The conversion's loop lists every `.nnue` it finds first, to confirm there is no other.
   **Listed 2026-11-06:** on Drive, two files, both `TEMPERCH`, both of run 1:
   `gen-1/lambda-polyline/candidate.nnue` and `gen-2/teacher.nnue` (generation 2's copy of its
   teacher). None in this machine's `data/`; the training machine's still to list.
2. **The display name is "Kalo Chess"** everywhere, never a bare "Kalo" (which names the project).
3. **The Lichess account:** `temper-chess` cannot be renamed. It has played no game, so this is the
   moment. **Done (the developer, 2026-11-06): the account `kalo-chess` exists.** Still to do: make
   it a bot, point lichess-bot's configuration at it, and close `temper-chess`.
4. **The name checked (2026-11-06, web searches):** no chess engine named Kalo or Kalo Chess was
   found, on chessprogramming.org, CCRL's site or GitHub. The nearest names, Kaola (a small UCI
   engine on GitHub) and Kallisto (an old DOS engine), are distinct enough. Not checked against
   CCRL's full engine list by hand.
5. **One `sed` the developer runs** for the text (`temper-chess` to `kalo-chess`, `temper_chess` to
   `kalo_chess`, `TEMPER_CHESS_` to `KALO_CHESS_`, `Temper Chess` to `Kalo Chess`; the magic by
   hand), after a `git mv` of the crate directory and the wrapper; then a case-insensitive
   search for `temper` outside `claude/`, which must find nothing. Claude cannot run it (no Bash), and 649 matches in 106 files are
   too many to edit one by one.

## Constraints (from the last rename)

- The arena's binary names are compiled in: every smithy and arena stopped, every machine switched
  in one go.
- `cargo clean -p temper-chess` (and `--release`) on each machine, so no stale binary is installed.
- lichess-bot's configuration is not tracked: the developer points it at `kalo-chess.exe` by hand,
  and an install creates the new names.
- PGNs already played keep their old event text; nothing parses it.

## Steps

1. Agree the decisions above, then an implementation plan beside this document.
2. Stop the PeSTO self-play cleanly (resumable) or wait for `complete.txt`.
3. The developer: `git mv`, the `sed`, tests and clippy, commit.
4. Claude: the remaining hand edits (CLAUDE.md, README, DEV, TODO note, what the search found).
5. Every machine: pull, `cargo clean`, rebuild; an install; lichess-bot's configuration.
