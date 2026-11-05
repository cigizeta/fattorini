# Stacked lockstep arms: every lambda of a generation as one batched network

Agreed 2026-10-11. Status: implemented, steps 1-8 done; betty gen-0002 finished on it. Ready to move
to `done/`. Step 6's comparison is against a slice of `Trainee`s through the same `run_round`, the old
code's per-arm behaviour, since the old lockstep body is gone. Betty gen-0002
switches to the stacked code mid-run, from a backup copy (`gen-0002.backup`). Supersedes the "not needed for psq" item
in TODO.md and lockstep-lambdas.md's "Not this plan" paragraph, both of which assumed the GPU would
not be the bottleneck once batches were shared.

## Why

Lockstep (lockstep-lambdas.md) builds each batch once, but every arm still does its own forward pass,
backward pass and AdamW step, one after another, from the main thread. For psq at batch 4096 every
kernel is tiny, so the time goes on launching them, not on running them.

The histories show it. Train seconds per round against the number of arms still training:

| run | arms | train s/round |
|---|---|---|
| amanda gen-0002 | 9 | ~22 |
| amanda gen-0002, only 0.95 left (from step 157696) | 1 | ~4.4 |
| betty gen-0002 | 11 | ~35 |

That is about 2.2 s per round shared (batch building included) plus 2.2 s per arm for amanda and
3.0 s per arm for betty. Betty's extra 16-wide hidden layer adds next to no arithmetic but about 35%
per arm: the signature of per-kernel overhead. 1024 steps x 11 arms in 35 s is about 3 ms per
arm-step, for roughly a hundred kernels (forward, backward, and twelve-odd elementwise operations per
parameter tensor in `AdamW::step`).

The arithmetic itself is small. A psq arm-step is about 10 GFLOP (the dense `[4096, 780] x [780, 256]`
transform, both perspectives, forward and backward), so eleven arms are about 110 TFLOP a round: a
few seconds on the training machine's GPU. Stacking should take betty's round from about 35 s to well under 10 s;
the measurement in step 7 says how far.

## Design

### Scope

The NNUE track's lockstep, all four kinds (psq, psq_threats, halfkp, halfka_v2_hm): they share one
shape, a transform applied to both perspectives, concatenated, then a value head, differing only in
input width and activation. The transformer track keeps its single unstacked network.

### The stacked network

A `StackedNetwork` holds one tensor per parameter with a leading arm dimension `A`, the arms
currently training, under the same names `Network` registers today:

| name | stacked shape |
|---|---|
| `transform.weight` | `[A, E, In]` |
| `transform.bias` | `[A, E]` |
| `value_head.hidden<i>.weight` | `[A, out, in]` |
| `value_head.hidden<i>.bias` | `[A, out]` |
| `value_head.output.weight` | `[A, 1, in]` |
| `value_head.output.bias` | `[A, 1]` |

Forward, for a batch whose features `x` (`[B, In]`, one per perspective) every arm shares:

- **Transform: one matmul for every arm.** `x.addmm(bias.view([A*E]), weight.view([A*E, In]).t())`
  gives `[B, A*E]`, viewed `[B, A, E]` and permuted to `[A, B, E]`. The input is shared, so no
  `bmm` is needed here, and this is the largest matmul of the step.
- **Activation and concatenation** as today, on the last dimension: `[A, B, 2E]`.
- **Value head: `baddbmm` per layer.** `baddbmm(bias.unsqueeze(1), h, weight.transpose(1, 2))`,
  then the activation, through to the output: `[A, B, 1]`, squeezed to pawns `[A, B]`.

The labels are the batch's `[B, arms]` matrix, its columns selected for the active arms and
transposed to `[A, B]`. `matching_loss` and the test MSE take a mean over `B` only, giving one loss
per arm, `[A]`.

**The objective is the sum over arms, not the mean.** Each arm's parameters reach only its own loss,
so the gradient of the sum on arm `k`'s slice is exactly the gradient of arm `k`'s loss alone, what
it gets today. A mean would divide every arm's gradient by `A`.

### The stacked optimizer

A `StackedAdamW` beside today's `AdamW` (the transformer keeps that one), the same update written
over stacked tensors. Every operation in AdamW is elementwise, so arms cannot leak into each other;
what has to become per arm is everything that is a scalar today:

- **Rate, warmup and bias correction per arm.** Arms change stage at different rounds. The step
  count `t` is a `[A]` tensor on the device. Each step computes, on the device, the effective rate
  `lr[a] * warmup(t[a])` and the corrections `1 - beta^t[a]`, broadcast as `[A, 1, ...]`. This is a
  handful of kernels per step, not per arm. `lr` is a `[A]` tensor, rewritten only when an arm
  changes stage.
- **Weight decay** stays one scalar, as today.
- **Bounds** (`FIRST_HIDDEN_WEIGHT_BOUND`) are a clamp of the whole stacked tensor: elementwise, so
  the same as clamping each arm.

### Arms that stand at different step counts

Today an arm further ahead than the stream skips batches until the stream reaches it
(`round::run_training_steps`). The stack keeps this rule as a per-arm mask `[A]` of the arms that
step on this batch:

- the objective is `(loss * mask).sum()`, so a skipping arm gets a zero gradient;
- the update writes `where(mask, new, old)` for parameters, `m` and `v`, and `t += mask`.

