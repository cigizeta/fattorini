# Generation 2

**Correction (2026-10-16): today's amanda-0002 is lambda 0.99, not 0.95.** Her files in
`data/networks/amanda/gen-0002/` on the training machine say: `champion.txt` lambda 0.99, step 151,552, test loss
0.15431; `margin.txt` 125; `selection.txt` a round robin of 0.90 to 1.00 won by 0.99, then the gate
against amanda-0001-0.85 passed by SPRT (elo_d 35, +25 -60 =29, 74.5 - 39.5). So generation 2 was
redone on that grid after the 2026-10-10 promotion below, and this doc was not updated; when and why
is not recorded here. The 0.95 account below is that earlier promotion's, kept as history.

**Status (2026-10-12): done.** amanda-0002 promoted 2026-10-10 (lambda 0.95, margin 150, +157 Elo
at depth 9 in the gate); betty-0002 only drew her at depth 9, so betty-teaches-betty was not run.
Caroline and dorothy (psq_threats, without and with a hidden layer) are not this doc's caroline:
they are new lineages taught by amanda-0002 (psq-threats.md, generation-3.md).

**Status (2026-10-10):** the replay and amanda-0001's self-play have ended; next, `prepare` and
`train` on the training machine for both networks. What follows is the status as of 2026-10-08.

**Status (2026-10-08):** the corpus is being labelled. amanda-0001's replay runs on the Lichess machine at her
tuned margin of 125 cp per ply, with tablebase positions labelled from the tables
(replay-tablebase-labels.md), about 1,000 to 1,400 positions/s there; with all three machines about
a waking day. `data/machines.txt` lists three machines, each taking its own third. Before the training machine or
another machine join: `git pull`, amanda's `margin.txt` copied from Drive into `data/networks/amanda/gen-0001/`,
any local `data/games/replayed/*/amanda` deleted. **Self-play by `amanda-0001` has not started**
(20% of the corpus, about 73M positions, at her tuned margin); `prepare` waits for it as for the
replay. It starts once the replay ends (the developer, 2026-10-08), with
`bin/rotor-arena self amanda-0001` on every machine.
Then `prepare` and `train` on the training machine for both networks, `rotor-arena select` (round robin, the
winner's margin search, the gate), `rotor-train promote`.

**Checked ready 2026-10-08:** both configs carry the 0.60 to 1.00 grid and the same training
fields; betty's first-layer clipping is in training (`FIRST_HIDDEN_WEIGHT_BOUND`, though the `i8`
kernel it serves was shelved); both networks have a promoted generation 1; both
`gen-0002/teacher.txt` on Drive name `amanda-0001`; amanda's `margin.txt` is on Drive.

Moved here from TODO.md's "Now" on 2026-10-08, where it had grown day by day.

## Training (2026-10-10, the training machine)

- `champion.vs` for `gen-0001` was missing on the training machine: `train` did not complete a local generation
  from Drive as the arena does; copied by hand. Since 2026-10-10 `train` fetches the previous
  champion's files from Drive itself (`pull_previous_champion`), tests to run.
