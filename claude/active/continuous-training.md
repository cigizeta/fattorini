# Continuous training, gateless: design discussion

**Status: design in progress (2026-11-03), revised 2026-11-04 for training-corrections.md.** Nothing
implemented. The generational pipeline keeps running; this is a second experiment beside it, not its
replacement.

**Training corrections (2026-11-04):** every teacher's centipawns are read through the logistic at
S, `sigma(cp / S)`, the networks' own unit, with no fitted g; the loss is the binary cross entropy.
So no iteration calibrates its teacher, and every label in the window is made the same way.

**Words (the developer, 2026-11-03):** continuous training runs in **iterations**, numbered from 1;
**an iteration's network** is the winner it publishes; **the starting network** is what iteration 1
starts from, made outside the loop. "Generation" belongs to the generational pipeline alone.

## Why again

The first continuous loop (continuous-self-play.md, self-training.md) was dropped for generations
(self-training-simpler-attempt.md) after its tests showed four problems: a bootstrap where random
students never beat PeSTO, two kinds of patience, a gate too coarse for small gains, and memorising.
Three of the four came from the gate and from patience. Lc0 trains continuously **without a gate**:
the newest network always teaches, and strength is only watched. This loop drops the gate and
patience, and controls memorising by its shape.

## The loop (the developer, 2026-11-03)

Each iteration:

1. **Play:** the teacher (below: the latest tournament's winner) plays N new positions of depth-9
   self-play (games enter whole), scores in centipawns.
2. **Window:** the N positions added, the oldest N dropped, so the window holds the latest W.
3. **Train two arms** in lockstep, one pass each over the shuffled window, each at its own lambda
   (below): labels `(1 - lambda) q + lambda z` with `q = 2 sigma(cp / S) - 1`, the binary cross
   entropy against `sigma(net / S)`.
4. **Match** the two arms; the winner is the iteration's network, the student the next iteration
   trains from.
5. **Every 7 iterations, a tournament** picks the next teacher (below).

Consequences:

- **A position is trained W/N times over its life:** reuse and memory are one setting. The earlier
  memorising was too much reuse; the loss on each iteration's new positions, taken before training
  on them, shows whether W/N is too high.
- **Staleness:** the oldest label is W/N iterations old, every label from a recent teacher, unlike a
  generation's corpus, all as old as the corpus.
- **The generational pipeline is the limit** of a huge N, trained to convergence each time.
- Play and training overlap: the arenas never stop; games come from a network an iteration or two
  old.
- The trainer handles W positions for every N new ones, per arm.

**Nothing in the loop decides whether progress is happening; two curves watch it:** the loss on each
iteration's new positions before training on them (honest: never trained on, from the current
teacher), and the judge (open). **No gates (the developer):** they would defeat a fast loop.

## Lambda: never 1 for us

Lc0 trains its value head on the result alone, or nearly (`q_ratio` 0 by default; tried in T52,
"roughly same strength"). Its search's knowledge reaches the network through the policy head. Rotor's
networks are value-only under alpha-beta: **the search score s is the only channel by which the
search improves the network**, the result z one noisy bit per game, a correction and an anchor. So
lambda stays below 1 (lambda-zero.md and the grids: small for Lc0-started runs, 0.06 winning emily's
generation 2; high for PeSTO-started ones).

## lambda* measured from the labels (Claude, 2026-11-03)

**History (2026-11-04):** the measuring test (`lambda_star.rs`) was deleted with training-corrections.md
and the curves below are the old ones. The conclusion stands: lambda* cannot measure lambda for our
self-play. Lc0's figure, 0.036, stands too, Lc0's Q being unchanged.

For a position of true value v, the expected result under the teacher's play: `z = v + e_z`, the
result, and `s = v + e_s`, the teacher's score in [-1, 1]. With the errors independent of each other
and of v, the blend `(1 - lambda) s + lambda z` is closest to v at the inverse-variance weight, and
since `Cov(s, z) = Var(v)`:

    lambda* = (Var(s) - Cov(s, z)) / (Var(s) + Var(z) - 2 Cov(s, z))

