# Step 4a of the textbook evaluation: pawn shield

**Archived:** implemented as a big five term, deleted when the big five was retired 2026-10-03. The hand-crafted evaluation is PeSTO only; see textbook-evaluation.md.

Status: done. Applied; tests pass. Part of textbook-evaluation.md (step 4 of 4, shield only; pawn storm and king
attack come later). Branch `alessio` only.

## Goal

Add a pawn shield to `eval.rs`, in the form decided in textbook-evaluation.md: one table by
square, the king's square deciding which squares count, castling as a max. No search changes.

## The rule

- `SHIELD`: 64 entries by the pawn's absolute square, a8 first, read through `table_index` like
  every other table.
- Active squares for a king on `k`: three files, the center being the king's file clamped to b..g,
  on the two ranks one and two ahead of the king, in `color`'s direction. Computed directly with
  `Square::try_offset` (at most 6 squares); a precomputed mask is an optimization, for later.
- `shield(k)` = sum of `SHIELD[sq]` over the own pawns on the active squares. A doubled pair
  counts twice; the doubled penalty handles it.
- The term is the max of `shield` at the king's square and at `king_to` of each castling right
  still held (`CastlingSide::squares`). Only the right is checked.
- Middlegame only: added to `mg`, not to `eg`.

## Starting values, from CPW

CPW-Engine (`wKingShield`, `bKingShield`; `SHIELD_1 = 10`, `SHIELD_2 = 5`) scores a king on files
f..h by pawns on f2/g2/h2 (10) or else f3/g3/h3 (5), a king on files a..c by a2/b2/c2 or a3/b3/c3,
and a king on d or e by nothing. So the table is:

- rank 2: 10 on files a, b, c, f, g, h; 0 on d, e;
- rank 3: 5 on the same files; 0 on d, e;
- all other ranks 0 (ranks 1 and 8 by construction).

This reproduces CPW exactly for a king on a1, b1, g1, h1 with no doubled pawns in front. It
differs where our zone differs from CPW's fixed files: a king on c1 sees b, c, d (CPW: a, b, c),
f1 sees e, f, g (CPW: f, g, h), and any king off its first rank (CPW keeps scoring the first-rank
squares). `P_NO_SHIELD` is unused in CPW and is not carried over.

## Layout

- `src/cpw.rs`: `SHIELD_MG`, a literal 64-entry table, with a doc comment on its CPW origin.
- `src/eval.rs`: `shield_at(board, color, king)` and `king_shield(board, color)`, the latter
  added to `mg` in `tapered_terms`. The module doc gains a paragraph.

## Tests

- Starting position still 0; color mirrors still equal.
- Active squares: a king on h1 counts f2, g2, h2, f3, g3, h3; a pawn on the king's rank or behind
  counts nothing.
- A doubled pair (g2, g3) counts 10 + 5.
- Castling: a king on e1 with the short right and pawns f2, g2, h2 scores 30 (from g1), and 10
  (from e1, only f2 active) once the right is gone.
- Black reads the mirrored squares: a king on g8 with pawns f7, g7, h7 scores 30.
- Only mg: a position's `tapered_terms` eg is unchanged by the shield.

## Verification

- `cargo test`, and `cargo test --features training`.
- No new warnings.
