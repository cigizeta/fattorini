# Textbook hand-crafted evaluation

**Archived:** these terms were implemented as the big five network (cpw-king-shield.md, king-shield-storm.md, king-attack.md, bishop-pair.md, opposite-bishops.md), retired 2026-10-03 with its code removed. The hand-crafted evaluation is PeSTO only, and tuning its weights is dropped. TODO.md cites this document for what remains of the ideas.

Status: design in discussion, nothing implemented. Paused waiting for the developer's tuning
algorithm, which decides how the parameters are represented and what data the tuner needs.

## Goal

A better hand-crafted evaluation for the lineage root: the player whose depth-9 search labels
generation 1 (today `hand-crafted`, material + Michniewski PST). Later generations replace it with
their own champions, but on one PC each generation costs a full replay, so a stronger starting
teacher saves whole generations.

The evaluation is the textbook "big five": material, piece-square tables, pawn structure, mobility,
king safety. **Textbook style**: simple and clean, each term describable in a paragraph, not
pretending to be Stockfish. **Tuned automatically**: without tuning, the weights are guesses and the
terms fight each other (a PST already rewards a centralized knight, mobility rewards it again), so
the evaluation would be effectively random. Every weight is a starting value for the tuner.

## Reference

**CPW-Engine** (the Chess Programming Wiki's teaching engine) is the reference for the term
definitions: the closest match to "textbook", and its values are hand-picked, so they serve as
starting points for the tuner. Start from CPW and make a term more complex only when a measurement
warrants it. Fruit 2.1 is the reference if more structure is ever warranted; Ethereal (classical,
before NNUE) is a reference for tuning, not for term definitions. PeSTO covers material and PST
only. Authorship, license and the exact CPW terms are still to be verified against the source.

The five terms below are the first proposal, to be reconciled with CPW's own terms point by point.

## Plan of work

Decided: the evaluation function is **rewritten from scratch**, not revived from `eval.rs`'s dead
code, and built in four steps: (1) PeSTO material and PST, then (2) pawn structure, (3) mobility,
(4) king safety. It is implemented in full first; only afterwards does its effect on search
(margins, benchmarks) get looked at. The old `eval.rs` evaluation and all its dead code go; the
simple predicates listed below may be carried over where the new code needs them, rewritten to the
new style rather than kept as they are.

## Step 1: PeSTO material and PST

 Step 1 is material and PST only, from PeSTO, whose
values are already tuned, so no tuner is needed yet.

- PeSTO's own game phase for the taper (knight 1, bishop 1, rook 2, queen 4, 24 at the start,
  capped at 24), since its tables were tuned against it, not Rotor's clamped 0..128 `game_phase`.
- PeSTO's native values (pawn 82 mg / 94 eg): no rescaling. Whether search margins need review is
  a search question, for later.
- PeSTO's tables run a8..h1; Rotor's are LERF. A test pins known entries (e.g. a White knight on d4
  reads PeSTO's d4 value, and Black reads the mirrored square).
- `eval::piece_value` stays: SEE and move ordering use it as a fixed ruler, not as evaluation.
- Decided: the new evaluation **replaces `hand-crafted`** in place; no new player name. Hazard:
  `rotor-arena replay` is labelling CCRL/TCEC chunks as `hand-crafted` with the old evaluation right
  now, and a replay machine rebuilt mid-run would label the rest with the new one, under the same
  name. Handled by branches: the replay runs on `main`, this work is on `alessio`, so the new
  evaluation must not be merged into `main` before the replay is complete. Open, for later: whether
  the finished `hand-crafted` replay is then kept (labelled by the old evaluation) or replayed again
  with the new one.

## Step 2: pawn structure (decided)

CPW-Engine's pawn terms, minus the protected passer, each duplicated into an mg and an eg version
starting from CPW's single value (CPW does not taper its pawn terms; the tuner will separate them):

- Doubled: a penalty for a pawn with another friendly pawn on its file (CPW: about -20).
- Weak: a pawn no friendly pawn can ever defend (no friendly pawn on an adjacent file, level with
  it or behind), covering isolated and backward pawns in one test. A per-square table, plus an
  extra penalty on a half-open file (CPW: about -4).
- Passed: no enemy pawn ahead of it on its own or an adjacent file. A per-square table.
- Dropped: CPW's protected-passer table.

CPW's material and PST are the same shape as PeSTO's (same 24-point phase) but less tuned, so step
1 stands. Its material adjustments (bishop pair, knight and rook pairs, knight/rook value by pawn
count) are a sixth kind of term, left out; the bishop pair is the first candidate if a measurement
warrants one. Exact CPW values to be read from its source.

## Step 3: mobility (done, cpw-mobility.md)

## Step 4: king safety (done)

Two parts, read from CPW-Engine and discussed point by point.

**Pawn shield (done, cpw-king-shield.md).** Not CPW's fixed squares (CPW checks f2/g2/h2 for any king on files f-h,
at any rank, so a king on g4 still collects the full bonus from pawns behind it). Instead:

