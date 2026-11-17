# A fixed budget and a falling schedule

**Status:** decided 2026-11-17 (the developer), design below; implementation plan to follow, no code
yet. Replaces the patience rule of `stages.rs`. The rules shelved in learning-rate-rules.md stay
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

1. **The budget in positions:** `passes: 4` in the config, times the training set's size, read from
   the corpus (not a round count, so a corpus of another size keeps the same meaning), rounded up to
   whole rounds.
2. **The shape: geometric decay** (decided 2026-11-17, the developer: the most intuitive): after the warmup round, from 3e-3 to 3e-5,
   the same range as today's five rates, falling by the same factor every step; about 0.983 a round
   over 270 rounds. Equal time per decade of rate, as today's log-spaced rates intended, and like
   nnue-pytorch's per-epoch factor. The alternatives: cosine (more time at the high rates, a fast
   drop at the end) or bullet's steps (the five rates, each a fixed share of the budget).
3. **The candidate:** the checkpoint with the lowest test loss, as today. With the rate falling to the
   end it is usually one of the last; nothing in the gate, export or duel changes.
4. **Replay:** the rate a function of the step alone, so a resume needs nothing from `history.tsv`:
   simpler than today's replay.
5. **Config:** `learning_rate` becomes a start and an end (`[3.0e-3, 3.0e-5]`), `passes: 4` is new,
   `patience_rounds` goes. `warmup_rounds` stays.
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
