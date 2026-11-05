# Self-play segments from multiple machines

Status: implemented and verified by the developer (tests pass).

## Goal

Generate self-play segments on more than one machine, copy them to the training machine, and
train on all of them. Along the way, name a segment by the time it was *compressed* (when its
batch finished), not when its batch started.

## Decisions (from discussion with the developer)

1. **The staging file is `data/self/hand-crafted/{stem}.pgn`.** It replaces
   `{stem}-{timestamp}.pending.pgn`. With a fixed name there is nothing to discover, so
   `find_pending_file`, `looks_like_timestamp` and `TIMESTAMP_LEN` go away, and so does the
   ambiguity where one stem is a prefix of another.
2. **A segment is `{stem}-{timestamp}.pgn.zst`, with the timestamp taken at compression.** It is
   still UTC `YYYYMMDD-HHMM`, so names compare correctly across machines.
3. **Compression writes `{stem}-{timestamp}.pgn.zst.tmp`, then renames it to the final name.**
   The `.tmp` suffix doesn't match the `*.pgn.zst` glob (`self_play_segments`), so a trainer can
   never read a half-written segment. `rename` silently replaces an existing target, so assert
   the final name doesn't exist before renaming. This is the "never clobber a finished segment"
   guard (point 4 of the discussion), moved from `File::create_new` to the rename step.
4. **The stem stays an explicit, required `--stem`.** Hostnames were rejected: they may not be
   clearly set or unique. The operator gives each machine its own stem. Only `*.pgn.zst` files
   are copied to the training machine; `{stem}.pgn`, `{stem}.count` and any `.tmp` file are that
   machine's private state. rsync is safe for the copy, because it writes to a hidden temp name
   and renames when done:

   ```
   rsync -av --include='*.pgn.zst' --exclude='*' <host>:git/personal/rotor/data/self/hand-crafted/ data/self/hand-crafted/
   ```

5. **The trainer keeps a list of used segments and picks the next one from those available.**
   It no longer sweeps one sorted list taken at startup, and no longer asserts that the first
   unprocessed segment in sorted order is the checkpoint's current segment. That assert breaks
   as soon as a segment from another machine arrives and sorts earlier (`the training machine-...` before
   `the Lichess machine-...`).
6. No provenance header (`[Site ...]`) is added to the PGN output.
7. **`{stem}.count` is deleted at compression**, as the very last step (after the staging file is
   removed). The count then means "games in `{stem}.pgn`". The cumulative total was never read
   anywhere else, and it is derivable anyway (segments times `CHECKPOINT_GAMES`). This order
   makes the one new crash window harmless: a count left behind at a multiple of
   `CHECKPOINT_GAMES` with no staging file is a checkpoint boundary, so the next run simply starts
   a fresh batch. The reverse order would turn that window into a manual-recovery panic.
8. **One `rotor-self` at a time per machine**, enforced by an advisory lock:
   `data/self/hand-crafted/rotor-self.lock`, `File::try_lock` (std, Rust 1.89+), held for the
   whole run. It's the same file for every stem. The OS releases the lock on any exit, crash
   included, so a stale lock never blocks a restart. The lock file is never deleted, since
   deleting a held lock file would let a third process lock a new file under the same name. The
   lock is per checkout, and relies on `flock`, which is reliable on WSL2's Linux filesystem but
   less so on `/mnt/c`.
9. **At most one active stem per machine.** A stem is active while it has an unfinished batch:
   its `{stem}.pgn` exists, or its `{stem}.count` holds a value that isn't a multiple of
   `CHECKPOINT_GAMES`. After taking the lock, a run panics if any *other* stem is active. Other
   machines' segments never count as active, so stems can be switched at batch boundaries only.

## Implementation

### `src/self_play/self_play.rs`

- `write_results`: staging path is `{games_dir}/{stem}.pgn`, resolved by
  `resolve_initial_pending_path(games_dir, stem, completed)`. Rules:
  - `completed % CHECKPOINT_GAMES != 0`: `{stem}.pgn` must exist, otherwise panic (unchanged).
  - `completed % CHECKPOINT_GAMES == 0`: `{stem}.pgn` must not exist, otherwise panic (unchanged).
    New panic message, since there is no matching timestamp to point to any more:
    - If the newest `{stem}-*.pgn.zst` already holds these games (the crash came after the
      rename but before the staging file was removed), delete `{stem}.pgn`.
    - Otherwise (the crash came during compression) compress it by hand and delete it:
      `zstd -3 --rm {stem}.pgn -o {stem}-<YYYYMMDD-HHMM>.pgn.zst`.
    - In either case, any leftover `.tmp` can be deleted.
