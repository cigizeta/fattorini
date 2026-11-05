# Rotor's search against Stockfish's: the uses of the static evaluation

**Status:** steps 1 and 2 done 2026-10-31: `corr` and `blend` kept, `pv`, `root` and `ttquiet`
reverted ("Decided"); `blend`'s SPSA follow-up waits for SPSA. Step 3 dropped with the quiet filter
(quiet-filter.md). The plan was three steps (the developer, 2026-10-31): 1, this comparison, limited
to where the search reads the static evaluation; 2, each change chosen tested on its own in arena
against today's search, kept only if it gains; 3, then the experiment with the full quiet filter
(two moves) on the new corpus.

**Why:** `-quiet` lost its duels against `lc0-psq-threats-screlu-g1-r1` by about 100 to 160 Elo
while ordering siblings better at quiescence and worse statically (TODO, 2026-10-30). The
hypothesis: the search trusts the static evaluation at nodes that are not quiet (pruning decisions
made before quiescence, stand-pat with a capture pending), and quiescence never corrects those.
Stockfish trains on a filtered corpus and still wins, so its search may lean on the static
evaluation less, or more carefully. This compares the two.

**Sources:** Stockfish's `src/search.cpp`, master as fetched 2026-10-31
(raw.githubusercontent.com/official-stockfish/Stockfish/master/src/search.cpp), quoted, not
recalled. Rotor's `crates/rotor/src/search.rs` on branch `elio` the same day. Margins are not
comparable number for number: Stockfish's internal scale and its tuning differ from ours.

## Side by side

| Use | Stockfish | Rotor |
|---|---|---|
| **Root** | Evaluated; feeds `improving`, the correction history and ProbCut's threshold. No futility, razoring, reverse futility or null move there. | Evaluated only when `remaining <= FUTILITY_MAX_DEPTH` (3). Classical futility prunes quiet root moves in those iterations. |
| **Node types** | PV, cut and all nodes. Razoring only at all nodes; reverse futility never at a `ttPv` node (PV now, or ever stored as PV); null move only at cut nodes; quiet futility skipped on the line being followed at a PV node. | None: every pruning below applies at PV and non-PV nodes alike. |
| **Correction history** | Every static evaluation corrected before use (`to_corrected_static_eval`), in `search` and `qsearch`: pawn, minor-piece, white and black non-pawn keys, and continuation correction at 2, 4 and 6 plies back. Updated at each node's end when not in check, the best move not a capture, and the error's sign consistent with the bound. | None. |
| **In check** | No evaluation; `staticEval` copied from two plies back, so `improving` still means something. | No evaluation; `improving` false for the node. |
| **Razoring** | `allNode && eval < alpha - 342 * depth`, returns `qsearch`. | Any node below the root, any depth, `eval < alpha - margin * remaining`, returns quiescence. |
| **Reverse futility** | `!ttPv && depth < 19 && eval >= beta && (no TT move or TT move a capture)`; margin scales with depth, lowered by `improving` and `opponentWorsening`, raised without a TT hit and by the correction's size. Returns `(661 * beta + 363 * eval) / 1024`, not `eval`. | `ply > 0 && remaining <= 3`, margin per ply fixed. Returns `static_eval`. No TT-move condition. |
| **Null move** | Cut nodes only; `staticEval >= beta - 13 * depth - 47 * improving + 365` (plus earlier null-move fail highs); `R = 7 + depth / 3 + (staticEval - beta) / 256`; a verification search from depth 16. | Any node below the root, `eval >= beta`, `R = 3 + remaining / 4 + (eval - beta) / margin` (at most 3 from the eval), no verification. |
| **Futility, quiet moves** | Not at the root. On `lmrDepth` (depth after the expected reduction, adjusted by history), `lmrDepth < 12`; value `staticEval + 119 * lmrDepth + 90 * (staticEval > alpha) + 164`; raises `bestValue` to the futility value when it prunes. | At the root too. `remaining <= 3`, margin per ply fixed, first legal move never pruned. `best_score` untouched. |
| **Futility, captures** | `lmrDepth < 8`, not giving check: `staticEval + 234 + 247 * lmrDepth + captured piece + capture history <= alpha` prunes. | None found in the main search (SEE pruning only). |
| **Late-move pruning** | `(3 + depth^2) / (2 - improving)`. | The same formula, at nodes below the root. |
| **ProbCut** | Uses `staticEval` as the capture picker's threshold. | Not implemented. |
| **Stand-pat** | Corrected evaluation, refined by the TT's value when its bound allows; a cutoff returns `(441 * value + 583 * beta) / 1024`. | Raw evaluation, returned as it is. |
| **Quiescence pruning** | `futilityBase = staticEval + 306`; for a capture not giving check, not a recapture, not a promotion: after two moves skip, below alpha with the victim skip (raising `bestValue`), SEE below `alpha - futilityBase` skip. Then SEE below -74 skip. | Delta pruning on the victim plus `delta_margin`; a move-count cap (`QUIESCENCE_MOVE_LIMIT`) with the same exemptions; SEE below `-see_margin`. |

