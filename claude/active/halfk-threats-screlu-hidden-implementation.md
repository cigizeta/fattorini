# halfk_threats_screlu_hidden: implementation plan

Status: written 2026-11-02; steps 1 and 2 applied together the same day, to test (a loaded network
measures its pawn scale at once, so the format could not load without the quantized head). As
built: `QuantizedHead::TwoHidden { first, second, output }` through `stack_head_value`; the f32
reference `clipped_layer` twice; the incremental test runs every width with and without the two
layers. Steps 1 and 2 tested by the developer (tests, clippy, bench nodes unchanged). Step 3
applied the same day, to test: `QuantizedHead::TwoHiddenI8` and `QuantizedRowsI8`, built by
`QuantizedPsqNetwork::quantize_with(network, HiddenArithmetic::Int8)` (`quantize` stays int16);
`dots_u8_i8_8_avx2` beside `dots_i16_8_avx2`, the two sharing `reduce_8_epi32`; the second layer
and output factored into `second_layer_and_output`, shared by the stack and both two-hidden heads.
The zero-squares count moved to step 4, with the bench. Step 3 tested by the developer. Step 4's
code the same day: `nnue_bench --halfk --hidden` (16 and 32, or `--head`), `--int8` re-quantizing
any network through `Network::from_psq_with`, and the share of zero squares and of all-zero blocks
of four printed after the timings. Steps 5 and 6 applied the same day, to test. Decided (the
developer, 2026-11-02, "without"): **no weight bound** on the first hidden layer. Psq's
`FIRST_HIDDEN_WEIGHT_BOUND` exists for a fixed scale of 64; the engine quantizes halfk's rows on one
scale each, which fits any range, so `weight_bounds` leaves halfk out and the export checks the bound
for psq only (`weights_from`'s `bounded`). As built: `with_options` takes halfk with no hidden layer
or two, passed to `with_rows`; `halfk_network_from` takes the widths; the stacked test checks each
arm, its gradient and the transform's, with no hidden layer and with two; the export agreement test
runs both shapes, the first layer drawn beyond psq's bound. Implements halfk-threats-screlu-hidden.md: step 1's network
with its 512 -> 1 output replaced by 512 -> 16 -> clip -> 32 -> clip -> 1, no skip, trained end to
end from scratch; the first layer in int16 and in int8 both, chosen on measurements. Each step with
its tests; the developer runs them and commits between steps.

## Findings that shape the plan (2026-11-02, from the code)

- **The trainer has the head.** `Head::Plain` (`ValueHead`, `Activation::ClippedRelu`) with
  `value_head_hidden_dims: [16, 32]` is 512 -> 16 -> clip -> 32 -> clip -> 1 over whatever it reads;
  a screlu network feeds it the squares already (`NetworkPsq::pawns`). Two asserts in
  `Network::with_head` refuse it today: a halfk network with hidden layers, and a screlu network
  with a plain hidden head.
- **The engine has it in int16.** `QuantizedHead::Hidden` (the first layer `QuantizedRows`, the rest
  and the output f32) is today reached by non-screlu networks only; `stack_head_value` computes the
  same shape over squares or products for a stack (first layer per row in int16 over `0..=127`, the
  second in f32 with `clipped_dot_products_8_avx2`, then the output). Step 2 routes a screlu
  network's `Hidden` head through the squares (`screlu_squares`) into that computation.
- **The f32 reference has it.** `PsqNetwork::evaluate_accumulators`' generic path runs hidden layers
  with clipped ReLU; `evaluate_screlu_accumulators` only needs its `[hidden]` arm (hidden-beside)
  kept apart from two layers.
- **The file needs a tag.** 21, `halfk_threats_screlu_hidden`: halfk's transform, the screlu
  activation, two hidden layers and the output, the widths in the header as other hidden networks'.

## Steps

### 1. The engine's network (`crates/rotor/src/nnue.rs`)

- `FEATURE_SET_HALFK_THREATS_SCRELU_HIDDEN = 21`, its name, shape (`halfk::ROWS`), `write` and
  `from_bytes` with the two hidden layers.
- `validate`: a halfk network is screlu over threats with no hidden layer or exactly two
  (`hidden[0]` 512 -> A, `hidden[1]` A -> B, the output B -> 1), no stack, no material, not pairwise.
  A and B within `MAX_HIDDEN_OUTPUTS`, A a multiple of 8.
- The f32 reference: `evaluate_screlu_accumulators` sends two hidden layers through the squares,
  each layer clipped, then the output (scalar, the reference).
- Tests: write and load; the f32 value by hand on a small network (width 16, layers 8 and 8).

### 2. The int16 head

- `quantize`: a screlu network with two hidden layers gets `QuantizedHead::Hidden` with its first
  layer `QuantizedRows` at the squares' one (`PRODUCT_ONE`), as a stack's.
- `evaluate_screlu_lanes`: `Hidden` reads `screlu_squares`, then `stack_head_value`'s computation
  (factored so both call one function).
- Tests: quantized within tolerance of f32 on random positions; vectorized equal to scalar; the
  incremental path equal to the refresh across king moves, as step 1's.

### 3. The int8 head

- `QuantizedHead::HiddenI8`: the squares requantized to `u8` (`0..=127` as they are, so no loss
  there), the first layer's weights `i8` on one scale per row (`127 / largest`), biases f32; the
  second layer and the output f32 as `Hidden`'s.
- An AVX2 kernel `dots_u8_i8_8_avx2` (`_mm256_maddubs_epi16` then `_mm256_madd_epi16` by ones, eight
  rows at a time), with its scalar twin; the nonzero-chunk skip (Reckless's `find_nnz`) only if a
  count on real positions shows many zero squares, measured in this step (the share of zero squares
  over the bench positions with a random network, then with the trained one).
  Saturation: `maddubs` adds two `u8 x i8` products into `i16`, at most 2 x 127 x 127 = 32,258,
  within `i16`.
- Which head `quantize` builds: an argument (`HiddenArithmetic::{Int16, Int8}`), int16 by default;
  `nnue_bench --int8` and the gap check choose it explicitly.
- Tests: vectorized equal to scalar; within tolerance of f32 (looser than int16's, the measurement
  below deciding).

### 4. The measurements before training (the developer)

- `nnue_bench --halfk --hidden` (a random network of this shape) against step 1's, in int16 and in
  int8; then each at depth 9 in the bench's search; one idle machine, back to back.
- The gap to f32 of a random network in each arithmetic (the existing quantized-against-f32 check),
  mean and p99. A random network's gap only warns; the trained one decides (step 7).
- **Measured 2026-11-02**, random networks, width 256, each run twice:
  - Speed per evaluation: halfk with no hidden layer 147 to 155 ns (head 109 to 114 ns); hidden
    [16, 32] in int16 303 to 308 ns (head 263 to 268 ns); in int8 276 to 284 ns (head 233 to 235
    ns). The two layers about double the evaluation; int8 takes about 8% off it. The search at
    depth 9 does not compare: a random network's trees differ (3 to 9 times the nodes).
  - Zero squares 51.9%, all-zero blocks of 4 only 6.9%: the nonzero skip is not worth building.
  - Gap to f32 over 12,680 positions of a duel's games: int16 mean 0.21 cp, p99 1.20, max 2.72;
    int8 mean 0.34 cp, p99 1.82, max 4.15; none 5 cp or more in either.
  - So far int8 is faster at no visible cost; both kept until step 7's trained winner decides.

### 5. The trainer (`smithy`)

- `ArchitectureKind::HalfkThreatsScreluHidden` (`kind: halfk_threats_screlu_hidden`), screlu, feature
  set `HalfKav2Hm`, `HeadShape::Plain`, `hidden_dim` two widths.
- `Network::with_head`: halfk allowed with a plain head of two hidden layers; the screlu assert
  accepts it for halfk only (psq's screlu stays as it is). Starting weights: `start_factorised` as
  step 1's; the head `nn::linear`'s default draw.
- Export: halfk's fold of the factor rows as step 1's, the two hidden layers and the output, tag 21,
  `quantize` run before writing (int16, the default).
- Tests: the value by hand through the head; the gradient reaching the transform (end to end); the
  tch value equal to the engine's f32 value through a file, as step 1's.

### 6. Config and docs

- `configs/lc0-halfk-threats-screlu-hidden.yaml`: step 1's with its kind and `hidden_dim: [16, 32]`;
  the config tests' list.
- architectures.md's entry; TODO.md's step 2.

### 7. After generation 1 (the developer, then a short change)

- The trained winner's gap to f32 in int16 and int8, and its share of zero squares; with step 4's
  speeds, the developer chooses. The loser's code removed before the duels, so that the engine plays
  one arithmetic.
- Duels against step 1's winner, depth 9 then 10+0.1, both `@default`.

## Commands for the developer

After each code step:

```bash
cargo test --workspace --release
```

```bash
cargo clippy --workspace --all-targets --release -- -D warnings
```
