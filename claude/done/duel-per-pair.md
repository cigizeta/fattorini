# The duel per pair, with a measured deviation and a lower cap

**Status:** built and in use 2026-10-31, from `corr`'s `-quiet` clocked duel on (arena's
`duel.rs`, the smithy's copy in `matches.rs`, arena's usage and the page's line). Measured in play:
`sigma_hat` about 0.22 at 10+0.1 (draws about 40 to 47%), 0.25 to 0.28 at depth 9; drawn duels
curtailed at 370 to 392 pairs (36 to 39 minutes at 10+0.1, against about 46 before); clear gaps
decided in 12 to 136 pairs. A debug build's unsigned overflow in `other_side` fixed the same day (a
release build wrapped to the right value, so no verdict was touched); `sigma_hat` and the pair
count dropped from the printed line (open point 4).

**Why:** a drawn duel runs to about 900 games, about 45 minutes at 10+0.1, and every duel of
2026-10-31's search work was drawn (fifteen of them, all within +/-1.6 standard errors). Winners are
already found early; drawn duels are what costs (the developer: "what is annoying is having to wait
900-1000 games for a duel that is drawn"). TODO's "drawn duels end too late" holds the discussion
that led here.

## Today's rule (`crates/arena/src/duel.rs`)

After every pair, with N games and S the challenger's share: a side past `0.5 + C * 0.5 / sqrt(N)`
wins (`C = 3.09`, `Phi^-1(0.999)`); at `MAX_GAMES = 1000` the duel is drawn; before that, curtailed
as drawn once neither side could pass the cap's bound winning every game left. The 0.5 is a game's
largest possible standard deviation (no draws); the games are taken as independent.

## The rule proposed

Changes decided together (the developer, 2026-10-31; a lower C was first among them, then dropped
as below):

1. **The pair is the unit.** A pair's two games share a UHO opening and are not independent: the
   side with the opening's edge tends to win it, so pairs split 1-1 more often than two
   independent games would. A pair's score `y` is the challenger's points in it, halved: 0, 0.25,
   0.5, 0.75 or 1. With P pairs, S is the mean of the `y`s.
2. **The deviation measured:** `sigma_hat`, the standard deviation of `y` over the pairs played
   (divided by `P - 1`). It catches both the draws and the pairs' cancelling, which the fixed 0.5
   ignores (fishtest's pentanomial model does the same). The threshold becomes
   `0.5 + C * sigma_hat / sqrt(P)`.
3. **A lower cap,** `MAX_PAIRS`, so a drawn duel ends sooner: a drawn duel ends near the cap
   whatever the deviation, so the cap alone sets its length. `sigma_hat` gives back the resolution
   a lower cap would lose.
4. **C general, as today:** `Phi^-1(0.999) = 3.09`, from the confidence of one check, not fitted.
   Weighed and set aside the same day: C chosen so the bounds at the cap are 45% to 55% (the
   developer). With `sigma_hat` that is `C = 1 / sigma_hat`, a confidence varying by duel; fixed
   at the worst case `sigma_hat = 0.5` it is `C = 2.0`, about 97.7% per check and far more than 5%
   false winners. 3.09 kept (the developer: option 1): the bounds at the cap follow from each duel,
   about 46.6% to 53.4% at `sigma_hat` 0.22, 42.3% to 57.7% at the worst case.

**Nothing in the rule is fitted to Rotor's networks (the developer, 2026-10-31: "C cannot depend on
our networks; it must be general and work for every network").** C comes from a confidence;
`sigma_hat` is measured within each duel from its own pairs, so the threshold adapts to any two
engines (many draws or cancelling pairs: a finer one; no draws: near today's); the cap is a cost,
the most pairs a duel may take. The resolution at the cap, `C * sigma_hat / sqrt(MAX_PAIRS)`,
follows from each duel. **Whether the false winners stay at about 5% must be verified, not
assumed:** fewer checks lower them, but today's 5% was measured with the worst-case 0.5, which at
50% draws stands near 4.4 real standard errors, while 3.09 times `sigma_hat` stands at 3.09. The
existing simulation test, played on pairs of several splits, says which; if above 5%, the
developer decides between a higher confidence per check and accepting it.

Curtailment stays, and stays exact: it never changes a verdict (below).

## False winners, measured (2026-10-31)

`false_winners_per_pair` (`duel.rs`, ignored): equal engines, 4,000 seeded duels per split of the
five pair results, curtailment left out (it changes no verdict). Proposed: pairs, `sigma_hat` with
the two pseudo-pairs, C 3.09, cap 400 pairs. Today: games, 0.5, C 3.09, cap 1,000 games.

| Split | Proposed | Today |
|---|---|---|
| independent games, no draws | 3.60% | 2.75% |
| independent games, 45% draws | 2.85% | 0.15% |
| independent games, 70% draws | 1.73% | 0.00% |
| pairs cancelling (UHO-like: 5/20/50/20/5%) | 1.80% | 0.00% |
| pairs cancelling, many draws (1/12/74/12/1%) | 0.93% | 0.00% |
| pairs reinforcing (40/0/20/0/40%, worst) | 6.92% | 18.27% |

Under 5% wherever pairs are not reinforcing; today's rule is far stricter there, which is its
resolution lost. Where pairs reinforce, both exceed 5%, today's much more: today's rule assumes
independent games, which reinforcing pairs break, while the pairs' measured deviation follows them.
UHO's openings are meant to make pairs split, not reinforce.

## Open points

1. **The cap: 400 pairs, 800 games (the developer, 2026-10-31),** not the 250 proposed: "we
   cannot really afford false winners", and C stays at 99.9% (99.5%, C 2.58, would have shortened
   duels about 30% for some 15% to 25% false winners). At 400 pairs the resolution would be about
   24 Elo for duels like 2026-10-31's if their `sigma_hat` is about 0.22 (estimated, to be
   measured), about 38 for engines that never draw (`sigma_hat` about 0.35), 55 at the worst case
   0.5; a drawn duel of `sigma_hat` 0.22 curtailed near 373 pairs, about 746 games, about 36
   minutes at 10+0.1 (today about 903 games, 45 minutes, at 34 Elo). Measuring 2026-10-31's
   fifteen duels' `sigma_hat` from their `results.db` (a pair's games are consecutive rows) tells
   what to expect; it sets nothing.
