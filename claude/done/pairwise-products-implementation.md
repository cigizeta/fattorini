# Pairwise products: implementation plan

**Status:** plan, written 2026-10-15; applied the same day, not yet compiled or tested (Claude has
no shell). Implements pairwise-products.md (the design,
its decisions recorded under its open questions). First network: greta (`configs/greta.yaml`),
`kind: psq_pairwise`, `embedding_dim: 256`, no hidden layer.

## What is built

Per perspective: the psq transform, clipped to `[0, 1]`, split into `a` (lanes `0..dim/2`) and `b`
(lanes `dim/2..dim`), multiplied element-wise, `p = a * b`, `dim/2` values with no weights of
their own. Both perspectives' `p`, side to move first, go into the usual head, which now reads
`dim` values instead of `2 * dim`. The incremental accumulator is psq's, unchanged.

- **Training:** f32, `a * b`, nothing else. No fake quantization, no 512.
- **Engine:** halves in `0..=255` (1.0 at 255), product `(a_q * b_q + 256) >> 9` in `0..=127`,
  whose 1.0 is `255 * 255 / 512` (about 127.002); that constant replaces 127 in the head's f32
  rescale. Rounded to nearest, not truncated as Stockfish does: Rotor does not simulate
  quantization in training, and truncation already cost caroline 2.1 cp mean as a bias on its
  activations (`QuantizedPsqNetwork::rounding`).

## Pairwise is a flag, not a feature set

The data is psq's exactly: same rows, same batches, same cache. So `FeatureSet` stays `Psq`, and the
product is a `pairwise: bool` carried by the network, as `threats` is carried by `PsqNetwork`. A
later `psq_threats_pairwise` is then the same flag on `PsqThreats`, with nothing else to add.

On disk, though, the engine dispatches on the feature-set tag alone, so pairwise gets its own tag
(`FEATURE_SET_PSQ_PAIRWISE = 6`), rather than a new header field: a new field would mean bumping
`VERSION`, and every existing `.nnue` would stop loading. A pairwise network loaded by an older build
is then refused as an unknown kind, never read as plain psq.

## Steps

### 1. Config (`crates/smithy/src/net_config.rs`)

- `ArchitectureKind::PsqPairwise` (serde: `psq_pairwise`), doc comment beside `PsqThreats`'s.
- `load`: for `psq_pairwise`, assert `embedding_dim` is even, naming the field and the reason (the
  halves). Not a multiple of 32: the engine falls back to its scalar path otherwise (step 6), as
  psq does for widths not a multiple of 16.
- `Architecture::pairwise()`: true for `PsqPairwise` only.
- The existing test that loads every config in `configs/` covers greta; extend its kind assertions
  (line 585) so greta is matched as `PsqPairwise`.

### 2. smithy plumbing (`crates/smithy/src/main.rs`)

Every `match` on `ArchitectureKind` gains `PsqPairwise` beside `Psq` (lines 272, 307, 397, 483,
569, 610). `feature_set_of(PsqPairwise)` is `FeatureSet::Psq`, with its test (line 2704).
`engine_runs` is true for it. `write_engine_network` passes `config.architecture.pairwise()` to the
export (step 5).

### 3. Training network (`crates/smithy/src/nnue_network.rs`)

- `NetworkPsq` gains `pairwise: bool`. `with_rows` builds the head on `embedding_dim` inputs when
  pairwise, `2 * embedding_dim` otherwise.
- `NetworkPsq::pawns`: after each perspective's clipped ReLU, when pairwise,
  `own = own.narrow(1, 0, half) * own.narrow(1, half, half)`, the same for `opponent`, then the
  concatenation as today.
- `Network::new` keeps its signature (every existing caller is non-pairwise); a
  `Network::new_pairwise(vs, embedding_dim, hidden)` builds `Network::Psq` with the flag, and
  `Network::of` calls it when `architecture.pairwise()`. Variable names are unchanged, so checkpoints
  and `arm_slice` need nothing.

### 4. Stacked network (`crates/smithy/src/stacked_network.rs`)

- `StackedNetwork` gains `pairwise: bool`, set by `new` from `architecture.pairwise()`;
  `with_shape` takes it as a parameter and passes it to the per-arm `Network` it builds the
  parameters from, so the head's shapes come out right.
- `head`: when pairwise, each of `own`/`opponent` (`[arms, batch, embedding]`) becomes
  `x.narrow(2, 0, half) * x.narrow(2, half, half)` before the `cat`. `sparse_transform` and
  `transform` are untouched.
- `weight_bounds` unchanged: it bounds the first hidden layer, whatever feeds it.

### 5. Export (`crates/smithy/src/export.rs`)

`psq_network_from` and `network_from` take `pairwise`; `weights_from`'s `head_input` is
`embedding_dim` when pairwise, `2 * embedding_dim` otherwise, so a wrong flag fails on the first
head layer's shape. `PsqNetwork { pairwise, .. }`.

### 6. Engine (`crates/rotor/src/nnue.rs`, `crates/rotor/src/simd.rs`)

**Format.** `FEATURE_SET_PSQ_PAIRWISE = 6`. `shape_of(6)` is `(psq::ROWS, 1)`: head input is one
`embedding_dim`. `feature_set_name(6)` is `"psq_pairwise"`. `PsqNetwork` gains `pub pairwise: bool`,
set by `load` from the tag; `write` picks the tag from `threats` and `pairwise` (both at once is
refused for now, there being no tag for it); `validate` passes `embedding_dim` as the head input
when pairwise, and refuses an odd `embedding_dim`.

