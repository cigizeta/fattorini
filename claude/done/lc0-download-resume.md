# Lc0's downloads resume after a reset

## The problem

`smithy lc0` abandons a download below 256 KiB/s for 60 s and reconnects (`SLOW_SECONDS`,
`download` in `lc0/mod.rs`). On 2026-10-30 one tar, about 816 MB, stayed slow on every new
connection (about 36 KB/s), while another of about the same size downloaded at full speed. Each
reset started the tar over, since the code assumed the server serves no byte ranges (curl error 33,
2026-10-17).

## What was measured (2026-10-30)

- Both tars were `cf-cache-status: MISS`: Cloudflare served neither from its cache, so the speed is
  Lc0's server's, per file or per moment, and nothing on our side changes it.
- The server answers `Accept-Ranges: bytes`, and a range request gives `206` with exactly the bytes
  asked, both at the tar's start and 400 MB into it.

So a reset can keep what it has and continue.

## The change

1. **Resume within a tar's attempts.** The first attempt starts the `.tmp` as today. Every later
   attempt, after a slow or failed one, passes curl `-C -`, which continues from the `.tmp`'s size.
2. **A refused range starts over.** If the server answers a resumed request without a range, curl
   exits 33: the `.tmp` is deleted and the next attempt starts from zero, as today, without
   counting as a failure.
3. **The slow window: 15 s instead of 60.** A reset no longer loses bytes, so there is no reason to
   wait a minute to drop a slow connection.
4. **The board measures each attempt from where it resumed:** the attempt's `Flow` starts at the
   `.tmp`'s size, so its rate and ETA are this attempt's while the total stays the tar's (the
   start/total rule of `progress::resumed`).
5. **The final check stays:** a whole tar must be exactly its `Content-Length`, or the run stops.
   It also catches a resume that appended wrongly.
6. **Comments corrected:** the "no byte ranges" claims in `download` and at startup are now wrong
   and are rewritten.

## Not changed

- **Across runs:** startup and Ctrl+C still delete every `.tmp`, so a stopped Convert restarts its
  tars from zero. Keeping them would need the file's identity checked across runs (its `ETag`); a
  later step, if a restart proves costly.
- **The order of the tars.** Resuming keeps every byte a slow tar gets, but cannot make a slow file
  fast: a tar slow on every connection is skipped instead (lc0-skip-slow-tars.md).

## Risk

A tar replaced on the server between two attempts, at the same size, would be spliced. Lc0's tars
are not rewritten after publication (`Last-Modified` is their hour), and the attempts are minutes
apart, so this is accepted. An `If-Range` with the tar's `ETag` would close it, if ever needed.

## Tests

- `attempt(Some(33))` is its own case, a refused range.
- The arguments of a first attempt and of a resumed one (a small function building them, so the
  test needs no network).

## Status

Done 2026-10-30: in a real Convert, the slow tar reconnected every 15 s "where it stopped", never
starting over. It stayed slow all the same: the slowness was the file's (lc0-skip-slow-tars.md).
