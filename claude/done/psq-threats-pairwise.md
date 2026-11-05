# psq_threats_pairwise

Implemented, tested and committed 2026-10-17; the bench is still to run. `nnue_bench` needed nothing: `--threats --pairwise` builds
the shape already. Experiment 3 of TODO.md's network experiments (2026-10-17): the
pairwise products over caroline's inputs, `lc0-psq-threats-pairwise` and
`self-psq-threats-pairwise`.

## The shape

(4876 -> 256) x 2, each perspective's clipped transform split in two halves of 128 multiplied
element-wise, both perspectives concatenated to 256, straight to the output. That is psq_threats'
transform (psq's 780 rows plus each perspective's own 4,096 threat rows) with psq_pairwise's head
(pairwise-products.md). Nothing new is computed: each half exists already and only their
combination is refused today.

## What exists, and what refuses the combination

- **Trainer (`nnue_network.rs`, `stacked_network.rs`):** `NetworkPsq` applies the products after
  the clamp whatever its transform's rows, and `StackedNetwork::head` likewise. Only
  `Network::with_head`'s assert ("pairwise products exist for psq only") refuses `PsqThreats`.
- **Export (`export.rs`):** `network_from` already takes `threats` and `pairwise` apart;
  `psq_threats_network_from` passes `pairwise: false`.
- **Engine, f32 (`nnue.rs`):** `accumulators` adds the threat rows, `evaluate_accumulators`
  branches on `pairwise`: composes already.
- **Engine, quantized:** `evaluate_by_color_cp` adds the threat rows into the scratch accumulators,
  saturating to `i16`, then calls `evaluate_lanes`, which branches on `pairwise`. A saturated lane
  is clamped to `accumulator_one` before any product, so saturation stays harmless at the pairwise
  scale (255, not 127). `quantize`'s bound covers the piece-square rows only, as for psq_threats.
- **File format:** no tag. `validate` refuses `pairwise && threats` ("which has no tag yet"),
  `write` picks the tag from `threats` first, and `load` sets `pairwise` only for tags 6 and 7.

## Changes

1. **`net_config.rs`:** `ArchitectureKind::PsqThreatsPairwise` (`kind: psq_threats_pairwise`),
   psq_threats' rows; `pairwise()` includes it, so the even-`embedding_dim` check applies.
2. **`main.rs`:** `feature_set_of` maps it to `FeatureSet::PsqThreats`; it joins the NNUE arms of
   `check`, `calibrate`, `correlate`, `export_candidate` and `engine_runs`; `write_engine_network`
   passes `architecture.pairwise()` to `psq_threats_network_from`.
3. **`nnue_network.rs`:** the assert allows pairwise for `PsqThreats` (a stack still psq only);
   `Network::PsqThreats` built with `pairwise`.
4. **`export.rs`:** `psq_threats_network_from` takes `pairwise`.
5. **`nnue.rs`:** `FEATURE_SET_PSQ_THREATS_PAIRWISE = 8`, shape `(psq_threats::ROWS, 1)`, named
   `psq_threats_pairwise`; `write` picks it for `threats && pairwise`; `load` sets both flags from
   it; `validate` drops the refusal (a stack over threats stays refused, experiment 4's).
6. **Configs:** `lc0-psq-threats-pairwise.yaml` and `self-psq-threats-pairwise.yaml`, as
   `lc0-psq-threats` and `self-psq-threats` field for field but the kind; the config tests list them.

## Tests

- Trainer: a pairwise psq_threats network's value is its halves' products through the head, by
  hand, as psq's (`a_pairwise_networks_value_is_its_halves_products_through_the_head`); every
  stacked arm equals its unstacked network.
- Export: a pairwise psq_threats export agrees with tch through a file (f32), as
  `the_engines_pairwise_forward_pass_agrees_with_tch_through_a_file`.
- Engine: round trip as its own kind; the quantized pairwise network stays close to f32 with threats
  included; incremental evaluation along random games equals the from-scratch one, saturated lanes
  included (`heavy_threats_network` made pairwise).
- `pairwise_threats_are_refused` and `a_pairwise_network_with_an_odd_width_or_threats_is_refused`
  turn into their opposites, the odd width still refused.

## Measured after

Evaluation speed on the Lichess machine (`nnue_bench`) against psq_threats and psq_pairwise, as for every new
shape: the threats are recomputed at every evaluation either way, the products halve the head.

## Open

- None known. The stack over threats (experiment 4) needs `evaluate_by_color_cp`'s threat path to
  pass the position's buckets (today `[0; 3]`), its own plan.
