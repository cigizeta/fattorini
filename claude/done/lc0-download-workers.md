# lc0-download-workers

## Problem

`smithy lc0` leaves its connections idle in two ways:

1. **The queue is too short.** Only `DOWNLOADS_AHEAD` (two) tars may wait behind the one
   converting, so both downloads stop once they are two tars ahead.
2. **Downloads start only between conversions.** The loop that starts them runs on the
   main thread, which spends about 3 minutes inside `convert_tar` for each tar. A
   connection that finishes partway through a conversion stays idle until that
   conversion ends.

The developer (2026-10-17): at most two connections, but download whenever the cap allows.

## Design

- **`CONNECTIONS` (2) download workers**, one thread each, looping:
  1. Claim the next tar in date order.
  2. Read its size with a HEAD request.
  3. Wait for its turn to be approved.
  4. Download it, then claim the next one.
- **Approval** happens strictly in date order, so the cap check for a tar always counts
  every tar before it. A tar is approved when:
  - its predicted output fits under `cap_bytes`, beside the rows already converted and
    the predictions for every approved tar not yet converted (`passes_cap`, as now), and
  - staging has room for it: approved but unconverted tars plus this one stay within
    `staging_bytes`, or staging is empty, so that a single large tar can never deadlock
    the run.

  An empty tar, or a tar already complete in staging, is approved without a connection.
- **Shared state** (`Mutex` and `Condvar`) holds the rows, one slot per pending tar
  (`Unclaimed`, `Sized`, `Approved`, `Ready`), the next indices to claim and to approve,
  the index of the tar being converted, the bytes in staging, and a `stopped` flag. Every
  change is followed by `notify_all`.
- **The converter** takes the tars in date order:
  - It waits until the tar's size is known, then calls `decide`. On `Stop` it sets
    `stopped`, which also makes the workers exit, writes `complete.txt` and returns.
  - Otherwise it waits until the tar is `Ready`, printing "waiting for … download" if
    it has to wait.
  - After converting, it appends the tar's row, frees its staging bytes and wakes the
    workers.

  When the converter reaches a tar, everything before it is converted and staging is
  empty, so a tar that `decide` lets through is always approved: the two cannot deadlock.
- **Config.** `staging_bytes: 100000000000` (100 GB) goes in `lc0.yaml`. The training machine has
  401 GB free.
- **Exit.** As now: a download still running at the stop ends with the process, and its
  `.tmp` is cleared at the next start.
- **Side fix.** Today's lookahead leaves the tar being converted out of `ahead`, which
  undercounts by one prediction. The new count includes it until its row is written.

## Test

- Unit test for the approval rule (`may_fetch`): the cap, the staging limit, and the
  empty-staging exception.
- Existing tests unchanged.
- First real run: both download bars stay busy while the conversion runs, and staging
  grows when downloads are faster.
