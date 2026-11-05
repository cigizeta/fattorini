# The label scale: centipawns to scores and back

**Superseded 2026-11-04, see training-corrections.md and training-corrections-implementation.md:**
f is now the logistic at S = 70.37 centipawns with the binary cross entropy, and no teacher has a
fitted g; Lc0 is read as Q through Lc0's own curve.

**Status: implemented and tested (2026-11-03), label-scale-implementation.md.** Lc0's corpus fits
at a one pawn score of 0.50 or a little below, so its Q is read as it is, through the curve it was
written with. Found while measuring lambda for
continuous training (continuous-training.md); concerns the generational pipeline as much.

## The problem

Training blends a teacher's score with the game's result `z` in [-1, 1], so a teacher's centipawns
must become a score first. Today one fixed curve, `score(pawns) = atan(pawns / A) / K` (`labels.rs`),
its constants chosen (+1 pawn is 0.5, 32 pawns is 1), not fitted, serves every conversion:

1. **Teacher labels:** a teacher's centipawns (PeSTO's, our networks') to the score `s` blended with z.
2. **The student's loss:** the network outputs centipawns, compared with the label through the curve.
3. **Lc0's corpus:** Convert writes Lc0's Q as pawns through the curve's inverse; the labels apply the
   curve again, an exact round trip.
4. **The network's unit:** the curve of (2) decides what centipawns the network learns, so what the
   search's margins, the tuned parameters and UCI's displayed score mean.

The search itself needs no curve: alpha-beta compares centipawns, and any monotone curve orders moves
alike.

**Measured (2026-11-03, `lambda_star_of_corpus`, continuous-training.md):** in emily-g1's depth-9
self-play a pawn up ends at 0.71 on average where the curve says 0.54; from 1.5 to 4 pawns the curve
would need about 2.5 times the pawns to say what the games give. Lc0's corpus through the same table:
about calibrated (ratio 0.99 at 1.1 pawns, 0.7 to 0.8 beyond). So today's self-play labels blend a
timid s with a steep z, and lambda partly corrects the scale rather than weighing two estimates.

## The design (the developer and Claude, 2026-11-03)

**Two roles, two curves:**

- **f, the network's unit,** used in the loss (2) and so defining what our centipawns mean (4).
  **Fixed forever.**
- **g, a teacher's calibration,** turning that teacher's centipawns into expected results for its
  labels (1). **Fitted per teacher** on the teacher's own games, the pairs (centipawns, z): the
  reliability table turned into a curve. g is in f's family with its own constants, so a calibrated
  teacher has g = f.

**Why f can stay fixed: one correction, then none.** A timid teacher's +1.1 pawns means 0.71 in its
games while f says 0.54. Its labels say 0.71, and the student learns the centipawns f maps to 0.71,
about +2 pawns: the student is calibrated, its centipawns through f the expected result its
teacher's games showed. The next teacher's g comes out close to f, and the labels pass nearly
unchanged. Units move only as much as the games' character really changes.

**By source:**

- **Lc0:** no curve for its labels; Q is already a score. Label = `(1 - lambda) Q + lambda z`. Lc0's
  data cannot fit a curve of our centipawns (there are none in it): with Lc0 data, f is pure
  convention, and the network learns `f^-1(label)`.
- **PeSTO:** its own g, fitted on PeSTO's games, its centipawns being hand-made.
- **Our networks from generation 2 on:** g fitted on the teacher's self-play.

**Generational and continuous alike:** g fitted once per generation on the teacher's corpus
(hundreds of millions of positions for a few constants), or per cycle on that cycle's games.
Labels are scores, free of any unit, so a window may mix cycles of different g; each position
carries its own teacher's calibration, as its own lambda. f being fixed, the network's unit never
jumps.

## g fitted on emily-g1's games (2026-11-03)

