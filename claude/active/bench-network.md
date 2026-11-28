# bench-network: a bench of any network file

**Why (2026-11-28):** both dual networks, the first trained under `unit: elo`, scored ahead of their
`lc0` twins at depth 9 and behind them at 10+0.1 (trials 22 to 25). The suspect is the aspiration
window: `aspiration_delta`, 10 untuned, is not translated by the pawn scale
(`SearchParameters::translated`), so on an `elo` network, whose pawn reads about 2.3 times larger,
it is about 2.3 times narrower. That would mean more re-searches and more nodes a depth. To measure
it, the nodes and nodes/s at a fixed depth are needed for any network file, with its untuned
parameters, and again with the window overridden. `fattorini bench` benches only the embedded
network.

## The command

```
bin/fattorini bench-network <network> [<depth>] [--aspiration <delta>]
```

- `<network>`: a network file's path, loaded by `Network::load`, its tuning read beside it as the
  engine does (`default_parameters_for`). For the duels' `@untuned` players, a copy of the
  `.nnue` in a folder of its own, with no `search-parameters.txt` beside it, or the candidate's
  `.nnue` in place, where there is none.
- `<depth>`: as `bench`'s, default `BENCH_DEPTH` (9).
- `--aspiration <delta>`: `aspiration_delta` overridden, the rest kept.
- Prints the network's pawn percent, the parameters used (`SearchParameters::NAMES` and values,
  one line), then `bench_line`'s `<n> nodes <n> nps`, last, as `bench`.

`bench_with` already takes a network and parameters, so the command is a few lines in `main.rs`,
its usage line extended. No test beyond the existing ones: it only wires `Network::load` to
`bench_with`.

## The measurement

On one idle machine, plugged in, nothing else running, each command run twice (the node count must
repeat exactly; nodes/s within a few percent):

1. Each of the four networks at its untuned parameters: screlu-hidden and screlu dual, pairwise-hidden
   and pairwise dual, all `-512-8-budget`.
2. Each dual again with `--aspiration` at 10 times its pawn percent over its twin's (about 23).

Read: if a dual's node count is clearly above its twin's at step 1 and falls back near it at step 2,
the window is the cause; if the node counts are close but a dual's nodes/s is lower, the inference
is; if neither, the cause is elsewhere (other untranslated constants, integer resolution).

## Afterwards

The command stays: it is how any network's cost per depth is compared from now on.
