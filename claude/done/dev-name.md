# `-dev` in the UCI name of every build but a release

**Status:** decided by the developer 2026-11-16 ("release as now, -dev for snapshots"), implemented
the same day; tests to run.

## Why

Since `id name` lost the build's description (2026-11-16), every build said `Fattorini 2026.11.0`:
the release, and every snapshot after it, though they play other code (the version only moves at a
release). PGNs and GUIs record the name alone, never the startup `info string`.

**Then the version moved on (the developer, 2026-11-16):** `2026.11.0` to `2026.11.1`, so a build
after a release says `2026.11.1-dev`, not `2026.11.0-dev`, which reads as a build from before it;
DEV.md's releases gain a step 7 doing so after every release.

## Decisions

1. A release (`arena release`) is named `Fattorini <version>`, as now.
2. Every other build is `Fattorini <version>-dev`: snapshots, a match's UCI players, and a plain
   development build (`bin/fattorini`), which is no release either.
3. Neither the commit nor a guessed next version in the name: a name changing with every commit
   splits a bot's records, and the next version (`2026.11.1` or `2026.12.0`) is not known ahead.
   The description, commit included, stays in the startup `info string` and `about`.
4. Marked explicitly: `FATTORINI_RELEASE`, set by `arena release` alone (`EmbeddedBuild::release`),
   read by `build.rs` and passed to the engine as `FATTORINI_RELEASE_BUILD`; not inferred from the
   notices a release also embeds.