Three moments, one pass, no match. Valid only where s is unbiased given v: the slope of z on s,
`Cov / Var(s)`, must be at most 1 (above it s is squeezed toward 0, and lambda* goes negative).

The ignored test `lambda_star_of_corpus` (smithy, `lambda_star.rs`) reads every searched position of
a corpus (`ROTOR_LAMBDA_DIR`, Lc0's by default), certain scores (`±1`) left out, and prints lambda*
overall, by plies to the end with each bucket's slope, pooled over the usable buckets (slope at most
1), and over the segments; `ROTOR_LAMBDA_ONE_PAWN` rescales the scores to g first.

- **Lc0's corpus:** 1.16 billion positions, the last 40 plies squeezed (Lc0's Q cautious about
  converting); pooled over the usable buckets, **0.0358, 68.5% of the positions**, between generation
  1's grid winners (0.02 for emily, about 0.05 on psq); lowest, 0.03, in the middlegame, 0.07 at 300+
  plies from the end. **lambda_0 for an Lc0-started run is 0.036.**
- **Emily's generation 2 self-play, emily-g1's games:** 342M positions, lambda* -0.16, the scores
  squeezed almost everywhere: the label scale (label-scale.md: a pawn up, today's curve says 0.54,
  the games 0.71). Rescaled to g (one pawn 0.62, ceiling 128): -0.05, the middlegame still squeezed
  (slopes 1.01 to 1.13), a "usable" 0.16 from the tails alone.

**So lambda* cannot measure lambda for our self-play:** with s fitted to z, the pairs (s, z) cannot
tell the teacher's noise from the game's luck. Hence the two arms below.

## lambda* per iteration instead of two arms: the experiment (the developer, 2026-11-04)

**The idea:** recompute lambda* from each iteration's own games and train one arm at it, replacing
the two arms and their match below. Under the logistic, s is no longer fitted to z, and against a
weak teacher may be over-confident (slope of z on s below 1) rather than squeezed, where lambda* is
valid. Its limits: a lower bound (the search and the game share errors), the best label rather than
the strongest network, and from iteration 2 a teacher trained on earlier labels, maybe squeezed
again.

**First step:** the ignored test `lambda_experiment` (temporary, smithy) over PeSTO's corpus
(`data/pesto/train`), read through `LOGISTIC`: lambda* and the slope, overall, by plies to the end
and by progress. **Then:** compare in play. Close: the two arms go. Far, or slopes above 1: they
stay.

**Measured (2026-11-04), a sample of about 1M positions (two segments, about 10,000 games) of
PeSTO's depth-9 self-play:** mates and tablebase scores left out (15%), every other score kept, s in
f64.

- **Valid everywhere:** the slope of z on s 0.84 overall, at most 0.92 in every bucket; under the
  logistic PeSTO's s is over-confident, not squeezed.
- **Overall lambda* = 0.22,** against the prior of 0.95, set high under the gentler atan curve.
- **By progress** `p = i / n`, the i-th of a game's n searched positions from the book exit, ten
  half-open buckets `(10 i) / n` in integers: lambda* rises smoothly and monotonically, 0.111, 0.122,
  0.149, 0.187, 0.240, 0.342, 0.493, 0.636, 0.844, 0.972. By plies to the end it rose again past 200
  plies, an artifact of the few long games; by progress it does not.
- **The caveat:** game length depends on the result, so buckets by progress are partly selected by
  z; the values are a shape, each a lower bound (the search and the game share errors).

**Lambda by progress, a polyline (the developer and Claude, 2026-11-04):** each iteration measures
the ten bucket lambdas from its own games; a position's lambda is the polyline through them,
linear between bucket centres (0.05, 0.15, ..., 0.95), flat beyond the end centres: no overshoot,
continuous, ten numbers. No chosen end values: the measured buckets replace them. **Each bucket's
lambda* clamped to [0, 1] (the developer, 2026-11-04)** before the polyline: below 0 the teacher's s
is squeezed, which a blend cannot correct, so s alone; above 1, z alone. No monotone rule: the shape
is the teacher's (Lc0's falls, below). On PeSTO's sample it gives a mean lambda of 0.34 over the
positions, above the single 0.22: less weight on the result in the first 40% of a game, much more
after. **To test in play:** training on the polyline against the constant 0.22, on the same data.

