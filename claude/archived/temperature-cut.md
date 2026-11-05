# The temperature cut: implementation plan

**Archived 2026-11-04: superseded, see direct-blend.md. Implemented, then retired 2026-10-22.**

The why is in dynamic-lambda.md: Lc0's training games play deliberate non-best moves (temperature),
and AB(λ) propagates their consequences into earlier labels. This plan implements the rule there:

- the move played from ply \(p\) is not the best move its comment names: \(L_p=E_p\), its own score;
- otherwise \(L_p=\lambda L_{p+1}+(1-\lambda)E_p\), as today.

**Status: implemented 2026-10-19, not yet built or tested** (Claude cannot run anything): the
open questions below were settled at their defaults, `cut: non-best`, a cut ply on \(E_p\) alone,
the count at the freeze only. The current training
(`lc0-psq-threats-pairwise`) finishes first, on today's code. Then `lc0-psq` (780 rows) is retrained
with the cut, and the old method stays available: a network without the cut in its config labels
exactly as today, bit for bit.

## Where things are today

- `labels::compute_labels(scores, outcome, lambda)` (`rotor/src/labels.rs`): the backward recursion,
  White-relative, decisive scores taking \(\pm1\). Scores only; it never sees a move.
- `length_cache::self_play_game_labels(collected, lambda, game_index)`: one game's labels, side to
  move relative. Deliberately replays no board.
- `length_cache::self_play_game_positions(collected, game_index)`: replays the board, and already
  checks that each comment's best move is legal (`best_moves`, from `{<score> <best>}`).
- Both callers replay anyway: `pgn_stream.rs`'s `worker` (training) and `freeze.rs`'s `sample` (the
  frozen test set) call both functions for every game.
- The frozen test set keeps one label file per lambda (`labels_dir(dir, lambda)`), and `forge`
  refuses to start when one is missing.

## Changes

1. **The config.** `label:` gains an optional field, `cut: non-best`; absent means no cut. Read
   beside `label.lambda` (`preprocessing_config.rs`'s `LabelSection`, which denies unknown fields, so
   an old binary refuses a cut config rather than ignoring it). `arena` reads the same section and
   only needs to tolerate the field.
2. **The flags.** `self_play_game_positions` also returns, per ply after the book, whether the move
   played is the comment's best move, compared as moves on the replayed board (SAN played, UCI best),
   never as text. A ply with no best move in its comment (self-play before generation 4) is refused
   when the cut is asked for: it cannot be judged.
3. **The recursion.** A new `labels::compute_labels_cut(scores, cut, outcome, lambda)`, where `cut[t]`
   makes ply `t` take its own score: `score(scores[t])`, or \(\pm1\) when decisive. `compute_labels`
   stays as it is, and a test asserts that `compute_labels_cut` with no cuts equals it on the existing
   fixtures. A cut ply's label still becomes `next` for the ply before it.
4. **The callers.** `self_play_game_labels` takes the flags as an `Option`: `None`, today's path
   through `compute_labels`; `Some`, the cut. `worker` and `sample` replay first, then label.
5. **The frozen test set.** A cut label set is another label set: its own directory beside the
   lambda's (for instance `lambda-0.25-cut`), frozen by `smithy freeze lc0 <network>` like any
   missing lambda, from the same reservoir slots, so its positions are the same rows. The old sets
   are untouched.
6. **The count.** `freeze` prints the share of plies cut, and the share in the first 20 plies and
   after, over its sample. This is step 1 of dynamic-lambda.md, for free, before any training.
7. **The network.** `configs/lc0-psq-cut.yaml`: `lc0-psq`'s config field for field, plus
   `cut: non-best`. Its runs live under `data/networks/lc0-psq-cut`, beside `lc0-psq`'s, which stays.

## Judging it

- **The select:** `select lc0-psq-cut`'s table against `select lc0-psq`'s. The hypothesis predicts
  0.85 and 0.95 no longer clearly worse, and a rise toward the long horizons.
- **The duel:** `lc0-psq-cut-0001` against `lc0-psq-0001`, both untuned, at 10+0.1. Neither has
  `margin.txt`; if one is ever tuned, the other is too before they meet.
- **Test loss** does not compare across the two: each is measured on its own labels.

## Settled (2026-10-19)

- The field: `label.cut: non-best` (`preprocessing_config::Cut`), an enum so another kind of cut
  can be added beside it, with `label.cut_drop`, required.
- **Only costly non-best moves are cut (the developer, 2026-10-19), `cut_drop: 0.10`.** The first
  count (`smithy freeze lc0 lc0-psq-cut --count`): 39.18% of 7.9M plies are not Lc0's best (46.04%
  in the first 20 plies, 37.80% after). The mover's evaluation falls, in Q, over a best move: mean
  0.0106, over 0.02 16.2%, over 0.05 3.6%, over 0.10 0.9%, over 0.20 0.2%; over a non-best move:
  mean 0.0430, over 0.02 45.6%, over 0.05 26.6%, over 0.10 12.1%, over 0.20 3.9%. Most non-best
  moves are near-equal choices, and cutting them all would shorten every horizon to a ply or two;
  above 0.10 they fall 13 times as often as best moves, about one ply in twenty. Label sets are
  named by the drop, `lambda-0.25-cut-0.10`.
- A cut ply takes \(E_p\) alone, as the hypothesis does.
- The freeze counts (`CutTally`, printed by every `smithy freeze`, cut or not); the training stream
  does not.

## Steps for the developer

1. Build and test, once `lc0-psq-threats-pairwise` has stopped training.
2. `smithy freeze lc0 lc0-psq-cut`: adds the ten `lambda-*-cut` label sets, and prints the count,
   step 1 of dynamic-lambda.md. Read it before training.
3. `smithy forge lc0-psq-cut`, then `arena select lc0-psq-cut`, then the duel.

**2026-10-20:** `lc0-psq-threats-pairwise`'s training was suspended to make room, and
`smithy forge lc0-psq-cut` has ended. Next: `arena select lc0-psq-cut`.
