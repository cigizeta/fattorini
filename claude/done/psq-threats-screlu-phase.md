# psq_threats_screlu with output buckets by game phase

**Status:** implemented 2026-10-24, steps 1 to 6, every test passing. Next: the run on the
training machine (step 7).

## Why

psq_threats_screlu_material (psq-threats-screlu-material.md) beat psq_threats_screlu by about 1.3%
in test loss but drew both duels (48.3% at depth 9, 49.8% at 10+0.1). The developer's hypothesis:
78 output buckets are too many. The experiment: the same network with PeSTO's game phase as the
bucket key, 25 buckets instead of 78.

Claude's caveat, recorded: the head is factorised (a shared set plus a delta per bucket), so a rare
bucket leans on the shared weights rather than starving, and 78 buckets showed no overfitting in
test loss. The phase also drops pawns from the key. The lambda 0.02 duel of run 1 (TODO.md) is still
worth running beside this.

## Decisions (the developer, 2026-10-24)

1. **The phase:** knight and bishop 1, rook 2, queen 4, both sides, pawns and kings 0, capped at 24
   (`pesto::PHASE_INC`, `pesto::PHASE_MAX`): 25 buckets, phase 0 to 24, a promotion staying in 24.
   phase-distribution.md measured it: no bucket starved, the rarest (23) 1.2% of the rows.
2. **A config of its own:** `configs/lc0-psq-threats-screlu-phase.yaml`, a copy of
   `lc0-psq-threats-screlu-material.yaml` with the architecture changed; the material config stays
   as it is. (First decided as the material config's next run, changed the same day.)
3. **Still factorised:** shared plus delta, as the material head, so only the bucket key changes.

## Design

- **A new architecture, `psq_threats_screlu_phase`** (`ArchitectureKind::PsqThreatsScreluPhase`),
  beside the material one, everything shared but the bucket key and count.
- **A new feature-set tag, 14, `psq_threats_screlu_phase`:** same layout as tag 13, 25 output sets
  instead of 78. The file needs no new field, nor the in-memory network: the number of sets tells
  the key (`stack_buckets::output_bucket_of_board`).
- **One bucket definition** in `stack_buckets.rs`, beside the material one: `PHASE_BUCKETS = 25`,
  `phase_bucket(phase)` (capped), `phase_of_canonical`, `phase_bucket_of_board` (from the bitboards'
  counts, as `material_bucket_of_board`). It calls `pesto`'s constants, so the engine, the trainer
  and `smithy phases` cannot drift apart.
- **The batch:** a fourth bucket column, the phase, computed where the other three are
  (`batch.rs`, `buckets_of_canonical` becoming `[usize; 4]`). Nothing is stored on disk: the columns
  are computed per batch. The material head reads column 2 or 3 by its key.

## Bench (2026-10-24, one idle machine, random networks, width 256, depth 9)

Per search node: psq_threats_screlu 187.5 ns, with 78 material sets 191.9 ns, with 25 phase sets
186.6 ns. The phase costs nothing measurable. (Its "from scratch" line, 1515 against about 1310 ns,
is the refresh, which the phase does not touch: noise.)

## Plan

1. **Buckets** (`crates/rotor/src/stack_buckets.rs`): the phase functions and constant; tests on
   the start (24), kings and pawns (0), a queen each (8), a third queen (28, capped 24), board and
   canonical agreeing, Black to move as well as White.
2. **Engine** (`crates/rotor/src/nnue.rs`): tag 14 and its name; loading reads `PHASE_BUCKETS`
   sets for 14; `validate` accepts 25 sets for screlu only; `write` picks 14 for 25 sets; the f32
   and quantised heads take the bucket by the set count. Tests: tag 14 round-trips, pairwise refuses
   25 sets, the phase's set is chosen, quantised close to f32.
3. **Bench** (`nnue_bench --material --screlu --phase`): one line, to check the phase costs nothing
   either.
4. **Trainer:** `ArchitectureKind::PsqThreatsScreluPhase` (`net_config.rs`, `screlu()` and the
   bucketed head including it, a load test); the batch's fourth column (`batch.rs`);
   `StackHead::material` sized and its column chosen by the kind (`stack_head.rs`); the export
   writing 25 sets (`export.rs`, its agreement test extended to tag 14); `candidates.rs`' lists;
   the smithy's forge-config test.
5. **Config:** `configs/lc0-psq-threats-screlu-phase.yaml`, copied from the material one.
6. **Docs:** TODO.md, architectures.md.
7. Build, test, then the run on the training machine; the duels against
   `lc0-psq-threats-screlu-g1-r1` as before (depth 9, then 10+0.1).
