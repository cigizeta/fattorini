# SCReLU

Status: implemented 2026-10-24, all five steps, tests passing; run and judged the same day, kept
(TODO.md: 52.0% at depth 9, 51.6% at 10+0.1 against threats). Continued by
psq-threats-screlu-material.md.

## What (the developer, 2026-10-23, TODO.md)

The squared clipped ReLU, `clamp(x, 0, 1)^2`, each accumulator lane squared on its own, in place of
the clipped ReLU between the transform and the head: the usual activation of single-layer networks
(bullet's engines). A config differing from `lc0-psq-threats` by its activation alone, judged as
pairwise was: its loss no evidence, a duel at depth 9 against threats, then, if at least level, one
at 10+0.1.

Why it may pay where pairwise did not: our head has no hidden layer (`hidden_dim: []`), so the head is
linear in the activations and every nonlinearity it has comes from the activation. SCReLU makes the
output quadratic in each lane, with a record in single-layer networks; pairwise's gain in Stockfish
is mostly speed, halving a hidden layer's input, which we do not have.

## The function

Per perspective, `a = clamp(W x + b, 0, 1)` as today, then `a * a`, lane by lane; both perspectives
concatenated, side to move first, into the same linear output: `2 * embedding_dim` inputs, as plain
psq_threats, against pairwise's `embedding_dim`. Gradient `2a` inside `(0, 1)`, zero outside, so
lanes near 0 learn slowly: a dead lane stays dead as with the clipped ReLU, and an almost-dead one
contributes almost nothing.

## The integer form: pairwise's arithmetic, a lane times itself

The engine already has everything for it. A pairwise network's halves are quantized to `0..=255`
(`HALF_ONE`, the accumulator scaled by `255 << shift`) and two of them become one head input by
`product_of(a, b) = (a * b + 256) >> 9`, in `0..=127`, whose 1.0 is `PRODUCT_ONE` (255 * 255 / 512).
SCReLU is `product_of(a, a)`: the same scale, the same bound, the same rescale in `output_value`,
the same `quantize` (with `HALF_ONE` for its accumulator). The delicate part named in TODO.md, "a
squared value having another scale", is pairwise's, solved and tested (a product off by under 0.008).

What is new in the engine:
- a `screlu` flag beside `pairwise`, read from a new feature-set tag (`psq_threats_screlu`), so a
  build without it refuses the file;
- the scalar path, `product_of(activation_of(lane), activation_of(lane))` per lane, the reference;
- one AVX2 kernel, `screlu_dot_i16_avx2`: each lane's activation squared and rescaled as it is
  loaded, multiplied by its `i16` weight with `madd`, as `pairwise_dot_i16_avx2` does with two
  lanes; nothing stored between, as pairwise learned (about 15 ns of 54 when stored, another machine,
  2026-10-15);
- the tests pairwise has: f32 head by hand, quantized close to f32, vectorized equal to scalar,
  incremental equal to from scratch with saturated threat lanes, write and load as its own kind.

**Cost in the engine:** the head reads 512 values, as plain psq_threats, plus one square and shift
per lane; pairwise reads 256 products of 512 lanes. So about pairwise's head cost or a little more,
far below the accumulator's update. To measure before training, `nnue_bench` on a random network
of each kind: if the head costs more than expected, it is known before the training machine spends a day.

## The trainer

A `screlu` flag beside `pairwise` (not `Activation::Screlu`, as first planned: the square applies to
the accumulator alone, never to hidden layers, and the stack's one `activation` serves both), applied
to the transform's output in `NetworkPsq::pawns` (the reference) and in `StackedNetwork`'s head
(what trains), exported as pairwise is (`export.rs`, `candidates.rs`: the tag, the flag). `ArchitectureKind::PsqThreatsScrelu`,
`architecture.kind: psq_threats_screlu`, same data as psq_threats.

## The experiment

- `configs/lc0-psq-threats-screlu.yaml`: `lc0-psq-threats.yaml` with its kind changed, nothing else;
  generation 1 on Lc0's labels, lambdas 0.01 to 0.09, gated against PeSTO; on the training machine.
- Its winner against `lc0-psq-threats-g1-r1`, both `@default` (neither tuned: a tuned side against
  an untuned one is never fair, TODO.md), from the page's tests: a duel at depth 9 (the evaluation
  alone), then, if at least level, a duel at 10+0.1 (its speed cost too).
- **Judged by the games alone** (pairwise's lesson, 2026-10-23: a 0.6% lower loss gave nothing
  visible). Its loss is recorded beside threats', not read as a verdict.

## Decisions (the developer, 2026-10-24)

1. **Scope: psq_threats with no hidden layer, nothing else.** Every other combination (psq rows, a
   hidden layer, pairwise, a stack, material sets) is refused, in the trainer's config and in the
   engine's file check. Extended only if it wins.
2. **The name: `psq_threats_screlu`**, the kind, the file tag and, as `lc0-psq-threats-screlu`, the
   config.
3. **The bench first**, before the training machine trains: the head's cost known in minutes.

## Implementation plan

Each step with its tests; the developer runs them and commits between steps.

1. **The engine** (`rotor/src/nnue.rs`, `rotor/src/simd.rs`):
   - `FEATURE_SET_PSQ_THREATS_SCRELU`, its name `psq_threats_screlu`, read and written; `PsqNetwork`
     gains `screlu: bool`. The file check refuses screlu without threats, with pairwise, a hidden
     layer, a stack or material sets.
   - The f32 head (`evaluate_accumulators`): each perspective's lanes clamped to `[0, 1]` and
     squared, own first, into the output.
   - `quantize`: the accumulator at `HALF_ONE` as pairwise's, the head's input 1.0 `PRODUCT_ONE`.
   - `evaluate_lanes`: scalar, `product_of(a, a)` per lane into the output's dot; vectorized,
     `screlu_dot_i16_avx2`, a lane's activation squared and rescaled as it is loaded (pairwise's
     `mullo` and shift, one lane for both factors), `madd`ed with its weight, nothing stored.
   - Tests, as pairwise's: the f32 head against a hand computation; quantized within pairwise's
     tolerance of f32; vectorized equal to scalar; incremental equal to from scratch along random
     games, threat lanes saturated; write and load as its own kind; every refused combination.
