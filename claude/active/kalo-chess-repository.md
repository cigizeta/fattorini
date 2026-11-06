# The repository becomes kalo-chess

**Status:** steps 1 to 4 done 2026-11-06 (tests passed, this machine's venv recreated); step 5 waits
until the other machines are idle.

## Decisions (the developer, 2026-11-06)

- **One name:** the repository, venv `.kalo-chess`, Drive root `G:/My Drive/kalo-chess` and firewall rules `kalo-chess smithy`, `kalo-chess arena`. The project
  is Kalo Chess; "Kalo Games" and "Kalo Go" leave the current docs. This supersedes the umbrella name
  of kalo-chess-rename.md.
- **No `chess/` level:** `configs/chess/` becomes `configs/`, `data/chess/X` becomes `data/X`, and
  `chess/books`, `chess/syzygy` under the data root become `books`, `syzygy`. Reverses that decision
  of temper-games-restart.md.
- **Kept:** the file tags `CHFC`, `CHFL`, `CHTR` and the magic `KALO`: changing them would break files
  already written, for no gain. No fallback to the old paths or names.
- **History stays:** `claude/` documents keep what they say.

## The code must not reach a machine before Drive does

The new code reads only `G:/My Drive/kalo-chess/data/...`, which does not exist yet. Machines running
old binaries are unaffected, but a machine that pulls and rebuilds before the Drive step finds no
store. So: commit on the branch, and leave it unmerged and unpulled elsewhere until step 5.

## Steps

1. The developer, the configs:

   ```
   git mv configs/chess/*.yaml configs/
   ```

2. The developer, the text, outside `claude/`:

   ```
   git grep -lIE 'kalo-games|Kalo Games|(data|configs)/chess|chess/(books|syzygy)' -- ':!claude' | xargs sed -i -E -e 's/kalo-games/kalo-chess/g' -e 's/Kalo Games/Kalo Chess/g' -e 's#(data|configs)/chess#\1#g' -e 's#([^-])chess/(books|syzygy)#\1\2#g'
   ```

3. Claude, by hand: the prose the `sed` leaves wrong (CLAUDE.md's opening, Kalo Go, README, DEV.md,
   "the data root's books/"), the TODO.md note, and a review of what scans `data/` or `configs/`
   directly, now that they are flat. A search for the old names must find only `claude/`.
4. The developer: tests and clippy, then the commit, on this machine.

   ```
   cargo test
   ```

   ```
   cargo clippy --all-targets
   ```

5. **When every machine is idle** (every smithy, arena and lichess-bot stopped, Drive fully synced):
   - Once, in the browser: rename the Drive folder to `kalo-chess`, then move the contents of its
     `data/chess/` up into `data/`.
   - lichess-bot's configuration: its engine is a snapshot on Drive, so its path follows the new
     Drive root and the flattened `data/snapshots/`.
   - The GitHub repository, in its settings.
   - Each machine: pull with the updated remote, rename the folder, flatten `data/` as was done here,
     recreate the venv as
     `.kalo-chess`, replace the firewall rules (DEV.md), rebuild. A machine may still hold rules of
     an older name, `temper-chess` here: list them all by name and delete what is old. This machine
     has its venv and firewall rules done. There is no release root any more: nothing in the
     repository reads or writes `%LOCALAPPDATA%`.

## To check at step 5

Files already written may record old paths as text: a run's `config.yaml` carries them only in
comments, but install records, `spec.txt` files and `results.db` should be searched for `data/chess`
in the local `data/` before calling this done. Anything that only records history can stay.

## Verification

`cargo test` passes; `bin/smithy` starts with the new venv active; `bin/arena` sees the moved data and
the Drive store.