**Lc0, measured the same day:** 20 segments of its corpus, 17.7M positions, Q decoded through
`LC0_FILES` (the first 11 segments, rewritten or being rewritten, skipped). Pooled over the buckets
of plies to the end with slope at most 1: 0.0374, the earlier whole-corpus 0.036 reproduced, so the
code is right. By progress: valid and small in the first half (0.012, 0.029, 0.033, 0.034, 0.025),
**squeezed in the second** (slopes 1.00 to 1.03, lambda* -0.0004 to -0.19): Lc0's Q cautious about
converting, as found before by plies to the end. Clamped: lambda 0, Q alone, from mid-game on; the
network learns Q's mild caution in late positions, accepted (removing it would mean rescaling s, a
fitted g again). **So the shape is the teacher's:** PeSTO, over-confident, rising toward the result;
Lc0, accurate and cautious late, near 0 throughout. Our networks may fall either side, which is why
each iteration measures its own.

## Lambda by two arms and a match, after SPSA (the developer and Claude, 2026-11-03)

SPSA's way: many short, noisy comparisons whose averages carry the signal.

- **A centre lambda, handled in log-odds,** `theta = ln(lambda / (1 - lambda))`, so steps are
  proportionate at any lambda and lambda never leaves (0, 1).
- **Two arms each iteration,** the centre's odds halved and doubled (`theta -/+ c`, `c = ln 2`),
  trained one pass in lockstep on the same window. **Wider than the step:** neighbouring lambdas (the
  flat round robins) cannot be told apart by a short match, and +-0.01 an iteration by the sign alone
  would be a random walk wider than the useful range.
- **Each arm labels the whole window at its own lambda** (the developer), so the match compares
  lambda- with lambda+, not two mixtures; every position read through the same `sigma(cp / S)`.
  Labels are computed at training time.
- **Lambda may also correct the scale:** with no g, a teacher whose centipawns are timid against
  its games' results (measured for emily-g1 under the atan curve) gives labels lambda partly
  corrects, rather than weighing noise alone (training-corrections.md). Accepted: the controller
  follows whatever lambda plays best.
- **Never a fresh optimizer (the developer):** both arms start each iteration from the winner's
  weights and its AdamW state, so training goes on as if the winner had simply continued; a fresh
  AdamW would relearn its averages every iteration, a warm-up 33k steps long.
- **The winner** of the match becomes the iteration's network (an exact tie: the lower lambda).
  Restarting both arms from it keeps each match about one iteration's lambda, not two drifting
  histories, and picks the better of two every iteration, a selection that rejects nothing.
- **The centre moves toward the winner in proportion to the score:** `theta += (c / 2) (r - 0.5)`,
  r the score of the higher arm. A tie does not move it, a sweep a quarter of the way to the winning
  arm (odds times about 1.19); luck accumulates as a random walk, a real advantage linearly (52% on
  average doubles the odds in about 100 iterations). c and the step stay constant, unlike SPSA's:
  the best lambda moves as the network improves.
- **lambda_0: 0.036 for an Lc0-started run** (measured), first pair 0.018 and 0.070; **0.95 for a
  PeSTO-started run** (the developer: no measurement possible, PeSTO a weak teacher, every PeSTO grid
  set high; the controller moves it, slowly: at least 17 iterations of sweeps down to 0.5), first
  pair 0.905 and 0.974.

