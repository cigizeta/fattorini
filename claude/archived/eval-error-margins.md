# Pruning margins from the evaluator's own error

**Superseded, see margin-search-command.md:** a network's margin is now tuned by play.

**Status:** closed 2026-10-08: the measured margins lost their duel (48.2% at 141 games, stopped;
"The duel" at the end). The scaled margins of psq-node-cost.md stay; next, expected-score-margins.md.

## Why

The developer, 2026-10-08: `MARGIN_PER_PLY` may want tuning, but a tuning tied to one evaluation
function cannot be rerun for every network Rotor produces.

A margin is a bet: the static evaluation will not be wrong by more than `margin(d)` within the `d`
plies below. Its right size depends on two properties of the evaluator, not on the function itself:

1. **Its scale**: how big its pawn is. psq-node-cost.md measures this at load
   (`nnue::measure_pawn_percent`, amanda-0001 70%) and scales the margins by it.
2. **Its error**: how far a search of `d` plies moves away from the static evaluation. Nothing
   measures this today. The scaled margins assume it is proportional to the scale, i.e. that 2
   pawns/ply was right for hand-crafted and a network errs in the same proportion.

This proposal measures (2) directly, per evaluator, and derives the margins from it, so that a new
network gets its margins without a tuning run. Texel tuning does not apply: the margins never enter
a static evaluation, only which subtrees are searched, so their effect is visible in play only.
SPSA over games is out of budget, and its gains (a few Elo) are below what the duel rule resolves
(about +34 at 1000 games).

## Idea

For an evaluator `E` and a small depth `d`, the **residual** of a position is
`r_d = search_d(E) - static(E)`, in `E`'s own units. Its distribution over a fixed set of positions
says how far `d` plies of search can move `E`'s judgement. The margin at `d` is a high quantile of
it: `margin(d) = Q_q(|r_d|)`, or one-sided (below).

The only constant left is `q`, dimensionless and the same for every evaluator. **Chosen after step
0 (decided 2026-10-08, the developer):** step 0 prints several quantiles (90, 95, 99%), and `q` is
the one whose margins for hand-crafted come closest to today's 2 pawns/ply, the pinned anchor
already known to play well. **No duel chooses `q`; duels judge the framework** (the developer,
2026-10-08): with `q` fixed, the champion network with measured margins against the same network
with scaled margins. A duel per `q` would bring back the tuning by games this design avoids. It
assumes one `q` suits every evaluator, which that duel tests.

## Step 0: measure before deciding (a bench, no engine change)

The design is worth building only if it changes the margins. Before any engine change:

- A bench mode (`nnue_bench --residuals`, say) prints the residual quantiles at `d` = 1 to 6 for
  hand-crafted, amanda-0001, betty-0001 and caroline-0001.
- Compared with today's margins, `2 pawns x d x scale`:
  - **Close for every evaluator:** the scale was a good enough proxy; this proposal is shelved and
    the scaled margins stay.
  - **Different:** the scale corrects the wrong quantity, and the rest of this plan goes ahead.
- It also shows the **shape** in `d`: linear (today's "per ply"), sub-linear (errors partly cancel,
  `sqrt(d)`-like), or something else. If not linear, the margin becomes a small table per depth,
  not a constant per ply.

Minutes of CPU on the Lichess machine, no games.

### Step 0: implementation plan (drafted 2026-10-08, to approve)

A new mode of `benches/nnue_bench.rs`, beside `--eval-scale`; no change to `src/`, so the label
path is untouched.

- **Flag:** `--residuals [dir]`, with `--max-depth D` (default 4) and `--sample N` (default
  2,000 positions). With a network path it measures that network; with `--hand-crafted`, the
  hand-crafted evaluation. One evaluator per run, so the four are four commands.
- **Positions:** `selection_positions(dir)` as `--eval-scale` reads them (side to move not in
  check), by default betty's 10+0.1 duel against hand-crafted (`--quantization`'s default:
  amanda's generation-1 selection is on the training machine only), `N` spread evenly over them, so every
  evaluator gets the same positions. Decision (1) of the open questions, game positions or nodes
  from inside searches, is read from these numbers: if the residuals vary strongly with `|static|`,
  game positions understate interior nodes, and the printout below shows it.
