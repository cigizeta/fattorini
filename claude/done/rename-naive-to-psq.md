# Rename naive to psq

The architecture table (architectures.md) calls the piece-square NNUE `psq`; the code still calls
it `naive`. This plan renames it everywhere, in one commit, with no alias left behind.

## Why no alias

The kind name lives in three places outside the code:

- **The configs** (`kind: naive` in amanda.yaml and caroline.yaml): edited in the same commit as the
  code, so any build reads a config that matches it. A machine pulling the branch gets both
  together. An alias accepting `naive` would be read by nothing, so it would be dead code.
- **`run_config.txt`** freezes `format!("{:?}", config.architecture)`, so it records
  `kind: Naive`. After the rename a resumed run reads `kind: Psq` and refuses to continue. This
  only matters for a training run that is in progress when the rename is merged. Finished runs are
  never resumed. So: **merge only when no `rotor-train train` is in progress.** Caroline's replay
  and self-play are not affected; her training run starts after the merge and freezes the new
  name.
- **The `.nnue` feature-set tag**: a number (3), not a name. It keeps the number 3, so every
  existing `.nnue` file still loads. Only the constant is renamed.

## What changes

Identifiers, in the code and its tests:

- `nnue.rs`: `FEATURE_SET_NAIVE` becomes `FEATURE_SET_PSQ` (still 3), its display name `"naive"`
  becomes `"psq"`, `NaiveNetwork` becomes `PsqNetwork`, `Network::Naive` becomes `Network::Psq`,
  `load_naive` becomes `load_psq`. The comment on tag 3 says the number is kept so old files load.
- `features.rs`: the `naive` module becomes `psq`. `psq_threats` already extends it, by name too.
- `train/net_config.rs`: `ArchitectureKind::Naive` becomes `Psq` (serde: `kind: psq`).
- `train/feature_set.rs`: `FeatureSet::Naive` becomes `Psq`.
- `train/export.rs`: `naive_network_from` becomes `psq_network_from`.
- `train/batch.rs`, `train/nnue_network.rs`, `train/mod.rs`, `train/value_head.rs`,
  `train/cache.rs`, `train/transformer_vocab.rs`, `train/round.rs`, `search.rs`, `simd.rs`,
  `labels.rs`, `pipeline/generation.rs`, `arena/*.rs`, `benches/nnue_bench.rs`: every use, test
  name and doc comment.
- `run_config.rs` tests: the sample value `"Naive"` becomes `"Psq"`.
- Error messages that name the kind, such as "only naive and psq_threats ones", become "only psq
  and psq_threats ones".

Files:

- `examples/net_naive.yaml` becomes `examples/net_psq.yaml` (`git mv`), with its test in
  `net_config.rs`. `examples/net_nnue.yaml` and `net_config.rs`'s module comment are updated.
- `configs/amanda.yaml` and `configs/caroline.yaml`: `kind: psq`. `configs/dorothy.yaml`: the
  comments say "psq's rows".

Left alone:

- The English word "naive" where it describes an approach, not the architecture (`tt.rs`,
  `search.rs:681`, `movegen/see.rs`, `uci.rs`).
- `claude/` documents written before the rename: they are history. TODO.md's entry "Rename naive to
  `psq`, later" is closed.
- `companion/scratch.md`: the developer's scratch.

## How

As carlo did: a branch from `bartolomeo` in its own worktree, so the main tree keeps building for
caroline. Claude edits there, the developer builds and tests:

- `cargo build --release --features training --all-targets` with no warnings,
- `cargo build --release --features move-stats --all-targets`,
- `cargo test --release --features training`,
- a grep for `(?i)naive` under `src/`, `benches/`, `configs/` and `examples/` that finds only the
  English uses listed above.

Then merge into `bartolomeo` while no training run is in progress, and remove the worktree.