## Candidate changes, ranked

Ranked by how directly each bears on trusting a misjudged static evaluation, then by cost. Each is
tested alone (step 2); none is kept for being Stockfish's.

1. **No razoring, reverse futility or null move at PV nodes** (`beta - alpha > 1`). Cheap: a guard
   on each. On the principal variation an evaluation error prunes the line the engine plays.
   Stockfish's finer split (cut against all nodes, `ttPv`) can follow if this one gains.
2. **No futility at the root.** One guard; small expected effect, deeper iterations never reaching
   it.
3. **Reverse futility only without a quiet TT move** (no TT move, or a capture). Cheap. Keeps the
   static cutoff away from nodes where an earlier search found a quiet move mattering.
4. **A correction history**, pawn and non-pawn keys first, continuation later. The largest of the
   five, and the most relevant: it learns where the evaluation errs, positions with a capture
   pending among them, and corrects stand-pat and every pruning margin together.
5. **Blended returns** for reverse futility and stand-pat (toward beta). Cheap; the gain unknown.

Out of scope for now: ProbCut, null-move verification, cut-node tracking, capture futility, LMR
and the other parts of the search the developer limited away (2026-10-31).

## Defaults, by candidate

Every test is untuned, so a candidate's default is what the test measures: each is chosen by what
it means, written here before any code (the developer, 2026-10-31). Stockfish's values are tuned
for its own search and evaluation, so none is copied for being Stockfish's.

**The existing margins stay as they are in this round.** Six margins share `MARGIN_PER_PLY`, two
pawns per ply in the evaluator's pawn: a pruned move would need to swing more than two pawns a ply
to matter. One value for six questions is crude, but changing it beside a candidate would mix two
effects in one test. Revisited after the five, each margin by what it asks.

### 1. No razoring, reverse futility or null move at PV nodes

No parameter. A PV node is `beta - alpha > 1`.

### 2. No futility at the root

No parameter. The root's evaluation is then read by nothing, so it is no longer computed there.

### 3. Reverse futility only without a quiet TT move

No parameter. Prunes when the node has no TT move, or its TT move is a capture (en passant
included) or a promotion: Stockfish's `!ttData.move || ttCapture`, its capture stage counting a
queen promotion.

### 4. A correction history

**What it stores:** per key, a running average of the search's error, `score - static_eval`, in the
evaluator's units. Two keys: the pawn structure (both sides' pawns), and each side's non-pawn pieces
(two tables, white's and black's), each indexed by a hash of its pieces modulo the table's size and
by the side to move. Continuation keys left out: one kind of key at a time, each understood.

**When it learns:** at a node's end, as Stockfish: not in check; the best move not a capture (a
capture's score is the exchange, not the evaluation's error); and the error's sign agreeing with
the bound. A fail high only proves the score at least beta, so it may raise the average, never
lower it; a fail low the reverse; an exact score either way.

**How much a sample counts:** `entry += (error - entry) * min(depth, 8) / 16`. A sample from depth
8 or more moves the average halfway to itself, as much as everything learnt before; one from depth
1, a sixteenth. A deeper search is a better witness; 8 is a round cap, and a depth 9 search's
nodes rarely sit deeper than that below them. The error is clamped to two pawns
before it enters: a larger error is a tactic, not the evaluation's bias.