2. **`sigma_hat` while it is noisy.** After a few pairs it can be far too small (four 1-1 pairs give
   0), and a lucky streak would pass a tiny threshold. Proposed: compute it with two pseudo-pairs
   added, one 2-0 and one 0-2, to the deviation only (not to S): no parameter to pick, its weight
   fading as pairs accumulate (at 100 pairs, about 2%). Alternative: no win check before a minimum
   number of pairs, which would be a second parameter.
3. **Curtailment, exact.** The cap's bound now depends on the final `sigma_hat`, so on the results
   still to come. For each total the leader's remaining pairs could score (every value from 0 to
   all won), its best chance is the remaining scores closest to equal (the lowest final deviation
   for that total: the remaining `y`s on the two grid values around their mean), so checking those
   totals, at most `4 * pairs left + 1` cases, is exact. Curtailed only when none passes, so the
   verdict is the full duel's, as today; the test `curtailment_gives_the_verdict_the_cap_would` is
   kept, played on pairs.
4. **What arena prints.** S and the bounds as today, from the champion's side; result.txt the
   same. `sigma_hat` and the pair count were printed at first, then dropped (the developer,
   2026-10-31): the pairs have their own column, and the bounds carry `sigma_hat`,
   `(bound - 0.5) * sqrt(pairs) / 3.09`. The ETA's projections (win, curtailment, cap) redone
   with `sigma_hat` and pairs.
5. **The old duels' records.** result.txt keeps its text; a duel resumed from an older run's
   `results.db` is judged by the new rule, its pairs being whole by construction.

## Plan, once the open points are settled

1. The fifteen duels' `sigma_hat` per pair: an ignored arena test reading their databases,
   information only.
2. The rule in `duel.rs`, its tests rewritten on pairs (symmetry, no early win, the cap,
   curtailment exact, false winners at most 5% with pairs of several splits, the ETA).
3. Then the next duel runs on it: `blend`'s, after `corr`'s two clocked duels on today's rule.
