# Why a lower test loss does not make a stronger engine

**Status:** answered 2026-10-28: the sibling ordering after quiescence (sibling-ranking.md); the
remedy under discussion in settled-positions.md. Read "Where it stands" and what follows it first. The
tool is `crates/smithy/src/engine_loss.rs` (engine-loss.md says how to run it).

## The question

Since generation 1 on Lc0's labels, the test loss has ranked the variants, and the duels have not
followed it. pairwise_hidden ended 5.5% to 7.9% below SCReLU and lost both duels, about 42%, roughly
-55 Elo at depth 9. If the link between loss and strength is broken somewhere in the pipeline, every
later decision made from losses (including width 512's) is suspect.

## The record, loss against duel (both `@default`)

| variant against its baseline | test loss | depth 9 | 10+0.1 |
|---|---|---|---|
| threats against psq | about -5% | | 57.3%, about +51 |
| SCReLU against threats | -1% | 52.0% | 51.6% |
| pairwise against threats | -0.6% | 48.9% | 46.6% |
| phase against SCReLU | -2% | 51.9% | 47.9% |
| count against SCReLU | -0.6% | 49.9% | 48.3% |
| taper against SCReLU | -0.1% | 49.5% | 49.0% |
| pairwise_hidden against SCReLU | -5.5% to -7.9% | 42.4% | 42.0% |

Each duel's error is about +/- 20 Elo. Calibrated on threats against psq, about 10 Elo for each 1%
of loss; by that rule SCReLU (+10), pairwise (+6), count (+6) and taper (+1) are all where their
duels put them, within noise. Two rows need explaining: phase (+20 predicted, about 0 measured) and
pairwise_hidden (+55 to +80 predicted, about -55 measured).

## What is already known (2026-10-27)

- **The duels play with 3 to 6 piece Syzygy tables,** WDL and DTZ (`arena/src/data_root.rs`), and a
  game may end on a tablebase result. A gain at 3 to 6 pieces cannot show in a duel.
- **Phase's gain sat mostly there:** -9% to -34% at 3 to 6 pieces, -2% to -4% at 7 to 13, -0.5% to
  -1.5% from 14 up (TODO.md). Counted from 7 pieces up, its gain is about 1% to 2%, +10 to +20
  Elo by the rule above, within a duel's noise. Count's and taper's gains were almost all at 3 to
  7 pieces. **The tablebases likely explain the bucket variants' nulls:** the loss over every
  piece count overstated them.
- **pairwise_hidden's gain is everywhere** (`pieces.tsv`, lambda 0.03, each run's last round,
  against SCReLU's): -27% to -46% at 3 to 6 pieces, -7% to -12% at 7 to 16, -3% to -5% from 17 up.
  The tablebases explain none of its loss: its network fits Lc0's labels better at every stage of
  the game and plays worse. **This is the real anomaly.**
- **Quantization is not all of it:** the hidden layer in f32 (hidden-beside-f32.md) halved the
  quantization tail and still scored 44.9% at depth 9.
- Every hidden layer on psq inputs has lost at equal depth, whatever its loss (betty, dorothy,
  pairwise_hidden).

**A correction (2026-10-27):** the pairwise_hidden that played the duels, `-g1-r1`, is the winner
of its round robin, **lambda 0.01** (`generations.tsv`), not the lambda 0.03 candidate every
`engine_loss` measurement below used until then; SCReLU's winner is 0.02, pairwise's 0.03. The
measurements below that compare pairwise_hidden are of the wrong network, and compared losses on
different lambdas' labels; the duel-free ones (export, quantization, the engine's path) still
hold. Redone on lambda 0.01, both scored on lambda 0.02's labels (`ROTOR_ENGINE_LOSS_LAMBDA`).

## Hypotheses for pairwise_hidden, cheapest test first

1. **The loss is measured in float, the engine plays integers.** **Refuted 2026-10-27
   (engine-loss.md):** as the engine computes it, pairwise_hidden's loss is still 6.7% below
   SCReLU's (0.060983 against 0.065343), quantization costing it 0.02% and SCReLU 0.33%. The
   export is exact. The network the engine runs fits Lc0's labels far better and plays worse.
2. **The search does not evaluate the positions the loss is measured on.** **A sharper form
   (2026-10-27):** training filters nothing, so positions in check or with a winning capture pending
   are trained and tested on, their labels holding the tactic's outcome. A network with more
   capacity can learn to predict it from the static position and lower its loss, while the search
   never reads an evaluation in check and plays captures out in quiescence: an evaluation that has
   priced the capture in counts it twice. Stockfish's trainers drop such positions. Test: the loss
   split into quiet, winning capture (SEE > 0) and in check (`engine_loss`). Lc0's positions are
   from games; the search evaluates leaves of a tree, after quiescence, many of them absurd. A
   hidden layer may fit the game positions better and extrapolate worse. Test: a loss over
   positions the search evaluates, for instance the duels' own positions (kept with their games in
   `data/tests/`) scored against each game's result, or leaves sampled during a search, labelled by
   a deeper search.
