# lc0: an honest conversion rate

## Problem

The Corpus section's `converting` line showed about 15 MiB/s while whole tars converted at about a
third of that. Its rate was the tar's bytes read so far over the time since the tar was opened, but
`seal_and_upload` (zstd, sync, the copy to Drive) advances no byte: before a tar's first seal the
rate is the reading and gunzipping alone, and the last seal runs after every byte is read, at 100%
and an ETA of 0.

## Plan

- The board keeps the run's converted tar bytes and the time spent converting them, from opening each
  tar to its last upload, seals included and the waits for a download excluded.
- The conversion's rate is that ratio, `None` until the run's first tar is converted.
- The current tar's ETA is its whole conversion at that rate less the time already spent on it, so a
  seal at 100% still counts down; `None` once that is spent, never a negative or a false 0.
- The converting line gives the tar's size alone and its percentage by time, the time spent on it
  over its whole conversion at the run's rate, held at 99% until done (the developer, 2026-11-19: a
  byte count beside a time percentage would contradict it). Before the first tar's rate, the size
  alone. The rate itself is not shown (the developer, 2026-11-19): the run's average, it read as
  the tar's live speed beside the downloads'.
- Nothing names a seal on the page (the developer, 2026-11-19: too technical); the ETA still
  counting down at 100% says enough.
- The corpus line is unchanged: its rate is sealed positions over the run's wall time, seals
  included already.

## Verification

- `cargo test -p smithy`
- On the page, the converting line shows no rate for the first tar, then one near the log's pace
  between tars, and an ETA still counting down while a tar at 100% is sealed.
