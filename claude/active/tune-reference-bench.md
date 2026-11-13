# A tune's reference speed, benched once

Status: **agreed and implemented 2026-11-13** (the developer: "we stop the tune, fix the bug, clean
the tune, and restart it fresh"). Waiting for the tests.

## Why

The time tune of `lc0-psq-threats-screlu-g1-r1` showed three references in one evening: 1,260,500,
1,393,274 and 1,019,970 nodes/s per thread. The smithy benched again at every start and resume, two
of them restarts for the page, likely while the arena on its own machine still played; each arena
benches once and keeps its figure. So the clock every arena was dealt moved between restarts, by
up to about 0.8 of 10+0.1: a time tune tunes against the clock itself, so this matters more than
it did for the search tune. smithy-bench.md's unfinished step (no bench while the smithy's machine
plays) is the same cause.

## Decided

- **One bench a tune:** the first start benches (`loaded_bench`, as before) and writes the result to
  the tune's `spec.txt` as `reference_nps`; every resume reads it back and benches nothing. A
  restart can no longer move the clock.
- **The first bench on an idle machine,** the developer's to ensure; the page shows the figure as
  before.
- A spec without the line (a search tune of before) benches once at its next start and gains it.
- A test still benches at its start (one start a test; its resume is rare); unchanged here.

## Steps

1. `Tune` holds `reference_nps: Option<f64>`, written into `spec_text` as `reference_nps\t<v>` when
   set, read back by `open` from an existing spec before `check_spec`.
2. `Tune::record_reference(nps)` sets it and rewrites `spec.txt`.
3. `tune_job`: under a clock, the spec's reference, else a bench recorded.
4. Tests: a reference round trips through the spec and survives a reopen; a spec without one reads
   as none.
5. The developer: stop the tune, move `time-tune/` away (here and on Drive), make the smithy's
   machine idle, start the time tune fresh.
