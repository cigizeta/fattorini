# Pawn structure

## Design

Three checks, chosen because each is a plain static file/rank comparison to detect (unlike
backward, which needs relational "does an enemy pawn control this square" reasoning --
deliberately deferred, see `TODO.md`): isolated, doubled, passed.

### Definitions

- **Isolated** (per pawn): no friendly pawn on either adjacent file, regardless of rank.
- **Doubled** (per file, not per pawn): if a color has two or more pawns on the same file,
  apply one flat penalty for that file. Deliberately simpler than Stockfish's own version
  (which checks each pawn individually against whether it's otherwise diagonally supported,
  and can end up applying the penalty to some but not all of a file's extra pawns) -- three
  pawns stacked on a file is exactly as penalized as two, once per file, full stop.
- **Passed** (per pawn): no enemy pawn on the same file or either adjacent file, anywhere
  ahead of it (from that pawn's own side's perspective).
- **Backward** (per pawn, design settled 2026-09-14, not yet implemented): mirrors Stockfish's
  own real formula exactly (`pawns.cpp`, same pinned commit as below), fetched directly rather
  than paraphrased from memory --

  ```
  blocked    = theirPawns & (s + Up);                       // enemy pawn sits on the stop square
  leverPush  = theirPawns & pawn_attacks_bb(Us, s + Up);     // enemy pawn attacks the stop square
  backward   = !(neighbours & forward_ranks_bb(Them, s+Up)) && (leverPush | blocked);
  ```

  Translated into Rotor's own plain rank/file comparisons rather than new bitboard mask
  primitives (`forward_ranks_bb` has no Rotor equivalent and doesn't need one -- see "No new
  bitboard infrastructure needed" below): a pawn is backward when (1) it has at least one
  friendly pawn on an adjacent file (otherwise it's isolated, not backward -- see the
  mutual-exclusion note below), but none of those neighbors sit at its own rank or behind it
  (the direct rank comparison `forward_ranks_bb` is computing), so no friendly pawn could ever
  advance to defend it, AND (2) its own stop square (the square directly ahead) either already
  has an enemy pawn sitting on it (`blocked`) or is attacked by an enemy pawn (`leverPush`) --
  a plain `pawn_attacks`-based check, the same primitive already imported in `eval.rs`. Note
  `blocked` specifically means an *enemy pawn* occupies the stop square, not any piece --
  backward is a pure pawn-vs-pawn relationship on both halves of the condition.

  **Mutually exclusive with isolated**, confirmed directly from Stockfish's own `pawns.cpp`
  (same pinned commit as below) rather than assumed: its scoring is an `if (support | phalanx)
  ... else if (!neighbours) { Isolated } else if (backward) { Backward }` chain, not
  independent flags -- a pawn with zero neighbours takes the isolated branch and never even
  reaches the backward check. Rotor's own `is_backward` should require "has a neighbor" as an
  explicit precondition to match this, rather than let the two penalties stack on the same
  pawn the way naively combining their two definitions in isolation would allow.

### Source and magnitudes

Stockfish's own classical (pre-NNUE) evaluation, pinned at a specific commit so the citation
is stable:

```
// pawns.cpp
constexpr Score Isolated = S(5, 15);
constexpr Score Doubled  = S(11, 56);

// evaluate.cpp
constexpr Score PassedRank[RANK_NB] = {
    S(0, 0), S(10, 28), S(17, 33), S(15, 41), S(62, 72), S(168, 177), S(276, 260)
};
```
(`official-stockfish/Stockfish`, commit `804a29c738847b7ea5f8a4bff001964bd234d332`.)

`S(mg, eg)` is Stockfish's tapered-score pair: the same feature is worth a different amount
depending on how much of the game is left, blended continuously via a game-phase estimate
Rotor doesn't compute (full tapering deliberately deferred, see `piece-square-tables.md`).
Rather than either inventing flat numbers or building continuous blending, these values are
used exactly as published, gated by the `is_endgame` predicate already built for the king's
own table switch: `is_endgame` true selects each value's `eg` component, false selects `mg`.
Same binary-switch shape already established, no new phase machinery.

`PassedRank` is indexed by rank 1 through rank 7 (rank 1 is always `0`, unreachable for a
pawn anyway) -- for a pawn belonging to `color`, the relevant index is how many ranks it's
advanced from its own second rank, mirrored for Black the same way PST's own `square ^ 56`
already is.

All three constants are stored as plain positive magnitudes and applied with the sign
decided at the call site (`-=` for Isolated/Doubled, `+=` for the passed-pawn bonus) --
Stockfish's own convention (confirmed by reading `pawns.cpp` directly: `score -= Isolated`,
`score -= Doubled`), and consistent with how `evaluate()` already accumulates everything
additively, deciding sign only once, right at the very end.

**Superseded for Isolated, 2026-09-14 -- see "Founding Rotor's own constants" below.** This
section's numbers were landed under the "use exactly as published" reasoning stated above,
before this project had articulated the objection later raised against Material's real values
(term 1) and Imbalance's real coefficients (term 2): these are fishtest self-play output,
jointly tuned against Stockfish's *own* other terms, not an independent chess fact Rotor can
actually cite. That objection applies here just as much -- `Isolated`/`Doubled`/`PassedRank`
were tested for bugs (does the code do what it says), never for whether `5`/`15`/`11`/`56`/the
`PassedRank` curve are remotely right for an engine with Rotor's own piece values and search.
Isolated's, Doubled's, and Passed's numbers are all resolved below; Backward's own magnitude
is the one still open (see "Not yet settled").

### Founding Rotor's own constants: Stockfish's own ratios, not its absolute numbers

Neither of the two honest options already used elsewhere in this codebase quite fit here.
Zero-seeding (`Imbalance`'s resolution for its 20 unproven pairs) is right when there's
*nothing* to reason from, but isolated pawns already have a working, shipped penalty --
zeroing it would be a real playing-strength regression with nothing to replace it until
TDLeaf(lambda) exists (`eval-tuning.md`), which is itself deliberately sequenced well out,
behind the search-technique work. A hand-picked placeholder (the `ASPIRATION_INITIAL_DELTA`/
king-safety style) is honest about being a guess, but a guess pulled from nowhere is no more
chess-principled than the Stockfish number it would replace.

The resolution: Stockfish's own pawn is worth `136` in the middlegame, `208` in the endgame
(`sf_10`, `types.h` -- the real numbers `piece_value_mg`/`_eg`'s own doc comment already cites
and deliberately didn't adopt). Its isolated-pawn penalty, `S(5, 15)`, is therefore `5/136 ≈
3.7%` of a pawn's own value in the middlegame and `15/208 ≈ 7.2%` in the endgame. Those
*ratios* -- not the absolute centipawn numbers -- are the part of Stockfish's fishtest result
plausibly still true regardless of the surrounding engine: "an isolated pawn costs a small
fraction of a pawn's worth of value in the middlegame, meaningfully more in the endgame" is
independent chess judgment (Nimzowitsch's "isolani" discussion and every strategy text since),
not an artifact of Stockfish's own specific tuning. Rescaling those ratios onto Rotor's own
pawn value (`100`, both phases, per `piece_value`) gives `100 * 5/136 ≈ 4` and
`100 * 15/208 ≈ 7`.

`ISOLATED_PENALTY_MG`/`_EG` are set to **`(5, 10)`**, close to but not exactly that computed
`(4, 7)` -- deliberately rounded to keep the numbers simple, since this is still an unmeasured
placeholder either way (TDLeaf(lambda) is what eventually gives it a real value), not a
figure precise to the centipawn. What matters is preserving the *shape* Stockfish's own data
independently supports (small in the middlegame, roughly double in the endgame) while
anchoring the absolute scale to Rotor's own pawn value instead of borrowing Stockfish's.

Same treatment for Doubled: Stockfish's `S(11, 56)` is `11/136 ≈ 8.1%` of a pawn in the
middlegame, `56/208 ≈ 26.9%` in the endgame -- a much steeper mg-to-eg jump than isolated's
(roughly 3.3x here vs. roughly 2x there), matching genuine chess judgment independent of
Stockfish's specific tuning: doubled pawns are a mild blemish with pieces still on the board,
but a serious liability in king-and-pawn endgames specifically, where a single enemy piece can
blockade both pawns on the same file and neither can ever defend the other. Rescaled onto
Rotor's pawn (`100`): `100 * 11/136 ≈ 8` middlegame, `100 * 56/208 ≈ 27` endgame.

`DOUBLED_PENALTY_MG`/`_EG` are set to **`(10, 30)`** -- rounder than the computed `(8, 27)`,
same "don't hand-pick precision that isn't real" reasoning as Isolated's `(5, 10)`: trying to
land on an exact centipawn by hand doesn't make an unmeasured placeholder any more correct,
just harder to read.

Same treatment for `PassedRank`, all 6 real entries (rank 1 stays `0`, unreachable for a
pawn), rounded to the nearest 5 below ~50 and the nearest 10 above it:

| rank | Stockfish `S(mg,eg)` | Rotor, rescaled and rounded |
|---|---|---|
| 2 | (10, 28) | (5, 15) |
| 3 | (17, 33) | (15, 15) |
| 4 | (15, 41) | (10, 20) |
| 5 | (62, 72) | (45, 35) |
| 6 | (168, 177) | (120, 90) |
| 7 | (276, 260) | (200, 130) |

Rank 5's rescaled mg (45) lands above eg (35), reversing Stockfish's own absolute ordering
(62 < 72) -- an artifact of Stockfish's own pawn value stretching more from mg to eg (136 to
208) than Rotor's flat 100 does, not a mistake. Kept as computed rather than straightened out
by hand: per the developer, none of this curve is meant to be independently chess-justified
rank by rank -- it's a mechanically-rescaled seed for TDLeaf(lambda) to actually correct, not
a claim that rank 5 genuinely matters more in the middlegame than the endgame.

Same treatment for Backward: Stockfish's `Backward = S(9, 24)` (fetched directly from
`pawns.cpp`, not recalled) is `9/136 ≈ 6.6%` of its own pawn in the middlegame, `24/208 ≈
11.5%` in the endgame -- rescaled onto Rotor's pawn (`100`), `≈6.6`/`≈11.5`, rounded to the
nearest 5/10 as **`(5, 10)`**. Coincidentally identical to Isolated's own resolved `(5, 10)`
(Isolated computed to `(4, 7)` before rounding, Backward to `(6.6, 11.5)`; both land on the
same rounded pair) -- approved as-is rather than nudged apart for visual distinctness, since
neither is meant to be more than an unmeasured TDLeaf(lambda) seed.

### No new bitboard infrastructure needed

Isolated and doubled only need "how many of this color's pawns are on each file" -- a plain
`[u32; 8]` count built by iterating the color's pawn bitboard once and indexing by
`square.file() as usize`, not a new bitboard file-mask primitive. Passed only needs per-pawn
comparisons against the (small) set of enemy pawns on the same and adjacent files, ahead of
it -- direct square/file/rank comparisons, the same shape `Square`'s existing methods already
support, not a new primitive either. Backward needs one more building block beyond those: a
"does an enemy pawn attack this square" check for the stop square, via `pawn_attacks` (already
imported in `eval.rs` for other terms) -- everything else in its formula (the neighbour/rank
comparison, the blocked check) is the same direct square/file/rank comparison shape as the
other three.

### OCB halving: no carve-out, resolved

All three checks are halved the same as material and PST in a genuine opposite-colored-bishop
ending -- no exception for passed pawns, despite an earlier, plausible-sounding argument that
they deserved one (they're the main winning mechanism in those endings, so halving them
alongside inert material seemed like it would undervalue exactly what matters). That argument
didn't survive checking the actual reference implementation: Stockfish's own classical
evaluation computes its entire score first -- material, pawns and passed pawns among
everything else -- and only afterward determines its OCB-aware scale factor from material
composition, applied uniformly to the finished total at the very last step. Nothing is carved
out there either. Rotor now follows the same order, and `eval.rs`'s own
`scaled_material_and_positional_value` doc comment (which used to argue the opposite) has been
corrected to match.

## Not yet settled

- None of the four resolved rescalings (Isolated, Doubled, Passed, Backward) are applied to
  `eval.rs` yet -- still `(5, 15)`, `(11, 56)`, Stockfish's original `PassedRank`, and no
  `Backward` term at all there as of this entry.
- Exact function/module shape for `is_backward` (mirroring `eval.rs`'s existing
  `has_bishop_pair`-style predicates, presumably, but not yet worked out) -- the formula itself
  is settled above; only its concrete Rust shape (helper function boundaries, what it returns,
  how the stop-square-attacked check calls `pawn_attacks`) remains.

Implementation order isn't a design decision -- isolated and doubled are simply simpler than
passed (no rank table, no mg/eg selection), so that's the likely sequence, settled as a matter
of course rather than something needing its own discussion.

## Progress log

2026-09-12: Design discussion. Landed: three checks (isolated, doubled, passed; backward
deferred, recorded in `TODO.md`); doubled defined as a flat per-file penalty, not per-pawn
or support-gated, simpler than Stockfish's own version; magnitudes taken directly from
Stockfish's own source (`Isolated = S(5, 15)`, `Doubled = S(11, 56)`,
`PassedRank = {S(0,0), S(10,28), S(17,33), S(15,41), S(62,72), S(168,177), S(276,260)}`),
pinned at commit `804a29c738847b7ea5f8a4bff001964bd234d332`; phase handling via the existing
`is_endgame` predicate picking `mg` or `eg` per value, not new tapering; positive-magnitude-
plus-call-site-sign convention, matching Stockfish's own. Nothing implemented yet.

2026-09-12 (continued): Resolved the OCB-halving carve-out question by checking Stockfish's
actual evaluate() order directly rather than reasoning from theory alone -- it computes its
whole score first (pawns and passed pawns included), then determines its own OCB scale factor
from material composition afterward, applying it uniformly to the finished total with nothing
excluded. Rotor follows the same order: no carve-out for passed pawns, or anything else.
Corrected `eval.rs`'s `scaled_material_and_positional_value` doc comment, which had argued the
opposite before this was checked, and `piece-square-tables.md`'s own matching stale note.
Still nothing implemented -- this closes the last open design question before writing code.

2026-09-12 (continued): Landed isolated pawns, the first of the three checks. `pawn_file_counts`
(a plain `[u32; 8]` tally, shared groundwork the doubled check will reuse too), `is_isolated`
(both neighboring files empty, board edges treated as an empty neighbor rather than a special
case), `ISOLATED_PENALTY_MG`/`_EG` (Stockfish's own `5`/`15`), wired into `material_value`
picking the penalty via `is_endgame`. Tests: `pawn_file_counts` directly, `is_isolated` (true,
false, both edge files), plus two `evaluate` integration tests (an isolated pawn penalized, the
same pawn unpenalized once a neighbor supports it). Next: doubled pawns.

2026-09-12 (continued): `cargo test` surfaced 6 failures, all stale hardcoded scores -- the
same pattern as PST's own landing. Five share one cause: the a7 pawn in `search.rs`/`uci.rs`'s
shared "wins more material" test position becomes Black's only, unsupported pawn once Nxc5
removes c5, so it's isolated -- no queens on the board there means the endgame penalty (15)
applies, raising the White-relative balance by that much (230 -> 245 throughout). The sixth
(`evaluate_does_not_halve_outside_the_endgame_even_with_opposite_bishops`) is a genuine
coincidence: the a2 pawn's own PST bonus (+5) and its isolation penalty (-5, middlegame
magnitude there) canceled exactly, landing back on the pure-material number (-400) by accident,
not for any structural reason. All six recomputed by hand and fixed, not pasted from the
observed failure output blind. 259 passed (6 new tests), 1 ignored, 0 failed.

2026-09-12 (continued): Landed passed pawns, the last of the three checks. `PASSED_PAWN_BONUS_MG`/
`_EG` (Stockfish's own `PassedRank` table, all 7 entries), `passed_pawn_rank_index` (White reads
absolute rank directly; Black mirrors, the same rank-flip idea used everywhere else in this
file, applied to just the rank component), `is_passed` (a direct per-pawn scan against the
enemy pawns, not a bitboard mask -- no new infrastructure needed, matching the design doc),
wired into `material_value`. Tests: `passed_pawn_rank_index` (White direct, Black mirrored),
`is_passed` (unopposed, blocked same-file, blocked adjacent-file, enemy behind not ahead, enemy
on a non-adjacent file, and the same logic reversed for Black), plus three `evaluate`
integration tests (bonus alone, bonus without isolation once supported, no bonus when blocked).

Landing this immediately meant re-deriving every existing isolated/doubled test that happened
to use a lone or unsupported pawn with no enemy pawns anywhere -- every one of those pawns
turned out to also be passed, which nothing had accounted for yet. Caught and fixed six of
`eval.rs`'s own tests plus the three shared `search.rs`/`uci.rs` tests, all recomputed by hand,
one arithmetic slip caught and corrected mid-fix (mixed up which direction a bonus added to the
*opponent's* total moves the side-to-move-relative balance -- a penalty subtracted from the
opponent raises the balance, a bonus added to the opponent lowers it, the opposite of what
applies to the pawn's own side). A broader sweep for any other affected position (an
`Explore` agent searching `search.rs`/`uci.rs` for any other pawn-containing FEN paired with
a hardcoded evaluate()-derived score) found nothing further -- every other candidate asserted
either a mate/draw sentinel, or something unrelated to a literal score (moves, nodes, PV,
seldepth, booleans, a live comparison against `evaluate(&board)`). `cargo test` confirms 274
passed (263 plus these 11 new tests), 1 ignored, 0 failed.

All three checks (isolated, doubled, passed) are now landed and verified. Backward pawns
remain deferred, recorded in `TODO.md`. This closes the pawn-structure work.

2026-09-12 (continued): Landed doubled pawns, the second of the three checks.
`DOUBLED_PENALTY_MG`/`_EG` (Stockfish's own `11`/`56`), `is_doubled_file` (`pawn_count >= 2`,
deliberately not scaled by how many pawns beyond the first, and not support-gated the way
Stockfish's own version is), wired into `material_value` iterating the same `pawn_file_counts`
the isolated check already built. Tests: `is_doubled_file` directly (below two, at or above
two), plus two `evaluate` integration tests -- one confirming the penalty, one confirming it
stays a single flat penalty with three pawns stacked on a file rather than scaling with the
extra count. 263 passed (4 new tests), 1 ignored, 0 failed -- clean this time, no ripple effect
on any other hardcoded score (no other test position happens to have two of the same color's
pawns sharing a file). Next: passed pawns, the last of the three.

2026-09-14: Reopened (moved back from `claude/done/`) to add backward pawn detection and to
revisit the Isolated/Doubled/Passed magnitudes -- the developer's own objection: those numbers
"were tested for bugs, but not tested for chess principles," the same critique already applied
to Material's real values (term 1, `stockfish-eval-terms.md`) and Imbalance's real coefficients
(term 2), just not yet extended to pawn structure when it landed.

Checked Stockfish's actual `pawns.cpp` (same pinned commit) directly rather than assuming: its
`backward`/`Isolated` scoring is an `if/else if` chain (`support|phalanx` branch, then
`!neighbours` branch, then `backward` branch), so a pawn is isolated *or* backward *or*
neither, never both -- confirms the mutual-exclusion design recorded above.

Worked out a methodology for founding Rotor's own magnitudes without either copying
Stockfish's absolute numbers or zero-seeding a term that already works: rescale Stockfish's
own penalty as a *fraction of its own pawn value* (`136`/`208` mg/eg, `sf_10`) onto Rotor's own
pawn value (`100`) instead of copying the absolute centipawn figure. For Isolated: `5/136 ≈
3.7%` and `15/208 ≈ 7.2%` of a pawn, computed onto Rotor's scale as `≈4`/`≈7`, settled by the
developer at `(5, 10)` -- close to the computed figures, deliberately rounded since this is
still an unmeasured placeholder either way. Full reasoning and the resulting section rewrite
recorded above under "Founding Rotor's own constants." Not yet applied to `eval.rs` -- next
step is landing `(5, 10)` for `ISOLATED_PENALTY_MG`/`_EG`, then working through Doubled,
Passed, and Backward's own magnitudes the same way, one at a time, per the developer's own
framing.

2026-09-14 (continued): Doubled, worked through the same way. Stockfish's `S(11, 56)` is
`11/136 ≈ 8.1%`/`56/208 ≈ 26.9%` of its own pawn -- rescaled onto Rotor's pawn (`100`), `≈8`/
`≈27`. Settled by the developer at `(10, 30)`, rounder than the computed figures on the
explicit reasoning that hand-picking exact centipawns doesn't make an unmeasured placeholder
any more correct, just harder to read -- the same rounding call as Isolated's `(5, 10)`. Full
reasoning recorded above. Not yet applied to `eval.rs`. Next: Passed's `PassedRank` curve, then
Backward's own magnitude.

2026-09-14 (continued): Passed, the last of the three already-landed checks. Same rescaling
across all 6 real `PassedRank` entries at once (table recorded above), rounded to the nearest
5 or 10 per the developer's own instruction. Approved as computed, rank-5 mg/eg inversion
included -- per the developer, this whole exercise is "just the seed for tdleaf," a mechanical
rescaling rather than a claim that each rank's relative mg/eg split is independently
chess-justified. Isolated, Doubled, and Passed are now all resolved on paper; only Backward's
own magnitude remains open. Nothing yet applied to `eval.rs` -- next step is landing all three
rescaled constant sets there in one pass, then returning to Backward's detection logic and its
own magnitude.

2026-09-14 (continued): Confirmed the two-denominator rescaling itself was already being done
correctly (developer's own check, prompted by the rank-5 inversion looking surprising): every
ratio divided its mg component by Stockfish's own mg pawn value (`136`) and its eg component
by Stockfish's own eg pawn value (`208`) separately, never a single shared denominator -- the
rank-5 inversion is direct evidence of that (it can only appear because `208 != 136`), not a
sign the math was wrong. Nothing to revise.

2026-09-14 (continued): Backward, the last term. Fetched Stockfish's actual `Backward`
constant and its exact surrounding formula directly from `pawns.cpp` (same pinned commit)
rather than continuing from memory -- `Backward = S(9, 24)`; `blocked` specifically means an
*enemy pawn* on the stop square, not any piece; `leverPush` is the stop square being attacked
by an enemy pawn. Rescaled the same way as the other three: `9/136 ≈ 6.6%`/`24/208 ≈ 11.5%` of
a pawn, onto Rotor's `100`, rounded to `(5, 10)` -- coincidentally identical to Isolated's own
resolved value, approved as-is by the developer rather than nudged apart.

Per the developer's explicit instruction, `is_backward`'s design (recorded above under
"Definitions") now mirrors Stockfish's real formula structurally -- the same `neighbours`/
`forward_ranks`/`blocked`/`leverPush` shape, translated into Rotor's own direct rank/file
comparisons and a `pawn_attacks` lookup rather than new bitboard mask primitives -- instead of
the looser independently-reasoned paraphrase from the original 2026-09-14 discussion.

All four terms (Isolated, Doubled, Passed, Backward) are now fully resolved on paper: formula
and magnitude both settled for Backward, magnitude-only revisions settled for the other three.
Nothing yet applied to `eval.rs`. Next: land all four in one pass -- the three constant
rescalings plus the new `is_backward` predicate and its own penalty, wired into
`material_value` the same way the other three already are.

2026-09-14 (continued): Landed all four in `eval.rs`. `ISOLATED_PENALTY_MG`/`_EG` -> `(5, 10)`,
`DOUBLED_PENALTY_MG`/`_EG` -> `(10, 30)`, `PASSED_PAWN_BONUS_MG`/`_EG` -> the rescaled 7-entry
tables, all per the derivations recorded above -- doc comments on each constant rewritten to
cite the rescaling instead of Stockfish's own numbers directly. `BACKWARD_PENALTY_MG`/`_EG`
(`5, 10`) and `is_backward` added (mirrors Stockfish's real `neighbours`/`forward_ranks`/
`blocked`/`leverPush` formula via a direct per-pawn scan, matching `is_passed`'s own style, plus
a `pawn_attacks`-based stop-square-attacked check reusing the exact trick `movegen/attacks.rs`'s
`is_square_attacked` already uses for pawns), wired into `material_value` as its own loop
alongside isolated/doubled/passed.

Six new unit tests on `is_backward` directly (true via an attacked stop square, true via a
blocked one, false with a supporting neighbor, false with a safe stop square, false with no
neighbor at all -- confirming the isolated/backward mutual exclusion at the unit level too, not
just by reading Stockfish's source -- and the same direction reversed for Black), plus one
`evaluate`-level integration test. The integration test took real care to isolate the term
exactly rather than accept a reinforcing-but-uncancelled confound the way some earlier tests in
this file do: the second White pawn sits on f5 (backward scenario) or f2 (supported scenario),
two squares `PAWN_PST` happens to value identically (`10`) despite the rank difference, with
Black's f6 blocking passed status for every White pawn in both positions alike -- so the only
difference between the two positions is `e4`'s own backward penalty, and `evaluate(backward) <
evaluate(supported)` follows by exactly `BACKWARD_PENALTY_EG`, not by an accident of PST or
passed-bonus asymmetry.

Checked the existing test suite by hand for ripple effects before handing off to the developer
for `cargo test` (no Bash access in this project): every pre-existing test with both colors'
pawns on adjacent files either has no enemy pawns at all (backward can never fire without one)
or no same-color neighbor pawn at all (backward can never fire without one either), so none of
them can newly trigger the added `Backward` term. The one pre-existing test with an exact
hardcoded score (`evaluate_does_not_add_a_passed_pawn_bonus_when_blocked`, expects `20`) is
unaffected by the Isolated/Doubled/Passed rescaling for a structural reason, not luck: both
sides' lone pawns are isolated by the same amount, and a symmetric penalty cancels out of a
White-minus-black difference regardless of its magnitude. Not yet confirmed by an actual
`cargo test` run.
