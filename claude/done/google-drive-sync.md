# Google Drive as the source of truth, through a synced folder

**Done:** Drive, through Drive for desktop at `/mnt/g/My Drive/rotor`, is the shared store (`remote.rs`). Much detail below is historical: `rotor-self`, `rotor-prepare`, `rotor-evolve` are gone, champions are `champion.*` (champion-in-its-generation.md), `--offline` was removed (remote-only.md), and replay shares chunks through claims.

Status: designed with the developer 2026-09-26. Step 1 done (`src/machine.rs`, `src/remote.rs`,
included into `rotor-self` but not called yet); the developer ran `rotor-self`'s tests, 52 pass.
Step 2 done; 55 pass. Step 3 done; 62 pass. Step 4 done; 63 pass. Step 5 written (`promote [--offline]`, `publish`; the published files are
anchored as `/initial.*`, since an unanchored rclone filter matches at every depth); `rotor-train`'s
259 tests pass. Step 6 written (`src/prepare.rs`, `dataset_cache.rs`'s final manifest, and
`copy_down` made `--immutable`); `rotor-prepare`'s and `rotor-self`'s tests pass. Step 7 (README) done. Two-stage Ctrl+C done (tests pass,
`rotor-evolve` builds); the check before a batch added after, awaiting a build. rclone replaced by the synced folder (decision 1): `remote.rs` rewritten, callers
and README updated; every binary's tests pass (the check before a batch included). `G:` is mounted
on the training machine and `rotor/` exists in My Drive. `--hand-crafted` / `--network` split done; 63 pass. Remaining: step 8, the first run by hand.

Later: every `--offline` and `rotor-arena self --sync-only` were removed, since `/mnt/g` is always
mounted; see remote-only.md. What this document says about them no longer holds.

## Goal

Three PCs generate self-play games, and one GPU machine trains and promotes. Google Drive is the
source of truth and the backup for games and promoted networks, and it says which generation is
current. Paying for Google One later is acceptable.

## Decisions

1. **A synced folder, not an API.** Google Drive for desktop (Windows, **Stream** mode, My Drive on
   `G:`) syncs the Drive; WSL mounts `G:` at `/mnt/g` from a `[boot]` command in `/etc/wsl.conf`.
   Rotor only copies files into and out of `/mnt/g/My Drive/rotor` (`remote::ROOT`, a constant):
   no Drive API, no OAuth app, no child process, no new crate.

   Superseded, 2026-09-27: the first implementation drove `rclone`. Authorizing it needed either
   rclone's shared OAuth client, which the developer rejected, or the developer's own Google Cloud
   app, which Google would not publish without a registered domain, leaving it in Testing with
   refresh tokens that expire every 7 days. Mirror mode was rejected too, since it keeps the whole
   Drive on every PC. The `rclone` commands named below are now file copies in `remote.rs` with the
   same rules: write-once, `.partial` then rename, refuse a same-named file of another size. A
   file's arrival on other machines waits on Drive for desktop's sync, usually under a minute;
   nothing in the design depends on it being faster.
2. **Drive mirrors the local layout under one remote root.** `data/X` locally is `<remote>/data/X`
   on Drive, e.g. `/mnt/g/My Drive/rotor/data/self/amanda/gen-0001/`.
3. **Segment names do not change**: `{stem}-{YYYYMMDD-HHMM}.pgn.zst`, timestamp at compression,
   UTC. The directory already says who played the games; the stem says which machine; the timestamp
   is unique per stem and needs no state to survive a crash or a disk loss. No counter.
4. **Files on Drive are immutable.** A segment is written once, under a `.tmp` name, and renamed
   when complete (already the case). Uploads use `rclone copy --immutable`, which refuses to replace
   a file on Drive whose content differs, instead of silently overwriting it.
5. **The current generation lives on Drive** in a pointer file,
   `<remote>/data/networks/{name}/CURRENT`, holding one line such as `gen-0003`. Only the GPU
   machine writes it, from `rotor-train promote`, after uploading the generation it names.
6. **Only the GPU machine trains and promotes.** Generator PCs only read networks from Drive.
7. **Every segment has two files next to it**, uploaded and immutable like the segment:
   - `{segment}.pgn.zst.sha256`: the SHA-256 of the compressed bytes, in `sha256sum`'s own format
     (`<hex>  <segment file name>`), so `sha256sum -c` checks it with no Rotor code. Computed with
     the `sha2` crate, this change's one new dependency.
   - `{segment}.pgn.zst.count`: the number of trainable positions, one decimal number and a
     newline. A trainable position is one with an evaluation in the PGN, i.e. a move annotated with
     a score (`score_cp` or `mate_in`); a game's final position has none and is not counted. Counted
     from the PGN text (`trainable_positions`: one non-empty `{...}` comment per scored move, the
     only brace comments self-play writes), so one function serves the compressor and a batch
     resumed from a staging file an earlier process wrote, with nothing new persisted.

   `{stem}.count` (stem only, positions in the unfinished batch) stays private. The two are told
   apart by name: a stem-and-timestamp name ending in `.pgn.zst.count` belongs to a finished
   segment. `other_active_stem` must skip those, since today it strips `.count` from any name and
   would take `alpha-20260922-1702.pgn.zst` for a stem with an unfinished batch.