The ignored test `one_pawn_of_corpus` (`lambda_star.rs`) fits today's atan family, `atan(pawns / a) /
atan(ceiling / a)`, with its two knobs: the score of one pawn (today 0.5) and the ceiling (today 32
pawns), by the mean squared error against the results over every position (0.01-pawn bins). On
emily's generation 2 self-play, 342M positions:

| Curve | Error |
|---|---|
| Today: one pawn 0.50, ceiling 32 | 0.23983 |
| One pawn 0.62, ceiling 32 | 0.23320 (2.8% less) |
| Both fitted: one pawn 0.63, ceiling 2.5 | 0.23135 (3.5% less) |

- **The anchor was the problem:** the best score of one pawn is 0.62 at almost every ceiling (0.62
  to 0.64), robust.
- **The ceiling is not fitted (the developer and Claude):** a ceiling of 2.5 pawns labels every
  advantage beyond as a certain win (the games give 0.93 to 0.98), losing every distinction above
  it, which is what the long tail was chosen to keep. Squared error against mostly decisive results
  barely punishes 1.0 where the truth is 0.95; the error falls smoothly from 32 to 2.5 with no real
  optimum, and buys only 0.8% more. **The ceiling stays 32 pawns, a design choice.**
- **What remains:** at 32 the fitted curve is still low past 2 pawns (0.81 where the games give 0.90
  at 2.1 pawns): atan cannot rise fast and keep a long tail. Accepted.

**So g for emily-g1: one pawn at 0.62, ceiling 32.** g has one fitted knob, the score of one pawn.

## The ceiling raised to 128 pawns (the developer, 2026-11-03)

32 pawns (one side's non-pawn army, the queen counted 10) is exceeded by ordinary material: the
network clamps its output to it, so the engine never says more than +32, and every label past it is
exactly 1, where Stockfish and Lc0 go far above. **The largest material advantage chess allows is
103 pawns** (nine queens, two rooks, two bishops, two knights against a bare king); **128** is the
first power of two above it, 12,800 centipawns in an `i16`, far below the mate band, about where
Lc0's curve reaches 1 (from memory). So:

- **f: one pawn at 0.50, ceiling 128.** Below 3 pawns the curve moves by under 0.01: the network's
  unit stays where nearly every position is.
- **g at 128 for emily-g1: one pawn at 0.62 again.** The best score of one pawn is 0.62 at ceilings
  32, 64, 103 and 128; the error rises from 0.233195 at 32 to 0.233448 at 128 (2.7% below today's
  0.239826 either way), the curve slightly lower past 4 pawns, the room kept for advantages up to
  128.
- **Lc0's files are decoded with the curve they were written with,** today's 0.50 and 32, frozen under
  a name of its own, straight into Q: no reconversion, and what the files store no longer depends
  on f.
- **Networks trained before** keep playing: they never output beyond 32.

## When g is measured (the developer, 2026-11-03)

**Only when the next generation trains.** Playing needs no curve: the network outputs centipawns,
and the search, its margins and UCI's score are all centipawns. f is called only in the trainer's
loss, g only where labels are made from finished games. So a run started from Lc0:

1. Generation 1 trains on Q and z: no g. Its round robin and gate play in centipawns.
2. Generation 2's Play: the winner's depth-9 self-play, its scores written in centipawns, as today.
3. **g fitted on those games:** one pass, the score of one pawn at ceiling 128, recorded with the
   generation beside its corpus.
4. Generation 2 trains on `(1 - lambda) g(cp) + lambda z`, the student through f. Round robin and
   gate in centipawns.

Every later generation does the same with its own teacher. Expected: about 0.62 for generation 2's
teacher, then near 0.50 once a student has absorbed the correction, then small moves as the games
change; a g far from 0.50 generation after generation would say the reasoning is wrong.

## Open

1. **f: today's atan, kept (2026-11-03).** Lc0 displays its Q as centipawns with a scaled tan, so
   its centipawns-to-Q curve is atan too (`labels.rs` says so): about `atan(cp / 90) / 1.5638`
   (from memory, not checked in Lc0's source), +1 pawn at Q about 0.54 where ours gives 0.50, Q = 1
   at about 128 pawns where ours reaches 1 at 32. Nearly the same curve where it matters, so
   adopting Lc0's constants would buy little and break with every network so far.
2. **g's family and fit: settled (2026-11-03),** f's family, the ceiling at 32 pawns, the score of
   one pawn fitted by the squared error against z (above).
3. **Lc0's corpus: settled (2026-11-03),** decoded with the frozen curve it was written with (above),
   no reconversion.
4. **lambda: settled for generations (the developer, 2026-11-03): the pipeline chooses it,** nine
   arms in lockstep and the round robin, as today, the grid centred as now (generation 1 at 0.05 on
   Lc0's labels, measured lambda* 0.036; from then on the winner's). The lambda* formula cannot give
   lambda for our self-play: with `s = g(cp)` fitted to z, the pairs (s, z) cannot tell the
   teacher's noise from the game's luck, and on emily-g1's games rescaled to g (one pawn 0.62,
   ceiling 128) it gave -0.05 overall, the middlegame still squeezed (slopes 1.01 to 1.13, atan's
   one knob unable to follow 1 to 4 pawns), a "usable" 0.16 from the tails alone. A deeper search
   on a sample would measure it (`lambda = Cov(z - s, d - s) / Var(z - s)`, a lower bound); needed
   only for continuous training, one arm and no round robin, not designed.
5. **Mates and tablebase results** stay outside every curve, `±1`, as now.
6. **The frozen test sets' labels** depend on g where the teacher is ours: a test loss compares only
   under one g. To work out with (2).
