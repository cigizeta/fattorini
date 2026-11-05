# psq_threats_screlu at width 512

**Status:** plan approved 2026-10-27 (the developer). Steps 1 and 2 done the same day; generation 1
waits for the training machine.

**Step 2, the dev smoke run (scale 1024):** clean through training, export, quantization and the
round robin (864 games, lambda 0.03 first); the gate against PeSTO failed, 2.5 of 34. Not the
width's doing: the candidate's quantization is clean (`--quantization` over the round robin's
58,915 positions, mean 0.64 cp, p99 3.31, max 5.36), its pawn scale 25%, a random network's, and
the control, `lc0-psq-threats-screlu` at width 256 on the same dev tree, failed its gate alike, 3.0
of 34. At 1/1024 of the training no Lc0 network beats PeSTO at depth 4: a dev Lc0 run's gate says
nothing about the network.

## Why

The psq line ends at `lc0-psq-threats-screlu` (the developer, 2026-10-27): every hidden layer lost
at equal depth (betty, dorothy, pairwise_hidden), and SCReLU beat pairwise directly (55.4%, about
+38 Elo at depth 9). Two checks remain before the line is closed: a generation 2 on self-play, and
the accumulator's width, 256 per perspective since caroline. Single-layer SCReLU networks elsewhere
run 512 to 1536 wide; at 256 the network may be short of capacity rather than of architecture, and
width, unlike a hidden layer, is capacity in a form known to work at this scale.

## Decisions (proposed)

1. **512 first, and last.** Doubles the transform, a clear step. Nothing wider, whatever its result
   (the developer, 2026-10-27).
2. **A config differing by the width alone:** `configs/lc0-psq-threats-screlu-512.yaml`,
   `lc0-psq-threats-screlu.yaml` with `embedding_dim: 512`: the same grid (centre 0.05, 0.01 to
   0.09), schedule, batch sizes and corpus, so its losses compare round by round with SCReLU's run 1.
   No engine change and no tag: the width is in the file, and nothing in the engine or the trainer
   ties it to 256 (the engine's kernels need multiples of 16, pairwise an even width).
3. **Before generation 2.** Both need the training machine. If 512 wins, generation 2's self-play
   starts from the wider network; done the other way round, a self-play cycle would be spent on the
   network about to be replaced.
4. **Judged as every variant:** test loss by round and by piece count against SCReLU's, then a duel
   at depth 9 (the evaluation alone), then at 10+0.1 (its speed included), both `@default`, against
   `lc0-psq-threats-screlu-g1-r1`. The 10+0.1 duel decides, since this is the first variant whose
   speed cost is large enough to count.

## Cost

**Engine, benched 2026-10-27** (random networks, `--threats --screlu --head ""`, depth 9, two runs
each, one idle machine):

| width | update | head alone | evaluation a node | search, a node |
|---|---|---|---|---|
| 256 | 33.4, 35.7 ns | 143.7, 152.2 ns | 177.1, 187.9 ns | 0.74, 0.73 us |
| 512 | 49.5, 49.8 ns | 205.0, 206.3 ns | 254.5, 256.1 ns | 0.85, 0.82 us |

The evaluation about 40% dearer, not twice: the head's threat rows are recomputed at the same cost
at either width. The search a node about 14% slower, roughly a fifth of a ply: on the order of 10 to
15 Elo at 10+0.1, the bar the wider evaluation must clear.

**Training:** the transform 1.25M parameters to 2.5M; round time guessed 1.3 to 2 times SCReLU's
91.4 s, measured at round 1. About 1B positions should suffice at 512; engines at 1024 and beyond
usually train on several billion.

## Steps

1. **Config:** `configs/lc0-psq-threats-screlu-512.yaml`, its header saying what it differs by.
2. **Checks:** `cargo test -p smithy` for the config's parsing (`net_config.rs`), and a dev smithy
   smoke run of the config at the default scale, end to end through export, quantization and the
   gate, catching anything that assumes 256 in a path the bench does not take.
3. **Generation 1 on the training machine,** compared with SCReLU's run 1 at rounds 1, 10, 20 and at
   the end, overall and by piece count (`pieces.tsv`).
4. **Duels** against `lc0-psq-threats-screlu-g1-r1`, depth 9, then 10+0.1.
5. **Docs:** this plan's results; TODO.md (the psq line's last checks).

## Expectations, set before the result (the developer, 2026-10-27)

- **512 beats 256 at depth 9,** the evaluation alone: twice the capacity on the same data. At
  10+0.1 a win is hoped for but not certain, the search a node about 14% slower (above).
- **512 tuned beats 512 at `@default`:** the defaults are hand-set, the margins untuned. A tune that
  gains nothing points to the tune's setup, not to good defaults.

**If 512 does not beat 256 at depth 9, the checks, in order:**

1. **Quantization:** `--quantization` over a round robin's positions, its tail against 256's.
2. **The pawn scale,** against 256's.
3. **Patience** (`patience_rounds: 6`, the same as 256's): whether each learning rate was left
   while the test loss still fell, read from `history.tsv` at each rate's last rounds. A wider
   network may need longer at each rate; if so, generation 1 again with more patience.
4. **The data:** the positions each lambda trained on (its steps times 4096) against Lc0's corpus,
   about 1B. Near or past one pass, and the loss still falling at its last rounds, 512 may want more
   of Lc0's data than is converted today. **256 already went past it:** its lambdas trained 146 to
   197 rounds of 4096 x 4096 positions, about 2.4 to 3.3 billion, 2.4 to 3.3 passes over the corpus.

## Open

- Whether 512 wins at depth 9 but loses at 10+0.1: then the evaluation is better but not by enough,
  and the question becomes the engine's speed (the threat rows' recomputation, the update), not the
  width.
