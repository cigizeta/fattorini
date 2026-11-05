# Step 3 of the textbook evaluation: mobility

Status: done. Applied; tests pass. Part of textbook-evaluation.md (step 3 of 4). Branch `alessio` only.

## Goal

Add CPW-Engine's mobility to `eval.rs`, as mobility alone: without the rook open/half-open file
bonus CPW merges into the same score (a separate kind of term, dropped), and without anything else
CPW's `EvalKnight`/`EvalBishop`/`EvalRook`/`EvalQueen` also do (trapped pieces, returning bishop,
early queen, material adjustments by pawn count). The king-attack count those functions collect
belongs to step 4. No search changes.

## Source

CPW-Engine's "CPW-Engine_eval" page (CC BY-SA 3.0), read from the saved copy.

## The rule

For each knight, bishop, rook and queen, `mob` is the number of squares it attacks that are not
occupied by its own pieces: empty squares, and enemy-occupied ones. Sliders stop at the first piece
in each direction. No other exclusion.

Score: **a table lookup by `mob`**, one mg and one eg table per piece type, generalizing CPW's
linear `weight * (mob - baseline)`. Sizes follow from the largest possible count: knight 9
(0..=8), bishop 14 (0..=13), rook 15 (0..=14), queen 28 (0..=27); 66 entries per phase.

Starting values are CPW's formula written out, so the evaluation starts equal to CPW's and a tuner
is free to bend the curve:

| Piece  | mg weight | eg weight | baseline |
|--------|-----------|-----------|----------|
| Knight | 4         | 4         | 4        |
| Bishop | 3         | 3         | 7        |
| Rook   | 2         | 4         | 7        |
| Queen  | 1         | 2         | 14       |

e.g. knight mg and eg: -16, -12, -8, -4, 0, 4, 8, 12, 16. The baselines are folded into the
values and do not exist as constants.

## Layout

- `src/cpw.rs`: `KNIGHT_MOBILITY_MG`/`_EG`, `BISHOP_MOBILITY_MG`/`_EG`, `ROOK_MOBILITY_MG`/`_EG`,
  `QUEEN_MOBILITY_MG`/`_EG`, written out as literal arrays (readable against the formula, and what
  a tuner will overwrite), with a doc comment giving the formula they come from.
- `src/eval.rs`: a `mobility` function returning `color`'s (mg, eg), added before the taper like
  pawn structure. `mob` is `(attacks & !own_occupancy).count()`, with the attack sets from
  `movegen` (`knight_attacks`, `bishop_attacks`, `rook_attacks`, `queen_attacks`) against the full
  occupancy. The module doc gains a sentence on mobility.

## Tests

- Starting position still 0; color mirrors still equal (the existing pairs cover pieces off their
  start squares).
- Mobility counts, pinned: a knight on d4 on an otherwise empty board (kings far away) counts 8, on
  a1 counts 2; a rook on a1 behind its own pawn on a2 counts only the rank; a bishop whose diagonal
  holds an enemy piece counts that square and stops.
- The tables equal CPW's formula entry by entry (a test computes `weight * (m - baseline)` and
  compares), and have the sizes above.
- The score: a lone White knight on d4 contributes 16 in mg and eg. The existing king-and-knight
  test changes on purpose, from 307 to `(366 + 322 * 23) / 24 = 323`.

## Verification

- `cargo test`, and `cargo test --features training`.
- No new warnings.
