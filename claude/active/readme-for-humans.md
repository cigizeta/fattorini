# A README for humans, and the first release's version

**Status:** README rewritten 2026-10-13 (steps 1 and 2). Step 3 decided 2026-10-14: the release is
named by the workspace's version, `0.1.0` (see Versions); `id name Rotor 0.1.0` done 2026-10-14,
its `uci` test passing.

**To revise at the first real release (the developer, 2026-10-13):** the release is one file, the
network embedded in `rotor.exe` (no `rotor.nnue` beside it), and its network is the best
architecture of the experiments, not necessarily amanda's shape. The README already says both;
Strength, Inside and the `EvalFile` row get the released network's figures then.

## Why

Before Rotor is offered to CCRL, its README has to tell a reader, a tester above all, what Rotor is,
how strong it is, how to run it and why it is original, in a couple of minutes. Today's README is a
developer manual: about 500 lines of toolchain, Drive, venv and pipeline before any of that.

## Versions

**Decided 2026-10-14 (the developer), replacing the month names below:** the engine is the only
product, the go and turf engines being out of the picture, and its network is embedded, so there
are no network releases. A release is named by the workspace's version, `[workspace.package]
version`, `0.1.0` first: `id name Rotor 0.1.0`, from `env!("CARGO_PKG_VERSION")` with nothing to
edit by hand, and the release file `rotor-0.1.0.exe`. A new release raises the version in
`Cargo.toml`.

Superseded (2026-10-13), kept for the history:

- **cigizeta-games**, the workspace, is `0.1.0`: `[workspace.package] version` already says so, and
  every crate inherits it.
- **A Windows build of an engine** (rotor today, turf someday) is named by its release month,
  `YYYY-MM`: the first is `Rotor 2026-10` or later. It is what `id name` reports and what the
  release's file names carry (`rotor-2026-10.exe`, `rotor-2026-10.nnue`).

Open: where the release month comes from.

- **Recommended:** one constant in `rotor` (`RELEASE: &str = "2026-10"`), edited by hand when a
  release is cut, and `id name` is `Rotor <RELEASE>`. Visible in the source, one commit per release,
  nothing to configure at build time.
- The alternative, an environment variable read at compile time (`option_env!`), keeps the source
  unchanged but lets two builds of one commit name themselves differently.

Open: a second release in the same month. Proposed: `YYYY-MM.N` from the second on (`2026-10.2`),
the first keeping the bare month.

## One file, rewritten (the developer, 2026-10-13)

**Reversed the same day (dev-md.md):** `README.md` is for users of the release, `DEV.md` for
developers; section 8 of the outline below now lives in `DEV.md`.

No second file: `README.md` stays the only one, rewritten throughout. The reader first, the
developer at the end. Today's content is condensed, not moved: what a developer must do to set up a
machine and run a generation stays, as commands with a line each; explanations of why, workarounds
already handled by the scripts and the history of decisions go. A detail dropped from the README
that only Claude needs belongs in `CLAUDE.md` or a `claude/` doc, never in the README.

## The README's outline

1. **Rotor.** One paragraph: a UCI chess engine in Rust whose evaluation networks are trained on
   one PC with one consumer GPU, starting from a hand-crafted evaluation, never from another
   engine's evaluations. One sentence on cigizeta-games (the workspace holding the engine, the arena
   and the trainer).
2. **Strength.** About 2795 on Stockfish 19's CCRL Blitz scale (amanda-0002, 2026-10-13), from the
   skill-level ladder, in a short table (levels 7, 9, 10 only), with its caveats in one sentence.
   The Lichess bot account by link, not by number: the number changes daily.
3. **Running Rotor.** Download the release, keep `rotor.nnue` beside `rotor.exe`, load it in any UCI
   GUI. Requirements: Windows, x86-64 with BMI2, AVX2 and FMA (Intel Haswell or later, AMD Zen 3 or
   later; Zen 1 and Zen 2 run it, slowly, since their PEXT is microcoded). No fallback build.
4. **UCI options.** The six advertised: `Hash` (MB, 1 to 1024, default 16), `Threads` (1 to 128,
   default 1), `SyzygyPath`, `MultiPV` (1 to 500, default 1), `EvalFile` (default: the `rotor.nnue`
   beside the binary; empty for the hand-crafted evaluation), `Ponder`.
5. **How it learns.** The generation chain: the hand-crafted PeSTO evaluation labels generation 1,
   each promoted champion labels the next. Positions from replayed TCEC and CCRL games and from
   self-play; every label from Rotor's own search, with AB(lambda). No Stockfish or Leela data, no
   other engine's network. A new network replaces the champion only by beating it. No formulas.
6. **Inside.** A few lines each: the search (alpha-beta with the usual pruning and reductions,
   Lazy SMP, pondering), the network (`(780 -> 256) x 2 -> 1`, an incrementally updated NNUE, int8),
   Syzygy tablebases through `SyzygyPath`.
7. **Building from source.** Rust via rustup and the MSVC build tools, then `./bin/rotor`.
8. **Development.** At the end, for whoever works on Rotor:
   - **The cycle**, first, in plain words: the champion plays itself and replays archive games, its
     searches label the positions, `smithy` trains one candidate per lambda, `arena` picks the best
     and promotes it only if it beats the champion. The four lineages compared (with or without a
     hidden layer, with or without threats). Strength is decided only by games. The flowchart.
   - **Setting up a machine**: Git Bash, Defender exclusions and power mode, the `.rotor` venv with
     its pinned torch, `data/machine.yaml`, `data/engines.yaml`, the book and tablebase scripts,
     Google Drive in stream mode. Commands, a line each.
   - **Running a generation**: `arena self`, `smithy prepare`, `forge`, `arena select`, `promote`,
     `install`, one line each.
   - **Matches**: `match`, `sprt`, `duel`, the player names, `--depth`/`--time`, briefly. The
     Stockfish skill-level table and the measured ratings move up to Strength (section 2) or stay
     here in short.
9. **License.** AGPLv3, as today.

Written for people, not for Claude: short sentences, no history of decisions, no explanation of
what the scripts already handle. Every figure is checked against the source or the duel records
when written, not recalled.

## What left the README (2026-10-13)

The README before the rewrite is `README.md` at commit `aad834a`, in full. The details below left it
and are still needed when working on Rotor:

- **torch pinned to 2.11.0:** `tch` 0.24.0 checks the LibTorch version strictly; an unpinned install
  pulls a newer wheel that does not link. Its own README's `2.13.0` was stale.
- **A stale `torch-sys` link path** after the venv or the repository moves (`unable to find library
  -ltorch_cpu`, or LNK1181 on the old path): `cargo clean --release -p torch-sys`.
- **CUDA silently dropped by the linker:** `crates/smithy/build.rs` forces a `torch_cuda.dll` symbol
  in; every GPU subcommand prints `device = Cuda(0)` first, to be checked.
- **`.cargo/config.toml`** carries `LIBTORCH_USE_PYTORCH` in `[env]` and puts torch's DLLs on `PATH`
  through the runner, since `[env]` does not reach the binaries cargo runs.
- **NumPy** in the venv, else torch warns `Failed to initialize NumPy`.
- **Git Bash's prompt:** `VIRTUAL_ENV_DISABLE_PROMPT=1` and a `PS1` showing the venv on the path line.
- **Drive's cache cap:** `HKLM:\Software\Google\DriveFS`, `ContentCacheMaxKbytes` (KB).
- **The arena's run folders:** `data/matches/<champion>-vs-<challenger>/<YYYYMMDD-HHMM>/`,
  `spec.txt`, `result.txt`, resume rules, the hash guard (all tables under half the physical RAM),
  equal footing (UCI, same `Hash`, `Threads` 1) under a clock or against an external engine.
- **The duel rule:** a player past 0.5 + 3.09 * 0.5 / sqrt(N) after a pair wins; drawn at 1000.
- **The corpus:** `configs/corpus.yaml` (`file`, `tcec`, `ccrl`, `self`), chunk layout under
  `data/games/`, `bin/ccrl-download.sh`, `bin/tcec-months.sh`.
- **`arena self`'s Drive behaviour:** moves segments to Drive with `.sha256` and `.count`, picks up a
  promotion between segments, one Ctrl+C finishes the games in flight, two abort.
- **Stockfish 19's skill formula:** level = ((37.2473 e - 40.8525) e + 22.2943) e - 0.311438,
  e = (Elo - 1320) / 1870 (`src/search.h`, `struct Skill`); the full level table.
- **Perft:** the six standard positions, run through `go perft <depth>`.
- **`smithy install`:** renames a running `rotor.exe` aside to `rotor.exe.old-<seconds>`.

## Steps

1. Before rewriting, every detail about to leave the README that Claude still needs goes to
   `CLAUDE.md` or a `claude/` doc.
2. The new `README.md` from the outline.
3. `id name Rotor <CARGO_PKG_VERSION>` (Versions, 2026-10-14), with the `uci` test that checks
   the `id` lines updated. Run `cargo test --release -p rotor uci`.
4. Links: anything pointing at a README section that moved (`smithy install`'s hints, other docs).
