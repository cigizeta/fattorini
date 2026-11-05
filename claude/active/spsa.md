# spsa: tuning several search parameters per network, by coordinate gauntlets

**Status:** design, 2026-10-15, discussion in progress with the developer; the method decided
2026-10-16 (standard SPSA over 10 parameters, below). **2026-10-20, the developer: SPSA out of
the box first**, OpenBench with fastchess (open-bench-spsa.md), rather than Rotor's own
implementation; the adaptive stage scheme is compared against it later. No implementation plan yet. Meant to replace `arena margin`'s search for `margin.txt` (margin_search.rs) once agreed.

The name is the developer's. From 2026-10-16 the method is Spall's SPSA as fishtest runs it. Until
then it was coordinate descent with a pattern search per coordinate, whose step adapted to the
gauntlet's winner, kept below for the record.

## Why

The network changes every generation, and the search parameters measured in evaluation units have to
follow it. Today one parameter follows it, `MARGIN_PER_PLY`, found per network by `arena margin`'s
gauntlet: up to four best-of-400 matches on a fixed 25 cp grid (200 against 175, 150, 125, 100).
Every search so far chose 125 or 150.

**This revises a standing decision.** Since 2026-10-03 (search-simplification.md), and restated
2026-10-12, search parameters are picked by reasonable magnitudes, not tuned, the margin the one
exception. Tuning a few more is the developer's call, to be recorded as a decision with its reason
before the plan is written.

## Decided 2026-10-16 (the developer): standard SPSA over 10 parameters

This supersedes decisions 1 to 3 below (coordinate descent by gauntlets), kept for the record. With
one parameter a gauntlet is far cheaper (today's margin search takes about 80 minutes, SPSA an
estimated 3.5 to 8.5 hours); its cost grows with every parameter, SPSA's barely, so at 10
parameters SPSA is the one affordable.

- **Per network, for now,** before its gate (decision 4 stands; open item 8 set aside).
- **On the training machine, at STC (10+0.1),** about 1,200 games an hour (measured 2026-10-16: 968 games in
  48 minutes, games played out; below); fixed nodes
  was considered for speed and set aside for now, the developer's intuition favouring a real clock.
