# Search defaults, and what the forge searches with

**Status:** open, 2026-11-08. Two questions, no decision yet. Prompted by the lc0 network's first
real tune (`lc0-psq-threats-screlu-g1-r1`, 16,384 pairs at 10+0.1) and by an outside proposal
(claude_context_fattorini_spsa_tuning_search_defaults.md, archived), whose table of new defaults
misread two parameters but whose point stands: the flat default is not reasoned enough.

## Where things stand

`SearchParameters::DEFAULT` (`search.rs`) holds ten values. Six margins share
`MARGIN_PER_PLY`, two pawns, one value for six mechanisms: picked once for all of them, never
measured for strength. Margins are in `piece_value`'s pawn, converted to the evaluator's own pawn
where they meet a static evaluation (`EvalMargins`), so a network's score scale does not move them.

| Parameter | Default | What it is, in this engine |
|---|---|---|
| `razoring_margin` | 200 | per ply: evaluation this far below alpha goes to quiescence |
| `reverse_futility_margin` | 200 | per ply: static evaluation this far above beta needs no search |
| `futility_margin` | 200 | per ply: a quiet that cannot plausibly lift the evaluation this far, to alpha, is skipped |
| `null_move_margin` | 200 | not a gate: each multiple of it by which the evaluation clears beta buys the null search one more ply of reduction (`null_move_plies`) |
| `delta_margin` | 200 | quiescence: a capture whose victim plus this cannot reach alpha is skipped |
| `see_prune_margin` | 200 | per ply: a move losing more than `margin x remaining` by SEE is skipped; must stay positive |
| `see_margin` | 50 | quiescence: a capture may lose this much by SEE and still be searched (Stockfish: 74) |
| `aspiration_delta` | 20 | the first window's half-width (Stockfish 10's) |
| `history_pruning_multiplier` | -35 | history pruning's threshold, in bonuses of `remaining^2` (Stockfish's order) |
| `good_quiet_threshold` | -128 | move picker: quiets at or below it go after the losing captures (duelled against 0) |

The tune's end values (16,384 pairs, ended 2026-11-08), beside where it stood at 63%:

| Parameter | Default | at 63% | End | Still moving at the end |
|---|---|---|---|---|
| `reverse_futility_margin` | 200 | 127 | 95.7 | yes, down |
| `razoring_margin` | 200 | 153 | 141.2 | yes, down, slowing |
| `see_prune_margin` | 200 | 162 | 152.6 | yes, down, slowing |
| `aspiration_delta` | 20 | 9.4 | 7.2 | yes, down |
| `delta_margin` | 200 | 221.5 | 215.7 | turned back |
| `good_quiet_threshold` | -128 | -142 | -139.9 | settled |
| `futility_margin` | 200 | 191.6 | 189.8 | near its default |
| `null_move_margin` | 200 | 202.9 | 204.0 | near its default |
| `see_margin` | 50 | 49.1 | 49.8 | near its default |
| `history_pruning_multiplier` | -35 | -37.8 | -37.6 | near its default |

The duel of `@tuned` against `@untuned` (test 3, 2026-11-08, clock 10+0.1): won by the tune, 27 to
13 over 40 games (67.5%), about +130 Elo, the duel stopping at its first threshold (67.3%) after
20 pairs, so the direction is sure and the size rough. The defaults leave much on the table on a
clock, and the parameters still moving at the end say a longer tune may find more.

## New defaults, agreed 2026-11-08, being validated

| Parameter | Old | New |
|---|---|---|
| `reverse_futility_margin` | 200 | 100 |
| `razoring_margin` | 200 | 150 |
| `see_prune_margin` | 200 | 150 |
| `aspiration_delta` | 20 | 10 |
| `good_quiet_threshold` | -128 | -140 |

The other five unchanged. First chosen by following the tune where it moved clearly, rounded, short
of its end where it was still falling; then argued one by one, independently of the evaluator, which
gave the same values (question 1).

**Validated before any code changes**, as `@tuned` (new) against `@untuned` (old), by a
`parameters.txt` holding the new values. The validation duels in the test log are named `@tuned`.
**Temporary, undone 2026-11-08** (PeSTO's file deleted, the lc0 tune's values restored; its copy
`parameters.tune-16k.txt` kept):

- `data/spsa/pesto/parameters.txt` (local), written for this: PeSTO had no tune. To delete.
- The lc0 network's `gen-1/lambda-polyline/parameters.txt` on the remote: its tune result moved
  aside to `parameters.tune-16k.txt` beside it (also kept as `gen-1/tune/parameters.txt`), the new
  values in its place. To restore.

