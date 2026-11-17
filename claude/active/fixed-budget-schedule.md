# A fixed budget and a falling schedule

**Status:** decided 2026-11-17 (the developer), design below; implementation plan to follow, no code
yet. Replaces the patience rule of `stages.rs` for new configs. **The old implementations are not
dropped, only deprecated** (the developer, 2026-11-17): patience, the stages and the rollback, and
choosing the candidate by the lowest test loss, stay selectable per config, as the grid did beside
the polyline. The rules shelved in learning-rate-rules.md stay
shelved: this is the standard technique, not one of them.

## The decision (the developer, 2026-11-17)

- **A budget of 4 passes over the training set** ("big epochs"): about 4 x 1.14B positions, about
  270 rounds of 4096 x 4096. Seeing a position about 4 times is enough: repeating data up to about 4
  epochs is nearly as good as fresh data, past it the returns fall quickly (Muennighoff et al.,
  "Scaling Data-Constrained Language Models", 2023, a different domain), and our runs agree (run 2
  trained about 4 passes, its test loss still improving through the fourth).
- **A falling schedule over that budget,** the standard technique (bullet, nnue-pytorch): the rate a
  function of the step alone.
- **Patience dropped altogether,** and with it the rollback and the stages.

## Why

- **The length is now known,** which is what ruled a fixed schedule out before
  (learning-rate-rules.md, idea 2).
- **Patience wastes the tails:** in `lc0-halfk-threats-screlu` run 2, 3e-3 ran 57 rounds, gaining
  about 0.0006 in its last 40, noise renewing its best.
- **A cap alone would not do:** `lc0-halfk-threats-screlu-hidden` at round 270 was still at 1e-4
  (0.61836); its best, 0.61787, came from its 3e-5 stage, reached at round 288. Patience does not
  know a budget exists.
- **Predictable runs:** every network of a comparison trains the same positions at the same rates,
  so a duel compares architectures, not how patience happened to fall; a run's length and cost are
  known at its start.

## Design

1. **The budget in epochs** (the developer: think in epochs): `epochs: 4` in the config, an epoch one
   pass over the training set, its size read from the corpus (not a round count, so a corpus of
   another size keeps the same meaning), rounded up to whole rounds.
2. **The shape: geometric decay** (decided 2026-11-17, the developer: the most intuitive): after the warmup round, from 3e-3 to 3e-5,
   the same range as today's five rates, falling by the same factor every step; about 0.983 a round
   over 270 rounds. Equal time per decade of rate, as today's log-spaced rates intended, and like
   nnue-pytorch's per-epoch factor. The alternatives: cosine (more time at the high rates, a fast
   drop at the end) or bullet's steps (the five rates, each a fixed share of the budget).
3. **The candidate: chosen by games, among checkpoints** (decided 2026-11-17), never by the lowest
   test loss: the minimum picks the luckiest round late in a run (checkpoints differ by about the
   5e-5 jitter), the test set both choosing and scoring it; loss is a coarse proxy for strength; and
   a minimum lets each run stop at a different, noise-chosen round, undoing the budget. The test loss
   is reported, never used to choose. (Briefly decided the same day: the final network; replaced
   on reading how Stockfish chooses.)

   **Stockfish's way** (nnue-pytorch's `run_games.py`, read 2026-11-17 from a mirror, not the
   official file): every saved checkpoint a candidate; each plays a gauntlet against a fixed
   reference, master Stockfish, at 4+0.04, 200 games a round, a 3-move book; ordo rates them, anchored
   to master; each round plays the 3 with the highest `rating + 1.5 * error`, a UCB-style bandit, so
   untried nets are tried early and promising ones keep collecting games; an endless loop beside the
   training, the best read from ordo's ranking. What follows (a Fishtest SPRT) not verified.

   **Ours:**
   - **Replaced the same day by a round robin** (the developer: simpler): the candidates the
     checkpoints at **every quarter epoch from epoch 2 to 4: 2, 2.25, 2.5, ..., 3.75, 4, nine**
     (an epoch one pass over the training set; none before half the budget; every epoch's end among
     them, where every position has been seen equally often). Generous on purpose for the first
     runs (the developer: no reason to be Scrooge at first): the first round robin shows how
     strength grows over the last two epochs, and fewer candidates may follow. A round robin among
     them at **depth 9** (checkpoints of one network search alike, so depth measures what differs,
     the evaluation), about 2,000 games a candidate: 36 pairings of 125 pairs, 9,000 games, about
     15 minutes. The most points win, then the gate and the duels. The existing round robin, so it must survive the grid's removal (TODO.md,
     "Code to clean up"). The bandit below kept as the record of what was weighed.
   - **No reference: the candidates play each other** (the developer, 2026-11-17). An external
     reference, the previous experiment's winner, was proposed and dropped: experiments vary, which
     is the previous one is not always clear, and the very first has none. So the choice within a run
     is self-contained, the same for every run, its first included; comparisons between runs stay
     what they are, the gate and the duels, their opponents chosen by hand. The bandit picks pairs,
     ordo rates them unanchored: the ratings only rank the run's own checkpoints, all the choice
     needs.
   - **The bandit:** UCB-style allocation as Stockfish's, pairs instead of singles, and **a stop**,
     which its loop lacks: a fixed game budget.
   - **The winner: the most-played network,** as MCTS's robust child, not the highest rating: a
     high rating on few games may be luck; the bandit's repeated returns confirm a lead. Then the
     gate and the duels as now.
   - **Open:** which checkpoints are candidates (Stockfish: every one saved; ours to choose, with
     the save period), the game budget, the control (depth 9 or a short clock), and whether the
     games run beside the training or after it.
4. **Replay:** the rate a function of the step alone, so a resume needs nothing from `history.tsv`:
   simpler than today's replay.
5. **Config: the config decides the strategy** (the developer, 2026-11-17), and new configs are
   created for the budget, the old ones left as they are:
   - `schedule: patience`, the default when absent, so every existing config reads as before: the
     rate list, `patience_rounds`, the rollback, the lowest-loss candidate. Deprecated.
   - `schedule: budget`: `epochs: 4`, `learning_rate` a start and an end (`[3.0e-3, 3.0e-5]`),
     the geometric decay, the quarter-epoch candidates from epoch 2 and their round robin;
     `patience_rounds` refused.
   - One switch for both the rate and the candidate, not two: the pairs belong together (a budget
     has no single best round by loss to roll back to; patience has no fixed checkpoints), and four
     combinations would be four paths to keep. `warmup_rounds` common to both.
   - New configs, as `lc0-halfk-threats-screlu-hidden-budget.yaml` and its 512 twin, each a copy of
     its patience config differing by the schedule alone, so each pair compares the schedules on one
     network. The `-budget` suffix confirmed (the developer, 2026-11-17).
6. **The page:** the stage and patience columns go; progress and the ETA read rounds done against the
   budget's rounds.

## Open

- Old runs' `run_config.txt` and histories: the page and resume must still read them, or old runs are
  read-only. To check in the code.
- What `stages.rs` and its tests become, and which other readers of patience exist (the page, the
  ETA, `training-eta.md`).
- `lc0-halfk-threats-screlu-hidden-512` run 1 trains under patience (started 2026-11-17): it finishes
  so, and compares with screlu-hidden at 256, also trained under patience. The first budgeted run is
  the next new network's.
