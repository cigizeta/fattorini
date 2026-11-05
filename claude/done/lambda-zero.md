# Lambda 0 on Lc0's labels: implementation plan

**Status: run 2026-10-20; lambda 0 is weaker.** Trained on the training machine, 159 rounds at about 20 s, candidate
test loss 0.07051, the lowest of every uncut `lc0-psq` arm (0.15's: 0.07092; the cut's 0.05 and 0.15:
0.07056 and 0.07046). The duel against `lc0-psq-0001-0.15`, stopped by the developer at 168 games: +41
-62 =65, 43.8%, about -44 Elo (95% roughly -86 to -3), unresolved by the duel rule but clear enough.
The lowest loss lost: test loss on a network's own labels does not rank strength across label sets.
Some blending helps; how, direct-blend.md asks next.

## Why

Three selects on Lc0's labels (`lc0-psq768`, `lc0-psq`, `lc0-psq-threats`, TODO.md) came out flat from
0.05 to about 0.75, the winner changing from run to run (0.55, 0.15, 0.25) and the longest horizons
worse every time. Lc0's `best_q`, from an 800-visit search, is already a good estimate, so the
game's future adds mostly noise and temperature drops (continuous-self-play.md, "Why"). If lambda 0,
each position's own score alone, is as strong as the best of the grid, every `lc0-` network trains
one arm instead of ten and skips the lambda select, and the temperature cut (temperature-cut.md)
is moot, since nothing propagates backward. For Lc0's labels only: self-play keeps its grid.

0 was never tried: `label.lambda` refused it ("a label from the teacher's score alone ... has never
been wanted"), and so did the arena's player names.

## Changes

1. `preprocessing_config::validate_lambdas`: `[0, 1]` instead of `(0, 1]`, its comment saying why;
   the test's refused list loses `vec![0.0]`, and `[0.0]` is accepted.
2. `match_player::parse_network_name`: a lambda in `[0, 1]`, so `lc0-psq-zero-0001-0.00` is a player.
3. `configs/lc0-psq-zero.yaml`: `lc0-psq`'s config field for field, `lambda: [0.00]`. The committed
   configs test names it.

`labels::compute_labels` needs nothing: at 0 every non-decisive label is `score(scores[t])`, as its
comment and test already state.

## Judging it

One duel, both untuned, at 10+0.1: `lc0-psq-zero-0001-0.00` against `lc0-psq-0001-0.15`, the winner
of `select lc0-psq` (2026-10-19). A draw or a win makes lambda 0 the rule for `lc0-` networks.

## Steps for the developer

1. Build and test (`cargo test -p rotor -p smithy -p arena`). the training machine is busy with
   `arena select lc0-psq-cut`, so test on the Lichess machine, or on the training machine after the select.
2. On the training machine: `smithy freeze lc0 lc0-psq-zero`, which adds the `lambda-0.00` label set to the frozen
   test set.
3. On the training machine: `smithy forge lc0-psq-zero`, one arm.
4. `arena select lc0-psq-zero`: a one-value grid names its one lambda.
5. The duel above.
