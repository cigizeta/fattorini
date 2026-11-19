# The speed profile: where the network's cost over PeSTO's goes

**Status:** measured 2026-11-19 (TODO.md's "Speed", step 3), findings below; no code changed. Each
fix is designed on its own before it is written.

## How

samply 0.13 (ETW), 4,000 samples a second, on an idle machine, the bench built with line tables
(`CARGO_PROFILE_BENCH_DEBUG=line-tables-only cargo bench --bench nnue_bench --no-run`), then:

- `nnue_bench --network lc0-psq-threats-screlu-g1-a1 --search-only --depth 16`: 50,148 samples;
- `nnue_bench --pesto --search-only --depth 16`: about 64,000 samples.

Speeds without the profiler, the same machine and depth: **the network 2.29M nodes/s** (27.6M nodes
in 12.05 s), **PeSTO 4.43M** (67.1M in 15.13 s). Per node: 437 ns against 226 ns, so the network
costs 1.93 times PeSTO here (2.5 on the machine of 2026-11-13). Depth 12 was too short (1.5 s): each
bench position clears a fresh table, and the clears (`TranspositionTable::clear`, its relaxed
atomic stores) then filled 7% of the samples.

## The network's search, by part

Totals (the function and everything under it), as a share of the run and per node:

| Part | Share | ns per node |
|---|---|---|
| Evaluation, `evaluate_by_color_cp` | 38.7% | 169 |
| of which finding the threats, `push_threat_rows_from` | 16.5% | 72 |
| of which the rest: the accumulator copied, the threat rows added (`add_rows_saturating_i16_avx2`), the head | about 22% | 97 |
| Accumulator updates, `QuantizedPsqNetwork::update` | 11.8% | 52 |
| of which `ChangedRows::between`, the rows found | 6.2% | 27 |
| of which `update_i16_avx2`, the rows added and removed | 4.8% | 21 |
| Accumulator refreshes, `refresh_by_color` | 0% | -- (psq has no king buckets) |
| Move picking, `MovePicker::next` | 16.3% | 71 |
| of which `sort_descending` | 6.3% | 28 |
| of which `generate` | 3.1% | 14 |
| Transposition table, `probe` and `store` | 3.3% | 14 |
| The rest: the search, make and unmake, legality, quiescence | about 30% | 131 |

Evaluation and accumulator upkeep together: about 221 ns a node, the whole gap over PeSTO (211 ns)
and more, PeSTO's own evaluation being cheap.

In PeSTO's run, `sort_descending` is 6.5% and `bitboard::next` the top function at 9.4%.

## Findings

1. **Finding the threats is the network's largest single cost,** 72 ns a node, recomputed at every
   evaluation. threats-timing.md bounded it at 19% of a search; measured, 16.5%. Incremental threats,
   closed on 2026-11-13 for today's sparse threats, are worth reopening on this figure alone: the
   fused kernel (fused-threats.md) sped up the copy and add, never the finding.
2. **`ChangedRows::between` diffs the two boards, all twelve piece bitboards** (`nnue.rs`, `between`
   and `between_halfk`), 27 ns a node, more than the SIMD update it feeds (21 ns). The move already
   says which two to four rows changed: a `between` from the move is the cheapest fix found.
3. **Move ordering sorts the whole list,** `sort_descending` 6.3% with the network and 6.5% with
   PeSTO, twice the cost of generating the moves. A cutoff usually comes in the first move or two:
   picking the best each time, or sorting only above a threshold as Stockfish does for quiets, gains
   for every evaluator. A search change, so a bench for speed and a duel for strength.
4. **The transposition table is cheap** (3.3%), and refreshes cost nothing for psq; both stay as
   they are. A halfk network refreshes at every king move across buckets: to profile the same way
   with `lc0-halfk-threats-screlu-hidden-g1-a1`.

## Next

In order of gain over effort, each designed before it is written:

1. `between` from the move (finding 2): about 27 ns of 437, up to 6%.
2. Partial move sorting (finding 3): up to 6%, PeSTO included.
3. Incremental threats (finding 1): up to 16%, the largest and the hardest.
4. The halfk network's profile, for refreshes and its wider transform.
