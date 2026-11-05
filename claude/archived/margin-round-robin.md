# A round robin for `MARGIN_PER_PLY`

**Superseded, see margin-search-command.md and tuned-margin-storage.md:** the round robin became a
gauntlet (round 1, cutechess-cli), then the search tree of `rotor-arena margin`, run inside
`rotor-arena select`.

**Status:** design decided 2026-10-08; round 1 played (cutechess-cli), round 2 replaced.

## Why

Untangled with the developer, 2026-10-08: a network's margin is `MARGIN_PER_PLY * pawn scale`.
The pawn scale is a unit conversion, measured (on Lumbra, pawn-scale-positions.md); it is not a
choice. `MARGIN_PER_PLY` (200 cp, in hand-crafted's pawn) is the one free choice, the pruning risk
the search accepts, the same for every evaluator. The developer wants play to decide it, coarsely:
a small round robin of values around 200.

The duel of pawn-scale-positions.md is already a two-entry version: amanda at 55% against 70% is
200 against about 255 cp per ply on the correct unit, heading for a draw (48.1% at 108 games).

## Design

- **Engines differing only in the margin:** `MARGIN_PER_PLY` read at compile time from
  `ROTOR_MARGIN_PER_PLY` (`option_env!`), 200 when unset, so the ordinary build is untouched.
  TEMPORARY: removed after the round robin, the winning value written in as the constant. Each
  value is one release build copied aside as `~/rotor-margin-<value>.exe`.
- **The tool:** cutechess-cli's `-tournament round-robin`, one `-engine` per value, as the duels:
  10+0.1, 1 thread, 16 MB hash, UHO_Lichess_4852_v1 `order=random`, colours reversed, 8 at once,
  on idle the Lichess machine, plugged in.
- **The network:** amanda-0001 (generation 2's teacher), at her Lumbra pawn scale of 55%, once the
  current duel has adopted it.
- **Reading it:** points, as the arena's own round robin ranks (in a balanced round robin the
  same order as a rating fit), each with its standard error.

## Two rounds, coarse then fine (the developer, 2026-10-08)

- **Round 1, large steps:** 100, 200 and 400 cp per ply, each a factor of 2 from the next, far
  enough apart that a round robin can separate them.
- **The rule, each round (the developer, 2026-10-08):** a value replaces the incumbent only if its
  score in their direct pairing (400 games) is more than 2 standard errors above 50%, about 54.5%
  or +30 Elo. Looser than the duel rule (57.7% at 400 games), since each round is one planned
  comparison, not a sequence of checks. Otherwise the incumbent stays. Round 1's incumbent is 200.
- **Round 2, a search on a 25 cp grid (the developer, 2026-10-08), superseding a factor of 1.4
  either side:** one match at a time, the probe against the best value so far, played as
  `rotor-arena`'s best of 400 (in-process-clock.md): the first past 200 points wins, the match
  ending as soon as the lead cannot be caught; 200 each keeps the best value so far. **Not the
  54.5% rule above** (the developer, 2026-10-08: "this does not need to be exact"): neighbours
  25 cp apart likely differ by 5 to 10 Elo, so a step is often decided by noise, accepted. Linear steps, since inside [100, 200] they barely differ
  from geometric ones. The tree, from round 1's 100 against 200:

  | match 1 | match 2 | match 3 | match 4 | possible winners |
  |---|---|---|---|---|
  | 100 wins | 150 vs 100: 150 wins | 125 vs 150 | -- | 125, 150 |
  | 100 wins | 150 vs 100: 100 holds | 125 vs 100 | -- | 100, 125 |
  | 200 holds | 150 vs 200: 150 wins | 175 vs 150 | -- | 150, 175 |
  | 200 holds | 150 vs 200: 200 holds | 250 vs 200 | 225 vs 250, only if 250 won | 200, 225, 250 |

  Reachable: 100, 125, 150, 175, 200, 225, 250; the builds only along the path taken. No search
  below 100 nor above 250: 100 winning everything, or 250 holding against 225, is recorded as the
  bracket's edge, not searched further. 400 is not searched (round 1 shows it weaker, 40.7% at 108
  games); for later networks the upper side stays as the last row.

- **400 games per pairing (the developer, 2026-10-08):** 1,200 games a round, about 70 minutes;
  each value plays 800, a standard error of about 1.5% on its score.

## Per network, in every generation's pipeline (the developer, 2026-10-08)

The developer holds that each network, once trained, needs its own margin. A generation then runs:

1. **The lambda round robin** chooses the best lambda, every candidate at `MARGIN_PER_PLY` = 200
   times its own pawn scale (measured at load on the Lumbra positions), so every candidate prunes at
   the same risk in its own centipawns: lambda likely changes a network's output scale.
2. **The margin round robin** (this document's two rounds) tunes the selected candidate's margin.
3. **The challenge** against the champion, each at its own tuned margin (the champion's tuned when
   it was promoted).
4. **Promotion**: the promoted network labels the next corpus with its tuned margin.

Consequences, to design next:

- A tuned margin cannot be measured at load: it is stored with the network (in the `.nnue`, or in
  a file beside it) and read by the engine, the pawn scale remaining for networks with none (every
  candidate in step 1).
- About 2 h 20 min per network for both rounds, under 5% of a 48-hour replay; only the selected
  candidate, never all nine lambdas.
- amanda-0001, today's champion and generation 2's teacher, is tuned first, by the two rounds as
  designed above.

## Decisions

All decided. The pawn-scale duel was stopped inconclusive (pawn-scale-positions.md); amanda plays
at her Lumbra scale of 55%, the tournament compensating for any scale.

**A gauntlet, not a round robin (the developer, 2026-10-08):** the rule only compares each value
with the incumbent, so the challengers' pairing with each other never enters the decision. Each
round is the incumbent against each of the other two, 400 games each: 800 games, about 45 minutes.
Should both challengers clear the rule, the one scoring more against the incumbent is taken.

**Superseded for round 2 by in-process-clock.md (one build, `@<cp>` players), which removes the
build variable; round 1's three executables, already built, stay valid.** **Implemented
2026-10-08:** `EVAL_MARGIN_PER_PLY` in `search.rs`, `MARGIN_PER_PLY` unless the build
sets `ROTOR_MARGIN_PER_PLY` (parsed at compile time, a malformed value failing the build), read
by every evaluation margin (`set_pawn_percent`, delta pruning); SEE pruning keeps `MARGIN_PER_PLY`,
being material against material. Unset, the build is unchanged.

**Round 1's builds (2026-10-08, the Lichess machine):** amanda-0001 at 55%, depth 9, the 49 bench positions:
100 cp 880,361 nodes (0.56 s), 200 cp 1,194,538 (0.72 s, the ordinary build's, unchanged), 400 cp
1,404,802 (0.80 s). Each build recompiled when the variable changed. Copied aside as
`~/rotor-margin-100.exe`, `-200`, `-400`.

3. The decision rule: what result moves `MARGIN_PER_PLY` away from 200.
