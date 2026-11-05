# Continuous self-play: design discussion

**Superseded, 2026-10-22: see self-training-simpler-attempt.md**: generational forges replaced the
continuous loop.

**Status: design in progress (2026-10-20).** Nothing implemented. For rotor-alone's self-play only;
Lc0's corpus keeps AB(lambda) as it is.

## Next: implement the whole loop (planned 2026-10-21, the developer)

Implement the continuous loop properly, then, time permitting, test it on a machine without a GPU
(the Lichess machine) if the training machine is still busy. First, as CLAUDE.md asks, an implementation plan
(`continuous-self-play-implementation.md`), which settles these points before any code:

1. **Step size:** games per step and training steps per step; play and train at once (training one
   network behind, as Lc0) or strictly alternate.
2. **The window:** how many recent positions training samples from, and how old ones drop out.
3. **The learning rate** in a run that never ends: constant, or otherwise.
4. **The gate:** how often a checkpoint challenges the champion, the match's size and rule.
5. **The lambda ladder's rule:** what makes the lower rung "stronger", and whether lambda may step
   back up (the controller section below).
6. **The blend:** AB(lambda) or direct, from direct-blend.md's select if it has ended.
7. **The random start:** a random-network player (forge's `initial.vs` is never a player today), and
   whether its games need adjudication or a ply cap despite the UHO openings. No temperature
   (decided, "No temperature" below).
8. **Server and client:** `arena spsa`'s distribution reused, both on one machine first.
9. **Records and resume:** what each step writes, and how a stopped run picks up again.

Also to settle: the corpus size, today global in `self.yaml` (`train.positions`), and whatever of
today's forge, select and promote the loop reuses rather than replaces.

## The idea

Today's self-play runs in big generations: the champion plays a whole corpus (hundreds of millions
of positions), a network trains on it until patience runs out, and it replaces the champion only by
winning a duel. The new self-play keeps the same loop (play, train, play with the trained network)
but in many small steps, so close together that the teacher improves almost continuously, as
Lc0's training does. **It starts from a random network, with no PeSTO (the developer, 2026-10-20):**
the first games are played by random weights and labelled by their results alone (lambda at or near
1), as AlphaZero starts tabula rasa, so rotor-alone trains really alone, not even PeSTO's tables
taught in. caroline-0001's lineage stays as it is (the open question in TODO.md, settled by this
design).

## Why: AB(lambda) against the direct blend

With `s_t` the search score at ply `t`, `z` the game's result (+1 win, 0 draw, -1 loss, for the side
to move) and `T` the game's last ply:

- **AB(lambda), today's:** `label_t = (1 - lambda) * s_t + lambda * label_{t+1}`, `z` at the tail.
  Unrolled, ply `t` takes in every later score with geometrically falling weights, and `z` with
  weight `lambda^(T - t)`.
- **The direct blend, Lc0's:** `label_t = (1 - lambda) * s_t + lambda * z`. The position's own score
  and the result, nothing in between.

Three effects explain why AB(lambda) works only partly (reasoning from the formulas and the three
selects, not measured):

1. **`z` hardly reaches early positions.** Its weight at `d` plies from the end is `lambda^d`:

   | lambda | d = 10 | d = 40 | d = 100 |
   |---|---|---|---|
   | 0.55 | 0.003 | ~0 | ~0 |
   | 0.95 | 0.60 | 0.13 | 0.006 |
   | 0.98 | 0.82 | 0.45 | 0.13 |

   Self-play's 0.98 won because it is the closest AB(lambda) gets to "give me `z`", and still an
   opening position learns mostly the teacher's own later opinions. The direct blend gives every
   position the full `lambda` of `z`.
