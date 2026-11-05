# psq_threats_pairwise_hidden

**Status:** designed 2026-10-26; plan written. Step 1 (engine) done the same day, rotor's tests
and clippy passing; step 2 benched (below). Steps 3 (trainer, export: screlu_hidden's head shared,
`Architecture::hidden_beside`, `export::hidden_beside_network_from`), 4 (config) and 5 (docs)
done the same day, tests and clippy passing. Next: generation 1 after screlu_hidden's. Step 1: screlu_hidden's head generalised (`hidden_beside_head`, `QuantizedHead::HiddenBeside`), a
pairwise network taking it when its output reads `2 * width + embedding_dim` values, the plain
pairwise hidden layer (tag 8) unchanged.

## The bench (step 2, 2026-10-26, one machine, random networks, width 256, depth 9, two runs each)

| | head alone | per search node | parameters |
|---|---|---|---|
| psq_threats_pairwise | 140.7, 146.5 ns | 177.2, 181.6 ns | 1,248,769 |
| psq_threats_pairwise_hidden | 223.6, 212.8 ns | 260.6, 247.6 ns (1.4x) | 1,252,913 |
| psq_threats_screlu_hidden | 266.3, 257.0 ns | 301.8, 292.5 ns | 1,257,265 |

The hidden layer costs pairwise about 75 ns of head, against about 115 for screlu (its own row
here against screlu's 147 ns, psq-threats-screlu-hidden.md): half the inputs, not half the time,
the products and the skip's kernel weighing the same as before. A node about 15% cheaper than
screlu_hidden's.

## Generation 1 (2026-10-26)

**Rounds 1 to 26, all at 3e-3 (lambda 0.02):** test loss against SCReLU's at the same round -5.3%,
-3.7%, -4.1% (rounds 1 to 3), -4.6% at 10, -5.3% at 20, -5.9% at 26 (0.06945 against 0.07379);
lambda 0.01 -5.4% at 26. The gap widening: SCReLU flat at 3e-3 from about round 20, pairwise_hidden
still descending. Already at 3e-3 below SCReLU's 1e-3 stage until its round 47; SCReLU's final
0.06513 is the bar the later stages must clear. Against kings detached at round 9: -2.9%. About 95 s
a round, SCReLU's 91.5.

**By piece count at round 26 (lambda 0.02), against SCReLU's round 26:** -33%, -53%, -37%, -24% at
3 to 6 pieces; -8% to -9% at 7 and 8; -5% to -7% from 9 to 14; -3% to -4% from 15 to 28; -4% to -9%
at 29 to 32. Below everywhere. Unlike kings (round 9: -14% to -19% at 3 to 5, but -0.1% to -1.6%
from 7 to 30), the middlegame gains too, 3% to 4% steadily: the hidden layer helps beyond the
endgames. Against SCReLU's final network (round 197, after every stage): already 16% to 38% below at
3 to 6 pieces and level at 7 and 8, still 3% to 17% above from 9 up, rising with the count, the gap
the later stages must close.

**Training ended (2026-10-26), each lambda's candidate against SCReLU's:** 0.01 -7.8% (0.06014
against 0.06521), 0.02 -6.9%, 0.03 -6.7%, 0.04 -5.5%, 0.05 -7.9%, 0.06 -7.2%, 0.07 -5.9%, 0.08 -5.9%,
0.09 -7.1%: -5.5% to -7.9%, about -6.8% on average, below at every lambda. The round robin next,
then the gate, then the duels against `lc0-psq-threats-screlu-g1-r1`, depth 9 then 10+0.1.

**Round robin, gate, duels (2026-10-26):** round robin flat (lambda 0.06 and 0.01 tied near 51.5%,
0.07 alone clearly behind at 46.5%); the gate against PeSTO passed. **Both duels lost** against
`lc0-psq-threats-screlu-g1-r1`, both `@default`: depth 9, 185 of 436 (42.4%, about -53 Elo); 10+0.1,
158 of 376 (42.0%, about -56 Elo) (tests 17 and 18). Depth 9 searches the same tree for both, so
speed is not the cause: the evaluation itself plays worse, despite a test loss about 7% lower.

**To resume (next session), in this order:**
1. **Quantization:** does the engine compute what was trained? Tag 19's quantized head was tested on
   small random networks only. `cargo bench --bench nnue_bench -- --network
   lc0-psq-threats-pairwise-hidden-g1-r1 --quantization
   data/gen/lc0-psq-threats-pairwise-hidden/run-1/gen-1/attempt-1`: a few centipawns, as earlier
   networks, rules it out; more means a bug or a precision limit in `QuantizedHead::HiddenBeside`.
   **Done 2026-10-27, not the main cause, its tail heavier.** The run's folder is on Drive only
   here, so the bench was given the `.nnue` file and `round-robin.pgn` by absolute path (the
   folder alone holds no `games.pgn` below it). Over the round robin's 3,242,784 positions not in
   check, |quantized - f32| in cp, lambda 0.03 against SCReLU's winner (lambda 0.02): mean 4.20
   against 3.65, p50 0.92 against 1.70, p90 11.37 against 9.32, p99 49.55 against 26.13, max 226.85
   against 76.51; 20 cp or more 5.1% against 1.2%. The bulk is as good as SCReLU's, which plays
   well with it; the tail is about four times heavier, likely the 16 lanes near their clip (a small
   error squared) or a rounding between the two activations. Unlikely alone to cost about 50 Elo;
   worth finding in the integer head, which every hidden head shares.
   **The tail's cause, found 2026-10-27 (`--weights`, now printing the head beside the output):**
   the hidden layer reads the same 7-bit products as the skip (`product_of`, 0 to 127), and its
   rounding error is magnified on the way out. The skip's weights are small (|w| p50 0.14, max 1.0),
   so its 256 rounding errors average out; the head's weights on the 16 values reach 10 to 11 (row
   10: 5.76 clipped, 10.06 squared; row 12: -7.41, -11.31; rows 2, 4, 9, 13, 15 at 5 to 9 squared),
   on rows whose own weights are the largest (rows 8 to 13, max |w| 3 to 5.4). The output's slope in
   a row's sum is up to `|w clipped| + 2 |w squared|`, about 30 for row 12, against at most 1 for a
   skip input. Rows 10 and 12 nearly cancel each other, large opposite terms whose errors add while
   their values cancel. The engine plays the quantized network, so the duels played this tail too.
   A fix would give the hidden layer finer input (the products before `>> 9`, or the layer in f32),
   at some speed cost; not designed yet.
   **Confirmed the same day (`--hidden-precision`, simulated in f32 from the f32 network, lanes at
   8 bits, weights f32), |variant - f32| in cp over the same 3,242,784 positions:**

   | variant | mean | p50 | p90 | p99 | max | >= 20 cp |
   |---|---|---|---|---|---|---|
   | engine (7 bits into both) | 3.45 | 0.70 | 9.19 | 42.35 | 192.44 | 4.2% |
   | hidden layer's input whole | 1.23 | 0.44 | 3.01 | 13.26 | 61.70 | 0.3% |
   | skip's input whole | 3.29 | 0.51 | 8.95 | 42.01 | 192.23 | 4.1% |

   The simulation reproduces most of the engine's tail (its p99 49.55, 5.1%, the rest the weights'
   and accumulator's own rounding). The hidden layer's 7-bit input is the cause: given the whole
   product, p99 falls to 13 cp and the 20 cp share fourteenfold, below SCReLU's own 26 cp and 1.2%;
   the skip's input changes nothing.
