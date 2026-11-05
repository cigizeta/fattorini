# halfk_threats_pairwise_hidden: implementation plan

Status: written 2026-11-02 and applied the same day, ahead of step 2's duel (the developer, "we can
implement"), to test. Implements halfk-threats-pairwise-hidden.md: step 2's network with each
perspective's 256 lanes clipped and their halves multiplied, 256 products into 256 -> 16 -> 32 -> 1.

## Findings that shape the plan (2026-11-02, from the code)

- **The f32 reference has it:** `pairwise_head` clips each half, multiplies, then runs every hidden
  layer clipped and the output: two clipped layers, the second read alone.
- **The quantized path almost has it:** `evaluate_pairwise_lanes` stores the products in `0..=127`
  (`product_of`) in scratch, then dispatches on the head; only the two-hidden heads are missing, and
  step 2's `stack_head_value` (int16) and `two_hidden_i8_value` (int8) take any `0..=127` inputs.
- **The accumulator is unchanged:** halfk's transform, its bound per bucket in `quantize_with`, the
  refresh cache and the threat rows know nothing of the head; `reads_products` already covers
  pairwise.
- **The trainer:** `NetworkPsq::with_rows` and the stacked network already compute pairwise
  products and halve the head's input; halfk only hard-codes `pairwise: false, screlu: true`.

## Steps

### 1. The engine (`crates/rotor/src/nnue.rs`)

- `FEATURE_SET_HALFK_THREATS_PAIRWISE_HIDDEN = 22`: `shape_of` (`halfk::ROWS`, head input 1),
  name, `write` (a halfk pairwise network), `from_bytes` (two hidden layers; halfk, threats,
  pairwise).
- `validate`: a halfk network is screlu with no hidden layer or two, or pairwise with two.
- `quantize_with`: the two-hidden arms for a halfk network, screlu or pairwise (a psq pairwise
  network with two hidden layers keeps `Hidden`).
- `evaluate_pairwise_lanes`: `TwoHidden` through `stack_head_value`, `TwoHiddenI8` through the
  products narrowed to `u8` and `two_hidden_i8_value`.
- Tests: write and load; the f32 value by hand; int16 and int8 within step 2's tolerances of f32;
  vectorized equal to scalar; the incremental path equal to the refresh across king moves.

### 2. The bench (`crates/rotor/benches/nnue_bench.rs`)

- `--pairwise` beside `--halfk --hidden`: a random network of this shape; the zero share counts
  products.

### 3. The trainer (`smithy`)

- `ArchitectureKind::HalfkThreatsPairwiseHidden`, pairwise, `FeatureSet::HalfKav2Hm`, plain head;
  the config check: two hidden widths.
- `with_options`: halfk pairwise with two hidden layers, its flags passed to `with_rows`.
- `halfk_network_from`: pairwise or screlu, the head reading `embedding_dim` products or
  `2 * embedding_dim` squares.
- `candidates.rs`: the kind's arms.
- Tests: the stacked halfk test over the pairwise shape; the export agreement test through a file.

### 4. Config and docs

- `configs/lc0-halfk-threats-pairwise-hidden.yaml`, the config tests' list; TODO.md,
  architectures.md.

## Commands for the developer

```bash
cargo test --workspace --release
```

```bash
cargo clippy --workspace --all-targets --release -- -D warnings
```
