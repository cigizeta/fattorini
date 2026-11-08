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

The tune, at 10,399 of 16,384 pairs (63%): `reverse_futility_margin` 127, `razoring_margin` 153,
`see_prune_margin` 162, `aspiration_delta` 9.4, `delta_margin` 221.5, `good_quiet_threshold`
-142; `futility_margin`, `null_move_margin`, `see_margin` and `history_pruning_multiplier` near
their defaults. To be replaced by its end values, and by the duel of `@tuned` against `@untuned`.

## Question 1: reasoned defaults

A default per mechanism, each argued from what it guards, its unit and what a wrong prune costs
there, instead of one value for six. Sources:

- other engines' values, translated to this engine's definitions (a margin per ply or not, a gate
  or a reduction step, material or evaluation);
- the tune's end values, which say what helps on a clock;
- the outside proposal's table, as a list of candidates only: its `null_move_margin` 15 and
  `see_prune_margin` -35 rest on definitions this engine does not have.

Open: whether the defaults should aim at the clock (then the tune's values are their evidence) or
at the forge (question 2), since they are also what the forge plays.

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

1. The tune ends; its end values and the duel's result go in "Where things stand".
2. Question 1 discussed parameter by parameter.
3. Question 2 discussed, then a plan, if anything changes.