2. **The pawn scale:** `--label-scale` for both networks (`lc0-psq-threats-pairwise-hidden-g1-r1`,
   `lc0-psq-threats-screlu-g1-r1`): a scale far from SCReLU's would distort every margin.
   **Done 2026-10-27, clean.** Over the bench's 10,000 positions, both searched at 40%: `100 / c`
   40.5% (residual RMS 0.2296) against SCReLU's 37.9% (0.2250), each bin's `s_net` within a few
   hundredths of SCReLU's. The slope `p`, 88.7% against 51.2%, fits worse than `c` for both
   (0.2725, 0.2302) and is not read. Neither scale distorts the margins.
3. **If both are clean: betty again.** Lower loss on Lc0's positions, worse in play: the hidden layer
   fits the training distribution and extrapolates worse in the search's positions. A result, not a
   bug; then rethink the grid (TODO.md) and the release before going further.
   **Reached 2026-10-27, one last check decided (the developer):** screlu-hidden's generation 1
   (psq-threats-screlu-hidden.md), to tell whether the lesson holds for the hidden layer itself or
   for pairwise's products only. If it too has a lower loss and loses its duels against
   `lc0-psq-threats-screlu-g1-r1`, the hidden layers are dropped (the grid, kings2 with them) and
   the release ships SCReLU. **Dropped later that day (the developer):** the psq line ends at
   SCReLU, screlu_hidden's run not made; hidden-beside-f32.md has the fixed engine's replay
   (44.9%) and dorothy's check.

