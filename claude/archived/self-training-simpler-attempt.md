# Simplified self-training

**Archived 2026-11-04: superseded by the forge runs (`smithy server start --forge`); attempts removed
by no-attempts.md; continuous-training.md is the open successor.**

Let generation 0 be AB with PeSTO as evaluation function.

Let the current generation be generation 0.

Run self-play games with the current generation, using AB at search depth 9, for a total of 100 * 1024 * 1024 positions, of which 4 * 1024 * 1024 are test positions.

Fix a grid of lambdas in {0.91, 0.92, 0.93, 0.94, 0.95, 0.96, 0.97, 0.98, 0.99}

Blend the search evaluations with the game result using the grid of lambdas, obtaining 9 training targets.

Train the 9 lambdas, with descending learning rates in {3e-3, 1e-3, 3e-4, 1e-4, 3e-5}, patience of 6 rounds, one round is 4096 steps, one step is a batch of 4096 positions.

At the end of training, run a round-robin tournament among the 9 lambdas, the winner is the challenger.

The challenger plays agains the current teacher. If it loses, we stop. If it wins, it becomes the new champion and the new teacher, and the generation counter is increased by one.

When advancing to the next generation, the set of lambdas is still a grid of 9 lambdas, but centered on the best lambda of the previous generation, with the only constraint that clamping of lambdas is needed if the better lambda was, for instance, 0.97.

## Decisions and remarks (the developer and Claude, 2026-10-21)

**Why this route (the developer).** Today's generations worked up to generation two with the
backpropagating AB(lambda). This is essentially the same, with the direct blend instead and less
data per generation. It replaces the continuous loop (self-training.md), whose the Lichess machine tests ran but
showed the loop's complications: a bootstrap problem, two kinds of patience, a gate too coarse for
students below their teacher.

1. **Generational, not continuous.** Each generation trains to convergence on its own corpus,
   warm-started from the champion. Within one generation the teacher is fixed, so forge's patience
   on the frozen test set, with rollback to the best checkpoint, is sound there, and it stops a run
   that starts memorising (the Lichess machine test showed the training loss falling while the loss on
   unseen games rose).
2. **Generation 0 is PeSTO** (AB at depth 9 with the hand-crafted evaluation): the bootstrap is
   simply generation 1, with no special start.
3. **The corpus:** 100 x 1024 x 1024 positions a generation, 4 x 1024 x 1024 of them the frozen test
   set, so 96 x 1024 x 1024 for training.
4. **The direct blend**, `label = (1 - lambda) * score + lambda * result`.
5. **9 lambdas in steps of 0.01, centred on the previous generation's best**, odd so it has a
   centre (the grid above lists ten, 0.90 to 0.99; nine with 0.99 on top are 0.91 to 0.99).
   Clamping keeps the width and slides the grid to stay in range, so a best of 0.97 gives 0.91 to
   0.99 again. Steps of 0.01 move the grid at most 0.04 a generation: if the best keeps landing on
   an edge, generation after generation, wider steps are the evidence-led fix (the lambda ladder of
   continuous-self-play.md, equal steps in log-odds, is the candidate).
6. **Never 1.00:** with a PeSTO bootstrap, the result alone would throw away the teacher.
7. **A failed gate is not the end (the developer).** The same teacher plays another 96 x 1024 x
   1024 training positions, the test set staying as it is (same teacher), and the generation trains
   again from the champion on the whole, larger corpus, the grid unchanged. This repeats until a
   second patience runs out: consecutive failed gates for one teacher, reset by a promotion. Its
   count is left to empiricism: each attempt's duel score is recorded, so a challenger far below
   50% again and again says more data is not the answer, while one narrowing the gap (40%, 45%,
   48%) says keep adding. This replaces "if it loses, we stop" above.

**Cost per generation**, from earlier figures: about 4 h of self-play on the three machines (more
with a network teacher than with PeSTO), about 2.4 h of forge for the stacked lambdas, about 40
minutes of select, then the gate: roughly a generation a day.

