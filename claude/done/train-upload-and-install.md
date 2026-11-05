# rotor-train: `publish` becomes `upload`, and a new `install`

**Done:** `publish` became `upload`, and `rotor-train install` builds `rotor` into `<data root>/bin/` with the champion's network, read from Drive since remote-only.md, as `champion.*` (champion-in-its-generation.md); install also copies the opening book (opening-book.md).

## Step 1 -- rename `publish` to `upload` (applied)

`publish` copies the current generation's champion to the mounted remote and rewrites the remote
pointer. The word is freed for nothing: it is renamed `upload` so that `upload` (remote) and
`install` (local) say where the files go. No alias is kept for the old name.

- `src/train/mod.rs`: the dispatcher arm, `USAGE`, the module doc, `run_publish` -> `run_upload`,
  `publish_or_exit` -> `upload_or_exit`, `PUBLISHED_GENERATION_FILES` ->
  `UPLOADED_GENERATION_FILES`, every "rerun: rotor-train publish" hint, and the "published:"
  confirmation -> "uploaded:".
- `Cargo.toml`: the `rotor-train` comment.
- `README.md`: the promote paragraph.
- Untouched: `rotor-arena`'s `publish_segments`/`publish_all_segments` (a different operation),
  and `google-drive-sync.md` (a record of past work).

## Step 2 -- `install` (applied)

`rotor-train install <network-name>`: builds a release `rotor` and installs it, with the network's
most recent champion, into `<data root>/bin/` (`~/.local/share/rotor/bin/` by default), where
lichess-bot runs it. Local only: no remote, no prior `upload` required.

### Steps

1. Load `configs/<name>.yaml`. Refuse a transformer ("not trained in generations -- there is
   nothing to install") and a network still in generation 1 (no champion yet: its games are played
   by the hand-crafted evaluation), with the same wording `upload` uses.
2. The champion is `generation::initial_network_path(gen-NNNN)` of the current generation,
   `initial.nnue`: the weights `promote` wrote after their gate passed. Refuse if it is missing.
3. Build: spawn `cargo build --release --bin rotor` from the repository root (where every binary
   already runs). Remove `LD_PRELOAD` and `LD_LIBRARY_PATH` from the child's environment: the
   `bin/rotor-train` wrapper exports them for libtorch, and every `rustc` would otherwise preload
   `libtorch_cuda.so`. Only `--bin rotor`, never the running `rotor-train`. A failed build stops
   the install with nothing copied. `.cargo/config.toml`'s CPU flags apply as for any build, which
   is right since lichess-bot runs on this machine.
4. Install into `<data root>/bin/`, created if missing. Each file is written as
   `<file>.partial` and renamed into place, so a running bot keeps its old engine (no "text file
   busy") and never sees a half-written file:
   - `rotor`: `target/release/rotor`, mode 0755.
   - `rotor.nnue`: the champion.
   - `rotor.txt`: what was installed (below), written last, so it only ever describes files that
     are in place.
5. Print what was installed and the lichess-bot settings it needs.

### `rotor.txt`

Written by the existing `write_record`, one `name<TAB>value` line each like `initial.txt`:

```
network                 amanda
generation              7
weights                 data/networks/amanda/gen-0007/initial.nnue
champion_<field>        every field of gen-0007/initial.txt, prefixed, since its own
                        `generation` is the one the champion was trained in
commit                  <git rev-parse HEAD>, plus " (modified)" if `git status --porcelain`
                        is non-empty
installed               2026-09-27 14:03 UTC
```

The commit is read by spawning `git`; if git is unavailable the line says `unknown` rather than
failing the install.

### lichess-bot

Set once in its `config.yml`; later installs only replace the files. No `EvalFile` since step 3:

```yaml
engine:
  dir: "/home/<you>/.local/share/rotor/bin/"
  name: "rotor"
```

### Code

- `src/train/mod.rs`: `Some("install") if rest.len() == 1 => run_install(...)`, `USAGE` line,
  module doc.
- The data root: `data_root()` moves to `src/pipeline/data_root.rs`, included by `rotor-train`.
  `src/arena/data_root.rs` keeps the book and tablebase paths (it imports `rotor::tablebase`) and
  includes the pipeline file as a submodule, so `rotor-arena` and `rotor-curate` are unchanged.
- Tests: `install_record` and `describe_commit` (pure). The refusals exit the process and the
  build and copy touch the machine, so those are exercised by running it.
- `README.md`: a short "Playing on Lichess" section with the command and the config above.

## Step 3 -- the installed engine loads its network by default (applied)

SCID ran the installed `rotor` with the hand-crafted evaluation, because `EvalFile` defaulted to
empty and SCID sends none. Now `uci::run` starts the engine with `rotor.nnue` from beside its own
executable (`current_exe`, which resolves symlinks) when that file exists.

- `Engine::new` is unchanged (hand-crafted), so the tests stay independent of the filesystem;
  `Engine::with_default_eval_file` loads the default and `uci` advertises it as `EvalFile`'s
  default. `setoption name EvalFile` still overrides, `<empty>` still selects hand-crafted.
- A `rotor.nnue` that fails to load ends the process, like a bad `EvalFile` always has: the
  discussion proposed falling back to hand-crafted, but that is the silent wrong-engine case the
  existing `EvalFile` handling refuses, and a Lichess bot playing hand-crafted unnoticed is worse
  than one that does not start.
- A build in `target/` has no `rotor.nnue` beside it, so development, matches and gates are
  unaffected. The arena's `hand-crafted` player still sends `EvalFile <empty>` explicitly, so it
  never depends on that.
- Not chosen: embedding the network (a rebuild per champion), and the data root's
  `bin/rotor.nnue` as the default for every build (the development engine would silently pick up
  the installed network).
- `install`'s printout, `README.md` and the lichess-bot config drop `EvalFile`.
