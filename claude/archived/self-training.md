# Rotor: Lc0-style self-training for alpha-beta + NNUE

**Archived 2026-11-04: superseded, see self-training-simpler-attempt.md, itself superseded by the
forge runs; continuous-training.md is the open successor.**

## Claude's rewrite of the idea

Lambda in Rotor's convention: the weight of the game result `z`, `1 - lambda` the weight of the
search score (the opposite of the blend formula in "Self-play loop" below). Lambda is not scheduled:
it is found by selection, each promotion telling which of two adjacent lambdas trains the stronger
network.

**The rungs.** Rung 0 is lambda 1.00 (the result alone). From rung 1 (0.99) on, the logit ladder of
continuous-self-play.md with `gamma = 2`: 0.99, 0.98, 0.96, 0.93, 0.86, 0.76, ... Steps of 0.01 match
the ladder near the top but shrink to luck-sized differences further down, so the ladder is used.

**The rungs stay in (0, 1).** Each rung divides lambda's odds by `gamma`, so
`lambda_k = 1 / (1 + 99 * gamma^(k-1))` for `k >= 1`: the logit goes to minus infinity, lambda only
tends to 0, never reaching it nor going negative. The ladder should stop long before: lambda 0 lost to
0.15 on Lc0's labels (lambda-zero.md), and rungs 12, 13, 14 (0.05, 0.02, 0.01) would differ from each
other by less than training luck.

**The loop.**

1. **Start:** the teacher is a random network with a value head only. The two students, on rungs
   0 and 1 (lambda 1.00 and 0.99), both start from the teacher's weights; both CUSUM sums are 0.
2. **Play:** N self-play games with the teacher, AB at depth 9, UHO openings, MultiPV for 6 plies
   after the book, then the search's best move only.
3. **Label:** each position gets two targets from the same games, one per student's lambda.
4. **Train:** each student trains K steps on its own targets, continuing from its own weights.
5. **Match:** each student plays the teacher, 8 game pairs. Two matches per round, never student
   against student.
6. **CUSUM:** each student keeps its own sum `S`, updated from its match as in "CUSUM rule" below.
7. **Promote:** the first student whose `S` exceeds `h` becomes the teacher. If both cross in the
   same round, the larger `S` wins: no tie-break match. If neither crosses, go back to 2 with the
   same teacher; the students keep training.
8. **Move the pair:** the winner keeps its rung, the other student moves to the rung on the far side.
   - The lower rung wins: the pair steps down one rung (rungs 0, 1 become 1, 2).
   - The higher rung wins: the pair steps up one rung (rungs 3, 4 become 2, 3). At the top it cannot
     go higher: rung 0 winning keeps rungs 0, 1.
9. **Restart:** both students take the new teacher's weights, both `S` reset to 0; back to 2.

**Expected trajectory.** Early, the result is the better signal and the higher rung wins, so the pair
stays at the top. As the teacher strengthens, the lower rung starts winning and the pair descends.
Near the best lambda, wins alternate between the two rungs and the pair oscillates around it. Lc0's
labels suggest that point lies near rungs 9 to 10 (0.28, 0.16) for a very strong teacher.

**Caveats.**

- **Two arms double the false promotions.** The first of two CUSUMs to cross roughly halves
  ARL0, so `h` is calibrated by simulating two students against the teacher, not one.
- **Lambda's slowdown is confounded.** Lambda moves only at promotions, and promotions become rarer as
  gains shrink. A slower descent then shows nothing by itself: record which rung wins each promotion.
  Lower rungs still winning, rarely: lambda is still falling. Rungs alternating: lambda has settled.
- **Still missing:** the replay window (games from earlier teachers), the learning rate, N and K,
  the blend (AB(lambda) or direct), and the random network's first games (adjudication, ply cap).

## Design decisions (2026-10-21)

Fixing the rewrite's weak points one at a time.

