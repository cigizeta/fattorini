# Tuned constants in the network's unit, the pawn scale only to start them

**Status:** design agreed 2026-11-20 (the developer); open decisions below; not implemented.

## Why

The search's constants are reasoned in PeSTO's classical pawns, where hand reasoning works ("razoring:
a pawn and a half a ply"); a network's unit is set by S, an expected score, and a material pawn
reads as about 30 there (the release's network, `fattorini pawn-scale`). Today the measured pawn
scale translates every constant at every load, tuned ones included, and three constants are never
tuned at all (`search.rs`):

- delta pruning's victim value, `piece_value(victim)` (`in_evaluator_pawns`);
- correction history's caps, one pawn on a correction, two on an update (`eval_margins.pawn`);
- the time management's stability window, `EVAL_WINDOW_CP` (Reckless's 12, hand-picked).

So SPSA tunes classical numbers that the search then rescales (a margin and the scale trade off
against each other), and three constants carry a PeSTO judgement through one average rate forever.

## The design (the developer, 2026-11-20)

- **The pawn scale starts the constants, never more.** A network with no tune starts from the
  classical defaults times its measured pawn scale: the reasoned values, translated. Nothing else
  uses the pawn scale.
- **SPSA owns the values, in the network's unit.** A tune's `search-parameters.txt` and
  `time-parameters.txt` hold the values the search uses as they are: no rescaling at load, no
  measurement for a tuned network.
- **The three untuned constants join SPSA,** started like the rest (classical value times the pawn
  scale): the victim values, the correction cap (the update cap stays twice it), the stability
  window. The score trend's unit follows the same rule.
- **Old tunes are not converted.** A tune file written before this change is classical: it is not
  read as a tune; its network plays the translated defaults, as untuned, until tuned again. Binaries
  already built (the release, the snapshots) keep their embedded tune and play as before; a new
  build of an old network plays untuned. The release's network needs a new SPSA run before its next
  snapshot or release.
- **PeSTO** is unchanged: its pawn scale is 100% by definition.

## Open decisions

1. **The victim values:** one tuned factor on fixed relative values (1, 3, 3, 5, 9), keeping SPSA's
   space small; or five tuned values.
2. **Telling an old tune file from a new one:** a version line in the new files, its absence
   meaning classical, so an old file is refused as a tune rather than misread.
3. **SPSA's bounds** for the new parameters: around the translated start, as wide as the existing
   margins' relative bounds.

## Not touched

S, training, the labels, the network files: the unit is the network's, as now.
