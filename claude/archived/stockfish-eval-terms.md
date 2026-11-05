# Stockfish classical evaluation terms -- term-by-term review

**Superseded, see evolutionary-training.md and eval-tuning.md.** Every term adopted here was a
placeholder pending AB(lambda) tuning of the hand-crafted weights, which will not happen: decided
with the developer, there is neither the time nor the resources to develop a strong hand-crafted
evaluation, and playing strength comes from the trained network instead. The hand-crafted
evaluation stays material plus piece-square tables only.

## Goal

Work through Stockfish's own classical (pre-NNUE) evaluation term list, one term at a time,
deciding for each whether Rotor should adopt it, and in what simplified/adjusted form. Each
adopted term gets a real, citable source and value. Magnitudes chosen during this pass are
placeholders, same as every hand-picked constant in this codebase, pending AB(lambda)
(`eval-tuning.md`). This pass fixes *structure* -- which features exist at all -- not final
numbers.

## Method

For each term: read Stockfish's actual source at `sf_10` (the pinned reference release), decide
a Rotor-appropriate simplified version given Rotor's own architecture (no pawn hash table, no
pinned-piece detection, continuous mg/eg tapering via `game_phase`/`taper`), land it with its own
tests.

## Terms -- current status

1. **Material** -- done. Piece values stay Michniewski's own flat scale (`piece_value`,
   `eval.rs`), not Stockfish's own mg/eg numbers (those are calibrated jointly with Stockfish's
   other terms via fishtest; importing only the material scale would distort every other term's
   relative weight without retuning). `piece_value_mg`/`piece_value_eg` exist as the eventual
   AB(lambda) targets, seeded from `piece_value`, currently a no-op via `taper`. `piece_value`
   itself stays fixed as the ruler move ordering and `game_phase` depend on.