1. **The replay buffer: a sliding window (agreed in principle).** A student trains on a window of the
   most recent positions, whichever teacher played them; a promotion does not clear it, the new
   teacher's games pushing out the oldest, as Lc0's. **Each round (the developer):** N new
   positions enter, the N oldest leave, the window is shuffled, and the students train one full
   pass over it. A position then lives W/N rounds and is learned once in each: its lifetime reuse
   is W/N, above 1 by necessity. Reuse 1 would mean W = N, no window, and catastrophic forgetting;
   sampling N positions from a larger window (Lc0's, AlphaZero's about 0.7) would leave about 37%
   never seen at reuse 1, which self-play's cost cannot afford. The quality knob is W/N alone;
   proposed start 4, raised if forgetting shows, lowered if the students overfit the window.
   **The numbers (the developer):** W = 4096 x 4096 (about 16.8M positions, one of today's rounds),
   N = 1024 x 4096 (about 4.2M), so W/N = 4. Self-play adds N, then the window is shuffled whole
   (it fits in the training machine's RAM) and each student trains one round, 4096 steps of 4096. About 15 minutes
   of self-play a round on all three machines (about 16M positions an hour with psq, estimated from
   generation-2.md and windows-self-play-throughput.md), so a gate about every 15 minutes.
   **The start:** the window grows, 1N, 2N, 3N, then full at round 4 and sliding from there; a
   round's pass is always the whole window, so no early game is wasted.
2. **The learning rate: today's stepped rates, patience counted in promotions (the developer and
   Claude).** The rates stay 3e-3, 1e-3, 3e-4, 1e-4, 3e-5, over the whole run, never restarting at a
   promotion. Today's trigger, `patience_rounds` without a new best test loss, does not fit: the
   window, the teacher and the lambdas drift, so test losses of different rounds measure different
   things. Instead **any promotion restarts patience**, whichever student wins and whether lambda
   goes down, up or stays; when neither student beats the teacher for P rounds, the rate steps down
   and both students restart from the teacher's weights. Today's rollback to the best checkpoint is
   thus built in: the teacher is the best checkpoint. Lambda and the rate are independent controls:
   lambda follows which student wins, the rate whether any wins. Open: P, and what follows when
   3e-5 stalls too.
   **Revised the same day (the developer): patience counts promotions and test loss both.** The
   mechanics test on the Lichess machine showed promotions alone cannot tell students improving below the
   teacher from students stalled: random students against PeSTO lost every round, so patience spent
   all five rates in 20 rounds while they learned. So a round is progress if it promotes, **or** if
   either student reaches a new best loss on the test set: a fixed slice of the current teacher's
   games held out from training (`window.test_positions`, N / 16), played at the start and after
   every promotion. Between two promotions the teacher and the lambdas are fixed, so these losses
   compare round to round; the best losses start over at a promotion, and at a rate step, whose
   students restart from the teacher. As in Lc0, loss still promotes nothing: games alone decide the
   teacher.
3. **The gate: 16 pairs a student a round, to start; 32 tried later (the developer).** Gate and
   self-play share the CPUs: gate time costs new positions, slow detection costs freshness (the
   students train every round regardless, but the games keep coming from a teacher they have
   outgrown) and delays the rate's steps. Detection time scales about inversely with the pairs
   (sigma^2, so h and the delay, fall with them). With 15 minutes of self-play a round, about 0.125
   minutes of machine time a pair, d = 0.5:

   | Pairs | Gate a round | CPU on the gate | Rounds to detect +30 | Wall time |
   |---|---|---|---|---|
   | 8 | 1 min | 6% | 56 | about 16 h |
   | 16 | 2 min | 12% | 28 | about 8 h |
   | 32 | 4 min | 21% | 14 | about 4.4 h |
   | 64 | 8 min | 35% | 7 | about 2.7 h |

   16 keeps the gate near a tenth of the CPU; 32 if the teacher visibly lags behind its students.
   **Idea for later:** the gate's games are depth-9 games with scores and results too, and could
   enter the window, making the gate nearly free; kept out of the first version, being student
   against teacher, not the teacher's self-play. **delta: +30 Elo** (score 0.543, so
   delta about 0.0215), as in "CUSUM rule" below: early gains from a random network dwarf it, and
   late the patience of decision 2 and more pairs can respond. **h: one false promotion about
   every 200 rounds** (about 840M new positions, N about 4.2M a round), counting both students:
   each student's CUSUM is calibrated for about 400. h itself comes from a simulation using the
   draw rate measured in the first matches ("Open items").
   **First estimates, closed form (Claude, 2026-10-21, not simulated).** Games independent with draw
   rate d under the null: sigma of a round's score `sqrt(1 - d) / 8` with 8 pairs (the first proposal; at
   32 pairs sigma halves, h and the detection time divide by 4),
   `h = sigma^2 * ln(400) / (2 * delta)`, mean rounds to detect a gain of exactly +30 Elo about
   `h / 0.0215` (drift `2 * delta - delta`):

   | d | sigma | h | Rounds to detect +30 (mean) | New positions |
   |---|---|---|---|---|
   | 0.2 | 0.112 | 1.74 | about 87 | about 365M |
   | 0.5 | 0.088 | 1.09 | about 56 | about 235M |
   | 0.8 | 0.056 | 0.44 | about 23 | about 97M |

   UHO pairs often split 1 to 1 (each side winning with the favoured colour), which lowers sigma
   below the independent games' figure, so detection is likely faster than the table says.
   **To gather:** the counts of the five pair scores (0, 0.25, 0.5, 0.75, 1) from the first
   rounds' gate matches. **Then the simulation** (a small Rust test or `arena` subcommand, part of
   the implementation plan) draws pairs from those frequencies and gives h for 400 rounds per
   student, and P (below).
