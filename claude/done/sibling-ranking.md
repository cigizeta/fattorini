# Sibling ranking against Stockfish

**Status:** implemented and run 2026-10-28. **Hypothesis 6 holds:** concordance at quiescence ranks
the five networks in their duels' order (Results).

## Results (2026-10-28)

9,298 roots of the phase against count duel's games, Stockfish at 1,000,000 nodes, MultiPV over
every move (about 16 to 28 minutes on 15 processes; a first pass, keyed by MultiPV line, lost moves
an iteration reordered and was redone keyed by move). Pairs 30 cp apart or more: 3,443,208.

| network | quiescence | static | top-1 (quiescence) |
|---|---|---|---|
| SCReLU | 80.95% | 68.39% | 27.08% |
| threats | 80.44% | 67.89% | 26.83% |
| plain pairwise | 79.61% | 67.01% | 26.93% |
| pairwise_hidden | 79.40% | 68.93% | 26.47% |
| psq | 77.05% | 62.24% | 25.48% |

Each +/- about 0.2% to 0.25% (two s.e. over roots, per network; a paired gap's error is smaller).

- **Calibration:** threats 3.4 points above psq, its +51 Elo: about 15 Elo a point.
- **Every duel's sign, roughly its size:** pairwise -1.34 against SCReLU (about -20; its duels
  about -36), pairwise_hidden -1.55 (about -23; -24 to -55), threats -0.51 (about -8; SCReLU's
  duels about +12), pairwise -0.83 against threats (its duels -24 and -8).
- **pairwise_hidden orders siblings best statically** (68.93%) and worse than SCReLU once quiescence
  has played the captures out: its evaluation is good on quiet positions, weaker on the material
  imbalances quiescence lands on. Why it fit Lc0's positions better and lost.
- **Top-1** follows, within noise. Concordance at quiescence is the measure.
- **A screening measure:** about 18 s a network against the cached pass, before any duel; width
  512's first. Hypothesis 6
of loss-and-strength.md, on a machine other than the training machine (CPU only).
- **Amended 2026-11-04, after quiet-filter.md: concordance at quiescence alone is no screen.**
  `-quiet` ordered siblings better at quiescence than g1-r1 (83.81% against 80.90%) yet lost its
  duels by about -160 and -108 Elo: it ordered them worse statically (62.75% against 68.31%) and
  misjudged positions with a capture pending, which the search's static uses (futility, razoring,
  null move, stand-pat) read on non-quiet nodes and quiescence never corrects. A network passes the
  screen only if its static ordering does not fall behind either.

## Why

On positions Rotor's games reach, scored against the games' results on games neither played, both
pairwise networks fit as SCReLU does, and both lose to it: pairwise_hidden by -24 to -55 Elo, plain
pairwise by about -36 (rotor-positions-loss.md, calibrated: the measure sees threats' +51 Elo against
psq clearly). Fit does not explain the duels. Alpha-beta compares siblings: what decides is whether
the evaluation orders a position's children as they truly stand, and a lower mean error can come with
no better ordering. This measures the ordering directly.

## Decisions

1. **The positions: the clean test's neutral games,** `lc0-psq-threats-screlu-phase-g1-r1` against
   `lc0-psq-threats-screlu-count-g1-r1` (`data/matches/.../20261028-0716/games.pgn`), replayed by
   `duel_positions`: out of check, 7 pieces up (what the duels can see), one in ten, about 10,000;
   none of the networks measured played them.
2. **The reference: Stockfish at full strength, MultiPV over every legal move, fixed nodes (the
   developer, 2026-10-28).** One search a position scores every move together, consistently; about
   1,000,000 nodes, `Threads` 1 and several processes side by side, every logical core but one, the
   machine otherwise idle. Its executable from the registry's `stockfish` entry, `data/engines.yaml`
   (gitignored, read as the arena reads it). Scores from the mover's side, mates mapped beyond any
   centipawn score.
3. **The reference cached** to `data/sibling-ranking/<games' folder>/stockfish-<nodes>.tsv` (local,
   gitignored): one line a position, its FEN and every move with its score. The Stockfish pass runs
   once; every network reads the file.
4. **What a network ranks by: its quiescence and its static evaluation, both reported (the
   developer, 2026-10-28).** Each child scored from the parent's mover's side: `-quiescence(child)`
   as `move_agreement` does (the search's shallowest view), and `-evaluate(child)` (the evaluation
   alone). Quantized, as the engine plays.
5. **The measures, by band (7-16, 17-32) and in all:**
   - **Pairwise concordance:** over pairs of siblings Stockfish separates by at least 30 cp, the
     share the network orders the same way; ties in the network count half. Closer pairs are left
     out: Stockfish's own noise at fixed nodes.
   - **Top-1 agreement:** the network's best move Stockfish's best.
   An error for each over positions (a position's pairs are correlated): two standard errors of
   the per-position share.
6. **The networks:** SCReLU (`lc0-psq-threats-screlu-g1-r1`), pairwise_hidden and plain pairwise,
   then threats and psq as the calibration (+51 Elo apart).
7. **Code:** two `#[ignore]` tests in `engine_loss.rs`, beside the others: `sibling_reference`
   (the Stockfish pass, its cache; `ROTOR_DUEL_PGN` names the games) and `sibling_ranking`
   (`ROTOR_DUEL_PGN` and `ROTOR_DUEL_LOSS_NETWORKS`, any number of networks, comma-separated). The
   UCI talk is a few lines of `std::process::Command`, no new dependency. A unit test parses a
   MultiPV `info` line, mates included.

## Reading it

- **Both pairwise networks order siblings worse than SCReLU,** at quiescence or statically, while
  their fit is level: hypothesis 6 holds, and concordance becomes the screening measure, width 512's
  included.
- **Level too:** the cause lies deeper in the search (its ordering heuristics, the margins'
  interplay beyond the first ply), and only games can judge.
- **Calibration first in reading:** threats must order better than psq, or the measure is blind at
  this size.

## Steps

1. Code (decision 7), the unit test passing.
2. `sibling_reference` on the neutral games, the machine idle: about 10 to 30 minutes.
3. `sibling_ranking` for the five networks.
4. Results here, in loss-and-strength.md and TODO.md.

## Open

- The paired error of a gap between two networks (same roots), sharper than each one's own.

- Whether 1,000,000 nodes is enough: if a second pass at 4,000,000 on a few hundred positions
  reorders many pairs beyond 30 cp, raise it.
