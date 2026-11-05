# Tests: duels, matches and SPRTs between the smithy's networks

Status: built and tested 2026-10-23, used from the page the same day. Built as below, but:
the duel's rule is copied into the smithy (`matches.rs`), as `sprt.rs` copies arena's SPRT, rather
than moved into a shared module; each test keeps a small `test-NNNN.tsv` of its pairs beside its
PGN, pruned with it; and a candidate is offered from its generation's latest attempt only, the one
its player name (`<run>-NNNN-<lambda>`) reaches.

## What

From the smithy's page, any two of the smithy's own networks (or one and PeSTO) play a **duel**, a
**match** or an **SPRT**, as a job, the arenas playing its pairs. The GUI helps choose and lets every
possible test be run.

## Decisions (the developer, 2026-10-23)

1. **Players: only the smithy's networks.** Every candidate under
   `data/gen/<config>/run-<n>/gen-NNNN/attempt-N/lambda-*/candidate.nnue` on the store, and PeSTO.
   Networks from before the forge clean-up (`lc0-psq-0001-0.15`, `caroline-0001`, ...) are not
   offered.
2. **Search: depth or clock, chosen per test.** Defaults depth 9 (as the round robin and the gate)
   and 10+0.1 (as the earlier duels). Under a clock, each arena scales the clock against the smithy's
   speed with the test's networks (`reference_nps`, `spsa::clock_factor`), as a tune does, and half
   the logical cores play.
3. **Parameters: chosen per side,** each network's own tune (`parameters.txt`) or the defaults. The
   GUI shows which networks have a tune; tuned against untuned is allowed, but marked plainly when
   chosen and in the result, so it is never mistaken for a fair comparison (TODO.md's rule).
4. **The kinds and their settings:**
   - **Duel:** arena's rule as it is (`duel.rs`): a win past `0.5 + 3.09 * 0.5 / sqrt(N)`, drawn at
     1,000 games, curtailed once neither can pass. Nothing to set, so every duel compares with the
     earlier ones.
   - **Match:** a number of pairs, default 500. Its result: the score, an Elo estimate and its 95%
     margin.
   - **SPRT:** elo0 and elo1, defaults 0 and 5; alpha and beta fixed at 0.05. The gate's SPRT, one
     `elo_d`, becomes the general form, the gate passing `(0, elo_d)` (to check against its
     present definition before changing it).
   All are played in colour-reversed pairs from the book's random openings.
5. **A test is a job:** one job at a time, as a forge or a tune; started from the page when no job
   runs, stopped with Stop.
6. **Kept like logs, the latest N = 20 tests,** each with its record and its PGN; older ones deleted,
   record and games together. A stopped test is logged as stopped, with what it played, and
   **resumes** (revised the same day) while it is kept: Resume on its log line, `--resume-test
   <number>`, from its recorded pairs, under its number and start, its line replaced at its end. A
   test cut short without a stop (the smithy killed) has no line, so cannot be resumed.

## Storage

Local to the smithy's machine, not on Drive:

- `data/tests/tests.tsv`: one line a test, newest last: its number, start and end, kind, both
  players, each side's parameters (own or defaults), the limit, the kind's settings, games, points,
  the Elo estimate and margin (a match), the LLR (an SPRT), the verdict (`won by <player>`,
  `drawn`, `curtailed`, `H0`, `H1`, `stopped`), and `mixed` when one side is tuned and the other not.
- `data/tests/test-NNNN.pgn`: its games, appended as pairs arrive.
- When a test starts and more than 20 are logged, the oldest record and its PGN are removed.

## The smithy

- A job `--test <kind> ...` naming both players, their parameters, the limit and the settings, as
  `--tune <player>` names its player.
- It plays as the gate does, one pairing through `play_pairs`, with a `pair` chore (`clock` and
  `reference_nps` under a clock, as `spsa` chores carry them), until its rule decides.
- The duel's rule moves from `arena` to a module both crates use (the arena's `duel` command keeps
  it), so the smithy's duels are decided exactly as arena's were.
- The smithy's `Sprt` gains elo0 and elo1.

## The page

- **Home:** a **Tests** box: the latest test's verdict, and New test.
- **The tests screen:** the logged tests, newest first, each with its players, kind, settings, score
  and verdict, `mixed` marked; New test.
- **New test:** the kind; each side's network, picked from a list grouped by config, run, generation
  and attempt (each candidate by its lambda, the gate's winner marked), PeSTO among them; each side's
  parameters, `own` offered only where the network has a tune; depth or clock with their defaults;
  the kind's settings. Start, disabled while a job runs.
- **A running test:** its live view on its own screen, a gauge for its progress: an SPRT's LLR
  between its bounds (as the gate's), a duel's score against its two thresholds, a match's pairs of
  its total.

## Tests

- The test log keeps the latest 20, removing a record and its PGN together.
- A duel ends at the same games and verdict as arena's (`duel.rs`'s own tests, reused).
- An SPRT with elo0 and elo1 decides as the standard form; the gate unchanged in its verdicts.
- A tuned side's chore carries its parameters in full; an untuned side's `own`.
- The page refuses a test while a job runs.