- One table, `SHIELD[square]`: 64 entries by absolute square, a8 first, read through the same flip
  as the other tables. Ranks 1 and 8 never hold a pawn, so those 16 entries are 0 by
  construction, as in CPW's pawn tables; only ranks 2..7 are parameters. Starting values (CPW's
  10 / 5 for a king on its first rank): 10 on the second rank, 5 on the third, 0 on ranks 4..7.
- The king's square decides which squares are **active**: `ACTIVE[king_sq]`, a precomputed
  bitboard of the three files around the king (the center clamped to b..g, so a king on h1 looks
  at f, g, h), one and two ranks ahead of it.
- `shield(k)` = the sum of `SHIELD[sq]` over the own pawns on `ACTIVE[k]`. A plain sum: a doubled
  pair counts twice, and the pawn-structure doubled penalty is left to handle it.
- Castling: the shield is the **max** of `shield` at the king's square and at the destination of
  each castling right still held. A king on e1 with only the short right scores
  max(shield(e1), shield(g1)); with both, max(shield(e1), shield(g1), shield(c1)). Only the right
  is checked, not whether castling is legal now.
- Middlegame only, as CPW.

**Tables (decided).** Four, of two kinds:

- Tunable values, `[i32; 64]` by the pawn's square, a8 first, middlegame only: `SHIELD_MG`,
  `STORM_MG`.
- Fixed geometry, not tunable, bitboards by the king's square: `SHIELD_ZONE`, `STORM_ZONE`, built
  by a `const fn` from one rule (three files, center clamped to b..g, a range of ranks ahead of
  the king). Today's `shield_at` computes the shield zone with `try_offset`; it moves to
  `SHIELD_ZONE` together with the storm.

**`SHIELD_ZONE` (decided).** `[[Bitboard; 64]; 2]` by king color and square: the three files
around the king (center clamped to b..g), one and two ranks ahead in the color's direction, minus
ranks 1 and 8 (no pawn can stand there). At most 6 squares; empty for a king on its 7th or 8th
rank. The castling max is not in the zone: the scoring takes the max of the shield over the
king's square and each held right's destination (short right on e1: max of e1 and g1), each
looked up in `SHIELD_ZONE`.

**`SHIELD_MG` (decided).** `[i32; 64]` by the pawn's absolute square, a8 first, through
`table_index`; all zeros, filled by tuning. Only ranks 2..7 (48 entries) can be read.

**`STORM_ZONE` (decided).** Same shape and rule as `SHIELD_ZONE`, but **four** ranks ahead of the
king instead of two (a king on g1: f..h, ranks 2..5), minus ranks 1 and 8; at most 12 squares.
It holds the enemy pawns, where `SHIELD_ZONE` holds the own ones.

**`STORM_MG` (decided).** The same style as `SHIELD_MG`: `[i32; 64]` by the enemy pawn's
absolute square, a8 first, read through the king owner's `table_index`; all zeros, filled by
tuning; added to mg with its own sign (tuning makes threats negative).

**Pawn storm (done, king-shield-storm.md).** `storm(k)` = sum of `STORM_MG[sq]` over the enemy pawns on
`STORM_ZONE[k]`, under the same castling max. Starting values (decided): `STORM_MG` is all
zeros, since CPW has no storm; its values come from tuning only. `SHIELD_MG` too is reset to
all zeros (decided), dropping the CPW-derived 10 / 5 now in `cpw.rs`: king safety as a whole
starts neutral and is learned.