4. **P, the patience: above the detection time of a gain of exactly delta.** Shorter, and the rate
   drops while a real gain is still building up in S; larger gains are detected faster and never
   test P. Set by the simulation, as the rounds within which a +30 Elo gain is detected about 90%
   of the time (about 1.3 times the mean); placeholder **about 36 rounds** at 16 pairs (about 150M
   new positions), about 18 at 32 pairs (about 75M). At 8 pairs it would have been
   60 to 80 rounds (about 250M to 340M), letting only about three of the five rates run out of
   patience in a run of about 840M positions: the reason the pairs went up rather than P alone down,
   since a shorter P alone drops the rate on gains still building up in S.

5. **The blend: direct (the developer).** `label_t = (1 - lambda) * score(s_t) + lambda * z`, never
   AB(lambda)'s backward chain, for the bootstrap and the loop alike.
6. **No random start: a PeSTO bootstrap (the developer).** PeSTO (`hand-crafted`) plays depth-9
   self-play from the UHO openings, 96 x 1024 x 1024 positions (about 100.7M, 6 windows); a first
   network is trained on those games to convergence (forge's rounds with patience, PeSTO being a
   fixed teacher) with the direct blend at lambda 0.98, distilling PeSTO's games. That network is the loop's
   first teacher: PeSTO only plays and labels round 0, and is never a teacher in the loop (only a
   fixed yardstick of progress). Rotor still learns alone, PeSTO being a hand-written table, not
   another engine's knowledge (TODO.md's 2026-10-15 position, reversing continuous-self-play.md's
   "random network, no PeSTO" of 2026-10-20). The window starts holding the latest W of PeSTO's
   positions, pushed out over the first rounds by the networks' games; the growing start (decision
   1) and the game-length question of a random network fall away. About 3.5 to 4 hours of
   self-play on all three machines (hand-crafted's rates, generation-2.md and
   windows-self-play-throughput.md). **The students start on rungs 2 and 3 (0.98, 0.96):** one keeps
   the first teacher's lambda, the other tries one step more trust, the direction the hypothesis
   predicts; the symmetric rule moves the pair back up if 0.98 proves too low. This replaces the
   loop's start on rungs 0 and 1 in "Claude's rewrite of the idea".
