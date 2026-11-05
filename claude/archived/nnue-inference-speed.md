# NNUE inference speed: the f32 path first, then int8

**Archived 2026-11-04: phases A and B done; the one item left, betty's integer head, is moot since
betty was removed (old-networks-cleanup.md).**

**Status (2026-10-14):** open, kept: betty is still trained (generation 3, not stronger than amanda
so far), and a faster integer head is worth having (the developer). Earlier status: plan, drafted
2026-10-07. Phase A steps 1, 3 and 4 and Phase B done the same day,
int8 +32 Elo against f32 at 10+0.1. Phase A step 2 (the fused update) done too. Left: betty's integer
head, about twice what its arithmetic should take.

## Why

Measured 2026-10-07 on idle the Lichess machine, plugged in (TODO.md, "The speed gap, measured"):
`nnue_bench --search-only`, depth 9, Stockfish's 49 positions, one thread.

| | nodes/s | time per node | of which evaluation (bench) |
|---|---|---|---|
| hand-crafted | 2.99M | 0.33 us | 21 ns |
| caroline (psq, no hidden layer) | 1.43M | 0.70 us | update 102 ns + head 104 ns |
| betty (psq, hidden 16) | 1.08M | 0.93 us | update 98 ns + head 385 ns |

Caroline's node costs 0.37 us more than hand-crafted's, but her evaluation explains only about
0.21 us of it. Betty's extra 0.23 us over caroline matches her slower head (281 ns more).

What the code does today (`nnue.rs`, `simd.rs`, `search.rs`):

- **An update for moves that are then pruned.** The main search's move loop calls
  `update_accumulators` as soon as a move is found legal (`alpha_beta_with_buffer`, right after the
  `is_in_check` test), before futility and late-move pruning decide to skip it. A pruned move pays a
  full update (about 100 ns) and never reads it. Quiescence updates only after its SEE filter, so it
  is not affected.
- **The update rediscovers the move.** `PsqNetwork::update` walks all 12 piece bitboards for each
  perspective to find what changed, copies the parent's accumulator (`clone_from`), then adds and
  subtracts each changed row in its own pass over the 256 floats: up to five passes per perspective
  where one fused pass would do.
- **The head allocates.** `evaluate_accumulators` builds a fresh `Vec` of 512 floats, clamps it in
  a separate pass, and each `Layer::forward` allocates its output. Caroline's head is one 512-wide
  dot product, about 30 ns of arithmetic, and takes 104 ns.
- **Betty's hidden layer is 16 separate dot products**, each ending in a horizontal sum, over
  512 inputs: 8,192 multiply-adds in 385 ns.

## Profile (2026-10-07, samply on the Lichess machine, caroline, `--search-only --depth 11`)

Self time, inverted call tree, 3.4 s: `RtlAllocateHeap` 13%, of which 12% is the `Vec::with_capacity`
in `evaluate_accumulators`; `RtlFreeHeap` 2.1%; memory copies about 9% (`copy_nonoverlapping` 4.6%,
`vcruntime140` 4.5%, callers not yet expanded). No NNUE kernel (`add_assign_avx2`,
`dot_product_avx2`) in the top rows. So the head's allocation alone costs about 14% of the search,
the Windows heap being slow at it: step 3 comes next, ahead of step 2.

## Phase A: the f32 path, no change to the format or the trainer

Each step is measured alone: `nnue_bench` (evaluation timings) and `nnue_bench --search-only` for
caroline, betty and `--hand-crafted`, on idle the Lichess machine, plugged in.

1. **Update only the moves the search enters.** Move the main loop's `update_accumulators` call
   after the pruning decisions, just before the recursion, checking that nothing in between reads
   `accumulators[ply + 1]`. The tree is unchanged, so the node counts must be identical to the
   last digit, the test that this step is pure speed.
   **Measured 2026-10-07: no gain.** Node counts identical (caroline 1,711,110, betty 1,579,278),
   times 1.20 to 1.19 s and 1.47 to 1.46 s, within noise. The wasted updates were not where
   caroline's unexplained 0.16 us per node goes; a profile, not a guess, should find it before
   step 2.
