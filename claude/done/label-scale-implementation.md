# The label scale: implementation plan

**Status: implemented and tested (2026-11-03),** before the current Lc0 run resumes. Two deviations from the steps below: **g is fitted at the start of Train,** on
demand (`scale_fit::ensure`, called in `train` before the test set is frozen and the corpus
scattered), not at the end of Play, which covers resumes and runs whose Play is already done, minutes
on the training machine; and **the run's page does not show g yet** (the log says it, `scale.txt`
holds it). Lc0's Convert writes through `LC0_FILES`, unchanged in output. Implements label-scale.md: f, the networks' unit,
at one pawn 0.50 with its ceiling raised to 128 pawns; g, each teacher's calibration, fitted on its
games; Lc0's files decoded with the curve they were written with. Each step with its tests; the
developer runs the tests and clippy between steps.

## Where the curve is used today

- `rotor/src/labels.rs`: `CEILING_PAWNS` (32), `A`, `K`, `score`, `pawns_from_score`, and
  `compute_labels_direct`, which reads every score through `score`.
- `rotor/src/nnue.rs`: `centipawns` clamps a network's output to `CEILING_PAWNS`; `fit_label_scale`
  (a network's pawn against PeSTO's) reads through `score`.
- `smithy/src/nnue_network.rs`: `score` on tensors with `A` and `K`, the loss of every NNUE network.
- `smithy/src/lc0/convert.rs`: Lc0's Q written as pawns through `pawns_from_score`.
- `smithy/src/game_labels.rs`: `self_play_game_labels`, called by the scatter (`pgn_stream.rs`), the
  freezer (`freeze.rs`) and the diagnostics (`engine_loss.rs`).
- `rotor/benches/nnue_bench.rs`: prints tails in label space through `score`.

## 1. A curve as a value: `Scale` (rotor, `labels.rs`)

- `pub struct Scale { a: f64, k: f64, ceiling: f64 }`, the atan family `atan(pawns / a) / k`, clamped
  to [-1, 1], reaching 1 at `ceiling`: `Scale::new(one_pawn, ceiling)` (a by bisection, as
  `lambda_star.rs`'s `curve_for`; k = `atan(ceiling / a)`), `score(pawns)`, `pawns(score)` (the
  inverse, clamped first), `one_pawn()`, `ceiling()`.
- **f: `pub const NETWORK: Scale`**, one pawn 0.50 and ceiling 128: `t = sqrt(1 - 2 / 128) =
  sqrt(63) / 8`, so `a = 8 / sqrt(63)` and `k = acos(1 / 127)`, written as literals with a test
  deriving them, as today's `A` and `K`. `CEILING_PAWNS` becomes 128.
- **The frozen decoder: `pub const LC0_FILES: Scale`,** today's curve, one pawn 0.50 and ceiling 32,
  `A = 4 / sqrt(15)`, `K = acos(1 / 31)`: what Lc0's PGN was written with, and is still written
  with (step 5), never changed again.
- `score` and `pawns_from_score` stay as `NETWORK`'s, for the engine and the benches.
- `compute_labels_direct` takes the scale its scores are read through: `compute_labels_direct(scores,
  outcome, lambda, scale)`.
- Tests: `NETWORK` derived from its two choices; `LC0_FILES` equal to today's `score` at every
  pawn the current tests use; `Scale::new(0.5, 32)` equal to `LC0_FILES`; the inverse exact;
  `NETWORK` within 0.01 of `LC0_FILES` below 3 pawns; 128 pawns (12,800 centipawns) below
  `is_decisive`'s threshold.

## 2. The networks' unit (rotor, smithy)

- `nnue.rs`: `centipawns` clamps to the new `CEILING_PAWNS`, 128. Networks trained before keep
  playing: they never output beyond about 32.
- `nnue_network.rs`: the tensor `score` reads `NETWORK`'s `a` and `k`.
- `fit_label_scale` reads through `NETWORK`.
- `nnue_bench.rs`: its comments and label-space tails through `NETWORK`.
- Tests: the existing loss tests with the new constants; a tensor `score` equal to `NETWORK.score`.