**How much it moves the evaluation:** the plain average of the three tables' entries, clamped to
one pawn: enough to fix a pawn structure or a piece placement the network misjudges, never to
outweigh a piece. No weights between the keys: no evidence for any.

**Where:** every static evaluation the search reads, in the main search and in quiescence's stand
pat, corrected the same way. The TT keeps the uncorrected evaluation, so a table that has learnt
more since corrects it afresh.

**Keys:** the board has none today, only the full Zobrist `hash` (checked 2026-10-31). Every
piece's arrival and departure goes through `place_piece` and `remove_piece`, which already update
`hash`, so three more `u64`s kept there (pawns; white's non-pawn pieces; black's), from the same
`PIECE_SQUARE` keys, cost three XORs a change and nothing in `make_move`. Whether the kings belong to
the non-pawn keys: to decide. Stockfish's `nonPawnKey[c]` includes its side's king, its pawn key
holds pawns only, and a third key, `minorPieceKey`, holds both sides' knights and bishops
(`position.cpp`, master, fetched 2026-10-31).

**Size:** 16,384 entries a table and side, fresh per search as the other histories are. The
tables are tiny next to the TT; a pawn structure or a set of pieces repeats often enough within one
search for an entry to learn.

### 5. Blended returns

One parameter, both uses: **the midpoint, `(value + beta) / 2`,** where reverse futility returns
`static_eval` and stand pat returns its evaluation on a cutoff. A cutoff proves only `>= beta`; the
excess over beta is the evaluation's say alone, trusted half. Stockfish's weights (about 57% beta
at stand pat, 65% at reverse futility) are tuned; the midpoint is the one weight with a meaning.
Mate and tablebase scores are returned unblended.

## Results

Each duel: the old search champion, the switched one challenger, both `@default`. Arena prints
every number from the champion's side (`+W -L =D`, S), so a score above 0.5 favours the old search.

- **`pv`, depth 9 (2026-10-31):** g1-r1 `+316 -287 =329` for the old search, S 0.5156, the switch
  about -11 Elo, drawn (curtailed at 932 games); `-quiet` `+345 -325 =254`, S 0.5108, the switch
  about -8 Elo, drawn (curtailed at 924).
- **`pv`, where the cuts fire (2026-10-31):** counted with today's search, depth 9, PeSTO, six
  positions (`static_cuts_at_pv_nodes`). PV nodes are 2.09% of the nodes that reach the cuts (1,197
  of 57,308). There razoring fires 120 times, reverse futility 4, null move 4: 128 cuts, against
  29,968 elsewhere (0.43%). The switch changes about one node in ten on the PV and almost nothing
  else, so draws within noise are what it can show.
- **`pv`, 10+0.1 (2026-10-31):** g1-r1 `+236 -241 =431` for the old search, S 0.4972, the switch
  about +2 Elo, drawn (curtailed at 908 games, 44 min); `-quiet` `+294 -308 =318`, S 0.4924, the
  switch about +5 Elo, drawn (curtailed at 920 games, 45 min).

- **`root`, depth 9 (2026-10-31), `pv` on both sides:** g1-r1 `+292 -290 =324` for the old
  search, S 0.5011, the switch about -1 Elo, drawn (curtailed at 906 games); `-quiet`
  `+325 -317 =270`, S 0.5044, the switch about -3 Elo, drawn (curtailed at 912).
- **`root`, 10+0.1 (2026-10-31):** g1-r1 `+254 -228 =448` for the old search, S 0.5140, the
  switch about -10 Elo, drawn (curtailed at 930 games, 45 min; 1.2 standard errors); `-quiet`
  `+262 -299 =379`, S 0.4803, the switch about +14 Elo, drawn (curtailed at 940; 1.6 standard
  errors the other way). Pooled, about +2 Elo: nothing.
- **`root`, what it changes (2026-10-31):** depth 9, PeSTO, the 47 bench positions with a legal move
  (`root_futility_at_the_root`). Without the switch, classical futility skips 315 root moves, in 9
  positions; the switch changes no best move, one score, and the nodes by +0.01%. So the four duels'
  leans, the +24 below included, are noise: the two players nearly always play the same move.
