# NNUE architecture revision: why amanda is too slow, and what replaces it

**Archived:** a 2026-09-28 discussion, nothing agreed; the speed problem it raised was answered by the smaller networks of architectures.md (psq at 256, optional hidden 16) and psq-threats.md. Its amanda is the old naive 512-wide lineage; the name has since been reused.

Status: under discussion, nothing agreed or implemented. Written 2026-09-28 on the older PC
(another machine) at the end of a session, so the next one (possibly on another PC) can resume with no other
context. Resume by reading this, then "Open decisions" below.

## What happened

amanda's generation 1 was trained on the hand-crafted evaluation's self-play games and promoted
(N_1, stored as `data/networks/amanda/gen-0001/champion.*`, see champion-in-its-generation.md).
Two observations the same day:

- On Lichess (`rotor-bot`, via lichess-bot, `rotor-train install amanda`) it lost every game.
  Expected for a first generation; not by itself a reason to change anything.
- Self-play with N_1 (`rotor-arena self --network amanda` on another machine) ran at about
  **7.2 positions/s**. The hand-crafted evaluation's self-play ran at **300 and 600 positions/s**
  on the other two PCs. At N_1's rate a generation (`corpus_positions` = 17,301,504 in
  `configs/self_play.yaml`) takes weeks even across all machines. Self-play on another machine was stopped;
  its unfinished batch in `data/self/amanda/gen-0001/` is local only and can be deleted.

Decision taken by the developer: **amanda is abandoned as a lineage; the architecture has to be
revised.** amanda's N_1 stays installed for Lichess and as a reference point.

## Why the network is slow

amanda (`configs/amanda.yaml`): `kind: naive`, `embedding_dim: 512`, value head `[256, 128, 64]`.
`NaiveNetwork::evaluate_raw` (`src/nnue.rs`) recomputes everything at every node, in `f32` with the
AVX2/FMA kernels of `src/simd.rs`. Per evaluation:

| Stage | Operations | Share |
|---|---|---|
| Feature transform (up to 69 active rows x 512) | about 35k adds | 17% |
| Head layer 1 (512 x 256) | 131k multiply-adds | 63% |
| Head layers 2-4 (256x128, 128x64, 64x1) | about 41k | 20% |
| Total, every node | about 207k | |

At a realistic 5-10 GFLOP/s per core that is 20-40 us per evaluation, 25-50k evaluations/s per
thread, which accounts for the 40-80x drop against the hand-crafted evaluation. It is the opposite
of how NNUE gets its speed: Stockfish-style nets keep the feature transform wide (cheap, because it
is updated incrementally) and the head tiny. Here most of the cost is the head, which cannot be
updated incrementally, so an incremental accumulator on amanda's shape would save only about 17%.

## Why the features are also a problem

- **`naive` does not encode where a piece is, only its file and its rank, separately**
  (`features::naive::dense_rows`, `src/features.rs`). A knight on c3 activates "own knight, file c"
  and "own knight, rank 3" as two independent rows. Own knights on c3 and f6 give exactly the same
  input as own knights on c6 and f3. Its whole input is 204 rows; its own doc comment calls it "the
  cheap baseline".
- **One perspective**, oriented for the side to move. Every move flips the side to move and so
  reorients every feature: an incremental accumulator needs two copies, one per color.

## What the code already has

- Training supports two-perspective networks (`src/train/nnue_network.rs`: `NetworkHalfKp`,
  `NetworkHalfKav2Hm`): one shared `transform` applied per perspective, the two outputs
  concatenated (own first), then the shared `ValueHead`. Adding another feature set follows that
  pattern (`FeatureSet`, `batch.rs` dense expansion, `features.rs` module).
- The engine has a forward pass for `naive` only (`src/nnue.rs`); `export_challenger` skips the
  other kinds with a note. The `.nnue` format has a feature-set tag (`FEATURE_SET_*` in `nnue.rs`).
