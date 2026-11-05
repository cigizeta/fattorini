# The restart as temper-games

**Status:** decided 2026-11-05 (the developer). The old repository is `temper-chess-archive`, archived
and private. `temper-games` is created, public from the start, with one first commit. Steps 1 to 5
done on one machine; steps 6 to 8 next. Replaces the remaining steps of temper-chess-rename.md.

## Why

The project is to hold more than chess: Go is planned, not started. The repository becomes
`temper-games`, with one first commit and no old history. Networks start over too.

## Decisions

- **Repository and folder:** `temper-games`.
- **Binaries:** `temper-chess`, `smithy`, `arena`; `temper-go` is future work, a README mention only,
  with no crate and no wrapper.
- **The rule (the developer, 2026-11-05):** "Temper" alone is crowded, and whatever distinguishes
  Chess from Go, Checkers, Othello and so on is to be taken care of now.
- **Names:** Temper Games is the project, Temper Chess the engine; plain "Temper" stays as a shorthand
  inside the docs. UCI says `id name Temper Chess <version>`.
- **Crate layout:** unchanged. `crates/temper-chess` keeps its name, and `pipeline` stays in it under
  that name.
- **Venv:** `.temper-games`.
- **Data root:** `%LOCALAPPDATA%\temper-games`. **Drive root:** `G:/My Drive/temper-games`, a new
  folder. The `temper-chess` folder stays, so the archive's build keeps working on it.
- **A `chess/` level (agreed 2026-11-05):** `configs/chess/` and `data/chess/` in the repository, so
  `data/chess/` on the remote too, the rule `data/X` to `{ROOT}/data/X` unchanged. `machine.yaml` and
  `machines.yaml` stay in `data/`, a machine being the same for every game. In the data root,
  `chess/syzygy/` and `chess/books/`; `bin/` stays shared, its file names saying the game.
  `examples/` and `archived/configs/` stay where they are.
- **File tags, by a two-letter game code:** `ROTORNNU` becomes `TEMPERCH`; `RTFC`, `RTFL`, `RTTR`
  become `CHFC`, `CHFL`, `CHTR`. Go would be `TEMPERGO`, `GOFC` and so on. No fallback: old networks,
  frozen sets and round-progress files no longer load.
- **Variables:** `TEMPER_EVALFILE` becomes `TEMPER_CHESS_EVALFILE`; `TEMPER_SELF_PLAYER` and
  `TEMPER_SELF_SECONDS` become `TEMPER_CHESS_SELF_PLAYER` and `TEMPER_CHESS_SELF_SECONDS`.
- **Old networks** stay in the old Drive root and play from a build of the archive, in matches run
  by CuteChess or similar. Such a match measures search and network together.
- **README:** titled Temper Games; the Strength section removed, its estimate to be measured afresh;
  the `rotor-bot` line removed; Go mentioned as planned, not started.
- **Lichess:** the new account `temper-chess`, created and upgraded to a bot 2026-11-05, no game
  played. README links it.

## Drive

To `temper-games/data/chess/`, in the browser, `machines.yaml` to `temper-games/data/`:

| Moved | Copied | Left behind |
|---|---|---|
| `lc0/` (except `frozen/`), `pesto/`, `lumbra/` | `books/`, `machines.yaml` | `networks/`, `networks-old/`, `self/`, `lichess/` |

PeSTO's play stops before its folder moves. A move costs nothing; a copy through `G:` downloads and
uploads everything, and the browser cannot copy folders, so `books/` is copied through `G:`.

## Steps

1. Claude, in this tree, uncommitted: the code, wrappers and docs above; the tags and their tests.
2. Claude: a search for what remains, then a leak check of the whole tree against CLAUDE.md.
3. The developer: tests, clippy, the bench's nodes unchanged.
4. The developer: Drive as above, every smithy and arena stopped first.
5. The developer: an orphan branch and one commit, then an empty `temper-games` on GitHub and a push
   of that branch alone. `git log --oneline` shows one line.
6. Each machine: a fresh clone, the gitignored local files moved across, the venv created anew,
   DEV.md's setup. The training machine moves its local data root, if its data is kept.
7. lichess-bot's configuration updated by hand, the account created, an install made.
8. A new run on the training machine, freezing Lc0's test set again.

## Moved data

`bin/smithy lc0-rewrite` is finished: `curve.txt` moved with the corpus and says so, so a second run
stops at once, and its done-file was deleted at the end. `tars.tsv` holds no `data/` path. Buckets
are scattered anew, since `buckets.tsv` records full paths.

## Open

- **Atan support stays until the experiment's verdict (the developer, 2026-11-05):** the training
  corrections are not proved yet, so `ATAN_VERSION`, `refuse_atan_teacher` and the old curve's
  branches are left as they are. A clear loss brings the old training back by hand from the archive,
  since `temper-games` has no commits to revert; Lc0's corpus would then need a reverse rewrite or a
  new download.
- **The experiment's duel:** generation 1 against emily's is now a CuteChess match; the TODO to say so.
