# Learning-rate rules: every idea so far

**Status:** discussion (the developer, 2026-11-17), no plan yet, no code. Every rule considered for
when to lower the learning rate, with what decided each. Supersedes sign-driven-rate.md, folded in
here. gentle-rate-steps.md stays the note of its own experiment, its config already written.

## The problem, from run 2

`lc0-halfk-threats-screlu` run 2, on Drive (`history.tsv`): 271 rounds of 4096 steps, about 35 s
each, about 2.6 hours, about 4.5B positions. Today's rule (`stages.rs`): five rates, 3e-3, 1e-3,
3e-4, 1e-4, 3e-5; one warmup round; the next rate after 6 rounds without a new best test loss, rolled
back to the best; the run over when the last rate runs out.

| rate | rounds | gap at its end (train - test) | drop at the next switch |
|---|---|---|---|
| 3e-3 | 57 | about 0.0010 | 0.0021 |
| 1e-3 | 53 | about 0.0007 | 0.0011 |
| 3e-4 | 37 | about 0.0006 | 0.0004 |
| 1e-4 | 72 | about 0.0003 | 0.0001 |
| 3e-5 | 52 | about 0.0001 | (end) |

- **The rollbacks cost little:** 6 rounds at each of the 5 rates, 30 of 271 (11%).
- **The tails cost much:** each rate gains most in its first 10 to 15 rounds, then crawls. 3e-3
  gained about 0.0006 in its last 40 rounds, while the first round at 1e-3 gained 0.0021.
- **Why patience misses it:** the test loss jitters about 5e-5 from round to round, as large as a
  tail's progress, so noise keeps setting new bests and restarting patience. Patience 3 would have
  left 3e-3 at about round 47, patience 2 at about round 34.
- **In the 3e-3 tail** (rounds 16 to 57) the test loss fell in 28 of 42 rounds, about 67%: small
  but steady progress falls more often than not.
- **The pairwise-hidden run** (run 1, 2026-11-17, read from its chart) differs: 3e-3 about 32
  rounds, 1e-3 about 56, 3e-4 about 62. Histories depend on the architecture: a rule tuned on one is
  confirmed on another.
- **Unexplained:** twice the train loss dropped suddenly while the test loss did not, the gap nearly
  vanishing (steps 585728 to 598016, and 876544 to 888832), both just before a switch. Possibly new
  passes over the data; the corpus size not yet checked against those steps.

## What others do

- **nnue-pytorch** (Stockfish): Ranger by default (AdamW and schedule-free offered); 8.75e-4 times
  0.992 per epoch (about 0.9987 per round of ours, assuming its 100M-position epoch, not verified);
  length fixed by `--max-epochs`, 800 by default, about 80B positions. No plateau rule found.
- **bullet:** AdamW, weights clipped to 1.98; its example 1e-3 times 0.1 every 18 superbatches of
  about 100M positions, 40 superbatches (about 4B positions, close to run 2): about 0.979 per round
  of ours. Only fixed-length schedules. Neither trainer's run loop was read.
- **The literature** on rules driven by the loss: see the sign-driven rule below.

## The ideas

### 1. Schedule-free AdamW: set aside

Defazio et al. 2024, proposed in a chat with Gemini: no schedule, averaged weights usable at any
round, one stop at the end. Too new to trust first (the developer), though nnue-pytorch offers it;
our weight bound would need care (clamping its z keeps its average x inside). Gemini's brief also
proposed Elo gauntlets mid-training as the stop: dropped, close to the candidate tournament set aside
before (learning-rate-schedule.md, superseded by one-learning-rate.md).

### 2. A fixed decay: ruled out

A fixed geometric or step decay over a set number of rounds, as bullet and nnue-pytorch do: the
length of a run is not known in advance (the developer). Warmup-stable-decay (a constant rate until
a stop rule, then a short decay) avoids that, but keeps the noisy stop decision.

### 3. The slope against the stage's start: set aside