2. **Later scores bring bias, little variance reduction.** Consecutive search scores of one game
   are strongly correlated, so averaging them removes little noise, while every non-best move
   (temperature, a teacher's mistake) pushes its drop back into the positions before it. The cut
   (temperature-cut.md) patches this; the direct blend has no path for it at all.
3. **The chain assumes the played line is the one whose value is wanted** (TD(lambda) is on-policy).
   With non-best moves it estimates a worse game than the position deserves, compounded ply by ply.
   The direct blend has the same flaw only once, in `z`, which volume averages out.

On Lc0's labels `s_t` is already near perfect play, so short horizons do as well and only the longest
suffer (the flat curves of the three selects); this design leaves them alone.

## What Lc0 does (lczero-training, read 2026-10-20)

- `tf/tfprocess.py`: `q_ratio = cfg['training'].get('q_ratio', 0)` and
  `qMix = lambda z, q: q * q_ratio + z * (1 - q_ratio)`, the value loss taken against `qMix(z, q)`.
  `q_ratio` is Rotor's `1 - lambda`; **its default 0 is `lambda = 1`: the value head trains on the
  game result alone**, as AlphaZero's. `configs/example.yaml` does not set it.
- `q` is `best_q` (`tf/chunkparser.py`), the value `smithy lc0` already extracts. Both `z` and `q` are
  win/draw/loss triples, the value head trained by cross-entropy on the blended triple.
- A moves-left head beside value and policy, loss weight 1.0 in the example config.
- The search's knowledge reaches the network through the policy head (visit counts); the value head
  only learns who wins.
- **Not confirmed:** which `q_ratio` their production runs used (their configs are not in the
  repository), and the `z`/`q` tensor mapping in `chunkparsefunc.py` (not read).

Rotor's alpha-beta networks have no policy head, so their value must carry the search's knowledge
too; that is why a blend with `lambda < 1` may still be needed here, unlike Lc0's default.

## The loop

1. **Play:** clients play a batch of games with network `n`, each game recording `n`.
2. **Train:** the server trains a fixed number of steps on a window of the most recent positions,
   warm-started from network `n`, labels by the direct blend.
3. **Publish:** network `n + 1` replaces `n` for the next jobs; games of `n` still arriving are kept.

Network 0 is random weights; its games, and the first steps', learn from the result alone. To work
out: a random network's search plays near-random games, long and mostly drawn by the 50-move rule or
repetition, so the early results carry little signal. The UHO openings, unbalanced on purpose, should
make results far more decisive (the section below); if not enough, adjudication, a ply cap, or
tablebases at the end (decisive results from few pieces) may be needed for the first steps to teach
anything; how AlphaZero and Lc0 handled their first, random generation is worth reading first.

## No temperature; diversity from the openings (the developer, 2026-10-20)

**Every move after the opening is the search's best: no temperature.** The method works best that way:

- AB(lambda)'s chain becomes honest: with no deliberate non-best moves, the played line is the line
  whose value is wanted (TD(lambda)'s on-policy assumption holds), and there is nothing for the cut
  (temperature-cut.md) to fix.
- The result means more: it reflects how the teacher plays at its best, a cleaner target at every
  lambda.
- The moving lambda needs it: trusting the network more as it strengthens assumes its scores and its
  games both reflect its real strength; temperature makes the games weaker than the network.

**Diversity, which temperature buys Lc0 and AlphaZero, comes from elsewhere:**

1. **The book: about 1M UHO openings.** A step of a few thousand games almost never repeats a start,
   so there is no diversity issue.
2. **MultiPV for 6 plies after the book,** as Rotor's self-play already does: variety where it is
   cheap, near the start. Any drop those choices cause can push back only into the few plies before
   them, not into the rest of the game.
3. **The changing network:** the teacher changes every few steps, so even a repeated opening leads to
   a different game, a source of variety today's big generations hardly have.

**UHO also helps the random start.** Its openings are deliberately unbalanced, one side clearly
better, so results are more decisive and less drawish and `z` carries more signal. That matters most
at the first rungs, where labels are almost the result alone and near-random play would otherwise
end mostly in draws.

## Server and client, on `arena spsa`'s pattern

Self-play splits perfectly by game, so every client thread adds speed. The distribution reuses what
distributed-spsa.md solved: a server hands out jobs ("play 8 games with network `n`"), clients fetch,
play and return, machines join and leave, and a stop resumes. Differences from SPSA: the job names a
network, which a client downloads when it changes, and results are games, not a score.

- **Server on the training machine**, beside the only GPU; training runs there between, or alongside, the games.
- **Clients:** the training machine (10 threads), the Lichess machine (8), another machine (4), each only when free.
- **One machine first:** server and client both on the training machine, to prove the loop before spreading it;
  written as server and client from the start, so the others join later with no redesign.

## The first experiment: AB(lambda) at 0.98 against 1.00 (the developer, 2026-10-20)

Two lineages under the same loop, both labelled by AB(lambda) (`labels::compute_labels`), lambda their
only difference:

- **A: AB(lambda) at 0.98**, self-play's winner under today's generations.
- **B: AB(lambda) at 1.00.** Not quite Lc0's `z`: `compute_labels` gives a decisive ply (mate or
  tablebase score) its own `±1` and propagates it backward, so at 1.00 every position takes the first
  decisive score after it in the game, or the result if none. It differs from `z` only where a game's
  result contradicts a decisive score found earlier (a mate seen, then thrown away), rare in
  self-play.

If A wins, the search scores add something a value-only network needs (Rotor has no policy head to
carry them). If B wins, the result alone, with decisive anchors, stands. The direct blend's intermediate values (0.90,
0.95) are tried only if A and B come close. To decide: whether both lineages run at once (halving
each one's games) or one after the other, and how they meet (a duel of their latest networks after
equal games, or each against the fixed judge).

