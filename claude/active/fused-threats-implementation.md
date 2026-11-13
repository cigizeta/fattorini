# Fused threats: implementation plan

Status: **steps 1 to 5 implemented 2026-11-13**, waiting for `cargo test`, clippy and the
measurements. As built: `evaluate_by_color_cp` calls `evaluate_by_color_cp_fused(.., true)`, the
`false` side kept for the test that compares the two; `threat_chunk` has no test of its own, the
comparison along random games with saturated lanes covering it, as the existing
`screlu_incremental_evaluation_matches_a_from_scratch_one_with_saturated_lanes` now does for the
fused path.

## Steps

1. **simd.rs, the activation from a register.** Split `activations_16` in two: a new
   `unsafe fn activations_of(loaded: __m256i, activation: Activation) -> __m256i` holding today's
   clamp, round and shift, and `activations_16` as the load followed by it. Every caller unchanged.

2. **simd.rs, the chunk with its threats in a register.** Split `add_rows_saturating_i16_avx2`'s
   per-chunk body into `unsafe fn threat_chunk(source: *const i16, rows: &[i16], length: usize,
   offset: usize, added: &[u16]) -> __m256i`: today's widen, `add_epi32` of each row, `packs`,
   `permute4x64` 0xD8, returned instead of stored. `add_rows_saturating_i16_avx2` stores what it
   returns; its scalar tail unchanged.

3. **simd.rs, the fused kernel.** `pub(crate) fn screlu_dot_threats_i16_avx2(own: &[i16],
   opponent: &[i16], own_rows: &[u16], opponent_rows: &[u16], transform: &[i16], weights: &[i16],
   activation: Activation) -> i32`: for each perspective (own first, at weight offset 0, then the
   opponent at `own.len()`), each 16-lane chunk is `threat_chunk`, then `activations_of`, then
   `screlu_dot_i16_avx2`'s square (`(a * a + 256) >> 9`) and `madd` with the weights, summed;
   `horizontal_sum_epi32` at the end. Each perspective a multiple of 16 lanes (`debug_assert`),
   as `screlu_dot_i16_avx2`.

4. **nnue.rs, the fused path.** In `evaluate_by_color_cp`, inside `THREAT_SCRATCH`, before the
   copies: when the network is screlu, not pairwise, its head `QuantizedHead::Output`, and a
   perspective a multiple of 16 lanes, fill the two row `Vec`s (the scratch's third, plus a fourth
   for the opponent's rows, so both live at once), call `screlu_dot_threats_i16_avx2`, and return
   `centipawns(output_value(bias, sum, scale, PRODUCT_ONE))`, as `screlu_output` does. Every other
   network keeps the copy path. psq and halfk alike: the rows come from `push_threat_rows(self.halfk,
   ...)` either way.

5. **Tests (nnue.rs):**
   - The fused value equal to the copy path's, on every position of the bench's 49 and on a few
     hundred random legal positions from random play, for a sample screlu threats network with an
     output head (`sample_network`'s threats screlu shape), psq and halfk.
   - A position and weights chosen so a lane saturates (rows large enough to pass `i16::MAX`),
     the fused value still equal: the `packs` path pinned.
   - `threat_chunk` against the scalar tail's formula on random rows.

6. **`cargo test --workspace --release` and `cargo clippy --workspace --all-targets`,** both clean.

7. **Measured, one idle machine, before and after:**
   - `cargo bench --bench nnue_bench -- --network lc0-psq-threats-screlu-g1-r1 --threat-parts
     <the smoke test's games.pgn> --depth 13`: the whole evaluation, which should fall by most of
     the 57 ns copy and add. The bench's parts read the copy path, so "copy and add" as a part is
     reported as before; the whole is what moves.
   - A snapshot of each build, `bench`: the same nodes (exactness), more nodes per second.

8. **Then decided (fused-threats.md, C):** pawns by bitboard shifts, only if finding is then the
   larger share.

## Not changed

The scratch path for every other head and kind, the training side (`features.rs`), the network
file format, the search.
