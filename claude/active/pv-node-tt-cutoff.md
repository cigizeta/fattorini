# No TT cutoff at PV nodes

**Status:** design agreed (2026-11-15), waiting. Not to be implemented or measured until the
current time tune and the 180+2 and STC check duels are over: a search change on the machines now
would disturb them, and their results must be read against today's search.

## Why

The main search's transposition-table cutoff (`alpha_beta_with_buffer` in `search.rs`, right after
`self.tt.probe(board.hash())`) checks only `ply > 0` and the stored depth, never the window. So a
PV node can return straight from the table:

- its PV is cut short there, often to one move;
- the line below it is never walked again, so a repetition or the fifty-move rule along it (path
  state the table does not hold) goes unseen, the Graph History Interaction problem;
- a hash collision lands directly on the main line.

Stockfish cuts off at non-PV nodes only (`!PvNode`).

## Change

1. Add `beta - alpha == 1` to that cutoff's condition. A null window is how this search tells a
   non-PV node from a PV one; there is no `is_pv` flag and none is added.
2. Leave the rest as it is: the probe, `tt_move` for move ordering, every other reader of
   `tt_entry`, and the stores.
3. Reword the comment before `raw_eval` ("after the TT cutoffs above so a cutoff never pays for
   it"): at a PV node no cutoff happens before it now.
4. Quiescence's cutoff stays as it is. Guarding it too, as Stockfish does, is a separate,
   separately measured change if wanted.

## The ponder fallback

Already in place: `ponder_move` in `uci.rs` takes the TT move of the position after the best move,
checked against the legal moves, when the PV has one move. Nothing to do. It reads
`reports.last()`, the last completed depth's PV. If a one-move PV after a fail-high cut short by
time ever shows up without a ponder move, look at that separately.

## Tests

- A PV-window node at `ply > 0` with an exact entry deep enough still searches: the returned PV is
  longer than the cutoff would give.
- A null-window node with the same entry still cuts off.

## Measurement

The change affects strength, not only the PV shown, so it ships only after an A/B match through
`bin/arena` on one idle machine, both sides on it. Expected: neutral or slightly positive, since
PV nodes are few.
