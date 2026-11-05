# The reported PV runs past a fifty-move draw

**Status: agreed 2026-10-20 (the developer: both changes); implemented, not yet built or tested.**

## Cause

Seen in the OpenBench smoke tune: fastchess warned "PV continues after fifty-move rule". In
`alpha_beta_with_buffer`, a node at the fifty-move mark is not stopped at entry, as repetition and
insufficient material are, since checkmate must win over the rule: the draw is called only after
move generation. By then the node has searched its children, and every improving child wrote its
line into this ply's PV row. The node returns `0` with the row still full, and the parent splices
it into its own line. The score was always right; only the reported line was wrong.

## Changes

1. **The fix:** the late fifty-move check clears this ply's PV row before returning `0`, so a drawn
   node reports no continuation, as every other terminal node. Not at the root, whose row is where
   `search_root` reads the move it plays.
2. **The speedup, as Stockfish and Rotor's own quiescence:** at entry, `ply > 0` and not in check
   and at the fifty-move mark, return `0` at once: without check no mate is possible, so nothing
   needs searching. The late check stays, for the in-check case and the root.

Change 2 alters node counts at such nodes, so `rotor bench`'s node count may change: a running
tune's server and clients should run one build (a mismatch only warns).

## Tests

- A fifty-move node below the root, in check with evasions, searches them and leaves its PV row
  empty.
- One not in check returns `0` having searched only itself (one node).
- Checkmate still wins over the rule (the existing test).
