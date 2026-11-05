# Per-lambda training: one run and one candidate per lambda of `label.lambda`

## Status

Done: both binaries' tests pass, except `rotor-arena`'s `the_committed_gate_sections_load`, which
fails until the ladder replaces the gate. Not yet run on a GPU. The ladder that chooses among the candidates (`rotor-arena ladder`) is the next
step, not this one; see "Contract with the ladder" for what this step assumes of it.

## Decided with the developer

- **(a) One layout for every network**, a one-value grid included: every lambda's run lives in
  `gen-NNNN/lambda-<l>/`. Amanda gets `gen-NNNN/lambda-0.7/`; a run of amanda's made before this change
  (files directly in `gen-NNNN/`) is moved into `lambda-0.7/` by hand once, or retrained.
- **(b) `rotor-train train <network>` trains every lambda in turn**, in the grid's order, each to its
  own stopping rule. No `--lambda`: one GPU runs them one after another anyway. Superseded on
  2026-10-03: every lambda now trains in lockstep over one shared stream, see lockstep-lambdas.md; the
  layout and each lambda's own run, stop and candidate are unchanged.
- **(c) Naming**: each lambda's run exports its **candidate**; the ladder's winner is the
  generation's **challenger**, the one that meets the champion.

## Layout

```
data/networks/<network>/gen-NNNN/
  initial.vs                       gen-0001 only: the one random initialisation every lambda starts from
  lambda-0.65/                     one training run, the files a generation directory held until now:
    run_config.txt  model.vs  optimizer.opt  history.tsv  round_progress.state  checkpoints/
    candidate.nnue  candidate.txt  its best checkpoint, exported when the run stops; .txt last
  lambda-0.7/ ...
  ladder.txt                       the ladder's results and its winner (next step)
  verdict.txt                      the final duel's verdict, pass or fail (next step)
  champion.nnue/.vs/.txt           on promotion, from the winning lambda's candidate
```

- Test losses, the best checkpoint and checkpoint pruning stay per run.
- Every lambda starts from the same weights: `gen-0001/initial.vs`, or the previous champion's
  `champion.vs`. The first lambda to start writes `initial.vs`; the others load it.
- `run_config.txt` is per run and freezes its lambda.

## `train`

1. Refuse, as today, once the generation has finished training: every lambda has a candidate. What
   comes next is then the ladder, `promote`, or nothing (`generation::training_state_check`).
2. For each lambda in grid order: skip it if its candidate exists; otherwise check its labels are
   built, train it to its stop (resuming its backup), and export its candidate.
3. Ctrl+C stops the lambda in progress; the rerun resumes it and moves on.

## `check`, `calibrate`, `correlate`

`rotor-train check <network> <step> [--lambda <l>]`: `--lambda` is required when the grid holds more
than one value, and refused when it names a value the grid lacks.

## Contract with the ladder

- The ladder reads each lambda's `candidate.nnue`, and writes `ladder.txt` with a `winner<TAB><l>`
  line among its results, then `verdict.txt`.
- `promote` requires `verdict.txt` to say `pass`, reads the winner from `ladder.txt`, copies that
  lambda's candidate checkpoint into `champion.*`, and records `lambda` in `champion.txt`.
- Until the ladder exists, `rotor-arena gate` gates the only candidate of a one-value grid and writes
  `ladder.txt` naming it; a grid is refused.