A line fitted to the current stage's last W rounds (W = 8): its slope s, the loss gained per round.
Cut when s falls below a quarter of s0, the slope over the stage's first W rounds. Fits only within
the current stage: a window across a switch measures the drops at the switches, not the rate (the
developer asked for W as the last three stages; the drops ruled the window out, the length idea
kept). On run 2's 3e-3 stage it cuts at about round 14. Set aside: s0, the start of a stage, is its
noisiest, least predictable part (the developer).

### 4. The slope against the last cut's drop: not pursued

Cut when the next W rounds would gain less than a cut does: W s < J, J the drop measured at the
previous switch. Both in loss units, so no threshold. The first stage has no previous switch.

### 5. The slope against the train-test gap: not pursued

The train loss is the round's average over its batches, each before its own step; the test loss
the round's end weights on held-out positions. Their gap is about half a round's progress plus the
rate's step noise, and in run 2 tracked the next switch's drop (table above). The gap is flat
within a stage and steps down at each switch, so it says what a cut is worth, not when: cut when
W s < gap. Not pursued for now; the unexplained train-loss drops above would also move the gap.

### 6. Gentle steps: config written

Today's rule, other numbers: rates from 3e-3 to 3e-5 in steps of about 0.7 (14 rates), patience 3,
rollback kept; no code. gentle-rate-steps.md, `configs/lc0-halfk-threats-pairwise-hidden-gentle.yaml`,
run only if pairwise wins the halfk line. Then discussed: steps of 0.9 (44 rates). With steps that
small most stages end at their patience, so patience becomes the decay rate (at most 0.9 every p + 1
rounds; patience 4, about 0.98 per round), and the rollback (up to 172 rounds over 43 switches)
would have to go. The multiplier can be calibrated, from one run's stage lengths: stages ending as
soon as patience allows mean steps too gentle; long ones, too big.

### 7. The sign-driven rule: a candidate

After every round, compare its test loss with the previous round's: fell, the rate times `a`,
slightly above 1; rose, times `b`, slightly below 1 and further from it. Clear progress makes the
rate rise; noise makes it sink. No stages, list, patience or rollback.

**Settings: `p` and `r` in the config, `a` and `b` derived** (the developer):

- `p`, the balance point: the share of falling rounds at which the rate holds steady; more, it
  rises; fewer, it sinks. `p ln a + (1 - p) ln b = 0`.
- `r`, the pace in pure noise: rounds falling and rising half the time each, the rate times `r` per
  round on average. `r = sqrt(a b)`.

      ln a = 2 ln r (1 - p) / (1 - 2p)
      ln b = 2 ln r p / (2p - 1)

`p` strictly between 0.5 and 1, `r` strictly between 0 and 1, refused otherwise.

**Starting values p = 0.85, r = 0.95** (a about 1.022, b about 0.883). Run 2's 3e-3 tail fell in 67%
of its rounds, so `p` must sit well above that for the rate to drop there:

| p | r | a | b | per round at 67% falls | 3e-3 to 3e-5 at that pace |
|---|---|---|---|---|---|
| 0.70 | 0.98 | 1.031 | 0.932 | 0.997 | about 1500 rounds |
| 0.80 | 0.98 | 1.014 | 0.948 | 0.991 | about 510 |
| 0.85 | 0.95 | 1.022 | 0.883 | 0.974 | about 175 |
| 0.90 | 0.95 | 1.013 | 0.891 | 0.971 | about 156 |

At 0.85 and 0.95: about 0.974 per round in such a tail, from about round 16; about 1.015 per round
where 95% of rounds fall; 0.95 in pure noise.