7. **Distribution: distributed-spsa.md's LAN transport as the backbone, as implemented on
   2026-10-20 (the developer).** TCP, one message per connection, pull not push, a job unfinished
   after 30 minutes handed out again, the server found through its file on Drive with a fresh
   token. **The server on the training machine**, beside the window and the GPU (unlike the SPSA server, on
   the Lichess machine). Two kinds of job: self-play games with teacher n, and gate pairs, a student against
   teacher n. Games come back inside the reply (a round about 20 to 25 MB at 5 bytes a position);
   networks go out by hash when they change. the training machine's own client runs fewer threads, sparing the
   training's batch building. A machine off the LAN sits out, its games being a past
   teacher's.
   **The server's machine is not fixed (the developer, 2026-10-21):** the first, throwaway test
   runs on the Lichess machine as server and client both, then the training machine serves and every other machine is a
   client. **The server trains, always on its own machine:** on its GPU if it has one (the training machine, the
   usual server), on its CPU otherwise (the Lichess machine); clients only play. So any machine can serve;
   on a CPU the round's numbers (W, N, steps) are made small in the run's configuration, the test
   checking the loop's mechanics, not its strength.
8. **Resume at round granularity (the developer).** At each round's end, before the next begins, the
   server writes everything the loop needs: the teacher's and the students' weights with their
   optimizer states, each student's S, the rungs, the rate and its patience counter, and the
   window. A stop mid-round loses at most that round's self-play (about 15 minutes), replayed from
   its start on restart.
9. **The window: one file per round (the developer).** Each round's N new positions are one file,
   games with their scores and results in today's self-play PGN format; the window is the latest
   four. Sliding is a file in, the oldest out, and a resume needs nothing more than the files on
   disk. The bootstrap's 100M positions are 24 such files, the window starting as their last four.
   Files leaving the window are archived, never deleted (about 20 to 25 MB a round): the run's
   history, and the data for later experiments such as relabelling. **The archive is a disk on another
   machine (the developer),** not always reachable, so a file leaving the window waits in a staging
   folder on the training machine, and is moved when the archive is reachable, deleted from staging
   only once the archive's copy is verified by hash (`rclone move`, which checks both ends). The
   loop never reads the archive, so an unreachable archive never stops a round; staging grows about
   100 MB an hour (4 rounds of 20 to 25 MB) while it waits.
