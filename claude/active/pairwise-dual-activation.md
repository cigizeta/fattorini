# Pairwise with squares in the head: design

Status: proposed 2026-11-25 (the developer asked for an architecture combining pairwise with
screlu, "trying to get the best of both"). Not decided, not planned. Waits behind the inference
work and the 512 duels (TODO.md, 2026-11-25), and possibly behind the piece-count buckets (the
developer, 2026-11-25).

## The idea in plain terms

The two kinds differ in one place, the transform's output:

- **screlu** squares each lane: 1024 values reach the head at width 512. Rich, but the head's
  first layer reads twice as many values, and fewer of them are zero.
- **pairwise** multiplies the lanes in pairs: 512 values reach the head. Half the first layer's
  work, more zeros to skip, but less for the head to work with (the loss gap of 2026-11-25).

Squaring is a way to give a layer more nonlinearity than clipping alone. It costs per value
squared, so it is expensive on the transform (hundreds of lanes) and almost free after the head's
first layer (16 values). Stockfish puts it there: pairwise on the transform, and each of the
first hidden layer's outputs passed on both clipped and squared. So the proposal keeps pairwise
where it saves the most, and adds screlu's squaring where it costs the least.

## The network

Today's `halfk_threats_pairwise_hidden` at width 512 (`-512-8-budget`):

```
(9,740 -> 512) x 2, each lane clipped; first 256 lanes times last 256: 512 products
512 -> 16, clipped
 16 -> 32, clipped
 32 -> 1
```

Proposed (the developer, 2026-11-25: "15 * 2 -> 32, with a plain shortcut, constant 1"):

```
(9,740 -> 512) x 2, each lane clipped; first 256 lanes times last 256: 512 products
512 -> 16
  outputs 1 to 15: each v clipped to [0, 1], then both v and v * v passed on: 30 values
     30 -> 32, clipped
     32 -> 1
  output 16, the shortcut: not clipped, added to that 1 as it is
score = (32 -> 1) + output 16
```

Two changes from today's head, in one attempt (the developer's choice): the 15 values passed on
clipped and squared, and the 16th added straight to the score.

- **The squares** (SCReLU beside CReLU) give the head screlu's nonlinearity where it is cheap: 15
  values squared, not 512 lanes.
- **The shortcut** is the first layer's own linear estimate of the score, unclipped, so large
  scores need not be built from values capped at 1; the deeper layers learn corrections to it, and
  the first layer gets a direct gradient from the score (a residual connection, as transformers
  and Lc0's networks have).
- **Its factor is 1, not a constant.** The 16th output's weights are learned, so any factor would
  only rescale them. In the engine it is dequantized by its row's own scale, computed at export as
  every row's, so no number is chosen by hand. If its loss curve shows the shortcut learning too
  slowly at the budget's rates, a larger factor is the remedy, decided then.

The second hidden layer reads 30 values instead of 16: about 450 more multiplications in f32 per
evaluation, against the 8,192 of the first layer. The transform, the products and the first layer
are unchanged, so both inference optimizations (sparse and 8-bit first layer) apply as they are.

## Name and unit (the developer, 2026-11-25)

The kind `halfk_threats_pairwise_dual` ("dual": CReLU and SCReLU over the same 15 values; the
shortcut comes with the head), its config `lc0-halfk-threats-pairwise-dual-512-8-budget`, with
`unit: elo` (elo-scale.md), as every new config. A screlu twin, if choice 2 makes one, would be
`halfk_threats_screlu_dual`.

The unit is a third difference from `-pairwise-hidden-512-8-budget` (`unit: lc0`). The loss
compares expected scores only, so test losses stay comparable round by round, but the network's
output in pawns spreads about 2.5 times wider under Elo's S (173.7 centipawns against 70.37), so
the shortcut carries larger values: its learning speed (the factor of 1) to watch in the loss.

## Choices to make (the developer)

1. **The shortcut too: decided (the developer, 2026-11-25),** with the squares, in one attempt, so
   the duels measure the two together.
2. **Which kinds get it.** The squares in the head are independent of the transform's kind, so
   screlu could have them too. Proposed: a head option for both kinds, trained first on pairwise
   (the question is whether pairwise can catch up); screlu with it only if pairwise does, so that
   a screlu win is not just the head.
3. **Order against the piece-count buckets.** Both change the head. Proposed: whichever runs first,
   the other is applied on top of its winner, never both in one attempt.

## How it is judged

- **Loss first, at no extra cost:** the attempt runs the same corpus and schedule as
  `-pairwise-hidden-512-8-budget`, so its test loss compares round by round, as on 2026-11-25. A
  gap to screlu closed by half or more is the sign it works.
- **Then duels at equal time** on the optimized engine: against `-pairwise-hidden-512-8-budget`
  (the squares' own effect), then against `-screlu-hidden-512-8-budget` (the question itself).
- **What it cannot show:** if it loses to screlu, it says that this head was not enough, not that
  pairwise is worse in general.

## For the implementation plan

- A new architecture kind or a head flag (to choose in the plan), its tag, its config; a
  `-512-8-budget` config differing from pairwise's by the head alone.
- Trainer: the first layer's outputs split 15 and 1; the 15 clipped and squared, concatenated,
  into a second layer of 30 inputs; the 16th added to the output, factor 1.
- Engine: the second layer is already f32 (`second_layer_and_output`), so the squares and the
  shortcut are added there; the first layer untouched, its 16th row dequantized by its own scale.
- Export and load: the second layer's shape changes; a quantized and an f32 test as every kind's.
