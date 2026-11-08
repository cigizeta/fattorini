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
**Temporary, to undo afterwards:**

- `data/spsa/pesto/parameters.txt` (local), written for this: PeSTO had no tune. To delete.
- The lc0 network's `gen-1/lambda-polyline/parameters.txt` on the remote: its tune result moved
  aside to `parameters.tune-16k.txt` beside it (also kept as `gen-1/tune/parameters.txt`), the new
  values in its place. To restore.

Duels, each at 10+0.1 and at depth 9: PeSTO and the lc0 network. Results: to be added.

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
3. The validation duels end (PeSTO and lc0, 10+0.1 and depth 9); their results go above.
4. The temporary files undone: PeSTO's `parameters.txt` deleted, the lc0 network's tune result
   restored from `parameters.tune-16k.txt`.
5. A plan, then the code: the new values in `SearchParameters::DEFAULT`, which becomes the one
   source of the tune's starting values (`spsa::SPECS` keeps each parameter's range and `c_end`
   alone), the ranges re-centred on the new values, and `aspiration_delta`'s floor of 5 lowered, the
   last tune having ended at 7.
6. The new tune of the lc0 network (the developer, 2026-11-08): from the new defaults, double the
   budget, `length: 32768` in its config (about 15 hours). One tune a network: its finished tune's
   `gen-1/tune/` moved aside first, here and on the remote. `parameters.txt` beside the network
   keeps the 16,384-pair tune's values, `@tuned` playing them, until the new tune ends and replaces
   them; `parameters.tune-16k.txt` stays as their copy.
7. Question 2 discussed, then a plan, if anything changes.
