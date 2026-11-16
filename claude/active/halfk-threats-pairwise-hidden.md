# halfk_threats_pairwise_hidden: design

Status: designed 2026-11-02, decisions by the developer; implemented the same day, ahead of step 2's
duel (the developer, "we can implement"; halfk-threats-pairwise-hidden-implementation.md), trained
unless step 2 (halfk-threats-screlu-hidden.md) loses its depth-9 duel against step 1's winner (the
developer, 2026-11-16, replacing "only if step 2 wins or draws": pairwise halves the first hidden
layer's cost, so a step 2 lost at 10+0.1 alone, to its speed, is the case it is for). Duelled
against step 2's winner if step 2 won or drew at 10+0.1, otherwise against step 1's. Step 3
of the halfk line: step 2's network with SCReLU replaced by pairwise products, and castling and en
passant rows added (decision 7).

## The network

```
(26,636 -> 256) x 2, each lane clipped; the first 128 lanes times the last 128: 256 values
256 -> 16, clipped to [0, 1]
 16 -> 32, clipped to [0, 1]
 32 -> 1
```

## Decisions (the developer, 2026-11-02)

1. **Replace SCReLU with pairwise**, everything else step 2's: halfk's features and threats, the
   32 buckets factorised in training, Reckless's head, no skip, end to end, from scratch.
2. **Width 256 kept.** The head reads 256 values instead of 512, halving the first hidden layer's
   cost, the gain pairwise is tried for (TODO.md). Doubling to 512 would double the transform's
   cost and its updates, and give that gain back: two changes, not one.
3. **The products as psq's pairwise has them:** each lane clipped to [0, 1], then multiplied; the
   engine's products in `0..=127`, the squares' range, so the head reads them as it reads the
   squares.
4. **The arithmetic inherited:** whichever of int16 and int8 step 2 keeps (the other's code is
   removed before step 2's duels); not measured again.
5. **No weight bound**, as step 2: the engine quantizes the first hidden layer one scale per row.
6. **Starting weights as step 2's:** the transform factorised as halfk's start, the head drawn
   fresh.
7. **Castling and en passant rows added** (the developer, 2026-11-16): psq's 12, 4 castling rights
   relative to the perspective and 8 en passant files, outside the king buckets as the threat rows
   are (26,624 + 12 rows in the engine). Halfk had dropped them, as Stockfish's; no duel could
   measure them under alpha-beta, but this network is the one planned for MCTS (mcts-nnue.md),
   where the value head scores a leaf with no search beneath it, the policy ranks castling and en
   passant moves, and Lc0's labels were searched knowing both. So step 3 differs from step 2 in
   two ways, pairwise and these rows; its duel measures them together. Their code is new work for
   the implementation plan: the rows in `halfk`, the engine's incremental updates on castling, king
   and rook moves and en passant squares, the trainer's batches; tag 22's shape changes, no
   network having been trained under it.

## For the implementation plan

- **Tag 22, `halfk_threats_pairwise_hidden`**, `ArchitectureKind::HalfkThreatsPairwiseHidden`,
  `configs/lc0-halfk-threats-pairwise-hidden.yaml` (step 2's with its kind); a pesto twin only if
  wanted.
- **Engine:** halfk's accumulator and refresh cache unchanged; the products from psq's pairwise
  code over halfk's lanes into step 2's head.
- **Trainer:** `with_options` takes halfk pairwise with two hidden layers; export as step 2's with
  the pairwise flag.
- **The bench before training**, as step 2's: speed at depth 9 against step 2's winner, and the
  share of zero products and of all-zero blocks of four (a product is zero when either lane is, so
  more zeros than the squares; whether skipping them pays in int8).
- **Duels:** against step 2's winner, or step 1's if step 2 lost at 10+0.1 (Status), depth 9 then
  10+0.1, both `@default`. **Decided 2026-11-16:** step 2 drew both (tests 13 and 14), so against
  `lc0-halfk-threats-screlu-hidden-g1-r1`, both untuned. The castling and en passant rows:
  halfk-castling-en-passant-implementation.md.
