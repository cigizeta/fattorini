# Snapshots: implementation

**Status:** planned 2026-11-06 (rewritten the same day for the named, self-describing snapshots, the
suffixes and the margin's removal), not started. Design: snapshots.md. Releases are out of scope
(binary-release.md). Builds on no-localappdata-implementation.md's commit, not pushed: both go out
in one switch.

**Steps 1 to 6 written 2026-11-06, not yet built.** Changes from the plan: the snapshot request
carries the player, its network and its parameters' path (`-` untuned), the page a tuned/untuned
choice; `may_snapshot` kept (only a generation's winner, never a scaled run's); `bench_with` added
beside `bench_at_depth`, so `kalo-chess bench` searches with the embedded parameters while the
SPSA clients' bench is unchanged; a UCI player's binary is built at the start of an `arena`
command's match (`match_player::build_uci_player`), its parameters written beside it only when they
are not the defaults; the tests that named `EvalFile` only to show it refused were dropped, so the
step 7 search stays clean; `candidates::PARAMETERS_FILE`, left unused, removed.

## Step 1: the engine (`crates/kalo-chess`)

- **`build.rs`:** three variables, each embedded, empty when unset, each with `rerun-if-env-changed`
  (and `rerun-if-changed` for the files): `KALO_CHESS_EVALFILE` (exists) to `embedded.nnue`,
  `KALO_CHESS_PARAMETERS` to `embedded-parameters.txt`, `KALO_CHESS_DESCRIPTION` (a text, not a
  path) to `embedded-description.txt`.
- **`uci.rs`:**
  - `EMBEDDED_PARAMETERS` and `EMBEDDED_DESCRIPTION` beside `EMBEDDED_NETWORK`. The parameters
    parsed at startup with `SearchParameters::parse_file_text`; a malformed set exits, as a bad
    network does.
  - The engine starts with what it embeds: network (PeSTO when empty), parameters (`DEFAULT` when
    empty). `Engine::embedded()` replaces `with_default_eval_file`, `startup_eval_file`,
    `eval_file_beside`, `EVAL_FILE_BESIDE`, `EMBEDDED_EVAL_FILE`.
  - `id name Kalo Chess <version>` followed by the description when there is one; the same
    description in an `info string` at startup (stderr stays for errors).
  - `EvalFile` removed: `UciCommand::SetOptionEvalFile`, its parse arms, its `option` line, its
    handler, `default_eval_file`, `set_evaluator`'s path; the comments naming it. An unknown
    `setoption` is already ignored.
  - Tests: the `EvalFile` parse test, `a_default_eval_file_...`, `the_startup_eval_file_...` go;
    `a_search_with_an_eval_file_plays_a_legal_move` sets the evaluator directly through a test-only
    constructor; the advertised options lose `EvalFile`.
- **`main.rs`:** `kalo-chess about` prints the description (or "Kalo Chess <version>, PeSTO,
  untuned, no description: a development build"); `bench` searches with what is embedded.
- **`search.rs`, the margin player removed:** `SearchParameters::with_margin` and
  `choose_best_move_with_margin` deleted, `choose_best_move` calling
  `choose_best_move_with_progress` with `None`; the tests at about 7005 and 7027 deleted; the
  comments at 264, 270, 333 rewritten (`MARGIN_PER_PLY` is the default of the tuned margins, no
  word of `@<cp>` or `arena margin`).
- **`nnue.rs`:** the `margin.txt` test (about 5290) deleted, the comment at 1119 without it.

## Step 2: the build, in the arena (`snapshot.rs`)

- `pub(crate) fn build_embedded(network, parameters, description, out)`: `cargo build --release --bin
  kalo-chess --target-dir target/snapshot`, the three variables set, or removed from the child's
  environment when `None`; the `.exe` copied to `out`.
- The description: `<player>@<tuned|untuned>, commit <short> (modified when dirty), taken
  <YYYY-MM-DD HH:MM> UTC, network <first 12 hex of SHA-256> (or PeSTO), parameters <12 hex>` (or
  untuned). A hash crate only if `sha2` is not already in the tree; else FNV-1a 64 by hand.
- `run` (a snapshot): fetches the network and its `parameters.txt` as now, or pesto's
  (`players::pesto_parameters`' file); names it `kalo-chess-<player>-<tuned|untuned>-<YYYYMMDD-HHMM>.exe`
  in `data/chess/snapshots/`; refuses when the file exists; builds into it.
- `records()` becomes `snapshots()`: the `kalo-chess-*.exe` file names in the folder, newest first
  by the stamp at their end.
- Gone: `kalo-chess.txt`, `snapshot_record`, `pesto_snapshot_record`, `write_record`,
  `move_aside`, `copy_into_place`, `take_parameters`, `remove_if_present`, `CANDIDATE_FILE`, the
  single `kalo-chess.nnue`, `data/chess/snapshot/`.

## Step 3: the protocol and the page

- `Alive.snapshot_record` becomes `Alive.snapshots`, a list of file names, one line each
  (`snapshot.<n>\t<file>`), newest first, at most 20.
- `SnapshotRequest` unchanged (boot, network); the request also says tuned or untuned (the page
  offers both for a tuned player, untuned only otherwise).
- The smithy's `MachineView`, the page's table: per claimed machine, its snapshots' file names.

## Step 4: matches against external engines (`match_player.rs`)

- `kalo_chess_entry(net, false)` builds with `build_embedded` into `target/uci-players/<file>.exe`
  (the player's name with `@` and `/` replaced), its network and the parameters its suffix says,
  and returns a UCI entry with no options. Built once per match, at its start.
- `uci_suffix`: pesto `@tuned` over UCI when it has a tune, as a network.
- `kalo_chess_binary` and the empty-`EvalFile` comment go; the two tests asserting `EvalFile`
  assert the built path and no options; the header's `EvalFile` lines rewritten.

## Step 5: the suffixes, `@tuned` and `@untuned`

- `match_player.rs`: `Suffix::Default` to `Suffix::Untuned`; `t1` to `tuned`, `default` to
  `untuned` in the parser, its messages, the header, the tests; `@t2`'s refusal and its "one tune"
  message go (no number left to refuse); `Suffix::Margin` and its parse, messages and tests go
  (`main.rs:119`'s usage, `engines.rs:48`, `in_process.rs:30`); `in_process.rs:161` and
  `match_player.rs:555`, `:676` set one field of `DEFAULT`.
- `smithy/src/matches.rs`: `Side::text` and its parse, `tuned`/`untuned`; tests.
- `page.html`: the test form's choice values and labels (159, 877).
- `smithy/src/tune.rs:610`: the `pesto@150` case stays refused, as any unknown suffix.
- `runner.rs:445`: the closure `with_margin` renamed `label`.
- DEV.md 59, 316 and the arena's usage: `@tuned`, `@untuned`, no `@<cp>`.

## Step 6: docs

- DEV.md: snapshots (where, named, dated, deleted by hand; `kalo-chess about`; lichess-bot's
  `config.yml` names one).
- CLAUDE.md: `data/chess/snapshot/` becomes `data/chess/snapshots/`.

## Step 7: check (the developer)

Each prints nothing:

```
git grep -nI 'EvalFile\|eval_file_beside\|startup_eval_file\|with_margin\|margin\.txt\|arena margin\|@<cp>\|@t1\|@default' -- ':!claude'
```

```
git grep -nIE '"t1"|Suffix::(Default|Margin)' -- ':!claude'
```

Then:

```
cargo test --workspace
```

```
cargo clippy --workspace --all-targets
```

```
bin/kalo-chess bench
```

The bench's nodes as before (nothing embedded in `target/release`).

## Step 8: the switch

Every machine at once, PeSTO's play stopped and resumed (two protocol changes, one switch).
Optional (the developer asked, 2026-11-06): left as it is, the tests log's old lines still list, but
lose their live block on the page and a stopped one cannot resume; new tests are written with the
new suffixes and replace the old within 20. To keep them whole, on each smithy machine, the smithy
stopped:

```
sed -i -e 's/@t1\b/@tuned/g' -e 's/@default\b/@untuned/g' data/chess/tests/tests.tsv
```

Then a snapshot of pesto on the Lichess machine from the page; lichess-bot's `config.yml` pointed
at it and at the account `kalo-chess`; `data/chess/snapshot/` and `%LOCALAPPDATA%\kalo-games`
deleted by hand.

## Step 9: the command line only (snapshots.md, point 10)

**Written 2026-11-06, not yet built.** Changes from this step: the command is left out of `arena`'s
usage, being hidden, and written in DEV.md and `main.rs`'s header; the smithy's
`the_usage_offers_no_snapshot_command` deleted, since the smithy has no snapshot at all; the
registry is read (`Engines::load`) to resolve the player, `Engines::none` being test-only.

- `arena/src/main.rs`: `snapshot <player>@<tuned|untuned>` in the dispatch and the usage, calling
  `snapshot::take(name)`.
- `snapshot.rs`: `take(name)`: `match_player::resolve_player(name, false, &Engines::none())` for
  the network and parameters; `may_snapshot(player)`, moved from the smithy: pesto, or a network
  that `players::parse_network_name` reads as a winner (no candidate), whose generation
  `players::run_winner` finds passed (run home local, else Drive), and whose frozen `config.yaml`
  (run directory, local else Drive) has no `scale:`; then the name, the refusal of an existing
  file, `describe`, `build_embedded`. `Snapshot`, `Snapshot::of`, `run`, `fetch` and `snapshots()`
  go. The parameters of a tuned player are written to the file beside the target `.exe`, as
  `build_uci_player` does, then removed.
- `chores.rs`: the `/snapshot` arm and the `snapshot` mutex go; the `Alive` is built without them.
- `protocol.rs`: `SNAPSHOT_PATH`, `SnapshotRequest`, `UNTUNED`, `MAX_SNAPSHOTS_SAID`, the `Alive`'s
  `snapshot` and `snapshots` and their tests go; the header's `POST /snapshot` line.
- `smithy.rs`: `ask_snapshot`, `snapshot_on`, `snapshot_files`, `publish_replacing`,
  `may_snapshot`, `SNAPSHOT_CONTROL_PATH` and its two arms, `SNAPSHOT_JOIN_WAIT`,
  `Claims::snapshotting` (and its test's use; `release_idle` keeps every arena idle-released),
  `MachineView`'s two fields. `unscaled` stays (tests use it).
- `page.html`: the `SNAPSHOT` screen, its menu entry, `snapshotting`, `snapshotScreen`,
  `drawSnapshot` and its call, the button's handler.
- `smithy/src/main.rs`: the header and usage lines on snapshots, the test
  `the_usage_offers_no_snapshot_command` (still true, kept).
- DEV.md: Snapshots says `bin/arena snapshot`; `smithy` no longer asks for them (DEV.md 19, 151, 231).
- Check: `git grep -nI 'SnapshotRequest\|SNAPSHOT_PATH\|snapshotting\|ask_snapshot\|control/snapshot' -- ':!claude'`
  prints nothing; tests, clippy, the bench as before; `bin/arena snapshot pesto@untuned` writes a
  file whose `about` says so.

## Step 10: on Drive (the developer, 2026-11-06)

Written 2026-11-06. Snapshots go straight to Drive's `data/chess/snapshots/`, never locally:
`snapshot.rs` checks the drive is mounted, names the file under `remote::remote_path`, keeps the
temporary parameters file in `target/snapshot/`, and `build_embedded` copies through
`remote::copy_replacing` (`.partial`, then a rename), for the UCI players too. DEV.md, CLAUDE.md,
`data_root.rs` and the arena's header say so. Check: tests, clippy, `bin/arena snapshot
pesto@untuned` writes the file on Drive.

## Done when

The searches clean, tests and clippy pass, the bench unchanged, `about` says what a snapshot is,
and one plays on Lichess. Then snapshots.md, this plan and the two no-localappdata documents go to
`done/`.