- **The search (decided: pruning off):** a plain search written in the bench, not `Search`, so
  that no pruning of the engine's leaks in. Full-window negamax alpha-beta to depth `d`, moves
  ordered captures first by MVV-LVA (ordering changes the node count, never the value), no TT, no
  extensions, no null move; at the leaves a quiescence of captures and promotions with stand-pat,
  no delta or SEE pruning. Mate and stalemate scored as the engine scores them; a position whose
  value at any depth is a mate score is left out (a margin never acts there). The evaluator is a
  closure `Fn(&Board) -> i32` (`network.evaluate` or `eval::evaluate`), refreshed from scratch
  per call: slow, but exact and simple; `std::thread::scope` over the positions, one per logical
  core, keeps it within minutes.
- **Residual:** `r_d = value_d - static`, from the side to move, in the evaluator's own
  centipawns, for `d` = 0 (quiescence alone, delta pruning's) to `D`.
- **Printed, per `d`:**
  - the mean of `r_d` (a bias: a network regressed toward a blended target may lean one way);
  - the 50, 90, 95 and 99% quantiles of each tail: `r_d` (static underestimates: razoring,
    classical futility, delta) and `-r_d` (static overestimates: reverse futility, null move);
  - today's margin, `MARGIN_PER_PLY * d` in the evaluator's pawn (`pawn_percent`), beside them;
  - the same quantiles restricted to `|static|` above 300 cp, for decision (1).
- **Timing:** the seconds per depth, for decision (4), at load or stored.
- **Tests:** as built (2026-10-08), a check at the start of every `--residuals` run, since a
  `harness = false` bench runs no `#[test]`: on 8 of the positions, at depths 0 to 2, the plain
  search whose full-window value is `v` fails high on `(v - 1, v)` and low on `(v, v + 1)`, as a
  fail-soft alpha-beta must and a broken cutoff would not (hand-crafted evaluation; the property
  does not depend on the evaluator). Two earlier versions never finished, against the search
  without alpha-beta and against the reverse move order: without a TT, both explode in quiescence
  on tactical positions. The `r_0` check was dropped:
  implied by stand-pat.

### A reference PGN (planned 2026-10-08, to approve)

The bench's positions so far come from betty's duel against hand-crafted: Rotor's own engines,
2,000 positions, so a p99 rests on about 20. Discussed with the developer:

- **Not CCRL:** the networks train on it (every generation's corpus), so their error there
  understates their error elsewhere; and engine games stay near equality.
- **Human games, held out from every corpus:** `LumbrasGigaBase_OTB_ELO2400.pgn` (Lumbra's
  Gigabase, over-the-board games of players rated 2400 or more), downloaded by the developer
  2026-10-08: 863,790 games, too many to load whole.
- **Cut once to a fixed subset:** 5,000 games evenly spaced over the file, skipping games under 40
  plies (short draws), written zstd-compressed (the arena's encoder, level 19) to
  `data/lumbra/LumbrasGigaBase_OTB_ELO2400-5000.pgn.zst`, never regenerated. **On Google Drive
  (the developer, 2026-10-08)**, as the rest of `data/` is (`pipeline::remote`, which mirrors
  `data/X` at `G:/My Drive/rotor/data/X`), **with the whole source beside it, recompressed as
  `LumbrasGigaBase_OTB_ELO2400.pgn.zst`** (the developer, 2026-10-08, instead of the original
  `.7z`: one format for every PGN on Drive, and the cut reads it straight from Drive, so the
  reference can be rebuilt from Drive alone).
  A one-off mode of the bench, `nnue_bench --cut-reference <big.pgn>`, streams the big file twice
  (a count, then the cut), writes the subset under a `.partial` name and renames it, then copies
  up the subset alone (`remote::copy_up`). The source's `.pgn.zst` the developer writes directly
  to `G:/My Drive/rotor/data/lumbra/` with the `zstd` CLI; the cut reads it there, decompressing
  as it reads. No uncompressed copy is kept anywhere. `--residuals`, finding no local copy, copies it down first (`remote::copy_down`), so
  every machine gets the same file with no step of its own. **Implemented 2026-10-08.**
- **The bench's defaults:** `--residuals` reads that file, decompressing it as it reads,
  `--sample` 10,000 positions, so that a p99 rests on about 100. A directory argument still
  overrides it, as today.
- Results so far, from betty's duel, stay below as they are; the four evaluators rerun on the
  reference.

### Step 0: results

**Hand-crafted (2026-10-08, the Lichess machine, 2,000 positions of betty's duel, 4.0 s in all).** Quantiles
in cp; "under" is `r = value - static`, "over" is `-r`; today's margin 200 cp per ply.

| d | margin | mean r | under p90 / p95 / p99 | over p90 / p95 / p99 |
|---|---|---|---|---|
| 0 | 0 | +51.6 | 140 / 359 / 746 | 0 / 0 / 0 |
| 1 | 200 | +60.9 | 162 / 365 / 826 | 5 / 28 / 232 |
| 2 | 400 | +52.5 | 205 / 389 / 886 | 33 / 65 / 265 |
| 3 | 600 | +56.6 | 208 / 398 / 890 | 39 / 82 / 620 |
| 4 | 800 | +46.9 | 223 / 417 / 901 | 60 / 113 / 676 |

Beyond 300 cp of `|static|` (about 700 positions) every tail is wider: under p95 498 to 829, over
p95 0 to 310.

What it says:

1. **The error does not grow linearly with depth.** From `d` = 1 to 4 the under tail barely
   moves (p95 365 to 417); the over tail grows, but far below 200 cp per ply (p95 28 to 113).
   Today's linear margin is too narrow for the under tail at `d` = 1 (200 against a p95 of 365)
   and wider than either tail's p95 by `d` = 3.
2. **The two tails differ by a factor of 3 to 10.** The under tail is mostly quiescence's own
   gain (`d` = 0 already has a p95 of 359): game positions are often mid-exchange, with a
   recapture pending that the static evaluation does not see. The over tail, what reverse futility
   and null move bet on, is small. One margin for both is the wrong shape.
3. **So no single `q` reproduces 200 cp per ply** for hand-crafted, as the plan assumed it would:
   the calibration that was to fix `q` has no solution. The shape, not only the scale, is off.
4. **Far from equality the errors are larger** (decision (1)): game positions near equality
   understate them, so interior nodes, often unbalanced, likely err more than this sample.
5. Cost: 2.9 s at `d` = 4 with hand-crafted on 16 threads.

**Which residual each pruning bets on (read in `search.rs`, 2026-10-08).** Razoring returns
quiescence's value outright, and classical futility skips quiet moves only (captures are still
searched): both bet on `value_d - value_0`, the search above quiescence, not on `value - static`,
whose under tail above is mostly quiescence's own gain. Reverse futility returns the static
evaluation: it bets on `static - value_d`, the over tail above. Null move's margin only sets its
reduction (`null_move_plies`), not a cutoff: no tail. So the bench now also prints
`value - qsearch`, and the networks run with all three tails.

**Hand-crafted, rerun with the right tails (2026-10-08, the Lichess machine, 3.2 s):**

| d | margin | `value - qsearch` p95 / p99 | `static - value` p95 / p99 |
|---|---|---|---|
| 1 | 200 | 53 / 105 | 28 / 232 |
| 2 | 400 | 70 / 431 | 65 / 265 |
| 3 | 600 | 92 / 490 | 82 / 620 |
| 4 | 800 | 115 / 769 | 113 / 676 |

This corrects points 1 to 3 above, which read the wrong tail for razoring and futility:

- **Both tails are now alike** and grow with depth, roughly linearly at p95 (about 25 to 30 cp per
  ply) and at p99.
- **Today's 200 cp per ply sits near the 99% quantile** of both from `d` = 2 on (431, 490, 769
  against 400, 600, 800; 265, 620, 676 likewise), and is wider than both at `d` = 1. So the
  calibration has a solution after all: **`q` is about 99%** for hand-crafted, meaning today's
  margins prune wrongly about once in a hundred.
- The tails are heavy: p99 is 4 to 8 times p95, so the margin is set by rare tactical positions,
  not by typical ones. Far from equality, p99 is wider still (809 to 881 at `d` = 2 to 4).

Next: the three networks, read at `q` = 99%, against their scaled margins.

**Hand-crafted on the Lumbra reference (2026-10-08, the Lichess machine, 10,000 positions, 18.6 s).** The cut
kept every 155th of the 778,390 games of 40 plies or more.

| d | margin | `value - qsearch` p95 / p99 | `static - value` p95 / p99 |
|---|---|---|---|
| 1 | 200 | 48 / 66 | 8 / 67 |
| 2 | 400 | 36 / 91 | 29 / 81 |
| 3 | 600 | 56 / 109 | 27 / 91 |
| 4 | 800 | 51 / 123 | 43 / 106 |

- **The tails are 4 to 7 times narrower than on betty's duel** (p99 at `d` = 4: 123 against 769,
  and 106 against 676). Masters' positions are calm; the duel's, between two weak engines at
  10+0.1, are not. So **the quantile today's margins correspond to depends on the positions
  measured**: about 99% on the duel, far beyond 99% here. `q` read from one source does not
  transfer to another, and neither source is the search's interior nodes (open question 1).
- **Beyond 300 cp of `|static|`, the Lumbra positions are mid-exchange, not unbalanced:** p50 of
  `value - static` is about +340 at every depth, a recapture pending. In balanced human games, a
  large static evaluation means a capture just made, so that subset says nothing about lopsided
  positions here.
- An odd-even pattern: `value - qsearch` p50 is 15 at `d` = 1 and 3, 0 at 2 and 4, the side that
  moves last gaining a tempo.

**What still transfers: ratios between evaluators on the same positions.** If a network's p99
over hand-crafted's is about the same on both sources, that ratio, not an absolute `q`, sets the
network's margin (`200 cp * ratio` per ply), anchored to hand-crafted's pinned 200. So the
networks run on both sources, and the ratios are compared.

**Amanda on the Lumbra reference (2026-10-08, 74.5 s in all), p99 against hand-crafted's, each in
its own centipawns:** `value - qsearch` 49 / 66, 57 / 91, 67 / 109, 73 / 123 at `d` = 1 to 4
(ratios 0.74, 0.63, 0.61, 0.59); `static - value` 70 / 67, 64 / 81, 86 / 91, 75 / 106 (1.04, 0.79,
0.95, 0.71). Both bracket her pawn scale of 70%: the scaled margins are in the right range,
perhaps a little wide for razoring and futility. Her `value - static` at `d` = 0 (p95 56 against
282) shows she already sees most pending captures, trained as she was on search scores.

**The second source is not betty's duel (the developer, 2026-10-08):** local to the Lichess machine, not a fixed
reference, and betty is an evaluator under measurement. A second attempt, **random walks from the
reference** (`--random-plies P`: each Lumbra position walked 1 to `P` random legal plies, seeded),
was run once and then **rejected and removed (the developer, 2026-10-08): Rotor never plays random
moves.** A search tries bad moves too, but alpha-beta refutes most within a ply or two; it almost
never follows a line where both sides blunder for several plies, which a uniform walk weighs
heavily. So the walked positions overstate how unbalanced interior nodes are, and are no evidence.
Kept below as a record only.

**Walked 1 to 6 random plies, no evidence (2026-10-08, the Lichess machine, 9,954 positions), p99, amanda /
hand-crafted:**

| d | `value - qsearch` | ratio | `static - value` | ratio |
|---|---|---|---|---|
| 1 | 76 / 146 | 0.52 | 251 / 102 | 2.46 |
| 2 | 479 / 282 | 1.70 | 113 / 109 | 1.04 |
| 3 | 335 / 341 | 0.98 | 271 / 158 | 1.72 |
| 4 | 625 / 378 | 1.65 | 149 / 178 | 0.84 |

The ratios swung from 0.5 to 2.5, and amanda's error alternated with the parity of `d` (mean
`value - static` 40, 79, 50, 92 cp), which hand-crafted's did not: plausibly a psq network, with no
threat features, misjudging positions full of pieces en prise. On positions the search rarely
visits, so not a finding about play.

