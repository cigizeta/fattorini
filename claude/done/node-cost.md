# Why a Rotor node costs more than its network explains

**Status (2026-10-12): answered, done. The "slow" node was the bench, not the engine.**
`--search-only` allocated a fresh 64 MB TT per position, and Windows hands fresh memory over one
zeroed page per first touch: amanda-0002's small trees (about 19k nodes a position) paid a page
fault on nearly every probe, about 14% of her profile in `ntoskrnl` and more in time. Fixed in the
bench (one table per thread, cleared before each position, only search time counted); node counts
identical. Measured again, the Lichess machine plugged in, faster of two:

| | nodes | before | after | ns/node after |
|---|---|---|---|---|
| hand-crafted | 3,140,380 | 3.09M nodes/s | 4.46M nodes/s | 224 |
| random network, amanda's shape | 22,089,723 | 3.31M | 3.71M | 270 |
| amanda-0002 | 950,209 | 1.58M | **3.32M** | 301 |

Against Stockfish 19's 2.03M (`bench 16 1 13`, same machine and conditions), **Rotor with
amanda-0002 runs at about 1.6x Stockfish's nodes/s**, not 0.76x. The engine never paid this cost
(one table per game), so play was never affected; every `--search-only` figure over the 49
positions before 2026-10-12 carries it, the small trees most (`--games` already kept one table per
game and is unaffected). The network costs about 77 ns a node over hand-crafted. The sections below
are the investigation as it went; its first fit (800 ns an interior node) was this artifact.

## The question

On one thread, the Lichess machine plugged in and idle (2026-10-12), Rotor with amanda-0002 does 1.55M nodes/s
(`nnue_bench --search-only`, depth 9, 950,209 nodes in 0.61 s), against Stockfish 19's 2.03M
(`bench 16 1 13`) with a network hundreds of times larger: about 0.76x. Why is Rotor slower per node
with a tiny network?

## What the numbers already say

Per node, from the measurements on record (the Lichess machine unless noted):

| | ns per node | source |
|---|---|---|
| hand-crafted, whole node | about 305 | 3.28M nodes/s, 2026-10-07 |
| hand-crafted evaluation | about 21 | generation-2.md |
| amanda's evaluation work per node | about 70 | psq-node-cost.md (the training machine): update 60.6 ns x 0.88 updates/node + head 20.9 ns x 0.75 evaluations/node, before the fused update |
| caroline-0001, whole node | about 510 | 1.97M nodes/s, 2026-10-07 |
| amanda-0002, whole node | about 650 | 1.55M nodes/s, 2026-10-12 |

If the search machinery cost the same under a network as under hand-crafted, an amanda node would
cost about 305 - 21 + 70 = **about 355 ns, about 2.8M nodes/s**, faster than Stockfish. It costs
650. **About 300 ns a node are unexplained**, more than the whole evaluation. caroline-0001 already
had part of it (nnue-inference-speed.md: "caroline's unexplained 0.16 us per node"), and the head's
heap allocation found then is gone since (`evaluate_accumulators`).

So the question is narrower than "Rotor against Stockfish": **why does the same search cost about
twice as much per node under a network as under hand-crafted, beyond the evaluation itself?**

## Measured (2026-10-12, the Lichess machine plugged in, replay stopped, today's build)

Depth 9, the 49 positions, one thread, faster of two runs; node mix from a `node-stats` run.

| | nodes | time | ns/node | quiescence share | interior nodes |
|---|---|---|---|---|---|
| hand-crafted (`--hand-crafted`) | 3,140,380 | 1.02 s | 325 | 77.3% | 712,646 |
| random network, amanda's shape (no `.nnue`) | 22,089,723 | 6.67 s | 302 | 90.9% | 2,018,024 |
| caroline-0001 | 1,265,318 | 0.78 s | 617 | | |
| amanda-0002 | 950,209 | 0.60 s | 635 | 31.2% | 653,737 |

Without `--hand-crafted`, `nnue_bench` uses a random network of amanda's shape, not hand-crafted:
the 2026-10-07 "hand-crafted" figures were `--hand-crafted` (3,140,380 nodes, unchanged since).

**Fitted per kind of node** (random network and amanda-0002 share the network and its code, so
solving time = interior x c_i + quiescence x c_q over the two): **an interior node about 800 ns, a
quiescence node about 250 ns.** Hand-crafted fits the same picture (its interior nodes about 750 ns
if its quiescence nodes cost about 200).

What this settles:

