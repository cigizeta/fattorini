# psq_threats_screlu with two tapered output sets

**Status:** designed 2026-10-25; step 1 done, tests passing; step 2 done, benched, the fused kernel bringing the cost from 18% to about 3% per node; steps 3 and 4 done, tests passing; step 5 done; a dev smithy smoke run ended without errors 2026-10-25. **Done 2026-10-26, a null, parked:** see Result.

## Why

psq_threats_screlu_material (78 buckets), psq_threats_screlu_phase (25, psq-threats-screlu-phase.md)
and psq_threats_screlu_count (8, psq-threats-screlu-count.md) each pick one output set per position.
The experiment: two sets, a middlegame expert and an endgame expert, blended by PeSTO's taper, so
every position trains both and the output weights move smoothly with the phase instead of jumping at
a bucket's edge.

The taper is the phase network with its 25 sets forced onto a straight line between two ends,
`w(p) = w_eg + (p / 24) * (w_mg - w_eg)`: it cannot fit better than the phase network, so the
question is whether the restriction generalises better. Stockfish keys its outputs by piece count,
not by a taper; whether they tried one is not known (the developer, 2026-10-25), so the duels decide.

## Decisions (the developer, 2026-10-25)

1. **The model:** `y = (p * (w_mg . x + b_mg) + (24 - p) * (w_eg . x + b_eg)) / 24`, `x` the
   `2 * 256` SCReLU values, `p` PeSTO's game phase as the phase sets' (knight and bishop 1, rook 2,
   queen 4, both sides, capped at 24). The blend's weights are fixed, not learned.
2. **Not factorised:** both experts see every position, so no shared plus delta.
3. **No dense layer:** `hidden_dim: []`, as the other three.
4. **Name:** architecture `psq_threats_screlu_taper`, config
   `configs/lc0-psq-threats-screlu-taper.yaml`, a copy of the phase config with the architecture
   changed.
5. **Duels:** its winner against `lc0-psq-threats-screlu-g1-r1` and against the phase winner, depth
   9, then 10+0.1.

## Design

- **A new architecture, `psq_threats_screlu_taper`** (`ArchitectureKind::PsqThreatsScreluTaper`),
  beside the phase one, its phase computed the same way.
- **A new feature-set tag, 16, `psq_threats_screlu_taper`:** tag 13's layout with 2 output sets,
  middlegame first. No flag after all (revised while implementing, 2026-10-25): 2 sets differ from
  8, 25 and 78 as those differ from each other, so the set count tells the taper apart, as it tells
  the keys apart (`stack_buckets::TAPER_SETS`); for 2 sets `output_bucket_of_board` gives the capped
  phase, which blends both sets instead of picking one.
- **Engine:** both dot products, each set's value in f32 as today, then the blend
  `(p * mg + (24 - p) * eg) / 24` in f32 (`stack_buckets::taper`), one function for the f32 and the
  quantized paths, so no integer rounding is needed: the centipawn conversion rounds once, after.
- **Trainer:** a `HeadShape::Taper` in `stack_head.rs` reading the batch's phase column (already
  there): both outputs computed and blended (`stack_head::taper`), so `w_mg` gets the gradient
  scaled by `p / 24` and `w_eg` by `(24 - p) / 24`, by autograd alone. The head is one plain layer
  of two outputs, `value_head.output`, `[2, 2 * embedding_dim]`, in the single network and the
  stacked one alike.
- **Export:** the layer's two rows as the engine's two sets (`export::taper_network_from`), no sum
  of shared and delta.

## Bench (2026-10-25, one machine, random networks, width 256, depth 9)

Two runs back to back, PeSTO's 20.4 and 20.6 ns showing the machine steady. Head alone: taper 384.0
ns against the phase's 316.7, per search node 439.4 against 372.2, +67 ns or 18%. The kernel ran
twice, clamping and squaring every lane twice; `screlu_dot2_i16_avx2` now squares each lane once
for both sets. Benched again the same way: head alone 332.2 ns against 322.1, +10 ns or 3%; per
search node 394.9 against 377.6, +17 ns, of which the incremental update, which no output set
touches, took 7 (62.7 against 55.5, noise). The taper costs about 3% per node, kept. ("From scratch" is the f32 reference, scalar, which runs both
sets in full: 3427 against 2454 ns, never on the search's path.)

## Plan

1. **Engine** (`crates/rotor/src/nnue.rs`): tag 16 and its name; loading reads 2 sets for 16 and
   marks them blended; `validate` accepts the taper for screlu only, with exactly 2 sets; `write`
   picks 16 for it; the evaluation blends. Tests: tag 16 round-trips, pairwise refuses it, the blend
   at phase 0 and 24 equals the endgame and middlegame set alone, phase 12 their mean, quantised close
   to f32.
2. **Bench** (`nnue_bench --material --screlu --taper`): one line, against the phase's, back to back
   on one idle machine, to check the second dot product costs nothing measurable.
3. **Trainer:** `ArchitectureKind::PsqThreatsScreluTaper` (`net_config.rs`, `screlu()` including
   it, a load test); `HeadShape::Taper` in `stack_head.rs` with a test that phase 0 and 24 give one
   set's output; the export writing tag 16 (`export.rs`, its agreement test extended to it);
   `candidates.rs`' lists; the smithy's forge-config test.
4. **Config:** `configs/lc0-psq-threats-screlu-taper.yaml`.
5. **Docs:** TODO.md, architectures.md.
6. Build, test, a dev smithy smoke run, then the run on the training machine and the duels.

## Result (2026-10-26)

**Generation 1:** lambda 0.03 promoted (round robin flat, 49.1% to 51.0%, +/- about 1.0% each;
0.03 first, 0.02 last), gate against PeSTO passed 36.5 to 3.5 in 40 games. Best test loss 0.065053
(lambda 0.02), 0.065113 (0.03): SCReLU's 0.065127 within 0.1%, where phase was 2.0% below and count
0.6%. 161 to 197 rounds, between SCReLU's 146 and phase's 217.

**Loss by piece count** (`pieces.tsv`, lambda 0.02, each run's last round, against SCReLU's):

| pieces | taper | phase |
|---|---|---|
| 3 | -8% | -34% |
| 4 | -5% | -36% |
| 5 | +1% | -18% |
| 6 | -1.6% | -10% |
| 7 to 13 | -1.4% to +0.6% | -1.9% to -4.8% |
| 14 up | within +/- 0.5% (31: +1.3%) | within +/- 0.8% (31: -4.3%) |

The round 1 gain (-14% to -19% at 3 to 5 pieces) faded by round 24 and never came back; only 3 and
4 pieces keep a gain, too few positions to move the total. The prediction (between SCReLU and
phase) failed: level with SCReLU.

**Duels, both `@default`, against `lc0-psq-threats-screlu-g1-r1`:** test 15, depth 9, 452 of 914,
49.5%, about -4 Elo; test 16, 10+0.1, 451.5 of 922, 49.0%, about -7 Elo; both drawn (curtailed).

**A hypothesis, not tested:** the taper adds `(p / 24) * (w_mg - w_eg) . x`, a term linear in the
phase times the features. The phase is linear in the piece features, and SCReLU's squares already
give products of accumulator values, so the plain head can approximate that term once trained; the
taper is only a head start. Phase's buckets are a step function of the phase, which the squares
cannot build, consistent with phase keeping -2% to -5% at 7 to 13 pieces where the taper has none.
If so, the material-blended taper (the follow-up, PeSTO's 24 replaced by material on the 78 scale)
is linear too and would fail alike: dropped.