## Why

psq_threats_screlu_hidden (psq-threats-screlu-hidden.md) puts a detached hidden layer of 16 beside
SCReLU's output. Pairwise drew against SCReLU without a hidden layer, but in Stockfish its gain is
partly speed: it halves the input of the first hidden layer, and ours had none to halve. With the
hidden layer that changes, so pairwise is to be tried too (the developer, 2026-10-26).

## Decisions (the developer, 2026-10-26)

1. **screlu_hidden with SCReLU replaced by pairwise** ("you just replace a screlu with a pairwise"):
   the same head over pairwise's 256 products (each side's clipped 256 lanes split in two halves of
   128 multiplied element-wise) in place of SCReLU's 512 squares. Everything else as screlu_hidden:
   the skip, the detach, both activations after the 16.
2. **Its own tag, 19, `psq_threats_pairwise_hidden`** (`ArchitectureKind::PsqThreatsPairwiseHidden`,
   config `configs/lc0-psq-threats-pairwise-hidden.yaml`, lc0-psq-threats-pairwise's with its kind
   and `hidden_dim: [16]`).
3. **Order:** after screlu_hidden's generation 1; compared round by round with it, with pairwise's
   and with SCReLU's.

## Design

```
256 products --(detached)--> 16 units, clipped, and squared beside them --> 32 values --+
     |                                                                                   +--> 1
     +-----------------------------------------------------------------------------------+
```

- The output reads 32 values and the 256 products, 288 inputs, one bias; the skip is pairwise's
  own output.
- **Trainer:** `value_head.hidden0` (256 -> 16), `value_head.output` (288 -> 1); the detach
  `stack_head::kings_stage_input`, the output's input `hidden_output_input` over the products.
- **Engine:** screlu_hidden's head, its input the products at a product's scale (pairwise's
  integer arithmetic) instead of the squares.
- **Expected cost:** the hidden layer 256 x 16 = 4,096 multiply-adds, half screlu_hidden's.

## Implementation plan

Each step with its tests; the developer runs them and commits between steps.

1. **Engine** (`crates/rotor/src/nnue.rs`): tag 19, its name and shape; `validate` accepting one
   hidden layer on a pairwise network for 19 alone; the f32 reference and the quantized head,
   sharing screlu_hidden's (`QuantizedHead::ScreluHidden` generalised over squares or products
   where that stays simple); load and write. Tests as screlu_hidden's.
2. **Bench:** `--threats --pairwise --beside --head "16"` builds one (without `--beside`, the
   same `--head` is the plain hidden layer, tag 8's); benched against pairwise and screlu_hidden,
   back to back.
3. **Trainer** (`smithy`): the kind, its head in `nnue_network.rs` and the stacked network over the
   products, the export writing 19. Tests as screlu_hidden's.
4. **Config:** `configs/lc0-psq-threats-pairwise-hidden.yaml`; the config tests.
5. **Docs:** architectures.md, TODO.md.
6. Generation 1 after screlu_hidden's; judged by loss against it at the same rounds; if close, a
   duel.
