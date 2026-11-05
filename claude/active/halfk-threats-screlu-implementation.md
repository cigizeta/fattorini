# halfk_threats_screlu: implementation plan

Status: written 2026-11-02. Step 0 done the same day: tests and clippy passing, the bench's nodes
unchanged (3,684,206 before and after; the 3,117,647 of 2026-10-28 predates `corr` and `blend`).
Step 1 written the same day, to test. As built: `psq_threats::push_threat_rows_from` (first row and
mirror) beside the unchanged `push_threat_rows`; `halfk::piece_row` takes the unmirrored oriented
square and mirrors it itself; no castling or en passant rows, as Stockfish's. Step 1 done.
Step 2 done the same day, tests and clippy passing, the bench's nodes unchanged. As built: tag 20; `PsqNetwork::halfk` (and the quantized
network's); `halfk::piece_rows` added for the engine's incremental part; the refresh cache a
thread-local keyed by a `CacheId` each quantization takes, so it never needs clearing (not on
`ucinewgame` either: an entry always holds the exact sums of the pieces it records); the search's
code untouched, `update` refreshing a perspective whose king changed bucket or mirror.
Step 3 done the same day. As built: `nnue_bench --depth 9`'s own search stands for `bin/rotor
bench`, which cannot take a random network.

**The bench (2026-11-02, one idle machine, random networks, width 256, two runs each):**

| | update | head | per node | search per node (depth 9) |
|---|---|---|---|---|
| psq_threats_screlu | 34.4, 32.7 ns | 114.2, 114.3 ns | 148.6, 147.0 ns | 0.68, 0.68 us |
| halfk_threats_screlu | 42.3, 37.4 ns | 122.0, 109.9 ns | 164.4, 147.3 ns | 0.66, 0.67 us |

The update about 4 to 8 ns dearer (its rows spread over an 11.5 MB transform), the head the same
within noise, a search node the same: no measurable cost at depth 9, where the refresh cache meets
real king moves. The searches are short (three positions, about 0.1 s), so a cost of a few percent
would not show; the 10+0.1 duel judges it.

Steps 4 to 6 done the same day, tests and clippy passing. Left: step 7's scaled run. As built: `FeatureSet::HalfKav2Hm`'s batches put every
piece row's factor row after the bag's own rows (`halfk::TRAINING_ROWS`, the factor rows past the
engine's); the starting weights rescale `nn::linear`'s draw from the training width to psq_threats'
(`NetworkPsq::start_factorised`), the piece rows then zeroed; `DenseBatch::psq` reads any kind's
psq-form batch; the export runs `QuantizedPsqNetwork::quantize` before writing. Both configs load
from the committed list's test; `lc0-halfk-threats-screlu` reads today's Lc0 corpus until the
forges read `data/lc0-counts`. Implements halfk-threats-screlu.md. Each step with its
tests; the developer runs them and commits between steps.

## Findings that shape the plan (2026-11-02, from the code)

- **Threats are already apart in the engine.** psq_threats' threat rows are never kept in the
  incremental `i16` accumulator: they are added at evaluation, in `i32`, saturating
  (`QuantizedPsqNetwork::quantize`'s comment, `add_rows_saturating_i16_avx2`). So the design's
  decision 8 (two accumulators) is today's structure; halfk replaces only the incremental part.
- **The engine quantises at load, not the exporter.** The file holds f32; `quantize` picks the
  largest scale at which every lane's worst case (bias plus its `MAX_ACTIVE_ROWS` largest rows)
  and every weight fit `i16`, and refuses the network otherwise. The design's decision 7 maps onto
  this: no clip in training, and the export refuses by running the engine's own `quantize` on the
  folded network before writing, never saturating. For halfk the worst case is taken per king
  bucket (the largest rows within one bucket's 704, since only one bucket is active per
  perspective), much tighter than over all 22,528 rows.
- **Row counts fit today's types.** halfk plus threats is 22,528 + 4,096 = 26,624 rows, plus 704
  factor rows in training only: `u16` row indices and the batch's `i32` flat indices (16,384 test
  rows x 27,328, about 448M) both fit.

## Steps

### 0. The old halfkp and halfka_v2_hm removed

- `rotor::features::{halfkp, halfka_v2_hm}` and their tests; `FEATURE_SET_HALFKP` and
  `FEATURE_SET_HALFKA_V2_HM` (`nnue.rs`), their names in the tag table, and the test refusing a
  halfkp file. Tags 1 and 2 are left unused for good, so an old file can never load as anything.
- The trainer: `FeatureSet::HalfKp`, the old `HalfKav2Hm`, `DenseBatchHalfKp`,
  `DenseBatchHalfKav2Hm`, `HostBatch`'s two dense arms, `dense_halfkp`, `dense_halfka_v2_hm`, both
  `accumulate_perspective_*`, `NetworkHalfKp`, `NetworkHalfKav2Hm`, and every match arm and test
  naming them. `DenseBatch` keeps its psq arm.
- No behaviour change for any psq network: the bench's nodes and every psq test unchanged.

### 1. The features (`crates/rotor/src/features.rs`, `pub mod halfk`)

- `BUCKETS = 32`, `IDENTITIES = 11`, `HALFK_ROWS = 32 * 11 * 64 = 22,528`, `THREAT_ROW =
  HALFK_ROWS`, `ROWS = THREAT_ROW + 64 * 64`, `FACTOR_ROWS = 11 * 64` (the trainer's).
- `king_bucket(king: Square) -> (bucket, mirrored)`: the king oriented for the perspective
  (`oriented_square`), mirrored left-right when on files e to h; bucket = rank * 4 + file.
- `row(bucket, mirrored, square, identity)`: the piece oriented, then mirrored with its king;
  identity: our pawn to queen 0 to 4, theirs 5 to 9, either king 10 (Stockfish's shared plane).
  Our own king is a feature too, in plane 10, as in Stockfish's halfka_v2_hm.
- `threat_row` and `push_threat_rows` generalised to take the first threat row and the mirror, so
  psq_threats' callers pass `psq::ROWS` and no mirror, unchanged.
- `expand(canonical) -> PositionFeatures` as psq_threats': both perspectives' halfk rows then their
  threat rows. `factor_row(row) -> usize` for the trainer: `HALFK_ROWS`-relative identity and square,
  dropping the bucket.
- Tests: hand-computed rows on a position with each king on a- and on h-files (mirrored and not),
  both perspectives; the starting position's 32 rows per perspective; the colour-swapped mirror
  giving swapped perspectives (psq_threats' test reused); threat rows mirrored with their king.

### 2. The engine (`crates/rotor/src/nnue.rs`)

- A new tag (the next free one), its name `halfk_threats_screlu`, its shape `(halfk::ROWS, 1)`;
  `load`, `write` and `validate` accept it with screlu, no hidden layer, no stack, no output sets.
- `quantize`: for halfk, each lane's worst case is the bias plus the largest `MAX_ACTIVE_ROWS`
  rows within one bucket, the maximum over the 32 buckets.
- The accumulator: the incremental part over halfk rows; threats as today. A move of a perspective's
  own king that changes its bucket or its mirror refreshes that perspective; the other perspective
  updates incrementally (its own king did not move).
- **The refresh cache:** per search thread, per perspective, one entry per king bucket and
  mirror: 64 entries a perspective, since a bucket is reached unmirrored from files a to d and
  mirrored from e to h (d1 and e1 share bucket 3, every piece flipped between them). A king crossing
  the d/e line changes its mirror, so it goes through the cache like any bucket change (Stockfish's
  way, the design's open question). Each entry an accumulator plus the pieces' bitboards it was computed from. A refresh
  takes the entry, adds and removes the rows of the pieces that differ, stores it back, and copies
  it into the stack's accumulator. Memory: 2 x 64 x 256 x 2 bytes plus bitboards, about 66 KB a
  thread. Entries start as the bias with no pieces. Cleared on `ucinewgame` and a network load.
- Tests, as psq_threats_screlu's: write and load; quantized within tolerance of f32; vectorized
  equal to scalar; incremental equal to from scratch along random games that move kings across
  buckets and across the d/e line, with distinct weights per bucket so a wrong bucket fails; the
  cache equal to a full rebuild after many refreshes; the per-bucket bound refusing a network that
  overflows and accepting one that fits only per bucket.

### 3. The bench (`crates/rotor/benches/nnue_bench.rs`, then the developer)

- `--halfk --screlu --threats` builds a random network of this shape (width 256).
- The developer, on one idle machine, back to back: `nnue_bench` for psq_threats_screlu and for
  this, then `bin/rotor bench` at depth 9 with each random network, which plays real king moves
  (the bench's positions under search exercise the cache as games do; `nnue_bench`'s fixed
  positions are the worst case, as kings' were). The numbers go here, then the developer decides
  whether to train.

### 4. The trainer (`smithy`)

- `FeatureSet::HalfKav2Hm`, now the new halfk: sparse batches as psq_threats'
  (`active_indices` at `halfk::ROWS + halfk::FACTOR_ROWS`), each halfk row followed by its factor
  row (`halfk::ROWS + factor_row`). Threat rows as they are.
- `ArchitectureKind::HalfkThreatsScrelu` (`net_config.rs`, `kind: halfk_threats_screlu`):
  `screlu()` true, `hidden_dim` empty, feature set `HalfKav2Hm`.
- `NetworkPsq::with_rows` serves it at `halfk::ROWS + halfk::FACTOR_ROWS` rows. Initialisation:
  the 22,528 real rows zeroed after construction; the factor and threat rows, and the bias, drawn
  as a fresh psq_threats_screlu transform would be (its bound, from its own fan-in), so step 0's
  network is a fresh psq_threats_screlu's in distribution.
- Export (`export.rs`): each real row plus its factor row, the factor rows dropped, the threat rows
  as they are, written with the new tag; the engine's `quantize` run on the result first, the export
  refused with its message if it fails. `candidates.rs`' lists.
- Tests: the batch's indices for a hand-computed position, factor rows included; the factorised
  network's value equal to the folded network's on the same positions (tch forward against the
  engine's f32 forward through a file); the real rows zero at construction.

### 5. Configs

- `configs/lc0-halfk-threats-screlu.yaml`: `lc0-psq-threats-screlu.yaml` with its kind and
  header, reading `data/lc0-counts` like step 0's config.
- `configs/pesto-halfk-threats-screlu.yaml`: the identity line's, from `pesto-psq`'s, its grid
  0.91 to 0.99 (grid centre 0.95). Its form to check against `pesto-psq.yaml` when written.
- The config tests list both; the page's order of configs.

### 6. Docs

architectures.md's halfk_threats entry (step 1 of the line, SCReLU, factorised); TODO.md's halfk
item pointing here; DEV.md if a command changes.

### 7. Checks

Tests and clippy; a scaled run (1/1024) of `lc0-halfk-threats-screlu` from the page on any
machine; then, after step 0 and the bench's decision, the full run on the training machine.

## Commands for the developer

After each step:

```bash
cargo test --workspace --release
```

```bash
cargo clippy --workspace --all-targets --release -- -D warnings
```
