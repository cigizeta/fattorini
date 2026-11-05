# Timing the threats at evaluation

**Status:** design proposed 2026-10-29, to discuss. Answers TODO's "Make the threats incremental"
first step: whether the threats are a large enough share of an evaluation to be worth making
incremental.

## Today's path (`nnue.rs`, `QuantizedPsqNetwork::evaluate_by_color_cp`)

For a psq_threats network, at every evaluation, for each of the two perspectives:

1. **Allocate:** `Vec::with_capacity(32)` for the rows, a heap allocation per perspective per
   evaluation.
2. **Find:** `psq_threats::push_threat_rows`, every attack by the perspective's pieces on an
   opponent piece, as rows.
3. **Copy and add:** `add_rows_saturating_i16_avx2` writes the piece-square accumulator plus every
   threat row into a scratch copy, summed in `i32` and saturated, one pass over the lanes.

Then the head (`evaluate_lanes`), the same as a network without threats.

## As built (2026-10-29, the developer: "let us just measure, then we think")

No threshold decided in advance. `nnue_bench --threat-parts [dir]`, beside its other timings, not
an ignored test: the whole evaluation from built accumulators; the same head without threats (the
network's psq rows alone, quantized again); the allocation; the finding; the copy and add as what
is left; the rows per perspective; and evaluation's share of a depth-9 search, an upper bound
(nodes times the whole evaluation's cost over the search's time).

## Results (2026-10-29, `lc0-psq-threats-screlu-g1-r1`)

| per evaluation | 7 bench positions | 111,722 duel positions |
|---|---|---|
| whole | 147 ns | 264-277 ns |
| head without threats | 26 ns | 99-100 ns |
| allocation | 38 ns | 38-40 ns |
| finding | 29-31 ns | 60 ns |
| copy and add | 52-54 ns | 66-77 ns |
| threat rows a side, mean (largest) | 2.6 (8) | 1.7 (12) |
| evaluation's share of a depth-9 search, at most | 18% | 33-35% |

The duel positions' accumulators (about 114 MB) miss every cache, which a search's do not: their
head and copy overstate, the seven positions understate the variety; the truth between. The
allocation is constant and waste.

**Decided (the developer, 2026-10-29):** the allocation goes first (rows kept per thread in
`THREAT_SCRATCH`, the bench's allocation part removed with it), judged by `rotor bench`, same nodes,
more nodes per second. Incremental threats, or adding in place without the copy, decided later.

**The allocation removed (committed 2026-10-29):** `nnue_bench --search-only`, the 49 positions,
one thread, the same machine: depth 12, 4,225,698 nodes both sides, 2,515,046 nodes/s before and
2,812,220 after, **+11.8%**; depth 9, 783,938 nodes both, 2.43 M and 2.76 M (one run each). More
than the 4-5% the per-evaluation timing projected: a heap allocation likely costs more inside the
search, its heap busier, than alone in a loop.

## Decisions first proposed

1. **Four parts timed apart,** on the same positions: the allocation, the finding, the copy and add,
   and the head. Plus the evaluation whole, so the parts are checked against their sum, and the
   active threat rows per perspective counted (mean and largest), the size of the add.
2. **Positions from a real search:** the positions the bench's own search evaluates (`rotor bench`,
   depth 9), collected once, so the mix is what search sees, not a training set's.
3. **The network:** the current line's winner, psq_threats_screlu at width 256 (the 512 run's when it
   is the winner), from a file named on the command line.
4. **Where:** an ignored test in `nnue.rs`, run by hand with `--release`, its file named by an
   environment variable, as `lc0`'s by-hand checks are: no new command.
5. **The machine:** idle, plugged in, every side of a comparison on the same machine (CLAUDE.md).
6. **What decides, read off the shares:**
   - allocation a visible share: a buffer kept per thread, as the scratch is, a fix of minutes;
   - copy and add plus finding large (say a third of the evaluation or more): incremental threats
     designed, quantization bound first;
   - small: incremental threats dropped, the TODO entry closed with the numbers.

## Not here

X-rays and pins (the developer, 2026-10-29): not addressed today; a features experiment of its own
later, its cost timed then.