- `compress_and_clear(games_dir, stem, staging_path, timestamp)`, with the timestamp passed in so
  tests are deterministic (`write_results` passes `current_timestamp()`):
  1. `segment_path = {games_dir}/{stem}-{timestamp}.pgn.zst`, `tmp_path = {segment_path}.tmp`.
  2. Assert `segment_path` doesn't exist.
  3. Compress the staging file into `tmp_path` (`File::create`, so a stale `.tmp` from an earlier
     crash is simply overwritten), finish the zstd stream, flush, and `sync_all` so the data is on
     disk before the rename makes the segment visible.
  4. `fs::rename(tmp_path, segment_path)`.
  5. Remove the staging file.
- Delete `find_pending_file`, `looks_like_timestamp` and `TIMESTAMP_LEN`.
- Update the doc comments on `run`, `write_results`, `resolve_initial_pending_path`,
  `compress_and_clear`, `current_timestamp` (it is now a compression time, not a start time) and
  `CHECKPOINT_GAMES`. `run`'s comment also says the stem must be unique per machine.
- Tests (new `#[cfg(test)]` module, temp directories):
  - `compress_and_clear` produces `{stem}-{timestamp}.pgn.zst` that decompresses to the staging
    file's exact bytes, and leaves neither the staging file nor a `.tmp`.
  - `compress_and_clear` panics if the target segment already exists, leaving it untouched.
  - `resolve_initial_pending_path`: panics at a boundary with `{stem}.pgn` present, and panics
    off a boundary with it absent.

### `src/self_play/self_play.rs`, second pass (decisions 7 to 9)

- `compress_and_clear` also takes `count_path` and removes it after removing the staging file.
  Its test asserts the count is gone.
- `try_acquire_lock(games_dir) -> Option<fs::File>`: `None` on `TryLockError::WouldBlock`, and a
  panic on any other error. `run` exits with a clear message on `None`, and otherwise keeps the
  file alive for the whole run.
- `other_active_stem(games_dir, stem) -> Option<String>` implements decision 9. `run` panics,
  naming the other stem, when it returns `Some`.
- Order in `run`: create the directory, take the lock, check for another active stem, then read
  the count.
- Tests:
  - A second `try_acquire_lock` fails while the first is held, and succeeds after it's dropped.
  - `other_active_stem` flags another stem's `.pgn`, and another stem's count when it isn't a
    multiple of `CHECKPOINT_GAMES`.
  - It ignores another stem's count that is a multiple of `CHECKPOINT_GAMES`, segments, `.tmp`
    files, the lock file, and our own stem's files.

### `src/self_play/main.rs`

- Usage and doc comment: `--stem` names the generating machine and must be unique across all
  machines whose segments will be merged.

### `src/preprocessing_config.rs`

- `self_play_segments`: update the doc comment (the timestamp is a compression time). Add a test
  that a `.pgn.zst.tmp` and a bare `{stem}.pgn` are both excluded.

### `src/train/main.rs` (`run_train_streaming`)

- Replace `'segments: for segment in &segments` with `'segments: loop`, picking each segment as
  follows:
  - If `resume_cursor` is `Some`, resume that exact segment by name, whatever its sorted position.
    Panic if its file no longer exists, since the checkpoint can't be honored.
  - Otherwise, glob again (`self_play_segments(&parent)`) and take the first segment, in name
    order, that isn't in `processed_segments`. If there is none, leave the loop (the existing
    "every currently available segment has been swept" ending).
- Globbing again after every finished segment means segments copied in during a run get picked
  up without a restart.
- Put the choice in a small helper,
  `next_segment(available: &[String], processed: &HashSet<String>) -> Option<String>`, so it can
  be unit-tested without a directory.
- The upfront "no self-play segments found" check stays as it is.
- `StreamCheckpoint` needs no format change: `processed_segments` is already the list of used
  segments.
- Tests: `next_segment` skips processed segments, returns `None` when all are processed, and picks
  a segment that sorts earlier than ones already processed (the late-arriving-machine case).

### `claude/active/rotor-evolve.md`

- Update the "Output" section: staging name, timestamp at compression, `.tmp` then rename, one
  stem per machine, and only `*.pgn.zst` files are copied.

## Existing data

`data/self/hand-crafted/alpha-20260922-1702.pgn.zst` is named with its start time. Renaming it to
its compression time is optional and manual (its mtime gives the time). Don't rename it if a
streaming `rotor-train` run has already recorded it in `processed_segments` or as its current
segment, because the run would treat the renamed file as new and train on it again. No staging
file exists right now, so there is nothing to migrate from the old `.pending.pgn` naming.