**The match: a best of 400 games, 200 pairs (the developer),** as the arena's best of n
(`best_of.rs`): each pair one opening with both colours, at depth 9, **won by the first arm past 200
points (200.5), stopped there:** only once the result can no longer change, not on a statistical
lead as a duel (whose value is its verdict, not its score). **The step reads the margin over all 400
games,** the unplayed ones counted drawn (as `pairs_left_estimate` assumes): `r = 0.5 + (points of
lambda+ - points of lambda-) / 800`. A full match gives its own score; a stopped one its margin
spread over 400, shrinking the exaggeration of a score that stopped because it got there (200.5 to
139.5 after 340 games: 0.576, where the games played would say 0.590). Close matches rarely clinch
before their last games. One game's score in emily's self-play (about 41% draws) has a standard
deviation of about 0.38, the match's about 0.019: luck about 0.007 in log-odds an iteration, about
+-7% in odds over 100 iterations; arms a point apart (51%, about +7 Elo) drift the centre about
0.0035 an iteration, visible within a few dozen. Its cost, under 0.5% of an iteration's N in
positions, on the critical path (the next teacher waits for it), over every arena worker. 400 pairs
(the round robin's and a duel's most) if iterations turn out long.

## Sizes (proposed by Claude, to try, the developer)

**N = 2^25** (about 33.5M positions an iteration, 1/32 of a generation; Lc0's T80 about 10M a
network, our arms and match a fixed cost per iteration), **W = 2^27** (about 134M): **W/N = 4
passes**, the window the last four iterations' games, about 33k steps per arm an iteration at batch
4096. An iteration lasts about the longer of `N / R` (self-play rate R) and `W / T` (training rate T
per arm, in lockstep); to aim for `W / T` about `N / R`, to check once both are measured.

## The start (the developer, 2026-11-03)

The starting network: for an Lc0-started run, a network trained on Lc0's labels. **The window
grows:** no position dropped until it holds W: iteration 1 trains on its own N, iteration 2 on 2N,
from iteration W/N on the full window. Every position still lives W/N iterations (iteration 1's
trained in iterations 1 to 4, dropped in iteration 5), so no pass more than in steady state; the
first iterations merely train on less. No prefill.

## The window in memory (the developer, 2026-11-03)

No scatter: 134M positions are about an eighth of Lc0's corpus. A fixed 32-byte record: the position
packed (a 64-bit occupancy mask, 4 bits a piece for at most 32, the side to move and castling: 25
bytes) and what the labels are made from (the teacher's centipawns and the result: 3 bytes, plus
the iteration, its age), padded to 32. **About 4.3 GB at W = 2^27,** feasible on the training machine beside
training (the developer). A ring: each iteration the oldest N slots are replaced, the whole shuffled
in place for its pass. If memory runs short: W = 2^26 (about 2.1 GB).

**Open (2026-11-04, after polyline-from-buckets.md):**

- **The polyline's source:** the generational `polyline::measure` reads scattered buckets, which this
  loop does not have. The same sums are fed from the new games or the ring instead. The ring's raw
  centipawns tell a certainty directly, where a bucket's labels can only leave out wins and losses.
- **Progress in the record:** the polyline needs each position's i and n, which the record above
  does not hold. As two `u16` (4 bytes), they leave almost nothing for the iteration within 32 bytes:
  either the record grows, or progress is stored compactly, a single byte perhaps.
- **The tables' labels:** a generational scatter labels the positions the Syzygy tables answer; this
  design does not say yet. If the loop does too, it happens as positions enter the ring.

## Resume (the developer, 2026-11-03)

Each iteration saves its network and AdamW state (both arms start from it, so one copy) and the
centre lambda. **The window is rebuilt on resume** from the last W/N iterations' games, kept on
disk as PGN anyway: minutes, rarely, rather than a 4.3 GB dump every iteration.

## Teacher and student, and the tournaments (the developer, 2026-11-03)

**What Lc0 does (its wiki and blog):** two measures. A self-play Elo, each new network against
earlier ones of the run (once the previous one alone, later several), the first network at 0: a
health curve, known to inflate (a network beats its own ancestors more than it gains against other
engines), not comparable across runs. And real strength, from occasional matches against engines of
known rating.

