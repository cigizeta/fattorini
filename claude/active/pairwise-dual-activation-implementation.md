# halfk_threats_pairwise_dual: implementation plan

Status: written and applied 2026-11-25 (the developer: "implement"). Implements
pairwise-dual-activation.md: today's `halfk_threats_pairwise_hidden` with its first hidden layer's
16 outputs split 15 and 1, the 15 clipped and squared side by side into the second layer (30 in),
the 16th added raw to the score.

## Findings that shape the plan (2026-11-25, from the code)

- **The config keeps `hidden_dim: [16, 32]`:** the first width counts the shortcut. The second
  layer reads `2 * (16 - 1) = 30`, which is how the engine tells the head apart from tag 22's by
  shape alone (`PsqNetwork::dual`), as `pairwise_hidden_beside` does, with no new field.
- **Parameter names unchanged:** `value_head.hidden0`, `hidden1`, `output`, only `hidden1`'s input
  differs, so checkpoints, the optimizer and the stacked arms need no new names.
- **The quantized first layer is reused:** its integer sums are split out of `first_layer_values`
  (`first_layer_sums`); the 15 values and the raw 16th are computed from them by the same scalar
  arithmetic on every path, so vectorized and scalar still agree exactly.
- **The castling and en passant rows come with pairwise** (`halfk_state`), unchanged.

## Steps

### 1. The engine (`crates/fattorini/src/nnue.rs`)

- Tags 29 (32 buckets), 30 (8), 31 (10), `FEATURE_SET_HALFK*_THREATS_PAIRWISE_DUAL`;
  `split_layout` and `with_layout` read one table of each kind's three tags, no longer ranges.
- `shape_of`, the names, `write`, `from_bytes` (the second layer read as 30 in), `validate`
  (`validate_dual`), the threats, halfk and pairwise lists.
- f32 reference `dual_head`; `QuantizedHead::Dual`, quantized as `TwoHidden`, evaluated by
  `dual_head_value`.
- Tests: write and load as its own kind; the f32 value by hand; quantized close to f32 and
  vectorized equal to scalar; incremental equal to from scratch.

### 2. The trainer (`smithy`)

- `ArchitectureKind::HalfkThreatsPairwiseDual`: halfk, pairwise, `HeadShape::Dual`, two hidden
  widths; `candidates.rs`' arms and feature set (`HalfKav2HmState`).
- `stack_head::dual_input`: the raw first layer to the 30 inputs and the shortcut, shared by
  `NetworkPsq`'s `Head::Dual` and `StackedNetwork::head`.
- `export::halfk_network_from` takes `dual`; `weights_from` reads `hidden1` at 30 in.
- Tests: the stacked arms against the unstacked network; the export through a file, tag and value.

### 3. Config and docs

- `configs/lc0-halfk-threats-pairwise-dual-512-8-budget.yaml`, `unit: elo`; the config tests'
  list; TODO.md.

## Commands for the developer

```bash
cargo test --workspace --release
```

```bash
cargo clippy --workspace --all-targets --release -- -D warnings
```
