# psq_threats_stack

**Archived 2026-11-04: abandoned, never trained; its configs are archived. The line moved to SCReLU,
then halfk (architectures.md).**

Implemented, tested and committed 2026-10-17; the bench is still to run. Experiment 4 of TODO.md's network experiments (2026-10-17): the stacked
head over caroline's inputs, `lc0-psq-threats-stack` and `self-psq-threats-stack`. Follows
psq-threats-pairwise.md, whose products it reads.

## The shape

psq_threats_pairwise's transform and products (4876 rows, 256 lanes, each perspective's halves of
128 multiplied, 256 products in all) into psq_stack's head: 256 -> 16 by our king's square (64
sets), 16 -> 16 by their king's (64), 16 -> 1 by the material at 1/3/3/5/9 (78), trained factorised
(`stack_head.rs`), summed at export. hilda's head, caroline's inputs.

## What exists, and what refuses the combination

- **Trainer:** every psq_threats batch carries its rows' three buckets (`batch::psq_threats_indices`),
  and `StackedNetwork::head` and `NetworkPsq::pawns` read them from any psq-form batch. Only
  `Network::with_head`'s "a stack head exists for psq only" refuses it.
- **Export:** `stack_network_from` checks the transform against `psq::ROWS` and writes
  `threats: false`.
- **Engine, f32:** `evaluate_pawns` refreshes with the threats and passes `buckets_of_board`:
  composes already.
- **Engine, quantized:** `evaluate_pawns`, the from-scratch reference, passes `self.buckets(board)`.
  But `evaluate_by_color_cp`'s threats branch calls `evaluate_lanes(.., [0; 3])`: a stack over
  threats would read bucket 0 at every position in the search. **The one real change.**
- **File format:** `validate` refuses a stack with threats; `load` reads a stack only under tag 7,
  at `psq::ROWS`.
- **Bench:** `--stack` builds `random_stack_network` without threats.

## Changes

1. **`net_config.rs`:** `ArchitectureKind::PsqThreatsStack` (`kind: psq_threats_stack`);
   `pairwise()` and `stack()` include it, so the two-hidden-widths and even-width checks apply.
2. **`main.rs`:** `feature_set_of` maps it to `FeatureSet::PsqThreats`; the NNUE arms of `check`,
   `calibrate`, `correlate`, `export_candidate` and `engine_runs`; `write_engine_network`'s stack
   arm for either feature set, passing `threats`.
3. **`nnue_network.rs`:** the stack assert allows `PsqThreats`; `with_rows` builds the stack head
   for it.
4. **`export.rs`:** `stack_network_from` takes `threats`: its rows and its flag.
5. **`nnue.rs`:** `FEATURE_SET_PSQ_THREATS_STACK = 9`, shape `(psq_threats::ROWS, 1)`, named
   `psq_threats_stack`; `write` picks it for a stack with threats; `load` reads the stack for tags 7
   and 9, threats for 5, 8 and 9; `validate` allows a stack with threats (still pairwise only);
   `evaluate_by_color_cp`'s threats branch passes `self.buckets(board)`.
6. **`nnue_bench.rs`:** `--stack --threats` builds a stack over threats.
7. **Configs:** `lc0-psq-threats-stack.yaml` and `self-psq-threats-stack.yaml`, the pairwise ones
   field for field but the kind and `hidden_dim: [16, 16]`; the config tests list them.

## Tests

- Trainer: every stacked arm of a psq_threats stack equals its unstacked network, deltas randomised
  so a row's buckets matter (as `head_networks_and_stack` does for psq).
- Export: a psq_threats stack agrees with tch through a file, deltas randomised.
- Engine: round trip under tag 9; quantized within tolerance of f32 with threats on the board;
  incremental evaluation along random games equals the from-scratch one, with saturated lanes and
  distinct weight sets (`sample_stack_network` over threats), which fails on the `[0; 3]` bug.
- `a_stack_over_threats_is_refused` (trainer) and the engine's stack-with-threats refusal turn into
  their opposites; a stack without pairwise products stays refused.

## Measured after

With pairwise's, `nnue_bench` on the Lichess machine against psq_threats_pairwise and psq_stack.
