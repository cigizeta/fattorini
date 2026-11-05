# `labels: lc0`: freeze and forge on Lc0's corpus

Implementation plan, 2026-10-17, drafted for the developer to review. Decision 4 of lc0-data.md:
amanda-lc0 and caroline-lc0, the same architectures as amanda and caroline, trained on
`data/lc0` (lc0-converter.md) instead of a teacher's self-play.

## The idea: `lc0` as a source, generation 1 forever

Everything downstream of forge (rounds, patience, candidates, `arena select`, its gate against the
hand-crafted evaluation for a generation 1, `promote`) already works for any network. What differs
is only where the games come from. So:

- **`lc0` is a source** beside `hand-crafted` and `<network>/gen-NNNN`: its corpus root is
  `data/lc0`, where a teacher's is `data/self/<source>`. `train/`, `test/` and `frozen/` sit under
  it the same way.
- **An Lc0 network has only generation 1.** It starts from random weights (`initial.vs`), as every
  generation 1 does, and has no teacher: forge takes the source `lc0` from `labels: lc0`, not from a
  `teacher.txt`. `smithy teacher` already refuses `labels: lc0`.

## Changes

1. **`preprocessing_config`:** `corpus_root(source)`, `data/lc0` for `lc0`, `data/self/<source>`
   otherwise; `self_play_train_dir` and `self_play_test_dir` built on it. `LC0_SOURCE = "lc0"`.
2. **`smithy freeze lc0 <network>`:** `run` takes `lc0` as the teacher argument, its source `lc0`,
   its test-games threshold `lc0.yaml`'s `test.games` (not `self.yaml`'s); `frozen_dir` uses
   `corpus_root`. Everything else unchanged: the same reservoir, seed and files, in `data/lc0/frozen/`.
3. **forge under `labels: lc0`:** `dataset_source` returns `lc0` without reading `teacher.txt`;
   `self_play_inputs` (renamed `streamed_inputs`) serves both `self` and `lc0`, with two differences
   for `lc0`: no teacher to fetch, and in place of `self.yaml`'s `train.positions` the check below.
   Both `labels: self` branches of `run_train_generation` and `train_lambdas_in_lockstep` take `lc0` too.
4. **The corpus must be complete before forge starts.** `run_config.txt` freezes `train_rows`, the
   epoch's positions, and a resumed run with another count is refused, so training must not start
   while the converter is still adding tars. `smithy lc0`, when it stops at the cap, writes
   `data/lc0/complete.txt` (the positions and segments it holds); forge under `labels: lc0` refuses
   to start without it, naming `smithy lc0`. Raising the cap later means a new corpus: delete
   `complete.txt` by hand, and train new networks.
5. **Configs:** `configs/amanda-lc0.yaml` and `configs/caroline-lc0.yaml`, copies of amanda's and
   caroline's with `labels: lc0`, the lambda grid below, and a comment on where they come from.
6. **Messages:** where forge says "play them with `arena self <player>`", under `lc0` it says
   "convert them with `smithy lc0`".

## Tests

- `corpus_root`: `lc0` and a teacher's source.
- `freeze`'s source for `lc0`, and the refusal of `arena self lc0` (Teacher::parse refuses it, as
  today).
- `amanda-lc0-0001` parses as network `amanda-lc0`, generation 1 (`Teacher::parse` splits at the
  last hyphen); the committed configs load with `Labels::Lc0`.
- The converter writes `complete.txt` exactly at a cap stop.

## Order of use

```
bin/smithy freeze lc0 amanda-lc0        (now: the test set is complete)
bin/smithy freeze lc0 caroline-lc0      (adds nothing if the lambdas are the same)
... smithy lc0 reaches the cap, writes complete.txt ...
bin/smithy forge amanda-lc0
bin/smithy forge caroline-lc0
bin/arena select amanda-lc0             (round robin, margin, gate against hand-crafted)
```

## Decided (the developer, 2026-10-17)

- **The lambda grid: `[0.10, 0.25, 0.50, 0.75, 0.90, 1.00]`,** wide on purpose: Lc0's `best_q` comes
  from a search far stronger than Rotor's depth 9, so the optimum may weigh it much more than
  self-play's 0.95 to 1.00 did. 0.10, not the proposed 0.0: a lambda must be in (0, 1]
  (`validate_lambdas`, and `parse_network_name` for a player at that lambda). 1.00 stays the canary.
- **The names: `amanda-lc0` and `caroline-lc0`.** Checked: every parser of a player or teacher name
  splits at the last hyphen (`Teacher::parse`, `match_player::parse_network_name`), so
  `amanda-lc0-0001-0.50` is network `amanda-lc0`, generation 1, lambda 0.50.

## Implemented (2026-10-17), `cargo test -p smithy` passing on the training machine

Lc0's test set frozen the same day (`smithy freeze lc0 amanda-lc0`, then `caroline-lc0`, the same
grid), in `data/lc0/frozen/`. Training waits for `complete.txt`.

As above, with `streamed_inputs` (was `self_play_inputs`) creating `gen-0001/` for an Lc0 network,
which has no `teacher.txt` fetch to do it, and refusing any generation but 1.