Duels, each at 10+0.1 and at depth 9: PeSTO and the lc0 network (the arena's command line, first the
old defaults, then the new; `+W -L =D` the old defaults' side).

| Player | Search | Games | Old - new | New's share | Old `+W -L =D` | Verdict |
|---|---|---|---|---|---|---|
| PeSTO | 10+0.1 | 694 | 318.5 - 375.5 | 54.1%, about +29 Elo | +166 -223 =305 | new stronger (2026-11-08) |
| PeSTO | depth 9 | 738 | 372.0 - 366.0 | 49.6%, about -3 Elo | +224 -218 =296 | drawn, curtailed at 369 pairs (2026-11-08) |
| lc0 | 10+0.1 | 148 | 62.5 - 85.5 | 57.8%, about +54 Elo | +32 -55 =61 | new stronger (2026-11-08) |
| lc0 | depth 9 | 740 | 364.0 - 376.0 | 50.8%, about +6 Elo | +246 -258 =236 | drawn, curtailed at 370 pairs (2026-11-08) |

PeSTO's on a clock passes more than its "no clear harm": values inspired by the lc0 network's tune,
then argued independently of the evaluator, help a far weaker evaluator too. At depth 9, even: the
tighter pruning costs PeSTO no measurable quality at the forge's search, question 2's first row if
it also searches fewer nodes.

**The validation passed (2026-11-08):** the new defaults are stronger on a clock for both evaluators
(PeSTO +29, lc0 +54 Elo, the latter about 40% of its full tune's +130), and even at depth 9 for both.
The same pattern twice: no quality lost at a fixed depth, a gain on a clock, as fewer nodes for the
same depth would give; to confirm by the speed measurement (question 2).

Then, if they pass: `SearchParameters::DEFAULT` and `spsa::SPECS` changed together, `DEFAULT` made the
one source of the starting values, the ranges re-centred (`aspiration_delta`'s floor of 5 was near
the tune's 7).

## Question 1: reasoned defaults (answered 2026-11-08)

**The principle (the developer, 2026-11-08):** each default is argued from what its mechanism means
in chess, independently of the evaluator. The units allow it: the margins are in pawns, converted
to each evaluator's own pawn (`EvalMargins`). What suits one evaluator better is its own tune's to
find (PeSTO's in `data/spsa/pesto/`, a network's beside it), not a default's. Hence the PeSTO duels
check only that the new defaults do no clear harm: values inspired by the lc0 network's tune need
not suit PeSTO, whose evaluation is far less accurate.

| Parameter | What it asks, in chess terms | Default | lc0's tune |
|---|---|---|---|
| `reverse_futility_margin` | how much the opponent can win back per ply without a tactic: a positional swing of about a pawn | 100 | 96 |
| `futility_margin` | how much a quiet move can gain per ply: the same pawn, but skipping a quiet wrongly can lose the best move outright, while a wrong reverse-futility cut only misjudges a node's score, so the safer, wider side | 200 | 190 |
| `razoring_margin` | how far below alpha a quiet move can still lift a node per ply, before trusting quiescence, which sees captures alone: a pawn, and half a pawn for that blindness | 150 | 141 |
| `delta_margin` | what a capture can bring beyond its victim: the classic two pawns, a promotion threat included | 200 | 216 |
| `see_prune_margin` | how much material a move may give up per ply for compensation the search could still find: a pawn and a half | 150 | 153 |
| `see_margin` | how much a capture SEE calls losing may lose and still be right: half a pawn (Stockfish lets through 74) | 50 | 50 |
| `null_move_margin` | the surplus over beta that buys one more ply of reduction: two pawns a ply | 200 | 204 |
| `aspiration_delta` | how far a score moves from one depth to the next: a property of the evaluator, so no default is independent of it; a middle value, each player's tune doing the rest | 10 | 7 |
| `history_pruning_multiplier` | in history bonuses, not evaluation: already independent; Stockfish's order | -35 | -38 |
| `good_quiet_threshold` | in history points: already independent; duelled -128 against 0, the tune settled at -140 | -140 | -140 |

The reasoning and the tune agree within a few units on seven of the ten; the reasoning gives the
same values as the table agreed above, now each with its argument.

Not answered here: whether the forge should play these defaults or tuned values (question 2).

## Question 2: what the forge searches with

Today the forge (self-play, round robins, gates) plays `FORGE_PARAMETERS`, the defaults, at a
fixed depth, never a tune (the developer, 2026-11-03): a tune maximises Elo on a clock, and each
generation should change the network alone.

At a fixed depth, pruning works differently than on a clock:

- **On a clock,** pruning saves nodes, the saved time buys depth, and a tighter margin can gain
  strength. This is what a tune maximises.
- **At a fixed depth,** pruning saves nodes and buys only throughput (positions/s); each wrong prune
  makes a label less accurate, with no depth gained in return.

So tuned parameters in the forge would likely give faster, noisier data, and wider margins slower,
cleaner data. The trade-off is label quality per unit of compute, which neither "tuned" nor
"untuned" settles and Elo at depth 9 does not measure.

**First evidence, the depth-9 validation duels (the developer, 2026-11-08).** Elo at depth 9 is
the quality of moves, and so far as they go together of scores, at the forge's own search. Read
beside each side's speed at that depth:

| Depth-9 result, new against old | Nodes a position | For the forge |
|---|---|---|
| about even | fewer | clearly better: the same quality, faster; the forge takes the new defaults |
| new loses | fewer | a real trade-off, noisier labels against more positions an hour: this question |
| new wins | either | clearly better |

- **The speed, by `bin/fattorini bench`** (the developer, 2026-11-08): a development build benches
  PeSTO with the built-in defaults, every bench position to depth 9, one thread, a fresh table each;
  its node count is exact for one binary, so the bench before the code change against the bench
  after it is the old defaults' cost against the new ones'.
  - Old defaults (2026-11-08, before the change): **3,684,206 nodes**, 4,730,673 nps.
  - New defaults (2026-11-08, after the change): **2,636,930 nodes**, 4,265,640 nps. New over old:
    **0.716 of the nodes**, 0.90 of the nps, so about **0.79 of the time** to depth 9. The nps fell
    too: machine noise, or the nodes now pruned the cheap ones; the time is what the forge pays.
  - The depth-6 fixed-search test, re-recorded with the change (PeSTO, six positions): 16,896 nodes
    against 24,154, the same move in four of six, but the score moved by 72 and 98 centipawns in two.
    A hint, not a measurement: equal Elo at depth 9 need not mean equal labels, and the forge trains
    on the scores.
  - The lc0 network's would need a snapshot embedding it; a new `@untuned` snapshot replaces the
    older one, which lichess-bot's `config.yml` may name, so PeSTO's alone unless asked.
- **Elo is a proxy:** the forge trains on depth-9 scores, not on the moves. Pruning can shift a
  score without changing the move chosen, so a depth-9 draw does not prove the labels unchanged; Elo
  and score accuracy usually move together, so it is a fair first signal.
- **Expected:** tighter pruning at a fixed depth usually costs some Elo, each prune a risk that buys
  no depth back. The new defaults holding even at depth 9 would be a strong result for the forge.

Directions to weigh, none chosen:

- forge parameters of their own, chosen for data quality per compute, apart from clock parameters
  chosen for Elo;
- a node budget per move instead of a fixed depth, so pruning's savings buy depth in the forge too;
- seeding generation g+1's tune from generation g's result instead of the defaults (the outside
  proposal): fewer pairs spent, but every tune starting from the defaults (DEV.md) lets a run's
  generations be tuned in any order.

## Next

1. ~~The tune ends; its end values and the duel's result go in "Where things stand".~~ Done.
2. ~~Question 1 discussed parameter by parameter.~~ Done.
3. ~~The validation duels end (PeSTO and lc0, 10+0.1 and depth 9); their results go above.~~ Passed.
4. ~~The temporary files undone.~~ Done.
5. ~~A plan, then the code~~ (new-search-defaults.md). Done: `DEFAULT` and `SPECS` held together by
   their existing test rather than one source, the forge pinned to the old values.
6. The new tune of the lc0 network (the developer, 2026-11-08): from the new defaults, double the
   budget, `length: 32768` in its config (about 15 hours). One tune a network: its finished tune's
   `gen-1/tune/` moved aside first, here and on the remote. `parameters.txt` beside the network
   keeps the 16,384-pair tune's values, `@tuned` playing them, until the new tune ends and replaces
   them; `parameters.tune-16k.txt` stays as their copy.
7. Question 2: carried on in tuned-self-play.md (the developer, 2026-11-08: the teacher plays its
   own tune in self-play, since the tune cannot be escaped).
