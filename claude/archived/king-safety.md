# King safety

**Archived:** the hand-crafted king-safety term was deleted, and so were the learned king-danger and shield/storm submodules that followed (the big five, retired 2026-10-03). The hand-crafted evaluation is PeSTO only (pesto-evaluation.md); tuning its weights is dropped.

## Status

Superseded in `eval.rs`. No hand-crafted king-safety term exists there any more. The
TSCP-derived shield/storm mechanism this document originally designed and landed was later
deleted outright and replaced by two learned submodules -- king danger, and pawn shield/storm --
both zero-initialized and currently no-ops pending training. See `learned-evaluation.md`'s
"Concrete instance: king danger submodule" and "Concrete instance: pawn shield/storm submodule"
sections for their current design and status. This document is kept as the reasoning trail for
why piece-attacker-counting was rejected as a *hand-crafted* mechanism, not as a description of
what `eval.rs` currently contains.

**Stale elsewhere**: `TODO.md`'s own evaluation-roadmap entry still describes king safety as
"done: TSCP-inspired pawn shield and storm" -- that predates this document's own supersession and
needs correcting.

## Why piece-attacker-counting was rejected as hand-crafted

Both Stockfish's king-ring attacker count/weight and Crafty's distance-weighted tropism convert a
raw danger number into centipawns via a formula only ever calibrated as part of a much larger,
jointly-tuned system (self-play/fishtest, or hand-tuning/genetic algorithms) -- a tuning pipeline
this project doesn't have. Rounding Stockfish's own coefficients to nearby round numbers doesn't
fix this: they're weights in an internal, unitless "danger" scale jointly tuned against each other
and a shared quadratic conversion step, not independently meaningful centipawn magnitudes the way
Isolated/Doubled/Mobility's source numbers are (those really are honest centipawn quantities on
Stockfish's own pawn scale, so dividing/remultiplying is a genuine unit conversion --
`kingDanger`'s coefficients never were). No real engine surveyed (Stockfish, Crafty, Rustic, TSCP)
solves "attacker weight in centipawns" without a tuning pipeline of some kind. A later
addend-by-addend attempt to found Stockfish's real 16-term `kingDanger` mechanism got 5 addends in
before the same problem resurfaced, and was abandoned rather than continued.

## What the TSCP-derived mechanism was (deleted from `eval.rs`)

Structural pawn facts only, no attacker-counting: is a shield pawn on its home square, has it
advanced, is it gone; how close has the nearest enemy pawn on that file stormed -- scaled
continuously by the specific opponent's own material (not Rotor's board-wide `is_endgame`), with
no center-file shield table (a flat penalty per fully-open file among the three centered files
instead). Ported from TSCP's real `eval.c`, correcting an off-by-one in its file indexing and
dropping its hard endgame PST cutoff (unnecessary once king safety is no longer bundled with the
king PST, since continuous material scaling already reaches zero smoothly on its own). Landed,
then deleted outright once the pawn shield/storm submodule (`learned-evaluation.md`) was designed
and settled as its outright replacement, not an addition alongside it.

## Source

TSCP (Tom Kerrigan's Simple Chess Program), 1997 (`tckerrigan.com`; mirrored at
`github.com/terredeciels/TSCP`), `eval.c` -- the citable precedent for the deleted shield/storm
mechanism above.