2. **The bench** (`nnue_bench.rs`): `--screlu`, with `--threats` and `--head ""` only. Then the
   developer, on an idle machine, benches threats, threats pairwise and threats screlu at width 256,
   and the numbers go here.
3. **The trainer** (`smithy`): `ArchitectureKind::PsqThreatsScrelu` (`net_config.rs`), refused with
   a hidden layer; `Activation::Screlu` (`value_head.rs`), the clipped ReLU squared; `NetworkPsq`
   with it on the transform's output (`nnue_network.rs`, the reference) and the same in
   `StackedNetwork`'s head (what trains); the export (`export.rs`, `candidates.rs`) writing the tag.
   Tests: every arm of the stack computes what its own network does; the engine's forward pass agrees
   with tch's through a file; the gradient is `2a` inside the clamp.
4. **The config**: `configs/lc0-psq-threats-screlu.yaml`, `lc0-psq-threats.yaml` with its kind and
   header changed, nothing else; the page's order of configs (`page.html`) with it after
   psq-threats-pairwise.
5. **The docs**: architectures.md's entry; TODO.md's SCReLU item pointing here.

## The bench (step 2, another machine, 2026-10-24)

Random networks, width 256, no hidden layer, `nnue_bench --depth 9`, one after the other on another machine:

| | head alone | update | per search node | search, per node |
|---|---|---|---|---|
| psq_threats | 314.9 ns | 73.0 ns | 388.0 ns | 1.12 us |
| psq_threats_pairwise | 315.5 ns | 56.0 ns | 371.6 ns | 1.10 us |
| psq_threats_screlu | 308.5 ns | 54.6 ns | 363.0 ns | 1.16 us |

**SCReLU costs nothing measurable.** The head is about 310 ns for all three because recomputing the
threats dominates it; the head's own arithmetic (512 squares and a dot) is lost in that. The
incremental update, the same code in all three, moves by 18 ns between runs: that is the noise, and
every difference above is inside it. So the 10+0.1 duel will judge the evaluation, not a speed
cost.

Then the run, on the training machine: generation 1 of `lc0-psq-threats-screlu`, and the two duels above.
