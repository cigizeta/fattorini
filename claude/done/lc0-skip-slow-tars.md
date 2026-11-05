# Lc0's Convert skips a slow tar, and records it

## The problem

On 2026-10-30, `training-run2-test91-20260104-1817.tar` (about 816 MB) downloaded at about 9 to 36
KB/s on every connection, resumed or fresh, while its neighbours 1717 and 1917, measured in the same
minute on fresh connections, gave about 1.75 MB/s. The slowness belongs to the file, on Lc0's side
(Cloudflare serves none of these tars from its cache: `cf-cache-status: MISS`).

The converter takes the tars in date order (`converting`), so one such tar, about 6 hours at that
rate, holds up the whole Convert once staging is full.

## The rule (the developer, 2026-10-30)

- **A tar is skipped** when its download averages below `SLOW_BYTES_PER_SECOND` (256 KiB/s) over
  its first `SKIP_AFTER` (2 minutes), resumes included. Long enough that a short dip on a good tar
  does not skip it; a tar this slow shows its rate in seconds.
- **Every skipped tar is recorded**, in `data/lc0-counts/skipped.tsv` on the remote: the tar, when it
  was skipped, the bytes received and the average rate. A row is appended at each skip, so a tar
  skipped again on a later run has one row per time.
- **A skipped tar is not done.** It gets no row in `tars.tsv`, so the next Convert lists it as
  pending again, in its date order, and tries it once more: converted if the server serves it at
  speed by then, skipped and recorded again otherwise.
- **Said:** the terminal names the tar at its skip; the Convert's last line names every tar
  skipped in the run.

## The change

1. **`download` returns how it ended:** `Downloaded`, or `Skipped { bytes, rate }`. It measures the
   tar's average rate since its first attempt, across resumes; at `SKIP_AFTER` below the threshold,
   it stops curl, deletes the `.tmp`, and returns `Skipped`. After `SKIP_AFTER` the tar is never
   skipped: a dip later on is only a resume.
2. **`Slot::Skipped`**, a new state after `Approved`: the worker sets it instead of `Ready`, gives back
   the tar's bytes of `staged_bytes`, appends its row to `skipped.tsv`, and takes its next tar.
3. **The converter** waits for `Ready` or `Skipped`. On `Skipped` it writes no row, moves
   `converting` past the tar, adds it to the run's skipped list, and goes on with the next tar.
4. **`ahead`** (the cap's prediction for the tars approved and not yet converted) leaves a skipped
   tar out, as it does an empty one.
5. **The cap's `decide`** is unchanged: a tar is decided before its download, and a skipped one adds
   nothing to `tars.tsv`, so the cap counts what was converted alone.

## Consequences, accepted

- **The test set's membership.** The earliest tars make the test set (`test.games`). A skipped tar
  there leaves its games to the tars after it, and, converted on a later run, it goes wherever
  `decide` sends it then. `data/lc0-counts` is a new corpus, so nothing frozen depends on its
  membership today; `skipped.tsv` and `tars.tsv` together say what happened.
- **Gaps, by design (the developer, 2026-10-30).** A run that stops at the cap with tars skipped
  before it leaves gaps in the date order, and a later Convert stops at the cap before trying them:
  they are never converted. That is the point of skipping. The corpus is a size, not a span of
  dates: the cap counts bytes, and Lc0's hourly tars are interchangeable samples of one run, so a
  later hour stands in for a skipped one. `complete.txt` is written as today, and `skipped.tsv`
  names every gap. Not a bug to fix.

## Tests

- The skip rule as a function: the bytes received at `SKIP_AFTER` against the threshold, below
  skipped, above not; past `SKIP_AFTER`, never skipped.
- `skipped.tsv`'s row format reads back what it wrote.
- A skipped tar counts nothing in the cap's prediction (`ahead`).
- Not written: a pipeline with three pending tars, the middle one skipped. It needs the workers'
  downloads replaceable by a fake, a larger refactor than this change; the real run exercises it.

## Status

Done 2026-10-30, with the resume (lc0-download-resume.md): in a real Convert, `20260104-1817` was
skipped after its first two minutes, and the two workers went on at full speed with the next tars.
