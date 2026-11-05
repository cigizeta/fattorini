# Training the learned evaluation

**Archived: the training design of the `rotor-curate` another machine, overtaken throughout.** Gone: the
`rotor-curate` SQLite pipeline and the Lumbra corpus (now replayed TCEC/CCRL plus self-play,
corpus-windows.md), the `[0, 1]` and then `[-1, 1]` label scales and the `eps = 0.01` clamp (networks
now output pawns, `CEILING_PAWNS = 32`), quiet filtering and `preprocessing.pv`, windows, the cyclic
cursor, `early_stopping.state` and the bracket (now rounds, a shuffle buffer, a list of rates,
one-learning-rate.md, and round-robin-selection.md). Still true and in the code: AB(lambda), the
hand-rolled AdamW, `history.tsv`, `check`/`calibrate`/`correlate`, a flat `.nnue` apart from the
training checkpoints.

## Status

Design settled end to end: dataset source and generation, the AB(lambda) label mechanism and its
centipawn/expected-score conversion, the training loop shape, weight handoff, and the
strength-comparison pipeline. The club-level strength gate is cleared: search now has pruning,
extensions, and iterative deepening well past the depth-4, no-pruning, material-only baseline it
was written against.

`rotor-curate generate` populates a SQLite database with every ply's raw `Search(9)` score (see
`centipawn-calibration.md` for that pipeline's own schema, parallelization, and resume design).
`rotor-train prepare` computes AB(lambda)'s labels directly from that database, replaying the PGN
once to attach each label to its position. The network module (`moe.md`, archived -- superseded
by `rotor-halfkp.md`/`rotor-halfkav2hm.md`/`rotor-naive.md`) is feature-complete at the mechanism
level: the full training loop (window/checkpoint/resume/early-stopping machinery below), the
hand-rolled `AdamW`, and the evaluation-window loop are all built and tested, wired together by
`main.rs`'s `train` subcommand -- one long-lived invocation, resuming from a checkpoint directory
when one exists.

**Not yet done**: an actual end-to-end training run against the full corpus hasn't produced a
trained checkpoint. The candidate-selection tool (reads `history.tsv`, picks the best N
checkpoints) and `rotor-evolve` (the tournament/SPRT match runner) are both not built.

## Gate

Training labels must come from Rotor's own search, never another engine's output (CLAUDE.md rule
1), and that search must be dramatically better than the old depth-4, no-pruning, material-only
baseline before any label is trusted -- satisfied by search *maturity*, not evaluation quality,
which is what lets generation 0's evaluation stay simple while still producing trustworthy labels
(deep, well-pruned search turns even a mediocre static eval into a meaningfully better backed-up
value, per Veness, Silver, Uther & Blair 2009).

## Centipawn scaling

Every quantity in this pipeline -- an ordinary `Search(9)` score, a detected forced mate, a real
game outcome, and the network's own trained output -- passes through one fixed, closed-form
conversion, never a fitted one. The formula itself works in **pawns**, not centipawns --
centipawns are purely a UCI display convention, converted at whichever boundary actually needs to
talk in that unit, never baked into the sigmoid math itself:

```
score(pawns)      = sigmoid(log(2.0) * pawns)
pawns(score)      = logit(score) / log(2.0)
```

anchored on the assumption that a pawn is worth a 2/3 expected score -- a fixed design choice, not
fit from a dataset. `labels.rs::compute_labels` is the one place centipawns (`plies.score`'s own
database unit) convert to pawns before reaching `score` (`scores[t] as f64 / 100.0`, right at that
call site); `round.rs::run_cp_check` is the one place a pawns-scale error converts back to
centipawns, for its own human-readable report (`* 100.0` at the very end, after accumulating in
pawns throughout the sweep). A live engine's own `info score cp X` would be the analogous boundary
on the UCI side, once the network is ever wired into play -- not implemented yet.

- **No fitted win-probability curve anywhere in this pipeline.** `centipawn-calibration.md`'s own
  dataset-generation engineering (schema, parallelization, resume logic) is unaffected; only its
  earlier fitted-curve sections (now removed there too) no longer feed anything.
- **No separate win-likelihood network, and no sigmoid anywhere in the network itself.** `score`
  exists purely to build the label (`compute_labels`, blending outcome and search score in `[0, 1]`
  probability space -- see below). The network's own raw output is never squashed through `sigmoid`,
  and (as of the current iteration) never compared through `pawns_from_score` either -- see "No
  sigmoid in the network, MSE against the bounded label directly" below for how that came about.

**AB(lambda)'s own recursion works directly in `score`'s own `[0, 1]` domain** -- a sigmoid, not a
tanh, so there is no signed, zero-centered scale anywhere. A real game outcome (`games.outcome`'s
`0`/`0.5`/`1`) is already exactly this domain, used as-is. An ordinary `Search(9)` score becomes a
label via `score(cp / 100)` directly, no recentering. No special-case branch is needed for
mate-scale scores: `MATE_SCORE` (`32_000` centipawns, `320.0` pawns) run through `score` saturates
to a value indistinguishable from `1`/`0` at `f32` precision (`tch::Kind::Float`), the same way a
real decisive outcome already lands on `1`/`0` by construction. This blending has to happen in
`[0, 1]` probability space -- it's the only space where a discrete `0`/`0.5`/`1` outcome and a
`score(pawns)` search estimate can be linearly interpolated coherently.

The label produced by that recursion is used as-is at the loss boundary, still in `[0, 1]`: the
network's own raw, unbounded output is regressed directly against it with plain `mse_loss`, no
`score`/`pawns_from_score` conversion on either side. See the next section for how this loss
boundary got here.

### No sigmoid in the network, MSE against the bounded label directly

This went through three iterations before landing here.

**Iteration 1 (reverted): raw logits + `binary_cross_entropy_with_logits`.** `value_head::
ValueHead::forward` originally applied `sigmoid()` internally, with training's loss calling plain
`binary_cross_entropy` on that already-squashed output. Found after two real training runs
(`zelda`, transformer, `lr = 1e-4`; `ylenia`, naive NNUE, `lr = 1e-3`) both plateaued near `ln(2)`
test loss after several rounds, despite `rotor-train check`'s own baseline comparison confirming
real (if modest) learning was happening, not a data bug. Hypothesis: sigmoid's own gradient
(`p * (1 - p)`) vanishes as a prediction saturates toward `0`/`1`, so a network whose raw logits
swing large early in training (more likely at a higher, unscheduled LR with no warmup) can get
stuck even while still confidently wrong. Fix tried: `forward` stopped applying `sigmoid()`, loss
switched to the fused `binary_cross_entropy_with_logits` (mathematically the same loss value, but
its gradient w.r.t. the raw logit reduces analytically to `prediction - label`, which never
vanishes regardless of how saturated the equivalent probability would look).

**Why iteration 1 didn't actually change anything:** retraining `ylenia` from scratch under the
fix showed the same near-`ln(2)` plateau from the very first round (`train_loss` `0.6776 -> 0.6752`,
`test_loss` `0.6826 -> 0.6843` -- essentially flat, test loss if anything moving the wrong way). The
gap: BCE and BCE-with-logits only differ in gradient once a prediction is actually saturated near
`0`/`1` -- at `ln(2)`-adjacent losses the network's predictions are still sitting near `0.5`, nowhere
near saturated, so the two forms were never going to behave differently this early. The real
symptom -- no visible learning even in the easy, unsaturated regime -- pointed at something more
basic than saturation.

