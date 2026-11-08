# The tune's ETA

**Status:** planned 2026-11-08, asked by the developer before the lc0 network's first real tune.

The tune's screen shows only `done of pairs` and a bar, while every other phase that plays pairs
(round robin, gate, tests) also shows elapsed, games/s and an ETA. A tune of 16,384 pairs at
10+0.1 takes most of a day, so its ETA is the line most worth having.

## Design

Reuse what the pair phases already have, with no new record on disk:

- **The times:** `Shared::pair_times`, the in-memory list a pair phase fills (`false` at each start
  of dealing, `true` at each pair's arrival). `run_tune` resets it to one start mark when the tune
  begins dealing; each tune pair that `apply` records pushes its arrival.
- **The pace:** `pairs_eta(&pair_times, pairs - k_done)`, unchanged: gaps above `PAUSE_SECONDS`
  (5 minutes) are a pause, not play, so a tune left without arenas does not slow its pace.
- **A resume counts from its own start.** The times live in memory only, so after a stop or a
  smithy restart the pace is measured afresh on the resumed pairs. Accepted: a few minutes of pairs
  give a usable pace, and a tune's own record need not change.
- **Only while it deals:** `WorkView::Tune` gains `time: Option<PlayTime>`, `Some` only while the
  work is `Work::Tune`. A finished tune, still shown while idle, has none: `pair_times` may by then
  be another phase's.

## Steps

1. `smithy.rs`:
   - `run_tune`: with `shared.work = Some(Work::Tune { .. })`, `shared.pair_times = vec![(unix_now(), false)]`.
   - `apply`, the `Work::Tune` arm: after `tune.record(...)`, `self.pair_times.push((unix_now(), true))`.
   - `WorkView::Tune { .., time: Option<PlayTime> }`; `work_view` fills it from `pairs_eta` when the
     work is `Work::Tune`, else `None`.
   - Test: a tune's view carries no time when idle, and a time once two pairs have arrived.
2. `page.html`, the `tune` case, laid out as a test's (the developer, 2026-11-08: the line above the
   bar, not under it): the heading `Tune of <player>`, then the line `done of pairs` followed by
   `times(view)` when `view.time` is there, then the bar.

**Implemented 2026-11-08**, awaiting the check below.

## Check (the developer)

```
cargo test -p smithy
```

```
cargo clippy --workspace --all-targets
```

Then a tune started from the page: within a minute or two of its first pairs, the line reads
`..., elapsed ..., ... games/s, ETA about ...`.
