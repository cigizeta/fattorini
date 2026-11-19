# lc0: retry the listing and the size query

## Problem

On 2026-11-19 storage.lczero.org answered a `HEAD` for a tar's size with HTTP 525 (Cloudflare could
not reach its origin over TLS). `remote_size` goes through `curl_text`, which fails the whole job on
the first curl error, although the tar downloads themselves are already retried (`RETRIES`,
`RETRY_DELAY`). The listing fetch in `tar_stems`' caller also uses `curl_text` and has the same gap.

## Plan

- `curl_text` retries a failed curl up to `RETRIES` times, `RETRY_DELAY` apart, printing one line per
  failure (the error and the attempt number), and fails as before once they are spent.
- Both callers (the listing and `remote_size`) get this, with no change of their own.
- A failure to spawn `curl.exe` at all still fails at once: no retry helps it.

## Verification

- `cargo test -p smithy`
- The next conversion run gets past a transient 52x.