**Detour: Stockfish's own NNUE trainer (nnue-pytorch)** uses neither form of BCE. It computes a
generalized power loss in probability space: `sigmoid(pred / scaling)` vs. a WDL/eval blend target
(the same lambda idea as `compute_labels`), `mean(|p - q|^power)` with `power ~= 2.6` (empirically
chosen, not derived). Considered and rejected as a direction here: it still sigmoids the raw output
and computes the loss in probability space, so it still carries the same `p * (1 - p)` gradient term
iteration 1 was trying to get away from -- copying it would have been a step backward, and it
doesn't address the unsaturated-regime symptom either.

**Iteration 2 (reverted): drop the sigmoid from the loss entirely, regress raw pawns.** Since
`pawns_from_score` is already the exact pawns-space inverse of `score`, the idea was to convert the
label to pawns once, at the loss boundary (`pawns_from_score(label)`), and regress the network's
raw, unbounded output directly against that value with `mse_loss` -- no sigmoid anywhere in the
loss path at all.

**Why iteration 2 was reverted before ever being retrained against:** `pawns_from_score` (a logit)
is not a mild transform -- it's flat near `label = 0.5` and blows up near the boundaries. AB(lambda)'s
own recursion (previous section) routinely pushes `label` close to `0`/`1` for positions in a
decisive game, *not because the position's own raw eval is extreme, but because hindsight from the
eventual outcome is blended in*. A perfectly ordinary position (raw eval `+50cp`) blended with
`lambda = 0.7` against an eventual White win can land at `label ~= 0.886`, which
`pawns_from_score` reconstructs to `~296cp` -- a ~6x inflation from the logit's own blow-up, not
from anything about the position itself. `is_trainable`'s f32-saturation guard only keeps `label`
strictly inside `(0, 1)` at f32 precision, which still permits reconstructed pawns targets in the
tens of pawns for labels close enough to that boundary. A handful of such outliers per batch, under
a squared-error loss, would dominate the gradient and drown out every ordinary, well-behaved
position sharing that batch -- a much more likely explanation for "no visible learning" than
sigmoid saturation ever was, and one that specifically requires `lambda > 0` to bite (`ylenia`'s own
`lambda = 0.0` config reconstructs losslessly, `pawns_from_score(score(x)) == x` exactly, so this
mechanism alone doesn't explain that run's own earlier plateau -- a separate, still-open question).

**Iteration 3 (current): keep AB(lambda) and its `[0, 1]` label completely untouched, keep the
network's own output sigmoid-free (still just "whatever the last linear layer produces"), but stop
converting either side through `score`/`pawns_from_score` at the loss boundary at all.** `compute_loss`
is now plain `prediction.mse_loss(&label, Reduction::Mean)` -- the raw, unbounded output compared
directly against the bounded `[0, 1]` label, no logit, no blow-up risk, deliberately deferring the
question of what the network's own output *means* (pawns? something else?) until after confirming
this loss actually goes down at all. `rotor-train check`'s own cp-translation is left as-is for now
(not meaningful under this loss yet) -- revisiting it is explicitly future work, not blocking this
test.

Requires retraining from scratch (the network's own weights, not the prepared data, are what's
affected) -- not yet re-verified against a real run as of this note.

**Real run under iteration 3:** `ylenia` (naive NNUE, `lr = 1e-3`, reset from scratch) showed a real
downward trend for a few rounds (`train_loss`/`test_loss` both dropping, `0.0279`/`0.0305` at step
`8192` to `0.0247`/`0.0283` at step `16384`) before flattening out by step `32768`
(`0.02350`/`0.02904` -> `0.02348`/`0.02811` -- train loss essentially frozen). Real learning, but it
plateaus early. Motivated the next question, asked directly by the developer: is the training data
itself the limiting factor, not the loss/output design any more?

### `is_trainable` is wrong: exclude non-quiet plies, but not decisive ones

`is_trainable` (`length_cache.rs`) used to exclude every mate/tablebase-scale ply outright (`cp.abs()
>= TB_WIN_SCORE - MAX_PLY`), on the theory (its own original doc comment) that `evaluate()` is never
actually called on such a position in real play -- `quiescence` scores a proven mate/tablebase result
directly, without ever reaching its own stand-pat `evaluate()` call, so training against that label
looked like teaching the network outputs it would never be asked to produce. Consequence: the entire
training set was, by construction, made only of ambiguous, not-yet-decided positions -- the network
never saw a single example where the correct answer was certain, so nothing ever anchored the scale
of its own output at the extremes. Decided with the developer that this reasoning, while true, was
the wrong conclusion: even though `evaluate()` is never asked to judge *exactly* that position in
real play, training on it still shapes the shared representations (embeddings, whatever pattern
recognizes "this is winning") used to judge every genuinely uncertain position nearby that it *is*
asked about. Teaching the network what a won or lost position actually looks like is teaching it the
real goal of the game, not wasted signal.

Decisive plies are not blended into AB(lambda)'s recursion the way ordinary plies are, though --
`compute_labels` now gives a decisive ply its own label directly, `1.0`/`0.0` by sign, with no
`lambda`-weighted blend at all: a proven forced mate is a certainty straight from the search, not an
estimate, so blending it with anything would only dilute a ground truth already in hand. That hard
label still becomes `next` for the recursion exactly like any other label, so it still propagates
*backward* into earlier, genuinely uncertain plies through the normal blend -- only the decisive
ply's own label skips it, not its influence on what precedes it. The two exclusions inside
`is_trainable` are otherwise unrelated and both stay: a genuinely decisive score now gets accepted
(via `labels::is_decisive`, the same threshold `compute_labels` itself now uses) with its correct,
deliberate `1.0`/`0.0` label; a merely-large-but-not-decisive score that happens to accidentally
f32-saturate stays excluded, since that saturation is a precision artifact, not a fact about the
position.

The developer's own caveat, addressed directly: won't a long, clearly-won endgame flood the training
set with many near-identical `1.0` labels and skew a plain MSE loss toward "always predict close to
1"? Decided not to worry about this for now -- it doesn't happen much in real human games (most
positions never reach a search-confirmed forced mate/tablebase result at all), and correctly labeling
the positions that genuinely are won or lost matters more than that population-balance concern.

This changes what `prepare` writes (previously-excluded decisive plies are now included), so it
needs the same rebuild-and-restart-from-scratch treatment as any other data-shape change -- no way to
resume an existing run or reuse an existing `positions.cache`/labels build across this. Not yet
rebuilt or retrained against as of this note.

**Real finding: `ylenia`'s config was `depth = 0`, so decisive plies never occurred in its own data at
all.** `rotor_curate.rs`'s own `depth == 0` branch is a single `eval::evaluate()` call, no tree search
-- mate detection only happens inside `choose_best_move`'s real search (`depth > 0`). Tablebase
probing is separately, independently a no-op for every depth: `rotor_curate.rs` always constructs an
empty `SyzygyTablebase` (no `SyzygyPath` option exists in `rotor-curate` at all). So the `is_trainable`
fix above is real and correct, but has no observable effect on any `depth = 0` run -- exercising it
needs a `depth > 0` config. Not yet done; `ylenia` kept running at `depth = 0` as a "does the pipeline
still behave sanely" check instead.

