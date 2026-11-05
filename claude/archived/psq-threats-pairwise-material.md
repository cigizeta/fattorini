# psq_threats_pairwise_material: experiment 3b

**Archived 2026-11-04: superseded, see psq-threats-screlu-material.md. Its config is archived.**

**Status: design agreed 2026-10-20 (the developer); implementation plan below, approved the same day; implemented, not yet built or tested.**
One departure from the plan: smithy's `stack: bool` became `stack_head::HeadShape` (`Plain`,
`Stack`, `Material`), and the material head is a one-stage `StackHead` (`StackHead::material`, its
stage reading bucket column 2), so the lockstep network and the plain one share the stack's code
path rather than gaining a third head type.

## What

psq_threats_pairwise (psq-threats-pairwise.md) with its output layer one of 78 weight sets, chosen
by the material on the board (1/3/3/5/9, pawns included, both sides, 1 to 78), and no king buckets:
`(128 + 128) x 2 products -> 1`, the output one of 78. architectures.md's
`psq_threats_pairwise_material`, TODO.md's experiment 3b.

## Why

It isolates the material buckets, one of the four things psq_threats_pairwise_stack changes at once
(the king buckets, the two hidden layers, the factorisation being the others), and decides whether
halfk_threats embeds them (architectures.md).

## Decisions (the developer, 2026-10-20)

- **Factorised, as the stack:** each of the 78 sets is a shared part plus its own delta, so rare
  materials learn mostly from the shared part; the engine adds the two once, at load, and infers
  at pairwise's cost. Independent sets would make a result hard to read, the factorisation being
  the stack's.
- **`lc0-` only:** one config, `lc0-psq-threats-pairwise-material`, on the same corpus, lambdas and
  training schedule as `lc0-psq-threats-pairwise`, the kind alone differing. `self-` networks come
  in a distant future, only after the best `lc0-` network has emerged.
- **No hidden layer**, as every experiment of the series (TODO.md).

## Reused

The stack's material buckets: `stack_buckets::material_bucket` and `MATERIAL_BUCKETS` (78) in the
engine, the factorised bucketed layers of smithy's `stack_head.rs`.

## Judged by

A duel against `lc0-psq-threats-pairwise-0001`, both sides tuned alike (both at `@default`, or both
tuned the same way), never a tuned side against an untuned one.

## Implementation plan

Read from the code as it is (2026-10-20). The engine has no kind enum: a psq network is flags
(`threats`, `pairwise`, `stack`) and a file tag; smithy has `ArchitectureKind`.

### Engine (`crates/rotor`)

1. **`nnue.rs`, the file:** a new tag, `FEATURE_SET_PSQ_THREATS_PAIRWISE_MATERIAL = 11`, joining
   `shape_of` (psq_threats' rows, as pairwise) and `feature_set_name`. Layout as pairwise's, the one
   output layer replaced by `MATERIAL_BUCKETS` output layers, each `[1][256]` and its bias, in bucket
   order: already summed (shared plus delta) by the export, as the stack's are.
2. **`PsqNetwork`:** a field `material: Option<Vec<Layer>>`, the 78 output sets, `None` for every
   other kind; `validate` (78 sets, each `[1][head_input]`, pairwise only, no hidden layer, no
   stack), `write` and `from_bytes` gain its branch, the latter reading the sets with the stack's
   `sets` pattern.
3. **f32 forward pass:** `evaluate_pairwise_accumulators`' products, then `material[bucket]` in
   place of `output`; `evaluate_pawns` takes the material bucket when `material` is set.
4. **Quantised:** `QuantizedHead::Material { weights: Vec<Vec<i16>>, scales: Vec<f32>, biases:
   Vec<f32> }`, each set quantised as `QuantizedHead::Output` is (its own `i16::MAX / max|w|`), so
   a bucket's evaluation is pairwise's exact path with that bucket's weight and scale:
   `pairwise_dot_i16_avx2` then `output_value`, and the scalar twin.
5. **`buckets`:** a material-only `stack_buckets::material_bucket_of_board` (no king lookups),
   used when the head is `Material`; the stack's three buckets stay as they are.
6. **Bench (`benches/nnue_bench.rs`):** a `--material` flag with `--threats --pairwise`, a
   `random_material_network`, and its label in `time_psq`, for the speed check against pairwise.

### Training (`crates/smithy`)

7. **`net_config.rs`:** `ArchitectureKind::PsqThreatsPairwiseMaterial`
   (`psq_threats_pairwise_material`); `pairwise()` true, `stack()` false, a new `material()` true
   for it alone; its config joins `the_experiments_differ_in_their_shape_and_labels_alone`, hidden
   dims `[]`.
8. **`stack_head.rs`:** a `MaterialHead`, one `Stage` of `MATERIAL_BUCKETS` buckets from the head
   input to one output, named `value_head.output.shared.*` and `value_head.output.delta.*`, deltas
   starting at zero, its forward `stage_forward` on the batch's material column (index 2); the
   lockstep `stacked_stage_forward` the same. `Stage`'s two functions are already generic over the
   bucket count.
9. **`nnue_network.rs` and `stacked_network.rs`:** `Head::Material(MaterialHead)`, chosen by
   `material()`, pairwise required (refused otherwise, as a stack without products is); the
   lockstep network's head the same.
10. **`export.rs`:** `material_network_from`, summing shared and delta per bucket into the 78
    `Layer`s, as `stack_network_from` does for its output stage; `write_engine_network` dispatches
    to it.
11. **`main.rs`:** the new variant in every match: `check`, `calibrate`, `correlate`,
    `export_candidate`, `engine_runs`, `write_engine_network`, `feature_set_of`
    (`FeatureSet::PsqThreats`: the data pipeline is unchanged, every psq_threats batch already
    carrying its buckets).
12. **`configs/lc0-psq-threats-pairwise-material.yaml`:** pairwise's config, `kind:
    psq_threats_pairwise_material` alone differing, its header comment saying what it is.
13. **Arena (`config.rs`):** the config joins the list of configs with their round-robin pairs.

### Tests

- Engine: the network survives a write and a load as its own kind (tag 11); a wrong set count or
  shape is refused; the f32 head takes the position's material bucket's set; quantised stays close
  to f32, vectorised equals scalar; incremental equals from scratch with saturated lanes.
- Smithy: the engine's forward pass agrees with tch through a file, the deltas randomised (as the
  stack's test); every material arm of the lockstep network computes what its unstacked network
  does, and gets its own gradient; a material head without pairwise products is refused; the
  parameter names and bounds are an unstacked network's; `feature_set_of` and the config test.

### Steps for the developer

1. `cargo test -p rotor -p smithy -p arena`.
2. `cargo bench --bench nnue_bench -- --threats --pairwise --material --width 256 --head ""`
   against the same without `--material`, both on one idle machine: the head is expected as fast.
3. `bin/smithy forge lc0-psq-threats-pairwise-material` on the training machine, after pairwise's own run.
