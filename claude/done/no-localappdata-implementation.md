# No LOCALAPPDATA, the install a snapshot: implementation

**Status:** planned 2026-11-06, agreed (the developer), not started. Design: no-localappdata.md.

Claude edits by hand (the uses differ too much for one `sed`); the developer runs the `git mv`,
tests and commit. Every "install" found by a search on 2026-11-06 is below, or left on purpose.

## Left alone

`winget install`, `uv pip install`, torch "installed" (DEV.md, `bin/smithy`, `smithy/build.rs`'s
reinstall comment, `smithy/Cargo.toml:32`, `arena/Cargo.toml:33`), `install_ctrlc_handler`
(`ctrlc_handler.rs`, `lc0/mod.rs`), `examples/engines.yaml`'s Stockfish, `LICENSE`.

## Step 1: the move (the developer)

```
git mv crates/arena/src/install.rs crates/arena/src/snapshot.rs
```

## Step 2: the protocol (`pipeline/protocol.rs`)

- `INSTALL_PATH = "/install"` to `SNAPSHOT_PATH = "/snapshot"`; the header's `POST /install` line.
- `InstallRequest` to `SnapshotRequest`, its parse error words (`"install"` to `"snapshot"`).
- `Alive.install` to `Alive.snapshot` (`-`, `running`, or how the last one ended), wire key
  `snapshot`; `Alive.installed` to `Alive.snapshot_record`, its prefix `installed.` to `record.`.
- Tests: `an_install_request_reads_back` to `a_snapshot_request_reads_back`, the `Alive` test's
  values (`"done: took pesto"`).

## Step 3: the arena

- `snapshot.rs`: header rewritten (a snapshot, its folder, nothing outside the repository);
  `RELEASE_BASE_VARIABLE`, `UNDER_RELEASE_BASE`, `release_root`, `release_root_from` and their two
  tests deleted; the folder `format!("{}/chess/snapshot", data_root::data_root())`, as every other
  path under the root is found, created if missing; `Install` to `Snapshot`, `run` taking it into
  that folder (no `bin/` level), `installed()` to `record()`; `pesto_install_record` and
  `install_record` to `pesto_snapshot_record` and `snapshot_record`, the field `installed` to
  `taken`; log words `install:` to `snapshot:`; their tests renamed alike.
- `chores.rs`: imports, the `install` mutex to `snapshot`, `POST SNAPSHOT_PATH`, its messages and
  log lines.
- `main.rs`: `mod snapshot;`, the header's line ("taking a snapshot of the engine when its smithy
  asks").

## Step 4: the smithy

- `smithy.rs`: `INSTALL_CONTROL_PATH = "/control/install"` to `SNAPSHOT_CONTROL_PATH =
  "/control/snapshot"`; `ask_install`, `install_on`, `install_network`, `installable`,
  `INSTALL_JOIN_WAIT`, `Claims::installing` to `ask_snapshot`, `snapshot_on`, `snapshot_network`,
  `may_snapshot`, `SNAPSHOT_JOIN_WAIT`, `Claims::snapshotting`; every message ("only a
  generation's winner is snapshotted", "a snapshot runs there"); the machine view's fields
  `install`, `installed` to `snapshot`, `snapshot_record` (the page's JSON); the test
  `..._but_one_installing` to `..._but_one_snapshotting`; comments at 423, 1569, 1869, 2090, 2250.
- `main.rs`: header and usage ("a snapshot of a player"), the test
  `the_usage_offers_no_install_command` to `..._no_snapshot_command`.
- `page.html`: the screen `INSTALL` to `SNAPSHOT` ("Snapshot" in the menu), `installing()` to
  `snapshotting()`, `installScreen`/`drawInstall` to `snapshotScreen`/`drawSnapshot`, the button
  "Take a snapshot", `data-install` to `data-snapshot`, `send("control/snapshot", ...)`, the
  table's heading "Snapshots" and columns `machine, network, commit, taken, last snapshot`, every
  sentence ("it builds kalo-chess and takes the snapshot").

## Step 5: the engine and the comments

- `uci.rs`: `INSTALLED_EVAL_FILE` to `EVAL_FILE_BESIDE`, `installed_eval_file` to
  `default_eval_file` (and its caller in `main.rs:56`), the comments at 571, 1180, 1200, 1214, 1229;
  the test `the_installed_eval_file_sits_beside_the_executable` to
  `the_default_eval_file_sits_beside_the_executable`, its example path `C:/engines/kalo-chess.exe`.
- `pipeline/data_root.rs`'s header: the root holds the snapshot too; nothing reads the
  environment, and nothing in the repository uses `%LOCALAPPDATA%`.
- `pipeline/players.rs:1`, `smithy/src/book.rs:187`, `smithy/Cargo.toml:2`, `smithy/build.rs:8`:
  "install" to "snapshot".
- DEV.md 19, 151, 231-232, 301: "takes snapshots of players", "a snapshot", "Snapshot claims the
  arena it runs on", "never snapshotted".
- CLAUDE.md, Windows section: "The data root is the repository's gitignored `data/`; nothing in
  this repository reads or writes `%LOCALAPPDATA%`, nor anywhere outside the repository and Drive,
  and a release `.exe` writes nothing."

**Steps 1 to 5 done 2026-11-06.** Changes from the plan: `installed_eval_file` became
`startup_eval_file`, since `Engine` already has a `default_eval_file` field; the snapshot's folder
has no `margin.txt` clean-up (the retired file never existed in a new folder); a source comment's
reference to a `claude/` document removed in passing.

## Step 6: check (the developer)

Each must print nothing but CLAUDE.md's rule, which names the variable on purpose:

```
git grep -nIi 'localappdata\|release_root' -- ':!claude'
```

```
git grep -nIiw 'install\|installed\|installing\|installable' -- ':!claude' ':!LICENSE'
```

The second leaves only the uses listed under "Left alone". Then:

```
cargo test --workspace
```

```
cargo clippy --workspace --all-targets
```

```
git add -A
```

```
git commit -m "The install is a snapshot in data/chess/snapshot: nothing uses LOCALAPPDATA"
```

## Step 7: the switch

The protocol changes (`/snapshot`, the `Alive`'s keys), so every smithy and arena stops and
switches together, as for the token header. PeSTO's play is stopped from the page and resumed
after. On the Lichess machine, by hand: a snapshot from the page, lichess-bot's configuration
pointed at `data/chess/snapshot/kalo-chess.exe` and at the account `kalo-chess`
(kalo-chess-rename.md), then `%LOCALAPPDATA%\kalo-games` deleted on every machine that has one.

## Done when

Both searches clean, tests and clippy pass, a snapshot taken from the page shows in its table, and
lichess-bot plays from it. Then this document and no-localappdata.md go to `done/`.
