# Ladder tie-break: a drawn ladder duel goes to the player with more wins

## Status

Superseded, see round-robin-selection.md: the ladder it amended is gone, and the round robin ranks
by points instead.

Was done (decided with the developer, 2026-10-02): `rotor-arena` tests pass. Amends decision (a) of
ladder.md. Not yet run on real candidates.

## Problem

Under ladder.md's decision (a), a drawn ladder duel keeps the holder. With the duel rule, engines
less than about 34 Elo apart usually draw at the 1000-game cap (duel.md), and neighbouring lambdas
plausibly sit that close. The grid is ascending, so the ladder systematically kept the lowest lambda:
its winner meant "no later lambda beat it clearly", not "the best lambda".

## Decided

- **A drawn ladder duel goes to the player with more wins.** If the newcomer has more wins than
  losses, it takes the ladder; if fewer, the holder keeps it.
- **Wins equal to losses: the old rule**, the holder keeps it.
- **The final duel, the gate, is unchanged**: only a challenger win passes, and a draw never
  promotes.
- Under `best_of`, a drawn match is a level score, so wins always equal losses and nothing changes;
  the SPRT never draws. In practice the tie-break only acts on the duel rule, but it is written for
  any rule.

## Code

- `runner.rs`: `Resolution` carries the challenger's record (`wins`, `losses`, `draws`), from the
  rule's own `record()`.
- `main.rs`:
  - `play_duel` returns the `Resolution` with its `ladder.txt` line.
  - A pure function, `newcomer_takes_ladder(outcome, wins, losses)`, decides; unit-tested.
  - A drawn ladder duel's label no longer claims the holder keeps it; the tie-break's result is
    appended to the line, so `ladder.txt` records which way it went and why.
  - `run_ladder`'s doc comment.
- `README.md`: the ladder paragraph.
- ladder.md: decision (a) gets a note pointing here.

## Verification

```
cargo test --bin rotor-arena
```
