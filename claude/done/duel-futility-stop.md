# The duel stops once a verdict is out of reach

**Status:** done 2026-10-08 as **deterministic curtailment** (`duel.rs`, `is_curtailed`), tests
passing (an even duel curtailed at 904 games, not 902; 600 seeded duels give the same verdicts with
and without it): drawn as soon as neither engine could pass its bound by 1000 even
winning every game left. `status` then says `curtailed at N games: neither can pass its bound by
1000`; the duel's drawn line no longer says "in 1000 games".

**The stochastic rule below was implemented first and dropped the same day (the developer:
"stop when it is mathematically proved that the bounds cannot be overcome").** Its seeded test,
400 duels of a true +40 Elo engine: 276 wins with it against 335 without, 17.6% of the wins lost,
since the projection took the observed mean as exact. Decisions 1 to 4 below are superseded;
5 stands (drawn, printed as curtailed).

## Why

The developer, 2026-10-08: a duel that can no longer reach either bound by `MAX_GAMES` plays on
to the cap for nothing. B4 (100 games) and B2 (450) were already stopped by hand as hopeless; the
scaled-margins duel ran its full 1000 at about 52.3%, never near 54.9%. This makes the judgement a
rule of `rotor-arena duel` (`src/arena/duel.rs`). Duels through cutechess-cli are untouched.

## The rule today

After every colour-reversed pair (`runner.rs` asks `outcome()` after each pair, never after a
single game), an engine whose score `S` is past `0.5 + C * 0.5 / sqrt(N)` wins (`C = 3.09`,
`Phi^-1(0.999)`); at `MAX_GAMES` (1000) with neither past it, the duel is drawn. About 5% false
winners overall between equal engines (duel.md, in `done/`).

## The addition

**Checked after every pair, as the bounds already are (decided 2026-10-08, the developer)**, after
the win checks and before the cap: if neither engine is likely to pass its bound by `MAX_GAMES`,
the duel ends **drawn** at once, as it would have at the cap.

Two strengths of "likely":

1. **Deterministic curtailment:** neither bound is reachable even if one engine wins every
   remaining game. It gives exactly the verdict running to the cap would. Checking `MAX_GAMES`
   alone suffices: with all wins the score rises as the threshold falls, so the cap is always the
   best chance. It fires late, though: near 50%, only after about 900 games.
2. **Stochastic curtailment (conditional power):** the probability of passing either bound by the
   cap, if the engines keep playing as observed so far, is below a stated `FUTILITY` (say 1%).
   Near 52% it fires after a few hundred games.

Either way, a curtailed duel can only turn a would-be win into a draw, never create a winner, so
the 5% false-winner rate is untouched. (2) costs a chosen, small chance of missing a real win.
(1) is the special case of (2) at `FUTILITY = 0`.

## The computation, for (2)

Per pair, as the checks are: the pairs so far give a mean and a variance of the pair score (0, 0.5,
..., 2), which also captures the correlation between a pair's two games (the same opening). With
`M` pairs left, the challenger's final points are approximately normal, mean `P + M * mu`, variance
`M * sigma^2`; the probability of ending past `threshold(MAX_GAMES)` is one tail, the champion's the
other. Only the final check is evaluated: intermediate checks add a little crossing probability,
which this ignores; the test below bounds what that costs.

## Decisions

1. **Stochastic, (2) (decided 2026-10-08, the developer).**
2. **`FUTILITY` = 1% (decided 2026-10-08, the developer):** the probability, both bounds
   together, of passing either by the cap.
3. **The observed mean (decided 2026-10-08, the developer):** the remaining pairs score as the
   pairs so far did, mean and variance alike. Its early noise is contained by (4).
4. **At least 200 games (100 pairs) before it may fire (decided 2026-10-08, the developer):** the
   observed edge's standard error is then about +/-25 Elo, small enough not to curtail a real +40
   engine on a bad start.
5. **`Outcome::Drawn`, printed as curtailed (decided 2026-10-08, the developer):** every consumer
   (the ladder, summaries, resume) treats it as today's draw, and the printed verdict says, e.g.,
   `drawn (curtailed at 412 games: 0.3% chance of a verdict by 1000)`, so that no one mistakes it
   for a 1000-game draw. `describe()` states the rule's new part.

All decided. Next: the implementation plan, below, once the developer approves this design.

## Tests

- Deterministic: a score that cannot reach the bound even with all wins is drawn at once; one that
  can, continues.
- Curtailment never produces a winner: the false-winner test (`two_equal_engines_rarely_produce_a_winner`)
  still passes unchanged.
- Missed wins: seeded duels of a true +40 Elo engine (past the about +34 the rule resolves) lose at
  most a stated share of their wins to curtailment, against the same duels without it.
- A resumed duel reaches the same curtailment from the same rows (`replay`).
