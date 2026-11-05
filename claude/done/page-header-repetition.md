# The job named once on its own screen

The developer, 2026-10-25, approved the same day. On a test's screen the two players were named three
times: the subtitle ("running test: duel of X against Y"), the "Running now" box, and the card's
title ("duel: champion X against challenger Y"). A run's screen repeated its job the same way: the
subtitle ("no job running; the last, X, ended") and the box ("Last job: X").

## Decisions

1. **The subtitle on the job's own screen (`jobHere()`) says its state alone:** `running`,
   `stopping` or `ended`, then the time it was updated. Every other screen keeps today's subtitle,
   which names a job shown elsewhere.
2. **On a test's own screen the box goes:** its Stop moves into the card, beside its title, which
   names the roles. The box stays on every other screen, where it is the one place saying what
   runs.
3. **On other job screens the box's heading drops the name:** "Running now" and "Ended", its line
   (generation, attempt, phase; how it ended) unchanged but for the ended line's "ended:" prefix,
   which the heading now says. On a config's screen, a job running
   elsewhere keeps "Running now: X" with Open.

## Steps

1. `draw`: the subtitle per decision 1.
2. `drawJob`: hidden on a test's own screen; headings per decision 3; the Stop button built by one
   helper, shared with the test card.
3. `work`: a test's card takes an optional Stop beside its title; `drawLive` passes it while the
   test runs on its own screen and binds it.
4. Check by hand on a dev smithy: a test running and ended, a forge running and ended, a config's
   screen with its job running.
