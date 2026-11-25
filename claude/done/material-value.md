# Why the networks value material so little

**Status:** agreed 2026-11-20 and written (`crates/smithy/src/material.rs`); to test, then run on
the release's network.

## Why

The release's network reads a material pawn as 30 centipawns (`fattorini pawn-scale`), and against
io-bot (Lichess, 2026-11-20) it judged an extra exchange for a pawn at +0.10, then repeated into a
draw. The pawn scale only measures this; the cause is in what the network learned. Three candidates,
each with its own cure:

1. **The teacher says it.** Lc0's Q itself gives material little weight: strong self-play converts
   a pawn up less often, and, the main suspect, **selection bias**: in strong self-play a material
   imbalance mostly comes from a deliberate, sound sacrifice, so "down material" is learned together
   with "has compensation". Cure: in the data.
2. **The blend dilutes it.** A label mixes Q with the game's result (the polyline's lambda); many
   games one side was winning still end drawn. Cure: in the blend.
3. **The network does not follow its labels** in the imbalanced positions: too few of them,
   capacity, regularisation or quantization. Cure: in training or the architecture.

## The measurement

One table separates them: positions grouped by material balance, and in each group the mean of what
the teacher said, what the network was told, and what it learned.

- **Positions:** Lc0's frozen test set (`data/lc0/frozen` on the remote), every position: test
  games, never trained on.
- **Settled positions only:** PeSTO's quiescence equal to its static evaluation (the rule
  `measure_label_scale` already counts by), so no capture sequence is pending and the material on the
  board is the material of the position. The positions left out are counted.
- **Material balance:** from the side to move, pawn 1, knight and bishop 3, rook 5, queen 9. Groups
  by balance, -9 or less to +9 or more, and, apart, the imbalances that are not a count of pawns
  alone: the exchange (a rook against a minor piece), a minor piece against pawns, a queen against
  pieces.
- **Columns**, each as an expected score in percent from the side to move:
  - **positions** in the group, and their share;
  - **Q**: the label at lambda 0, Lc0's Q alone (the tables' result where they answer);
  - **result**: the label at lambda 1, the game's result alone;
  - **label**: the polyline's, what the release's network trained on;
  - **network**: the network's own expected score, `curve.score(y)`, the quantized network the
    engine plays.
- **Reading it:** Q low in the imbalanced groups: cause 1; Q fine, label low: cause 2; label fine,
  network low: cause 3. Small imbalanced groups, or a result far above Q, point to selection bias.

## The tool

`smithy material --network <player>`, run by hand, read-only, on the CPU, in no usage line, as
`smithy king-buckets` (`census.rs`) is. It reads the frozen set with three label sets, lambda 0,
lambda 1 and the polyline; one missing is added by the freezer exactly as training would add it
(`freeze::ensure`, which needs the tables), never redrawing the positions. It prints the table.

## Check

`cargo test -p smithy material`, with a unit test of the grouping (the balance, the exchange, the
settled rule) on hand-made positions; then the developer runs it on the release's network.
