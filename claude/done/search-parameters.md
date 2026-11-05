# Search parameters: the ten as run-time values

Implementation plan, 2026-10-18; implemented the same day, `cargo test -p rotor --release` passing
(`FIXED_SEARCH_EXPECTED` unchanged). The every-parameter test searches to depth 9: null move's eval
term needs 6 plies left to have room under `remaining - 1`, and at depth 7 it never acted. **Done:**
the depth-9 node count on the 49 bench positions, hand-crafted, is 3,139,294 both before and after
the change (the training machine, 2026-10-18, `nnue_bench --search-only --hand-crafted` on `3c62ef9` and on its
parent's `search.rs`). `margin_per_ply_for`
became `default_parameters_for(evaluator)`: a requested margin now enters through
`SearchParameters::with_margin` in `choose_best_move_with_margin`. The four margins compared with a
static evaluation at every node are kept converted in `EvalMargins`. The first part of spsa.md's
"code changes it needs": the 10 tuned constants become run-time parameters of a search, today's
values the defaults, so that one process can play two parameter sets. **No change to the search's
behaviour:** every player searches exactly as today until something sets other values. SPSA itself
(the tuning loop, early-stop.md's adjudication, the parameters file) is planned separately and
built in parallel, while the network experiments run the search as it is.

## What today's search does with the ten

| # | Parameter | Today | Where |
|---|---|---|---|
| 1 | Razoring margin, per ply | the player's margin | `Search::eval_margin_per_ply`, razoring |
| 2 | Reverse futility margin, per ply | the player's margin | the same field, reverse futility |
| 3 | Classical futility margin, per ply | the player's margin | the same field, futility |
| 4 | Null move eval margin, per extra ply | the player's margin | `null_move_plies` |
| 5 | Delta pruning margin | the player's margin | quiescence, `self.margin_per_ply` |
| 6 | SEE pruning, main search, per ply | `MARGIN_PER_PLY`, always | `see_prunes` (line 717), a constant |
| 7 | `SEE_MARGIN` | 50 | quiescence's SEE gate |
| 8 | `ASPIRATION_INITIAL_DELTA` | 20 | `initial_aspiration_delta` |
| 9 | `HISTORY_PRUNING_MULTIPLIER` | -35 | history pruning |
| 10 | `GOOD_QUIET_THRESHOLD` | -128 | `MovePicker::new` |

"The player's margin" is `margin_per_ply_for`: an arena player's `@<cp>`, else the network's
`margin.txt`, else `MARGIN_PER_PLY` (200). **Parameter 6 never follows it today**, so a tuned
network's SEE pruning stays at 200: the defaults must keep that, or every tuned network's search
changes.

## Design

### `SearchParameters`

A plain `Copy` struct in `search.rs`, one `i32` field per parameter, named after its use
(`razoring_margin`, `reverse_futility_margin`, `futility_margin`, `null_move_margin`,
`delta_margin`, `see_prune_margin`, `see_margin`, `aspiration_delta`, `history_pruning_multiplier`,
`good_quiet_threshold`), each doc comment carrying the reason its constant has today, moved from
the constant.

- `SearchParameters::DEFAULT`: today's constants, 1 to 6 at `MARGIN_PER_PLY`.
- `SearchParameters::with_margin(margin_per_ply)`: `DEFAULT` with 1 to 5 at `margin_per_ply`, 6
  left at `MARGIN_PER_PLY`: today's behaviour for a player with its own margin.
- The constants `SEE_MARGIN`, `ASPIRATION_INITIAL_DELTA`, `HISTORY_PRUNING_MULTIPLIER` and
  `GOOD_QUIET_THRESHOLD` go, their values living in `DEFAULT` only, so there is one source of truth.
  `MARGIN_PER_PLY` stays: it is `margin.txt`'s default, read by `arena`, `uci.rs` and the margin
  search.

### `Search`

`margin_per_ply` and `eval_margin_per_ply` give way to `parameters: SearchParameters` and the
evaluator-pawn copies of the five evaluation margins (1 to 5), computed once in
`set_parameters` as `set_margin_per_ply` computes its one today (`in_evaluator_pawns`). Parameters 6
and 7 compare material (SEE), so they stay in `piece_value`'s pawn and are not converted, as today.
Every use in the table reads its own field. Each `Search { .. }` literal in the tests takes
`SearchParameters::DEFAULT`.

### The entry points

- `choose_best_move_with_parameters(.., parameters: Option<&SearchParameters>, ..)`: the new
  general form; `None` means today's choice, `with_margin(margin_per_ply_for(None, evaluator))`.
- `choose_best_move_with_margin` stays, as a thin call of it with `with_margin(margin)` when a margin
  is given, so `arena`'s in-process players, `@<cp>` and `arena margin` are untouched.
- `choose_best_move_with_progress` and the multipv path take the parameters the same way.
- The helper threads of a Lazy SMP search get the same parameters as the main one. The aspiration
  delta of thread `i` stays `aspiration_delta + ASPIRATION_THREAD_STEP * i`, the step not tuned.

### Unchanged

`uci.rs`, `arena`, `margin.txt`, `smithy` and every config: nothing outside `search.rs` needs to know
the parameters yet. SPSA's plan adds the parameters file beside the network and the arena players
built from a parameter vector.

## Tests

- **`FIXED_SEARCH_EXPECTED` unchanged:** every fixed search gives the same nodes, score and move at
  the defaults. The test that proves the refactor changed nothing.
- `choose_best_move_with_margin` at `MARGIN_PER_PLY` equals `choose_best_move`, and at another
  margin differs, as the existing test around line 6600 checks today.
- `with_margin(150)`: 1 to 5 at 150, 6 at `MARGIN_PER_PLY`, 7 to 10 at their defaults.
- `choose_best_move_with_parameters` with `DEFAULT` equals `choose_best_move`; with each of the ten
  moved alone, the node count of a fixed search moves too, so every field is really read where it
  belongs (a parameter wired to the wrong use, or to none, fails).
- The existing tests that name the removed constants read `SearchParameters::DEFAULT`'s fields.
- `cargo test -p rotor --release`, then `nnue_bench --search-only` at depth 9 against the node
  count before the change: identical. Any machine, busy or not: only the node count is compared.

## Steps

1. `SearchParameters`, `DEFAULT`, `with_margin`; the four constants folded into `DEFAULT`.
2. `Search` reads its parameters; `set_parameters` replaces `set_margin_per_ply`.
3. `choose_best_move_with_parameters`; `with_margin` and `with_progress` through it.
4. The tests above; `FIXED_SEARCH_EXPECTED` and the depth-9 node count as the proof.