8. **The generator loops until Ctrl+C**, online and offline alike: sync, then per batch resolve
   the generator from the pointer, play, compress, sync. It is meant to run unattended. This
   reverses the earlier "one segment per process, an external manager restarts it" rule.
   **A full file throws no game away** (revised 2026-09-26, after the first run showed 20 games
   aborted at every 3-minute file): once it reaches its size, no new game starts, the games in
   flight finish and are appended to it, and only then is it compressed. A file is bigger than
   `positions / files` by at most one game per thread.

   **Ctrl+C has two stages.** The first starts no new game and lets every game in flight finish
   and be saved; the second aborts them, losing them. Every step is printed: how many games are in
   flight at the first press, each saved game with how many remain, how many the second press
   aborts, and where the batch stands at the end. If the batch fills meanwhile, it is compressed and
   uploaded as usual. Copies to the synced folder run in process, so Ctrl+C never interrupts one
   halfway; a `.partial` left by a crash is invisible to every listing and replaced by the next copy. The loop checks for Ctrl+C again right before
   a batch, so one pressed while the pointer is read or the segments counted starts no batch.
9. **A generation's games are a fixed number of files of a fixed number of positions**, set in
   one file shared by `hand-crafted` and every network, `configs/self_play.yaml`. It is not a
   network's configuration, so it is not in `net.yaml`:

   ```yaml
   # M, the trainable positions of one generation's games. Every network's training section sizes
   # its epoch against it, so change them together.
   positions: 17301504   # 33 * 1024 * 512
   files: 128
   ```

   Both required, unknown keys refused, `positions` a multiple of `files`. A file closes at the
   first game boundary at or past `positions / files` trainable positions, so every file is
   slightly bigger, and `files` files always reach `positions`. The same values hold for every
   source, `hand-crafted` and every generation of every network.
10. **The generation is complete when `files` segments exist**, counting files, not summing their
    `.count` (each file is known to have the right size). Checked when a file has been compressed
    and uploaded, and at startup, so a restart never opens one file too many. Online, the count is
    the union of the segments on Drive and the local ones in the same directory; offline, local
    only. When complete, the generator plays nothing and checks the pointer every 10 minutes,
    resuming when a promotion moves it. Up to two files too many, when three PCs close their
    batches together, are accepted.
11. **`rotor-prepare` downloads the generation's games and waits for all of them.** It is the one
    step on the GPU machine that reads games, so it fetches them itself rather than through a
    separate command that could be forgotten. It builds only once at least `files` segments exist,
    and always from exactly the first `files` of them, ordered by compression timestamp and then by
    machine name. Extras, from PCs that closed their batches together, stay on Drive and are never
    used. A built dataset is final: its manifest freezes which segments it holds, and a segment
    appearing later never triggers a rebuild. (A segment compressed early but uploaded late, after an
    outage, can sort into the first `files` after the dataset is built; the manifest's frozen choice
    wins.)
    The GPU machine keeps its own complete copy, since the dataset is built from local files; a
    later purge command deletes old generations' copies.
12. **The GPU machine may also generate games.** It runs `rotor-self` like any other machine; its
    own segments are already local, and the download skips them.
13. **Networks on Drive: the promoted ones.** Each generation's `initial.nnue`, `initial.vs` and
   `initial.txt`. Not the `lr-*` arms, checkpoints, challengers or gate files. Datasets are not
   uploaded: they are rebuilt from the games.

## Mechanism

### `rotor-self` usage

Revised 2026-09-27: who plays is named explicitly. `--hand-crafted` plays the lineage root's games
(they belong to no network) and exits once they are complete. `--network <name>` plays the network's
current champion only, and is refused while the network has no generation or its current
generation has no `initial.nnue`; online, the check runs after following the pointer. Exactly one of
the two is required. Every command is run through `./bin/`.

```
usage: rotor-self --hand-crafted --threads <count> [--offline]
       rotor-self --network <name> --threads <count> [--offline]
       rotor-self --sync-only
  --network    the network the games are for: its current generation decides who plays them
  --threads    games in flight at once
  --offline    touch no remote: generate locally only, upload nothing, ignore the pointer
  --sync-only  upload every local segment missing from the remote, then exit without playing
```