**Fresh code, old code untouched (the developer).** The route gets new command names, reusing
existing code by calling it where appropriate; nothing old is changed, even at the cost of a messier
code base for now. The clean-up comes once this route is proven on psq. **The shape (the developer):
one server, `smithy gen-server <config>`,** running whole generations unattended: play the corpus,
freeze the test set, train the 9 lambdas to convergence, the round robin, the gate, then promote or
retry, and on to the next generation. Every game is played by LAN clients (the continuous loop's
transport, protocol and client, reused as they are): self-play jobs for the corpus, pair jobs for
the round robin and the gate. The corpus lives on the server's disk, in its own directory. **Its
printed log must be beautiful:** a better experience than today's chain of commands is the point.

**Every game at depth 9 (the developer, for simplicity),** the gate included, generation 1's against
PeSTO too: the LAN clients play nothing else. Known bias, accepted: at fixed depth a network's
costlier evaluation is free, so generation 1's gate flatters it against PeSTO; from generation 2 on
both sides share one architecture, and depth 9 measures what changes, the evaluation.

**The gate: SPRT at elo_d 35, as today's (the developer).** Pairs go out as LAN jobs; each result
feeds the test as it arrives, and once it decides, no more are handed out and late results are
ignored. The SPRT lives in arena (`sprt.rs`); the server, in smithy, gets a copy in the new code
rather than a move (old code untouched), deleted at the clean-up. Each attempt's verdict and score
are recorded, which the retry's empirical patience reads. **Search parameters are left out:** every
game plays the default search, with no SPSA tuning per network, fine for now.

**The round robin: 400 pairs a pairing (the developer),** as today's selects: 36 pairings among 9
lambdas, 28,800 games at depth 9, the most points winning. Neighbours 0.01 apart will often score
within luck of each other, so the winner may vary from rerun to rerun; the gate, not the round robin,
decides whether the lineage advances.

