# psq_threats_screlu_kings, second design (kings2)

**Archived 2026-10-27, abandoned (the developer):** its generation 1 stopped unfinished. The psq
line ends at SCReLU: every hidden layer on psq inputs lost at equal depth, see
psq-threats-pairwise-hidden.md and hidden-beside-f32.md. Its code and config stay in the tree
(tag 17).

**The intuition (the developer, 2026-10-26):** SCReLU's network as it is, plus a hidden layer that
is king-sensitive.

**Status:** design agreed 2026-10-26; the architecture takes the dropped kings' name (decision 3).
Step 1 (engine) done 2026-10-26, with step 2's bench code (the bench built the old layout); rotor's
tests and clippy pass. Step 2 benched (below). Steps 3 (trainer), 4 (config) and 5 (docs)
done 2026-10-26, the workspace's tests and clippy passing. Next: step 6, a dev smithy smoke run,
then generation 1. The stage names stay `l1` (our side) and `l2` (their
side), the output `value_head.output`.

## Generation 1, first try (2026-10-26), and the detached head

**Rounds 1 to 8 (lambda 0.02, rate 3e-3), test loss against SCReLU's:** -3.7% at round 1, then
+0.6%, +1.0%, +1.7%, +2.5%, +1.9%, +1.8%, +2.7% at round 8; train loss 2.1% behind, so
underfitting. By piece count at round 8: -18% to -41% at 3 to 6 pieces, about level at 7 to 14,
+2% to +6% at 15 to 23, +6% to +17% from 24 up. Better than the old kings (+6.4% at round 4 against
+1.7%), but levelling off at about 2% to 2.7% behind instead of closing.

