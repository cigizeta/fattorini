# Remote only: no `--offline`, no `--sync-only`, `install` from the remote

**Done:** `--offline` and `--sync-only` are removed; `rotor-train install` reads the remote pointer and that generation's champion. Files are `champion.*` now, not the `initial.*` below (champion-in-its-generation.md).

## Why

`/mnt/g` (Google Drive for desktop, `G:` mounted in WSL) is always mounted on every machine. Reads
and writes under it behave like a local folder: Drive syncs in the background, so the worst case is
a stale view of other machines' files, never a blocked one. Remote files are write-once, published
by renaming a `.partial`, and `CURRENT` is written only after its generation's files, so a stale
view is always an older but consistent state (an older pointer, fewer segments). Nothing needs a
local-only mode.

The three `--offline` flags only ever made sense together, as an all-local pipeline: `prepare
--offline` read the segments that `self --offline` left local, and `promote --offline` made a
champion only an offline generator could see. Their stated reasons in google-drive-sync.md were
"tests, or a machine without network": the tests do not need them (see step 1), and the machine
without network does not exist. Mixing modes on one network was the real hazard: an offline
promotion left the generators playing the old champion and `prepare` waiting for games that never
came, and a second one before an upload left a hole on the remote.

`--sync-only` existed to upload segments left local by an offline run or an outage; with neither
possible, the startup sync of `self` does the same job.

`install` currently reads the local `data/networks/<name>/gen-NNNN/`, so it only works on the
training machine. The remote already holds every promoted champion (`gen-NNNN/initial.{vs,nnue,txt}`)
and names the current one (`CURRENT`), so `install` reads from there, the same on every machine.

## Decisions

- No `--offline` on `rotor-arena self`, `rotor-train prepare`, `rotor-train promote`. Every
  `online: bool` parameter goes, keeping only its online branch.
- No `--sync-only` on `rotor-arena self`. The startup `publish_all_segments` stays.
- `remote::check_mounted` stays as a startup guard: it catches a WSL boot whose `[boot]` mount did
  not run, which would otherwise write silently into an empty `/mnt/g` on the Linux disk. Its
  callers' "Or pass --offline ..." hints go.
- `rotor-train upload` stays, for rerunning a promotion's failed upload only.
- `rotor-train install <name>` reads the remote only: `CURRENT`, then that generation's
  `initial.nnue` and `initial.txt`. It no longer reads `configs/<name>.yaml` or local `data/networks/`.
- Out of scope: the generator's warn-and-carry-on paths for remote errors (`follow_pointer`,
  `publish_segments`, `corpus_positions_so_far`'s listing). They are harmless, and whether they
  become fatal is a separate decision. Only their comments that mention offline are reworded.

## Steps

### 1. `rotor-arena self` (`src/arena/self_play_command.rs`, `src/arena/self_play.rs`)

- `self_play_command.rs`: drop the `offline` and `sync_only` variables, their match arms, the
  `--sync-only` block, `let online = !offline`, and the `if online` around `require_drive()` and
  the startup `publish_all_segments` (both now unconditional). `resolve` always calls
  `follow_pointer` for a network. `require_drive`'s message loses "Or pass --offline ...". Usage:
  the `[--offline]` suffixes, the `--sync-only` line and both flag descriptions go. Module doc:
  the `--offline` sentence goes.
- `self_play.rs`: `run` loses `online`; the post-batch `publish_segments` runs whenever `completed`.
  Its doc loses "Offline, only local ones." `publish_all_segments` loses its return value if nothing
  reads it any more (only `--sync-only` did).
- `corpus_positions_so_far(games_dir)` loses `online` and becomes a thin wrapper:
  `return count_positions(games_dir, &remote::remote_path(games_dir));`. The new
  `count_positions(local_dir, remote_dir)` does today's online logic over two plain directories:
  the union of both listings, each `.count` read locally when present, else from `remote_dir`. It
  needs a listing of an arbitrary directory, so `remote.rs` exposes one (make `list_files` public,
  or add a `pub fn list_dir`), and reads the remote companion with `fs::read_to_string` directly.
- Tests: the two call sites at lines 1291 and 1340-1341 call `count_positions(dir, missing_dir)`
  with a nonexistent remote directory, so they never touch `/mnt/g`. The test
  `corpus_positions_so_far_offline_...` is renamed (e.g. `count_positions_sums_finished_segments_of_every_machine`)
  and gains a case with a separate remote fixture directory: a segment only there, its `.count`
  only there, and a segment in both counted once.

