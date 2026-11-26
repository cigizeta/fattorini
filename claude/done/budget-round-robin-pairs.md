# The budget's round robin sized by the config

**Status:** applied 2026-11-22: steps 1 to 3 written, the old test removed (the pairs are now read
in one place, the config, with nothing of their own to test without playing); step 4 to run.

## The mistake

A budget's round robin among its nine checkpoints plays 125 pairs a pairing (`budget_pairs`, from
`BUDGET_GAMES_A_CANDIDATE` = 2000 games a candidate), and ignores the config's `round_robin_pairs`.
The grid's plays `round_robin_pairs`, 400 in every config: 6,400 games a candidate, 28,800 in all,
against the budget's 9,000. The budget's tournament should cost the same as the grid's (the
developer, 2026-11-22): 2,000 games a candidate measure one candidate to about +-7 to 8 Elo, too
coarse for nine checkpoints a few Elo apart. fixed-budget-schedule.md gave its size and its cost
(about 15 minutes), never why it was smaller.

## The decision (the developer, 2026-11-22)

The pairs are a configuration's setting: the budget's round robin plays `round_robin_pairs` a
pairing, as the grid's does. One setting sizes every round robin, and it survives the grid's removal
(TODO.md, "Code to clean up").

## Plan

1. **`crates/smithy/src/smithy.rs`:**
   - `budget_round_robin` (about line 4120) and the page's view (about line 3453) pass
     `config.round_robin_pairs` where they pass `budget_pairs(..)` today.
   - `budget_pairs` and `BUDGET_GAMES_A_CANDIDATE` removed. A scaled attempt needs nothing more:
     `scale_forge` already shrinks `round_robin_pairs` by `scale::games`, as `budget_pairs` did.
   - The test `a_budget_round_robin_plays_about_two_thousand_games_a_candidate` replaced by one
     that a budget's round robin is sized by `round_robin_pairs`, if the call can be tested without
     playing; otherwise removed, the pairs then read in one place alone.
   - `GenConfig::round_robin_pairs` gets a doc comment: pairs a pairing of every round robin, the
     grid's and the budget's.
2. **The configs' comment** above `round_robin_pairs`, in every budget config (`-hidden-budget`,
   `-hidden-512-budget`, `-hidden-8-budget`, `-hidden-10-budget`, and the two `-512-8-budget`):
   "Pairs a pairing of the round robin, a grid's lambdas or a budget's checkpoints: for nine, 6,400
   games a candidate, 28,800 in all." The other configs' comments checked for the same claim.
3. **fixed-budget-schedule.md:** "about 2,000 games a candidate: 36 pairings of 125 pairs, 9,000
   games, about 15 minutes" becomes "`round_robin_pairs` a pairing, 400 in the configs: 6,400 games a
   candidate, 28,800 games, about 48 minutes", with a line pointing here.
4. **Checks:** `cargo test -p smithy`; then, on the next scaled attempt with `schedule: budget`, the
   round robin's page shows the scaled `round_robin_pairs` as its pairs wanted.

## The attempt now starting

`lc0-halfk-threats-screlu-hidden-512-8-budget` freezes its config, the 400 included, when it starts,
and reaches its round robin only at the end of epoch 4. A smithy rebuilt with this change before then
plays 400 pairs a pairing on resume. If its round robin has already begun under the old build,
whether the new one plays on to 400 pairs from the record already played is to check in
`round_robin` before relying on it.