2. **A fused update, redesigned 2026-10-07 for the `i16` accumulators** (the original text below
   was written for f32 and is kept for its history). Today's `QuantizedPsqNetwork::update`, per
   perspective: copies the parent's 512 bytes into the child, then one full pass over the 256
   lanes per changed row (read, add or subtract, write), and finds the changed rows by scanning
   all 12 piece bitboards, once per perspective, so twice per update. About 65 ns per node.

   - **The changes found once, not per perspective.** One pass over the 12 bitboards, castling
     rights and en passant file collects what changed as (piece, square, added) entries plus the
     castling and en passant flags, into fixed-size arrays on the stack (no allocation). Then each
     perspective only maps those few entries to its rows (`piece_row_for`, orientation). The
     largest change: castling with both own rights lost (king and rook, 2 rights, an en passant
     file cleared) or a capturing promotion that takes a rook's castling right; 8 removed and 8
     added rows bound every case with room.
   - **One pass per perspective.** `child = parent - removed rows + added rows`, 16 lanes at a
     time: load the parent's lanes into a register, add and subtract each changed row's lanes,
     store once. The parent is read once, each row once, the child written once: no copy, no
     intermediate stores. A new kernel `update_i16_avx2(child, parent, added, removed)`, the rows
     given as slices.
   - **No specialisation at first.** Stockfish writes separate code for one added and one removed
     row (a quiet move) and for one added and two removed (a capture). Start with the general loop
     over the few rows; specialise only if the profile then shows the loop's overhead.

   Integer arithmetic, so exact: the existing random-game test, incremental against a fresh
   refresh with no tolerance, keeps guarding it; a kernel test checks `update_i16_avx2` against
   scalar sums. The tree is unchanged, so node counts must match to the last digit. Measured by
   `nnue_bench`'s update timing and `--search-only` on both networks. Expected: the update from
   about 65 ns to 30-40 ns, a few percent of a search.

   As built (2026-10-07, while betty's duel against hand-crafted ran on the Lichess machine, both sides
   affected alike): `ChangedRows::between` scans the boards once and maps each change straight to
   both perspectives' rows, with no intermediate (piece, square) entries; 16 rows per side and
   perspective. `update_i16_avx2(child, parent, rows, added, removed)` takes the row indices and
   the whole `i16` transform. `sub_assign_i16_avx2` went, unused. Node counts unchanged. Timed
   only together with three search changes of the same day (TODO.md): caroline +13% nodes/s,
   betty +10%, against hand-crafted's +10% from the search changes alone.

   Original f32 text: **A fused update from the move's changed rows.** Find the removed and added rows once (at most
   two removed and two added for an ordinary move; castling, en passant and promotion are still
   covered by the bitboard comparison, now done once, not per perspective), then for each
   perspective write `child = parent - removed + added` in a single pass over the 256 lanes. The
   existing random-game test (`incremental_updates_track_a_from_scratch_refresh_along_random_games`)
   keeps guarding it.