**`ylenia`'s own `lr = 1e-3` run plateaued for real, this time with enough rounds (9) to be sure it
wasn't just early noise**: `train_loss` flat around `0.023` for 5 straight rounds, then increasingly
noisy (a spike to `0.044` at step `73728` while `test_loss` simultaneously improved -- an outlier
batch, not a trend break). Total training set: `67,441,383` rows -- large enough that "not enough
data" doesn't explain the plateau (well past a 10x-100x-parameter-count heuristic for a ~277K-
parameter naive net), so the likely cause was `lr` itself being too large for continued fine
convergence at this point, not data volume or architecture capacity. Dropped to `lr = 1e-4` in
`net.yaml` and resumed the same run (no reset needed -- confirmed by reading `adamw.rs::load`
directly: only the `m`/`v` moment buffers and step count `t` are restored, `lr` is always read fresh
from `net.yaml` at process start). Result: train loss stopped spiking and fell smoothly, test loss hit
several new bests (`0.02459 -> 0.02361 -> 0.02336` and still slowly improving as of this note) -- a
real, durable gain from the LR drop alone, no data or architecture change involved.

Side note for future reference: PyTorch's/`tch`'s own `AdamW` never decays `lr` on its own (confirmed
against `tch`'s own generated bindings and cross-tested against this project's hand-rolled `AdamW`,
which matches it step for step) -- a continuously falling LR in past work came from a separately
configured (or, in HuggingFace's `Trainer`, silently defaulted-on) `lr_scheduler`, not from the
optimizer itself. `transformers.TrainingArguments.lr_scheduler_type` defaults to `"linear"`, not
`"constant"` -- easy to use `Trainer` for years without ever naming a scheduler and still get one.
Rotor has no scheduler at all right now; the LR drop above was a manual, one-off edit-and-resume, not
an automatic schedule. Explicitly decided not to build a real scheduler yet (`net.yaml` edits are
enough for now) -- if one is ever wanted, a `ReduceLROnPlateau`-style drop (tied to the same
`test_loss`/`patience` signal early stopping already tracks) fits Rotor's own open-ended, resumable
training loop much better than a fixed-horizon schedule like HuggingFace's default linear decay would,
since Rotor never knows its own total step count in advance.

### Centipawn calibration: measuring the raw output's own distribution first

With the loss boundary settled (`mse_loss` directly against the bounded `[0, 1]` label, no sigmoid,
no `pawns_from_score` on either side -- the "iteration 3" design above), the deferred question comes
back: what does the network's raw output actually *mean*, and how does `rotor-train check`/a future
UCI-facing report turn it back into centipawns. Developer's proposal: sweep the training set, find the
raw output's own `min`/`max`, derive a clipping constant `C = max(|min|, |max|)`, treat the output as
bounded to `[-C, C]`, map that to `[-1, 1]` then to `[0, 1]` as an expected-score proxy, then convert
to centipawns via the existing `pawns_from_score` formula.

Worked through together before writing any calibration code: the `[-C, C]`-symmetric-around-`0`
framing assumes the raw output's own natural center sits at `0`. But `compute_loss` trains the raw
output directly against `label`, which is centered near `0.5` (`score(0 pawns) = 0.5`), not against
anything zero-centered -- so there's a real, unverified question of whether the raw output is actually
centered near `0` (this framing's assumption) or near `0.5` (what the training objective would more
plausibly produce). A worked example: if the true empirical range were, say, `[0.02, 0.95]` (centered
near `0.5`, not `0`), the proposed formula would map the min -- the network's own most confident
"this is losing" prediction -- to a reported expected score of `~0.51`, nearly perfectly balanced.
Wrong regardless of what `C`'s magnitude turns out to be, because the formula's own hard-coded center
(`raw_output = 0 -> q = 0.5`) wouldn't match where the data's real center actually sits.

Decided: measure first, decide the formula's shape after. Implemented `rotor-train calibrate
<network-name> <step>` (`round.rs::CalibrationReport`/`run_calibration_sweep`, dispatched from
`main.rs` the same way `check` is) -- sweeps the network's raw output over every row of the *training*
set (not test: whatever calibration constant eventually comes out of this should be derived from data
the network was actually fit against, the same reasoning `check`'s own baseline-vs-test split already
established), reporting `row_count`, `min`, `max`, `midpoint` (`(min + max) / 2`), `mean`, and
`std_dev`. Deliberately measurement only -- no calibration formula applied yet. `mean` is reported
specifically because it's the robust read of where the distribution's bulk actually sits (unlike
`midpoint`, which only looks at the two single most extreme rows and is exactly as fragile to one
outlier as `C` itself would be) -- comparing `mean` against `midpoint` directly is how the next step
decides whether the raw output is centered near `0` or near `0.5`, before committing to any specific
calibration formula.

**Real run, `ylenia` at its own best checkpoint (step `311296`, lowest `test_loss` in its history):**
`67,441,383` train rows, `min = -0.3702`, `max = 1.4163`, `midpoint = 0.5230`, `mean = 0.5251`,
`std_dev = 0.0990`. `mean` and `midpoint` agree closely (no single outlier row distorting the
picture), but both sit near `0.5`, not `0` -- confirming the concern above was real, not hypothetical.
Under the originally proposed `[-C, C]` formula (`C = max(0.3702, 1.4163) = 1.4163`), the network's own
single most confident "this is losing" prediction (`r = -0.3702`) would have been reported as
`q = 0.369` -- a `37%` win probability, nowhere near the near-certain loss it actually represents.
Also notable: `std_dev = 0.099` is narrow relative to the full `[min, max]` spread (`1.79` wide) -- the
bulk of predictions cluster tightly around the mean, with `min`/`max` reached only by a handful of
unusually extreme rows (classic regression-to-the-mean behavior under `mse_loss` from a small,
low-capacity network).

This result is what surfaced the real, deeper issue the next section addresses -- not just where the
calibration formula's center should sit, but that `[0, 1]` was the wrong codomain for the label to live
in at all, given where this network's output is ultimately headed.

## Plan: switch `prepare`'s label convention from `[0, 1]` to `[-1, 1]`

**Status: implemented** (`labels.rs`, `length_cache.rs`, doc-comment sweep across `batch.rs`/`cache.rs`/
`round.rs`/`value_head.rs`/`nnue_network.rs`/`transformer_network.rs`) -- inherited the old anchor as
written up below (`SCALE = ln(2) / 2`, not a fresh anchor), per the developer's own go-ahead rather than
raising it again as a separate open question. Not yet built/tested, rebuilt, or retrained against as of
this note.

One thing that only became precise during implementation, flagged as a real unknown in the plan below:
`tanh` turns out to have a **symmetric** f32-saturation threshold (`length_cache.rs::is_trainable`'s own
doc comment now covers the derivation) -- both a winning and a losing blowout saturate to exactly
`1.0`/`-1.0` at the same magnitude, around `2500` cp, unlike the old `sigmoid`-based formula's markedly
asymmetric `~2400` cp (winning) / `~14980` cp (losing) pair. A genuinely nice side effect of the switch,
not something that needed deciding -- `tanh` being odd (`tanh(-z) == -tanh(z)` exactly, since IEEE 754
negation is exact) makes the two directions symmetric by construction, simplifying `is_trainable`'s own
test suite along the way (one shared-magnitude test replaces the old three-test asymmetric dance).

### Why

Negamax/alpha-beta search (`rotor::search`, the thing this network is ultimately meant to feed once
wired into `evaluate()`) flips perspective between plies by pure negation: `eval(position, other_side)
== -eval(position, side_to_move)`, used everywhere the search propagates a child node's score up to
its parent. `score(pawns) = sigmoid(ln(2) * pawns)`, the formula this whole pipeline has used since
`training.md`'s very first "Centipawn scaling" section, does not have this property -- it's
antisymmetric around `0.5`, not around `0` (`score(-x) == 1 - score(x)`, not `score(-x) == -score(x)`).
Flipping perspective in this convention needs `1 - score`, a different operation than every other
signed evaluation in the engine uses. The final pawns-scale conversion (`pawns_from_score`) already
*is* properly signed and odd around `0` (`pawns_from_score(0.5) == 0`, `pawns_from_score(1 - p) ==
-pawns_from_score(p)`) -- the distortion is specifically in the `[0, 1]` layer itself, which is exactly
the layer this session's redesign now trains the network's raw output directly against (`training.md`'s
own "No sigmoid in the network, MSE against the bounded label directly" section). Moving the label to
`[-1, 1]` fixes it at the source, so the network's own raw output becomes a proper negamax-compatible
evaluation with no extra transform needed.

This also resolves the calibration tension from the real run above: under `[-1, 1]`, a genuinely
balanced position's label is `0`, so a well-trained network's raw output really should center near
`0` -- the original `[-C, C]`-symmetric-around-zero calibration idea becomes the *correct* framing
again, not the wrong one the real numbers just argued against.

### The formula

`tanh` in place of `sigmoid`, chosen to inherit the *existing* anchor rather than requiring a new one:
`tanh(z) == 2 * sigmoid(2z) - 1` is a standard identity, so setting `z = k * pawns` with the constant
`k` below gives

```
k = ln(2) / 2                         (a fixed constant, ~= 0.346574 -- computed once, on its own)
score(pawns) = tanh(k * pawns)        (== 2 * sigmoid(ln(2) * pawns) - 1)
```

which is *exactly* the old formula's output affinely remapped from `[0, 1]` to `[-1, 1]` (`y = 2x -
1`) -- same "a pawn is worth a 2/3 expected score" anchor underneath, just re-expressed: `score(1.0
pawn) = 2 * (2/3) - 1 = 1/3` (verified directly: `tanh(k) = 1/3` exactly, since `e^k = sqrt(2)`). The
inverse:

```
pawns_from_score(y) = atanh(y) / k    (== 2 * atanh(y) / ln(2))
```

**Open decision, not presumed:** inheriting the old anchor this way avoids relitigating "what is a pawn
worth," but it does mean the new anchor's own headline number (`1/3`) is less immediately intuitive
than the old one's (`2/3`) -- if a rounder number on the new scale is preferred instead (e.g. picking
`k` so `score(1.0 pawn) = 0.5` exactly), that's a one-line change to `k`, not a structural one. Flagging
for the developer to confirm before implementing, not deciding unilaterally.

### Scope of changes

1. **`labels.rs`**: `score`/`pawns_from_score` reimplemented per the formula above (Rust's `f64::tanh`/
   `f64::atanh` are already stable std methods, no hand-rolled exponential needed this time, unlike
   `sigmoid`). `compute_labels`'s own public signature stays unchanged (`outcome: f64`, still the
   database's native `0`/`0.5`/`1`) -- the `0/0.5/1 -> -1/0/1` remap (`2.0 * outcome - 1.0`) happens
   *inside* `compute_labels`, at the same internal boundary where it already converts `scores[t]` from
   centipawns to pawns, keeping its external contract stable. The decisive-ply branch becomes
   `if scores[t] > 0 { 1.0 } else { -1.0 }` (was `1.0`/`0.0`). Every existing test needs its literal
   expected values redone for the new formula/outcome convention, not just re-labeled.
2. **`length_cache.rs`**: `is_trainable`'s f32-saturation guard becomes `probability > -1.0 &&
   probability < 1.0` (was `> 0.0`). The specific saturation thresholds documented in its own doc
   comment (`~2400` cp winning side, `~14980` cp losing side) need to be **re-derived from the actual
   new formula, not assumed to carry over unchanged** -- `tanh` isn't computed by literally composing
   `2 * sigmoid - 1` in code (direct `f64::tanh`), so its own `f32`-rounding behavior needs checking
   fresh, the same way the original thresholds were originally found empirically.
3. **`round.rs`**: `run_cp_check`/`run_calibration_sweep`'s own logic is already convention-agnostic
   (neither hardcodes `0.5` or `[0, 1]` anywhere -- they only ever call `score`/`pawns_from_score`
   through their real names). No functional changes expected there. Their *tests* do need auditing --
   anywhere a fixture uses a literal `0.5` to mean "balanced" or `0.0`/`1.0` to mean "certain loss/win"
   needs updating to `0.0`/`-1.0`/`1.0` respectively.
4. **`main.rs`**: no functional changes expected -- `compute_loss`/`compute_prediction` already just
   pass `label`/the raw prediction through generically (`prediction.mse_loss(batch.label(), ...)`),
   agnostic to whatever scale `label` actually lives in.
5. **`value_head.rs`/`nnue_network.rs`/`transformer_network.rs`**: no functional changes -- the raw
   output's own nature (unbounded, trained by direct `mse_loss` against `label`) doesn't depend on
   `label`'s own codomain. Doc comments referencing `[0, 1]` specifically should be swept for
   consistency.
6. **Naming, an open question, not presumed:** `score`/`pawns_from_score` were named for the `[0, 1]`
   "expected score" framing (a win-probability reading) -- worth asking whether those names should
   change now that the codomain is a signed `[-1, 1]` scale (something like `value`/`pawns_from_value`,
   or keep the existing names since the underlying meaning -- AB(lambda)'s blended judgment of a
   position -- hasn't actually changed, just its numeric range). Deferred to the developer.

### Data/training impact

Same treatment as every earlier data-shape change this session: this changes what `prepare` writes, so
it needs a full cache rebuild and a full reset-and-retrain-from-scratch for any in-progress network
(`ylenia`'s current weights were fit against the old `[0, 1]`-scaled target; there is no way to resume
across a change to what the target itself means).

**Real mistake, corrected during rollout:** originally assumed `positions.cache` could stay untouched
and only `labels` needed rebuilding, on the theory that only the label's numeric *formula* changed.
Wrong -- `is_trainable`'s own filtering *decision* changed too, not just what value gets written for an
accepted ply: the f32-saturation guard's threshold moved from the old `sigmoid`'s asymmetric `~2400`/
`~14980` cp to the new `tanh`'s symmetric `~2500` cp on both sides (the "real finding" a few paragraphs
up). A ply around `2450` cp on the winning side used to fail the old check and now passes; a losing-side
ply between roughly `2500` and `14980` cp used to pass and now fails. `positions.cache` and `labels` are
built by two fully independent PGN replays (`length_cache.rs`'s own module doc comment), each calling
`is_trainable` on its own -- rebuilding only one of the two leaves them disagreeing about which plies
qualify, caught immediately by `LengthCacheReader`'s own row-count sanity check
(`length_cache.rs:147`'s `assert_eq!`) the moment `train` tries to open both together. Both directories
need a full rebuild together, every time `is_trainable`'s own filtering logic changes -- not just when
the label's numeric formula does.

### Explicitly out of scope for this plan

- Actually wiring a trained network into `evaluate()`/negamax search -- this plan only fixes the label
  convention so that a *future* wiring step won't need an extra sign-flip transform; the wiring itself
  is separate, later work.
- The calibration formula's exact shape (what to do about `min`/`max`/`std_dev` once the network is
  retrained under `[-1, 1]`) -- likely simpler once retrained (the network's own center should already
  land near `0`, matching the new label's own center), but that's a follow-up decision made against
  real post-retrain numbers, not part of this plan.

## Plan: convert the raw output to pawns, and check for real learning via Pearson correlation

**Status**: implemented (`labels.rs::clamped_pawns_from_score`; `round.rs::run_cp_check` switched to
it on both sides, fixing the `atanh(±1.0)` divergence on decisive labels; `round.rs::
CorrelationReport`/`run_correlation_sweep`; `rotor-train correlate <network-name> <step>` dispatched
from `main.rs` the same way `check`/`calibrate` are, swept over the test set). Not yet built/tested or
run against a real checkpoint as of this note.

**Real run confirming the `[-1, 1]` switch worked as intended**: `ylenia` rebuilt and retrained from
scratch, best checkpoint so far at step `139264` (`test_loss = 0.09763596`). `calibrate` at that step:
`67,441,385` train rows, `min = -1.6326`, `max = 1.6781`, `midpoint = 0.0228`, `mean = 0.0560`,
`std_dev = 0.1952`. Both `mean` and `midpoint` now sit right at `0`, not `0.5` -- confirming the raw
output really is centering on the label's own true fixed point under the new convention.

### Converting the raw output to pawns

Because `compute_loss` trains the raw output directly (plain `mse_loss`) against a label that already
lives in `[-1, 1]`, the real numbers above show the raw output already lives in the label's own units by
construction -- unlike the abandoned `[-C, C]`-rescale idea under the old `[0, 1]` convention, **no
rescale is needed** here. Converting to pawns is just `pawns_from_score` applied directly, once one
domain problem is handled: `atanh` diverges at `±1`, and the raw output can slightly overshoot the
label's own bound (the real `max = 1.6781` above), so it needs a clamp first.

```
clamped_pawns_from_score(raw) = pawns_from_score(clamp(raw, -(1 - eps), 1 - eps))
```

`eps = 0.01`, chosen from a real tradeoff, not picked as a round number by feel. `atanh`'s derivative is
`1 / (1 - q^2)`, which blows up as `q -> ±1` -- the tighter the clamp, the more a fixed amount of raw
output noise near the boundary gets amplified into a large, likely spurious pawns swing. The achievable
ceiling has diminishing returns going the other way (`atanh` grows only logarithmically near `1`), so
chasing a smaller `eps` for a higher ceiling is a bad trade:

| `eps`   | pawns ceiling | cp of output error from a fixed `0.001` wobble in raw output, right at the boundary |
|---------|---------------|---------------------------------------------------------------------------------------|
| `0.01`  | `~7.64`       | `~14.5cp` |
| `0.001` | `~10.97`      | `~144cp` |
| `1e-6`  | `~17.6`       | `~144,000cp` (meaningless) |

Going from `eps = 0.01` to `eps = 0.001` buys only `~3.3` more pawns of ceiling but makes the conversion
`~10x` more sensitive to ordinary output noise; `1e-6` (reusing `is_trainable`'s own f32-ULP epsilon, an
idea considered and rejected here) is explosively unstable for barely more ceiling. `is_trainable`'s own
`2^-24` constant answers a different question anyway -- when `tanh` rounds to exactly `1.0f32` at label
*storage* time -- not how much noise a trained model's own prediction plausibly carries near saturation,
so reusing it here would have been a category error, not just a bad tradeoff.

This produces a pawns value, full stop -- the `* 100` to centipawns is never part of this conversion,
applied only at whichever call site actually needs a human-readable/UCI-facing number, same convention
as the rest of this document's "Centipawn scaling" section.

**Where it lives, and a real bug it fixes**: `clamped_pawns_from_score` goes in `labels.rs`, next to
`pawns_from_score`. `round.rs::run_cp_check` switches *both* sides of its comparison -- `record.label`
and the network's own raw prediction -- to call it instead of `pawns_from_score` directly. This fixes a
real, previously-unnoticed bug: decisive positions now carry a hard label of exactly `±1.0`
(`labels.rs::compute_labels`), and `pawns_from_score(±1.0)` is `atanh(±1.0)`, literally infinite --
any decisive row in the test set currently poisons `mean_absolute_error_cp`/`max_absolute_error_cp` with
`inf`/`NaN`. Routing the label side through the same clamp fixes this too, not just the prediction side.

### Pearson correlation: does the prediction actually track the label

A low `test_loss` alone doesn't rule out a network that has learned to output something close to a
constant (near the label's own mean) rather than a real function of the input -- that can still look
like a "good" MSE if the label distribution is concentrated. Correlation is the direct test, and it
needs none of the clamp/pawns-conversion machinery above -- it's scale/offset-invariant, so it operates
directly on `prediction` and `record.label`, both already bounded or near-bounded.

New sweep, same shape as `run_calibration_sweep`/`run_cp_check` (one no-grad pass over every row of every
reader, streaming accumulation, `O(1)` memory): accumulate `n`, `sum_x`, `sum_y`, `sum_xy`, `sum_x2`,
`sum_y2` over `(prediction, label)` pairs, then at the end

```
covariance  = sum_xy / n - (sum_x / n) * (sum_y / n)
std_dev_x   = sqrt(sum_x2 / n - (sum_x / n)^2)
std_dev_y   = sqrt(sum_y2 / n - (sum_y / n)^2)
correlation = covariance / (std_dev_x * std_dev_y)
r_squared   = correlation^2
```

`CorrelationReport { row_count, correlation, r_squared }` in `round.rs`, `run_correlation_sweep`
alongside the other two sweeps. Swept over the **test** set, not train (matching `run_cp_check`'s own
existing convention -- this is a question about real generalization, not about what the network was fit
against, unlike `calibrate`'s own deliberate train-set choice). Wired to a new `rotor-train correlate
<network-name> <step>` CLI subcommand, mirroring `check`'s/`calibrate`'s own dispatch shape in `main.rs`.

### Explicitly out of scope for this plan

- Any change to the loss function or the network architecture (e.g. a `tanh` output activation, discussed
  and rejected by the developer for now -- game-value-head precedent like AlphaZero's exists, but the risk
  of vanishing gradient right on the now-included decisive positions wasn't judged worth it yet).
- Wiring the trained network into `evaluate()`/negamax search -- this plan only fixes how a raw
  prediction reads as pawns/centipawns for diagnostic tooling, not for actual play.

**Fixed gap, surfaced by `rotor-train check`'s own real run:** `is_trainable`'s decisive-position
filter (`length_cache.rs`) used to exclude only mate/tablebase-scale scores (`TB_WIN_SCORE -
MAX_PLY` = `31799` centipawns), but `score` cast to `f32` already saturates to exactly `0.0`/`1.0`
far earlier -- around `|cp| > ~2500` (a large but perfectly ordinary material blowout, not remotely
rare in real OTB games heading toward resignation). Positions in that gap kept their label's own
precision destroyed even though they were nowhere near search-decisive. Fixed: `is_trainable` now
also tests `score(cp / 100.0) as f32` strictly inside `(0.0, 1.0)`, the exact same conversion
`compute_labels` itself uses, rather than a second, separately-chosen cp threshold. `score` is now
`pub(crate)` in `labels.rs` for exactly this reason. This changes what `prepare` writes, so any
existing `positions.cache`/labels files and any in-progress training run built against them are
stale and need rebuilding/restarting from scratch -- there is no way to resume a training run
across a data-shape change like this.

**`rotor-train check`'s own `CpCheckReport`, once actually run against real, rebuilt data, also
gained a `baseline_mean_absolute_error_cp` field** -- the trivial "always predict a coin flip"
baseline's own mean `|true_cp|`, computed in the same sweep. Motivated by a real false alarm: an
early `ylenia` (naive NNUE, `lr = 1e-3`) run showed `mean |predicted_cp - true_cp| = 80.5` at step
`32768`, which reads as "pretty good" in isolation but turned out to be only a modest improvement
over the baseline's own `94.8` -- without the baseline printed alongside it, that number alone
can't distinguish "the network learned something real" from "most real positions just happen to
sit fairly close to balanced, so even a do-nothing predictor looks reasonable." With the baseline
printed, `zelda` (transformer) and `ylenia` (naive NNUE, `lr = 1e-3` vs. `zelda`'s `1e-4`) both
show real (if still modest) improvement over their own baselines after rebuilding on the
`is_trainable` fix above -- not evidence of a data/alignment bug, just genuinely slow early
convergence. Decided with the developer: keep training rather than chase this further for now.

## AB(lambda): the per-position label mechanism

Derived from **TDLeaf(lambda)** (Baxter, Tridgell & Weaver 1999, [arXiv:cs/9901001](https://arxiv.org/abs/cs/9901001)
-- the algorithm behind KnightCap), differing in the one property that matters for how it's used
here: TDLeaf(lambda) is *online* (its own `v_{t+1}` at every step comes from the
currently-training weights). AB(lambda) instead gets its leaf values from a fixed teacher:
Rotor's own `alpha_beta_with_buffer` search (`Search(depth)`, any depth -- a property of which
teacher generated the scores, not fixed to `9` any more, see `rotor-transformer-depth0-experiment.md`'s
"Prerequisite fix") evaluated through the existing, non-learning `evaluate()`, never the network
being trained -- removing the self-referential bootstrapping dependency entirely, so every
position's label is a pure function of its game trajectory, computable once, offline, fully in
parallel across every game in the dataset.

**Construction.** Every position `t` in a trajectory (positions `0` to `N-1`, all `N` of a game's
real, scored plies -- `t = 0` the starting position, `t = N-1` the position the game's own last
move is played from) gets its own fixed regression target `L_t`, computed once, backward, from a
virtual, never-scored, never-emitted anchor position `N` (the game's actual final board, whatever
ended it) valued directly at the outcome:

```
L_N = game_outcome                              (virtual anchor, never itself a training label)
L_t = lambda * L_{t+1} + (1 - lambda) * score(Search(depth, pos_t))    for t = N-1 down to 0
```

No hardcoded exception for `t = N-1`: at `lambda = 0` every real label is `score(Search(depth,
pos_t))` with zero exceptions; at `lambda = 1` every real label is `game_outcome`, unchanged, with
zero exceptions either way. (`rotor_curate.rs::play_game` scores the board *before* each move is
made, not after -- see the same "Prerequisite fix" section for why that reindexing is what makes
`position[0]` the game's own starting position, real and scored, rather than never scored at all.)

`L_t` is computed once per position, entirely offline, by `rotor-train`'s own feature-cache
builder -- still just a bounded scalar in `[0, 1]` per position, so the network module and
training loop need no changes to consume it.

**Sign/orientation.** The backward recursion runs in one fixed orientation throughout a
trajectory (White-relative), converting to side-to-move-relative only once, at the final
per-position write.

**Lambda's value.** KnightCap used `0.7`; exposed as a tunable hyperparameter, not hardcoded.

**Every ply is labeled, no sampling** -- `L_t`'s recursion needs `L_{t+1}`, so a sampling gap
would break that assumption.

## Label generation pipeline

Dataset: Lumbra's Gigabase OTB Elite file (both players rated 2400+, over-the-board, CC
BY-NC-SA 4.0), already downloaded to `data/lumbra/`. Raw moves and results only, no embedded
computer-evaluation annotations -- historical fact, not another engine's implementation, so
doesn't conflict with CLAUDE.md rule 1.

The search: `Search(9)`, fixed depth, no stability test, no time budget (an odd depth, to
sidestep the Odd-Even Effect -- adjacent-depth score oscillation unrelated to genuine
improvement).

Full pipeline engineering (parallel workers, transposition-table handling, resume/restart,
Ctrl+C behavior, SQLite schema) is `rotor-curate`'s own concern, specified in
`centipawn-calibration.md`. What matters for AB(lambda)'s own correctness: the transposition
table stays warm across one whole game, and every ply of every game has its raw `Search(9)`
score captured, no per-game sampling -- `L_t` is computed downstream by `rotor-train`'s own
feature-cache builder, reading those raw scores back per game.

`Search(0)` (a single `evaluate()` call) is exactly a neural network's forward pass in
chess-engine vocabulary -- the thing actually being trained, cheap enough to run at every leaf of
Rotor's real search during play, unlike `Search(9)`, which is only affordable offline during
label generation.

## Quiet-position filtering

**Reverted** (see `curate-drop-pv-quiet.md`): the tactically-live check below (`plies.quiet`,
`is_position_quiet`) was removed from `rotor-curate`/`rotor-train` entirely, along with
`preprocessing.pv`. Training-data admissibility is decisive/score-window only now
(`length_cache.rs::is_trainable(cp: i32) -> bool`); the section below describes the design as it
stood before that reversion, kept for history.

Training should only ever happen on positions `evaluate()` (`Search(0)`) would actually be asked
to score during real play. Two ways a row can fail that:

- **Tactically live.** `plies.quiet` (`rotor-curate`'s own schema, `rotor_curate.rs`) is `false`
  whenever the search's own `best_move` at that position is a capture or promotion
  (`is_position_quiet`) -- the position is about to change materially in a way a bare, one-ply
  `evaluate()` call can't see coming.
- **Decisive.** A `Search(depth)` score that's mate-scale (or, once real tablebases are wired into
  curation, tablebase-scale) is never actually backed by an `evaluate()` call in the first place:
  `quiescence`'s stand-pat call to `evaluate()` is bypassed entirely for a checkmate/stalemate
  terminal node (scored by ply directly) and for a tablebase hit (`tablebase.probe`, checked
  *before* stand-pat). So during real play the network is never asked to evaluate a position whose
  true value is "forced mate" or "tablebase win/loss" -- training against such a label would push
  it toward outputs it will never actually be asked to produce.

**No second database column for the decisive case** -- `plies.score` already carries everything
needed; `prepare` tests it directly rather than `rotor-curate` persisting a redundant flag.
`rotor::search::TB_WIN_SCORE`/`MAX_PLY` (both already `pub`) give the one threshold that catches
both bands at once, since the tablebase band sits strictly below the mate band
(`TB_WIN_SCORE`'s own doc comment): `score.abs() >= TB_WIN_SCORE - MAX_PLY as i32`. Correct
already for a future tablebase-enabled `rotor-curate`, with no further change needed then.

**Status**: implemented. `length_cache.rs::is_trainable(quiet, score)` gates the write inside both
`build_positions_cache`'s and `build_labels`'s own per-ply loop -- both now take the stem's own
`Connection` (`build_positions_cache` didn't before) and read `plies.quiet`/`plies.score` per game
via the new `dataset::{PlyRow, prepare_game_plies_statement, read_game_plies}`. `compute_labels`
itself still runs over every ply's raw score, filtered or not -- AB(lambda)'s backward recursion
needs the full, unfiltered sequence; only the resulting `(position, label)` *write* is skipped.

Fixing this surfaced a real, pre-existing, unrelated bug in the same replay loop: both builders
captured each position *after* playing its own move rather than before, silently dropping every
game's starting position and substituting its true, never-scored terminal board in its place --
fixed in the same change (move `extract_canonical` before `make_move`, not after). Regression test:
`build_positions_cache_captures_the_position_before_its_own_move_not_after`.

Build and full test suite verified passing.

### Generalizing `pv > 1`: checking further into the PV, not just its first move

**Reverted** (see `curate-drop-pv-quiet.md`): `preprocessing.pv` and the whole quiet-depth check
this section describes were removed. Kept for history.

**Status**: implemented (`rotor_curate.rs::is_position_quiet`/`play_game`/`run_worker`/
`run_generate_core`/`run_generate`; the `pv <= 1` guards dropped from both `rotor_curate.rs::
run_generate` and `main.rs::run_prepare`) and confirmed via `cargo build`/`cargo test`.

Motivated by wanting to isolate `training.md`'s own open theory (`Search(0)` may exhaust patience
faster than `Search(9)`, since a depth-0 label has no minimax backing-up at all) -- testing it needs
a real `depth > 0` config, and the developer wants `pv: 2` for it, not `pv: 1`. Checked directly
against Stockfish's own precedent first (`gensfen`'s `depth 9` cited as its own "sweet spot",
matching Rotor's existing `Search(9)`, not `6`; the odd-even effect confirms why `Search(9)`'s own
depth is deliberately odd) -- see this session's own research for the sources.

`is_position_quiet` (`rotor_curate.rs`) has always taken a `pv` value in `preprocessing.pv` (baked
into every generated database's/cache's own filename via `preprocessing_config::generation_suffix`),
but never actually used it to decide how far to check -- it only ever tested the single first move
of the PV, regardless of what `pv` was configured to. A hard `assert!(pv.unwrap_or(0) <= 1, ...)` in
both `rotor_curate.rs::run_generate` and `main.rs::run_prepare` refused anything else outright.
`search::is_quiet`'s own doc comment already anticipated the real design ("also reused by
`rotor_curate.rs`'s own *position*-level quiet check... to ask the same *move*-level question about
a PV's own first two moves"), and an example config (`examples/net_transformer.yaml`) already has
`pv: 2` committed, unused until now.

**The fix**: `is_position_quiet(board: &Board, pv_line: &[Move], pv_depth: u32) -> bool` replays up
to `pv_depth` moves of the PV (`search::IterationReport::pv`, already returned by `choose_best_move`
via `reports.last()` -- `best_move` was always just `reports.last().pv[0]` under a different name,
so no new search call is needed), checking `search::is_quiet` fresh at each successive position,
short-circuiting to `false` the moment any one of them is a capture/promotion. `pv_depth == 0` or an
empty `pv_line` (the `depth == 0` case, which has no PV at all) is unconditionally quiet, matching
today's behavior exactly. `pv: None`/`Some(1)` (every existing config) resolves to `pv_depth = 1` at
the `play_game` call site, checking only the first move -- byte-for-byte the same behavior as before
this change, for every config that hasn't opted into a deeper check.

**Threading**: `pv: Option<u32>` flows `run_generate` -> `run_generate_core` -> `run_worker` ->
`play_game` -> `is_position_quiet`, resolved to a plain `u32` depth count (`pv.unwrap_or(1)`) once,
right at the `play_game` call site inside `play_game` itself.

**Both `pv <= 1` guards are removed**, not loosened to a new, still-limited ceiling -- there's no
longer any reason for one: `is_position_quiet`'s own `pv_depth.min(pv_line.len())` clamp already
handles a `pv_depth` deeper than the PV actually reaches, so no `pv` value can misbehave.

**Tests**: direct, isolated unit tests against `is_position_quiet` itself -- a hand-built `Board`
plus a hand-picked `pv_line: Vec<Move>`, not a `run_generate_core`-level integration test, since
guaranteeing a specific real PV out of a real search isn't something to rely on deterministically.
Cases: `pv_depth = 1` against a PV whose first move is quiet but second is a capture (still reports
quiet, matching today); `pv_depth = 2` against the identical PV (now reports not quiet); `pv_depth =
2` against a PV that's quiet two moves deep (reports quiet); `pv_depth` deeper than the PV's own
length (checks only what's actually there, doesn't panic); `pv_depth = 0`/an empty PV (always
quiet).

`data/networks/amanda/net.yaml`: `kind: naive`, matching `ylenia`'s own architecture/hyperparameters
(this run's whole point was isolating the depth/pv effect, not also varying architecture) --
`depth: 9`, `pv: 2`.

**Real gap found after implementing this**: checking only the single chosen best line's own next
`pv_depth` moves can't see a nearly-as-good tactical alternative sitting right behind it -- every
other root move is only ever "refuted, worse than X" by a narrow-window scout search, never given
its own exact score. Fixing this needs real MultiPV search (`multipv.md`, its own document -- a
genuine engine/search feature, not training-specific). The extra root candidates to check for this
are driven by `preprocessing.pv` itself, the same number `pv_depth` already resolves to -- not a
separate `K`, after a first version that tried exactly that got corrected (`multipv.md`'s own
"Consumer" section has the full story). Implemented and confirmed alongside `multipv.md`'s own
MultiPV engine feature.

**`amanda` abandoned**: `pv: 2` (both the depth-into-rank-1's-line check and the extra root
candidate) roughly doubles `generate`'s own throughput cost, a second independent full-width
`search_root` call at every `depth > 0` position. First surfaced as `amanda`'s own ETA running ~3-4x
longer than `betty`'s (`pv: 1`, otherwise identical) -- but that comparison was misleading on a first
pass: a bug (see `multipv.md`) meant `betty` was paying the same extra `search_root` call `amanda`
was despite its own `pv: 1`, so `betty`'s early runs were slow too, for the same reason, not a
separate one. Fixed now: `pv: 1` truly means the old single-search behavior again. Regardless, `pv: 2`
itself is still a real ~2x cost when it does apply, not worth the wait for generation 0 -- `betty`
(`depth: 9`, `pv: 1`) is the first network actually being generated for strength and neural-network-
convergence testing. `amanda`'s own partially-built database is left on disk, unused, not deleted.

## Generation 0 training loop

Scoped to generation 0 specifically; later generations may revisit any of these.

**Training shape**: no epochs. Training reads `train_cache` (`rotor-train`'s own feature-cache
builder produces it, game-level train/test split, reading directly from `rotor-curate`'s SQLite
database) sequentially, cycling back to row `0` once it runs out, via one advancing cursor
(`reader::CacheFile::read_batch_cyclic`) -- no RNG, no seed, nothing to reproduce on resume, just
an integer to persist. Held-out test loss is measured every `STEPS_PER_EVAL` (`8192`) training
steps. Loss is a (soft) cross-entropy between the network's own sigmoid output and the
recentered `[0,1]` label.

**Optimizer**: AdamW. The rate is `net.yaml`'s own `training.learning_rate`, not a hard-coded default
and no longer the `--lr` flag this paragraph described: it takes a list, and one `rotor-train train`
trains every rate in it as a lockstep population (rotor-train-learning-rate-grid.md). That subsumes the
short grid-search phase this paragraph used to describe -- a handful of candidate values from the same
starting checkpoint -- by running it every generation instead of once. No automated decay schedule
either way. **Batch size**: `4096` (`reader::BATCH_SIZE`) -- a starting point, not Stockfish's own
`16384` (tuned against a corpus of billions of positions; at Rotor's real scale, tens of
millions, the same batch size would leave too few steps per genuine full-corpus epoch).

**Run shape**: a single long-lived process (`main.rs`'s `run_train`) loops over evaluation
windows internally -- `tch`/`libtorch` loads exactly once per invocation, not once per window.
Resilience without restarting the OS process: the same `ctrlc`-crate pattern `rotor-curate
generate` uses (a shared flag a `Ctrl+C` handler sets once) is checked between windows, never
mid-window, so a stop always leaves a complete, consistent window's worth of checkpoint behind,
and rerunning the same command resumes correctly. A checkpoint is saved every window regardless
(cheap at this network's scale, comparable to Leorik's 768-input/256-wide net).

**Periodic mid-window checkpointing.** `window::run_window` takes a `checkpoint_interval:
Duration` and an `on_checkpoint` callback, checked (wall-clock, via `Instant`) only on training
steps, never the no-grad test pass; `main.rs` wires this to a 5 minute interval that re-saves the
same "latest" `model.vs`/`optimizer.opt` pair the window boundary itself saves -- a resume after
a crash mid-window loses at most 5 minutes of progress.

**Resuming an interrupted window.** The periodic checkpoint also persists `window_progress.state`
(`window_progress.rs`): the training stream's own current row cursor, plus the in-progress
window's running `(steps_done, loss_sum)` as of that checkpoint. There is no seed to
regenerate -- a cyclic sequential read has nothing to reproduce, only a position to remember.
`main.rs` resumes reading from `cursor`, runs the window's remaining `STEPS_PER_EVAL -
steps_done` steps, and seeds the accumulator from the persisted `loss_sum`, so the window's own
final `train_loss` is the correct weighted average over the whole window.
`window_progress.state` is rewritten unconditionally at the end of every window and
periodically mid-window -- no "in progress or not" distinction, the cyclic stream always has
*some* current position to persist. Proven directly: `window::tests::resuming_an_interrupted_
pass_reaches_the_same_final_loss_as_one_uninterrupted_call` splits a real multi-step pass across
two calls and checks the final loss matches an uninterrupted call within float noise. The
no-grad test pass has no equivalent resume mechanism (cheap, never mutates a weight worth
preserving).

**Stopping condition**: early stopping, patience `10`, on held-out test loss. Tracked by
`early_stopping.rs`'s `EarlyStopping` (best loss so far, the step it happened at, steps since
improvement), persisted alongside model/optimizer checkpoints (`early_stopping.state`) so a
`Ctrl+C`-then-resume continues the exact same patience window. `should_stop` is checked once,
immediately on loading a checkpoint, before the training loop runs anything.

**Candidate selection.** Every evaluation window's own weights are archived unconditionally
(`checkpoint-dir/checkpoints/step_NNNNNNNNN.vs`, weights only, no optimizer state), and every
window's own train/test loss is appended to a plain per-run log (`checkpoint-dir/history.tsv`,
`history.rs`), rather than tracking only a single running "best" checkpoint -- decoupling
archiving (cheap, unconditional) from candidate selection (choosing the best `N` checkpoints by
recorded test loss to enter the strength-comparison bracket below). The "latest" checkpoint
(`model.vs`/`optimizer.opt` at `checkpoint-dir`'s own root) exists separately, used only to
resume training. **The tool that reads `history.tsv`, picks the best `N`, and feeds those
checkpoints into the tournament below is not yet built.**

**Strength comparison**: superseded, see `rotor-evolve.md` -- this used to describe a
double-elimination bracket with a per-match SPRT test (`alpha = beta = 0.05`, fishtest's own
method); revisited directly with the developer and simplified to a single-elimination bracket
(seeded by the selected candidates' own recorded test loss ranking) with a World-Championship-
style decision rule per pairing (first to 6.5 of 12 games at a base control, tiebreak pairs at a
faster control if still tied) instead of SPRT. The bracket's winner becomes generation 0's
champion checkpoint. A periodic reconciliation tournament among past generations' champions is
flagged as a reasonable future safeguard, not yet needed with only one generation in progress.
**`rotor-evolve`, the tool that will actually play these matches, is not yet built** -- and
neither is the runtime network-selection mechanism candidate checkpoints would need to actually
be registered as distinct engines (`rotor-evolve.md`'s own open items).

**Weight handoff**: a custom flat binary format, not `tch-rs`'s own `VarStore` format (CLAUDE.md
rule 7 forbids linking `torch` into the shipped binary). The training binary extracts every
parameter tensor to a flat `Vec<f32>` in a fixed, documented order; the Rust side reads it with
plain `std::fs`. **Checkpoint format**: a short, self-describing header (magic bytes, format
version, each layer's shape, the step/generation number) precedes the raw weight bytes, so the
Rust loader can assert shapes match rather than silently misread a mismatch. The exact byte
layout within the flat format is not yet fixed -- a mechanical detail for whenever the Rust
inference loader is actually written.

**Two separate per-checkpoint artifacts**: a native `tch-rs`/`VarStore` checkpoint (weights plus
AdamW's own optimizer state, read/written only by the training binary, needed to resume training
without a cold, unadapted optimizer) and the self-describing flat format above (weights only, for
`rotor-evolve` and the shipped engine's inference side, neither of which needs optimizer state).

**AdamW is hand-rolled (`adamw.rs`), not `tch`'s own built-in `nn::AdamW`.** `tch`'s built-in
optimizer wrapper has no `state_dict` equivalent anywhere in its FFI surface, so it has no way to
save or restore its own internal moment estimates. The fix: reimplement AdamW's update rule
directly over tensors this project owns (`m`/`v` per parameter, a step counter), serialized via
the same primitives `VarStore::save`/`load` already use internally
(`Tensor::save_multi`/`load_multi`) -- full weights-plus-optimizer-state checkpointing.
Cross-checked step by step against `tch`'s own built-in `nn::AdamW` on an identical parameter and
loss (CLAUDE.md rule 7's "never assumed to agree" discipline), plus a save/load round-trip test
proving a restored optimizer reproduces the exact next update an uninterrupted run would have
made.

**Feature cache**: precomputed once, disk-backed (the training binary is a fresh process every
invocation, so an in-process cache alone would rebuild from scratch), storing the occupant
encoding as sparse indices rather than dense one-hot floats. A flat, fixed-offset binary file,
not a database -- nothing here needs filtering, joins, or arbitrary indexing, only "read row `i`'s
feature vector by index," `i` itself an advancing, wrapping cursor
(`reader::CacheFile::read_batch_cyclic`) rather than a freshly reshuffled permutation.

## Open questions

1. **Self-play vs. online play.** KnightCap's own headline result leaned specifically on learning
   from real human opposition on FICS, not self-play; Rotor's own constraint (training labels
   must come from Rotor's own search, rule 1) makes self-play the only realistic source of volume
   here. Whether AB(lambda)'s mechanism still offers something over a simpler approach without
   that ingredient is unresolved.
2. **Validating the generation-0 numbers.** Learning rate, batch size, the patience threshold,
   and the evaluation cadence (`STEPS_PER_EVAL`) are all reasoned starting points, not measured
   ones -- need real profiling once training actually runs, per CLAUDE.md rule 5.
