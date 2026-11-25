# halfk_threats_pairwise_dual_count: implementation plan

Status: written and applied 2026-11-25 (the developer: "plan and implement"). Implements
pairwise-dual-count.md: 8 dual heads by piece count over one shared transform.

## Findings that shape the plan (2026-11-25, from the code)

- **The batches already carry the key:** every psq-form batch, halfk's included, has five bucket
  columns, the piece count's the fifth (column 4, `batch::active_indices`).
- **The engine's `StackLayers` holds per-bucket layers already** (`l1`, `l2`, `output`, one `Layer`
  each per bucket): a halfk network with a stack is this kind (`PsqNetwork::dual_count`), its sets
  `COUNT_BUCKETS` each, so `PsqNetwork` gains no field. The psq stack's paths are kept off it.
- **The dual head's arithmetic is reused as it is:** `dual_head` in f32 and `dual_head_value`
  quantized, on the position's bucket's three layers.
- **Training's bucketed layers are new:** a plain `[buckets, out, in]` weight, each row through its
  own bucket's (all buckets computed at once, then gathered, as `stage_forward` does without the
  shared part).

## Steps

### 1. The engine (`crates/fattorini/src/nnue.rs`)

- Tags 32, 33, 34 (32, 8, 10 king buckets), a fifth row of `HALFK_TAGS`; `shape_of`, names.
- `dual_count`, `validate_dual_count`, `write`, `from_bytes` (8 sets of each layer).
- f32: `evaluate_dual_count_accumulators`, `evaluate_pawns` by the count bucket.
- `QuantizedHead::DualCount`, its bucket from `buckets`, evaluated by `dual_head_value`.
- Tests: write and load under each layout's tag; the f32 value is its bucket's dual head; quantized
  close to f32 and vectorized equal to scalar, on positions of several buckets.

### 2. The trainer (`smithy`)

- `ArchitectureKind::HalfkThreatsPairwiseDualCount`, `HeadShape::DualCount`;
  `stack_head::bucketed_linear` (one draw copied to every bucket), `bucketed_forward` and
  `stacked_bucketed_forward`.
- `NetworkPsq`'s `Head::DualCount`; `StackedNetwork::head` by the batch's count column.
- `export::dual_count_network_from`; `candidates.rs`' arms and feature set.
- Tests: the stacked arms against the unstacked network; the export through a file; every bucket
  starting equal.

### 3. Config and docs

- `configs/lc0-halfk-threats-pairwise-dual-count-512-8-budget.yaml`, `unit: elo`; the config
  tests' list; TODO.md.

## Commands for the developer

```bash
cargo test --workspace --release
```

```bash
cargo clippy --workspace --all-targets --release -- -D warnings
```