2. **Imbalance** -- implemented as a learned submodule (`learned-evaluation.md`'s "Concrete
   instance: imbalance submodule"), replacing an earlier hand-crafted pairwise-quadratic design
   (Stockfish's real `QuadraticOurs`/`QuadraticTheirs`, 21 piece-type pairs, 42 coefficients)
   that was landed then abandoned the same session -- the pairwise-only structure is a genuine
   ceiling no amount of tuning fixes (three-or-more-way piece-type interactions are invisible to
   it by construction), not just an unfounded-coefficients problem. Current mechanism: 10
   categorical piece-count features (90 binary inputs), a small MLP, zero-initialized output --
   currently a provable no-op pending training.

3. **Pawns (structure)** -- isolated, doubled, backward, and passed-pawn *detection* are all
   done (`is_isolated`/`is_doubled_file`/`is_backward`, `eval.rs`). Passed-pawn *scoring* is
   tracked separately as term 7. Shelter/storm and king-to-passed-pawn distance are tracked
   elsewhere (the pawn shield/storm submodule; term 7's king-tropism note), not missing from this
   term's own accounting.

   **Genuine, unadopted gap**: Stockfish's `Connected` bonus (`Score Connected[2][2][3][RANK_NB]`,
   indexed by opposed/phalanx/support-count/rank) rewards pawn chains and phalanxes directly --
   distinct from Rotor's four existing checks, which are all "missing a neighbor" or "blocked/
   overtaken" penalties. Not designed yet.

4. **Pieces / Mobility** -- mobility done (`mobility.md`), needs no rethinking (a lookup table
   over one clean scalar, no imposed functional-form ceiling). Ten per-piece-instance sub-bonuses
   Stockfish also computes in the same pass (`Outpost`, `MinorBehindPawn`, `KingProtector`,
   `BishopPawns`, `LongDiagonalBishop`, `CorneredBishop` [Chess960-only, out of scope],
   `RookOnPawn`, `RookOnFile`, `TrappedRook`, `WeakQueen`) are **settled as skipped outright**, not
   deferred: each is a property of one specific piece instance, so a learned-submodule version
   would need one call per qualifying piece rather than the fixed once-per-color shape every
   other submodule uses -- unaffordable given `search.rs`'s cost constraints for the same reason
   king danger itself stays unwired. Revisit only if `search.rs` gains real pruning.

5. **King safety** -- entirely two learned submodules now (`learned-evaluation.md`'s "Concrete
   instance: king danger submodule" and "...pawn shield/storm submodule"; see `king-safety.md`).
   No hand-crafted term of any kind remains. Stockfish's own named sub-terms in this family
   (`QueenSafeCheck`/`RookSafeCheck`/`BishopSafeCheck`/`KnightSafeCheck`, `PawnlessFlank`,
   `CloseEnemies`) are judged largely moot -- the submodules' raw per-square inputs already let a
   trained network discover tropism- and pawnless-flank-shaped facts on their own. Not
   re-verified line-by-line against the submodules' actual input coverage; worth a closer check
   only if training reveals a real gap.

6. **Threats** -- not present at all. Full family: `ThreatByMinor`, `ThreatByRook`,
   `ThreatByRank`, `ThreatByKing`, `Hanging`, `Overload`, `RestrictedPiece`, `WeakUnopposedPawn`,
   `ThreatByPawnPush`, `ThreatBySafePawn`, `KnightOnQueen`/`SliderOnQueen`. Directly motivated by
   a real game loss (an early Qd1-d3 that a `KnightOnQueen`/`SliderOnQueen`-style threat term
   would have discouraged). `KnightOnQueen`/`SliderOnQueen` is the first concrete candidate to
   design within this family. A raw-subboard learned-submodule design for the whole family
   already exists (`learned-evaluation.md`'s "Concrete instance: threats submodule") -- built and
   unit-tested, deliberately never wired (cost: over a million parameters).

7. **Passed pawns (scoring)** -- Rotor scores a rank-indexed bonus only (`PassedRank`). Not yet
   adopted: `PassedFile` (a file-indexed adjustment Stockfish folds on top), king tropism, and
   promotion-path safety.

8. **Space** -- not present. Middlegame-only central-space-control term, a direct popcount-based
   formula in Stockfish (no named sub-constant). Needs its own design discussion.

9. **Initiative** -- not present. A closing correction applied after every other term, a direct
   complexity-based formula (no named sub-constant). Possibly resists this document's usual
   "adopt the structure, found the numbers" treatment (asymmetry-dependent, not a clean
   independent feature) -- whether it ends up as a learned submodule like Imbalance/King safety,
   or rejected outright, is open.

10. **Tempo** -- not present. Simple, cheap, well-cited (`constexpr Value Tempo = Value(20)`,
    applied once for the side to move, added last in real Stockfish alongside the final
    side-to-move sign flip). Independent of everything else here -- no structural design work
    needed, just a decision on when to add it.

11. **PST** -- already landed (`piece-square-tables.md`), listed here only for this inventory's
    completeness. Note: in real Stockfish, PST isn't a term inside `evaluate()` at all -- material
    value is folded directly into Stockfish's own PSQT tables at startup and maintained
    incrementally on `Position`. Rotor's own PST is architecturally simpler: recomputed from
    scratch inside `material_value`'s per-piece loop every `evaluate()` call, same mathematical
    content, no incremental-update machinery.

12. **Contempt** -- not present. A deliberate bias toward avoiding draws (driven by UCI's
    Contempt/Analysis Contempt options), not a fact about the position -- a different kind of term
    than the other 11 (a playing-style/outcome-preference knob). Whether Rotor wants this at all
    is genuinely open, not just how to calibrate it -- not yet discussed.

## Not yet settled

- Sequencing among terms 6-10 beyond "Threats first" (directly motivated by a real observed game
  weakness) -- Space, Initiative, and extending Passed pawns' scoring are not sequenced.
- Whether Initiative ends up as a learned submodule (the route Imbalance and King safety both
  took) or gets rejected/founded conventionally.
- Term 3's `Connected` bonus -- not designed at all: whether Rotor wants Stockfish's full
  `[opposed][phalanx][support_count][rank]` indexing or a simplified version.
- Relationship to the search-technique gap (`TODO.md`) and `eval-tuning.md`: not yet decided
  which of search technique, this eval pass, and eventual AB(lambda) tuning comes next.
- Term 12 (Contempt): whether Rotor wants it at all.