- Amanda, nine lambdas in lockstep: GPU about 60% while training, about 80% during the test pass.
  Rounds of 20 to 28 s training, 5 s test. Test loss at round 10: lambda 0.95 0.0665, lambda 1.00
  0.3196 (labels from the game's result alone, hard to predict from a position), falling slowly.
- **Amanda's round robin (depth 9, 100 pairs a pairing, 1600 games each, 8.5 min):** 0.95 57.6%,
  1.00 57.0%, 0.90 52.1%, 0.85 49.8%, 0.80 48.5%, 0.75 47.5%, 0.65 46.6%, 0.70 46.6%, 0.60 44.4%
  (each +/-2.0%). Nearly monotone in lambda; 0.95 and 1.00 level within the error. Not the red flag
  (1.00 did not win), but the teacher's scores add little over the result at the top. Generation 1
  also won at 0.95. Then the margin search for amanda-0002-0.95 at 10+0.1, 200 against 100.
- **Margin search, match 1:** 100 beat 200, 201.0 - 189.0 after 390 games (+108 -96 =186 from
  100's side, 19.3 min), about +10 Elo, within noise: a best of 400 always names a side. Match 2:
  100 against 150, won by 150; match 3, 150 against 125, won by 150. **Amanda-0002-0.95's margin
  is 150** (generation 1's was 125).
- **Gate, depth 9:** amanda-0002-0.95 (margin 150, from her `margin.txt`) against amanda-0001-0.85
  (margin 125, hers): PASS at 80 games, 57.0 - 23.0 (+49 -15 =16, 71%, about +157 Elo), llr +3.02.
  The gate's lines named neither margin; since 2026-10-10 a match's opening line shows each
  player's `@<cp>`.
- **Promoted (2026-10-10):** amanda-0002, lambda 0.95, step 173,056, test loss 0.05975, margin 150;
  uploaded to Drive, `CURRENT` names gen-0002.
- 100 has beaten 200 in every margin search so far (the developer). **The floor
  stays at 100 (decided 2026-10-10, the developer):** a safety net against longer time controls,
  where a margin tuned at 10+0.1 may prune too aggressively.

## Lambda 1.00 is a canary (decided 2026-10-10, the developer)

Lambda 1.00 stays in every round robin but is never promoted: it labels by the game's result
alone, so it measures whether the teacher's scores help. If it ranks first, `select` warns, and
the best other lambda goes on to the margin search and the gate. Implemented 2026-10-10
(`round_robin::challenger_lambda`, a `warning` line in `selection.txt`), tests to run.

## Decisions (2026-10-07, the developer)

- **Amanda and betty both train on one corpus labelled by `amanda-0001`** (explicit-teacher.md),
  after the search work (B1 done, B6 shelved), with the replay and self-play on all three machines.
  Self-play stays 20%: only the teacher changes.
- **Caroline is retired:** amanda's shape, drawn with her, so one psq lineage is enough; nothing is
  copied, amanda continues from her own generation 1, and `caroline-0001` stays as an arena
  opponent. Her config is free, for a new architecture later (probably psq_threats).
- **Betty clips her first hidden layer's weights to +/-127/64 in training** (Stockfish's range, a
  fixed `i8` scale of 64), for the `i8` kernel; the transform stays `i16`, later layers f32.
- **The lambda grid is 0.60 to 1.00, nine values.** Generation 1 won at 0.95, the top of its grid
  (`L_t = lambda L_{t+1} + (1 - lambda) score_t`, so 0.95 averages the teacher's scores over about
  20 plies ahead; the result weighs about 13% 40 plies before the end), and 1.00 labels every
  position with the game's result alone, bar the search's decisive scores. `validate_lambdas`
  accepted only (0, 1) and now takes (0, 1]. **1.00 winning is a red flag:** the teacher's scores
  would carry nothing usable, and the labelling chain (the scores, their scale, the blend) gets
  investigated before generation 3. (Amanda's generation 1 round robin had put 0.85 first, the top
  of her grid then: 0.85 54.4%, 0.80 51.3%, 0.75 50.8%, 0.70 48.7%, 0.65 48.5%, 0.60 46.2%.)
- **The CCRL window runs to 2026-09** (`configs/corpus.yaml`). Its archive was downloaded
  (2026-10-07 on the Lichess machine, which needed 7-Zip and zstd through winget) and chunked (2026-10-07 on
  the training machine): 28,202 games, matching the `[Event` count of the file on Drive (an earlier note of 16,887
  was wrong), 3,175,561 positions, 32 chunks, 0 thrown away.

## The evidence behind them (generation 1, 2026-10-06 and 07)

- **Caroline and betty, generation 1** (betty-caroline-training.md's Results): both won by lambda
  0.95 and passed the depth-9 gate against hand-crafted at about +100 Elo. At 10+0.1 against
  hand-crafted: caroline 52.5%, drawn at 1000 games (about +17); betty 44.1%, hand-crafted stronger
  at 700 games (about -41). A network's advantage at fixed depth is given back under a clock in
  proportion to its slower evaluation.
- **The speed gap (2026-10-07, idle the Lichess machine, plugged in).** `nnue_bench --search-only`, depth 9,
  Stockfish's 49 positions, one thread:

  | | nodes/s | nodes | time |
  |---|---|---|---|
  | hand-crafted | 2.99M | 3.18M | 1.06 s |
  | caroline | 1.43M | 1.71M | 1.20 s |
  | betty | 1.08M | 1.58M | 1.47 s |

  Per evaluation: caroline's update 102 ns and head 104 ns, betty's update 98 ns and head 385 ns,
  against 21 ns hand-crafted. After phase A of nnue-inference-speed.md the same day: caroline 1.05 s,
  betty 1.19 s to depth 9, node counts unchanged; betty against hand-crafted at 10+0.1 drawn at 1000
  games, 49.2%.
- **With int8 and DTZ (2026-10-07, the Lichess machine, 10+0.1):** caroline beat hand-crafted, 57.2% at 470
  games (about +50); betty drew at 1000 games, 50.6% (+453 -464 =83 from hand-crafted's side, about
  +4), 53.2 min.
- **Amanda against caroline (2026-10-07, depth 9, the same network shape):** drawn at 1000 games,
  514.0 - 486.0 from amanda's side (+399 -371 =230, 51.4%, about +10 for amanda), 2.3 min.
  Caroline's recipe (the whole new corpus, lambda 0.95) is not shown better than amanda's (the
  older, smaller corpus, lambda 0.85). Corpus and lambda changed together, so this does not isolate
  the corpus. Two readings, not told apart: the psq shape is saturated by either corpus (capacity),
  or both imitate the same hand-crafted teacher (labels).
- **Caroline against betty (2026-10-07, depth 9):** drawn at 1000 games, 515.5 - 484.5 from
  caroline's side (+394 -363 =243, 51.6%), 2.7 min. Same corpus, same teacher, and the hidden layer
  of 16 is not shown to help: the labels, not the capacity, are the likelier bound, so generation
  2's main lever is relabelling by a network champion.

## Labelling speed, before and after the margin work

- **2026-10-07, the training machine:** amanda's replay at about 1,700 positions/s, about 48 h for the windows on
  the training machine alone. On the same 40 games, amanda took 1.92x hand-crafted's time: 1.35x the nodes, mostly
  the pruning margins set in hand-crafted's pawn, and 1.43x the cost per node, mostly the evaluation
  (psq-node-cost.md). An earlier note put it all on the cost per node from self-play rates
  (hand-crafted 4,305 positions/s, amanda 2,560); those searched different games and the conclusion
  was wrong. Amanda scales worse across 20 threads (5.0x her single thread against 6.0x); Syzygy
  costs nothing measurable.
- **2026-10-08, after the margin was tuned (psq-node-cost.md, margin-search-command.md):** about
  1,000 sustained on the Lichess machine once warm, 1,414 at the start: about hand-crafted's rate.
- The replay was started 2026-10-07 on the training machine, stopped the same day pending the scaled-margins
  duel, and restarted afresh 2026-10-08 under the tuned margin, its earlier outputs deleted.
- A `self hand-crafted` segment was added to generation 1's corpus on 2026-10-07 by a speed check
  (`beta-20261007-162227.pgn.zst`); nothing reads it now.
