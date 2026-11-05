# Piece-square tables (PST)

**Superseded, see pesto-evaluation.md.** The Michniewski tables, the bishop pair bonus and the single-phase design were replaced by PeSTO's tapered material and tables. Tuning hand-crafted weights is dropped.

## Status

Implemented in `eval.rs`: literal Michniewski PST (single-phase except the king), additive with
`piece_value`, wired into `material_value`. Bishop pair bonus and opposite-colored-bishop (OCB)
halving also landed.

## Design

Tomasz Michniewski's "Simplified Evaluation Function" (Chess Programming Wiki), adopted
literally -- hand-crafted, not statistically tuned (Texel-style tuning deferred to when/if Rotor
replaces hand-written eval with a learned one). Single-phase (no tapering) except the king, which
Michniewski's own function already splits into two whole tables rather than blending
continuously.

**Combination rule**: additive, `piece_value(piece_type) + pst_value(piece_type, color, square)`.
`piece_value` stays untouched, `pub(crate)` for `search.rs`'s move-ordering reuse; PST is an
`eval.rs`-only positional term, never threaded into search.

**Storage/orientation**: each table is a `[i32; 64]` indexed directly by Rotor's own LERF
`Square` numbering (a1=0..h8=63) -- transcribed rank-reversed from the published grid (which
lists rank 8 first). Black's value is read via `table[square ^ 56]` (flips rank bits only,
leaves file bits untouched -- `square = rank * 8 + file` puts them in disjoint bit ranges).
Seven named consts, not one `[[i32; 64]; 6]` array, since the king needs two tables and named
consts let each carry its own source-citing doc comment.

**King**: Michniewski's own stated switching condition -- "both sides have no queens, or every
side with a queen has no other pieces or at most one minor" -- implemented as `is_endgame(board)`,
a hard boolean predicate (not tapering), picking one of two whole tables
(`KING_MIDDLEGAME_PST`/`KING_ENDGAME_PST`).

**Opposite-colored bishops**: reuses the same `is_endgame` predicate (rather than a second,
independently-tuned "endgame" definition) as the gate for halving `evaluate()`'s scaled
material-and-positional total: `is_endgame(board) && exactly one bishop each && opposite-colored
squares`. The halving boundary is a named function (`scaled_material_and_positional_value`), not
applied to the whole `evaluate()` by default -- future terms opt into being halved deliberately
(matches classical Stockfish's own order: compute the full score first, apply OCB scaling
uniformly at the very end, no term carved out). `Square::is_light_square` (`board/square.rs`) is
the shared square-color-parity primitive, also used by `search.rs`'s insufficient-material
detection.

**Bishop pair**: `BISHOP_PAIR_BONUS = 50` (Kaufman's "half a pawn," flat, not tapered), gated on
`has_bishop_pair` (plain `count == 2`, no same/opposite-color check -- a same-colored pair is only
reachable via underpromotion, legal but vanishingly rare). Added inside `material_value`.
Mutually exclusive with the OCB halving condition by construction (one needs exactly one
bishop/side, the other exactly two), so there's no interaction to arbitrate.

## Published values (Chess Programming Wiki, "Simplified Evaluation Function")

Kept here exactly as published (rank 8 first) for reference against the Rust transcription, which
stores these rank-reversed (see Storage/orientation above).

Pawn:
```
 0,  0,  0,  0,  0,  0,  0,  0,
50, 50, 50, 50, 50, 50, 50, 50,
10, 10, 20, 30, 30, 20, 10, 10,
 5,  5, 10, 25, 25, 10,  5,  5,
 0,  0,  0, 20, 20,  0,  0,  0,
 5, -5,-10,  0,  0,-10, -5,  5,
 5, 10, 10,-20,-20, 10, 10,  5,
 0,  0,  0,  0,  0,  0,  0,  0
```

Knight:
```
-50,-40,-30,-30,-30,-30,-40,-50,
-40,-20,  0,  0,  0,  0,-20,-40,
-30,  0, 10, 15, 15, 10,  0,-30,
-30,  5, 15, 20, 20, 15,  5,-30,
-30,  0, 15, 20, 20, 15,  0,-30,
-30,  5, 10, 15, 15, 10,  5,-30,
-40,-20,  0,  5,  5,  0,-20,-40,
-50,-40,-30,-30,-30,-30,-40,-50
```

Bishop:
```
-20,-10,-10,-10,-10,-10,-10,-20,
-10,  0,  0,  0,  0,  0,  0,-10,
-10,  0,  5, 10, 10,  5,  0,-10,
-10,  5,  5, 10, 10,  5,  5,-10,
-10,  0, 10, 10, 10, 10,  0,-10,
-10, 10, 10, 10, 10, 10, 10,-10,
-10,  5,  0,  0,  0,  0,  5,-10,
-20,-10,-10,-10,-10,-10,-10,-20
```

Rook:
```
 0,  0,  0,  0,  0,  0,  0,  0,
 5, 10, 10, 10, 10, 10, 10,  5,
-5,  0,  0,  0,  0,  0,  0, -5,
-5,  0,  0,  0,  0,  0,  0, -5,
-5,  0,  0,  0,  0,  0,  0, -5,
-5,  0,  0,  0,  0,  0,  0, -5,
-5,  0,  0,  0,  0,  0,  0, -5,
 0,  0,  0,  5,  5,  0,  0,  0
```

Queen:
```
-20,-10,-10, -5, -5,-10,-10,-20,
-10,  0,  0,  0,  0,  0,  0,-10,
-10,  0,  5,  5,  5,  5,  0,-10,
 -5,  0,  5,  5,  5,  5,  0, -5,
  0,  0,  5,  5,  5,  5,  0, -5,
-10,  5,  5,  5,  5,  5,  0,-10,
-10,  0,  5,  0,  0,  0,  0,-10,
-20,-10,-10, -5, -5,-10,-10,-20
```

King (middlegame/opening):
```
-30,-40,-40,-50,-50,-40,-40,-30,
-30,-40,-40,-50,-50,-40,-40,-30,
-30,-40,-40,-50,-50,-40,-40,-30,
-30,-40,-40,-50,-50,-40,-40,-30,
-20,-30,-30,-40,-40,-30,-30,-20,
-10,-20,-20,-20,-20,-20,-20,-10,
 20, 20,  0,  0,  0,  0, 20, 20,
 20, 30, 10,  0,  0, 10, 30, 20
```

King (endgame):
```
-50,-40,-30,-20,-20,-30,-40,-50,
-30,-20,-10,  0,  0,-10,-20,-30,
-30,-10, 20, 30, 30, 20,-10,-30,
-30,-10, 30, 40, 40, 30,-10,-30,
-30,-10, 30, 40, 40, 30,-10,-30,
-30,-10, 20, 30, 30, 20,-10,-30,
-30,-30,  0,  0,  0,  0,-30,-30,
-50,-30,-30,-30,-30,-30,-30,-50
```

Bishop pair is named only as a stated goal in the source ("encourage the engine to have the
bishop pair"), never as an implemented bonus -- the only mechanism in the literal published
function is the Bishop > Knight piece-value gap (330 vs. 320), already present in Rotor.
`BISHOP_PAIR_BONUS` above is an original Rotor addition beyond the literal source.

## Open

- Whether `is_endgame`/the OCB predicate belong in `eval.rs` or are general enough to earn their
  own module -- deferred until something else needs them.