- **Not the network.** The random network runs the same code over the same 0.8 MB at 300 ns a node
  (hypotheses 2 and 3 out as main causes); `node-stats` also bounds the waste again: 9.0% of
  amanda's updates unread, 11.3% of her evaluations of a position the TT held.
- **Not a code slowdown** (4 out): caroline-0001's tree changed since 2026-10-07 (1,697,834 nodes
  then, before her margins scaled to her pawn), so its 510 ns then is not comparable; today the two
  trained networks cost the same per node.
- **The node mix (1) explains the gap between evaluations**: a trained network's tree is 69%
  interior nodes, hand-crafted's 23%, the random network's 9%. amanda's 650 ns is mostly 0.69 x
  800 ns.
- **Nodes/s across differently shaped trees misleads.** amanda reaches depth 9 in 0.60 s against
  hand-crafted's 1.02 s, with 3.3x fewer nodes: faster to depth, slower per node. The same caution
  applies to Rotor against Stockfish.

**The real target is the interior node, about 800 ns.** Why a trained network's tree holds so few
quiescence nodes is a separate question about the tree (pruning and margins), not about speed.

## Hypotheses, to be told apart by measurement

1. **A different node mix.** The two evaluations grow different trees (amanda's margins are scaled
   to her pawn of 55%; her tree here is 950k nodes against hand-crafted's 3.18M). If amanda's tree
   has fewer cheap nodes (quiescence leaves cut at stand-pat) and more expensive ones (interior
   nodes with move generation, ordering, history updates), the cost per node rises with no code
   being slower.
2. **Cache pressure.** The network's transform (0.8 MB in i16 for psq) and the accumulator stack
   compete with the TT, the history tables and the move lists for L2 (about 1 MB per core on a
   current CPU, L3 shared). Misses would show as time spread over the search's own functions, not
   in the NNUE kernels: exactly a flat profile.
3. **Work done for the network at nodes that never use it.** Updates for positions never
   evaluated (8.9% on 2026-10-07), evaluations of positions the TT already held (10.7%):
   measured small then, a few ns; to recount, not to assume.
4. **A slowdown in the code since 2026-10-07**, for the amanda-0002 against caroline-0001 gap
   (650 against 510 ns, same network shape).

## Steps (the Lichess machine idle, plugged in, replay stopped)

Each run twice, the faster kept, as on 2026-10-07.

1. **Baselines on today's build**, same command, same 49 positions, depth 9:
   - hand-crafted: `cargo bench --bench nnue_bench -- --search-only` (3,183,476 nodes expected; a
     different count means the search changed since);
   - caroline-0001 (1,697,834 nodes expected, 1.97M nodes/s then);
   - amanda-0002 (950,209 nodes, 1.55M nodes/s today).

   caroline-0001 at its old speed puts the 650-against-510 gap on amanda-0002's tree (1); slower,
   on the code (4).
2. **Node mix**, for hand-crafted and amanda-0002, under `node-stats`: the share of quiescence nodes
   (counted there since 2026-10-12, replacing `search.rs`'s leftover temporary counter), and the
   updates and evaluations per node. Then the cost split as (interior nodes x their cost) + (quiescence nodes x theirs),
   if the counters allow; if they do not, the profile's time in `alpha_beta_with_buffer` against
   `quiescence_from`.
3. **Profiles with samply**, hand-crafted and amanda-0002, `--search-only --depth 11` (longer, for
   enough samples), inverted call tree, self time. The table to fill, ns per node for each:
   evaluation (update, head), move generation, make/unmake, legality, TT probe and store, move
   picker, SEE, history updates, the search's own body, allocation, the rest. The two profiles side
   by side locate the 300 ns: a function that costs more per node under amanda than under
   hand-crafted, at the same work, is a cache effect (2); a function called more often per node is
   the node mix (1).
4. **Only then a fix**, chosen from the table. A tree-identical change is judged by node counts
   unchanged and time; a change to the tree needs a duel (the duel rule).

## Commands

```bash
cargo bench --bench nnue_bench -- --search-only
```

```bash
cargo bench --bench nnue_bench -- data/networks/caroline/gen-0001/champion.nnue --search-only
```

```bash
cargo bench --bench nnue_bench -- data/networks/amanda/gen-0002/champion.nnue --search-only
```

```bash
cargo bench --bench nnue_bench --features node-stats -- data/networks/amanda/gen-0002/champion.nnue --search-only
```

The profile (samply opens the result in the browser):

```bash
samply record target/release/deps/nnue_bench-<hash>.exe data/networks/amanda/gen-0002/champion.nnue --search-only --depth 11
```

The `<hash>` is the one `cargo bench` prints on its `Running` line.