**A multiplier growing with the loss's change: dropped.** Raising or lowering in proportion to
|delta| (the developer's first form) needs its scale calibrated, the weakest part (the developer):
it changes about twentyfold within a run and between architectures. A possible later refinement:
|delta| against its own recent median, only "larger" or "smaller than usual".

**Literature** (read 2026-11-17). The parts exist; the share of falling rounds as a noise-calibrated
signal, its balance point chosen, was found nowhere.

- **Bold driver** (Vogl et al. 1988, Battiti 1989; not read): raise 1% to 5% when the training loss
  over a full pass fell, halve and undo the step otherwise. Holds the rate only at about 93% falls:
  in a noisy tail it collapses; abandoned for stochastic training (course notes,
  people.willamette.edu/~gorr/classes/cs449/momrate.html).
- **Duffner and Garcia** (Orange Labs, about 2007), the closest: bold driver on the validation
  error; halved when the latest relative change opposes a smoothed trend larger than 1%, otherwise
  times 1.1; starting at 1e-10, its growth acting as warmup; then patience, rollback and cut, as our
  rule of today. A 64-10-10 network on 3323 digits, small gains. Its 1% is about 60 times our
  relative change per round.
- **Chin, Zhuang, Juan and Lin** (LIBMF): bold driver at 1.05 and 0.5 on the training objective per
  pass, after Gemulla et al. 2011 (not read).
- **Xu, Dai, Kemp and Metz** (Google, 2019): a reinforcement-learned controller multiplying the rate
  up or down every 10 steps from the validation loss and other signals; beats a tuned step decay on
  small image tasks; its training needs many runs.
- **L4** (Rolinek and Martius, 2018): a step size from each minibatch's training loss; lost test
  accuracy to a hand schedule on CIFAR-10.
- **SASA** (Lang, Zhang and Xiao, NeurIPS 2019): constant rate until a statistical test (Yaida's
  stationarity condition, 80% confidence, once per epoch) says progress stopped, then a fixed cut;
  derived for SGD with momentum, not claimed for Adam. Lesson: its variance-blind test was far too
  sensitive to noise.

### 8. The slope test: a candidate

The developer's. Fit a line to the test losses of the last N rounds; the fit also gives the slope's
standard error SE, from the rounds' scatter around the line.

- **Clearly down,** slope < -k SE: keep the rate.
- **Otherwise** (flat, up, or too small to tell from noise): the rate times c.

First stated as "cut when the slope is up": on run 2 that cuts at about round 57, as patience did
(a tail's slope stays negative until progress stops), then decays by noise turning the slope up
about half the rounds. "Not clearly down" cuts earlier, and needs no external scale: the threshold
is the slope's own noise, which shrinks with the rate. SASA's lesson points the same way.

Run 2's 3e-3 stage by hand, N = 8, k = 2, SE rough:

| rounds | slope per round | SE (about) | slope / SE | decision |
|---|---|---|---|---|
| 20 to 27 | -4.2e-5 | 0.6e-5 | -6.8 | keep |
| 35 to 42 | -1.1e-5 | 0.8e-5 | -1.4 | cut |

First cut at about round 40, against 57 today: earlier, not by much; progress from round 25 to 40
was slow but real. Settings: k (larger cuts earlier), N (longer finds slow progress more easily and
cuts later), c (mild, as 0.9, applied every round the test fails: a gradual decay once in a tail).
Never raises the rate, so needs no ceiling.

**Against the sign-driven rule:** in the same tail it starts decaying from about round 16 at about
0.974 per round; the slope test keeps the full rate to about round 40, then decays. Both scale-free;
the slope test is closer to what the chart shows.

## Common to the candidates (7, 8)

- **Start:** 3e-3 after one warmup round, as today.
- **Floor and end:** the run ends below 3e-5; the candidate is the best checkpoint by test loss.
- **Replay:** every round's rate a function of the test losses in `history.tsv`, so a resume lands
  where the run was, as `stages.rs`'s replay today.
- **The test loss only,** untouched by the train loss's sudden drops.
- **The comparison:** against pairwise-hidden's run 1, final best test loss and rounds, then a
  depth-9 duel; a rule kept is confirmed once on another architecture.

## Open

- Whether a tail's share of falls stays near two thirds at lower rates and on other architectures.
- Replaying both candidates on the pairwise-hidden run's history: valid only up to each rule's first
  cut, since the recorded run went on at the old rate.
- The corpus size against the train loss's sudden drops.
