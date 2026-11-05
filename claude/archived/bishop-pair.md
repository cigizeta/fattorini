# Bishop pair

**Archived:** implemented as a big five term, deleted when the big five was retired 2026-10-03. The hand-crafted evaluation is PeSTO only; see textbook-evaluation.md.

Status: done. Applied; tests pass. A sixth term beyond textbook-evaluation.md's big five, CPW-Engine's first
material adjustment. Branch `alessio` only.

## The rule

CPW-Engine: `if (b.PieceCount[WHITE][BISHOP] > 1) result += e.BISHOP_PAIR;`, `BISHOP_PAIR = 30`,
untapered. Here, as for pawn structure, duplicated into an mg and an eg value starting from CPW's:

- `cpw::BISHOP_PAIR_MG = 30`, `cpw::BISHOP_PAIR_EG = 30`.
- A side with at least 2 bishops gets it once. Square colors are not checked (CPW does not), so
  two same-colored bishops after an underpromotion also count; rare enough to ignore.

## Layout

- `src/cpw.rs`: the two constants.
- `src/eval.rs`: `bishop_pair(board, color) -> (i32, i32)`, added in `tapered_terms`. Module doc
  gains a sentence.

## Tests

- `bishop_pair`: 2 bishops give (30, 30), 1 gives (0, 0), 3 give (30, 30).
- Starting position still 0; color mirrors still equal.

## Verification

- `cargo test`, and `cargo test --features training`.
