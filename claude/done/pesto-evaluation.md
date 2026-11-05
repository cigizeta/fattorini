# Step 1 of the textbook evaluation: PeSTO material and PST

Status: done. Applied and committed; tests pass. Part of textbook-evaluation.md (step 1 of 4). Work on branch `alessio`
only: `main` is running the `hand-crafted` replay, and this changes what `hand-crafted` means, so
it must not reach `main` before that replay is complete.

## Goal

Rewrite the evaluation from scratch as PeSTO's tapered material and piece-square tables, with
PeSTO's own game phase and PeSTO's native values. The new evaluation replaces `hand-crafted` in
place. Nothing else: no pawn structure, mobility or king safety yet (steps 2 to 4), and no search
changes (margins and benchmarks are looked at only once the evaluation is complete).

## Source

PeSTO (Ronald Friederich; "Piece-Square Tables Only"), as published on the Chess Programming
Wiki's "PeSTO's Evaluation Function" page. The tables are **transcribed from that page, fetched
during implementation**, never typed from memory: 12 tables of 64 entries each is exactly where a
recalled value goes wrong silently. The module doc comment cites the page.

What PeSTO defines, to be confirmed against the page:

- Material, mg / eg: pawn 82 / 94, knight 337 / 281, bishop 365 / 297, rook 477 / 512,
  queen 1025 / 936, king 0 / 0.
- One mg and one eg table per piece type, 12 in all, each added to its piece's material.
- Game phase: knight 1, bishop 1, rook 2, queen 4, pawn and king 0; 24 at the start; capped at 24
  (promotions can exceed it).
- Taper: `(mg * phase + eg * (24 - phase)) / 24`, with `mg` and `eg` each the side to move's sum
  minus the opponent's.
- No tempo bonus, no other term.

## Layout

- `src/eval.rs`: rewritten whole. It holds `evaluate(board) -> i32` (side-to-move relative, the
  contract search and UCI already rely on), the PeSTO phase and taper, and `piece_value`, unchanged
  (100 / 320 / 330 / 500 / 900 / 0: SEE and move ordering use it as a fixed ruler, not as
  evaluation). Every current item of the file goes, dead code included: the sub-networks, the
  Stockfish-derived tables, the pawn predicates, the mobility and king-ring helpers, and their tests.
  Steps 2 to 4 write their own helpers as they need them.
- `src/pesto.rs` (new, `pub mod pesto;` in `lib.rs`): the material values and the 12 tables, and
  nothing else, so steps 2 to 4 and a future tuner find all parameters of step 1 in one place.
- `src/material.rs`: deleted. Its Michniewski tables and `piece_value_mg`/`piece_value_eg` have no
  other user.
- The `material-only-eval` Cargo feature: deleted, with its comment in `Cargo.toml` and the mention
  in `arena/self_play.rs`'s module doc. Self-play stopped using it on 2026-09-22 and nothing builds
  with it.
- `simd::dot_product_avx2`: check it still has a user (`nnue.rs`) once `eval.rs` stops importing it.

## Orientation

PeSTO's tables are printed with a8 first and h1 last. Its code (TSCP's board, a8 = 0) reads them
directly for White and applies `^ 56` for Black. Rotor's squares are LERF (a1 = 0), so the same
effect is: White reads `square ^ 56`, Black reads `square`. The tables are stored in `pesto.rs`
**exactly as printed**, so they can be checked against the page line by line, and the flip lives
in one function in `eval.rs`.

Status of the apply: `pesto.rs` and the new `eval.rs` are written; the tables were copied from the
page's HTML saved locally, and the knight table fetched separately matches it. `rand_distr`, used
only by the old sub-networks, left `Cargo.toml` too.

## Code style

Rotor's conventions: explicit `return`, no em dashes, no `claude/` references in code (the "why"
goes inline). Doc comments short: what a function computes and why PeSTO does it that way, not
derivation histories.

## Tests

In `eval.rs`:

- The starting position evaluates to 0.
- Color symmetry: a position and its color-flipped mirror (board flipped vertically, colors
  swapped, side to move swapped) evaluate the same, for a handful of asymmetric positions.
- Side to move: the same position with the other side to move evaluates to the negation.
- Phase: 24 at the start, 0 with kings and pawns only, capped at 24 with an extra queen.
- Orientation, pinned against the page: a White knight on d4 and a Black knight on d5 each score
  PeSTO's printed d4 entry (the knight tables in mg and eg), and a White pawn on e2 scores the
  printed e2 entry.
- One whole-evaluation check computed by hand from the page's values: a small position (for
  example kings and one knight), so a transcription or taper error shows as a wrong number.

Elsewhere: search and UCI tests compare against `evaluate()` computed live, so they keep passing.
A test that asserts a specific best move or node count may change with the new evaluation; any
such change is looked at one by one, not updated blindly.

## Verification

- `cargo test`, and `cargo test --features training`.
- Rebuild warnings: none new.
- A short match between the new evaluation and the old one is not part of this step: the old one is
  deleted here. Its strength check is the duel after all four steps (textbook-evaluation.md).

## Open

- Whether the finished `hand-crafted` replay on `main` is kept or replayed again with the new
  evaluation: decided when that replay completes, not here.
