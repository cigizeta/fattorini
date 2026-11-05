# lc0-download-size-check

## Problem

`smithy lc0`'s `download` trusts curl's exit status alone. curl retries a dropped connection by
itself (lc0-download-retry.md), and since storage.lczero.org serves no byte ranges, each attempt
starts the `.tmp` over. That relies on curl truncating the file before a retry, which was assumed
but never verified. A retry that appended instead would leave a `.tmp` longer than the tar, renamed
to `.tar`, and the conversion would then read past the tar's real end.

## Fix (the developer, 2026-10-17)

After curl succeeds and before the rename, compare the `.tmp`'s size with the tar's
`Content-Length` (`tar_bytes`, already read from the HEAD request):

- **Equal:** rename to `.tar`, as now.
- **Different:** delete the `.tmp`, and stop the run with a message naming both sizes. The next
  run downloads the tar again; tars already in staging are kept, as at every stop.

Stopping rather than retrying in-process is deliberate. A mismatch means the assumption above is
wrong or the server changed a file, and either one needs looking at before the run goes on.

## Test

No unit test, since it needs a real transfer. Checked by the current run's next restart: tars still
download and convert as before.
