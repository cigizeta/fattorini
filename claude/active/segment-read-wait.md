# A segment that cannot be read is waited for

**Status: design agreed with the developer 2026-11-04; implemented the same day, tests and clippy passing; a real wait not yet seen.
Two departures from the plan, recorded in steps 2 and 4.**

## Why

Lc0's first scatter on `data/lc0` (2026-11-02) died after 79 minutes: one segment's read from Drive's
streamed `G:` failed ("Incorrect function", os error 1), the stream worker panicked, and the scatter
was lost. The cause was a Google Drive sync not yet complete. Since resumable-scatter.md a failure
costs one batch, but the job still dies and waits for a Resume by hand.

Every reader of a segment (`SelfPlaySegmentReader`, `game_labels.rs`) panics on any I/O error, at
the open and at each game's read. A read from `G:` can fail because Drive has not finished syncing
the file or because the machine is offline; the program cannot tell which (both arrive as the same
I/O error), and waiting is the right answer to both.

## Decisions

1. **An I/O error is waited out, not fatal.** At a failed open or game read, the reader logs the
   segment and the error, waits 60 s, reopens the segment from the start (a zstd stream cannot be
   resumed mid-way) and skips the games it already delivered, by `next_game_index`, with a fresh
   collector. Then it reads the game that failed.
2. **The records are identical to an uninterrupted read.** The worker picks its file before the
   read, and game indices are unchanged, so every random draw and record is as without the error;
   the scatter stays reproducible.
3. **No limit on the wait.** A sync can take hours. The wait ends when a read succeeds, or at the
   job's Stop: the waiting reader checks the stop flag at each retry and returns as a stopped
   stream, so the scatter ends as `Prepared::Stopped` and its batch resumes later.
4. **Parse errors still panic at once** (a comment without a score, an illegal move, a bad tag):
   data bugs, which no wait cures.
5. **The cause is not guessed.** The page and the log say, as of the first failure:
   "cannot read `<segment>` from Google Drive for N minutes, retrying every minute: `<error>`. Drive
   may still be syncing, or offline." The developer checks Drive's own window.
6. **A truly corrupt file** (a zstd checksum failure in a damaged cache) also arrives as an I/O error
   and waits until stopped, its path on the page. Accepted: rare, and visible.
7. **Every reader gets it:** the scatter, the frozen test set's freeze and read, the polyline's
   sample, the Convert's readers. One place in the code.

## Plan

1. **`game_labels.rs`:** `SelfPlaySegmentReader::open` and `next_game` return
   `Result<_, Stopped>` and take a `ReadWait` (the stop flag, if any, and the wait, 60 s; tests give
   a short one). The retry loop as in decisions 1 and 3. Parse panics unchanged.
2. **The waiting read shown:** a static registry in the `progress` crate, as the lease's wait was
   shown (`lease_wait` in the status JSON): each waiting reader registers its segment, the first
   failure's time and the last error, and clears it when a read succeeds. The status JSON carries the
   list; the "Running now" box draws decision 5's line under the job's own. The terminal logs each
   retry once a minute.
   **As built:** a `progress::Notice`, listed by `progress::live` among the live bars as a bar with
   no length and no rate, which the page's `liveLine` draws as its text alone. No new JSON, no new
   box: the line shows wherever the live bars show.
3. **`pgn_stream.rs`:** `StreamOptions` carries the job's stop flag; a worker's `Stopped` ends it as
   a failed send does today, and the scatter's loop already checks the flag after the stream.
4. **Other callers** (`freeze.rs`, `polyline.rs`, `lc0/convert.rs`, `lc0/mod.rs`): pass their job's
   stop flag where they have one, none otherwise (they then wait until the read succeeds or the
   process ends). Which flag each has, read at the code.
   **As built:** only the scatter passes its stop (`buckets::prepare`, through `StreamOptions::stop`;
   after the stream it checks the stop again, so a batch ended short is never committed). Every other
   reader keeps `SelfPlaySegmentReader::open` and `next_game`, which wait forever: their waits show
   on the page, but a Stop does not end them. To revisit if one is ever seen waiting.
5. **Tests:** a reader over a fixture segment whose file is made unreadable for its first reads
   (opened elsewhere exclusively, or renamed away and back) delivers every game once, in order, the
   same as an undisturbed read; a stop during the wait returns `Stopped`; a parse error still panics.
6. **TODO.md:** the scatter entry's cause corrected (an incomplete sync, not a transient error), its
   "retry a failed read" idea replaced by this document.

## Check

Tests and clippy; then a scatter seen waiting, if one ever meets an unsynced file.
