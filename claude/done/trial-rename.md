# "Test" renamed "trial" for engine duels

**Status:** written, 2026-11-18 (the developer: files and directories on disk renamed too); the
module's `git mv` and the data's rename are the developer's.

## Why

What the smithy calls a test (a duel, a match or an SPRT between two players, from the page's
Tests section, with a verdict) is a trial, Italian's "collaudo" (the developer, 2026-11-18). "Test"
stays for held-out data and for `cargo test`, so one word no longer means two things.

## Renamed

- **The module:** `crates/smithy/src/matches.rs` to `trials.rs`; `TESTS_DIR` to `TRIALS_DIR`,
  `data/trials`; `tests.tsv` to `trials.tsv`; `test-NNNN.tsv` and `.pgn` to `trial-NNNN.*`; its
  docs, comments and messages ("a trial is a duel, a match or an sprt").
- **`smithy.rs`:** `Shared::test`, `TestDealing`, `View::Test`, `WorkView::Test` (its `test` field,
  the kind, becomes `trial`), `KeptTest`, `Catalog::tests`, `test_view`, `test_networks`,
  `TestPlayer`, `test_player`, `test_job`, `resume_test_job`, `play_test`, `test_record`; the job
  words `--test` and `--resume-test` to `--trial` and `--resume-trial`; `TESTS_SCREEN` `"tests"` to
  `TRIALS_SCREEN` `"trials"`; log lines and errors ("trial 12: ...").
- **The page:** the Trials section, "New trial", "no trial yet", the form and its functions
  (`trialTitle`, `trialWords`, `trialsScreen`, `trialScreen`, `bindTrialForm`, `pendingTrial`,
  `trialForm`, `TRIALS`), `startTrial`, `data-resume-trial`, `work.kind === "trial"`.
- **Tests' names** that say a duel's "test", for consistency.

## Not renamed

Everything about held-out data: the frozen test set (`TestSet`, `test.freeze`), test loss,
`test_positions`, `test_batch_size`, test rows and seconds, `test/` of the corpus, `test_cache`,
`grouped_test_set`; `cargo test` and `#[test]`; the arena's `SequentialTest`, a statistical test
(the SPRT, best-of), not a duel.

## The data on disk

`data/trials/` is local and gitignored. The developer renames once by hand, the smithy stopped:
`data/tests` to `data/trials`, `tests.tsv` to `trials.tsv`, every `test-NNNN.*` to `trial-NNNN.*`.
The commands are given with the commit.

## Plan

1. `git mv` of `matches.rs` to `trials.rs` (the developer), then the edits.
2. `smithy.rs`, `main.rs`'s `mod`, `page.html`.
3. `cargo test -p smithy`, clippy; the files renamed; the smithy started, the Trials section showing
   the kept trials, one resumed or started.
4. TODO.md's entry removed.
