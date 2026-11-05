# Generation 3

**Superseded (2026-10-18), see archives-removal.md:** forge's dataset path is gone and every NNUE
trains on `labels: self` or `lc0`, so betty, caroline and dorothy never retrain generation 3 on the
archive corpus. Amanda's retraining (below) is the last.

**Status (2026-10-16):** all four networks are being retrained from scratch: training never
measured the weights it starts from, and a rate switch kept a damaged network (rollback-to-best.md,
now in). The earlier trainings and amanda's select were deleted. **Amanda has retrained; her
`select` is next**, then betty, caroline and dorothy train.

## The first training and its failed gate (2026-10-13 to 15)

Without round zero and rollback, every lambda's first stage at 3e-3 damaged amanda-0002's weights
and every later stage went on from the damage; the best was exported only at the end. Every lambda
ran 113 rounds. The round robin's winner, amanda-0003-0.98, failed its gate against amanda-0002-0.99
twice: at depth 9 (+90 -73 =81, gate-at-stc.md), then rerun at STC (10+0.1) after that change,
failed again (the developer, 2026-10-16; the score is not recorded here). So the candidate was not
stronger under either control, not only judged at the wrong one. That round robin is still
evidence for the teacher's scores: with 0.99's head start wiped out by the shared damage, 0.98 won.

## The retraining (2026-10-16, the training machine)

Round zero and rollback to the best at every switch. Each lambda's best against its round zero,
amanda-0002's weights:

| Lambda | Round 0 | Best | Gain | Best at round |
|---|---|---|---|---|
| 0.90 | 0.053627 | 0.049246 | -8.2% | 107 |
| 0.93 | 0.060402 | 0.056804 | -6.0% | 107 |
| 0.95 | 0.069568 | 0.066794 | -4.0% | 83 |
| 0.98 | 0.113349 | 0.112329 | -0.9% | 75 |
| 0.99 | 0.162529 | 0.162001 | -0.3% | 36 |
| 1.00 | 0.306652 | 0.304366 | -0.7% | 95 |

- Every lambda beat its start; none exported the starting weights.
- The gain shrinks toward 0.99, the champion's lambda, which gains least and finishes first: the
  starting weights fit its labels best, so most of a low lambda's gain is adapting to its own blend.
  These gains do not rank the lambdas; the round robin does.
- 0.99 failed to beat its start at 3e-3, 1e-3 and 3e-4, rolled back to it each time, and first beat
  it at 1e-4: amanda-0002 sits near the best this shape reaches on these labels. Every higher rate
  only cost rounds; a lower first rate for warm starts is to be decided after this generation.
- Runs ended between rounds 36 and about 112, against 113 for every lambda before: a stage now
  starts from the best, so no rounds go to undoing the previous stage's damage.
- **Round robin (100 pairs, depth 9):** 0.99 53.3%, 0.97 52.3%, 0.98 52.0%, 1.00 51.7%, 0.95 51.0%,
  0.96 50.6%, 0.93 49.6%, 0.94 48.4%, 0.92 47.8%, 0.90 46.9%, 0.91 46.2% (each +/-1.8%). The top four
  are within each other's margins; 0.99 beat 1.00 only 102 - 98 (120 - 80 in generation 2).
- **Margin:** 200 (`lambda-0.99/margin.txt`), against amanda-0002's 125; @200 beat @150 in the
  candidate's own match.
- **Gate (10+0.1): FAIL.** amanda-0002 (@125) against amanda-0003-0.99 (@200): 130.5 - 113.5 for the
  champion (+76 -59 =109 from her side), 244 games, about +24 Elo for her, llr -2.949.
