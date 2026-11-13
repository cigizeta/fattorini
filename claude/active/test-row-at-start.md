# A test's row written at its start

Status: **implemented 2026-11-13** (smithy.rs `play_test`, `test_record`, `catalog`; matches.rs
`INTERRUPTED` and its test). Test 8's line added by hand and test 8 resumed, 2026-11-13. To
`done/` once a test interrupted after this fix is seen listed and resumed. Side effect accepted: while a test runs, its
row in the list already reads `stopped: interrupted` (Resume disabled, live counts), its live block
above saying what is actually happening.

## Why

On 2026-11-13 the power went off while test 8 (Reckless against Classic, 180+2) was running. The
tests page no longer listed it, and it could not be resumed. Its 128 pairs survived in
`test-0008.tsv` and its games in `test-0008.pgn`, but `tests.tsv` had no line for it.

The cause is in `play_test` (smithy.rs): `matches::put` writes a test's line only once the test
ends, whether by a verdict or by Stop. A test that dies without ending (power loss, a crash, a
killed process) exists only in the smithy's memory and in its pairs and games files. The page lists
only `tests.tsv`'s lines, and `resume_test_job` only resumes a test that has a line. `prune` still
counts the unlisted files, so after 20 more tests it would delete them without the test ever having
been visible.

## Design

1. **The line written at the start.** `play_test` calls `matches::put` right after `mark_start`,
   with `verdict = "stopped: interrupted"`, `ended = log_time()`, and the games and points of the
   pairs it starts from (none for a new test, the recorded ones for a resumed test). The line at the
   end replaces it as today (`put` replaces a line by its number).
   - With `stopped`, nothing else changes: `Record::stopped`, `resume_test_job` and the page's
     Resume button (page.html, `t.verdict.startsWith("stopped")`) all accept it. While the test
     runs, the page already disables Resume (`running`), so a live test's row cannot be resumed
     under it.
2. **The row's numbers taken from the pairs file.** An interrupted line's games and points are
   those at its start. `catalog` already reads each kept test's pairs to rebuild its block, so for a
   line whose verdict is `stopped: interrupted` it also overwrites `games` and `challenger_points`
   with `matches::judge(spec.kind, &pairs)`'s, so the list shows what was actually played. The
   file is not rewritten; the line at the end, or the resume's, corrects it.
3. **Tests.** In matches.rs: a record whose verdict is `stopped: interrupted` reports
   `stopped()`, and `put` of a final record over it leaves one line with the final verdict.

## Test 8, by hand, once

The fix covers tests started after it, not test 8. Before resuming test 8, the developer adds its
line to `data/tests/tests.tsv` (tab-separated, like test 7's line, which has the same settings):

```
8	2026-11-13 HH:MM UTC	2026-11-13 HH:MM UTC	duel	-	lc0-psq-threats-screlu-g1-r1@tuned@reckless	lc0-psq-threats-screlu-g1-r1@tuned@classic	clock 180+2	0	0	stopped: interrupted	0
```

The times are whatever is known. `games` and `points` at 0 are corrected by step 2 on the page and
by the resume's final line. Resume then works from the 128 recorded pairs.

## Not done

- No recovery at startup of tests with files but no line (option 2 discussed on 2026-11-13): once
  every test writes its line at the start, only test 8 lacks one, and it is fixed by hand.
