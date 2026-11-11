# CalVer

**Status:** implemented 2026-11-11, the month check included; tests to run. Replaces SemVer's
`0.1.0` before the first release (TODO.md, "Before the release or CCRL").

## Decisions (the developer, 2026-11-11)

1. **The format is `YYYY.M.N`:** the year, the month without a leading zero (Cargo refuses
   `2026.01.0`), and N counting that month's releases from 0. The first release is `2026.11.0`, a
   second one in November `2026.11.1`, December's first `2026.12.0`.
2. **The month is the release's,** not the commit's: the version is set when the release is
   prepared, in the month it is published.
3. **Tags:** `v2026.11.0`, and candidates `v2026.11.0-rc.1`, `-rc.2`, ... as before
   (binary-release.md, decisions 8 and 11). A candidate needs no version change of its own.
4. **`id name`:** `Fattorini 2026.11.0` followed by the description, as today
   (`Fattorini 2026.11.0 lc0-psq-threats-screlu-g1-r1@tuned ...`); a development build keeps
   `Fattorini <version>` alone.

## What changes

1. **`Cargo.toml`**, `[workspace.package]`: `version = "2026.11.0"`. `Cargo.lock` follows at the
   next build. Every crate already takes `version.workspace = true`.
2. **`crates/arena/src/release.rs`:** `check_tag` and `names_version` work on any version string
   and stay as they are. Their tests move to CalVer examples, keeping the same cases:
   - accepted against `2026.11.0`: `v2026.11.0`, `v2026.11.0-rc.1`, `v2026.11.0-rc.12`;
   - refused: `2026.11.0`, `v2026.11.1`, `v2026.11.0-rc`, `v2026.11.0-rc.`, `v2026.11.0-rc.x`,
     `v2026.11.0-beta.1`, `v2026.11.00`;
   - `names_version`: `2026.11.0` found in `# Fattorini 2026.11.0` and `` `v2026.11.0`. ``, not in
     `12026.11.0`, `2026.11.01` nor `2026.11.0.1`.
   The module comment (line 11) needs no change.
3. **`crates/arena/src/snapshot.rs:219`:** the test's `v0.1.0-rc.2` becomes `v2026.11.0-rc.2`.
4. **`RELEASE-NOTES.md`:** `0.1.0` becomes `2026.11.0` (title, `fattorini-v2026.11.0.exe`, the tag).
5. **`DEV.md`**, the release steps (around line 346): the version is set to the release's
   `YYYY.M.N` before each release, N from 0 within the month.
6. **`claude/` documents:** binary-release.md's decisions 8 and 11 note CalVer, pointing here;
   TODO.md's CalVer item is closed.

`uci.rs` and `main.rs` read `CARGO_PKG_VERSION` and need no change; their tests already use it.

7. **`arena release` checks the month** (the developer, 2026-11-11): `check_month` refuses a
   version whose `YYYY.M` is not the current UTC month, dry run included, so a release prepared in
   November but published in December cannot carry the old month. Tested with fixed dates.

## Verification

```bash
cargo test -p arena
```

```bash
cargo test -p fattorini
```

Then `bin/fattorini about` prints `Fattorini 2026.11.0: a development build, PeSTO, untuned`.