3. **A head without allocation.** The clipped ReLU fused into the first layer's input read, and
   per-search scratch buffers instead of `Vec`s per evaluation. For caroline the head becomes one
   pass: clamp and multiply-add together.
   **Done 2026-10-07:** per-thread scratch buffers (`HEAD_SCRATCH`), same arithmetic in the same
   order. Node counts identical; caroline 1.19 to 1.10 s (1.56M nodes/s), betty 1.46 to 1.36 s
   (1.16M nodes/s), about 8% each. A second profile then showed `memcpy` at 17%, 15% of it the
   copy of both accumulators into the scratch buffer: now `clipped_dot_product_pair_avx2` reads
   them in place for a head with no hidden layer (bit for bit the old result), and
   `clipped_copy_avx2` clamps while copying otherwise. Node counts identical; caroline 1.05 s
   (1.62M nodes/s, as fast to depth 9 as hand-crafted's 1.06 s), betty unchanged at 1.36 s.
4. **Betty's hidden layer eight outputs at a time.** Done 2026-10-07, without reordering the
   weights (a derived field would have broken every `PsqNetwork` constructor and the round-trip
   tests): `clipped_dot_products_8_avx2` keeps one FMA chain per output over the row-major rows,
   clamps each input block once as it reads the accumulators in place, and reduces the eight
   registers together. Betty's head 385 to 153 ns; search 1.36 to 1.19 s (1.33M nodes/s), node
   count still identical.

Steps 2 to 4 change the summation order, so an evaluation can differ in its last bits and, rarely,
by a centipawn after rounding: node counts may move slightly. Correctness is held by the existing
tolerance tests, not by identical node counts.

## Phase A in games (2026-10-07)

Betty-0001-0.95 against hand-crafted at 10+0.1, after steps 1, 3 and 4, on the Lichess machine (8 pairs):
drawn at 1000 games, betty 49.2% (+455 -439 =106 for hand-crafted), about -6 Elo. The day before,
on the old code and on the training machine (10 pairs), she lost at 44.1%, about -41 Elo. So roughly +35 Elo
from speed, rough since the machines differ: speed was most of her gap under the clock, and at
fixed depth she stays about +100 Elo above hand-crafted.

## Phase B: int8, quantized at load

Stockfish's scheme, in Rotor's shapes:

- **Accumulator in `i16`.** The transform's weights and bias are scaled by 127 (the clipped ReLU's
  one becomes 127) and rounded. 256 lanes of `i16` are 512 bytes, half today's per perspective, and
  an AVX2 register holds 16 lanes instead of 8.
- **Clipped ReLU to `u8`**, 0 to 127.
- **Hidden and output weights in `i8`** (betty) or `i16` (caroline's single output layer, where
  `_mm256_madd_epi16` costs nothing in range), with a power-of-two scale per layer.

