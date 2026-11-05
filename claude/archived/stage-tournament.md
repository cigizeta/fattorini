# A tournament between the stages

**Archived 2026-11-04: tried, then dropped 2026-10-18. Every stage round robin was monotonic (see
"Result" below).**

Design, then an implementation plan (2026-10-18, below), nothing implemented. **Taken up for
`arena select` (the developer, 2026-10-18):** "worthwhile to modify the current select with this
idea", first on `lc0-psq768` and `lc0-psq`. Proposed by the developer, 2026-10-16, for the Lc0 corpus
(lc0-data.md) first, then for self-play generations (rotor-alone-and-lc0.md) if it pays.

## The idea

Training stays as it is: per lambda, loss patience drives the learning-rate ladder (3e-3, 1e-3,
3e-4, 1e-4, 3e-5) and ends the run. What changes is what the run hands to selection:

- **Each stage leaves one candidate:** its checkpoint with the best test loss within that stage,
  five per lambda.
- **A tournament among a lambda's stage candidates** picks that lambda's network by Elo instead of
  by loss.
- **Then as today:** the round robin over lambdas, then the gate.

This is Elo-based patience applied between runs, not inside one (the way Stockfish's testers pick
a training length from Elo across snapshots): no match runs during training, nothing has to keep
pace with it, and no decision arrives late. Inside a run, loss still decides everything.

## What it tells us

- **Whether each later stage gains Elo or only loss.** If the 3e-5 candidate never beats the
  1e-4 one, later runs drop that stage, saving its rounds outright.
- **Whether best loss and best Elo pick the same checkpoint.** If they keep agreeing, the
  tournament can be retired and loss trusted alone.

## Result on lc0-psq768, and the pre-tournament dropped (2026-10-18)

`select lc0-psq768`, ten lambdas (0.05 to 0.95), five stages each, 400 pairs a pairing at depth 9:

- **Every table monotonic:** at all ten lambdas, stage 5 > 4 > 3 > 2 > 1, stage 5 scoring 64.1% to
  68.8%.
- **Stage 5 against stage 4, directly,** 800 games a lambda: stage 5 ahead at every lambda, by +11
  to +31 Elo; over the 8,000 games, 3109 wins, 2598 losses, 2293 draws, 53.2%, **+22 Elo at 6.8
  standard errors**. The ten gaps scatter no more than each one's own uncertainty (about +/-10
  Elo): one common gap, neither widening nor shrinking with lambda.
- **Loss agrees with games:** at every lambda test loss falls stage by stage, so stage 5 is also
  the lowest-loss candidate, and loss ranked all fifty stages exactly as the games did.

**Decided (the developer, 2026-10-18):** the next select, `lc0-psq`, runs without pre-tournaments
(`stage_round_robin_pairs` left out of its config), each lambda going on with its lowest-loss
candidate, here the same stage the games chose. The code stays, for a run whose lambdas might peak
at an earlier stage. The last learning-rate drop (1e-4 to 3e-5) still paid about 22 Elo; a further
stage at 1e-5 was judged not worth its rounds (the gains shrink by more than half a stage).

## Decided (the developer, 2026-10-16)

- **Every stage is trained, always.** No Elo-driven early stop: loss patience alone ends each
  stage, as today.
- **A round robin, not a chain.** A chain (stage 2 against 1, 3 against 2, ...) is noisier: each
  verdict rests on one match, and an early stage beating a late one goes unseen. Five candidates
  make 10 pairings.
- **Forge and select stay separate.** `smithy forge` trains every stage of every lambda, as
  today; `arena select`, run after it, plays each lambda's stage round robin, then the lambda
  round robin among their winners, then the gate.
- **Format:** depth 9, as the lambda round robin.

Cost: 10 pairings of 400 pairs a lambda, 8,000 games, about 13 min at the training machine's depth-9 rate of
about 10 games/s (dorothy's generation 1 lambda round robin, 12,000 games in about 20 min,
2026-10-16); six lambdas 48,000 games, about 1 h 20, before the lambda round robin's 20 min.

## Later, only if the round robins' data supports it

Two kinds of early stop, kept apart:

1. **A lambda's ladder cut short:** stage k+1 not beating stage k ends the remaining stages.
2. **A losing lambda dropped** (racing, successive halving): lambda 1.00, last in every round
   robin so far, would leave after a stage or two instead of five.

Both decided live, by SPRT with a cap rather than fixed matches, since a run once stopped cannot
come back. Mechanically, lockstep needs no change: an early stop is a patience that ran out
sooner.

## What the implementation needs

- **Keeping each stage's best checkpoint.** `checkpoint_store.rs` prunes `checkpoints/` to the
  few best rounds by test loss over the whole run, so early stages' bests (higher loss by
  construction) are deleted. Each stage's best must survive pruning, e.g. exported at the stage's
  end beside the run's candidate.
- **A selection step** in `arena select` that plays a lambda's stage round robin and records its
  result in the generation's `selection` directory, its winner becoming the lambda's candidate.
- **The config:** a switch for the stage round robin and its pairs, under `selection:`.

## Implementation plan (drafted 2026-10-18, for review)

### A stage, precisely

