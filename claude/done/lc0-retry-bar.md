# lc0-retry-bar

## Problem

On 2026-10-17, `20260104-0217`'s connection was reset at 799 of 829 MiB. curl's retry
(lc0-download-retry.md) started the tar over, and its bar fell to 27 MiB, but kept its
clock from the first attempt. It then showed 64.73 KiB/s and an ETA of 3.5 hours, while the
real speed was still about 1.96 MiB/s.

## Fix

In `download`'s polling loop, when the `.tmp` reads smaller than the last poll, curl has
started a new attempt, so reset the bar first. indicatif's `reset()` zeroes the position, the
elapsed time and the ETA, so the rate and the ETA describe the new attempt alone.

## Test

No unit test, since it needs a dropped connection. It is checked when the next reset comes:
the bar starts over at 0 with a normal rate.
