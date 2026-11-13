# Threat rows fused into the screlu head

**Abandoned 2026-11-13, see fused-threats-implementation.md:** A built, exact, no gain, reverted;
B and C dropped while the threats stay this sparse.

Status: **design, 2026-11-13**, agreed with the developer in outline ("A then C"); C revised below
after reading the code. No implementation plan yet.

## Why

Measured 2026-11-13 (threats-timing.md, TODO.md "Speed"): the shipped network's evaluation is 137
ns, of which finding the threat rows 45 ns and the copy and add 57 ns. Threats are at most 19% of a
depth-13 search, so making them free is worth at most about 23%. The engine already benches
faster than Stockfish (2.46M against 1.76M nodes/s); this is a gain taken before the two SPSAs, so
both tune the faster search (the developer, 2026-11-13).

## Today's path

`QuantizedPsqNetwork::evaluate_by_color_cp` (nnue.rs), for a threats network, per perspective:
`push_threat_rows` into a kept `Vec`, then `add_rows_saturating_i16_avx2` (simd.rs): each 16-lane
chunk widened to `i32`, the threat rows' chunks added, packed back to `i16` with saturation
(`packs`, then `permute4x64` 0xD8), **stored to a scratch copy**. The head
(`evaluate_screlu_lanes`, then `screlu_output`, then `screlu_dot_i16_avx2`) **loads the scratch
again**, activates (`activations_16`), squares (`(a * a + 256) >> 9`) and multiplies by the
output's weights with `madd`.

## A. The fused kernel (decided)

One new kernel in simd.rs, `screlu_dot_threats_i16_avx2(own, opponent, own_rows, opponent_rows,
transform, weights, activation) -> i32`: for each perspective's chunk, the same widen, add, packs
and permute in registers, then the packed chunk fed straight into the same activation, square and
`madd`. No scratch, no store, no reload.

- **Exact by construction:** every operation is today's, in the same order, on the same integers,
  only the store and the load between them gone. The activation step takes a register rather than
  a pointer: `activations_16`'s body split so both entries share it.
- **Where it applies:** a psq_threats network with a screlu head that is its plain output
  (`QuantizedHead::Output`), the shipped `lc0-psq-threats-screlu`. Every other head and kind keeps
  today's scratch path; nothing else changes.
- **The rows found as now,** into the two kept `Vec`s, before the kernel runs.
- **Tests:** the fused value equal to the scratch path's on many positions (the bench's, and
  random legal positions with many threats, including saturated lanes); a test with rows chosen to
  saturate, so the `packs` path is pinned.
- **Measured:** `--threat-parts` (the copy and add part should mostly vanish) and the snapshot's
  `bench`, same nodes, more nodes per second, on one idle machine, before and after.

## C. Finding, revised

The first idea, both perspectives' attacks found in one pass, gains nothing: a perspective counts
its own pieces attacking the opponent's, so the two perspectives' attackers are disjoint and no
attack set is computed twice. Finding costs about 1.4 ns a piece. What might still pay: **pawns
by bitboard shifts**, a side's pawn attacks at once, the attacking pawns looked up only where a
target is hit, since most pawns attack nothing. Decided only after A's measurement, if finding is
then the larger share.

## B. Incremental threats

Not now: a move changes threats far from its squares, so every make and unmake pays a
dirty-threats update, also at nodes never evaluated, against 1.6 rows a perspective on average;
and an incremental `i16` accumulator saturates differently from today's `i32` sum. Judged again
only if A and C leave threats a large share.