- **No `--stem`.** The machine's name is read from `data/machine.yaml`, a one-key file,
  `name: the Lichess machine`, written once by hand on each PC. `data/` is gitignored, so the file never travels
  with the repository. The name must match `[a-z0-9]+` (no `-`, which separates the stem from the
  timestamp in segment names). A missing or invalid file exits with a message saying how to create
  it. Typing the wrong stem on the command line is no longer possible.
- **Online by default, and the remote is not configurable.** `remote::ROOT = "/mnt/g/My
  Drive/rotor"`, a constant: every machine mounts `G:` at `/mnt/g`. The layout under
  it is decision 2's mirror, so nothing about paths is typed.
- **`--offline`** is the only switch: nothing touches rclone. For tests, or a machine without
  network.
- **`--sync-only`** only uploads: every local `data/self/**` segment (and its two files) missing on
  Drive. It reads neither the pointer nor `data/machine.yaml`, needs no `--network` or `--threads`,
  and plays nothing. Combined with `--offline` it is refused.

`rotor-train promote` and `publish` are online the same way, with `--offline` to skip the upload.

When online, if `/mnt/g/My Drive` is not there (`remote::check_mounted`), the program exits at startup with
a clear message: that is a setup error, not a network outage.

### `rotor-self`, at startup when online (before taking any other decision)

1. **Upload what is missing.** `rclone copy --immutable --include "*.pgn.zst" --include
   "*.pgn.zst.sha256" --include "*.pgn.zst.count" data/self/<source>/
   <remote>/data/self/<source>/` for every local games directory that holds segments. `rclone copy`
   skips files already on Drive, so this is cheap, and it is what retries every earlier failed
   upload, crash included.
2. **Read the pointer.** `rclone cat <remote>/data/networks/{name}/CURRENT`.
3. **If the pointer names a generation missing locally**, download
   `<remote>/data/networks/{name}/gen-NNNN/initial.*` into `generation.rs`'s staging directory
   (`.partial-gen-NNNN/`, invisible to `generations`), then rename it to `gen-NNNN/`. The existing rule (current generation = highest local `gen-NNNN`) then picks it up,
   so `resolve_generator` does not change.
4. **If Drive is unreachable** at 1 to 3, print a warning and carry on with the local state. The
   games are still labelled correctly, and the next run retries.
5. **If the pointer is behind the local state** (local has a higher `gen-NNNN`), trust local and
   warn. That only happens on the GPU machine between its two promote uploads, or by hand.

Since a run produces exactly one segment and exits, a promotion is picked up at the next segment
boundary, which is the granularity we want.

### Batches in positions

- `CHECKPOINT_GAMES` goes. The batch closes at `positions / files` trainable positions.
- The worker sends each game's `trainable_positions`, counted from its finished text, along with the
  text, and `{stem}.count` holds the positions in the unfinished batch (it was games). The final
  count is what `.count` beside the segment records.
- The `completed % CHECKPOINT_GAMES` boundary tests go too: a batch is unfinished exactly when
  `{stem}.pgn` exists. `resolve_batch_start` replaces `resolve_initial_pending_path` and recovers
  every crash window automatically instead of panicking with manual instructions:
  - no staging file: a fresh batch, removing a stale count;
  - a staging file below the batch size: resume it;
  - a staging file at or past it: if the stem's newest segment decompresses to exactly its bytes,
    remove it; otherwise `remove_orphans` (the earlier attempt's `.tmp` and companions with no
    segment) and compress it now.
- Each batch has its own stop flag for the workers, since the search aborts on one flag; the writer
  raises it when the file is full, and passes Ctrl+C on while polling.
- The progress bar counts positions.
- `run` loops: resolve the generator, skip and wait while the directory holds `files` segments,
  otherwise play a batch. Step 3 counts local segments only; step 4 adds Drive's.

### Writing a segment's files

- The `.count` beside the segment is the batch's final `{stem}.count`.
- `compress_and_clear` writes `.sha256` and `.count` first, each via `.tmp` and rename, and renames
  the `.pgn.zst` into place last. A visible segment always has both files.
- **No backfill.** The only segment without the two files, `alpha-20260922-1702.pgn.zst`, is
  broken and gets deleted by hand before the first online run, so it never reaches Drive.

Checking a segment by hand, from its directory:

```
sha256sum -c alpha-20260926-1405.pgn.zst.sha256
```

### `rotor-self`, after the segment is renamed into place

Run the same upload for this games directory. A failure prints a warning and leaves the segment
local; the next startup retries it. The segment is never deleted locally by this change.

### `rotor-prepare <name> [--offline]`

Online (the default), before building:

