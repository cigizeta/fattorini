# `length: chained`: a run's first tune, then fine-tunes down its lineage

**Archived 2026-11-04: moot. The tune's length format was redesigned; `length: chained` is now
rejected.**

**Status: agreed with the developer 2026-10-22, implemented the same day, smithy's tests passed (338).
`lc0-psq` run 1 is the first run with it; its first real check is that run's tunes.**

## Why

A forge config's `tune:` applies to every passed generation of a run. With `length: first`, each
generation's winner is tuned from Rotor's default parameters for 16,384 pairs, although generation 2
learned from generation 1 and generation 1's tuned parameters are the better start. A fine-tune
(25-50% of the pairs, half `C_end`) needs a player to start from, and a config cannot name it: the
previous generation's winner has no name until the run makes it.

## The value

```yaml
tune:
  length: chained
  control: 10+0.1
```

- The run's first passed generation: as `first`.
- Every later passed generation: as `{fine: <the previous passed generation's winner>}`, the winner
  it was gated against, its name resolved by the smithy when the tune is asked for.

`first`, `{pairs: n}` and `{fine: <player>}` keep their meanings. For pesto, which has no lineage,
`chained` is `first`.

## Order

The tunes run after the run ends, one job at a time. A fine-tune from a generation not yet tuned is
refused by the existing check (`fine_tune_specs`: no `parameters.txt` to fine-tune from), so the
developer tunes a run's generations in order, oldest first. A stopped tune is resumed before the next
generation's starts; nothing falls back to `first`.

## Changes

1. `tune.rs`: `args_of(config, player, previous)`, `previous` the player a chained tune fine-tunes
   from, `None` for the first in its lineage; `chained` read as `--first` or `--fine <previous>`.
   Tests: both cases, and `chained` with no previous as `first`.
2. `smithy.rs`, `tune_args`: for a run's winner, `previous` is the largest passed generation below
   its own, by `run_winners`, named by `players::network_player_name`; pesto passes `None`.
3. `configs/lc0-psq.yaml` and `configs/pesto-psq.yaml`: `length: chained`. `configs/pesto.yaml`
   keeps `first`.
