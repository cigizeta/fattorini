# Rotor: cutting AB(λ) at Lc0's temperature moves

**Archived 2026-11-04: superseded, see direct-blend.md, then lambda-polyline.md. The cut was retired
2026-10-22.**

## Context

Rotor uses AB(λ) to generate NNUE training labels. The recurrence combines the current evaluation
with the label propagated backward from the next ply. As in `compute_labels` (`labels.rs`), λ weights
the label propagated from the future and \(1-\lambda\) the current evaluation: λ is a decay a ply,
its horizon about \(1/(1-\lambda)\) plies.

For ply \(p\), in the White frame:

\[
L_p=\lambda L_{p+1}+(1-\lambda)E_p
\]

- \(E_p\): the search's evaluation at ply \(p\) (for Lc0's data, `best_q`).
- \(L_{p+1}\): the label propagated from the next ply.
- The backward recurrence is anchored by the final game result; a decisive score (a mate or a
  tablebase result) takes its own \(\pm1\) label.

## The observation

On Lc0's labels, three selects (`lc0-psq768`, `lc0-psq`, `lc0-psq-threats`, 2026-10-18 and 19) found
lambda flat from 0.05 to 0.75 within the round robin's margin, the winner changing from run to run
(0.55, 0.15, 0.25), and 0.95 last every time. On Rotor's self-play, the long horizon won (0.98).
Every arm of a select starts from the same weights and reads the same positions in the same order,
so the labels are the only difference between them.

## The hypothesis (the developer, 2026-10-19)

AB(λ) assumes that the move played reflects the evaluation: when the evaluation drops after a move,
the search had misjudged the position, and the future is the better guide. Rotor's self-play plays
its search's best move, so the assumption holds there.

**Lc0's training games break it.** Their moves are sampled with temperature, so Lc0 deliberately
plays moves it knows are not the best. After such a move, the future reflects the mistake, not the
position it was played from: propagating it back contaminates every earlier label within the
horizon. The longer the horizon, the more contamination, which would explain why the long horizons
lose on Lc0's data but win on self-play, and why the middle is flat, the gain of a longer average
roughly cancelled by its contamination.

## The rule

The converted PGN keeps, for every move, the move played and Lc0's best move (`{<score> <best>}`,
`smithy lc0`'s `convert.rs`). So, for each ply \(p\):

- **The move played from \(p\) is not Lc0's best move:** \(L_p=E_p\), the position's own search
  score. The chain is cut just before the deliberate mistake.
- **Otherwise:** \(L_p=\lambda L_{p+1}+(1-\lambda)E_p\), the old method.

Earlier plies propagate from a cut \(L_p\) as from any other. No new parameter: the grid stays the
ten lambdas, and the old method is the rule with no cuts. Decisive scores keep their \(\pm1\) label.

## Predictions

If the hypothesis is right, a select on cut labels shows:

- **The long horizons recover:** 0.85 and 0.95 rank far better than before, and longer values (0.98)
  may become worth a grid.
- **The curve gets a shape:** rising toward the longer horizons, as on self-play, not flat.

If the curve stays flat, temperature moves were not the cause.

## Steps

1. **Count, before any training.** Over a sample of converted games: how often the move played is not
   the best, at which plies, and how far the evaluation drops after those moves against after best
   moves. A few percent of moves, mostly in the opening, makes the hypothesis weak; frequent ones make
   it strong.
2. **Design the change.** `compute_labels` takes the scores alone; the cut needs, per ply, whether the
   move played was the best. Where the PGN is read into labels (`length_cache.rs`'s reader) and how a
   config asks for the cut are to be designed here, before any code.
3. **Train and select.** One existing shape on cut labels with the ten lambdas, its select compared
   with the same shape's select on the old labels.

## Later alternative: a per-ply lambda from the evaluations

First idea (the developer, with ChatGPT): a lambda per ply from the difference of consecutive
evaluations, \(\Delta_p=|E_p-E_{p+1}|\in[0,2]\), weighting the current evaluation more when they
disagree:

\[
\lambda_p=1-\mu\frac{\Delta_p}{2},\qquad \mu\in[0,1]
\]

Superseded by the rule above, for two reasons:

- **Scale.** Consecutive Lc0 evaluations mostly differ by a few hundredths, so \(\lambda_p\) is 0.95
  to 0.99 on almost every ply even at \(\mu=1\): the long-horizon end that loses.
- **Direction.** A drop has two causes calling for opposite labels: a deliberate non-best move (trust
  \(E_p\)) or a misjudged position (trust the future). \(\Delta\) alone cannot tell them apart; the
  best move can.

If the cut proves itself, a softer form (\(\lambda_p=\lambda_0(1-\Delta_p/2)^k\) on the remaining
plies) may be tried after it.
