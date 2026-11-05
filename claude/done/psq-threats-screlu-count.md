# psq_threats_screlu with output buckets by piece count

**Status:** designed 2026-10-25; step 1 done, tests passing; step 2 done, tests passing; step 3 done, benched; steps 4 and 5 done, tests passing; step 6 done; a dev smithy smoke run ended without errors 2026-10-25. Next: the run on the training machine.

## Why

psq_threats_screlu_material (78 buckets) and psq_threats_screlu_phase (25 buckets,
psq-threats-screlu-phase.md) both key the output on what the pieces are. Stockfish keys its 8 layer
stacks and its 8 PSQT outputs on how many there are: `(pieces including kings - 1) / 4`, 2 to 32
pieces into buckets 0 to 7. The experiment: the same network with Stockfish's key, 8 buckets, the
fewest and best filled of the three.

## Decisions (the developer, 2026-10-25)

1. **The key:** Stockfish's, `(count - 1) / 4` over every piece on the board, kings and pawns
   included: 8 buckets. A count above 32 cannot happen (promotions replace a pawn); the bucket is
   capped at 7 all the same.
2. **No dense layer:** `hidden_dim: []`, as the phase and material configs. Only the output's
   weight set is chosen by the bucket.
3. **Still factorised:** shared plus delta, as the material and phase heads, so only the bucket key
   changes against them.
4. **No PSQT skip term:** Stockfish's bucketed PSQT outputs of the accumulator are left out; a later
   experiment if the key pays.
5. **Name:** architecture `psq_threats_screlu_count`, config
   `configs/lc0-psq-threats-screlu-count.yaml`, a copy of the phase config with the architecture
   changed.

## Design

- **A new architecture, `psq_threats_screlu_count`** (`ArchitectureKind::PsqThreatsScreluCount`),
  beside the phase one, everything shared but the bucket key and count.
- **A new feature-set tag, 15, `psq_threats_screlu_count`:** tag 13's layout with 8 output sets. No
  new field in the file or in memory: the number of sets tells the key, as for the phase
  (`stack_buckets::output_bucket_of_board`), and 8 differs from 25 and 78.
- **One bucket definition** in `stack_buckets.rs`: `COUNT_BUCKETS = 8`, `count_bucket(count)`
  (capped), `count_of_canonical` (the canonical position's pieces plus the two kings),
  `count_bucket_of_board` (a popcount of the occupied squares).
- **The batch:** a fifth bucket column, the count, computed beside the phase in `batch.rs`. Nothing
  stored on disk. `StackHead` reads it for `HeadShape::Count`.

## Bench (2026-10-25, one machine, random networks, width 256, depth 9)

Per search node: with 8 count sets 384.1 ns, with 25 phase sets 371.7 ns, run back to back. The
3% gap is noise: the count's key is two popcounts against the phase's eight weighted ones, and the
"from scratch" line, which no bucket touches, moved 6% between the runs (2407 against 2280 ns). The
count costs nothing measurable. (Both twice 2026-10-24's 186.6 ns: another machine or load; the
comparison stands, both sides on the same one.)

## Plan

1. **Buckets** (`crates/rotor/src/stack_buckets.rs`): the constant and functions; tests on the start
   (32, bucket 7), kings alone (2, bucket 0), 5 pieces (bucket 1), 28 and 29 (buckets 6 and 7),
   board and canonical agreeing, Black to move as well as White.
2. **Engine** (`crates/rotor/src/nnue.rs`): tag 15 and its name; loading reads `COUNT_BUCKETS` sets
   for 15; `validate` accepts 8 sets for screlu only; `write` picks 15 for 8 sets;
   `output_bucket_of_board` dispatches 8 sets to the count. Tests: tag 15 round-trips, pairwise
   refuses 8 sets, the count's set is chosen, quantised close to f32.
3. **Bench** (`nnue_bench --material --screlu --count`): one line, to check the popcount costs
   nothing.
4. **Trainer:** `ArchitectureKind::PsqThreatsScreluCount` (`net_config.rs`, `screlu()` and the
   bucketed head including it, a load test); the batch's fifth column (`batch.rs`);
   `HeadShape::Count` in `stack_head.rs`; the export writing 8 sets (`export.rs`, its agreement test
   extended to tag 15); `candidates.rs`' lists; the smithy's forge-config test.
5. **Config:** `configs/lc0-psq-threats-screlu-count.yaml`.
6. **Docs:** TODO.md, architectures.md.
7. Build, test, then the run on the training machine; duels against `lc0-psq-threats-screlu-g1-r1` and the phase
   winner (depth 9, then 10+0.1).