- **The 10 parameters**, from `search.rs`'s constants (2026-10-16): `MARGIN_PER_PLY` split into its
  six uses, each its own parameter starting at 200 -- (1) razoring, (2) reverse futility, (3)
  classical futility, (4) null move's eval term, (5) delta pruning in quiescence, (6) SEE pruning in
  the main search (material, not scaled to the network's pawn) -- then (7) `SEE_MARGIN` (50), (8)
  `ASPIRATION_INITIAL_DELTA` (20), (9) `HISTORY_PRUNING_MULTIPLIER` (-35), (10)
  `GOOD_QUIET_THRESHOLD` (-128). Left out: the small integers (depth limits, plies, move counts),
  which a perturbation cannot move by a fraction; `ASPIRATION_THREAD_STEP` (tuning games run one
  thread); LMR (no constant); time management.
- **No change to the search algorithm; the code changes it needs** (the developer): the 10 become
  run-time parameters of a search, as the margin already is (`choose_best_move_with_margin`), with
  today's constants as defaults, so that one process can play two parameter sets. At the defaults
  the search is unchanged, which `FIXED_SEARCH_EXPECTED` confirms.
- **The method is fishtest's** (`spsa_handler.py`, `spsa_workflow.py`, fetched 2026-10-16): pairs
  of games, colours reversed on one opening, between theta + c_k Delta and theta - c_k Delta; each
  parameter moves by `R_k * c_k * (wins - losses) * flip`, with `R_k = a_k / c_k^2`, then is clipped
  to its range. `c_k = c / (k + 1)^0.101`, `a_k = a / (A + k + 1)^0.602`, `A = N / 10`. The start
  is worked back from the end: `c = c_end * N^0.101`, `a_end = r_end * c_end^2`,
  `a = a_end * (A + N)^0.602`.
- **What is chosen, and what follows:**
  - **N, the budget in game pairs**, chosen. It alone sets the precision (signal over noise grows
    as the square root of N).
  - **c_end per parameter**, from its scale: the gap at which theta +/- c_end differ by a few Elo.
    For a margin of 200, 25 cp (the margin gauntlet's grid); c barely shrinks (by 1.1 to 1.2 over
    a few thousand pairs), so c_end is in effect the perturbation of the whole run.
  - **r_end, derived, never chosen:** the smallest step that lets theta correct a starting error
    of about 2c within part of N. A 2-0 pair moves a parameter by 2 * r_end * c. fishtest's default
    0.002 suits its tens of thousands of pairs; with a few thousand, r_end is several times larger.
- **One Delta per pair, M pairs in flight** (M = half the training machine's logical cores, one game a core),
  theta updated as each pair finishes and the next pair started at once from the current theta.
  fishtest instead draws one Delta per worker task, about 12 games per concurrent slot at STC, for
  its round trips; we have none, and a fresh Delta per pair gives M times the directions.
- **A rough model for the margin alone** (Elo quadratic around 125, about 20 Elo lost 50 cp away, a
  guess): correcting 200 to within 10 cp of 125 takes about 10,300 games at r_end 0.002, 4,100 at
  0.005, 2,100 at 0.01, with an end noise of 2 to 4 cp. The guess is to be replaced by the curve
  measured from the margin matches (betty: @150 against @200 and @125).

- **The budget: N = 2,500 pairs (5,000 games) per network** (the developer, 2026-10-16), fixed in
  games, not hours: about 4 hours on the training machine at the estimated rate with every game played out, less
  with early stop, which shortens games but not the schedule. By the margin's model, that needs r_end of
  about 0.005 or more to travel 75 cp; a warm start from the champion's values asks less travel.
  The first tune starts from today's constants.

- **Games end early, always** (early-stop.md): SPSA's command turns on the runner's early stop,
  with no flag to forget; fishtest's draw rule plus a win rule and a tablebase rule, more aggressive
  than fishtest, to fit more pairs in the budget. No other command adjudicates.

- **The check after a tune: a duel at STC, tuned against untuned** (`arena duel`, the developer,
  2026-10-16), its games played out, without early stop. The tuned values stay unless the untuned
  win it; a tuned win or a drawn duel keeps them. Fast when one side is clearly stronger (a broken
  tune, or a large gain); a close one runs late, the curtailment firing near an even score after
  about 900 games, about 45 minutes at the estimated rate.

- **Ranges, c_end and r_end** (proposed and accepted, the developer, 2026-10-16). theta is kept
  as a real number, each player rounding its values to the nearest integer when it plays, and each
  parameter is clipped to its range after every update, as fishtest does. One r_end for all,
  **0.005**: by the margin's model it corrects a 75 cp error in about 2,100 pairs, within N = 2,500,
  which the first tune needs (six margins at 200, where the gauntlet finds 125 to 150).

  | # | Parameter | Start | Range | c_end |
  |---|---|---|---|---|
  | 1 | Razoring margin, per ply | 200 | 50 to 400 | 25 |
  | 2 | Reverse futility margin, per ply | 200 | 50 to 400 | 25 |
  | 3 | Classical futility margin, per ply | 200 | 50 to 400 | 25 |
  | 4 | Null move eval margin, per extra ply | 200 | 50 to 400 | 25 |
  | 5 | Delta pruning margin | 200 | 50 to 400 | 25 |
  | 6 | SEE pruning, main search, per ply | 200 | 50 to 400 | 25 |
  | 7 | `SEE_MARGIN` | 50 | 0 to 200 | 10 |
  | 8 | `ASPIRATION_INITIAL_DELTA` | 20 | 5 to 60 | 4 |
  | 9 | `HISTORY_PRUNING_MULTIPLIER` | -35 | -100 to -5 | 5 |
  | 10 | `GOOD_QUIET_THRESHOLD` | -128 | -512 to 0 | 32 |

Open: the game rate at STC on the training machine, to be measured (not known on 2026-10-16); then the
implementation plan.

**Measuring it (2026-10-16):** the duel amanda-0002 against caroline-0001 at 10+0.1 on the training machine,
both in-process, gate placement (half the logical cores, pinned), nothing else running
(`data/matches/amanda-0002-vs-caroline-0001/20261016-1339/`): its games and elapsed time give the
rate. Every game in it is played out: `arena`'s games end only by the rules (mate, stalemate,
insufficient material, the fifty-move rule, repetition, time; `game.rs`), with no resignation, draw
or tablebase adjudication. So its rate is a **lower bound on SPSA's**, whose games end early
(early-stop.md), and the hours above, estimated with games played out, an upper bound.

**Measured: 968 games in 48 minutes, about 20 games a minute, 1,210 an hour,** every game played
out. The estimate of about 1,200 an hour holds. N = 2,500 pairs (5,000 games) takes about 4.1 hours
at this rate, less with early stop.

## Decided before (superseded by the section above)

1. **Coordinate descent.** One parameter at a time, every other one held at its current value. Each
   parameter keeps its own step `c`.

   Random directions (a vector Delta of random signs over all parameters, as SPSA draws) were
   considered and rejected (the developer, 2026-10-15). Reasons:
   - The step `c` is the method's memory, and it belongs to a direction. Per coordinate, the next
     round asks the same question of the same axis, so the learned `c` stays useful. Along a random
     diagonal, the next round's line is a different one, and the `c` learned on the last says nothing
     about it.
   - A gauntlet asks for the best point on a line. Along one parameter's axis, that is the
     parameter's best value given the others. Along a random diagonal, the parameters are forced to
     move together in Delta's fixed proportions, so the winner is a compromise that is right for none
     of them.
   - SPSA works with random directions because it averages many small steps, whose useless parts
     cancel. A gauntlet chooses one point and jumps there, and choices do not cancel.

2. **A gauntlet of 7 players per round.** For parameter `x` at its current value `x0` with step `c`:
   `x0`, `x0 +/- c`, `x0 +/- c/2`, `x0 +/- c/4`. The current value plays, so that the step can shrink
   when `x0` is already best.

3. **The winner sets the next round: no schedule.**

   | Winner | Next `x0` | Next `c` |
   |---|---|---|
   | `x0 +/- c` | the winner | `2c`: the best lies further out |
   | `x0 +/- c/2` | the winner | `c`: the scale is right |
   | `x0 +/- c/4` | the winner | `c/2`: the best is close |
   | `x0` | `x0` | `c/2` |

   A parameter is settled once `c` falls below its resolution.

4. **When: just before the candidate challenges the champion** (the developer, 2026-10-15), where
   `arena select` runs the margin search today. The duels between lambdas select a candidate first;
   only that candidate is tuned, then the gate plays it, with its tuned values, against the champion,
   with the values tuned when it was a candidate. Both sides of the gate play their best settings, so
   the gate judges networks, not one network's settings against another's. One tuning per generation,
   not one per lambda.

## Open

1. **Which parameters.** Only those in evaluation units, the ones a new network moves. Candidates in
   `search.rs`:
   - `MARGIN_PER_PLY` (2 pawns), today's `margin.txt`;
   - **or `MARGIN_PER_PLY` split back into its four uses:** null move, razoring and both futilities,
     merged into one on 2026-10-03 because none was tuned separately;
   - `SEE_MARGIN` (half a pawn);
   - the aspiration window's delta (to check its value and unit).

   Not tuned: depths, reductions, IIR, history caps, time management. They do not depend on the
   network.

2. **How the gauntlet picks its winner.** The candidates near `x0` differ by a few Elo, so a winner
   needs many games, times six. A wrong pick between near-equal candidates costs little, as today's
   margin search already accepts. Options:
   - each candidate against `x0` only, six matches, with early elimination of a candidate clearly
     behind (as `arena sprt` stops early);
   - all seven at a fixed number of games each, a parabola fitted to their scores, the winner the
     candidate nearest its peak (less noisy than the best raw score, and free of the winner's curse:
     the best raw score is partly the luckiest);
   - a king-of-the-hill walk, as today's margin gauntlet, extended to seven players.

3. **The order of the coordinates** within a sweep, and when the whole tuning stops: every parameter
   settled, or a fixed number of sweeps.

4. **Starting values and steps.** Every parameter starts from the value it has today, or from the
   previous champion's tuned value. The first `c` per parameter: for the margin, 50 cp would cover
   today's 100 to 200 range in one round.

5. **The budget per network.** Today's margin search: at most 1,600 games. Several parameters, several
   rounds each, of six matches each, cost many times that. Several hours on the Lichess machine, or more?

   **Decided (the developer, 2026-10-16): well under a day, much less than the 17 hours a small SPSA
   tune of about 20,000 games would take on the training machine alone; Rotor uses no one else's computing power,
   and its search stays simple, with few tunable parameters.** Estimated on the training machine at 10+0.1 (about 20
   games a minute): a round of six 400-game matches is about 2 hours, a parameter 6 to 8 hours, so 3
   to 6 parameters over one or two sweeps would take 1 to 4 days, which is not affordable. The
   method is to be simplified: few parameters, not many, and a cheaper way of searching them (the
   developer's to propose).

6. **The control:** 10+0.1 (`STC`) as `arena margin` plays today, or a fixed depth for speed.

7. **Where the values live.** `margin.txt` holds one value beside the network. Several values need
   a file beside the network naming each one (format to decide), read by the engine at load, and
   embedded at release as `margin.txt` is planned to be (tuned-margin-storage.md).

8. **Tuning once, in the evaluation's own pawn (the developer, 2026-10-15).** Every margin stated in
   the evaluator's pawn rather than in centipawns, tuned once; a new network then needs only its pawn
   measured (`nnue_bench --eval-scale`, seconds), not a gauntlet. Decision 4's tuning before every
   gate would go, and with it most of open item 5's budget.

   For it:
   - The margin fits already: `200 * the network's pawn` (about 0.6 to 0.7 for amanda) predicts 120
     to 140, where the gauntlet landed (125 to 150).
   - `FALLING_SCORE_SCALE_CP` is already scaled this way, and carries over between networks.

   Where it may fall short:
   - **One number is not a network's whole scale.** amanda is compressed near equality (her pawn
     about 0.6 at p50 and p75) and stretched far from it (1.89 at p99, search-simplification.md).
     Futility and razoring act near equality, null move's term further out; a scale measured where
     each margin applies would fix it, at more cost.
   - **A margin guards against the evaluation's errors, not its scale.** Two networks with one pawn
     but different error spreads want different margins. For psq networks the two moved together
     (amanda's error tails 0.6 to 1.0 of hand-crafted's, eval-error-margins.md); a new architecture
     (hilda's buckets) may break that.
   - **A search change still needs a new tuning.** "Once" means once per search change or new
     architecture, not once ever.

   **Set aside: per network, for now (the developer, 2026-10-16).** A better network may want a
   more aggressive margin, not only a rescaled one (betty-0003 preferring 150 to 200 while
   amanda-0003 chose 200), so a pawn measurement alone cannot replace the tuning. Decision 4
   stands: about 10 parameters tuned per network, before its gate, within open item 5's budget.

   **Adoption, if taken up again:** tune once in pawn units on the branch; for the next few networks, run a cheap check
   beside it (today's margin gauntlet) against the scaled values; if they hold within one 25 cp step,
   drop the tuning per network, and tune again only when the search or the architecture changes.
