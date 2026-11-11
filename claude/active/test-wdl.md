# Wins, losses and draws on a test's card

**Status:** implemented 2026-11-08, awaiting the developer's check.

The arena's command line says `+66 -84 =134` for a duel; the smithy's card said only games and
points. Asked by the developer for duels, matches and SPRTs alike.

## Why not from the pairs

The smithy keeps one number a pair, its score in -2..=2 from the challenger's side. That loses the
split: a pair scoring 0 is two draws or a win and a loss. So the games are counted where both
results are at hand, when a pair arrives (`record`).

## Design

- **`matches::Wdl`:** a pair's wins, losses and draws from the champion's side, as the arena's
  command line counts them, its first player the champion (the developer, 2026-11-08); the champion
  Black in the first game and White in the second; summed over pairs.
- **The record:** each pair's line gains a seventh field, `W,L,D`, after its arrival. A line written
  before has none: its pair counts as unknown, never as zero.
- **In memory:** `Shared::pair_wdl`, beside `Shared::pairs` and aligned with it: filled from the
  record when pairs begin (`play_pairs`, `play_test`, a resume included), pushed as each pair comes.
  Every phase's pairs carry it (round robin, gate, test); only a test shows it.
- **The view:** `WorkView::Test` gains `wdl: Option<Wdl>`, the sum over the pairs the verdict counts
  (`Judged::pairs`), so pairs dropped after a duel's verdict are left out of both; `None` when any of
  them is unknown. An ended test's card, rebuilt from its record (`catalog`), carries it too.
- **The page:** the card's table gains a column, `+/-/=`, as the server counts them: `+` the
  champion's wins, `-` the challenger's. Blank when unknown. The tests' log (`tests.tsv`) is
  unchanged.

## Check (the developer)

```
cargo test -p smithy
```

```
cargo clippy --workspace --all-targets
```

Then a test started from the page: its card shows `+W -L =D`, matching its games and points.