## 3. Labels read through their teacher's scale (smithy, `game_labels.rs`)

- `self_play_game_labels(collected, lambda, game_index, scale)`; callers pass the scale of the
  corpus they read:
  - **Lc0's corpus:** `LC0_FILES`, so a label's score is Lc0's Q exactly, as today.
  - **A self-play generation's corpus:** its teacher's g (step 4).
  - **PeSTO's corpus:** PeSTO's g (step 4), once the corpus exists.
- `StreamOptions` gains `scale`; `buckets.rs`, `freeze.rs` and `engine_loss.rs` pass theirs.
- **The scatter's state records the scale:** `buckets.tsv`'s `labels` line becomes
  `tablebase one_pawn <v> ceiling <c>`; buckets of another scale are scattered again from the start,
  as a `labels` mismatch already is. **Lc0's buckets are not rescattered (the developer,
  2026-11-03: the current Lc0 run resumes on them once this lands):** an old `labels tablebase` line
  read with the scale `LC0_FILES` is today's labels exactly, so the line is rewritten in place to
  the new form and the buckets kept; any other mismatch scatters again as today. Test: a bucket
  directory with the old line and `LC0_FILES` kept, its line rewritten; with another scale,
  scattered again.
- **Generation 1 in progress continues under the new f:** its remaining rounds read the loss through
  the ceiling of 128; the curves differ by under 0.01 below 3 pawns, so its training and patience
  go on unaffected.
- Tests: Lc0's labels unchanged through `LC0_FILES`; a self-play game's labels through a scale with
  one pawn at 0.62 differing from `NETWORK`'s as expected; a bucket directory of another scale
  scattered again.

## 4. g fitted on a teacher's games (smithy)

- **The fit, moved from the test into the smithy:** `lambda_star.rs`'s one-pawn fit (0.01-pawn bins
  of each searched position's score in pawns, folded to its side, with the sums of z and z^2; the
  score of one pawn at ceiling 128 minimising the squared error against z), as `scale_fit.rs`, the
  grid refined to 0.001 by golden-section search, as `fit_label_scale`. Certain scores (mate and
  tablebase band) left out, as in the test.
- **When:** at the end of a self-play generation's Play, before the generation's test set is frozen
  and its corpus scattered: a CPU job, so a run on a machine without CUDA fits it too
  (cpu-phases.md). **Where:** `gen-<g>/scale.txt`, `one_pawn<TAB>0.62` and `ceiling<TAB>128`, beside
  the corpus, on the remote with it. Resumable: written once, read thereafter; refitted only if the
  corpus grows.
- **PeSTO's corpus:** fitted when its Play completes, `data/pesto/scale.txt`. Until it exists, a
  `pesto-` run's generation 1 refuses to start with a clear message (no PeSTO corpus exists today).
- **Shown** on the run's page beside the generation: "g: one pawn at 0.62".
- Tests: the fit recovers the one-pawn score of games drawn from a known curve; `scale.txt` read
  back; a generation without one fits it before freezing.

## 5. Lc0's Convert keeps its format (smithy, `convert.rs`)

- Writes Q as pawns through `LC0_FILES`, unchanged in output, byte for byte: the corpus already
  converted and any future conversion stay one format.
- Test: a converted record's comment identical before and after the change.

## 6. Docs

label-scale.md marked implemented; continuous-training.md's g step pointing at `scale_fit.rs`; the
TODO's label-scale item done; DEV.md if a command or a file changes.

## Not in scope

- **Lambda:** generations keep their grid and round robin, centred as now.
- **The frozen test sets of Lc0** keep their labels (Q, through `LC0_FILES`), so every Lc0 test loss
  still compares. Test losses are read through f: networks evaluated under the 128 ceiling report
  losses differing from those recorded under 32 by the curve's change, under 0.01 in label space
  below 3 pawns; the recorded ones are not recomputed.
- **Continuous training,** designed in continuous-training.md, planned separately.