1. Resolve the current generation's source, as today.
2. **Wait for the generation.** List the segments on Drive for the source (`remote::list`). Below
   `files`, print how many are there and how many are missing, and check again every 10 minutes.
3. **Choose** the first `files` segments, by timestamp then machine, from the union of Drive's and
   the local ones. Skipped when the dataset is already built: its manifest's segments are used.
4. **Download** the chosen segments missing locally, with their two files, `rclone copy
   --immutable` with one `--include` per file name: it adds what is missing, never deletes a local
   file, and fails on a local file whose content differs from Drive's.
5. **Verify** every chosen segment against its `.sha256`, with the same `sha2` code that wrote it.
   A missing or mismatching `.sha256` stops `rotor-prepare`, naming the file.
6. Build the dataset from the chosen segments only. Other local segments of the source (the GPU
   machine's own extras) are ignored.

Unlike on a generator, an unreachable Drive is an error here, not a warning: a dataset built from a
partial local copy is the silent failure the design exists to avoid. The waiting in step 2 is the
one exception, since a failed listing is retried at the next check.

`--offline` skips steps 2 and 4, chooses among local segments only, and refuses to build with fewer
than `files`; step 5 still runs.

Today a dataset is rebuilt when more segments appear than its manifest lists. That rule goes: the
manifest is final, and a missing chosen segment is an error, not a reason to rebuild.

### `rotor-train promote`, when online

After `gen-(k+1)/` is renamed into place locally:

1. `rclone copy --immutable` of `gen-(k+1)/initial.nnue`, `initial.vs`, `initial.txt` to
   `<remote>/data/networks/{name}/gen-(k+1)/`.
2. Only then `rclone rcat` (or `copyto` from a local temp file) the pointer, `gen-(k+1)`. Network
   before pointer, so a generator never reads a pointer to a network not yet on Drive.

If either step fails, `promote` exits non-zero with the command to rerun by hand. The local
promotion stands. A rerun of the upload alone should exist: `rotor-train publish <name>`, which does
steps 1 and 2 for the current generation. It is idempotent.

### Where the code goes

- `src/machine.rs`: reads and validates `data/machine.yaml`.
- `src/remote.rs`: `ROOT`, and a thin wrapper around `rclone`: `copy_up(local_dir, remote_dir, include)`,
  `copy_down(remote_dir, local_dir)`, `cat(remote_file) -> Option<String>`,
  `write_small(remote_file, text)`. Each returns a `Result` with rclone's stderr, and never panics
  on a network error. Pure helpers for building the argument lists and for parsing the pointer
  (`"gen-0003\n"` -> 3, anything else refused), so they can be unit-tested without rclone.
- `src/self_play/main.rs`: the new usage (`--stem` gone, `--offline`, `--sync-only`), the machine
  name from `data/machine.yaml`, and the startup steps before `resolve_generator`.
- `src/self_play/self_play.rs`: the upload after `compress_and_clear`.
- `src/train/main.rs`: the upload in `promote`, `--offline`, and the `publish` subcommand.
- README "Rotor offline flow": `data/machine.yaml`, naming the rclone remote `gdrive`, the
  one-time `rclone config`, `--offline` and `--sync-only`.

## Steps

1. `src/machine.rs` and `src/remote.rs`, with the name validation, pointer parser and argument
   builders, and their tests.
2. Segment `.sha256` and `.count`: counting in the writer, the write order in `compress_and_clear`,
   the `other_active_stem` fix, and tests for each (the `.sha256` passes
   `sha256sum -c`'s format, the count matches the annotated moves, a segment's `.count` is not an
   active stem).
3. `configs/self_play.yaml` (loader, validation, tests), batches in positions, and the loop with
   the generation-complete wait. `configs/amanda.yaml`'s `rounds_per_epoch` comment points at it
   for M.
4. `rotor-self`: new usage, `data/machine.yaml`, startup sync, pointer, network download,
   post-segment upload, `--offline`, `--sync-only`.
5. `rotor-train promote` upload, `--offline`, and `publish`.
6. `rotor-prepare`: wait, choose the first `files`, download, verify, the final manifest,
   `--offline`.
7. README.
8. First run by hand on the GPU machine: `rotor-train publish amanda`, then check on Drive that
   `data/networks/amanda/CURRENT` and `gen-NNNN/initial.*` are there. Then, on the Lichess machine, delete the
   broken `data/self/hand-crafted/alpha-20260922-1702.pgn.zst`, write `data/machine.yaml`, start
   `rotor-self --network amanda --threads <n>`, and after its first file check that the segment
   and its two files are on Drive and that `sha256sum -c` passes on a downloaded copy.

## Open

- **`positions`**: 33 * 1024 * 512 or 33 * 1024 * 1024, depending on how long a generation takes.
- **The purge command**: deferred until it is worked on.