**Verdict of step 0 (agreed 2026-10-08, the developer): shelved.** The one trustworthy
measurement, the played Lumbra positions, puts amanda's p99 at 0.6 to 1.0 of hand-crafted's,
around her pawn scale of 0.7: the scaled margins are already in the right range, and nothing
suggests a measured margin would do better. The scaled margins stay, and the replay restarts under
them. Reopened only with nodes sampled from the engine's own search, weighted as it visits them,
between generations. `--residuals` and the Lumbra reference stay in `nnue_bench` for then.

Commands, once built (the developer runs them on idle the Lichess machine, plugged in):

`cargo bench --bench nnue_bench -- --residuals --hand-crafted`
`cargo bench --bench nnue_bench -- data/networks/amanda/gen-0001/champion.nnue --residuals`

and likewise for betty-0001 and caroline-0001. The four printouts are recorded here; `q` is read
from hand-crafted's (the quantile nearest 200 cp per ply), then the networks' margins at that `q`
are compared with their scaled ones: go or shelve.

## Open questions

1. **Which positions.** Margins act at interior nodes, which are often unbalanced, not at game
   positions near equality (where `measure_pawn_percent` reads). Candidates: positions from the
   replay corpus's games (cheap, realistic for roots, too balanced for interior nodes); or nodes
   sampled from inside real searches, with their static evaluation and the value a re-search of
   `d` plies returns. The second is closer to what the margins see; to decide after step 0 shows
   whether the two differ.
