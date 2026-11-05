# The first binary release

Status: plan (the developer, 2026-10-25); not started. Decisions 1 to 4 agreed in discussion; the
steps to approve.

**When (the developer, 2026-10-25):** once the network experiments settle (the SCReLU variants
under way: phase, count, taper), which decides the embedded network. Then `0.1.0` is debugged
through as many draft releases as it takes, tagged and released here while the repository is
private, and it becomes public with the whole repository.

**The order (the developer, 2026-10-27):** the width 512 experiment (psq-threats-screlu-512.md)
decides the best network, nothing wider tried; that network is then SPSA-tuned; only then is
`0.1.0` released, and the workspace's version bumped to `0.2.0` for development.

## Decisions (the developer, 2026-10-25)

1. **One `rotor.exe`, its official network embedded,** not a separate `.nnue` beside it. Already
   supported: `build.rs` embeds the file `ROTOR_EVALFILE` names, `uci.rs` starts with it and
   advertises `EvalFile` as `<embedded>`, and a `rotor.nnue` beside the `.exe` still wins over it.
2. **It runs on any machine, not only a development one:** the release build links the C runtime
   statically (`crt-static`), so it needs no Visual C++ Redistributable. Only the release build:
   development builds, `smithy install` and `bin/arena` keep the dynamic runtime, `smithy`'s
   libtorch needing it. Its own target directory, `target/dist`, so its flags never force a rebuild
   of `target/release`.
3. **Several networks may be published, each with its tuning,** not only the embedded one. A
   network's tuning is the `parameters.txt` in its own directory (`nnue.rs`), so each is published as
   a zip of its own folder: `<name>/<name>.nnue`, and `<name>/parameters.txt` when tuned. A user
   unzips it anywhere and points `EvalFile` at the `.nnue`. Networks can be added to a release later,
   without a new `.exe`.
4. **Debugged while private:** this repository is the new one meant to go public (TODO, "Going
   public"), created and still private; its release is made and debugged here as a draft, and the
   repository made public only once the release is right.

## What a release holds

- `rotor.exe`: x86-64-v3, the C runtime static, the official network and its tuning embedded.
- `NOTICES.md` (name to confirm): the dependency notices `cargo-about` generates, then the ODbL
  notice for the training data (TODO, "The embedded network's data").
- One zip per published network, the embedded one included: the network itself is part of the
  corresponding source AGPLv3 asks for, `data/` being gitignored.
- The release notes: the version, the embedded network by name, every published network with how it
  was trained and how it scored in its duels, the CPU requirement, the license, and where the source
  is (the tag).

## The one code change: the tuning embedded with the network

Today only the `.nnue` is embedded, so the `.exe` plays its network with the default search
parameters even when the network's folder holds a `parameters.txt`. `build.rs` also copies the
`parameters.txt` beside `ROTOR_EVALFILE` when it exists (an empty file otherwise, as for the
network), and `uci.rs` gives the embedded network that tuning, parsed as `nnue.rs` parses the file.
The `info string` at startup then says "search parameters from parameters.txt" for it as for a file.
Tests: an embedded tuning round-trips, and an absent one is untuned.

Optional, to decide: the embedded network's name recorded at build time (from `ROTOR_EVALFILE`'s
folder name), so the startup `info string` says which network `<embedded>` is.

## The build script, `bin/release`

One Bash script, run from Git Bash, the flags written once:

1. Refuses a dirty working tree, and takes the network's folder as its argument.
2. `ROTOR_EVALFILE=<the .nnue> RUSTFLAGS="-C target-cpu=x86-64-v3 -C target-feature=+crt-static"
   cargo build --release --bin rotor --target-dir target/dist`. `RUSTFLAGS` replaces the config's
   flags, so it repeats `target-cpu`.
3. Checks the `.exe`'s imports (`dumpbin /dependents`): no `VCRUNTIME140.dll`, nothing but Windows
   DLLs; stops otherwise.
4. Runs `target/dist/release/rotor.exe bench` once, so a broken build never leaves the script.
5. Generates the notices with `cargo-about` (`about.toml`, the accepted licenses, and `about.hbs`,
   the template, which carries the ODbL notice itself; both written and tried 2026-10-27, the engine's
   tree alone listed, Rotor itself left out by `publish = false`), and
   zips the network folders asked for.
6. Leaves everything in `target/dist/release-<version>/`, ready to upload by hand.

The upload stays by hand on GitHub's page (or `gh release create --draft` later): Claude has no Git
or network write access, and a release is outward-facing.

## Steps

1. The tuning embedded (above), with its tests.
2. `about.toml` and `bin/release`; `cargo-about` installed (DEV.md says how).
3. A dry run in this repository's `target/dist`, nothing uploaded: the imports checked, the bench
   run, the notices read through.
4. The tag `0.1.0` (the workspace's version, already in `id name`) and a release here, a draft
   first, with the assets; downloaded back and tried as a user would: in a
   GUI, `EvalFile` pointed at a published zip's network.
5. If possible, `rotor.exe` run once on a clean machine (a virtual machine or a tester's), the only
   real check of the static runtime beyond the imports.
6. **The history, done (the developer, 2026-10-27):** squashed once more, so the passages the
   2026-10-25 sweep reworded are gone from it. If commits since then ever need the same, squash
   again before going public, the tag and draft made again on it.
7. Public: the repository made public, the draft published.

## Open

- Which networks are published beside the embedded one.
- The notices file's name and format, once `cargo-about`'s output is seen.
