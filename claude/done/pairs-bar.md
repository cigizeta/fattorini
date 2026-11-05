# The round robin's graphical bar, its time from the record

Status: implemented 2026-10-23, to build and test; not compiled when written.

## What (the developer, 2026-10-23)

A graphical bar in the round robin's section, as the training box's: `4,812 of 28,800 games (16.7%),
11m elapsed, ETA about 54m`, then the bar. The live box's text bar stays until indicatif goes
(TODO.md).

## Decisions

1. **Records first (the developer, 2026-10-23, a general rule):** where a record exists on disk, the
   time comes from it, as the training box's from `history.tsv`, never from the process's own clock.
2. **Each pair's arrival time in its record:** a sixth field, UTC Unix seconds, after `pairing r chore
   session seq`. The reader takes the first two, so older lines, without it, still read. The one
   writer serves every pairs record: the round robin's, the gate's, a test's.
3. **Elapsed is the sum of the gaps between consecutive arrivals, a gap over 5 minutes a pause** (the
   developer, 2026-10-23), so a long stop adds nothing.
3b. **Each start of dealing in the record too (the developer, 2026-10-23):** a line `#start<TAB><UTC
   Unix seconds>`, written when its phase begins dealing, a resume's included; the gap from it to
   the first pair counts, as any gap, a pause over 5 minutes left out, and the gap up to a start
   never. Seen on the first gate timed: its 21 pairs, dealt at once, arrived within 2 s, so arrivals
   alone gave "2s elapsed" for about a minute of play. Lines starting `#` carry no pair: every
   reader skips them.
4. **The ETA:** elapsed over the gaps counted, times the pairs left; none before two timed lines.
   Shown in games, as the heading counts them.
5. **The gate:** its elapsed time, no ETA, its SPRT ending whenever its evidence is enough.
6. **The rate (the developer, 2026-10-23):** games a second over the same counted gaps, beside the
   elapsed time, as the text bars write it: games a second from one a second, seconds a game below.
   On the round robin, the gate and the tests; not the training box, whose round time is its speed.
7. **A test** has its ETA from its record by the same rule: `test_eta`, and `View::Test`'s start and
   resumed games, go.

## The work

- `smithy.rs`: `Shared::pair_times`, read from the record when pairs are dealt
  (`recorded_times`), each arrival appended to the record and to it; `play_time` and `pairs_eta`;
  `elapsed_seconds` and `eta_seconds` in the round robin's, the gate's and the test's views, an
  attempt viewed later read from its record.
- `page.html`: the round robin's line and bar; the gate's elapsed; the test's elapsed beside its ETA.
