# The score shown to a GUI: Lc0's centipawns

**Status:** 2026-11-20, decided and written; to build, test, and judge on Lichess.

## Why

CCRL's live games showed Fattorini's evaluations as conservative. Measured with `fattorini
pawn-scale`: the release's network (`lc0-psq-threats-screlu-g1-a1@tuned`) reads a material pawn as
30 centipawns. Two causes:

- A network's output `y` (pawns) is trained so that `q = tanh(y / (2S))` is its expected score,
  `S = LC0_A_PAWNS * LC0_B / 2` (`labels.rs`), which matches Lc0's display curve only near 0: Lc0
  shows `90 * tan(1.5637 * q)`, and for large `q` that is far larger (`q = 0.9`: 550 against 207).
- Lc0's Q for a material pawn is modest in itself (strong self-play, drawish), hence the 30%.

The engine printed `100 * y`.

## The change

What a GUI sees is Lc0's centipawns for the network's own expected score:

```
q  = curve.score(y)          the network's curve, as trained (logistic for version 2, atan for 1)
cp = 100 * LC0.pawns(q)      = 90 * tan(1.5637 * q), Lc0's display formula
```

- **Display only.** The search keeps `100 * y`; margins, the pawn scale, the time management
  (`previous_score` is the search's own score) and the TT are untouched. The map is monotone, so
  it changes no move.
- **Every `info ... score cp`** line: the per-depth one, MultiPV's ranks, and the repeated one
  before `bestmove`. Mate and tablebase scores (`labels::is_decisive`) pass through unchanged.
- **PeSTO** (no network) prints as before: it has no expected score.
- **The network keeps its curve** (`QuantizedPsqNetwork::curve`), set at load where its pawn is
  measured; `Network::shown_centipawns` converts.
- **Bound:** `q` saturates at 1, where `LC0.pawns` is about 127.8 pawns (12,780 cp), below the
  tablebase band.

Not affected: the arena runs Fattorini in-process and reads the search's raw scores, so its
adjudication and PGN annotations keep `100 * y`. lichess-bot, GUIs and CCRL read the new numbers.

## Check

`cargo test --release -p fattorini uci` and `nnue`, then games on Lichess: a clear advantage should
now read as other engines show it, near equality unchanged.