### 2. `rotor-train prepare` (`src/train/prepare.rs`)

- `run` accepts `[name]` only; usage loses `[--offline]` and its description.
- `run_prepare` and `prepare_self_play_dataset` lose `online`. `games_dir` is always
  `remote::remote_path(&local_dir)` after `check_mounted`, whose message loses the hint; `counts`
  is always `wait_for_generation(...)`. The offline branch with its "drop --offline" error goes.
- Doc comments on `prepare_self_play_dataset` lose the offline sentences. Check whether `local_dir`
  and `segment_counts` still have other users; delete what becomes dead.

### 3. `rotor-train promote` and `upload` (`src/train/mod.rs`)

- Dispatch: `Some("promote") if rest.len() == 1`. Usage: `promote <network-name>`, "and uploads it
  to the remote".
- `run_promote` loses `online`: `check_mounted` unconditional (hint gone), `upload_or_exit`
  unconditional, the offline `println!` gone, doc reworded.
- `run_upload` doc: only for rerunning a failed upload.

### 4. `rotor-train install` (`src/train/mod.rs`)

`run_install(name)` takes the network name, not a config path:

1. `remote::check_mounted()`, exit on error.
2. `remote::cat(&remote::pointer_path(name))`, then `remote::parse_pointer`. A missing pointer
   exits with "`<name>` has no uploaded champion on the remote (`<path>` does not exist)"; a damaged
   one exits with the parse error.
3. `generation_dir = generation::generation_dir(&generation::network_dir(name), current)` (the
   local path), then the source is `remote::remote_path(&generation::initial_network_path(&generation_dir))`.
   Exit if it is not a file.
4. `initial.txt` via `remote::cat`, defaulting to empty as today.
5. `build_engine_or_exit()`, then `install_file_or_exit` for the engine and for the remote
   `initial.nnue` to `bin/rotor.nnue`. `install_file_or_exit` uses `fs::copy`, which copies
   permission bits onto the target; the target is on the Linux disk, so the `chmod` succeeds (the
   refusal `copy_bytes` works around is only for targets on the Drive).
6. `rotor.txt` as today, with `weights` naming the remote path.

The transformer check and the "generation 1 has promoted nothing" check go: only a promotion writes
a pointer, and the first one names generation 2. Dispatch passes `&rest[0]`. Usage: "installs it with
the remote's current champion". Check whether `current_generation(config_path)` and
`network_dir(config_path)` in `mod.rs` still have other users.

`rotor-train` still needs libtorch to run `install`. Moving `install` to `rotor-arena` would remove
that; not in this plan.

### 5. Docs

- README "Rotor offline flow" (line 304): the title becomes "Rotor training flow" (check anything
  linking to the old anchor); the `--offline` and `--sync-only` sentences at lines 350 and 355 go;
  line 366 loses "which also uploads a generation promoted with `--offline`". The install
  description, wherever it is, says it reads the remote.
- google-drive-sync.md (still in `active/`): a progress-log line saying `--offline` and
  `--sync-only` were removed, pointing to remote-only.md.

### 6. Verify

- `cargo build --release` and `cargo test`, run by the developer.
- `./bin/rotor-arena self --offline` and `--sync-only` are rejected as unknown flags.
- On this PC: `./bin/rotor-train install amanda` installs the remote's current champion, and
  `~/.local/share/rotor/bin/rotor.txt` names its generation and remote path.

## Progress

- Design agreed.
- Steps 1-5 written, not yet built or tested. Deviations from the plan above:
  - Step 1: `count_positions` lists both directories with the existing `local_segment_names`, so
    `remote.rs` needed no new listing function. `remote::list` now has no caller (kept; the module
    is `#[allow(dead_code)]` in both binaries). `publish_segments` and `publish_all_segments`
    return nothing, since only `--sync-only` read the result. New test
    `count_positions_takes_the_union_of_local_and_remote_segments`.
  - Step 4: `current_generation(config_path)` and `network_dir(config_path)` still have other
    users in `mod.rs`, so they stay. A missing or damaged pointer is one error, "`<name>` has no
    uploaded champion to install: ...".
  - Step 5: the README title "Rotor offline flow" is kept: "offline" there means training apart
    from play, not the flag. README line 291 now says "every Rotor command that uses Drive".
- Resume at step 6: the developer runs `cargo build --release` and `cargo test`, then
  `./bin/rotor-train install amanda` on the older PC.