3. **The evaluation's scale meets untuned margins.** Both sides play `@default`, whose pruning
   margins are fixed in centipawns. A network whose evaluations spread differently (more extreme,
   or noisier between siblings) prunes differently under the same margins. Test: the distribution
   of |evaluation| and of the difference between a position and its children, both networks over
   the same positions; then, if they differ, a duel with both sides tuned.
   **Measured 2026-10-27 (engine-loss.md):** both networks' pawn scale 40%, so the same margins
   in pawns; but pairwise_hidden's evaluations are compressed near zero (median 0.161 pawns
   against 0.210 from 17 pieces up, 0.313 against 0.446 at 7 to 16) and heavier in the opening's
   tail (p99 6.15 pawns against 2.51 from 17 pieces up). Confident evaluations, if sometimes
   wrong, make cutoffs that hide the lines that would correct them. Not proved: the test is a duel
   with every evaluation-based margin set wide for both sides. **The duel (2026-10-27):** `arena
   duel` at depth 9, both sides `@20000`, `SearchParameters::with_margin`'s five margins: 80
   pawns a ply after the 40% pawn scale, beyond the 64 between two clamped evaluations, so razoring,
   both futilities and delta never fire and null move keeps its base reduction alone; SEE pruning,
   history pruning and the aspiration window, which read no evaluation's size, unchanged. Against
   pairwise_hidden's 44.9% at depth 9 with the default margins (hidden-beside-f32.md's replay): if
   it now wins, the evaluation is better and the margins lose it, and a tune per network is the
   answer; if it still loses, hypothesis 2. **Result: SCReLU 504 to 452 of 956, pairwise_hidden
   47.3%, about -19 Elo, drawn (curtailed).** Against 44.9% (-36) with the default margins, +17
   Elo at most, within two duels' noise. The margins explain part of the deficit at most:
   **hypothesis 3 is not the main cause.**
   **Hypothesis 2's sharper form, measured 2026-10-27:** no. pairwise_hidden's gain is as large on quiet positions (-7.4%,
   -8.1% at 7 to 16 pieces) as on winning captures (-5.6%) and in check (-6.9%). Aside: the 29% of
   positions in check or with a winning capture carry twice a quiet one's loss, about 45% of the
   total.
5. **The search's evaluation is not the one measured (2026-10-27).** Both pairwise networks
   underperform their loss: plain pairwise lost to SCReLU by about 38 Elo at depth 9 with a loss
   about 0.4% worse, where the rule above predicts about 4. `engine_loss` measures the scalar
   from-scratch path (`evaluate_pawns`); the search reads accumulators updated move by move
   through the vectorized kernels (`update`, `evaluate_by_color_cp`), tested equal on random
   networks only. Test: `engine_loss` checks that path against the refresh along every line of two
   plies from one test position in 512, on the trained networks. **Refuted 2026-10-27:** 7,296,201
   evaluations each for SCReLU, pairwise and pairwise_hidden, none differing. Plain pairwise's
   quantized loss: 0.065430 overall (+0.13% against SCReLU's), 0.049178 on quiet positions (-0.1%):
   it fits the positions the search uses as well as SCReLU and lost to it by about 38 Elo. **The
   loss on this test set, quiet positions included, does not tell these networks apart by
   strength.**
6. **What the search needs is ranking, not fit.** **The test (the developer, 2026-10-27): move
   agreement,** in `engine_loss`. Lc0's test games carry, beside each position's score, the best
   move of Lc0's search (`lc0/convert.rs`). Over quiet positions of those games (not in check, no
   winning capture), each legal move is scored by the network as the search would at its shallowest:
   its child's quiescence (captures only, stand pat, MVV-LVA order), negated; agreement is the share
   of positions where the network's best is Lc0's. If SCReLU agrees more often than both pairwise
   networks, ranking explains the duels, and agreement becomes the screening measure before any
   duel, 512's included. **Result 2026-10-27,** 20,000 quiet positions, the same for each network:
   SCReLU 29.85%, pairwise 28.86% (-1.0 point, -1.5 from 17 pieces up), pairwise_hidden 30.43%
   (+0.6). **Plain pairwise is explained:** its quiet loss equal to SCReLU's, its ranking worse, as
   its duel. **pairwise_hidden is not:** better loss and better shallow ranking, and still lost; its
   weakness shows only inside a deeper search. Alpha-beta compares siblings; a lower mean error
   can come from better calibration with no better ordering. Test: over pairs of sibling positions
   from the test set's games, how often each network orders them as the label does.

## Where it stands (2026-10-27)

- **The duel network measured:** lambda 0.01, on lambda 0.02's labels against SCReLU's winner:
  quantized loss 0.060268 against 0.065343 (-7.8%), quiet -8.8%, agreement with Lc0's move
  30.81% against 29.85% at quiescence and 36.86% against 35.69% through the engine's search at
  depth 7. Better on every measure on Lc0's positions.
