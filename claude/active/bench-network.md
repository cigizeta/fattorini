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

## Run log

- **2026-11-28, depth 9:** the window cleared (scaled to 23: screlu dual 967,854 to 881,661 nodes,
  pairwise dual 2,135,276 to 2,195,460, opposite ways). Nodes/s: the duals 18% (screlu) and 9%
  (pairwise) below their twins.
- **2026-11-28, depth 13:** the `elo` duals' trees grow far slower, depth 9 to 13: screlu-hidden
  x39.5 (118.0M nodes, 72 s), screlu dual x11.6 (11.3M, 8.4 s), pairwise-hidden x24.2 (23.3M,
  14.5 s), pairwise dual x7.0 (15.0M, 9.9 s). Pawn percent 35 for both `lc0` networks, 90 and 85 for
  the `elo` ones (a ratio of about 2.5, S's 2.47).
- **The audit, 2026-11-28:** every score constant in the search is translated (the margins, the
  null move's eval plies, the correction cap and its error clamp, delta's victim, the time
  manager's window and slope) or has no unit (SEE, history, LMR, late move pruning, `improving`,
  the cutoff blend); the network's clamp, 128 pawns, is never reached. The aspiration window alone
  is untranslated, and measured small. So the head and the unit, changed together, are confounded.

## `--scale <factor>`: the unit without the head (2026-11-28)

The network's output multiplied by `<factor>` before it is rounded to centipawns, and its pawn scale
measured after, so the parameters translate as a real network of that unit would have them. An `lc0`
network at `--scale 2.47` (173.72 / 70.37) is the same evaluation in the `elo` unit's numbers.
Read: screlu-hidden's tree at depth 13 shrinking towards the duals' says the unit does it, through
something the audit missed (integer resolution the likeliest); staying near 118M nodes clears the
unit, and the dual head is what thins the search.

A field of `QuantizedPsqNetwork`, `output_scale`, 1.0 except here, applied where its evaluations
become centipawns (from scratch and from the accumulators). A multiplication by 1.0 is exact, so
every other network evaluates bit for bit as before. To remove once read, unless it proves useful.

## Afterwards

The command stays: it is how any network's cost per depth is compared from now on.