- **`ttquiet`, depth 9 (2026-10-31), `pv` and `root` on both sides:** g1-r1 `+319 -298 =309` for
  the old search, S 0.5113, the switch about -8 Elo, drawn (curtailed at 926 games); `-quiet`
  `+338 -339 =229`, S 0.4994, the switch about 0 Elo, drawn (curtailed at 906).
- **`ttquiet`, 10+0.1 (2026-10-31):** g1-r1 `+243 -237 =430` for the old search, S 0.5033, the
  switch about -2 Elo, drawn (curtailed at 910 games, 44 min); `-quiet` `+297 -276 =351`, S
  0.5114, the switch about -8 Elo, drawn (curtailed at 924, 45 min). Pooled, about -5 Elo: nothing.
- **`corr`, depth 9 (2026-10-31), `pv`, `root` and `ttquiet` on both sides:** g1-r1
  `+313 -286 =331` for the old search, S 0.5145, the switch about -10 Elo, drawn (curtailed at 930
  games); `-quiet` `+347 -337 =230`, S 0.5055, the switch about -4 Elo, drawn (curtailed at 914).
- **`corr`, what it changes (2026-10-31):** depth 9, PeSTO, the 47 bench positions
  (`correction_history_at_work`). Unlike the first three, a real change: 19 best moves of 47 and 43
  scores differ, and the nodes fall 4.65%. The corrections are small on average, 4.7 cp over 2.36
  million, 0.46% at the one-pawn clamp: most changed moves are between near-equal ones. At a fixed
  depth the fewer nodes cost it a little; at 10+0.1 they are time saved, so the clocked duels judge
  it fairly.
- **`corr`, 10+0.1 (2026-10-31):** g1-r1 `+242 -291 =419` for the old search, S 0.4743, the
  switch about +18 Elo, drawn (curtailed at 952 games, 46 min; 2.1 standard errors); `-quiet`
  `+221 -260 =303`, S 0.4751, the switch about +17 Elo, drawn (curtailed at 392 pairs, 784 games,
  39 min; 2.2 standard errors), the first duel on the per-pair rule (duel-per-pair.md), its
  `sigma_hat` 0.222. Pooled, about +17 Elo at about 3 standard errors: the first candidate with a
  gain on the clock with both networks, and the fewer nodes explain why depth 9 missed it.
- **`blend`, what it changes (2026-10-31):** depth 9, PeSTO, the 47 bench positions
  (`blended_returns_at_work`), against `pv`, `root`, `ttquiet` and `corr`. Reverse futility cut
  116,421 times, in all 47; the switch changes 18 best moves and 46 scores, and the nodes rise
  11.28%: a lower fail-high score cuts less above it. Its duels must pay for those nodes.
- **`blend`, depth 9 (2026-10-31), `pv`, `root`, `ttquiet` and `corr` on both sides, the per-pair
  rule:** g1-r1 `+64 -98 =82` for the old search, S 0.4303, `sigma_hat` 0.248, the switch about +49
  Elo, **won** at 122 pairs (244 games); `-quiet` `+82 -123 =67`, S 0.4246, `sigma_hat` 0.284, about
  +53 Elo, **won** at 136 pairs (272 games). The first winners of the search work; at a fixed depth
  the 11% more nodes are free, so the clocked duels decide.
- **`blend`, 10+0.1 (2026-10-31):** g1-r1 `+202 -201 =337` for the old search, S 0.5007,
  `sigma_hat` 0.219, the switch about 0 Elo (+/-8 at one standard error), drawn (curtailed at 370
  pairs, 740 games, 36 min); about -18 Elo at 160 pairs, noise. `-quiet` `+222 -234 =296`, S
  0.4920, `sigma_hat` about 0.223 (from its bounds), the switch about +6 Elo (+/-8), drawn
  (curtailed at 376 pairs, 752 games, 37 min). Pooled, about +3 Elo: flat. Depth 9's +49 is gone on the clock:
  its better scores and its extra nodes cancel at 10+0.1.
- **`blend`'s nodes by depth (2026-10-31),** `blended_returns_at_work` at depths 9 to 12, PeSTO:
  +11.28%, +6.07%, +2.08%, +1.15%; best moves changed 18, 16, 14, 22 of 47. The cost shrinks with
  depth, so it does not explain the clock (the guess that it grew was wrong); deeper, `blend`
  changes as many moves at almost no cost and still gains nothing at 10+0.1. Measured with PeSTO,
  not the networks the duels play.
