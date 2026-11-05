# Opposite-colored bishops

**Archived:** implemented as a big five term, deleted when the big five was retired 2026-10-03. The hand-crafted evaluation is PeSTO only; see textbook-evaluation.md.

Status: done. Applied; tests pass. Beyond textbook-evaluation.md's big five. Branch `alessio` only.

## The rule

The pure case only, Stockfish 11's first case (`opposite_bishops() && non_pawn_material() == 2 *
BishopValueMg`): each side has exactly one bishop, the two on squares of opposite colors, and no
knight, rook or queen on the board. Then the whole evaluation is divided by 2 (Stockfish scales the
eg score by 22/64; in this material the phase is 2 of 24, so the eval is almost all eg anyway, and
1/2 is the textbook number). Fixed, not tuned: a multiplicative factor.

Not carried over: Stockfish's milder scaling with other pieces on the board (where opposite
bishops favor the attacker), its few-pawns scaling, its 50-move decay.

## Layout

- `src/eval.rs`: `is_pure_opposite_bishops(board)`, and `evaluate` halves the White-relative score
  when it holds. The module doc gains a sentence.

## Tests

- Detection: bishops c1 / c8 (dark / light) with pawns is pure; same-colored bishops is not; an
  extra knight is not; two bishops for one side is not.
- The evaluation of a pure position is half the unscaled one.
- Existing tests unchanged (none is a pure opposite-bishop position).

## Verification

- `cargo test`, and `cargo test --features training`.