**What exists and what is new** (written before the shape above; the commands below are the old
pipeline's, whose parts the server reuses by calling them). Almost everything is today's pipeline: `smithy teacher`, `arena
self`, `smithy freeze`, `smithy forge` (lockstep lambdas, `blend: direct`), `arena select`, `arena
challenge`, `smithy promote`. To check or add before generation 1:

- `self.yaml`'s `train.positions` is global today; each attempt's target (96M, then 192M, ...)
  must be set per lineage.
- Re-centring the grid: by hand in the config each generation, or a small helper; nothing
  automatic is needed at a generation a day.
- A retry after a failed gate: forge must start the generation's lambdas over from the champion on
  the larger corpus, without the failed attempt's checkpoints.
- The continuous loop's code (`smithy loop-server`, `arena loop-client`) stays, unused, as decision
  10 of self-training.md promised: nothing old was broken by it.

**Names after the clean-up (the developer): `smithy forge-server` and `arena forge-client`,** for
today's `smithy gen-server` and `arena loop-client`: the pair says which side a machine is on, and
smithy forges. The old single-run `smithy forge` is retired at the clean-up if the experiments
succeed, and not before: until then it stays as it is. (Superseded 2026-10-21, see
lan-only-servers.md: `smithy server --forge` and `arena client`.)

**One server, one PC (the developer, 2026-10-21):** a run starts and ends on the machine that
serves it; no handover between machines, as no engine project automates one (Stockfish trains on one
machine and distributes testing; Lc0 distributes self-play to one central trainer). A dead GPU loses
the run's training state, not the lineage: every teacher is kept on Drive, so a
new machine starts from the last one. At the clean-up, **the server refuses to train without a CUDA
device unless given `--cpu`**, which development on a CPU machine keeps using. Today's move of
gen-psq-medium from a CPU machine to the GPU one is a manual copy, the exception of development.

**Success, for psq (the developer): going past generation 3,** where the old pipeline stopped. This
route changes what might have held it back (the direct blend, retries with more data after a failed
gate, a grid that follows the winner); if psq still stops at 2 or 3 with all of them in place, its
capacity is the likeliest limit, and HalfK the next step.

## A medium CPU experiment (the developer and Claude, 2026-10-21)

**The question:** not whether generation 1 passes its gate against PeSTO (it is not expected to),
but whether the gap narrows as failed gates add data. Planned on the Lichess machine, from a measured PeSTO
self-play rate of about 3,700 positions/s and about 4 steps/s for the 9 stacked lambdas:

| Setting | Value |
|---|---|
| Corpus per attempt | 16 x 1024 x 1024, about 75 min of play |
| Test set | 512 x 1024 |
| Round | 512 steps (2M positions, 1/8 of the corpus) |
| Shuffle buffer | 1M |
| Warmup | 1 round |
| Patience | 4 rounds |
| Round robin | 100 pairs (7,200 games) |
| `max_failed_gates` | 3 |

About 3 to 3.5 h an attempt, 11 h at most. 16M is about 50 times the Lichess machine test that memorised, so
a gate score that does not climb from 16M to 32M to 48M is a warning for the idea, not for scale.

**Reading it: the gate's length (the developer).** No minimum of gate games is needed: the SPRT
rejects a far weaker challenger fast (about 30 games in the smoke test), and takes longer the
closer the challenger gets, so the number of games to a rejection measures the gap along with the
score. `generations.tsv` records both for each attempt. One SPRT's length is noisy, so only a clear
trend across the attempts counts, say 30, then 60, then 150 games.

**Before it runs, a fix (the developer):** `gen-server`, `loop-server` and `loop-client` wrongly
write project data under the data root (`%LOCALAPPDATA%\rotor`), which is for releases and
third-party downloads only (`data_root.rs` says so). Their runs, the clients' network cache and
`gate-games.pgn` belong under the repository's gitignored `data/` (`data/gen/<name>/`,
`data/loop/cache/`, ...). The stale runs already there (`gen/gen-psq-smoke`, `loop/loop-psq-cpu`,
`loop/loop-psq-quality`, `loop/cache`, `loop/gate-games.pgn`) are deleted by hand.

## Generation 1 from Lc0 labels (the developer and Claude, 2026-10-21)

The PeSTO route above ran end to end in a smoke test. The generalisation (the developer): replace
generation 1's PeSTO labels with Lc0's, and change its grid. Everything from generation 2 on stays as
above.

1. **Generation 1 trains on the Lc0 data, about 1B positions,** with the direct blend of Lc0's score
   and the game result. No play phase: the data is already on disk, so 96M has no reason to apply
   (the developer's intuition is that 100M is too few to learn Lc0's labels). Patience on the test
   set ends training when the gain runs out.
2. **Its test set is 4 x 1024 x 1024 Lc0 positions held back by game, not by position** (or by file),
   never trained on: positions of one game are correlated, and the Lichess machine test showed what leaking
   them does to the test loss.
3. **Its grid centre waits for the lc0-psq-direct select** (whether the direct blend gives a smooth
   curve, and where it peaks).
4. **Its gate is against PeSTO, as a sanity check only:** it should massacre PeSTO, so the SPRT ends
   in a few dozen games. One attempt: a failure is a bug (labels, blend, data loading), so the run
   stops with a clear message instead of retrying with more data.
5. **Lc0 data is used for generation 1 only (the developer): it did its job there.** From generation 2
   on, the winner plays 96 x 1024 x 1024 positions of self-play as teacher, and failed gates add more,
   exactly as above. Nothing guarantees generation 2 beats generation 1: a simple architecture such
   as psq may already sit near its ceiling after 1B Lc0 positions, and pure self-play may forget what
   Lc0 taught. Either shows up as failed gates and a stopped lineage, which is itself the answer for
   that architecture. Mixing Lc0 data into later generations is left out, so a gain can only come from
   self-play; the hope is that richer networks such as HalfK do improve.
