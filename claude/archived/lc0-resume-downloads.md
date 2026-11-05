# lc0-resume-downloads

**Abandoned (2026-10-17):** storage.lczero.org serves no byte ranges. The first resumed run
failed with curl error 33, "HTTP server does not seem to support byte ranges. Cannot
resume." The kept parts:
- `bar_in`'s `start`.
- A Ctrl+C handler, which now deletes the `.tmp` files and exits.
- The startup deletion of `.tmp` files, restored.

See lc0-download-retry.md for what this means for curl's retries.

## Goal

The developer (2026-10-17): Ctrl+C stops `smithy lc0` cleanly, staging keeps every file,
and the next run continues each partial download from where it stopped instead of
starting it over.

## Why resuming is safe

- **A published tar never changes.** Lc0 writes each hourly tar once, and the listing
  shows only finished ones, so a `.tmp` left by an earlier run is an exact prefix of the
  same file.
- **curl writes in order.** A curl that was killed leaves a file ending wherever its last
  write stopped, so continuing from that size (`-C -`) is correct. It is the same thing
  curl's in-run retries already do.

## Design

- **No `.tmp` deletion at startup.**
- **`download` checks the `.tmp` first**, against the tar's `Content-Length`:
  - Larger than the tar: it cannot be a prefix, so it is deleted and the download starts
    over.
  - The same size: the earlier download finished, but the rename to `.tar` did not
    happen. It is renamed without running curl.
  - Smaller: curl continues it with `-C -`.
- **The bar starts at the bytes already there.** `progress::bar_in` takes a `start` as
  `resumed` does, so the ETA and the rate count only this run's bytes.
- **Ctrl+C handler** (`ctrlc`, already a dependency of smithy):
  - It sets a static `STOPPING` flag, prints that the run stopped and resumes next time,
    and exits with 130.
  - The curl processes share the console, so they receive the same Ctrl+C and die,
    leaving their `.tmp` behind.
  - Their deaths would otherwise reach `fail` as "failed to download" before the
    handler's exit. While `STOPPING` is set, `fail` parks the calling thread instead of
    printing, so the handler alone ends the process.
- **Unchanged:** segments from an unfinished conversion are cleared and made again at
  the next start, and complete `.tar`s in staging are converted without downloading them
  again.

## Test

- `cargo test -p progress`: `bar_in` starts at `start`.
- On the training machine:
  1. Press Ctrl+C during downloads: the stop message appears, without "failed to
     download", and the `.tmp` files remain.
  2. Restart: each download bar starts partway, and the rate shown is only this run's.
