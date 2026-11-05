# Retire the old naive, rename psq to naive

Implementation plan. Branch: `alessio`. Decided with the developer (`architectures.md`, `TODO.md`):
the old naive (`rotor-naive.md`) is a weak network and is retired; psq takes its name. No backward
compatibility.

## Decisions

Settled in discussion.

- **What goes:** everything that is only the old naive. `features::naive` (204 rows, one
  perspective), `NaiveNetwork`, `Network::Naive`, `FEATURE_SET_NAIVE = 0`, `FeatureSet::Naive`,
  `ArchitectureKind::Naive`, their dense-batch and network arms, `export::naive_network_from`, and
  their tests and comments.
- **What is renamed:** `psq` becomes `naive` everywhere in code. `features::psq` to
  `features::naive`, `PsqNetwork` to `NaiveNetwork`, `Network::Psq`, `FeatureSet::Psq`, and
  `ArchitectureKind::Psq` to their `Naive` names, `psq_network_from` to `naive_network_from`, the YAML
  and CLI spelling `psq` to `naive`, `nnue_bench --psq` to `--naive`, search's accumulator path, test
  names, and comments.
- **Tags keep 3.** The `.nnue` feature-set tag stays 3, now named `naive`, and so does the trainer's
  `feature_set.state` discriminant. Tag 0 becomes unknown and is refused: reusing it would let an old
  naive file be read as the new network, failing only at its shapes. A psq file stays loadable, as
  the same network.
- **amanda is retired with the old naive.** Every amanda generation holds old-naive weights.
  `configs/amanda.yaml` already describes the new naive (`kind: naive`, embedding 256, no hidden
  layers), so it is correct as written and a fresh amanda lineage trains the new naive from
  generation 1. Removing amanda's old generations, locally and on Drive, is the developer's action on
  the data, not code.
- **The installed engine.** The engine exits when its `EvalFile` fails to load (`uci.rs`,
  `load_evaluator_or_exit`). An installed `rotor.nnue` holding an old-naive amanda champion would make
  a newly installed binary exit at startup: do not `rotor-train install` amanda until a new-naive
  champion exists, and remove that `rotor.nnue` if it is amanda's.
- **Docs:** `claude/` docs are history and stay, except `TODO.md` and the current-state parts of
  `architectures.md`. `examples/net_naive.yaml` moves to the new shape; the
  `# halfkp | halfka_v2_hm | naive | psq` comments lose `psq`.
- **Out of scope:** moving the new naive to pawn output (its own design: training loss and engine
  conversion change).

## Steps

1. **Engine (`src/`):** delete the old naive in `features.rs` and `nnue.rs`; rename `psq` to
   `naive` in `features.rs`, `nnue.rs`, `search.rs`, and the comments elsewhere (`simd.rs`,
   `big_five.rs`, `labels.rs`, `pipeline/generation.rs`, the arena messages).
2. **Trainer (`src/train/`):** the same in `feature_set.rs`, `net_config.rs`, `batch.rs`,
   `nnue_network.rs`, `export.rs`, `mod.rs`, `run_config.rs` tests, `window.rs` tests, and comments.
3. **Bench:** `nnue_bench`'s naive path goes, `--psq` becomes `--naive`.
4. **Configs and examples:** `examples/net_naive.yaml`, `examples/net_nnue.yaml`; `amanda.yaml`'s
   comment if it mentions the change.
5. **Docs:** `TODO.md` (the rename item done, amanda's data removal for the developer),
   `architectures.md`.

## Tests

- The existing psq tests, renamed, pass unchanged: the forward-pass agreement with `tch`, the file
  round trip, the perspective swap, the mirror, the incremental-accumulator search.
- The old naive's tests are deleted with it.
- A tag-0 file is refused as an unknown feature set.
- `rg -i psq` over `src/`, `benches/`, `configs/`, `examples/` finds nothing.

## Verification

```
cargo test --release
```

```
cargo test --release --features training --bin rotor-train
```

## As implemented

Not yet compiled or run. Deviations and choices beyond the plan:

- The old `NaiveNetwork::load` (a load refusing other kinds) is not kept: no caller needed it. The
  file-format tests load through `Network::load`, with a test-only `load_naive` helper where a
  round trip compares the network.
- `nnue_bench`: the random network is naive by default (amanda's and betty's shape, 256 with no
  hidden layers); `--naive` names that default explicitly. `--width`, `--head`, `--big-five` stay.
- The engine's counter-network tests are ported to the two-perspective network (castling and en
  passant reaching the accumulator; the centipawn bound, now driven through the output bias since
  the clipped ReLU bounds the accumulator). The row-sharing count test is deleted: no naive row is
  shared any more.
- `amandas_own_shape_round_trips` now uses amanda's new shape, 256 with no hidden layers.
- The exporter's scrambled-transform test and transpose test are kept, on the new naive.