- **`ttquiet`, what it changes (2026-10-31):** depth 9, PeSTO, the 47 bench positions
  (`reverse_futility_with_a_quiet_tt_move`). Of 110,452 reverse futility cuts, 682 (0.62%) had a
  quiet TT move, in 44 positions; the switch changes one best move (gxf4 to Nc3 in
  `1r3k2/4q3/...`), seven scores, and the nodes by +0.51%. Rare, but not as rare as `root`'s.

- **The verdicts again, every candidate on (2026-10-31):** `-quiet` against g1-r1, depth 9:
  `+15 -3 =6` for g1-r1, S 0.7500, about -190 Elo for `-quiet` (was about -160), **lost** at 12
  pairs (24 games). So few pairs size the gap only roughly (one standard error about 0.08 in S),
  but the search changes did not close it. 10+0.1: `+20 -5 =15` for g1-r1, S 0.6875, about -137
  Elo for `-quiet` (was about -108), **lost** at 20 pairs (40 games, 2 min; one standard error
  about 0.06 in S). Both verdicts stand.
- **`-quiet`'s generation 2 again (2026-10-31), every candidate on:** generation 1
  (`-quiet-g1-r1`) against its failed candidate (`-quiet-g2-a1-l0.30-r1`), 10+0.1: `+34 -15 =23`
  for generation 1, S 0.6319, about -94 Elo for generation 2, **lost** at 36 pairs (72 games, one
  standard error about 0.045 in S). The gate was 48.5%, about -10: the new search widens the gap.

### Whether a change helps `-quiet` more

The hypothesis predicts that a change helps `-quiet`'s network more than g1-r1's (the developer's
observation, 2026-10-31). Per candidate, at 10+0.1, the switch's Elo with each network and their
difference. One duel carries about ±12 Elo at one standard error, a difference about ±17, so no
single row says much; pooled at the end, a sign test across the five (five of five the same way, 3%
by chance) does.

| Candidate | g1-r1 | `-quiet` | `-quiet` minus g1-r1 |
|---|---|---|---|
| `pv` | +2 | +5 | +3 |
| `root` | -10 | +14 | +24 |
| `ttquiet` | -2 | -8 | -6 |
| `corr` | +18 | +17 | -1 |
| `blend` | 0 | +6 | +6 |

Three of five positive (+3, +24, +6), two negative (-6, -1): no sign the changes help `-quiet`
more; a sign test needs five of five the same way.

## Decided

- **`pv`, `root` and `ttquiet` reverted (the developer, 2026-10-31, after the quiet filter was found
  to hurt):** kept below on draws at first, then judged shaky, every duel drawn and no gain shown.
  `DEFAULT` keeps `corr` and `blend` alone; the three stay as switches, off.
- **`pv` kept (the developer, 2026-10-31):** four draws, and it does not hurt and is safer. On in
  `DEFAULT`, so `@default` plays it from candidate 2 on and `+pv` is refused as on already.
- **`root` kept (the developer, 2026-10-31):** four draws pooling to about +2 Elo, and at depth 9
  no best move changed on the bench positions; kept as Stockfish's rule at no cost, on in `DEFAULT`
  from candidate 3 on.
- **`ttquiet` kept (the developer, 2026-10-31):** four draws (-8, 0, -2, -8), one best move in 47
  changed at depth 9; kept as Stockfish's rule at 0.5% nodes, on in `DEFAULT` from candidate 4 on.
- **`corr` kept (the developer, 2026-10-31):** four draws, but the two clocked ones agree at +18
  and +17 Elo (pooled about 3 standard errors), and 4.65% fewer nodes; on in `DEFAULT` from
  candidate 5 on.
- **`blend` kept (the developer, 2026-10-31):** won both depth-9 duels (+49, +53), drew both
  clocked ones (0, +6): flat, so on in `DEFAULT` at its midpoint, then tuned as two weights by
  SPSA (below). Every candidate is now on; none is left to try as a switch.

- **Every candidate tested, 1 to 5, in that order (the developer, 2026-10-31).** Each against
  today's search, alone; one kept changes the base the next is tested against.