**Tournaments, king of the hill (the developer):** a round robin among the first 8 iterations'
networks; its winner, with the next 7 iterations' networks, plays the next round robin; and so on.
The carried winner links consecutive tournaments, so one fit gives every network a rating without a
chain's piled-up error; new networks beating the carried winner say the loop improves, the old
winner winning tournament after tournament says it has stalled (a signal for the learning rate).
Self-play inflation applies as to Lc0's curve, and is accepted: no outside anchor (the developer).

**Every 7 iterations, one shape (2026-11-03):** the current teacher and the 7 newest networks. The
starting network is the first teacher: tournament 1 is the starting network and iterations 1 to 7,
tournament 2 its winner and iterations 8 to 14, and so on. **400 pairs per pairing (the developer),**
the generational round robin's `round_robin_pairs`, all against all: 28 pairings, 11,200 pairs
(22,400 games) a tournament, about 1,600 pairs an iteration, about 1.2% of an iteration's N; the
resolution of the generational round robin, since it now picks the teacher. Not on the critical
path: the current teacher keeps playing while the tournament runs, and the winner takes over once
known.

**Two roles, as AlphaGo Zero:**

- **The teacher** plays the self-play: **the tournament's winner, for the next 7 iterations.** A
  stable teacher: consistent labels within a block, the window mixing at most two
  teachers' games; a regressed network teaches only if it wins against 7 others. The cost: labels
  up to 7 iterations old when made, and the gate's old risk, milder: students losing to the teacher
  leave it in place, the loop going on as a generational pipeline in disguise, never stopped.
- **The student** is what training continues from: **the lambda match's winner, every iteration,**
  reverted only when the learning rate drops (below).
- **They do not fight (the developer):** the lambda match moves lambda and picks the student, never
  the teacher; the tournament picks the teacher among the student's networks and the current
  teacher, never lambda or the student. Within a block the lambda controller's target stays put; a
  new teacher may move the best lambda, and the controller follows.

## The learning rate

**The stages, dropped when the teacher stays (the developer, 2026-11-03).** The
  generational pipeline's stages (3e-3, 1e-3, 3e-4, 1e-4, 3e-5) kept; the loop drops to the next
  stage **when the teacher does not change for a while:** the carried teacher winning its
  tournaments, no student beating it. A strength signal, immune to teacher changes, which the loss on
  each iteration's new positions is not (its level jumps with every new teacher). **With a
  rollback (the developer):** when the rate drops, the student reverts to the teacher's weights and
  AdamW state (kept from its iteration until it is replaced) and goes on at the next stage, as the
  generational pipeline goes back to its best checkpoint: the teacher is the strongest network
  measured, the students since it no better, and a rate that stopped helping is lowered from the
  best point found. The window, the iteration count and the centre lambda stay. **Patience: 2
  tournaments (the developer, empirical, to try):** the rate drops once the teacher has survived two
  tournaments in a row, 14 iterations. **The stages by start (the developer, 2026-11-03):** a
  PeSTO-started run, its first teacher PeSTO's evaluation and its student untrained, runs **all 5
  stages from 3e-3**; an Lc0-started run first trains its starting network on Lc0's labels, all 5
  stages to convergence (a generational training), then continues with **the last 4, from 1e-3**.
  **While the teacher is PeSTO** a drop keeps the latest student, PeSTO having no weights to revert
  to; once a network has beaten PeSTO, the rollback applies. **After the last stage the run stops
  (the developer):** the teacher surviving two tournaments at 3e-5 means the loop can no longer
  improve it; that teacher is the run's result, as a generational run ends at a failed gate.

**No rating fit, no outside anchor (the developer, 2026-11-03):** nothing the loop decides needs a
rating across tournaments (the teacher is a tournament's winner, the learning rate follows whether
the teacher survived, lambda the iteration's match). Each tournament's standings, and whether a
student dethroned the teacher, tell the run's story; the results are recorded, so a fit can be
computed afterwards if a curve is ever wanted. Self-play inflation is accepted.

## Open

Nothing in the design. To measure once running: the rates R and T against N and W.