A round belongs to the stage it trained at: the stage `stages::stage_after` gives **before** that
round is read. The round that runs out of patience therefore belongs to the rate it trained at, not
the next one, and warmup rounds (the first, and those after a switch back to step 0) belong to the
stage they warm up. Step 0, the starting weights, belongs to no stage. A new pure function,
`stages::stage_of_each_round`, gives every round's stage by the same replay; unit tested beside
`stage_after`. Every stage has at least `patience_rounds` rounds, so each has a best.

With rollback at every switch, a stage that never beats the run's best still has a best of its own,
worse in loss than the stage before: still a candidate, since Elo may disagree. The stage holding
the run's best gives the same checkpoint as today's `candidate.nnue`.

### Step 1, smithy: retention keeps every stage's best (before the next forge)

`checkpoint_store::prune_checkpoints` keeps the `BEST_CHECKPOINTS_KEPT` lowest losses as now, **plus**
the lowest-loss round of each stage (from `stage_of_each_round`), never deleted. At most 5 more files
a lambda. The only step that must land before `smithy forge lc0-psq768`: a run pruned under today's
rule has lost its early stages' bests for good. **Implemented 2026-10-18:** `stages::stage_of_each_round`
and `stage_best_steps` (the replay factored into `Stages::read`, shared with `stage_after`),
`prune_checkpoints`' `protected` steps, computed per round in the training loop.

### Step 2, smithy: export each stage's best

At a lambda's finish (`export_candidate`), beside `candidate.nnue`: `stage-<k>.nnue` and `stage-<k>.txt`
(step, test loss, rate, checkpoint), `k` from 1, for every stage. Also a command,
`smithy stages <network>`, exporting them for a run already finished, so step 2 can follow a forge
started after step 1. **Implemented 2026-10-18:** `export_stages` (called by `finish_training`, and
by `smithy stages`), `generation::stage_network_path` and `stage_record_path` shared with the arena.
No unit test of its own: it is `write_engine_network`, already tested, over `stage_best_steps`,
tested in step 1.

### Step 3, arena: the stage round robin in `select`

- **Config:** `selection.stage_round_robin_pairs` (absent: today's selection, unchanged). 400 for
  `lc0-psq768` and `lc0-psq`.
- **Players:** `<network>-NNNN-<l>-stage<k>`, e.g. `lc0-psq-0001-0.75-stage3`, resolved to
  `lambda-<l>/stage-<k>.nnue`; `parse_network_name` learns the suffix.
- **Per lambda,** a round robin of its stage candidates at the selection's depth, with the existing
  interleaved runner, in `gen-NNNN/selection/stages/lambda-<l>/`; its standings there, its winner in
  `lambda-<l>/stage-winner.txt`. Resumable as the lambda round robin is.
- **Then the lambda round robin** among the stage winners, then the margin search (or, later, SPSA)
  and the gate, as today, on the winning lambda's winning stage.

**Step 3 implemented 2026-10-18:** `config::SelectionSpec::stage_round_robin_pairs` (400 in
`lc0-psq` and `lc0-psq768`); `select_stage` and `stage_candidates`; the round robin factored into
`play_round_robin`, shared by the stage and lambda round robins; `run_challenge`, refusing an
untuned winner (`nnue::read_tuning_beside`). The `-stage<k>` names (the developer, 2026-10-18: `-s<k>`
read as a step) are labels only: `parse_network_name`
does not read them, so a stage candidate cannot yet be named on `arena duel`'s command line.

### `select` split into commands (the developer, 2026-10-18)

The sequence becomes five commands, each run by hand: **`smithy forge`, `arena select`, `arena
margin` or `arena spsa`, `arena challenge`, `smithy promote`**. `arena select` ends at the winner
(the stage round robins, then the lambda round robin); it no longer runs the margin search nor the
gate. `arena challenge <network>` is the gate on its own, against the previous champion at STC.
Whether a tune is SPSA or the margin search is then the developer's choice per network, which the
lc0-psq comparison (spsa-implementation.md) needs.

### Open, for the developer

1. **What carries the winning stage downstream** (lambda round robin, margin, gate, `smithy
   promote`)?
   - (a) The winner's `stage-<k>.nnue` is copied over `candidate.nnue`, and `candidate.txt`
     rewritten to name its stage: nothing downstream changes, `margin.txt` stays beside it. But
     `candidate.nnue` stops meaning "the best loss".
   - (b) `candidate.nnue` stays the best loss, and every reader (the lambda round robin, the margin
     search, the gate, `promote`) follows `stage-winner.txt`. Cleaner, more places touched.
   **Decided (the developer, 2026-10-18): (a).** `select` copies the lambda's winning stage over its
   `candidate.nnue` and records the stage in `candidate.txt`.
2. **The canary 1.00:** **decided (the developer, 2026-10-18): no stage round robin.** It goes to
   the lambda round robin with its best-loss candidate, since it is never promoted; 8,000 games a
   network saved. **Moot the same day:** the experiments' configs no longer train 1.00 at all (the
   developer, 2026-10-18, "an optimistic bet"); the canary code in `select` stays, for the older
   lineages' grids.
3. **`lc0-psq768` and `lc0-psq`:** **decided (the developer, 2026-10-18): forged as soon as step 1
   lands, selected only once step 3 exists,** so both are selected the new way from the start.