- **The two evaluators are practically the same** (`nnue_bench --eval-scale` over the round robin's
  1,288,508 positions): pawn scale 55% both, p50 67 / 68 cp, p99 3200 both, slope 1.505 / 1.508,
  correlation 0.891 / 0.890. So the scale did not widen, and the gate mostly measured margin 200
  against margin 125. The candidate at the champion's margin (`bin/arena duel amanda-0002
  amanda-0003-0.99@125 --time 10+0.1`) was started and **stopped unfinished** (the developer,
  2026-10-16, about 52 minutes left): whether the network got worse or only no better stays open.
  Accepted either way: amanda-0002 stays the champion, and betty trains next.
- Rounds took 13.4 s training + 4.4 s testing. The test pass is about a quarter of the time; zelda's
  rounds were doubled for that reason (4096 steps, patience 6, still one Lc0 epoch).

**Earlier status (2026-10-15):** the corpus is labelled: the replay and the self-play by amanda-0002 are
finished on all three machines. Training on the training machine: **amanda, betty, caroline and dorothy have all
trained** (amanda and betty 2026-10-13, caroline and dorothy by 2026-10-15). **`arena select` has
run for none of them yet.** Betty does not look stronger than amanda so far, by the developer's
impression; her select will tell. Earlier status (2026-10-12): the corpus was being labelled.

**The labels come from two builds (2026-10-12).** The replay started on the build before
labelling-speed.md's tree-changing steps, B1 (staged generation) and B3 (pawns by shifts). Partway
through, all three machines (the Lichess machine, the training machine, another machine) were restarted on the build with both, about 30%
faster, keeping the chunks already labelled. Both builds label at depth 9 equally well: each step's
harm-check duel at depth 9 was drawn.

## The teacher

amanda-0002, generation 2's winner (generation-2.md): lambda 0.99, step 151,552, margin 125, per
her `champion.txt`, `margin.txt` and `selection.txt` (corrected 2026-10-16; this line said lambda
0.95, margin 150, the earlier promotion's figures). Every lambda of generation 3 starts from her
weights, which fit lambda 0.99's labels best: 0.99 has the least to gain and finishes first.
betty-0002 only drew her at depth 9, so betty does not teach.

## The students

- amanda and betty, as in generation 2.
- caroline (`psq_threats`, no hidden layer) and dorothy (`psq_threats`, hidden 16), new lineages
  taught by amanda-0002 on amanda's grid and training (psq-threats.md). With amanda and betty on
  the same data, the four cross the hidden layer with the threats. emily is retired.
- **Betty, caroline and dorothy retrain on 0.95 to 1.00, 400 pairs a pairing (the developer,
  2026-10-16):** amanda's round robins never ranked 0.90 to 0.94 near the top, so six lambdas train
  instead of eleven, and the round robin's 12,000 games (about amanda's 11,000) give each lambda
  4,000 games, about +/-1.25% against her +/-1.8%. 1.00 stays as the canary.

## Betty's retraining (2026-10-16, the training machine)

Lambdas 0.95 to 1.00, 400 pairs a pairing. About 9.8 s training + 3.3 s testing a round.

- **Round 0 already beat amanda's trained candidate:** betty-0002 untrained on this data, 0.063276 at
  0.95 and 0.156545 at 0.99, against amanda-0003's best 0.066794 and 0.162001. The hidden layer of 16
  fits these labels better than any training of amanda's shape.
- 3e-3 damaged every lambda again, and they rolled back to round 0.
- **0.98:** round 0 0.107140, best 0.104780 at step 149,504 (-2.2%, against amanda's -0.9%), 0.0075
  below amanda-0003-0.98.
- **Round robin (400 pairs, depth 9):** 0.98 52.7%, 0.96 50.4%, 0.99 50.4%, 0.97 50.1%, 0.95 48.7%,
  1.00 47.7% (each +/-1.3%). 0.98 clearly first; 1.00 last.
- **Margin:** 125, the champion's (`lambda-0.98/margin.txt`).
- **Gate (10+0.1): FAIL.** betty-0002-0.98 against betty-0003-0.98: 243.5 - 236.5 for the champion
  (+139 -132 =209 from her side), 480 games, about +5 Elo, llr -3.137. With both at margin 125 this
  is a clean comparison of networks: **even.** A 2.2% lower test loss bought no strength.

## Caroline's generation 1 (2026-10-16, the training machine)

psq_threats without a hidden layer, from random weights, taught by amanda-0002 on this data.
About 13.9 s training + 3.7 s testing a round, six lambdas.

- **Test loss, best:** 0.063832 at 0.95, 0.109123 at 0.98, 0.158333 at 0.99: below amanda-0003 at
  every lambda, above betty-0003. The threats alone, on amanda's shape, are worth about 0.003 at
  0.98 (-2.9%).
- 3e-3 helped from random weights (0.289 to 0.119 over 47 rounds); it only harms warm starts.
- **Round robin (400 pairs, depth 9):** 0.98 52.2%, 0.95 51.1%, 0.97 50.5%, 0.96 50.2%, 0.99 50.0%,
  1.00 45.9% (each +/-1.3%). 0.98 won with no head start, as for betty, so 0.98 looks like a real
  optimum on this corpus; 0.95 is not clear of it, so the grid is not widened. 1.00 is far last: the
  canary sits lower the richer the features (amanda 51.7%, betty 47.7%, caroline 45.9%).
- **Margin:** 100, the floor.
- **Gate (10+0.1): PASS** against the hand-crafted evaluation: 48.0 - 14.0 for caroline-0001-0.98
  (+46 -12 =4), 62 games, about +210 Elo, llr +3.111. Promoted 2026-10-17 as caroline-0001.
- **Against amanda-0002, depth 9, each at her own margin (125, 100): drawn.** 468.5 - 451.5 for
  caroline (+327 -310 =283), 920 games, 50.9%, about +6 Elo, S 0.4908 [0.4491, 0.5509] from amanda's
  side, curtailed: neither could pass its bound by 1,000
  (`data/matches/amanda-0002-vs-caroline-0001/20261016-1335/`). A test loss about 3% lower than
  amanda-0003's, and no stronger at fixed depth than amanda-0002.
- **Against amanda-0002, 10+0.1, each at her own margin: drawn, caroline ahead.** 516.5 - 451.5 for
  caroline (+335 -270 =363), 968 games in 48 min, 53.4%, about +23 Elo, S 0.4664 [0.4503, 0.5497]
  from amanda's side, curtailed: neither could pass its bound by 1,000
  (`data/matches/amanda-0002-vs-caroline-0001/20261016-1339/`). Drawn only for the 1,000-game
  budget. Both duels lean to caroline (+6 at depth 9, +23 under the clock): **the simplified threats
  help a little,** and their cost in speed does not undo it. Under the clock, caroline's lower margin
  (100) pays off and the search goes deeper, either of which may explain the larger lead there; not
  told apart (a depth-12 duel was set aside for dorothy). A lineage's history adds nothing: caroline,
  taught once by amanda-0002 from random weights and never by PeSTO, is level with or ahead of her.

## Dorothy's generation 1 (2026-10-16 to 17, the training machine)

psq_threats with a hidden layer of 16, from random weights, taught by amanda-0002 on this data,
six lambdas. She loads only since the threat sum moved to i32 (threat-sum-i32.md, commit 812113a).

- **Winner's test loss:** 0.065534 at 0.96, step 318,464.
- **Round robin (400 pairs, depth 9):** 0.96 53.5%, 0.99 51.3%, 0.97 51.3%, 0.98 51.2%, 0.95 50.9%,
  1.00 41.8% (each +/-1.3%). 0.96 clearly first, the first student of this generation not won by
  0.98 or 0.99. 1.00 far last again, the lowest canary yet (amanda 51.7%, betty 47.7%, caroline
  45.9%, dorothy 41.8%).
- **Margin:** 100, the floor. 200 against 175 drawn (200.0 - 200.0); 150 beat 200 (200.5 - 187.5)
  and 125 (200.5 - 193.5); 100 beat 150 (200.5 - 189.5).
- **Gate (10+0.1): PASS** against the hand-crafted evaluation: 43.5 - 10.5 for dorothy-0001-0.96
  (+42 -9 =3), 54 games, about +250 Elo, llr +3.051.
- **Promoted 2026-10-17** (`smithy promote dorothy`): dorothy-0001, uploaded to Google Drive, where
  `CURRENT` names her.
- **Against caroline-0001, 10+0.1, each at her own margin (both 100): drawn.** 463.5 - 452.5 for
  caroline (+294 -283 =339), 916 games, 50.6%, about +4 Elo for caroline, S 0.5060 [0.4490, 0.5510],
  curtailed: neither could pass its bound by 1,000
  (`data/matches/caroline-0001-vs-dorothy-0001/20261017-0401/`). **The hidden layer of 16 buys no
  strength on psq_threats,** as it bought none on psq (amanda against betty).

## What generation 3 shows so far

- Lower test loss does not mean more strength: betty-0002 fit the labels better than amanda-0003 and
  only drew amanda-0002 in generation 2; betty-0003 fit 2.2% better than betty-0002 and drew her.
- Neither gate shows the challenger weaker: amanda's -24 Elo carried a margin of 200 against 125;
  betty's, at equal margins, is +5 Elo for the champion, even within noise. Self-teaching on these
  labels has stopped adding strength, for both shapes.
- The gate demands +35 Elo (SPRT elo_d 35). A generation that adds +10 never passes; whether that
  bar suits generations this alike is an open question.

## Why: a replay corpus barely changes between generations (the developer, 2026-10-16)

The likeliest cause, still a hypothesis. The corpus is 80% replay and 20% self-play
(generation-2.md). The replay part is the same historical games every generation: only their
search scores are relabelled by the new teacher, and at lambda 0.98 to 0.99 a label is mostly the
game's result, fixed in the archive. So most of the target hardly moves from one generation to the
next, and the student converges to the same network. It fits what generation 3 showed: amanda gained
0.3% learning from herself, betty's 2.2% was mostly adapting from amanda-0001's labels to
amanda-0002's, and both gates came out even.

Two other limits may add to it, told apart by the coming work: the psq shape may be too simple to
learn what a depth-9 search finds (caroline and dorothy, the threats, on the same data), and each
teacher is only slightly stronger than the last.

Checks, if wanted: on the replay positions generations 2 and 3 share, how far amanda-0001's labels
and amanda-0002's differ at lambda 0.98; and the test loss split into its archive and self-play
parts (TODO.md), to see where training gained.

The remedy is already decided: self-play only from generation 4 (rotor-alone-and-lc0.md). Each
generation's games are new, played by its champion, so their results, and the target with them,
move with her.

## Before training

- The sparse transform (sparse-transform.md) is done: generation 3 trains with it on the training machine, much
  faster by the developer's observation.

## Next

For each student, in order: training (`smithy forge`), then `arena select` (the round robin of its
lambdas, the winner's margin at STC, the gate against its generation-2 champion), then
`smithy promote` if the gate passes.

| Student | Training | `select` | Gate against |
|---|---|---|---|
| amanda | retrained 2026-10-16 | gate failed 2026-10-16; a retry would be on self-play alone | amanda-0002 |
| betty | retrained 2026-10-16 | gate failed 2026-10-16 | betty-0002 |
| caroline | trained 2026-10-16 | gate passed 2026-10-16, promoted 2026-10-17 | the hand-crafted evaluation (her generation 1) |
| dorothy | trained 2026-10-16 | gate passed, promoted 2026-10-17 | the hand-crafted evaluation (her generation 1) |

`select` runs on the training machine only (the developer, 2026-10-14), where the candidates are trained: after
caroline's training, or beside it. Then:

- **The hidden layer of 16:** amanda against betty and caroline against dorothy decide it for psq
  and psq_threats (architectures.md). It also decides where the move head's hidden layer goes
  (best-eval-together-with-best-move.md).
- **Generation 4's teacher** is generation 3's strongest champion (ccrl-sources.md, TODO.md).
  Its corpus is self-play alone, 1B positions, with no CCRL or TCEC (rotor-alone-and-lc0.md,
  2026-10-16).
- **Caroline's generation 2 started (2026-10-17):** teacher caroline-0001, `labels: self`. Test games
  (`arena self caroline-0001 --test`) on the training machine, training games on the Lichess machine and another machine. **Test games done
  the same day:** 70,294 games in `data/self/caroline/gen-0001/test/` (65,536 asked).
