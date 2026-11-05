# psq_threats_screlu_kings

**Superseded, see psq-threats-screlu-kings2.md** (2026-10-26): dropped at round 10; the name
`psq_threats_screlu_kings`, its config and tag 17 now belong to that design. To be archived.

Status: designed 2026-10-25, plan written; step 1 (engine) done, tests passing; step 2 (bench) done,
below; steps 3 (trainer), 4 (config) and 5 (docs) done 2026-10-25, tests passing. The taper drew both
duels (2026-10-26, psq-threats-screlu-taper.md), so this runs; next, step 6: a dev smithy smoke run,
then generation 1 on the training machine.

## The bench (step 2, 2026-10-25, one machine, random networks, width 256, depth 9)

| | head alone | per search node |
|---|---|---|
| psq_threats_screlu | 183.9, 188.8 ns | 225.2, 229.1 ns |
| psq_threats_screlu_kings | 469.1 ns | 510.0 ns (2.3x) |
| psq_threats_screlu_kings, `--one-bucket` | 298.2 ns | 341.1 ns (1.5x) |

The first layer's arithmetic costs about 110 ns an evaluation; the rest of the 469 is the bench's 7
positions cycling through distinct king sets, a 16 KB first layer each, which a search, whose kings
rarely move between nodes, mostly does not pay: its cost is near the one-bucket line, about 1.5x
SCReLU's a node. The machine was not steady (PeSTO 13.1 to 26.0 ns across the runs), so about +/-
20%. **Kept, unoptimized, for now:** the depth 9 duel judges the evaluation alone; only if it wins
clearly is the speed worth work before the 10+0.1 duel, by Stockfish's two means: reading only the
non-zero squares (every lane clamped at 0 squares to exactly 0), and i8 first-layer weights
(`maddubs`, twice the throughput, half the memory). 1.5x a node is likely 15 to 25 Elo at 10+0.1. Runs only if the taper (psq-threats-screlu-taper.md) also shows
no strength a duel can see. Follows psq-threats-stack.md, whose head it reuses, and screlu.md, whose
activation it reuses.

## Dropped at round 10 (the developer, 2026-10-26)

Round 10 (lambda 0.02, rate 3e-3): test loss 0.08028, 7.7% above SCReLU's 0.07453 (phase 0.07442),
the gap grown from 7.2% at round 5 (kings -1.5% over rounds 5 to 10, SCReLU -2.0%). By piece count
against SCReLU: -14% to -54% at 3 to 6 pieces, +3% at 7, +5% to +10% from 8 to 24, +12% to +25%
from 28 up: round 5's pattern. Kings is dropped, its generation 1 stopped; its successor is
psq-threats-screlu-kings2.md, each side read by its own king, with SCReLU's output as a linear skip.

## Round 5, and Stockfish's head (2026-10-26)

**Round 5 (lambda 0.02, rate 3e-3):** test loss 0.08154, 7% above SCReLU's 0.07608 (phase
0.07552), nearly flat (-0.1% from round 4, SCReLU -0.8%); train loss as far behind, so underfitting.
Round 1 was ahead of SCReLU (0.08662 against 0.08837). By piece count against SCReLU: -25% to -54%
at 3 to 6 pieces, +1% at 7, +5% to +12% from 8 up (32: +29%). Hypothesis, untested: the 16-wide
clipped layer is a bottleneck that endgames, where the king's square decides much, fit through,
and middlegames, where many terms matter, do not; SCReLU's output reads all 512 squares directly.

**Stockfish's head (master, read 2026-10-26: `src/nnue/nnue_architecture.h`, `network.cpp`;
nnue-pytorch `layer_stacks.py`, `stacked_linear.py`):**

- **Sizes:** 1024 -> fc_0 32 -> fc_1 32 -> fc_2 1, 32 layer stacks.
- **Two skips around the deep path.** `fwdOut += fc_0_out[30] - fc_0_out[31]`: two of fc_0's raw
  outputs, before any activation, their difference added straight to the final output. And fc_2
  reads fc_0's activations as well as fc_1's: 128 inputs, `FC_0_OUTPUTS * 2 + FC_1_OUTPUTS * 2`.
