# halfk_threats_screlu_dual: implementation plan

Status: written and applied 2026-11-26 (the developer: "we can plan and implement now"). Implements
pairwise-dual-activation.md's choice 2 as superseded the same day: the dual head of
halfk_threats_pairwise_dual on screlu's transform, 1024 squares at width 512 into the same 16-output
first layer, then the same head (15 clipped and squared, 30 -> 32 -> 1, the 16th the shortcut), with
the castling and en passant rows and `unit: elo`.

## Findings that shape the plan (2026-11-26, from the code)

- **The trainer's forward already computes it.** `NetworkPsq::pawns` and `StackedNetwork::head`
  square a screlu network's lanes, then apply `Head::Dual` whatever the transform: only assertions
  and the kind's plumbing forbid the combination.
- **The castling and en passant rows are tied to pairwise,** in both crates: the engine's
  `PsqNetwork::halfk_state` is `halfk && pairwise`, the trainer asserts `HalfKav2HmState` if and only
  if halfk and pairwise, and `export::halfk_network_from` folds the state rows for pairwise alone.
  The rule becomes: **a halfk network has them if it is pairwise or has a dual head.** Tags 20 and 21
  (halfk screlu, plain or two hidden layers) still have none: their files load unchanged.
- **The engine tells a dual head by its shape** (`PsqNetwork::dual`: two hidden layers, the second
  reading `2 * (first - 1)`), today required pairwise too; dropped, so the shape alone decides, and
  the tag says which transform.
- **The quantized head is reused:** `QuantizedHead::Dual` is built from `network.dual()` before the
  two-hidden case, and `dual_head_value` reads any inputs; the screlu path of `evaluate_lanes` gains
  its `Dual` branch beside `TwoHidden`'s.

## Steps

### 1. The engine (`crates/fattorini/src/nnue.rs`)

- Tags 35 (32 buckets), 36 (8), 37 (10), `FEATURE_SET_HALFK*_THREATS_SCRELU_DUAL`, a sixth row of
  `HALFK_TAGS`, their names, `shape_of` (`rows_with_state`, 2).
- `dual` by shape alone; `halfk_state` is `halfk && (pairwise || dual)`; `write` picks the screlu
  dual tag for a screlu dual network; `from_bytes` reads it as two hidden layers, the second at
  `2 * (first - 1)`, and lists it among the threats, halfk and screlu tags.
- f32: `evaluate_screlu_accumulators` takes `dual_head` over the squares for a dual network.
- Quantized: the screlu path of `evaluate_lanes` takes `dual_head_value` over the squares.
- Tests: a screlu dual sample (`lively_halfk_two_hidden_network_in` screlu with the state rows and
  the dual second layer); it writes and loads as its own kind under each layout's tag; its f32 value
  is `dual_head` over the squares; quantized close to f32; incremental equal to from scratch, its
  state rows present; tags 20 and 21 still without them.

### 2. The trainer (`smithy`)

- `ArchitectureKind::HalfkThreatsScreluDual`: halfk, screlu, `HeadShape::Dual`, two hidden widths,
  `FeatureSet::HalfKav2HmState`; exported as halfk.
- `Network::with_options`: the state rows if and only if halfk and (pairwise or a dual head); a dual
  head on halfk, screlu or pairwise.
- `export::halfk_network_from`: a dual network screlu or pairwise; the state rows for pairwise or
  dual.
- Tests: the export through a file, tag and value, and the stacked arms against the unstacked
  network, each with the screlu dual case added.

### 3. Config and docs

- `configs/lc0-halfk-threats-screlu-dual-512-8-budget.yaml`: pairwise dual's, the kind changed;
  `unit: elo`.
- TODO.md's screlu dual item: written.

## Commands for the developer

After the running trial, never beside it:

```bash
cargo test --workspace --release
```

```bash
cargo clippy --workspace --all-targets --release -- -D warnings
```
