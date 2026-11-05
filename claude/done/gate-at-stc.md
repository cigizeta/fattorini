# The gate at STC

**Status (2026-10-16): done.** Applied in fb8c461. The rerun below was made: amanda-0003-0.98
failed her gate at STC too (generation-3.md), so the depth-9 control was not the only reason. That
generation 3 has since been retrained under rollback-to-best.md; its new gate plays at STC.

## Why

`arena select` tunes the winner's margin at STC (10+0.1), then gated it at the selection's fixed
depth 9. A margin pays off only in time saved, so at a fixed depth it buys nothing: the gate could
not reward the very thing tuned just before it. amanda-0003-0.98 failed its gate that way
(generation-3.md), against amanda-0002-0.99, +90 -73 =81 at depth 9.

## Decisions (the developer, 2026-10-15)

- The gate plays at `config::STC`, hard-coded like the margin search, so the margin is tuned and
  judged under one control by construction. No config key: `selection.depth` governs the round
  robin alone, `selection.gate` keeps `rule` and its value.
- The failed depth-9 gate of amanda-0003 is deleted by hand before the rerun; the gate's folder name
  does not carry the control.

## Changes

1. `run_selection` (`crates/arena/src/main.rs`): the gate's `play_duel` gets the STC control and
   `Run::placement` of it (half the logical cores, pinned to physical cores), not the round robin's.
2. `play_duel`: always in-process, since both players are Rotor (`match_player::plays_in_process`);
   it used to be in-process only at a fixed depth, which under a clock would have played over UCI.
   In-process players read their `margin.txt` (the challenger's beside its candidate, the champion's
   in its generation; the hand-crafted root has none).
3. Comments and help text that said everything runs at the config's one depth: `run_selection`'s
   doc, the start-up line, `--help`, `SelectionSpec`, `MAX_DEPTH`, `configs/*.yaml`.

## Rerunning amanda-0003

The developer deletes `data/networks/amanda/gen-0003/selection/amanda-0002-0.99-vs-amanda-0003-0.98/`,
then reruns `bin/arena select amanda` on the training machine with nothing else running: the round robin and the
margin rebuild from their records, the gate plays again, `verdict.txt` is rewritten. Timed games are
load-sensitive, so no select beside training any more.