- **The lambda is not the cause:** lambda 0.03's candidate against SCReLU's winner, depth 9, both
  `@default`: 223.5 to 162.5 of 386, 42.1%, about -55 Elo, resolved.
- **Its own games show the weakness** (`engine_loss::tests::duel_games`, the `@20000` duel's 956
  games): its evaluation fell a pawn or more between two of its own moves in 3.40% of its moves
  against SCReLU's 2.26%, 50% more often; it reached a position it judged lost (-2) in 406 games
  against 338; conversion and rescue no worse. Its search, led by its evaluation, walks into
  problems it sees one move late.
- **So hypothesis 2 in its first form:** what decides Rotor's games are positions Rotor's own
  games reach (the arena's book openings, played out at depth 9), and there the hidden layer
  generalizes worse than SCReLU, while Lc0's game positions, the test set's, do not show it. Loss
  and move agreement on Lc0's positions cannot screen an architecture; only games can.
- **For what follows:** 512's verdict stays with its duels, its loss only a sanity check. A
  generation 2, trained on Rotor's own self-play, trains on the positions Rotor's games reach, the
  remedy this points to; whether it closes a hidden layer's gap is a question for after 512.
  A measure on Rotor's positions (its duel games' positions, scored by a deeper search) would be
  the screening tool this lacks.

## The shift, likely but not proved (2026-10-28)

On the positions of a new depth 9 duel between the two (rotor-positions-loss.md), scored against
each game's result: from 7 pieces up, SCReLU's loss 2.4% +/- 2.1% below pairwise_hidden's, 3.9% +/-
2.2% where pairwise_hidden is to move, where on Lc0's test positions pairwise_hidden led by 7.8%.
**But the control, plain pairwise, moved the same way** (from level to 1.7% behind, after losing its
duel): a network scored on its own duel's games is biased toward the winner. pairwise_hidden's
swing, about 10 points against pairwise's 2, still points to a shift; the clean test scores both on
games neither played. The section below was that test's plan, run on new duels rather than these
PGNs.

**The clean test, the same day:** on a phase against count duel's games, from 7 pieces up, both
pairwise networks level with SCReLU (+0.36% +/- 0.87%, -0.35% +/- 0.77%). pairwise_hidden's Lc0
lead does not carry over (the shift, in that sense, real), but its deficit on its own duel was the
selection, and fit explains neither pairwise network's lost duels: back to the search, hypothesis
6's sibling ranking. Calibrated: threats 3.2% +/- 0.8% below psq on the same games, so the measure
sees a +51 Elo gap and the pairwise networks' "level" is level.

**Hypothesis 6 confirmed the same day (sibling-ranking.md):** against Stockfish's ordering of every
move's child, at quiescence, the five networks rank in their duels' order, SCReLU 80.95%, threats
80.44%, pairwise 79.61%, pairwise_hidden 79.40%, psq 77.05%, about 15 Elo a point from threats
against psq. pairwise_hidden orders best statically and loses it after the captures: its weakness is
the positions quiescence lands on. Concordance at quiescence is the screening measure.

**Amended 2026-11-04:** not alone. `-quiet` beat g1-r1 at quiescence and lost its duels, being worse
statically (sibling-ranking.md, quiet-filter.md): the screen needs both concordances, the static one
not falling behind.

## Next (agreed in principle 2026-10-27, done 2026-10-28)

**The direct test of the shift:** both networks' loss on Rotor's own positions. The `@20000` duel's
956 games are in
`data/matches/lc0-psq-threats-screlu-g1-r1@20000-vs-lc0-psq-threats-pairwise-hidden-g1-r1@20000/20261027-1759/games.pgn`
(local, gitignored), and the `@default` lambda 0.03 duel's 386 in
`data/matches/lc0-psq-threats-screlu-g1-r1@default-vs-lc0-psq-threats-pairwise-hidden-g1-a1-l0.03-r1@default/20261027-1825/games.pgn`.
Extend `duel_games`: replay each game from its `[FEN]` (`pgn_replay::try_resolve_san`, as
`game_labels::self_play_game_best_moves` does), and score every position with both networks,
quantized, against the game's result from the side to move (the label at lambda 1,
`rotor::labels::outcome_label`), positions in check left out; about 120,000 positions, the same for
both. If pairwise_hidden's loss there is worse than SCReLU's, reversing its 7.8% lead on Lc0's
positions, the shift is confirmed; if it still leads, an evaluation noisier everywhere is the
likelier reading. Two networks in one run: a second variable, `ROTOR_ENGINE_LOSS_OTHER`, or the
two players' files resolved from the PGN's names (`players::resolve_network_file`).

## Open

- Whether to measure, from now on, the loss from 7 pieces up beside the overall loss, since that is
  what the duels can see.
- Whether 512's verdict should wait on hypothesis 1, since it is cheap and would also check 512's
  quantization at full scale.