## Hypothesis: lambda falls as the teacher improves (the developer, 2026-10-20)

**The intuition, in the developer's words:** at the beginning the network is random and completely
unreliable, so we learn from the result; later the network is stronger, so we trust the network more.
Lambda measures how much we distrust the teacher. Lc0 can keep lambda at 1 throughout because its
policy head carries the search's knowledge; Rotor's value-only network has to carry it itself, so its
lambda must move.

**The best lambda depends on the teacher's quality.** A weak teacher's scores are far from the truth,
so the game's result must carry the signal: a high lambda. A strong teacher's scores are already near
the truth, so the result adds mostly noise: a low lambda. Lc0's labels are a very late generation's.
The evidence so far fits:

- Self-play, an early lineage (PeSTO's first generation, then caroline's): 0.98 won, on a grid of
  0.95 to 0.99.
- Lc0's labels: short horizons won (0.05 to 0.35 about equal), the longest lost; lambda 0, the score
  alone, lost its duel against 0.15 (lambda-zero.md), so even a strong teacher needs a little blending.

**The developer's forecast for the direct blend in self-play:** a smooth curve over lambda, favouring
higher lambdas at early generations and lower ones at late generations.

**What it means for the loop:** lambda follows a schedule, 1 or near it for the random network and
falling as the lineage strengthens, rather than one fixed value. To decide:

