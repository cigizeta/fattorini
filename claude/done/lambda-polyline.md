# Lambda by progress: a polyline instead of the grid

**Status: design in progress (2026-11-04).** Nothing implemented. Follows training-corrections.md
(the logistic at S, the binary cross entropy) and the measurements in continuous-training.md
("lambda* per iteration instead of two arms").

## The idea (the developer, 2026-11-04)

Today a generation trains nine arms in lockstep, one per lambda, lambda constant over a whole game,
and a round robin picks the winner. Instead:

1. **Measure** the corpus once, before training: lambda* in ten buckets of progress.
2. **Train one arm,** each position labelled at its own lambda, the polyline through the ten.
3. **No grid, no round robin:** the generation's one network goes on as the winner did.

Training is much faster (one arm, not nine; no round robin's games), and lambda follows the game.

## Definitions (agreed 2026-11-04)

- **s and z,** from each searched position's side to move: s the teacher's score read as a score in
  [-1, 1] (Lc0's Q; `sigma(cp / S)` for PeSTO and our networks), z the game's result.
- **lambda*** of a set of positions: `(Var(s) - Cov(s, z)) / (Var(s) + Var(z) - 2 Cov(s, z))`, the
  blend's inverse-variance weight; valid where the slope of z on s, `Cov / Var(s)`, is at most 1.
- **Progress** of the i-th of a game's n searched positions, counted from the book exit: `p = i / n`.
- **Ten buckets,** half-open, bucket `(10 i) / n` in integers.
- **Left out of the measurement:** mates and tablebase scores (`labels::is_decisive`), certainties
  rather than estimates; each game's final position, which has no score. Every other score kept, s
  in f64.
- **Clamp** each bucket's lambda* to [0, 1]: below 0 s is squeezed, a blend cannot help, s alone;
  above 1, z alone. No monotone rule: the shape is the teacher's.
- **The polyline:** linear between bucket centres (0.05, 0.15, ..., 0.95), flat beyond the end
  centres. A position's lambda is the polyline at its progress.

## Measured (continuous-training.md, 2026-11-04)

- **PeSTO,** depth-9 self-play, about 1M positions: 0.111, 0.122, 0.149, 0.187, 0.240, 0.342,
  0.493, 0.636, 0.844, 0.972; valid everywhere.
- **Lc0,** 17.7M positions: 0.012, 0.029, 0.033, 0.034, 0.025, then squeezed (clamped to 0) from
  mid-game on.

## The test (the developer, 2026-11-04)

Retrain `lc0-psq-threats-screlu`'s generation 1 on Lc0's labels with the new curve and the polyline,
one arm, and duel it against **emily's generation 1** (same architecture, same data). The curve and
the polyline change together: a win keeps both, a loss does not say which failed (accepted). On Lc0
the polyline is close to emily's 0.02, so this tests mostly the curve and the speed; the polyline's
real test is with a weak teacher, PeSTO.

## What has to be built

1. **Progress in the bucket cache: i and n (the developer, 2026-11-04),** two small integers per
   position beside its labels at lambda 0 and 1, exact; p and the bucket computed when needed, so a
   change of buckets or shape needs no rescatter. A rescatter once, to add them.
2. **The measurement, beside the corpus, once (the developer, 2026-11-04):** the experiment's pass
   made permanent, the ten clamped lambdas. A source's corpus (Lc0, PeSTO): measured once, kept
   beside it on the remote, shared by every run of the source. A self-play generation: measured on
   its own training games before training, kept in the generation's directory. Recomputed only when
   the corpus changes.
3. **One arm,** labelled at the polyline at batch time; the grid and the round robin skipped. **The
   generation's network is the arm's best checkpoint (the developer, 2026-11-04):** its checkpoint at
   the lowest test loss, exported, as each arm's candidate is today; with one arm the candidate is
   the winner. **The gate unchanged (the developer, 2026-11-04):** the arm's network takes the round
   robin winner's place, against the same opponent with the same rules.
4. **The frozen test set** labelled at the polyline.

5. **The grid kept, selectable per config (the developer, 2026-11-04):** a config chooses the
   nine-arm grid with its round robin, or the polyline with one arm; both paths maintained until
   one is dropped, so a later run can compare them side by side.

## Open

Nothing in the design. Next: the implementation plan.
