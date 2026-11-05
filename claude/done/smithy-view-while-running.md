# Smithy: view a run while another job runs

**Status:** designed and planned 2026-10-26; steps 1 to 3 written the same day, step 4 (the check
by hand) to do.

## Why

While any job runs, the page hides every other run's View (`runBlock`: "While a job runs, only its
own run's screen opens"; `syncView`: "none elsewhere, nor while a job runs"). With
lc0-psq-threats-pairwise-hidden training, lc0-psq-threats-screlu's generation 1 cannot be opened to
compare with it, which is exactly when a comparison is wanted (the developer, 2026-10-26: "of course
we need to fix the smithy").

## What the server already does

`/view/<run>/<generation>/<attempt>` (`Status::attempt_view`) reads a closed attempt from its files,
and the running forge's own attempt from memory: it already answers for any run while a job runs.
Reading finished files beside a running job conflicts with nothing. No server change.

## Design

- **A run that is not the running job's** gets View while a job runs, as when none does; its screen
  opens the view on its latest generation's latest attempt, with the same generation and attempt
  choices.
- **The running job's run** keeps Open and its job screen, unchanged. Viewing its own earlier
  generations while it runs is left out: its screen is the live job's.
- **The live bars** (`section.live`) are the running job's: on another run's screen they would read as
  that run's, so on a run screen they show only on the job's own (`jobHere`).
- New run and Resume stay disabled while a job runs: one job at a time is unchanged.

## Plan

1. `page.html`, `runBlock`: View for any run with a generation that is not the job's own, running or
   not.
2. `page.html`, `syncView`: the view wanted on a run's screen unless that run is the running job's
   (`jobHere`), its comments saying so.
3. `page.html`, `pollLive`: on a run screen, the bars only when `jobHere()`.
4. The developer checks by hand, with the current training running: View on
   lc0-psq-threats-screlu-r1, generation 1 opens with its table; the running run's Open still shows
   the live job; another run's screen shows no live bars.
