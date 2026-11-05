# psq_threats_screlu_material

Status: design agreed and plan approved 2026-10-24 (the developer); all five steps done the same
day, tests passing, the buckets costing nothing; the run on the training machine next.

## What

psq_threats_screlu (screlu.md) with its one output replaced by 78 weight sets, chosen by the material
on the board (1/3/3/5/9, pawns included, both sides, 1 to 78), no king buckets, no hidden layer:
`(4876 -> 256) x 2`, each lane clipped and squared, the 512 squares into the output of the position's
material bucket. The material head of psq-threats-pairwise-material.md, moved from pairwise's products
onto SCReLU's squares.

## Why

SCReLU won its two duels against threats by a little (TODO.md: 52.0% over 940 games at depth 9, 51.6%
over 932 at 10+0.1) and is the line now. Output buckets are the usual next step of single-layer SCReLU
networks (bullet's engines), and cost the engine nothing: one set read in place of another. A head
with no hidden layer is linear in its inputs, so one weight set must serve an opening and a pawn
ending alike; 78 sets let each material weigh the same features differently.

## Decisions (the developer, 2026-10-24)

1. **The existing 78 material buckets** (`stack_buckets::material_bucket`, `MATERIAL_BUCKETS`),
   factorised as before: each set a shared part plus its own delta, deltas starting at zero, summed
   once by the export. Rejected for now: 8 buckets by piece count (bullet's usual), which needs a new
   bucket function and a new batch column; a cheap follow-up if 78 shows nothing.
2. **A new config, `configs/lc0-psq-threats-screlu-material.yaml`:** `lc0-psq-threats-screlu.yaml`
   with its kind and header changed, nothing else (corpus, lambdas 0.01 to 0.09, schedule, gate
   against PeSTO).
3. **pairwise-material kept,** its kind and config, not deleted yet.
4. **The bench first,** before the training machine trains.

## Judged by

Its winner against `lc0-psq-threats-screlu-g1-r1`, both `@default`, from the page's tests: a duel at
depth 9, then, if at least level, one at 10+0.1. Judged by the games alone; its loss recorded beside
SCReLU's, not read as a verdict.

## Implementation plan

Each step with its tests; the developer runs them and commits between steps.

1. **The engine** (`rotor/src/nnue.rs`):
   - `FEATURE_SET_PSQ_THREATS_SCRELU_MATERIAL = 13`, named `psq_threats_screlu_material`, read and
     written: `from_bytes` sets `screlu` and reads the 78 sets as tag 11 does (`sets`, the head input
     `2 * embedding_dim` instead of pairwise's `embedding_dim`); `write` picks the tag from
     `screlu` and `material` together.
   - `validate`: screlu now allows material sets (still refusing pairwise, a stack or a hidden layer);
     the material check allows screlu as well as pairwise, its sets' input `2 * embedding_dim` for
     screlu.
   - The f32 head: `evaluate_material_accumulators` takes the screlu path through the bucket's set
     (the squares, own first) when `screlu`, pairwise's otherwise.
   - Quantised: `evaluate_screlu_lanes` accepts `QuantizedHead::Material`, the bucket's weights,
     scale and bias, through the same `screlu_dot_i16_avx2` and its scalar twin; `buckets` already
     gives the material bucket for a `Material` head.
   - Tests, as tag 11's and 12's: write and load as its own kind; wrong set count or shape refused;
     the f32 head takes the board's bucket's set (against a hand computation); quantised close to f32
     and vectorised equal to scalar; incremental equal to from scratch across captures, threat lanes
     saturated, several buckets visited.
2. **The bench** (`benches/nnue_bench.rs`): `--material` allowed with `--screlu` (today with
   `--pairwise` only), a random screlu material network. Then the developer, on one idle machine:

   ```bash
   cargo bench --bench nnue_bench -- --threats --screlu --width 256 --head "" --depth 9
   ```

   ```bash
   cargo bench --bench nnue_bench -- --material --screlu --width 256 --depth 9
   ```

   expected equal within the noise (about 18 ns on the update, screlu.md); the numbers go here.
3. **The trainer** (`smithy`):
   - `net_config.rs`: `ArchitectureKind::PsqThreatsScreluMaterial` (`psq_threats_screlu_material`),
     `screlu()` and `material()` both true, `pairwise()` false; refused with a hidden layer; the
     config test.
   - `nnue_network.rs`: the screlu assertion allows `HeadShape::Material`; the material head built on
     `2 * embedding_dim` inputs when not pairwise (already `head_input`'s rule).
   - `stacked_network.rs`: the lockstep head squares then reads the material stage, as
     `NetworkPsq`; its test, every screlu material arm computes what its unstacked network does and
     gets its own gradient.
   - `export.rs`, `candidates.rs`: `material_network_from` with `screlu`, its head input
     `2 * embedding_dim`, writing tag 13; the agreement test, the engine's forward pass against tch's
     through a file, deltas randomised.
   - `main.rs`: the new variant wherever the kinds are matched.
4. **The config:** `configs/lc0-psq-threats-screlu-material.yaml`, and the page's order of configs
   (`page.html`) with it after `lc0-psq-threats-screlu`.
5. **The docs:** architectures.md's entry; TODO.md's item pointing here.

Then the run on the training machine, generation 1, and the two duels.

## The bench (step 2, the training machine, 2026-10-24)

Random networks, width 256, no hidden layer, `nnue_bench --depth 9`, one after the other on the training machine:

| | head alone | update | per search node | search, per node |
|---|---|---|---|---|
| psq_threats_screlu | 158.9 ns | 37.4 ns | 196.2 ns | 0.66 us |
| psq_threats_screlu_material | 162.2 ns | 34.6 ns | 196.8 ns | 0.73 us |

**The buckets cost nothing measurable:** 0.6 ns a node of evaluation, inside the noise. The search
column is not comparable: the two random networks search different trees (346 nodes against 9,182 on
the start position), and a few thousand nodes time poorly.
