# Move finished segments to the remote instead of copying them

Goal: save space in `data/`. Once a finished segment is safely on the remote, its local
`.pgn.zst` and `.pgn.zst.sha256` are deleted. The remote keeps everything.

Builds on corpus-positions.md.

## Decisions

- **The `.pgn.zst.count` companion stays local.** A few bytes; it keeps
  `corpus_positions_so_far` reading this machine's own counts from disk rather than from Drive.
  Still uploaded, as today.
- **Delete only what is confirmed on the remote.** After `copy_up` succeeds, each local
  `.pgn.zst` / `.pgn.zst.sha256` whose remote counterpart exists with the same size is removed.
  A failed upload deletes nothing; the next publish retries, as today.
- **Every publish moves**: after each batch, at startup (`publish_all_segments`) and with
  `--sync-only`. Offline, nothing moves; the next online start uploads and removes.
- The segments already in `data/self/hand-crafted` are handled manually by the developer.

## The problem: crash recovery needs the segment locally

Today `compress_and_clear` renames the segment into place and then removes the staging file
`{stem}.pgn`. A crash in between leaves both, and `resolve_batch_start` tells this case apart by
decompressing the stem's newest local segment and comparing it with the staging file. Once
segments are moved away, that comparison finds nothing and the batch would be compressed and
published twice.

Also, `remove_orphans` treats every `.sha256` / `.count` whose segment is not local as an orphan,
so one recovery would delete all the kept `.count` files.

## The fix: seal the staging file under the segment's name

The developer's idea: rename the staging file before compressing it. Complemented with the
segment's timestamp in the new name, so recovery never needs the segment itself.

With `F = {stem}-{timestamp}.pgn` (e.g. `beta-20260926-1603.pgn`), whose segment is `F.zst`,
`compress_and_clear` becomes:

1. **Seal**: rename `{stem}.pgn` to `F`. The batch is closed and its timestamp fixed.
2. Write `F.zst.tmp` (synced), then `F.zst.sha256` and `F.zst.count` (each via its own `.tmp`).
3. Rename `F.zst.tmp` to `F.zst`.
4. Remove `F`, then `{stem}.count`.

`F.zst.tmp` exists from step 2 until step 3, and only step 3 removes it. So a sealed file found
at startup is resolved from local files alone:

| On disk besides `F`                 | Crash was        | Recovery                                      |
|-------------------------------------|------------------|-----------------------------------------------|
| `F.zst.tmp` exists                  | in 2, or before 3| remove the `.tmp` and any companions; redo 2-4 |
| no `.tmp`, no companions            | between 1 and 2  | redo 2-4                                      |
| no `.tmp`, companions exist         | between 3 and 4  | redo 4 only                                   |

The positions for `F.zst.count` come from `{stem}.count`, which is removed last, so it is there
whenever steps 2-4 are redone. A companion's own `.tmp` (e.g. `F.zst.count.tmp`) can only exist
during step 2, after `F.zst.tmp` was created, so it always falls in the first row and is removed
with the rest.

`resolve_batch_start`, in order:

1. A sealed file of this stem exists: recover it as in the table, then return `None` (the caller
   starts the next batch). At most one can exist, since a new batch starts only after this.
2. `{stem}.pgn` exists with a count at or past `file_positions` (crash after the last game,
   before sealing): seal and compress it now, return `None`.
3. `{stem}.pgn` exists below that: resume from its count.
4. Nothing: a fresh batch from 0; a stray `{stem}.count` is removed, as today.

## What goes away

- The decompress-and-compare of `resolve_batch_start`, and `newest_segment`.
- `remove_orphans`' directory-wide scan: orphans can only be the `.tmp` and companions of a sealed
  file, handled by the recovery table. Kept `.count` files are never touched.

## What must recognise a sealed file

- `other_active_stem`: a sealed `{other}-{timestamp}.pgn` is unfinished work of `other`. Today it
  strips `.pgn` and would report `beta-20260926-1603` as a stem, and, worse, would report this
  machine's own sealed file as another stem's. It must strip a trailing `-YYYYMMDD-HHMM` before
  comparing.
- Uploads: `is_segment_file` takes only `.pgn.zst`, `.pgn.zst.sha256`, `.pgn.zst.count`, so `F` is
  never published. rotor-prepare reads only `*.pgn.zst`. No change.

## Moving

1. `src/remote.rs`: `remove_uploaded(local_dir, accept) -> Result<(), RemoteError>`, with a
   `_under` variant for tests: removes each local file `accept` takes whose remote counterpart
   exists with the same size.
2. `publish_segments`: after a successful `copy_up`, `remove_uploaded` with a filter taking
   `.pgn.zst` and `.pgn.zst.sha256` only. A failed removal is a warning. Prints
   `moved {games_dir} to {remote}`. No exception for a sealed file or the newest segment: recovery
   no longer needs any segment.
3. `corpus_positions_so_far`: unchanged. Online, moved segments come from the remote listing and
   their counts from the local companions. Offline, moved segments are not counted:
   undercounting only costs extra batches.

## Docs

The doc comments of `run`, `compress_and_clear`, `resolve_batch_start`, `other_active_stem`;
`README.md` (the `--sync-only` line and the flow section).

## Tests

- `remove_uploaded_under`: removed when sizes match; kept when the remote copy is missing or
  differs; files outside the filter untouched.
- `compress_and_clear`: after it, only `F.zst` and its companions remain; no `F`, no
  `{stem}.pgn`, no `{stem}.count`.
- Recovery, one test per table row, built by laying out the files by hand, with no local segment
  in the third row (it was moved): the result is exactly one segment and its companions, never two.
- `resolve_batch_start` row 2: a full unsealed `{stem}.pgn` is sealed and compressed.
- `other_active_stem`: flags another stem's sealed file under the stem's name; ignores this
  stem's own sealed file.
- A kept `.count` of an older, moved segment survives a recovery.