10. **A new path beside the old, not a replacement (the developer).** Today's generations (`arena
    self`, `smithy forge`, `arena select`, the gate) and every existing command keep working
    unchanged, so they can still be run. The loop is new code and new commands, reusing the existing
    pieces (self-play's game loop, the trainer, the direct blend's labels, spsa's transport) without
    changing their current behaviour.
11. **Smaller points.**
    - **Gate matches:** depth 9, UHO openings, each pair one opening with both colours; no MultiPV,
      which stays in self-play only, as already implemented (the developer).
    - **When 3e-5 stalls too:** the run stops and reports; whether to change something and go on is
      decided by hand then.
    - **h and P:** placeholders until the simulation, run once the first rounds' pair counts exist.
    - **MultiPV's positions are kept (the developer).** Its non-best moves break the wish that every
      move is the best only for positions before them (their `z` follows a deliberate detour; `s_t`
      is the position's own best-play score either way), at most the 6 plies after the book. But
      MultiPV only plays moves within 50 cp of the best (`EXPLORATION_FILTER_CP`), so the bias is
      small, and no training data is skipped. Should it be tried later: self-play already records
      the played and the best move at every ply, so skipping every position up to the last non-best
      move is a filter at labelling time, no format change.

## Specialization of the idea with Lc0 labels instead of self labels

With Lc0 labels, we cannot run self-play games.

Instead, we simply train on 1B positions, blended with the
game result on a grid of lambdas, and run a tournament
of the final network where each competitor is a lambda.

**Running as direct-blend.md (`lc0-psq-direct`, forge started 2026-10-20).** The direct blend,
`(1 - lambda) * score(s_t) + lambda * z`, against AB(lambda)'s backward chain, whose selects gave a
zigzag over lambda rather than a curve. The question: does the direct blend give a smooth curve? Same
rows and config as `lc0-psq`, ten lambdas 0.05 to 0.95 on a linear grid; its peak maps to a rung
afterwards.

**Remarks (Claude, 2026-10-21).**

- **Why a grid suffices.** Lc0's teacher is fixed, and the best lambda depends on the teacher's
  quality, so there is a single optimum to find and nothing for a ladder to track. Promotions, CUSUM
  and moving the pair exist only because the self-play teacher changes; without that, the loop reduces
  to training every lambda once and comparing them.
- **Already under way.** This is the direct blend's select on Lc0's labels (direct-blend.md). Its role
  here: the endpoint the self-play ladder should approach for a very strong teacher.
- **The grid on the ladder's rungs:** 0.99, 0.98, 0.96, 0.93, 0.86, 0.76, 0.61, 0.44, 0.28, 0.16,
  0.09, so the result reads directly as "self-play should settle near rung k". Training is fast (the
  bottlenecks are self-play games and the preprocessing of Lc0's tars), so a denser grid costs little,
  but neighbours closer than a rung differ by less than training luck.
- **Read the tournament as a curve, not an argmax.** Training luck alone separates neighbouring arms
  by one to two points: fit the smooth curve over lambda the developer forecasts and take its peak.
- **Lc0's `z` is a weaker signal than self-play's.** The idea works best without temperature, every
  self-play move the search's best, which Rotor's own games can do and Lc0's cannot: their training
  games use temperature, so their results reflect a weaker game than the positions deserve. `z` is
  then worth less on Lc0's labels, and the optimum found there is biased low: a lower bound for a
  no-temperature lineage of equal strength, which should settle at or above it. Two ways to soften
  it, both unverified: if Lc0's runs used temperature only in the opening, `z` is mostly on-policy
  after it; and if the corpus kept the played and the best move per position, games with late non-best
  moves could be dropped.

## Developer compact algorithm rewrite

The following rewrite has weaknesses, like a missing replay buffer. Do not worry too much: try to understand the intuition behind, then we fix later.

1. Start with a random network with only a value head, and no policy head.
2. Plan N salf-play games using AB at search depth 9 using the random network as evaluator.
3. Generate targets blending the search evaluation with the final result, using lambda in {0.99, 1.00}.
4. Train both targets for one round ok K steps.
5. Pit the two trained networks against themselves and against the previous teacher: use 8 game pairs per match.
6. When one of the new networks becomes better (according to some CUSUM idea), it becomes the new teacher.
7. If the new teacher is 1.00, keep lambda in {0.99, 1.00}
8. If the new teacher is 0.99, switch to lambda in {0.98, 0.99}
9. Continue, progressing training and lowering lambda as needed by 0.01.

## Context
- Rotor: alpha-beta search, depth 9, no policy network.
- Goal: emulate Lc0's self-training loop with my own infrastructure (no Bullet).

## Core idea
Search acts as a policy-improvement operator. Training distils the search's
improved output back into the network, so the next search starts better.
In AB + NNUE this means: distil the search score into the static evaluation.

## Lc0 → AB + NNUE mapping

| Lc0 | AB + NNUE |
|---|---|
| MCTS (PUCT), ~800 nodes/move | AB at fixed depth or fixed nodes |
| Policy target (visit counts) | None; search score is the only improved signal |
| Value target (result + Q blend) | Result + search score blend (λ) |
| Transformer, ~1 eval/node | Tiny NNUE, millions of evals/s |
| Dirichlet noise + temperature | Random opening plies, MultiPV sampling |
| Sliding window of recent games | Same |
| No gating | Gating via CUSUM (see below) |
| Tablebase rescoring | Same |

## Key differences
- **No policy:** move ordering (hash, killers, history) does the policy's job.
  Each position gives one scalar target instead of a distribution;
  compensate with data volume.
- **Noisier target:** AB score is a single-PV minimax value, unreliable
  near tactics. Filter out positions in check and positions where the best
  move is a capture or promotion; skip the first few plies.
- **Network constraints:** large sparse first layer with incremental updates,
  small dense layers, quantised to integers for the engine. Training in
  floats; float → quantised conversion is part of the infrastructure.
- **Optional later experiment:** small policy head used only for move
  ordering at root or shallow nodes.

## Self-play loop
1. Self-play with the teacher at fixed depth or nodes.
2. Diversify: opening book, random early plies, or MultiPV sampling
   within a small score margin.
3. Record per position: FEN, search score, game result.
4. Filter to quiet positions.
5. Train on blended target:
   `target = λ · sigmoid(score / K) + (1 − λ) · result`
   (λ typically 0.7 to 1.0; K maps centipawns to win probability).
   Note: here λ weights the search score, the opposite of Rotor's convention.
6. Promote student to teacher when the switching rule fires; repeat.

## Replay buffer
- Window = data from the last 3 to 5 teacher generations, measured in positions.
- Reuse factor (samples drawn / new positions added): aim for ~1 to 4.
- Break correlation: subsample positions per game, shuffle across games.
- Deduplicate by position hash (openings otherwise dominate).
- Optional recency weighting (e.g. weight halves per generation back).
- **Relabelling (AB advantage):** rescore old positions with the current
  teacher (like MuZero Reanalyze). Results stay fixed, scores improve.
  Allows a bigger buffer without stale labels.

## Teacher switching
Full SPRT per candidate is too slow. Cheaper supports:
- EMA of student weights as the candidate teacher.
- Validation loss on a fixed held-out set (quiet positions labelled by a
  deep search, e.g. depth 14+): filter against obvious failures only.
- Occasional longer match vs a teacher several generations back to catch drift.

### CUSUM rule (main switching criterion)
Every 4096 training steps, play n game pairs (n = 8) student vs teacher:
same opening, both colours, fixed nodes. Rotate openings between checkpoints.

At checkpoint k:
1. `x_k = mean pair score − 0.5` (pentanomial pair scores: 0, 0.25, 0.5, 0.75, 1)
2. `S_k = max(0, S_{k−1} + x_k − δ)`
3. If `S_k > h`: promote student, reset `S = 0`.

Parameters:
- **δ:** half the smallest gain worth switching for, in score units.
  +30 Elo ≈ score 0.543 → δ ≈ 0.0215.
- **h:** controls false switches. Measure σ (std of x_k) from teacher vs
  itself, then approx `h ≈ ln(ARL₀) · σ² / (2δ)` with ARL₀ ≈ 200
  checkpoints between false alarms. Prefer calibrating by simulation.

Illustrative (σ ≈ 0.09 with 8 pairs; depends on draw rate):
- +30 Elo: h ≈ 0.95, detection ≈ 44 checkpoints (~350 pairs).
- +10 Elo: hundreds of checkpoints.

Limit: 8 pairs/checkpoint detects large gains fast, small gains slowly.
Later in training, increase pairs per checkpoint or raise δ.
Regressions reset S to 0 automatically.

Safeguard: keep the best-ever network; repeated failures suggest a bug
or bad hyperparameters.

## Open items
- SPRT/CUSUM calibration script (simulate null with measured σ).
- Data generator, NNUE architecture, trainer design.