**Why it matters:** the network holds SCReLU (the skip is SCReLU's own output), so it could always
match SCReLU; trailing even on training loss points at the optimisation, not the capacity.
Hypothesis: the king layers' gradient reaches the shared transform and pulls its features away from
what the linear readout needs, most where the king matters least, the openings.

**The detached head (the developer, 2026-10-26):** the squares feeding the two sides' stages
detached in training, so the transform learns from the skip alone, exactly as SCReLU's, and the king
layers learn on top of features they cannot move. Training only: the engine, the file and the export
are unchanged. Generation 1 stopped at round 8 and its run deleted; the next run trains detached.
- **If it matches SCReLU from 15 pieces up and keeps the endgames' gain:** the interference was the
  cause; kings keeps the detach.
- **If it still trails:** the cause is elsewhere, to be understood before any further design.

**Detached, rounds 1 to 4 (lambda 0.02, rate 3e-3):** test loss against SCReLU's -3.6%, -1.0%,
-2.2%, -1.9% (0.07526 at round 4, phase 0.07579, the first try 0.07801); train loss 2.0% below.
By piece count at round 4: -10% to -33% at 4 to 6 pieces (3 pieces -1%), -2% to -3% at 7 and 8,
-0.1% to -2.5% at every count from 9 to 32: below SCReLU everywhere, where the first try was 3% to
12% above from 20 up. The interference hypothesis holds. Training also faster, about 159 s a round
against 184: no gradient from the 64-bucket products back into the squares.

**Detached, rounds 5 to 9:** -1.4%, -1.8%, -1.7%, -1.3%, -1.6% against SCReLU (0.07364 at round 9,
phase 0.07465), steady, no drift; train loss 1.8% below. By piece count at round 9: -14% to -19% at
3 to 5 pieces, -7% at 6, -0.1% to -1.6% from 7 to 30, -2.5% to -3.8% at 31 and 32: below everywhere,
the endgames' lead narrowing as SCReLU learns them (-22% to -33% at round 4), the rest stable.

## The bench (step 2, 2026-10-26, one machine, random networks, width 256, depth 9, two runs each)

| | head alone | per search node | parameters |
|---|---|---|---|
| psq_threats_screlu | 150.7, 154.1 ns | 186.4, 190.5 ns | 1,249,025 |
| psq_threats_screlu_kings | 334.0, 336.1 ns | 367.6, 372.0 ns (about 2.0x) | 1,775,425 |

The extra parameters are exactly the two sides' sets and their output weights (2 x 64 x 16 x 257
plus 64). The old kings' bench (2026-10-25, another day) had its head at 2.5x SCReLU's, 469 ns
against 186; this one's is 2.2x, so no dearer, a little cheaper. As the old kings', the bench's 7
positions cycle through distinct king sets, 8 KB each side, which a search, whose kings rarely
move, mostly does not pay: the search's own cost should sit below the 2.0x. The bench printed
`hidden [16, 16]` for both sides' widths; it now prints the one width. The fixed-depth searches
are not compared: their node counts differ, the networks being random. The colour-flipped twin test was dropped from step 1: a position and its twin
give the same buckets in the side to move's frame, so it cannot see a missing flip; a direct test
does instead (their side's sets all zero but the flipped one). The output's inputs, in order: our
values clipped, ours squared, theirs clipped, theirs squared, our squares, their squares. Follows psq-threats-screlu-kings.md,
whose generation 1 trailed SCReLU by 7% at round 5.

## Why

Kings at round 5 (psq-threats-screlu-kings.md): 7% behind SCReLU in test loss and nearly flat,
underfitting; far ahead at 3 to 6 pieces (-17% to -54%), behind from 8 up (+5% to +12%). The
hypothesis: everything passes through 16 clipped units, enough for endgames, where the king's square
decides much, too few for middlegames. Stockfish's head (read 2026-10-26, same doc) keeps a linear
path to the output beside its hidden layers, and both activations after each layer; its buckets are
the piece count and queens, never the king, which enters through its input alone.

## The idea (the developer, 2026-10-26)

Each perspective read by its own king, in parallel, instead of both perspectives read by our king,
then by theirs:

```
our squares   256 --(our king, 64 sets)-->   16 --+
                                                   +--> 1
their squares 256 --(their king, 64 sets)--> 16 --+
```

As halfk ties each side's features to that side's king, in the head instead of the input.

## Decisions (the developer, 2026-10-26)

1. **The skip:** SCReLU's own output, the 512 squares -> 1, added to the head's output. The network
   is SCReLU plus a correction by both kings: at worst SCReLU's middlegames, the endgames' gain kept.
2. **Both activations:** after each side's 16, the clipped ReLU and its square, concatenated, as
   Stockfish: 32 values a side, the output reading 64.
3. **The name: `psq_threats_screlu_kings`, replacing the dropped kings (the developer,
   2026-10-26):** its kind, `ArchitectureKind::PsqThreatsScreluKings` and config
   `configs/lc0-psq-threats-screlu-kings.yaml`; this doc keeps its filename, kings2, so bare-filename
   references stay unambiguous beside the archived psq-threats-screlu-kings.md.
4. **The old kings removed:** its engine path, bench flags, trainer head (`HeadShape::Kings`,
   `StackHead::kings`), export and config, replaced by this design under the same names.
5. **Tag 17 reused** for the new layout (the developer, 2026-10-26), not a new tag: so no old
   kings file may survive anywhere. The dropped run is deleted, `data/gen/lc0-psq-threats-screlu-kings/run-1`
   and its smithy state, by the developer, before the new kings forges.

## Design (agreed, 2026-10-26)

- **Widths: 16 a side (agreed, the developer, 2026-10-26).** With the skip, the hidden layer carries a correction, not the whole
  evaluation; 32 a side doubles the engine's and the trainer's cost before the idea is shown.
- **Each king in its own perspective's frame (agreed, the developer, 2026-10-26).** `stack_buckets` gives both kings from the side to
  move (`oriented_square(.., mover)` for both); their accumulator is in their frame, so their king's
  square is rank-flipped (`square ^ 56`) before it picks their set. No new batch column: the trainer
  flips column 1, the engine flips the same way.
- **Two sides, separate weights (agreed, the developer, 2026-10-26):** our side's stage and their side's, each a shared weight and bias
  plus a delta per king square, factorised as today's stages, deltas starting at 0. Not shared
  between sides: the side to move matters.
- **The output (agreed, the developer, 2026-10-26):** one plain layer over 64 + 512 inputs, the
  head's activations and the squares, one bias: the head's 64 -> 1 plus the skip's 512 -> 1.
- **Initialisation as every network's (the developer, 2026-10-26):** random, `nn::linear`'s default,
  for every layer, the head's output included; only the factorised deltas start at 0, as today's
  stages. No special start: every architecture starts from a random network.

## The numbers

| | kings | kings2 |
|---|---|---|
| first stage, an evaluation | 512 x 16 = 8192 multiply-adds | 2 x 256 x 16 = 8192 |
| second stage | 16 x 16 | none |
| output | 16 -> 1 | 64 -> 1, plus the skip 512 -> 1 |
| head's parameters | about 0.54M | about 0.53M |
| training's every-bucket product | 512 -> 1024 | 2 x (256 -> 1024), the same arithmetic |

So the engine about kings' cost a node (about 1.5x SCReLU's, kings' bench), the skip's 512 -> 1
SCReLU's own head; training about kings' 197 s a round.

## Implementation plan

Each step with its tests; the developer runs them and commits between steps. The old kings is
replaced in place, step by step, never left half: each step removes the old form where it builds
the new one. The config's `hidden_dim` becomes one width, `[16]`, the per-side width.

1. **Engine** (`crates/rotor/src/nnue.rs`, today's kings at 118, 132, 158, 637, 1722, 1790 to
   1799, its sample network at 2225):
   - Tag 17 keeps its name; its layout becomes: the transform as psq_threats_screlu's; per side, a
     king stage `[64, 16, 256]` weights and `[64, 16]` biases (the export's sums), ours then theirs;
     the output, 64 + 512 weights and one bias. Its shape stays `(psq_threats::ROWS, ..)` as the
     loader needs it. The old stack-based loading and evaluation for 17 removed.
   - Evaluation: the squares stored once (as today's kings), the skip's dot product over all 512
     (SCReLU's own), each side's 256 through its king's set (ours `buckets.0`, theirs
     `buckets.1 ^ 56`), clipped ReLU and its square, the 64 through the output; summed in f32 with
     the skip, as the taper blends, one rounding at the centipawn conversion. Scalar and AVX2, as
     SCReLU's.
   - The f32 reference (`evaluate_accumulators`): the same in f32.
   - `validate`: tag 17 only with screlu, threats, no MLP, exactly 16 a side.
   - Tests, as today's kings': write and load as 17; quantised within tolerance of f32; vectorised
     equal to scalar; incremental equal to from scratch along random games, with distinct sets per
     king square so a wrong bucket or a missing flip fails; a test that their king is read flipped
     (a position and its colour-flipped twin give the same evaluation).
2. **Bench** (`crates/rotor/benches/nnue_bench.rs`): `--stack --screlu --threats` builds the new
   kings at random; the module comment's reference to a `claude/` doc replaced by the reason itself
   (CLAUDE.md). Then the developer, on one idle machine: SCReLU against the new kings, back to back,
   three runs each. **If it costs much more than old kings' 1.5x a node, stop and discuss.**
3. **Trainer** (`smithy`):
   - `stack_head.rs`: `HeadShape::Kings` reads columns 0 and 1, theirs flipped `^ 56`; a new
     `KingsHead`: per side a factorised stage, `l1_ours`, `l1_theirs`, 256 -> 16 over 64 buckets
     (`stacked_stage_forward`, unchanged), clipped ReLU and its square concatenated, then one output
     over the 64 and the 512 squares; `StackHead::kings` and the stages `("l1", 0), ("l2", 1)` removed.
   - `nnue_network.rs` and `stacked_network.rs`: the head built for it; the stacked forward splits
     the squares into ours and theirs before the stages.
   - `net_config.rs`: `hidden_dim` must be one width for psq_threats_screlu_kings (its error message
     and test rewritten); `kings()` stays.
   - `export.rs`: each side's shared plus delta summed into its 64 sets, the output as is, written
     as 17; the agreement test (`export.rs`, 656) on the new layout.
   - Tests: one row by hand through both sides' summed weights, the flip and the skip; the stacked
     arm equal to its one network; the engine agreeing with tch's forward through a file.
4. **Config:** `configs/lc0-psq-threats-screlu-kings.yaml`, `hidden_dim: [16]`, its header comment
   describing the new head; the config tests.
5. **Docs:** architectures.md's kings entry rewritten; TODO.md.
6. Build, test, a dev smithy smoke run, then generation 1 on the training machine (the dropped
   run's data already deleted), judged as kings was meant to be: loss by piece count at about
   round 10 and 20 against SCReLU and phase, then the depth 9 duel against
   `lc0-psq-threats-screlu-g1-r1`.

## Open

- **Kings dropped at round 10 (2026-10-26):** 7.7% above SCReLU, the gap grown from round 5's
  7.2% (psq-threats-screlu-kings.md); kings2 is the line's king head.
- **The engine's form:** the skip and the head summed in f32, as the taper blends (one rounding, at
  the centipawn conversion); tag 17, reused; the head's squares stored once, as kings' design.
- **The bench first,** as for kings: random networks, kings2 against SCReLU and kings.
- **The judgement,** as kings': loss by piece count at about round 20 against SCReLU's and phase's,
  then generation 1's winner against `lc0-psq-threats-screlu-g1-r1`, depth 9, then 10+0.1.
