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

Every machine at once, PeSTO's play stopped and resumed (two protocol changes, one switch). On
each smithy machine, the tests log rewritten once, with the smithy stopped:

```
sed -i -e 's/@t1\b/@tuned/g' -e 's/@default\b/@untuned/g' data/chess/tests/tests.tsv
```

Then a snapshot of pesto on the Lichess machine from the page; lichess-bot's `config.yml` pointed
at it and at the account `kalo-chess`; `data/chess/snapshot/` and `%LOCALAPPDATA%\kalo-games`
deleted by hand.

## Done when

The searches clean, tests and clippy pass, the bench unchanged, `about` says what a snapshot is,
and one plays on Lichess. Then snapshots.md, this plan and the two no-localappdata documents go to
`done/`.