- The search (`src/search.rs`) keeps one `Board`, calls `make_move`/undo on a scratch copy, and
  evaluates through `Searcher::evaluate(&Board)` (network or hand-crafted), which reads the whole
  board. There is no hook for an incremental update today.
- `benches/nnue_bench.rs` times the hand-crafted evaluation, feature extraction, and a network
  evaluation (random weights of amanda's shape, or a real `.nnue`), plus optionally a full search
  to `--depth N`. Its shape is hard-coded (`EMBEDDING_DIM`, `HIDDEN_DIMS`).

## Proposal (the original recommendation; "Decisions taken" below supersedes it where they differ)

1. **Feature set: plain piece-square, 768 = 12 piece types x 64 squares, one set per
   perspective.** The simplest standard NNUE input; knows exactly where every piece is. A move
   changes 2-4 rows per perspective (4 for castling, 3 for a capture, 2 otherwise, plus promotion
   and en passant cases), with no king bucketing, so a king move needs no full refresh.
   `halfka_v2_hm` is stronger in principle but needs a full refresh on king moves and far larger
   tables: a later experiment, once the simple version works.
2. **Width and head: 256 per perspective, concatenated to 512, then straight to the output
   (512 -> 1)**, or at most one hidden layer of 16. Per node: an incremental update of about
   4 x 256 adds per perspective, and about 512 multiply-adds to evaluate. Roughly 100x less work per
   node than amanda, which should bring search speed near the hand-crafted evaluation's.
3. **Float first**, reusing the AVX2 kernels. Quantizing (int16 accumulator, int8 head, as
   Stockfish does) only once float works and is measured.
4. **Incremental update in the search**: an accumulator stack beside the board, one entry per
   ply, pushed with `make_move` and popped on undo, refreshed from scratch at the root. The biggest
   code change, and entirely engine-side. Must agree exactly with a from-scratch evaluation (a test
   comparing the two after random move sequences).

Not affected: self-play, datasets and labels, the generation layout, the gate, remote and install.
The new network is a new lineage (name to choose, e.g. "betty"), starting at generation 1 from the
hand-crafted games already on the Drive (`data/self/hand-crafted/`), so no new self-play is needed
to start it. Its `prepare` builds a new dataset cache for the new feature set.

## First step once agreed: measure before building

Extend `benches/nnue_bench.rs` to time the proposed shape with random weights: a from-scratch
evaluation, the incremental update of one move, and the head alone. Compare against amanda's shape
and the hand-crafted evaluation. That confirms the expected speedup before the search changes and
the training side are written.

## Decisions taken

- **Feature set `psq`**, two perspectives: 768 piece-square rows (12 piece types x 64 squares)
  plus 4 castling rights (own king/queen side, their king/queen side) and 8 en passant files,
  780 rows per perspective, at most 37 active. Castling and en passant cost one row each in the
  incremental update and change rarely. Informally known elsewhere as "768" or bullet's
  `Chess768`; named `psq` because an identifier cannot start with a digit, YAML would read
  `kind: 768` as an integer, and the set is 780, not 768. King buckets, if ever added, become an
  option of `psq`, not a new name.
- **The new lineage is named betty** (`configs/betty.yaml`, `data/networks/betty/`).
- **Accumulator width 256 per perspective** (512 concatenated).
- **`naive` is kept, not retired**, feature set and engine forward pass: amanda's N_1 plays on
  Lichess with it. The name stays with the file-and-rank feature set; `psq` does not reuse it.
- **Float first, then integers, benchmarking each.** Float (AVX2 kernels) validates the
  architecture, training and incremental search in isolation; quantization follows as its own
  change: int16 accumulator (scale QA = 255, clipped ReLU as clamp to [0, QA]), int16 head
  (scale QB = 64, `_mm256_madd_epi16`), weights clipped in the trainer so the worst case (bias
  plus 37 active rows) fits in int16. int8 head only if the benchmark shows the head dominates.
  The float incremental-vs-from-scratch test needs a tolerance (rounding drift along a line of
  play); the int version must be exactly equal, and the float forward pass stays as the reference
  the quantized one is tested against. `benches/nnue_bench.rs` must take the network shape as a
  parameter instead of the hard-coded `EMBEDDING_DIM`/`HIDDEN_DIMS`, to time each head candidate
  in float and then in int.
- **First network: (780 -> 256) x 2 -> 1**, no hidden layer. Tried first; wider accumulators
  (512, 1024 per side), hidden layers (16 -> 16, 32 -> 32, 16 -> 32), output buckets by piece
  count and SCReLU are later candidates, each measured against it.

## Implementation plan

Each step ends with `cargo test` green and is one commit. Steps 1-7 are float; step 8 is the
integer version. Nothing in `naive`'s behavior changes at any step: amanda's N_1 must keep loading
and playing identically, checked by its existing tests (`nnue.rs`, `train/export.rs`) staying green
untouched.

Two choices made here rather than in the discussion, flagged so they can be vetoed:

- **Activation: clipped ReLU, clamp(x, 0, 1), for `psq` everywhere** (after the transform and
  after each hidden layer), in training and in the engine. Training in plain ReLU and quantizing
  later would change the function at step 8; training in clipped ReLU from the start makes the
  float and int networks the same function up to rounding. `naive` keeps plain ReLU.
- **The incremental delta is computed by diffing two boards, not inside `make_move`.** `Board` is
  `Copy` and every search site already holds the parent (`board`) and the child (`scratch`) side by
  side. XOR of each of the 12 piece bitboards gives the changed squares: removed = parent AND
  changed, added = child AND changed. Castling, en passant captures and promotions then need no
  special cases, and `make_move` (perft-tested) is not touched. Castling rights and the en passant
  file are compared directly. 12 XORs per move is negligible next to the row updates.

### 1. Bench takes the shape as a parameter

`benches/nnue_bench.rs`: replace `EMBEDDING_DIM`/`HIDDEN_DIMS` with `--width N` and
`--head a,b,...` (empty for no hidden layer), defaulting to amanda's shape so today's output is
unchanged. No `psq` code exists yet, so this step only makes the existing measurement
configurable. `NaiveNetwork::validate` and `load` require at least one hidden layer today; lift
that in `nnue.rs`, since `psq` shares `Layer` and its first head has none.

### 2. Feature set `psq` in `src/features.rs`

New `pub mod psq`, beside `naive`, reading the same `CanonicalPosition` (no cache format change):

- Row layout per perspective: `identity * 64 + oriented_square` for the 768 piece rows, identity
  `0..12` as `piece_type * 2 + (0 own | 1 opponent)` (kings included); `768..772` castling (own
  kingside, own queenside, opponent kingside, opponent queenside, relative to the perspective);
  `772..780` en passant file. `pub const ROWS: usize = 780`.
- Orientation per perspective via `oriented_square(square, perspective_color)`; the en passant file
  needs none (a vertical mirror keeps files).
- `pub fn active_rows(canonical, perspective_color) -> Vec<u16>` and
  `pub fn expand(canonical) -> PositionFeatures { own: Vec<u16>, opponent: Vec<u16> }`, own = side
  to move. Both engine and training call `active_rows`, so the row layout has one definition.
- `pub fn row(identity, oriented_square)`, `CASTLING_ROW`, `EN_PASSANT_ROW`, used by the
  incremental delta in step 5.
- Tests: starting position has 32 piece rows + 4 castling rows per perspective; the White and Black
  perspectives of a colour-symmetric position are equal; castling and en passant land in their rows;
  rows are all `< ROWS` and never repeat (unlike `naive`, a `psq` row is 0 or 1).

### 3. Training: `FeatureSet::Psq`

- `src/train/feature_set.rs`: variant `Psq`, CLI string `"psq"`, discriminant 3.
- `src/train/net_config.rs`: `ArchitectureKind::Psq`.
- `src/train/batch.rs`: `DenseBatchPsq { own_dense, opponent_dense, label }`, `[batch, 780]` each,
  set to 1.0 at each active row, and the matching `to_device` / `to_dense_batch` arms and tests,
  following `DenseBatchHalfKp`.
- `src/train/value_head.rs`: allow an empty `hidden_dims` (the output layer reads the input
  directly), and take the activation as a parameter (ReLU for the existing kinds, clipped ReLU for `psq`).
- `src/train/nnue_network.rs`: `NetworkPsq`, the same shape as `NetworkHalfKp` (one shared
  `transform` of `psq::ROWS -> width`, applied to both perspectives, concatenated own first) with
  clipped ReLU.
- `configs/betty.yaml`: `kind: psq`, `embedding_dim: 256`, `value_head.hidden_dim` from the step 7
  `[]`, the rest copied from `amanda.yaml`.

### 4. Engine forward pass and `.nnue` format

`src/nnue.rs`:

- `FEATURE_SET_PSQ = 3`. The header is unchanged in shape; for `psq` the transform is
  `[psq::ROWS][width]` (transposed, as for `naive`) and the head's input is `2 * width`.
- `PsqNetwork { width, transform, transform_bias, hidden, output, step, learning_rate }` with
  `refresh(board, perspective) -> Vec<f32>` (bias + active rows),
  `evaluate_accumulators(own, opponent) -> f32` (clipped ReLU, concatenate, head), and
  `evaluate_raw(board)` = refresh both + evaluate, for tests and as the reference.
- `pub enum Network { Naive(NaiveNetwork), Psq(PsqNetwork) }` with `load(path)` dispatching on the
  feature-set tag, and `evaluate(board)`. Every current `Option<&NaiveNetwork>` (search, uci,
  arena: 8 files, 48 occurrences) becomes `Option<&Network>`, a mechanical change.
- `src/simd.rs`: a clipped-ReLU kernel beside `relu_avx2`, and a subtract kernel beside
  `add_assign_avx2`.
- `src/train/export.rs`: `psq_network_from(vs, ...)`, and `export_challenger` handles `psq`
  instead of skipping it. Tests mirroring the existing `naive` ones: engine agrees with `tch` on
  `AGREEMENT_POSITIONS`, through a file too, and the scrambled-transform check.

At the end of this step betty can be trained and played, but slowly: `Search::evaluate` still
refreshes from scratch at every node.

### 5. Incremental accumulator

- `src/nnue.rs`: `PsqNetwork::update(parent: &[f32; 2], parent_board, child_board) -> [f32; 2]`
  via the bitboard diff above, per perspective: subtract removed rows, add added rows, adjust
  castling and en passant rows. The accumulators are stored per colour (White, Black), not
  own/opponent, so a move never swaps them; `evaluate_accumulators` orders them by the side to
  move.
- Test: from several starting positions, play long random legal move sequences (including
  castling, en passant, promotions, captures of castling rooks, and null moves), updating
  incrementally at each ply, and compare with `refresh` of the same board. Float: within a tolerance
  (relative 1e-4 on each accumulator entry); also report the largest drift seen, which is the number
  to watch.

### 6. Search uses the accumulator

`src/search.rs`:

- `Search` gains `accumulators: Vec<[Vec<f32>; 2]>`, sized `MAX_PLY + 2`, allocated once per
  search (each Lazy SMP thread has its own `Search`), used only when the evaluator is `psq`.
- At the root, `accumulators[0]` = refresh.
- After each `make_move` (`alpha_beta_with_buffer`, `quiescence`): `accumulators[ply + 1]` =
  `update(accumulators[ply], board, scratch)`. After `make_null_move`: a copy (no piece moved; the
  en passant file may disappear, which the diff handles).
- `Search::evaluate(board)` becomes `evaluate(board, ply)` and reads `accumulators[ply]` for `psq`;
  `naive` and hand-crafted are unchanged.
- Eager update at every node first. Lazy update (only when a node is actually evaluated) is a later
  optimization, measured separately.
- Test: a fixed-depth search with the incremental path returns the same best move and score as
  one that refreshes at every node, on the bench positions (float: same move, scores within 1 cp).

### 7. Benchmark float

`benches/nnue_bench.rs` gains `psq` timings with random weights for (780 -> 256) x 2 -> 1:
refresh, one incremental update, and the head alone, plus `--depth N` searches against
hand-crafted and amanda. Betty's generation 1 then trains on `data/self/hand-crafted/`. Other
shapes (see "Decisions taken") are benchmarked with the same flags when their turn comes.

### 8. Integers

A separate change, its own sub-plan written here once step 7's numbers are in: int16 accumulator
and head as in "Decisions taken", weight clipping in the trainer, a quantized `.nnue` (new
feature-set tag or version, decided then), incremental test tightened to exact equality, and the
float `PsqNetwork` kept as the reference. Then benchmark again.

A progress log follows below as steps land.

## Other pending items from the same session

- remote-only.md: committed and pushed. Move it to `done/` once the GPU PC has run the new code.
- champion-in-its-generation.md: implemented, built, tested; the remote migrated by hand (amanda's
  N_1 now at `gen-0001/champion.*`, `CURRENT` = `gen-0001`, old `gen-0002/` deleted). Still to do:
  on the GPU PC and every other machine, **pull before running any `rotor-*` command**, so the
  local `gen-0002/initial.*` there migrates automatically to `gen-0001/champion.*`; check with
  `ls -R data/networks/amanda | grep -v checkpoints`. Then move the document to `done/`.
- 34 compiler warnings in `rotor-arena` (mostly unused code, some from the champion change): clean
  up opportunistically.

## Progress

- 2026-09-28: problem measured and analysed, proposal written. Nothing implemented.
- 2026-09-28: second measurement, on the training machine: N_1 self-play at **18 positions/s** with
  `--threads 12`. Faster than another machine's 7.2, still one to two orders of magnitude too slow.
- 2026-09-28: feature set agreed: `psq` (768 piece-square + castling + en passant, two
  perspectives). See "Decisions taken".
- 2026-09-28: implementation plan approved, including clipped ReLU for `psq` and the
  board-diff incremental update. Step 1 written (not yet built or run): `nnue.rs` accepts a head
  with no hidden layer (new round-trip test), `nnue_bench.rs` takes `--width` and `--head`.
- 2026-09-28: step 1 verified (16 `nnue` tests pass). Baselines, random weights, `naive`:
  amanda's shape 10.3 us per evaluation (467x hand-crafted, 22 ns); width 256 with no hidden layer
  0.6 us (31x), almost all of it the from-scratch gather. Step 2 written: `features::psq`.
- 2026-09-28: step 2 verified and committed. Step 3 written (not yet built): `FeatureSet::Psq`
  (discriminant 3), `ArchitectureKind::Psq`, `DenseBatchPsq`, `NetworkPsq`, `value_head::Activation`
  (ReLU for the existing kinds, clipped ReLU for psq) with empty heads allowed, `configs/betty.yaml`.
  `export_challenger` skips psq with a note until step 4.
- 2026-09-28: step 3 verified and committed (`configs/betty.yaml` missed by `commit -am`, added
  with step 4). Step 4 written (not yet built): `nnue::PsqNetwork` (`refresh`,
  `evaluate_accumulators`, `evaluate_raw`), `nnue::Network` enum loading either kind from the
  file's tag, shared `validate_weights`/`write_weights`, `FEATURE_SET_PSQ = 3`; search, UCI and
  arena hold `Network`; `simd::sub_assign_avx2` (unused until step 5) and `clipped_relu_avx2`;
  `export::psq_network_from` with agreement tests against tch; challenger and champion export for
  psq.
- 2026-09-28: step 4 verified and committed. Step 5 written (not yet built):
  `PsqNetwork::update` (board diff over the 12 bitboards, castling rights, en passant file),
  accumulators indexed by color (`refresh_by_color`, `evaluate_by_color`, `evaluate_by_color_cp`),
  and a random-games test against a from-scratch refresh at every ply (tolerance 1e-4, largest
  drift printed).
- 2026-09-28: step 5 verified and committed. Largest drift over 40 random games of up to 300
  plies (width 16): **1.79e-7**, about 500x under the tolerance. Step 6 written (not yet built):
  `Search::accumulators` (`MAX_PLY + 2` pairs, set up by `set_evaluator` at the root),
  `update_accumulators` after each legal move (main search and quiescence) and after the null move,
  `Search::evaluate(board, ply)` reading them for psq. Eager update of every legal move, pruned or
  not. Test: a depth-5 search gives the same score, best move and node count incrementally and
  from scratch.
- 2026-09-28: step 6 verified and committed. Step 7, `nnue_bench --psq`, on the training machine, random
  weights, (780 -> 256) x 2 -> 1, float: hand-crafted 20.7 ns; psq from scratch 760 ns;
  incremental update 173 ns; head 58 ns; **per search node 232 ns, 11x hand-crafted** (amanda:
  10.3 us, 467x). Depth-9 searches, per node: 1.2x to 1.8x the hand-crafted search (whole-search
  times are meaningless with random weights: saturated clipped ReLU prunes almost everything, 348
  nodes against 9265 from the start position). Projected self-play about 170-500 positions/s,
  against amanda's 7-18. The update, not the head, is now the larger cost: the target for step 8
  and for updating lazily. Bench now also prints time per node. Rerun: update 101 ns (noisy
  between runs), per node 160 ns (7.7x); depth-9 searches **1.34x the hand-crafted search per
  node** over the three positions (1.71 us against 2.29 us). The games projection is now based on
  the per-node ratio.
- 2026-09-28: betty gen 1, epoch 1 trained (`prepare` reused the hand-crafted dataset, 129
  segments). Best arm lr 1e-3, test loss **0.04338** at the last step (every arm still improving),
  against amanda gen 1's 0.04015 (levelled off). Low rates much worse than amanda's (1e-4: 0.070,
  1e-5: 0.094). Training runs at amanda's speed: the data pipeline, not the network, sets it.
- 2026-09-28: the challenger in `nnue_bench --depth 9`: per node **1.04x** the hand-crafted search
  (1.72 us against 1.79 us), but 2.1x the nodes to depth 9, so **2.2x the time** (amanda: 40-80x).
  Projected self-play about 140-270 positions/s against the hand-crafted 300-600. Evaluation is no
  longer the bottleneck; node count is. The bench's games projection assumes equal node counts,
  which a trained network does not have: to fix.
- 2026-09-28: first gate attempt at 16 threads killed and WSL crashed: the training machine's WSL has 7.6 GB,
  and 32 players x 64 MB of hash on top of 3.3 GB in use ran it out of memory. Arena and self-play
  hash cut to 16 MB per player (a depth-9 search needs about 1 MB). `elo_d` set to 10 in
  `configs/betty.yaml` by the developer.
- 2026-09-28: **betty gen 1 epoch 1 passes its gate** against the hand-crafted evaluation at
  Search(9): +132 =32 -26 over 190 games, 77.9%, roughly +220 Elo (about +/-50).
- 2026-09-28: betty promoted to N_1. **Self-play on the training machine, 20 threads: 350-360 positions/s**,
  against the hand-crafted evaluation's 650 on the same machine (1.8x, better than the bench's
  2.2x) and amanda's 18 at 12 threads (about 30 scaled to 20). A generation of 17,301,504
  positions takes about 13.5 h on the training machine alone. The problem this document was opened for is
  solved in float; integers (step 8) are no longer needed for speed and wait for a reason.
