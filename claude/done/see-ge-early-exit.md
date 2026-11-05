# An early-exit `see_ge` (A5 of search-optimizations.md)

**Status:** plan, drafted and implemented 2026-10-07 while betty's duel against hand-crafted ran
on the Lichess machine; tested and committed the same day, node counts unchanged; done. Timed only together
with three other tree-identical changes of the day (TODO.md). Off the label path: `see_ge` must equal `see(...) >=
threshold` exactly, so node counts must come out identical.

## Today

`see_ge` was `see(board, mv) >= threshold`: the whole exchange on the target walked forward to
its last capturer, then resolved backward. It runs for every capture the move picker sorts
(the good-capture test, `-score / 18`, which called `see` directly), quiescence's SEE pruning and
the main search's SEE pruning.

## Why not Stockfish's `see_ge` as is

Its king rule differs from Rotor's `see`: Rotor values the king at 0 (`eval::piece_value`), so a
king capture is just a free piece that can be taken back for nothing, where Stockfish undoes a
king capture into remaining attackers. Ported as is, some answers would change, and with them
the tree.

## Design

The early exit derived from Rotor's own recurrence, `gain[i] = d[i] - max(0, gain[i + 1])`: the
question alternates between "the next side's best continuation is at most `b`" and "at least
`b`", `b` becoming `d[i] - b` at each ply; each `d[i]` is known before its capturer is searched,
so a decided bound exits before the next `attackers_to`. The derivation is in `see_ge`'s doc
comment. `see` and `see_ge` share their start (`exchange_start`). The move picker and the
`move-stats` path call `see_ge`; `see` stays, as the test oracle and the exact value.

## Verification

- An oracle test: every pseudo-legal move along random games, 13 thresholds each, including the
  exact value and one on either side: `see_ge` equals `see >= threshold`.
- `cargo test --release --lib`, and node counts: caroline 1,697,834, hand-crafted 3,183,476.
- Speed on idle the Lichess machine, with the other three tree-identical changes of 2026-10-07.