**Quantized when the `.nnue` loads, not when it is exported.** The file stays f32 and its format
unchanged, every existing network keeps loading, and the trainer is untouched. The scales come from
each layer's largest weight. Stockfish instead clips weights during training so that a fixed scale
fits; Rotor's trainer clips nothing today. Whether load-time scales lose too much is measured, not
assumed: the quantized evaluation against the f32 one over the selection positions
(`nnue_bench --eval-scale`'s positions), as the largest and the typical difference in centipawns.
Only if that error is too large does weight clipping in the trainer come in, as a separate plan.

Then the duel that judges it: the int8 engine against the f32 one, at 10+0.1, since quantization
changes the evaluation and so the tree.

## Phase B, measured and refined (2026-10-07)

`QuantizedPsqNetwork` in `nnue.rs`, a scalar reference, and `nnue_bench --quantization` (97,081
positions of betty's 10+0.1 duel games), absolute difference from f32 in centipawns:

| | mean | p99 | max |
|---|---|---|---|
| caroline, activations truncated | 2.09 | 14.4 | 26.6 |
| caroline, activations rounded | 1.16 | 7.8 | 16.1 |
| betty, rounded, `i8` hidden weights | 3.79 | 34.1 | 83.6 |
| betty, rounded, `i16` hidden weights | 2.42 | 23.1 | 61.9 |

So: activations round to nearest (truncating biased every lane the same way), and the hidden
layer's weights are `i16`, not `i8`. What remains is mostly the accumulator's scale: the
worst-case overflow bound gives caroline 1.0 = 508 and betty 254, while the largest lane seen uses
37% and 28% of `i16`. Kept as is (the developer): the duel judges whether 1 to 2 cp cost anything.

## Phase B, the steps

1. **The quantized engine path.** `QuantizedPsqNetwork` gets the incremental path `PsqNetwork`
   has: accumulators `[Vec<i16>; 2]` per ply, `update` with the same row differences on `i16` lanes
   (`_mm256_add_epi16`/`_mm256_sub_epi16`), and a head that clamps, rounds and shifts each lane as
   it loads it and multiplies with `_mm256_madd_epi16` (caroline's output in one pass; betty's
   hidden layer eight rows at a time, then her f32 output layer). `Network::load` quantizes, and the
   search, UCI and the arena evaluate only through it.
2. **The f32 incremental path goes**: `PsqNetwork::update`, `evaluate_by_color` and the f32 kernels
   only it uses (`dead_code` is denied). `PsqNetwork` stays as the loaded weights, the from-scratch
   reference (`evaluate_pawns`) and what `quantize` reads. The scalar quantized evaluation stays as
   the reference the kernels are tested against.
3. **Tests**: incremental `i16` accumulators equal a from-scratch quantized refresh exactly (integer
   arithmetic, no tolerance) along random games; the vectorized head equals the scalar quantized
   one exactly; the existing search tests on the new path.
4. **Measured**: `nnue_bench` timings and `--search-only` for both networks.
5. **The duel**: the f32 engine is built once from the commit before step 1 and copied out of
   `target/` (`data/engines/rotor-f32.exe`), the int8 build likewise (`rotor-int8.exe`), and the
   two play at 10+0.1 on idle the Lichess machine, each with `EvalFile` set to betty's champion. Not through
   `arena duel`, whose players are always the current build (`resolve_player`; the registry
   `data/engines.yaml` is `rotor-evolve`'s only): through cutechess-cli, with the arena's book,
   1 thread, 16 MB hash, 8 games at a time, 1000 games in colour-reversed pairs.

**Steps 1 to 4 done 2026-10-07.** As planned, plus: only the first hidden layer is quantized,
later ones stay f32 (two-layer networks still load); psq_threats is quantized too, its bound
adding `MAX_THREAT_ROWS` (16 pieces times 8 targets). All tests pass. Measured on the Lichess machine:

| | f32 (end of Phase A) | int8 |
|---|---|---|
| caroline update / head | 102 / about 30 ns | 64 / 19 ns |
| caroline, depth 9 | 1.05 s, 1,711,110 nodes | 0.98 s, 1,697,834 nodes |
| betty update / head | 101 / 153 ns | 66 / 141 ns |
| betty, depth 9 | 1.19 s, 1,579,278 nodes | 1.12 s, 1,643,715 nodes |

Betty's integer head is about twice what its arithmetic should take: to profile.

**The duel (2026-10-07, the Lichess machine, cutechess-cli 1.5.1):** betty int8 against betty f32 at 10+0.1,
1000 games: +362 -270 =368, 54.6%, **+32.1 +/- 17.1 Elo**, LOS 100%. Quantizing at load stays: its
1 to 2 cp of error cost nothing that shows against the speed. cutechess-cli is installed per user
by winget (`CuteChess.CuteChess`) in `%LOCALAPPDATA%\Programs\Cute Chess`, on `PATH` through
`~/.bashrc`.

## Weight clipping in the trainer: not now (discussed 2026-10-07)

Superseded the same day: generation 2 trains with weight clipping, for an `i8` first hidden layer
(betty-head-speed.md). The reasoning below still holds for today's `i16` weights.

Stockfish's nnue-pytorch clamps its hidden layers' weights after every step so that they fit a
fixed `int8` scale (about +/-2, stored times 64); its feature transform, `int16`, is not clipped.
Rotor needs none of that as built: every scale is computed at load from the weights themselves,
so nothing overflows, and the precision cost is measured (1.2 and 2.4 cp mean) and already paid
for (+32 Elo). Clipping would pay in two places, each a trainer change with its own plan and a
retraining:

1. Betty's hidden layer in `i8` (`_mm256_maddubs_epi16`, twice the products per instruction):
   with per-row scales it cost 3.8 cp mean and 34 cp p99, which clipping during training is what
   makes acceptable in Stockfish. The bigger lever if her head stays slow after profiling.
2. A finer accumulator: the overflow bound sums each lane's 37 largest row weights, so a few large
   transform weights coarsen the whole accumulator (betty's 1.0 = 254, 28% of `i16` used).
   Bounding the transform's extreme weights would raise the scale.

## Open questions

1. Phase A's step 1 is exact and the cheapest. Does it go in on its own first, measured and
   committed before the rest?
2. Phase B: load-time quantization (this plan) or a quantized file format written by
   `smithy export`? **Decided 2026-10-07: at load** (the developer). The trainer, the exporter
   and the format stay as they are; the trainer changes only if the measured error asks for
   weight clipping, as a separate plan.
3. Does Phase A need a duel? Steps 2 to 4 change evaluations by rounding only, so the proposal is
   no: the benches and the tolerance tests. Phase B does need one.
