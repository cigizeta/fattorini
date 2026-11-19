# Lc0 conversion: a corrupt game, and curls outliving the run

## What happened (2026-11-19)

One game in `training-run2-test91-20260124-1917`, `training.186053895.gz`, failed to gunzip
("incomplete deflate stream"), and `fail` ended the conversion with `process::exit(1)`. The tar itself
was whole: `download` checks its size against Content-Length, so the game is corrupt in Lc0's own tar,
and downloading the tar again would not help.

The job still looked alive for seven more minutes. A download's `curl.exe` had inherited the
converter's console output, so the smithy's pipe stayed open until that orphan gave up on its slow
transfer (exit 28). Only then did the smithy see exit code 1.

## Fixes

1. **A game that fails to gunzip is skipped, not fatal.** The converter logs one line naming the
   tar, the member and the error, and goes on. A broken tar structure (`next_file`) and a game that
   gunzips but fails to convert (`convert_game`) both stay fatal: they mean a bug or a truncated
   tar, not a single damaged member. The count is not added to `tars.tsv`, so its schema and the
   existing rows stay unchanged. The log line is the only record, because corrupt games are rare.
2. **Curls die with the run.** At startup, `lc0` puts its own process in a Job Object with
   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The curls it spawns join the job. When the process exits,
   however it exits, Windows closes the job's last handle and kills them. This needs `windows-sys` in
   `smithy` (the JobObjects, Threading and Security features).

## Resume

Run the conversion again. The converted tars are already in `tars.tsv`, and `1917` is converted from
the start again, without its corrupt game.
