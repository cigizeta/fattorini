# The Convert's slow downloads reconnected

Plan, 2026-10-30. Status: approved and implemented 2026-10-30; tests, clippy and a Convert run to check.

## Why

storage.lczero.org sometimes serves a connection at about 40 KiB/s for hours, while a new
connection gets 1.5 MiB/s or more. Seen on 2026-10-29 and 2026-10-30. curl from the same machine was
just as slow, a speed test showed the line fast, and switching Wi-Fi off and on, which made curl
start the tars over, made them fast at once. At 40 KiB/s one tar (about 830 MiB) takes 6 hours and
the converter, which takes the tars in date order, waits on it.

## Design

1. **Detect.** curl is given `--speed-limit 262144 --speed-time 60`: a transfer below 256 KiB/s for
   60 s in a row is aborted with exit code 28. The threshold is far above the slow connections
   (40 KiB/s) and far below a normal one (1.5 to 2 MiB/s); 60 s keeps a short dip from aborting.
   It also catches a connection that never sends its first byte.
2. **Reconnect in `download`, not in curl.** curl's `--retry 5 --retry-all-errors --retry-delay 10`
   is removed and `download` loops over attempts itself:
   - exit 0: the tar is whole, checked against its size as today;
   - exit 28 (too slow): a new attempt at once, without limit, since a slow connection is no
     failure and must never end the run;
   - any other failure (a reset, error 56, and the like): a new attempt after 10 s, at most 5 such
     retries per tar, then the run fails as today.

   Left in curl, slow aborts would spend the 5 retries and stop the whole Convert.
3. **Each attempt starts the tar at 0.** The server serves no byte ranges (curl error 33), and
   `-o` truncates the `.tmp`. The board's place for the worker is reset at each attempt, so the
   rate and ETA are measured afresh. Abandoning even 90% of a slow tar still wins: the rest at
   40 KiB/s is over half an hour, the whole tar at 1.5 MiB/s about 9 minutes.
4. **Reported on the terminal, not the page.** Each slow abort prints one line,
   `{date}: below 256 KiB/s for 60 s, reconnecting`, to the smithy's terminal, where the Convert's
   lines already go; the page's bar visibly starts over. The page is unchanged.
5. **A known edge.** A connect timeout is also exit 28, so with the network down a worker retries
   without end instead of failing after about a minute. Accepted: the Convert resumes by itself
   when the network returns, and Stop still ends it.

## Changes

`crates/smithy/src/lc0/mod.rs`:

- Two constants beside `DOWNLOAD_POLL`: `SLOW_BYTES_PER_SECOND = 256 * 1024` and
  `SLOW_SECONDS = 60`, each with its why.
- `enum Attempt { Whole, Slow, Failed }` and `fn attempt(code: Option<i32>) -> Attempt`, mapping
  curl's exit code (`None`, killed by a signal, is `Failed`).
- `download`: the attempt loop above around today's spawn-and-poll; its doc comment rewritten (no
  more "curl retries by itself").
- The comment at `fetch_tars`'s spawn ("the next run continues its `.tmp`") corrected: a run
  stopped mid-tar starts that tar over.
- The `CONNECTIONS` comment's "about 2 MB/s each" kept.

Tests, in `mod.rs`'s `tests`: `attempt` maps 0, 28, 56 and `None` correctly. The loop itself is
checked by hand: a Convert run, a tar seen restarting on the terminal when slow.

## Checks

```bash
cargo test -p smithy lc0
```

```bash
cargo clippy -p smithy --all-targets
```
