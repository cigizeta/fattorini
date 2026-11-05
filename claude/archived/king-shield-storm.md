# Step 4b of the textbook evaluation: shield and storm as zones and zeroed tables

**Archived:** implemented as a big five term, deleted when the big five was retired 2026-10-03. The hand-crafted evaluation is PeSTO only; see textbook-evaluation.md.

Status: done. Applied; tests pass. Part of textbook-evaluation.md (step 4 of 4; the king attack comes
later). Supersedes the values of cpw-king-shield.md, not its rule. Branch `alessio` only.

## Goal

Rewrite the pawn shield into the four-table form decided in textbook-evaluation.md, and add the
pawn storm in the same form. Both tunable tables start at zero, so the evaluation scores exactly
what it scored before the shield was added. No search changes.

## Tables

Fixed geometry, not tunable, in `eval.rs`:

- `SHIELD_ZONE: [[Bitboard; 64]; 2]` by king color and square: the three files around the king
  (center clamped to b..g), one and two ranks ahead in the color's direction, minus ranks 1 and 8.
- `STORM_ZONE: [[Bitboard; 64]; 2]`: the same, four ranks ahead.
- Both built by one `compute_zone(color, king, ranks_ahead)`, filled into `LazyLock` arrays like
  `movegen`'s knight table (`Bitboard` has no const constructor, so not a `const fn`).

Tunable, all zeros, in a new module `src/tuned.rs` (the evaluation's own parameters, as opposed to
`pesto.rs` and `cpw.rs`, which hold published values):

- `SHIELD_MG: [i32; 64]` by the own pawn's square.
- `STORM_MG: [i32; 64]` by the enemy pawn's square.
- Both a8 first, read through the king owner's `table_index`, middlegame only.

`cpw::SHIELD_MG` is removed, and `cpw.rs`'s module doc goes back to pawn structure and mobility.

## Scoring

- `king_safety_at(board, color, king, shield, storm)`: the sum of `shield[table_index(color, sq)]`
  over `color`'s pawns on `SHIELD_ZONE[color][king]`, plus the sum of `storm[table_index(color,
  sq)]` over the enemy's pawns on `STORM_ZONE[color][king]`. The tables are arguments so tests can
  pass their own.
- `king_safety(board, color)`: the max of `king_safety_at` over the king's square and `king_to` of
  each castling right held. The max is over shield plus storm together, one king position judged
  whole.
- Added to `mg` in `tapered_terms`, with `tuned::SHIELD_MG` and `tuned::STORM_MG`.

## Tests

- The zones: `SHIELD_ZONE` for White g1 is f2 g2 h2 f3 g3 h3; a1 is a..c; e1 is d..f; g7 and g8
  are empty; Black g8 is f7 g7 h7 f6 g6 h6. `STORM_ZONE` for White g1 is f..h, ranks 2..5; for
  g6, rank 7 only.
- The scoring, with a test table of 1 on every square: counts the pawns in the zone, a doubled
  pair twice, enemy pawns only in the storm and own pawns only in the shield.
- Castling: with a test table, a king on e1 with the short right scores the max of e1 and g1;
  without the right, e1 only; the queenside right alone does not reach the kingside.
- Middlegame only: with a test table, two positions differing only in a castling right differ
  in mg and not in eg (via `king_safety`, since `tapered_terms` uses the zeroed tables).
- The tuned tables are all zeros, and the evaluation is unchanged: the existing hand-computed
  and mirror tests stand as they are.

## Verification

- `cargo test`, and `cargo test --features training`.
- No new warnings.