- **Untuned: every network `@default` (the developer, 2026-10-31),** no SPSA parameters on either
  side, as the duels that judged `-quiet` were. A tune fits the margins to today's search, so a
  tuned side would favour the old search over the change.

- **Both networks (the developer, 2026-10-31):** each change tested with
  `lc0-psq-threats-screlu-g1-r1` and with `-quiet`'s generation 1 winner, each against itself with
  the old search. If the hypothesis holds, a change helps `-quiet`'s network more.
- **The verdicts played again once the search settles (the developer, 2026-10-31):** the search
  changes may change, if not the outcome, at least the margins of `-quiet`'s duels against g1-r1
  (about -108 and -160 Elo) and of `-quiet-r1`'s failed generation 2 gate (48.5%, about -10 Elo),
  all played with today's search. Both played again with the new one, before step 3.

- **Both depth 9 and 10+0.1 (the developer, 2026-10-31)** for every test, as the duels.

- **Judged by duels (the developer, 2026-10-31),** arena's symmetric test with no Elo margin; not
  an SPRT, whose `elo_d` would have to be guessed (the gate's 35 is far too large for one search
  change). Won: the change is kept. Lost: dropped. **Drawn: decided case by case** by the developer.
  From `corr`'s `-quiet` clocked duel on, the per-pair rule (duel-per-pair.md): at most 400 pairs
  (800 games), the bound `0.5 + 3.09 * sigma_hat / sqrt(pairs)` with `sigma_hat` measured from the
  duel's own pairs. A drawn duel says no gap above about 24 Elo at `sigma_hat` 0.22 (the clocked
  duels so far), about 30 at 0.28 (depth 9's), not no gain. The duels before it ran on the old rule:
  at most 1,000 games, the bound `0.5 + 3.09 * 0.5 / sqrt(games)`, about 34 Elo at the cap.
- **10+0.1 decides; depth 9 is a diagnostic.** At a fixed depth a change that prunes less looks
  stronger by searching more nodes (candidates 1 to 3 all do), so depth 9's duel is reported with
  each side's node counts, and the change is judged by 10+0.1, which charges it for its time.

## To decide

- The machine (idle, every side on the same one).

## Follow-up: `blend` tuned by SPSA (the developer, 2026-10-31)

**`blend` kept either way, as tunable weights (the developer, 2026-10-31):** off is a value in
their range (`w = 0`), so SPSA can take it back if its nodes cost more than its scores give. The
clocked duels, untuned as every candidate's, set only the default `@default` plays before a tune:
512 if flat or better, 0 if clearly worse. SPSA starts at 512 either way, so it can move both ways.

- **The switch becomes two weights,** run-time `SearchParameters` like the margins: a cutoff returns
  `(w * beta + (1024 - w) * value) / 1024`, `w` out of 1024, one at reverse futility and one at stand
  pat. `w = 0` is the switch off, 512 its midpoint. Two, not one: Stockfish's tuned weights differ
  (661 at reverse futility, 583 at stand pat, master's `search.cpp` fetched 2026-10-31), and a
  second parameter costs SPSA almost nothing.
- **Started at 512 both,** what the duels judge; Stockfish's values are a reference, not a start,
  as every default here. At 512 the search is the kept switch's, which `FIXED_SEARCH_EXPECTED`
  confirms.
- **Tuned with the margins, in spsa.md's run** (10+0.1, per network): the weight at reverse
  futility interacts with its margin, so both move together. The clock prices the nodes a weight
  toward beta costs (11% at 512, at depth 9), which a fixed depth cannot.
- **Tuned at STC only (the developer, 2026-10-31):** "we do SPSA at STC, and hope for the best".
  A weight tuned at 10+0.1 may not be the best at CCRL Blitz (2+1), where less pruning usually
  ages well but `blend`'s node cost may grow with depth; no longer check, which would take days.
- **To settle with spsa.md:** the range (perhaps 0 to 896, never 1024, which would drop the
  evaluation) and `c_end` (perhaps 64, by spsa.md's rule of a few Elo between the two sides).
- **Stockfish's two other blends,** a fail-high at the end of the search toward beta by
  `1 / (depth + 1)` and at the end of quiescence (about 55% beta), are candidates of their own, the
  first a switch with no free weight.
