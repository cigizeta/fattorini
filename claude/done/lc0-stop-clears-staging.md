# A Convert stopped at the cap clears staging

## Problem

The download workers fetch ahead of the converter on predicted outputs. When the converted tars
turn out larger than predicted, `decide` stops the run before a tar already downloaded whole, and
that `.tar` stays in staging for good: a complete corpus is never extended (raising the cap makes a
new corpus, `complete.txt` deleted by hand), so the tar only wastes disk until deleted by hand.

## Fix

On `Decision::Stop`, after `write_complete`, delete every `.tar` in staging. `remove_partial_downloads`
becomes `remove_staged(extension)`, called with `"tmp"` where it was and with `"tar"` at the stop.
A `.tmp` still downloading when the run stops ends with the process, as before, and the next
start deletes it.

## Test

None new: the deletion is a directory sweep like the existing one. Checked by hand at the next run
that stops at its cap: staging is empty afterwards.
