# The direct blend on Lc0's labels: implementation plan

**Status: agreed and implemented 2026-10-20** (decisions 1, 3 and 4 as written); `forge
lc0-psq-direct` started on the training machine the same day.

## Why

`lc0-psq-zero` (lambda-zero.md) had the lowest test loss of every uncut `lc0-psq` arm, yet was losing
its duel against `lc0-psq-0001-0.15` (+41 -62 =65 after 168 games, about -44 Elo, still running): some
blending helps in games, beyond what test loss on a network's own labels shows. AB(lambda) blends
through every later score of the game, so Lc0's temperature moves push their drops backward (the cut
removed much of that: 0.95's loss from 0.108 to 0.083). Lc0's own training blends differently:
`qMix(z, q) = q * q_ratio + z * (1 - q_ratio)` (lczero-training `tf/tfprocess.py`), the position's own
search value with the game's result, nothing in between (continuous-self-play.md, "Why").

The experiment: the same ten lambdas, 0.05 to 0.95, labelled by the direct blend,

    label_t = (1 - lambda) * score(s_t) + lambda * z

with `z` the game's result (+1, 0, -1 from White, flipped to the side to move like every label). It
asks whether the result's information helps once it arrives without the chain's temperature noise.

## Decisions to agree

1. **The field:** `label.blend: direct`, absent meaning AB(lambda) as today. Together with `label.cut`
   it is refused: a cut stops a chain the direct blend does not have.
2. **No decisive plies on Lc0's corpus:** `smithy lc0` writes `pawns_from_score(best_q)`, at most the
   32-pawn ceiling (3,200 cp), far below `is_decisive`'s mate and tablebase band, so every ply takes
   the blend. The function still gives a decisive ply its own `±1` without propagating it, for
   self-play's searches, which do score mates, should it ever use the direct blend.
3. **Label sets:** `lambda-0.25-direct`, beside `lambda-0.25` and `lambda-0.25-cut-0.10`, frozen by
   `smithy freeze lc0 lc0-psq-direct` from the same reservoir slots, so the positions are the same rows.
4. **The network:** `configs/lc0-psq-direct.yaml`, `lc0-psq`'s config field for field, the ten
   lambdas, plus `blend: direct`.

## Changes

1. `labels::compute_labels_direct(scores, outcome, lambda)`: in the White frame, as `compute_labels`;
   decisive plies `±1`, every other `(1 - lambda) * score(s_t) + lambda * outcome_label(outcome)`,
   clamped. Tests: at 0 it equals `compute_labels` at 0; at 1 every non-decisive label is the outcome;
   a decisive ply does not change its neighbours' labels; the side-to-move flip as the existing test.
2. `preprocessing_config`: the labelling as one enum, replacing `Option<Cut>` where it is passed
   (`freeze.rs`, `length_cache.rs`, smithy's `main.rs`):
   `Labelling { Chain, ChainCut { drop }, Direct }`, read by one `load_labelling`, refusing `cut` with
   `blend: direct`. `label_set_dir_name` spells `-direct`.
3. `length_cache::self_play_game_labels_with`: `Direct` calls `compute_labels_direct` through
   `game_labels`, as `Chain` calls `compute_labels`.
4. The config, and the committed configs test naming it.

To check while implementing: every place `freeze` and the training stream take the cut, so none
labels `Direct` as `Chain` silently.

## Judging it

- **The select:** `select lc0-psq-direct`'s table against `select lc0-psq`'s and `select lc0-psq-cut`'s.
  If the chain's noise is what hurt the long horizons, the direct blend's curve should not fall at
  0.85 and 0.95, and may peak higher than 0.15's.
- **The duel:** the winner against `lc0-psq-0001-0.15`, both untuned, at 10+0.1.
- **Test loss** only as the noise curve across lambdas, never across label methods (lambda-zero.md:
  the lowest loss lost its duel).

## Predictions, written before the select (2026-10-20)

- **The developer:** the direct blend's best is about as strong as AB(lambda)'s, and its select table
  is a smooth bell curve over the lambdas, unlike AB(lambda)'s zigzag.
- **Claude:** the curve peaks at a small lambda, 0.05 to 0.25, then falls steeply, the result's noise
  carrying full weight far from the game's end; the best arm is at most level with AB(lambda)'s 0.15,
  perhaps a little weaker, since what AB adds over lambda 0 is mostly smoothing over the next scores,
  which the direct blend lacks. About 60/40.

## Results (the training machine, 2026-10-21)

**Forge:** 159 rounds, about 2.7 passes over the 1B positions. The best test losses fit
`0.0706 + 0.006 lambda + 0.20 lambda^2` almost exactly (0.95: 0.2568 predicted, 0.2581 measured):
the score is learnable, the result adds unlearnable noise weighted by lambda^2. The curve's 0.0706
at lambda 0 matches `lc0-psq-zero`'s 0.07051. Test loss says nothing about the best lambda here.

**`select lc0-psq-direct`** (36,000 games, +/-1.0% a lambda): 0.05 55.0%, 0.15 54.6%, 0.25 53.8%,
0.35 52.1%, 0.45 51.8%, 0.55 49.8%, 0.65 49.0%, 0.85 47.5%, 0.75 43.3%, 0.95 43.1%. **Smooth and
nearly monotonic**, unlike AB(lambda)'s zigzag, spread over 12 points against about 7. One outlier,
0.75, taken as one unlucky run (its test loss is on the curve). The peak is at the grid's lower edge.

**The duel,** `lc0-psq-direct-0001-0.05` against `lc0-psq-0001-0.15`, both `@default`, 10+0.1:
904 games in 44 min, +298 -299 =307, 451.5 - 452.5, S 0.4994 [0.4486, 0.5514], curtailed: **drawn**,
the two within about 20 Elo.

**The predictions:** the developer's "about as strong" confirmed, "smooth" confirmed, "bell" not
(the peak sits at the edge); Claude's small-lambda peak confirmed, "falls steeply" not (steadily),
"at most level" confirmed, "perhaps a little weaker" not supported.

**Reading (the developer, 2026-10-21):** AB(lambda) injects noise that turns lucky, but nobody knows
where in advance. Its runs spread widely, and the select keeps the best of ten, harvesting that
spread; the direct blend's runs barely spread, so its best of ten gains little from luck. The duel
compares best of ten against best of ten, which favours the noisier method without its labels being
better. **Consequence:** the direct blend is kept (equal strength, a smooth curve that says where to
look, no label set per lambda, which gen-prepare.md relies on), and its luck can be bought on
purpose with tickets near its peak: `lc0-psq-direct-fine`, lambdas 0.01 to 0.09, the grid of
gen-server's generation 1, its winner against `lc0-psq-0001-0.15` (planned after the code clean-up
of 2026-10-22).

