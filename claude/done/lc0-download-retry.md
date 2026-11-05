# lc0-download-retry

## Problem

`smithy lc0` died on 2026-10-17 when storage.lczero.org reset the connection
(curl error 56) while downloading `training-run2-test91-20260103-0517.tar` in the
background. `download` calls `fail`, which runs `process::exit(1)` from the download
thread, so it also killed the conversion of `...-0317` at 96%. A rerun resumes
correctly, but a dropped connection on a ~830 MB download should not stop a run that
lasts hours.

## Fix

In `download` (`crates/smithy/src/lc0/mod.rs`), have curl retry by itself:

- `--retry 5 --retry-all-errors --retry-delay 10`: up to five more attempts, 10 s
  apart, on any error, including a connection reset in the middle of the transfer.
- `-C -`: each retry continues from the size of the `.tmp` file instead of starting over.
  This is safe because the `.tmp` belongs to this run only: leftover `.tmp` files are
  deleted at startup, so a resume never adds to a file from an earlier run.

**Correction (2026-10-17):** `-C -` is dropped. storage.lczero.org serves no byte ranges,
so with `-C -` every retry would have failed with curl error 33. Without it, curl
truncates the `.tmp` and starts each attempt over: a whole tar's download is lost on
each retry, but the run carries on.

If all six attempts fail, the run still stops, and a rerun resumes as it does now.

## Test

There is no unit test, since it would need a flaky server. It is checked by the next
real run; a retry is silent unless every attempt fails.