- **Both activations, concatenated:** after fc_0 and after fc_1, the squared clipped ReLU and the
  clipped ReLU of the same values side by side, so fc_1 reads 64 values from fc_0's 32.
- **The bucket is not the king:** `(pos.count<ALL_PIECES>() - 1) / 4 * 4` plus a bit for each
  side's queen. The king enters through the input (halfka) alone.
- **Training:** one wide `F.linear` over every bucket, then a gather by index, as our
  `stacked_stage_forward`; only the first layer factorised ("later there's a non-linearity and
  factorization breaks"). A grouped CUDA kernel (`grouped_l1`) replaces it on recent GPUs only.

Against ours (512 -> 16 -> 16 -> 1, clipped ReLU only, no skip, both king stages factorised), Stockfish's head
is twice as wide, keeps a linear path to the output, and lets the output read the first layer.

## What (the developer, 2026-10-25)

psq_threats_stack's head over SCReLU's squares instead of pairwise's products, and **without output
buckets**: 512 squares -> 16 by our king's square (64 sets) -> 16 by their king's square (64 sets)
-> 1, a single output set. Trained factorised (a shared set plus a delta per bucket, summed at
export), as the stack is.

## Why

The stack changes four things at once (products, two hidden layers, king buckets, material
buckets). Two of them are measured already, both null:

- **Pairwise products:** level with threats (tests 3 and 4). SCReLU replaces them: it leaned the
  other way (tests 5 and 6, about +12 Elo together), and is the line.
- **Output buckets:** material, phase and count all lowered the loss (phase by 2%) and drew every
  duel against SCReLU, within about +/- 10 Elo of one another (tests 7 to 14).

What is left untested is the stack's own idea: **hidden layers picked by the kings' squares**, the
king brought in through the head, a few thousand weights a bucket, instead of through the input,
where halfk gives each king square about 1/64 of the data. Dropping the output buckets isolates
it, and loses nothing a duel has seen. The two hidden layers also settle TODO.md's open question
of a hidden layer of 16.

## Decisions (the developer, 2026-10-25)

1. **SCReLU, not pairwise,** in front of the first hidden layer: pairwise brought nothing.
2. **Both kings, ours first, then theirs:** the first hidden layer by our king, the second by
   theirs, as psq_threats_stack.
3. **The name: `psq_threats_screlu_kings`** (kind, file tag, `lc0-psq-threats-screlu-kings`).
   "Stack" is kept for the full head, king layers plus a bucketed output: should the taper win, the
   kings over its tapered output would be `psq_threats_screlu_stack`.

4. **Hidden widths 16 and 16,** as the stack. With SCReLU the first layer reads 512 values, not
   pairwise's 256: twice the stack's first-layer work, 512 x 16 multiply-adds a node.
5. **The clipped ReLU after each hidden layer,** as the stack has now (`StackHead::forward` clamps
   after each hidden stage). SCReLU stays on the accumulator alone (screlu.md: the flag applies to
   the transform's output, never to hidden layers).
6. **The bench first:** `nnue_bench` on random networks, psq_threats_screlu against this, on one
   idle machine, before the training machine spends a day. SCReLU's head was lost in the threats'
   recomputation (about 310 ns); a 512 x 16 layer plus a 16 x 16 one per node may not be.

## Design

- **The squares are the stack's input as they are.** A square, `product_of(a, a)`, lies in
  `0..=127` with 1.0 at `PRODUCT_ONE`, exactly a pairwise product's range and scale, so the stack's
  quantized first layer (`quantize`'s stack arm) and `stack_head_value` take them unchanged; only
  their count doubles, 512 in place of 256.
- **Squares stored once, then read by 16 rows.** SCReLU's single output squares each lane as it is
  loaded and stores nothing (screlu.md, pairwise's lesson); with 16 rows to read them, squaring 512
  lanes 16 times would cost more than storing them once in a thread-local scratch, as the hidden
  layer's path (`ACTIVATION_SCRATCH`) already does for its activations. The bench checks it.
- **One output set, unfactorised.** A delta needs buckets to differ by; with one set there is
  nothing to factorise. The engine's stack keeps its `output` list, of length 1, read at index 0.
- **The two king stages factorised,** as the stack's (`l1`, `l2`: shared plus a delta per bucket,
  summed at export), reading the batch's columns 0 and 1, which every psq_threats batch carries.
- **A new feature-set tag, 17, `psq_threats_screlu_kings`:** tag 9's layout over 512 squares with
  one output set, so an older build refuses the file.

## Implementation plan

Each step with its tests; the developer runs them and commits between steps.

1. **Engine** (`crates/rotor/src/nnue.rs`):
   - `FEATURE_SET_PSQ_THREATS_SCRELU_KINGS = 17`, its shape `(psq_threats::ROWS, 1)`, its name;
     `load` reads the stack's layers for it with `screlu` set and one output set; `write` picks 17
     for a screlu stack; `validate` accepts screlu with a stack of exactly one output set, still
     refusing screlu with a hidden MLP, pairwise or any other stack.
   - `buckets`: the kings' squares from `buckets_of_board`, the third place 0 when the stack has one
     output set (no material count needed).
   - `evaluate_screlu_lanes`: for a stack, the 512 squares into a scratch (scalar, or AVX2 when a
     perspective is a multiple of 16 lanes), own first, then `stack_head_value` with `l1[our king]`,
     `l2[their king]`, `output[0]`.
   - The f32 head (`evaluate_accumulators`): clamp, square, then the stack's three layers.
   - Tests, as the stack's and SCReLU's: write and load as tag 17; quantized within tolerance of
     f32; vectorized equal to scalar; incremental equal to from scratch along random games, threat
     lanes saturated and distinct weight sets per king (`sample_stack_network`'s way), so a wrong
     bucket fails; every refused combination.
2. **Bench** (`crates/rotor/benches/nnue_bench.rs`): `--stack --screlu --threats` builds a random
   kings network. Then the developer, on one idle machine, benches psq_threats_screlu against it
   back to back; the numbers go here. **If it costs much more than expected, stop and discuss
   before training.**
3. **Trainer** (`smithy`):
   - `ArchitectureKind::PsqThreatsScreluKings` (`net_config.rs`, `kind: psq_threats_screlu_kings`):
     `screlu()` and `stack()` include it; `hidden_dim` must be two widths.
   - `HeadShape::Kings` (`stack_head.rs`): stages `l1` (column 0) and `l2` (column 1) factorised,
     then `output`, one plain linear layer with no delta; `StackHead::new` builds it, a stage's
     delta made optional.
   - `nnue_network.rs` and `stacked_network.rs`: the head built for it, SCReLU on the transform's
     output as screlu's.
   - Export (`export.rs`): the stack's summing for `l1` and `l2`, the output as is, written as tag
     17; `candidates.rs`' lists.
   - Tests: by hand, one row through its kings' summed weights and the plain output; every stacked
     arm equal to its own network; the engine's forward pass agreeing with tch's through a file.
4. **Config:** `configs/lc0-psq-threats-screlu-kings.yaml`, `lc0-psq-threats-screlu.yaml` with its
   kind, header and `hidden_dim: [16, 16]`; the config tests list it; the page's order of configs
   with it after the taper.
5. **Docs:** architectures.md's entry; TODO.md's item pointing here.
6. Build, test, a dev smithy smoke run, then, if the taper also fails, the run on the training
   machine and the duels.

## The experiment

- `configs/lc0-psq-threats-screlu-kings.yaml`: `lc0-psq-threats-screlu.yaml` with its kind and
  `hidden_dim: [16, 16]`; generation 1 on Lc0's labels, lambdas 0.01 to 0.09, gated against PeSTO.
- Its winner against `lc0-psq-threats-screlu-g1-r1`, both `@default`: a duel at depth 9 (the
  evaluation alone), then, if at least level, at 10+0.1 (its speed cost too).
- Its loss by piece count beside SCReLU's and phase's, recorded, not read as a verdict.
