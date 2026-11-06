# Snapshots and releases, one build

**Status:** design, to discuss (the developer, 2026-11-06). Supersedes the snapshot of
no-localappdata.md (its code committed the same day, not yet pushed): the folder, the name and the
rule against `%LOCALAPPDATA%` stay, the rest is replaced.

## Decisions (the developer, 2026-11-06)

- **Snapshots and releases are aligned:** both are one `.exe` with the network and its tuned
  parameters embedded. Nothing sits beside the `.exe`, nothing configures the network.
- **Snapshots are named by what they play, then dated:** `data/chess/snapshots/`, any number kept,
  deleted by hand at will. lichess-bot's `../lichess-bot/config.yml` names one, edited by hand. A
  date alone was rejected: "I will never know what a snapshot really does" (the developer).
- **The name:** `kalo-chess-<player>-<tuned|untuned>-<YYYYMMDD-HHMM>.exe`, UTC, as
  `kalo-chess-lc0-psq-threats-screlu-g1-r1-tuned-20261106-1530.exe`,
  `kalo-chess-pesto-untuned-20261106-1532.exe` (the developer, 2026-11-06). The same player twice
  in one minute: the second refused, a file never overwritten.
- **The `.exe` describes itself exactly,** the name being only a summary: player, tuned or untuned,
  the commit, the date, a short hash of the embedded network and of the embedded parameters. Shown
  by `kalo-chess about`, a startup `info string`, and UCI's `id name` (`Kalo Chess 0.1.0
  lc0-psq-threats-screlu-g1-r1@tuned (abc1234, 2026-11-06)`). No `.txt` beside it; the page reads
  the file names. A release carries the same description, with its version.
- **`@t1` becomes `@tuned`, `@default` becomes `@untuned` (the developer, 2026-11-06):** one tune a
  network, overwritten by the next, so a number promised what does not exist; the pair now says
  the one thing it means. `@<cp>`, a margin, stays. Everywhere a player's suffix is written or read: the
  arena's parser and messages (`match_player.rs`), the smithy's test sides (`matches.rs`), the page's
  choice, DEV.md, tests. The one stored name read back, the tests log `data/chess/tests/tests.tsv`
  (a stopped test resumes from its line), is rewritten once at the switch by a `sed`, both
  suffixes; no fallback in the parser. PGN labels already written keep the old suffixes: nothing
  parses them. `SearchParameters::DEFAULT` keeps its name: it is the untuned set.
- **A release** is the same build of the chosen winner, uploaded to GitHub under its version.

## Proposed

1. **The build:** `KALO_CHESS_EVALFILE` (exists) and a new `KALO_CHESS_PARAMETERS`, both read by
   `build.rs` and embedded with `include_bytes!`. Unset, nothing is embedded: PeSTO at the defaults.
   A snapshot of pesto embeds pesto's tuned parameters when it has some.
2. **Its own target directory** (`cargo build --release --target-dir target/snapshot`): an embedded
   build never lands in `target/release`, which the arena runs over UCI against external engines.
3. **The engine starts with what it embeds**, nothing else: the `kalo-chess.nnue` and `parameters.txt`
   beside the binary are no longer read (`EVAL_FILE_BESIDE`, `eval_file_beside`, the tuning beside
   it, gone).
4. **No record beside it:** the description is embedded (above, a third variable,
   `KALO_CHESS_DESCRIPTION`, set by the arena's build); the page lists every snapshot's file name
   on each claimed machine, newest first.
5. **Taken on the claimed machine, as now:** the page chooses a machine and a player; the smithy
   publishes the network and its records to the store; the arena builds and copies. No `move_aside`:
   a new snapshot never replaces a file.
6. **No `data/chess/snapshot/`:** renamed `snapshots/`; the one already written there, if any,
   deleted by hand.

7. **`EvalFile` removed (the developer, 2026-11-06: "only my eval files are understood anyway"):**
   the engine advertises and accepts no network option; it plays what it embeds, nothing can
   change it. Its tests in `uci.rs`, and `EMBEDDED_EVAL_FILE` (`<embedded>`), go with it.
8. **Arena matches against an external engine** (`match_player.rs`, the only user of `EvalFile`)
   run the Kalo Chess side as a snapshot build of that player: before the match, the arena builds it
   the same way into `target/uci-players/<player>.exe` (overwritten each match, so always of the
   current commit; incremental, only the engine crate recompiled) and runs it with no option but
   `Hash` and `Threads`. `kalo_chess_entry`'s empty `EvalFile` for pesto goes: the plain
   `target/release` build embeds nothing, so it is pesto.

9. **The margin player removed, with every trace of it (the developer, 2026-11-06):** it predates
   SPSA; only the deprecated `arena match|sprt|duel` still parsed it, the page, tunes and UCI
   refusing it. Gone:
   - `@<cp>`: `Suffix::Margin`, its parsing and messages (`match_player.rs`), the arena's usage
     (`main.rs:119`), `engines.rs:48`'s and `in_process.rs:30`'s comments, DEV.md (59, 316);
   - `SearchParameters::with_margin` and `choose_best_move_with_margin` (`search.rs`):
     `choose_best_move` calls `choose_best_move_with_progress` itself; their two tests deleted;
     the tests that borrowed `with_margin` for "some other parameters" (`in_process.rs:161`,
     `match_player.rs:555` and `:676`) set one field of `DEFAULT` instead;
   - `margin.txt`, the retired single margin: the `nnue.rs` test that it is ignored and the
     comments naming it (`nnue.rs:1119`, `candidates.rs:17`);
   - `arena margin`, a command already gone, from `search.rs`'s comments (264, 270, 333).
   - `runner.rs:445`'s local closure `with_margin`, which only labels a player: renamed `label`.

   Kept: `MARGIN_PER_PLY`, the default value of the tuned margins (`DEFAULT`), not a player.

## Open

- **The release's version and upload:** binary-release.md's plan, to be aligned with this.
- **Benches:** `search_bench` runs the engine over UCI; whether it ever sent `EvalFile` is checked
  in the plan.