**f32 reference (`PsqNetwork::evaluate_accumulators`).** When pairwise, skip the fused no-copy path;
write `clamp(own[i]) * clamp(own[i + half])` for `i < half`, then the opponent's, into
`HEAD_SCRATCH`'s `current`, and continue with the hidden layers and output as today. Scalar is
enough: this is the reference smithy's checks and the benches compare against, not what the search
runs.

**Quantized (`QuantizedPsqNetwork`).**

- Gains `pairwise: bool`. `ACTIVATION_ONE` stays 127 for psq; a pairwise network's half 1.0 is
  `HALF_ONE = 255`. `quantize`'s `fits` and scale use the network's one (`one()`: 255 or 127), so its
  `accumulator_shift` comes out one lower when the worst lane needs it; `accumulator_one`,
  `rounding`, `activation_of` and `activation()` read `one()` too. `activation_of` then yields
  `0..=255` for a pairwise network's halves.
- `product_of(a, b) = (a * b + 256) >> 9` on two halves' activations, in `0..=127`; the doc comment
  states the bound (`255 * 255 + 256 < 65536`, so it also fits `u16` in the vector kernel).
- The head's denominator: `output_value` and `hidden_value` take the input's 1.0 as an argument,
  `ACTIVATION_ONE as f32` for psq and `PRODUCT_ONE = 255.0 * 255.0 / 512.0` for pairwise, and
  `denominators` is computed from it at load. Every path keeps going through those two functions,
  so scalar and vectorized still agree exactly.
- `evaluate_lanes`, pairwise: the head's input is the `dim` products, not the `2 * dim` lanes.
  - Scalar: `scalar_dot` iterates over the products of `own` then `opponent`.
  - Vectorized, no hidden layer: one fused kernel, `pairwise_dot_i16_avx2(own, opponent, weights,
    activation)`, computes each 16 products in registers and `madd`s them with their weights at
    once, as `activation_dot_i16_avx2` does for psq. First written as two passes (products stored
    into `ACTIVATION_SCRATCH`, then a one-row dot), which cost the head 53.8 ns against psq's 39.0
    on a random width-256 network (Another machine, 2026-10-15); the fused kernel replaced it.
  - Vectorized, hidden layer: products computed once into `ACTIVATION_SCRATCH` (`dim` long) by
    `pairwise_activations_i16_avx2`, since every row reads them, then `dots_i16_8_avx2` as today.
  - Vectorized when each half is a multiple of 16 (`embedding_dim` a multiple of 32), scalar
    otherwise.
- `simd.rs`: `pairwise_activations_i16_avx2(destination, lanes, activation)`: for 16 lanes of each
  half at a time, the activation of each (clamp to `0..=one << shift`, add rounding, shift), then
  `_mm256_mullo_epi16` (the low 16 bits of a product below 65536 are the same signed or not), add
  256, `_mm256_srli_epi16` by 9 (logical, so a product above 32767 is read as unsigned). Equal to
  the scalar `product_of` exactly; tested on every lane value at the bounds.
- Everything incremental (`update`, `refresh_by_color`, `ChangedRows`) is untouched: the
  accumulator is psq's.

### 7. Bench (`crates/rotor/benches/nnue_bench.rs`)

`random_psq_network` takes `pairwise`; a `--pairwise` flag beside `--threats`, so `--quantization`
and the speed modes run on a random pairwise network as on a real one, and on greta's `.nnue` by
its tag.

## Tests

- **Config:** greta loads as `PsqPairwise`, `pairwise()` true; an odd `embedding_dim` is refused.
- **Training:** a pairwise `Network` has a head of `embedding_dim` inputs; its `pawns` equals a
  hand-computed `head(cat(a_own * b_own, a_opp * b_opp))` on a small network; gradients reach both
  halves of the transform (a nonzero `transform.weight.grad()` on rows of each half after one
  backward); a non-pairwise network's output is unchanged (the existing tests, run as they are).
- **Stacked:** the existing stacked-equals-unstacked tests (`networks_and_stack`) run for pairwise
  too, with and without a hidden layer, for `pawns`, `training_losses` and `test_losses`, sparse and
  dense paths.
- **Export:** the engine's f32 forward pass agrees with tch for a pairwise checkpoint
  (`the_engines_psq_forward_pass_agrees_with_tch` extended), and the file loads back with
  `pairwise` set and as tag 6.
- **Format:** round trip of a pairwise network; a file with tag 6 and an odd `embedding_dim` is
  refused; `threats` and `pairwise` both set is refused by `write`.
- **Quantized:** vectorized equals scalar exactly on a pairwise network (as for psq today); the
  quantized evaluation is within tolerance of the f32 one on the sample positions;
  `pairwise_activations_i16_avx2` equals `product_of` on every pair of halves at the edges
  (0, 1, 254, 255, above the clamp, negative); an accumulator at worst case does not overflow with
  `one()` at 255.
- **Unchanged networks:** node counts of the existing psq and psq_threats networks unchanged
  (`nnue_bench`'s and the search tests' fixed counts), since their quantization must be bit for bit
  what it was.

## Measurement

On idle the Lichess machine, after the tests pass:

- `nnue_bench --quantization` on greta's first candidate: cp mean and p99 against f32, against
  caroline's 1.16 / 7.8 and betty's 2.42 / 23.1.
- `nnue_bench` speed on a random pairwise network at width 256 against amanda's shape: head time
  and nodes/s. Expected about equal (pairwise-products.md, "Shape and cost").

Then greta trains on amanda-0002's corpus and its challenger plays amanda-0003.

## Out of scope

`psq_threats_pairwise`; the 256/128 power-of-two scales; fake quantization; `i8` head weights;
squared clipped ReLU (pairwise-products.md, question 5).
