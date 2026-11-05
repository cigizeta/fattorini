# Step 2 of the textbook evaluation: pawn structure

Status: done. Applied; tests pass. Part of textbook-evaluation.md (step 2 of 4). Branch `alessio` only,
like step 1 (pesto-evaluation.md).

## Goal

Add CPW-Engine's pawn structure to `eval.rs`, without its protected-passer table and without its
pawn hash (an optimization, for later). Every value is duplicated into an mg and an eg version,
both starting from CPW's single value, so the evaluation is unchanged in meaning until a tuner
separates them. No search changes.

## Source

CPW-Engine on the Chess Programming Wiki (CC BY-SA 3.0): `EvalPawn` on the "CPW-Engine_eval" page,
`weak_pawn_pcsq` and `passed_pawn_pcsq` on the "CPW-Engine_eval_init" page, both read from saved
copies of the pages.

## The rules, per pawn (CPW's `EvalPawn`)

Directions are the pawn's own: "ahead" is towards the rank it promotes on.

- **Doubled**: `DOUBLED` for each friendly pawn ahead of it on its own file. Two pawns on a file:
  the rear one pays once. Three: the rear pays twice, the middle once.
- **Passed**: no pawn of either color ahead of it on its own file, and no enemy pawn ahead of it on
  an adjacent file. Scores `PASSED[square]`.
- **Weak**: no friendly pawn on an adjacent file on its own rank or behind it. Scores
  `WEAK[square]`, plus `WEAK_UNOPPOSED` when no enemy pawn is ahead of it on its own file.

A pawn can be doubled, passed and weak at once, or any combination: the three are independent.

## Values (CPW), duplicated into mg and eg

- `DOUBLED_MG = DOUBLED_EG = -20`.
- `WEAK_UNOPPOSED_MG = WEAK_UNOPPOSED_EG = -4`.
- `WEAK_MG = WEAK_EG`: CPW's `weak_pawn_pcsq`, 64 entries, a8 first as printed. By file
  -10, -12, -14, -16, -16, -14, -12, -10 on ranks 7 to 2, except -8 on a2 and a3; 0 on ranks 8
  and 1.
- `PASSED_MG = PASSED_EG`: CPW's `passed_pawn_pcsq`, 64 entries, a8 first as printed. By rank:
  100 (rank 7), 80, 60, 40, 20, 20 (rank 2); 0 on ranks 8 and 1.

Per-square tables stay per square, even though CPW's vary only by file (weak) or only by rank
(passed): the tuner may learn more.

## Layout

- `src/cpw.rs` (new, `pub mod cpw;` in `lib.rs`): the parameters above and nothing else, next to
  `pesto.rs`. Module doc cites the source pages and the license.
- `src/eval.rs`: a `pawn_structure` function returning the (mg, eg) pair from White's side, added
  to the PeSTO terms before the taper. The tables use the same `table_index` flip as PeSTO's
  (printed a8 first: White reads `square ^ 56`, Black `square`).
- Each rule is a small function over the two pawn bitboards, in the same direct style as CPW's
  code: at most 16 pawns, so plain scans, no new bitboard machinery.
- The module doc of `eval.rs` gains a paragraph on pawn structure.

## Tests

In `eval.rs`:

- The starting position still evaluates to 0; color mirrors still evaluate the same (the existing
  mirror test, plus a pawn-heavy pair).
- Doubled: two pawns on a file pay once, three pay three times.
- Passed: a pawn with no pawns ahead on its file or adjacent files is passed; an enemy pawn ahead on
  an adjacent file makes it not passed; the rear pawn of a doubled pair is not passed.
- Weak: a pawn with a friendly pawn beside it or behind it on an adjacent file is not weak; an
  isolated pawn is weak; a pawn whose only neighbors are ahead of it (backward) is weak.
- Unopposed: a weak pawn with an enemy pawn ahead on its file gets no extra penalty; without one it
  does.
- Orientation, pinned against CPW's tables: a White passed pawn on e5 and a Black one on e4 each
  score 60; a weak White pawn on d3 and a Black one on d6 each score -16.
- A whole-evaluation value computed by hand for one small pawn position.

## Verification

- `cargo test`, and `cargo test --features training`.
- No new warnings.
