# Binary releases: implementation

**Status:** planned 2026-11-06, not started; left at plan status (the developer, 2026-11-06:
"We implement and complete later"), the notes' text shown and accepted as a draft. Design: binary-release.md. Builds on snapshots.md's
code (`snapshot::describe`, `build_embedded`, the snapshot's rule).

**Confirmed by the developer (2026-11-06, binary-release.md decisions 10 to 12):** the notices
embedded behind `kalo-chess licenses`; a tag must be `v` followed by the workspace's version, so
`id name` and the tag never disagree; the notes are not written by the arena but committed,
`RELEASE-NOTES.md`, the strength table filled from two full matches before the tag.

## Step 1: the notices, in the engine (`crates/kalo-chess`)

- `build.rs`: a fourth variable, `KALO_CHESS_NOTICES` (a path), embedded as the others, empty when
  unset, `rerun-if-env-changed` and `rerun-if-changed`.
- `main.rs`: `kalo-chess licenses` prints them, or "no notices embedded: a development build" when
  empty. In the usage, beside `about`.
- Test: a development build embeds no notices.

## Step 2: the shared build (`snapshot.rs`)

- `describe(player, source, taken)`: `source` is the commit text (as now: 7 characters, `-dirty`)
  or a tag, written whole. Its test gains the tag case.
- `build_embedded` takes one struct, `EmbeddedBuild { network, parameters, description, notices,
  target_dir, static_runtime }`, the snapshot and `build_uci_player` giving `target/snapshot`, no
  notices, no static runtime. With `static_runtime`, `RUSTFLAGS` is `-C target-cpu=x86-64-v3 -C
  target-feature=+crt-static` (it replaces `.cargo/config.toml`'s flags, so it repeats the CPU).
- The copy to `out` unchanged (`remote::copy_replacing`).

## Step 3: `arena release` (`crates/arena/src/release.rs`)

`arena release <player>@<tuned|untuned> [--dry-run]`, hidden like `snapshot` (dispatch, the arena's
header, not the usage). In this order, each refusal saying why and exiting before any upload:

1. **The player:** `snapshot::may_snapshot`, then `match_player::resolve_player`, as a snapshot.
2. **The tag:** `git describe --exact-match --tags HEAD`; `protocol::current_commit()` not
   modified; the tag is `v{version}` or a release candidate `v{version}-rc.{n}` (the developer,
   2026-11-06: release candidates, a tag never moved nor reused once seen; `v{version}` tagged only
   on the candidate that worked), the version the engine's `id name` says
   (`env!("CARGO_PKG_VERSION")`, the workspace's, to check it is shared). `--dry-run` skips these
   three and names the build `v0.0.0-dry-run`.
3. **The tools:** `cargo about --version` and, unless `--dry-run`, `gh --version`; `gh release view
   <tag>` failing (no release of that tag yet).
4. **The notices:** `cargo about generate about.hbs -o target/release-build/notices.txt`.
5. **The build:** `build_embedded` into `target/release-build`, the static runtime, the notices,
   the description with the tag; copied to `target/release-build/kalo-chess-<tag>.exe`.
6. **The checks on the `.exe`:** its bytes hold no `VCRUNTIME140.dll` (import names are ASCII in
   the file: no `dumpbin`, no Visual Studio shell); `about` prints the description; `bench` exits 0.
7. **The notes:** `RELEASE-NOTES.md` at the repository's root, committed (the clean tree of step 2
   says so), read as it is; refused unless it names the version (`0.1.0` for `v0.1.0` and its
   candidates). Checked first, with the tag, so a stale file stops the release before the build.
   `--dry-run` checks only that the file exists.
8. **The draft,** unless `--dry-run`: `gh release create <tag> target/release-build/kalo-chess-<tag>.exe
   --draft --verify-tag --title "Kalo Chess <tag>" --notes-file RELEASE-NOTES.md`; prints `gh`'s URL.

Tests: the tag rule (`v0.1.0` and `v0.1.0-rc.2` against `0.1.0` yes; `0.1.0`, `v0.1.1`,
`v0.1.0-rc`, `v0.1.0-beta.1` no), the notes' version check (a text naming `0.1.0` passes, one
naming only `0.0.9` or `0.1.01` is refused), the `VCRUNTIME140.dll` search on two byte strings.

## Step 4: docs

- DEV.md: a Releases section: `cargo-about` and `gh` installed (`gh auth login` once), the steps
  (bump the version; the two full matches against Stockfish's Skill Levels, `bin/arena match
  stockfish-<level> <player>@tuned --time 10+0.1 --hash 64 --full`, the player built over UCI with
  both embedded as a snapshot is; `RELEASE-NOTES.md` filled; commit, tag, push the tag; `bin/arena release ...`; read the
  draft, publish), `--dry-run` first.
- binary-release.md: the decisions above confirmed.

## Step 5: check (the developer)

```
cargo test --workspace
```

```
cargo clippy --workspace --all-targets
```

```
bin/arena release pesto@untuned --dry-run
```

```
target/release-build/kalo-chess-v0.0.0-dry-run.exe licenses
```

The dry run prints its checks passed, and the notes file reads right.

## Done when

A dry run passes; then, once the network is chosen and tuned, a real draft made, downloaded back and
tried in a GUI, and published with the repository.