- **The trigger:** by generation (step) count, or by the teacher's strength against the fixed judge
  (better: it follows what the hypothesis is about, the teacher's quality).
- **The form:** AB(lambda) or the direct blend, whichever direct-blend.md and its follow-ups show to
  be better; on Lc0's labels that is being measured now.
- **The experiment:** a falling schedule against a fixed lambda, two lineages, beside or after the
  0.98 against 1.00 experiment above, which tests the early end only.

**Caveat (Claude, 2026-10-20):** Lc0 itself trains on the result alone (`q_ratio: 0`) even late in its
runs. That does not refute the hypothesis: its policy head carries the search's knowledge, and Rotor's
value-only networks have no such outlet. So "late generation, low lambda" is claimed for value-only
networks, not for Lc0-style training in general.

## The lambda controller: a ladder on the logit scale (the developer, 2026-10-20)

How lambda moves down as the lineage strengthens, settled as a first experiment.

**Two arms in lockstep on adjacent rungs.** At any time the trainer trains two lambdas on the same
games, the current rung and the next one down. While the higher lambda is the stronger, nothing
moves; once the lower proves stronger, the pair steps down one rung. Expected: at the start, from a
random network, the higher lambda (more of the result) is stronger; later, the lower one (more trust
in the network) overtakes it. The stronger arm is the one that challenges the champion (open
question 4).

**The step, on the logit scale.** Lambda is bounded in [0, 1], so it moves by equal steps in its
log-odds rather than by a fixed amount or a fixed factor:

    odds(lambda) = lambda / (1 - lambda)
    odds(lambda_next) = odds(lambda) / gamma
    lambda_next = lambda / (lambda + gamma * (1 - lambda))

equivalently `logit(lambda_next) = logit(lambda) - log(gamma)`, with `gamma > 1`. With `mu = 1 - lambda`,
the trust in the teacher's score, the same rule multiplies `mu`'s odds by `gamma`: near `mu = 0` trust
grows almost exactly by `gamma` a step (AB(lambda)'s horizon, about `1 / mu` plies, divided by
`gamma`), near `mu = 1` the distrust lambda shrinks by `gamma` a step (the natural rule for the direct
blend); in between, the S-curve of logistic growth. It never leaves (0, 1), so no cap is needed.

**The start.** `lambda = 1` is a fixed point (infinite odds), so the first step is by hand: the lineage
starts at **`lambda_1 = 0.99`**, which for a random network is nearly the same as 1.

**First experiment: `gamma = 2`.** Each step halves AB(lambda)'s horizon near 1 and halves the
distrust near 0, a change in labels large enough to have a fair chance of showing in a match above
training luck. The ladder, computed each rung from the exact previous value, rounded to two decimals
only for the name (`label.lambda` takes two decimals, `lambda_text`), so rounding never accumulates:

| Rung | Exact | Named |
|---|---|---|
| 1 | 0.9900 | 0.99 |
| 2 | 0.9802 | 0.98 |
| 3 | 0.9612 | 0.96 |
| 4 | 0.9253 | 0.93 |
| 5 | 0.8610 | 0.86 |
| 6 | 0.7558 | 0.76 |
| 7 | 0.6074 | 0.61 |
| 8 | 0.4361 | 0.44 |
| 9 | 0.2789 | 0.28 |
| 10 | 0.1621 | 0.16 |
| 11 | 0.0882 | 0.09 |

Self-play's winner under today's generations, 0.98, is rung 2; Lc0's short horizons sit around rungs
9 to 10. If adjacent rungs prove indistinguishable, `gamma = 4` (coarser); if the lineage stalls
between two rungs, `gamma = sqrt(2)` (finer).

**Why steps on a ladder, not 0.01 at a time (discussed 2026-10-20).** Adjacent lambdas such as 0.76
and 0.75 differ by a few Elo at most, and lambdas trained side by side differ by one to two points
from training luck alone (the selects on Lc0's labels), so a match between them would measure luck.
Rungs a factor `gamma` apart in odds keep every comparison about equally large in effect.

**Open:**

- **What "proves stronger" means:** the match and its rule. One-sided is the likely form: step down
  unless the lower rung is clearly worse, since where two rungs cannot be told apart, which one is
  used costs nothing, and the hypothesis predicts a drift down.
- **A step back up,** should the lower rung later turn out worse, so the ladder corrects itself.
- **The blend:** the ladder serves AB(lambda) and the direct blend alike; which blend the loop uses
  follows direct-blend.md and its follow-ups.
- The first experiment above (0.98 against 1.00) is the ladder's first comparison in all but the
  start (0.99): it may simply become the controller's first rungs.

**The expected endpoint, from Lc0's labels (the developer, 2026-10-20).** Lc0's labels are a teacher
far stronger than any Rotor lineage will soon be, the ladder's far end: whatever lambda the direct
blend's select finds best there is about where a very strong lineage should settle. If it is 0.15 to
0.25, the last useful rungs are 9 and 10 (0.28, 0.16), and going much lower is wasted (lambda 0 lost,
lambda-zero.md). A lineage whose descent slows near that lambda would confirm the hypothesis. Two
caveats: a target only for the same blend (AB(lambda)'s lambda means something else), and an
endpoint, not a destination: the lineage may stop many rungs above it.

## Open questions

1. **Step size:** games per step and training steps per step, which set the CPU/GPU split. Play and
   train at once (training one network behind, as Lc0) or strictly alternate?
2. **The window:** how many recent positions a step samples from, and how fast old ones drop out.
3. **The learning rate:** a continuous run has no rate schedule ending in patience; constant, or
   stepped down by the window's age?
4. **The gate: the champion teaches until it is defeated (the developer, 2026-10-20),** as AlphaGo
   Zero: self-play always uses the current champion; training goes on from its own latest weights,
   never reverted; every so often the newest checkpoint plays the champion, and replaces it as the
   teacher only by winning. A checkpoint that loses is skipped as a teacher, not as a training
   state, and the next one tries again. (AlphaGo Zero: 400 games, 55% to pass, about every 1,000
   steps, from memory. AlphaZero and Lc0 later dropped the gate.) To decide: how often a checkpoint
   challenges, and the match's size and rule, so that a gap a few steps of training can open is
   measurable (Rotor's duel rule needs large gaps). Beside it, an occasional check against a fixed
   judge to see strength rising: PeSTO (`hand-crafted`), which still serves as a yardstick though it
   teaches nothing, then Stockfish at a level.
5. **lambda:** 1 or near it at the start, the network being random; 1.0 is Lc0's default. Fixed or falling as
   the teacher improves? One lambda, not ten arms: the lockstep lambdas and the lambda select do not
   fit a continuous run.
6. **Labels in the record:** the direct blend needs only `s_t` and `z` per position, so the label is
   computable at training time from the game; whether the record format changes.
7. **Architecture:** which shape the lineage trains (the best `lc0-` shape, once the experiments end?).
8. **Data:** where the window lives (local on the training machine), and what reaches Drive (networks only?).
9. **Interaction with other uses of the training machine:** forge and selects on Lc0's corpus cannot run beside it.
