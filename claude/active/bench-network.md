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

### Its run log

- **2026-11-28, first run, flawed:** screlu-hidden at `--scale 2.47`, depth 13: 27,967,726 nodes
  (from 118.0M), 1,729,318 nodes/s. But its pawn measured 65%, not about 86%: the scale kept the
  `lc0` curve, and the pawn is a fit in label space. Its margins were narrower in pawns than a real
  `elo` network's, so part of the shrinkage is the flaw. Fixed: `--scale` now reads the network
  through Elo's curve too. To run again.
- **2026-11-28, through Elo's curve:** pawn 90%, depth 13: 54,851,497 nodes (from 118.0M, about
  half), 1,664,516 nodes/s. The unit alone halves the tree, the evaluation the same; the duals'
  trees are 10x and 1.6x below their twins', so the unit is part of it, not all, in the screlu
  pair. Next: the same with `--aspiration 25`, the window at depth 13 (only depth 9 was tried).
- **2026-11-28, through Elo's curve, `--aspiration 25`:** 43,946,271 nodes, 1,733,190 nodes/s:
  fewer still, not back towards 118.0M. The window is cleared at depth 13 too.
- **2026-11-28, the correction history off (`correction_cap=0`), depth 13:** unscaled 34,688,671
  nodes (1,755,403 nodes/s), scaled through Elo's curve 44,680,294 (1,760,418). With it on: 118.0M
  and 54.9M. So the correction history triples the `lc0` tree and adds a fifth to the `elo` one:
  it is the unit-sensitive part, and its arithmetic, not a constant, is what differs. Without it the
  scaled tree is 29% larger, the rest (the pawn's rounding, 90 against 86.5, widens its margins
  about 4%; the evaluation's finer integers).

## Manual constants per unit (the developer, 2026-11-28)

The search's constants are hand-set per unit, not translated from PeSTO's pawn: `lc0`-manual for
the `lc0` networks, `elo`-manual for the `elo` ones, and, when PeSTO's turn comes, PeSTO-manual for
PeSTO (left alone for now). Every duel plays each side with its own unit's set.

- **`lc0`-manual:** today's `@untuned` at a pawn of 35, as every `lc0` duel has played it: the
  16k tune's values, hand-rounded, so in effect tuned for `lc0`. Nothing to do.
- **`elo`-manual:** calibrated on one evaluation in both units. Screlu-hidden at `--scale 2.47` is
  its own evaluation in `elo` numbers; its constants are moved until its bench at depth 13 matches
  the unscaled network's (118.0M nodes; the per-position counts alike too, not only the total).
  The set that matches searches that evaluation as `lc0`-manual does. Start: today's translation at
  a pawn of 90 (razoring 135, reverse futility 90, futility 180, null move 180, delta 180, victim
  90%, correction cap 90, aspiration 25). The correction history's integer step is tested first
  (`correction_cap` at 0, both units): if it is what halves the tree, its fix belongs to the design
  rather than to the constants, since no constant reaches it.
- **The duels:** each dual with `elo`-manual against its `lc0` twin at `@untuned`, at depth 9 and
  10+0.1. Their result is then the networks' (the head, and training under `elo`), not the search's.

**The calibration skipped (2026-11-28, Claude's recommendation, the developer agreeing):** with the
correction history in a fine grain (correction-grain.md), the same evaluation in both units'
numbers searches within 11% at depth 13 (23.7M and 26.3M nodes), part of it the pawn's rounding
(90 against 86.5). So the translated defaults are close to an `elo`-manual set already: the dual
duels are rerun at `@untuned` on the new build, both sides. Hand-set constants wait for the tune.

### `--set <name>=<value>`

`bench-network` overrides any of `SearchParameters::NAMES` by name, repeatable; `--aspiration`
goes, `--set aspiration_delta=<delta>` replacing it. An unknown name or a value that is not an
integer is the usage error.

### To settle before the duels

How an arena names a player with a given parameters set: the engine reads `search-parameters.txt`
beside the network (`read_tuning_beside`), so a copy of each dual's candidate in a folder of its own
with the `elo`-manual set beside it would be one; tuning-suffixes.md to read first.
**Read 2026-11-28:** no copy needed. `<network>@search-tuned` plays `search-parameters.txt` beside
the network, and refuses a missing file; `@untuned` plays the translated defaults. So the
`elo`-manual set, written beside each dual's `candidate.nnue` on Drive as `units\tnetwork` then one
`name\tvalue` line per parameter, makes it `<dual>@search-tuned`. The suffix says tuned where the
set is calibrated by hand: internal names only, said in each trial's row. The file is removed, or
replaced, when a real tune comes.

`--set` written 2026-11-28; `--aspiration` removed with it.

## Afterwards

The command stays: it is how any network's cost per depth is compared from now on.