## Results of the fine grid: `lc0-psq-r1`, generation 1 (the training machine and the Lichess machine, 2026-10-23)

The fine grid ran as the `lc0-psq` config's run 1 on the forge, generation 1 on Lc0's labels.

**Training:** every lambda finished its five rates in 119 to 156 rounds. Best test losses rise with
lambda, 0.06918 (0.01) to 0.07129 (0.09), the label noise of the curve above; 0.08 and 0.09 swap
places by 0.00001.

**Round robin** (36 pairings of 400 pairs, 28,800 games, +/-1.0% a lambda): 0.03 51.3%, 0.01 51.1%,
0.02 50.7%, 0.09 50.6%, 0.07 50.2%, 0.05 49.8%, 0.06 49.5%, 0.04 48.5%, 0.08 48.3%. A line through
score against lambda falls 0.17 points a step of 0.01 (about 1.4 over the grid), 1.3 standard errors
from flat, and the points scatter around it by 1.03, the games' own noise: **flat within noise**,
perhaps a slight tilt toward the low lambdas. The top three are level within their margins.

**Gate:** the winner, 0.03, against PeSTO: passed in 70 games, the challenger at 75.8% (about +200
Elo).

**Reading (2026-10-23):** across 0.01 to 0.09 the scores spread by 3 points, about what the games'
noise alone gives: these nine tickets bought little luck, so the direct blend's training runs barely
spread here either, as at the coarse grid. The winner is 0.03 by a margin no larger than noise. Still
to do: the planned duel of this winner against `lc0-psq-0001-0.15`, both untuned, at 10+0.1, and the
same grid on `lc0-psq-threats`, where a matching tilt would make it more than noise.

## Next: AB(lambda) without the result (proposed 2026-10-20)

AB(lambda) at a small lambda mixes two ingredients: **smoothing**, each label averaging a few of the
next search scores, which removes some of `best_q`'s jitter; and **the result**, whose weight
`lambda^d` is tiny except near the game's end. The direct blend has the result without the smoothing.
The missing variant has the smoothing without the result: the same chain, anchored on the game's last
search score instead of the outcome.

| Experiment | Smoothing | Result |
|---|---|---|
| lambda 0 (lambda-zero.md) | no | no |
| AB(lambda) (`lc0-psq`) | yes | yes, weak far from the end |
| Direct blend (this) | no | yes |
| Chain without the result | yes | no |

Whichever ingredient separates the weak from the strong is what blending provides. To design after
this select.

## Cost

Ten stacked arms, as `lc0-psq`: about 126 rounds at about 68 s, 2.4 h, then the select, 36,000 games,
about 40 min; the freeze before it. the training machine only.

## Steps for the developer

1. Build and test (`cargo test -p rotor -p smithy -p arena`).
2. `smithy freeze lc0 lc0-psq-direct`.
3. `smithy forge lc0-psq-direct`, then `arena select lc0-psq-direct`, then the duel.