2. **One-sided or two-sided.** Reverse futility and null move bet the evaluation does not
   **overestimate** (`static - search` small); razoring and classical futility bet it does not
   **underestimate** (`search - static` small). A network regressed toward a blended target may be
   biased one way, so the two tails may differ. Step 0 prints both; if they differ, each technique
   reads its own tail.
3. **Which search: pruning off (decided 2026-10-08, the developer).** `search_d` is a plain
   full-window alpha-beta plus quiescence, with none of the margin-based pruning it calibrates, so
   it measures the evaluator alone and is not circular.
4. **When it is measured.** At load like the pawn scale (deterministic, no format change, but a few
   seconds of searches before `uciok` is likely too slow for GUIs and lichess-bot), or once per
   network by `rotor-train`/`nnue_bench` and stored beside the `.nnue` (or in it: a format change).
   To decide from step 0's timing.
5. **Hand-crafted: pinned (decided 2026-10-08, the developer).** It keeps today's 2 pawns/ply
   exactly, and only networks are measured: its trees and node-count references do not move, and
   it stays the fixed anchor the networks are compared against. Step 0 still prints its residuals,
   for comparison.
6. **Rounding: 5 cp, clamped (decided 2026-10-08, the developer).** Each margin is rounded to
   5 cp of the evaluator's units, as the pawn scale is rounded to 0.05, so that near-identical
   networks search the same trees, and clamped (say 0.25x to 2x today's scaled margin) against a
   degenerate network. The clamp's exact range follows step 0.
7. **Delta pruning: the `d` = 0 residual (decided 2026-10-08, the developer).** It compares a
   static evaluation plus material against alpha, so its margin is the quantile of the quiescence
   residual (quiescence's value minus the static evaluation), measured like the others. Step 0
   prints it too. SEE pruning compares material with material and stays as is.

## Label path

Changes the search tree, so it lands between generations, never during a replay or self-play
(the freeze in TODO.md). Judged by one duel at 10+0.1 on idle the Lichess machine, the champion network on both
sides; as with the scaled margins, the aim is labelling speed without loss of strength, so the pass
condition (a draw passes, or not) is decided before the duel starts.

## Steps (after the scaled-margins duel and the discussion)

1. Step 0's bench; numbers recorded here; go or shelve.
2. Decide the open questions above from those numbers.
3. Implement, with tests: a known residual set gives known margins; hand-crafted's treatment as
   decided in (5); node counts recorded as new references.
4. The duel.

## The duel (reopened 2026-10-08, plan to approve)

The shelving verdict rested on measurement alone; the developer wants play to judge. One duel, as
an experiment: no `.nnue` format change and no storage of the ratios until the idea wins.

**The margins (decided 2026-10-08, the developer).** One ratio per tail, the mean over `d` = 1 to 4
of the network's p99 over hand-crafted's on the Lumbra reference (per-depth ratios rest on about
100 tail positions each and wobble more than they plausibly differ). The margin is
`MARGIN_PER_PLY * ratio` per ply, in the network's own centipawns, rounded to 5 cp (decision 6):

| amanda-0001 | ratio | per ply | scaled today |
|---|---|---|---|
| razoring, classical futility (`value - qsearch`) | 0.64 | 130 cp | 140 cp |
| reverse futility (`static - value`) | 0.87 | 175 cp | 140 cp |

Unchanged, still the pawn scale: the null move's reduction term (its margin sets plies, not a
cutoff) and delta pruning (decision 7 is not taken up: its `d` = 0 tail of `value - qsearch` is 0
by definition, and the experiment stays small). Hand-crafted pinned (decision 5).

**Pass rule (decided before the duel, 2026-10-08, the developer): measured must win.** Adopted only
if shown stronger by the duel rule (about 54.9% at 1000 games); a draw keeps the scaled margins,
since measured margins add a measurement per network and two margins instead of one, a cost that
needs a shown gain. A draw is the likely outcome: the margins differ by -7% and +25%.

**The build (temporary).** `Search` keeps two margins per ply instead of `eval_margin_per_ply`:
`razor_margin_per_ply` (razoring, classical futility) and `reverse_margin_per_ply` (reverse
futility). Scaled, both equal today's `eval_margin_per_ply`, so the scaled build's trees and node
counts do not move. The measured build sets them from two constants, `MEASURED_RAZOR_PERCENT = 64`
and `MEASURED_REVERSE_PERCENT = 87`, marked TEMPORARY with this doc's reason inlined, applied only
when a network is loaded. That build is copied aside as `~/rotor-measured.exe`; the constants are
amanda-0001's only, so it is never run with another network. After the duel the constants go: if
measured wins, a design for measuring and storing the ratios per network follows; if not, the two
fields fold back into one.

**The duel itself.** cutechess-cli on idle the Lichess machine, plugged in, as the scaled-margins duel:
`measured` (`~/rotor-measured.exe`) against `scaled` (`~/rotor-scaled.exe`, already aside), both
`EvalFile` amanda-0001, 10+0.1, 1 thread, 16 MB hash, UHO_Lichess_4852_v1 `order=random`, 8 at
once, 500 colour-reversed pairs, `duel-measured-margins.pgn`. Judged by the duel rule over the
whole PGN.

**Tests.** With both percents at the pawn scale, node counts on the 49 bench positions equal the
scaled build's (amanda-0001, `nnue_bench --search-only`); hand-crafted's 3,140,380 unchanged.

**Built 2026-10-08 (feature `measured-margins`).** All 615 library tests pass; hand-crafted
3,140,380 nodes, unchanged. Amanda-0001, depth 9, 49 positions: scaled 1,226,878 nodes (0.75 s),
measured **1,335,741 (+8.9%, 0.83 s)**: the wider reverse-futility margin (175 against 140) costs
more nodes than the narrower razoring and futility one (130) saves. So measured margins would
label about 10% slower; a win would have to be weighed against that. `~/rotor-measured.exe`
copied aside the same day.

**Result (2026-10-08, the Lichess machine): stopped by the developer at 141 games**, `measured` +36 -41 =64
(8 games without result, in progress at the stop), 48.2%, about -12 +/- 42 Elo
(`duel-measured-margins.pgn`): 54.9% at 1000 was out of reach. **Measured margins fail; the
scaled ones stay.** The temporary code (the `measured-margins` feature, its two constants, and
`Search`'s two extra margins) is removed. The developer does not want the scaled margins either:
the next attempt is expected-score-margins.md.