**`KING_ZONE` (decided, from Stockfish 11's `kingRing`).** The 3x3 block around the king's
square with its file clamped to b..g and its rank to 2..7: always 9 squares. A king on g1 or h1
watches f1..h3; a king on e4 the block around itself. The rank clamp is symmetric, so the zone
does not depend on the king's color: `[Bitboard; 64]`. Stockfish 11 also removes squares doubly
defended by own pawns (from memory, unverified); not carried over.

**King danger contribution (decided).** The same pieces as mobility (knight, bishop, rook,
queen). For each, the zone squares it attacks, with the attack sets mobility already computes
(full occupancy, no x-rays; a zone square counts whatever stands on it). Scored through a table
per piece type by that count (generalizing `weight * count`, as mobility did), all zeros, filled by
tuning. Defense ignored. Open: the combination, which must not stay a plain additive sum.

**King attack (done, king-attack.md; names included).** Also decided: no gate (CPW's "2
attackers and a queen" dropped); a zone square counts only if it counts as mobility (CPW); tables
sized to each piece's largest count (3, 5, 5, 9), entry 0 pinned at 0, 18 tunable; starting values
`round(PeSTO_mg * n / 10)`; cap 500; middlegame only.

- `KING_ZONE` (was `DANGER_ZONE`), as above.
- `ATTACK_KNIGHT`, `ATTACK_BISHOP`, `ATTACK_ROOK`, `ATTACK_QUEEN`: tunable tables by the number
  of `KING_ZONE` squares the piece attacks.
- attack units = the sum of those entries over the enemy's knights, bishops, rooks and queens.
- king attack = attack units² / 4096. Nonnegative by construction: a magnitude, not a signed
  score. D = 4096 is fixed, not tuned: scaling every table by k and D by k² changes nothing, so D
  is only the resolution (an entry of 64 is one unit).
- king safety = shield + storm - king attack, from the king owner's side; the castling max covers
  shield and storm only. Today's `king_safety` (shield + storm) is renamed `pawn_cover`. Shield
  and storm carry their sign in their tables; king attack cannot (the square loses it), so its
  minus is written in the formula.
- Open: starting values (later, with the tuning algorithm); middlegame only or also an endgame
  part (Stockfish: units / 16).

**CPW's king attack, for reference.** CPW: a zone around the enemy king (the 8-square ring plus 3 squares two
ranks ahead; CPW's code has a bug making it asymmetric), per attacking piece `weight * att` summed
into `attWeight` (knight 2, bishop 2, rook 3, queen 4; `att` = its mobility squares in the zone),
a gate (at least 2 attackers and a queen), and `SafetyTable[attWeight]` (100 entries, roughly
quadratic, flat at 500), untapered. To decide one point at a time: zone, contribution, combination,
phase. Also fix: the table index must be clamped.

## Beyond the big five

- Bishop pair: done, bishop-pair.md (CPW's rule, 30 mg / 30 eg).
- Opposite-colored bishops: done, opposite-bishops.md. The pure case only (one bishop each on
  opposite colors, no other pieces): the whole evaluation is divided by 2. Stockfish's other
  scalings left out.

## The five terms (proposed)

Every term is tapered between a middlegame and an endgame value by game phase, and linear in its
weights, which suits any tuning algorithm.

1. **Material**: an mg and an eg value per piece type.
2. **Piece-square tables**: an mg and an eg table per piece type (today only the king has two).
3. **Pawn structure**:
   - doubled: a penalty for each extra pawn on a file;
   - isolated: a penalty for a pawn with no friendly pawn on either adjacent file;
   - passed: a bonus for a pawn with no enemy pawn ahead of it on its own or an adjacent file, by
     rank.
   Backward pawns are left out: the existing `is_backward` reproduces Stockfish's lever/stop-square
   formula, more than a textbook teaches.
4. **Mobility**: for each knight, bishop, rook and queen, the squares it attacks that its own pieces
   do not occupy, times one weight per piece type. Not Stockfish's per-count tables, and no
   exclusion of squares attacked by enemy pawns.
5. **King safety** (middlegame):
   - pawn shield: a bonus for each own pawn on the three files around the king, one or two ranks
     ahead;
   - king attack: a penalty for each enemy piece attacking a square next to the king, one weight
     per piece type.
   Both linear; no attack-units `SafetyTable`.

## What the dead code in `eval.rs` contributes

- Candidates to carry over, rewritten as needed: `is_isolated`, `is_passed`, `pawn_file_counts`,
  `king_ring`, `piece_attacks`, `taper`. Not `game_phase`: step 1 uses PeSTO's phase.
- Deleted: the Stockfish-derived tables (pawn penalties, mobility tables) and their long derivation
  comments, `is_backward`, `mobility_area`'s exclusions, `is_opposite_colored_bishop_ending`, and
  all five sub-networks with random, untrained weights (king danger, pawn shield/storm, imbalance,
  pawn structure, threats) with their feature extraction and tests.

This replaces the "tolerated dead code" cleanup of `eval.rs`: the cleanup and the new evaluation
are one step.

## Consequences of automatic tuning

- All weights live in one parameter vector with a stable layout, not in scattered `const`s, so the
  tuner can read and write it and the engine can load a tuned set.
- Published values (PeSTO tables, the old rescaled Stockfish numbers, textbook values) are starting
  points only. The PeSTO TODO item becomes mostly a choice of starting tables.
- Measurement: tune the whole evaluation, then duel it against today's evaluation at depth 9 (where
  the teacher works, and where extra evaluation cost is paid in time, not depth).

## Constraints carried over

- **Player name**: the new evaluation is a new player (e.g. `hand-crafted-2`, its own sampled and
  replayed directories), never a silent change to `hand-crafted` while a replay labelled by it is
  running or in use. The CCRL/TCEC replay running now uses today's evaluation.
- **Search benchmarks**: they rely on material + PST as a fixed per-node cost. Keep it as a named
  baseline variant, or move the benchmarks to the new evaluation.
- **Scale**: search margins (futility, razoring) and the label curve assume a pawn of about 100.
  Tuned values must stay on that scale, or those margins need review.

## Open questions

1. The tuning algorithm (the developer's, to be discussed): its data, its objective, and the
   parameter representation it needs.
2. Passed pawns: a bonus per rank (6 mg/eg pairs, still textbook) or one weight times rank.
3. King safety middlegame only? The king PST's endgame table already covers king activity.
4. Pawn hash table: only if measurement shows pawn structure costs too much per node.
5. When the new player takes over from `hand-crafted`, and under what name.
6. The search-benchmark baseline.
