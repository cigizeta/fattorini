# halfk_threats_pairwise_hidden: design

Status: designed 2026-11-02, decisions by the developer; implemented the same day, ahead of step 2's
duel (the developer, "we can implement"; halfk-threats-pairwise-hidden-implementation.md), trained
only if step 2 (halfk-threats-screlu-hidden.md) wins or draws its duel against step 1's winner. Step 3
of the halfk line: step 2's network with SCReLU replaced by pairwise products, nothing else changed.

## The network

```
(26,624 -> 256) x 2, each lane clipped; the first 128 lanes times the last 128: 256 values
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
- **Duels:** against step 2's winner, depth 9 then 10+0.1, both `@default`.