Normally every arm steps and the mask is all ones. The alternative, requiring equal counts on resume,
was rejected: a crash between two arms' backups leaves them one backup interval apart, and nothing
could roll the ahead arm back to the others' count.

### An arm that finishes leaves the stack

At the round boundary where an arm finishes, the stack is rebuilt without it: `index_select` on dim
0 of every parameter, `m`, `v`, `t` and `lr`, and the active label columns updated. So the cost per
round follows the arms still training, as today. Rebuilding happens at most once per arm per run and
copies a few MB.

### Files stay per lambda

Nothing on disk changes. Every write cuts the arm's slice out of the stack and every read puts it
back:

- `model.vs` and `checkpoints/step-N.vs`: `Tensor::save_multi` of `select(0, k)` of each parameter,
  under its unstacked name. That must be byte-for-byte what `VarStore::save` writes for an unstacked
  network, which step 1 verifies.
- `optimizer.opt`: `m.<name>`, `v.<name>` and `t`, slices of the stacked moments and arm `k`'s
  count, in today's format.
- `history.tsv`, `round_progress.state`, `run_config.txt`, `candidate.*`: unchanged, per arm.

So `export`, `select`, `promote`, `check`, `calibrate` and `correlate` see nothing new. **A run
started under today's code resumes under the stacked code**, betty gen-0002 included, and the reverse
also works.

Starting weights load into every slice from the same file, as `seed_network` does today for each arm.

### The round loop

`round::run_round` serves the transformer (one arm) and NNUE lockstep (many). Today it drives a
`Vec<Trainee>`, one optimizer per arm. It becomes generic over something that trains a set of arms:
it reports the arms' step counts, steps a batch under a mask and returns the losses as a `[A]` tensor
left on the device, computes test losses as `[A]`, and backs up each arm. The stacked network
implements it for NNUE. A one-arm wrapper around today's `Network`/`AdamW` implements it for the
transformer. The loop's logic (the stream at the lowest count, `completed`, backups, the losses read
only at backups and boundaries) stays as it is.

### Not in this plan

- **Sparse input** (`embedding_bag` over the active indices instead of a dense matmul): the dense
  transform is 96% zeros, so it would cut the arithmetic stacking leaves. Only worth it if step 7
  shows the stacked step is limited by arithmetic.
- **TF32 matmuls**: one switch, but it changes numerics; measure separately.
- **libtorch's fused AdamW**: its rate is one scalar, not one per arm.

## Implementation steps

Each step its own commit, tests passing.

1. **Per-arm files from slices.** Functions that save arm `k`'s slice of a set of stacked named
   tensors as a `.vs`, and load a `.vs` into slice `k`. Test: a slice saved this way loads into an
   unstacked `VarStore` via `VarStore::load` and equals the original weights, and an unstacked
   `VarStore::save` loads back into a slice exactly.
2. **`StackedNetwork`** for the four kinds, built from `A` and the architecture. Test: with each slice
   loaded from an independent unstacked `Network`, the stacked pawns and per-arm losses equal each
   network's own, within float tolerance (the matmul's summation order differs, so not bitwise).
3. **`StackedAdamW`**, with per-arm rate, warmup, count, mask and bounds, saving and loading per-arm
   `optimizer.opt`. Tests: `A` independent `AdamW`s and one `StackedAdamW` over the same parameters
   and gradients agree after several steps, with different rates per arm and one arm masked for some
   steps; a masked arm's parameters, moments and count do not move; save and load round-trip per arm.
4. **Removing an arm from the stack.** Test: after removing arm `j`, the others' next steps equal
   those of a stack never containing `j`.
5. **`run_round` over a set of arms**, with the one-arm wrapper for the transformer. Its existing
   tests (an ahead arm skipping then joining, backups, completion) move to the new interface and keep
   passing.
6. **`train_lambdas_in_lockstep` on the stack**: `open_arm` loads each arm's files into its slice;
   history, stages, export and the switch and finish lines as today; an arm that finishes leaves the
   stack. Tests: two arms stacked for a few rounds end with the same weights, losses and histories,
   within tolerance, as two arms under today's code on the same batches; a run interrupted and
   resumed ends as an uninterrupted one.
7. **Measure on the training machine**: betty's train seconds per round before and after, at the same number of
   arms, nothing else running. Record the numbers here.

   Measured on betty gen-0002 (2026-10-11), eleven arms, from `history.tsv`: the per-arm code took
   38.5 to 42.5 s of training and about 6.8 s of test a round (rounds 133-137); stacked, 12.55 to
   12.85 s and about 5.8 s (rounds 139-162), 3.2 times faster in training, with the GPU at 90%. As
   arms finished and left the stack, a round fell to about 9.85 s + 4.5 s, then 6.6 s + 3.0 s. The
   GPU being the limit now, the next speed-up is less arithmetic: the sparse transform (TODO.md).
8. Delete what step 6 replaced; update lockstep-lambdas.md's "Not this plan" paragraph and the TODO
   item with a pointer here.

## When

Betty gen-0002 was stopped at step 140368 and copied to `gen-0002.backup` (2026-10-11); the stacked
code resumes it once step 6 passes. Until then the current code can resume it at any time.
