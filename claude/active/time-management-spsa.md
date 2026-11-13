# The time management's SPSA

Status: **design agreed, 2026-11-13** (22 parameters, a set per network in its own file, the
draft table). Next: the implementation plan, time-management-spsa-implementation.md.

## Why

The time management ships with Reckless's SPSA-born constants, tuned to Reckless's search, and
three of Fattorini's own picked by hand from one 180+2 profile (reckless-time-management.md,
reckless-only.md). TODO.md's "An SPSA of time management, after the search's" (the developer,
2026-11-11 and 2026-11-13) asks for them to be tuned against Fattorini's search. Test 8 showed
the untuned set is already ahead of Classic at 180+2 (53.4%), so this is an improvement measured
on its own, not a fix.

## Already decided (TODO.md, reckless-time-management.md)

- **After the search's SPSA,** ideally after the speed work too: the time rules depend on the
  search's curves, so they are tuned against the search that ships.
- **10+0.1, scaled by the bench** as every tune: its increment is 1/100 of the base against 3+2's
  1/90, so the clock goes through the same phases.
- **32k pairs** (`DEFAULT_TUNE_PAIRS`), about 12 hours over every idle arena; noisier values
  accepted for the number of parameters.
- **Reckless's hand-picked constants are never tuned:** the move overhead (100) and
  `EXTRA_MARGIN_MS` (15), which a lag-free tune would drive to zero; `inc_fraction` (0.75);
  `cyclic_soft` and `cyclic_hard`, never exercised by a Fischer tune; `eval_window_cp` (12).
- **The verdict:** a 180+2 duel of the tuned values against the untuned, for the depth 10+0.1
  does not reach. Values ending near their start are an answer too: the 180+2 duel can then go.

## Design

### 1. What is tuned: 22 parameters

The 19 SPSA-born constants of `TimeParameters` (TODO.md said 18; `nodes_a`, now Fattorini's
2.66, makes 19 counted with its group), and Fattorini's three hand-picked ones. **Decided (the
developer, 2026-11-13): 22.** The three are guesses from one 180+2 profile, which is what a tune
is for, and unlike the overhead a lag-free tune has no reason to push them to an extreme.

### 2. The values at run time

`TimeParameters` becomes a run-time value of a search, as `SearchParameters` already is:
`TIME_PARAMETERS` stays as the default, and the time manager reads the set it is given. At the
defaults the engine is unchanged (`time_bounds_by_hand` and the multiplier tests hold).

- **The engine (uci.rs):** no UCI option. A build plays the time set embedded with its network
  (section 3), or the compiled defaults.
- **The arena:** `InProcessPlayer` takes an optional `TimeParameters` beside its
  `SearchParameters`.
- **The chore's player text (protocol.rs):** the search's `name=value` list may carry the time
  parameters too, as `tm.<name>=<value>`, real numbers. Absent, the defaults, so every chore of
  before reads the same. A tune's chore always writes all 22 in full, as it writes the search's.

### 3. Where the result lives: per network

**Decided (the developer, 2026-11-13): one time parameter set per network,** as the search's
`parameters.txt`. The time parameters depend on the evaluation: directly through the score trend
and the score stability (scaled to its pawn by `pawn_percent`, which fixes the scale, not how much
its scores swing between depths), and indirectly through the tree it shapes, which the node
fraction, best-move stability and best-move changes count. `TIME_PARAMETERS` stays the default
for a network with no tuned set, and for every network until its time tune.

Consequences: a snapshot and a release embed the network's time set beside its search set; the
arena's players read it; a network's tune becomes two, the search's and then the time's, each
about 12 hours at 32k pairs. **The file (the developer, 2026-11-13): its own,
`time-parameters.txt`, beside `parameters.txt`,** so neither tune rewrites the other's result.
`@tuned` reads both, each absent meaning its defaults; `@untuned` neither.

### 4. The SPSA, generalized

Today `spsa.rs` is the search's: `Spec` holds `i32`s, `Theta` is `[f64; 10]`, and `point` rounds
to `SearchParameters`. It becomes a list of real-valued specs, `Spec { start, min, max, c_end }`
in `f64`, with two sets: `SEARCH_SPECS` (today's ten, rounded to integers when played, as now)
and `TIME_SPECS` (the 22, played as they are). The gain sequences, the update, the clipping and
`R_END` stay as they are; `a_end = R_END * c_end^2` is scale-free, so one `R_END` serves both.

A tune tunes one set. During the time tune the search plays the network's tune, both sides alike.

**The tune job (tune.rs, the smithy):** its state, `state.tsv`, `spec_text` and resume carry the
set's name and as many values as the set has. The page's tune screen lists the parameters by
name, as now. A kind of tune, `search` or `time`, chosen when it starts; `search` the default.

### 5. Ranges and perturbations (accepted by the developer, 2026-11-13; revisited after the first tune's traces)

`c_end` about a tenth of the value's scale, as the search's margins have an eighth; a slope near
zero gets a perturbation of its own size rather than of its value's. Ranges wide enough that no
start sits near an edge, and keep a factor's floor below its intercept.

| parameter | start | min | max | c_end |
|---|---|---|---|---|
| soft_max | 0.0594 | 0.03 | 0.10 | 0.006 |
| soft_rise | 0.0492 | 0.02 | 0.08 | 0.006 |
| soft_rate | 0.0386 | 0.01 | 0.10 | 0.006 |
| hard_fraction | 0.7281 | 0.30 | 0.90 | 0.06 |
| hard_soft_ratio | 3.0 | 1.5 | 6.0 | 0.4 |
| multiplier_min | 0.5 | 0.2 | 0.9 | 0.06 |
| multiplier_max | 2.0 | 1.2 | 4.0 | 0.25 |
| nodes_a | 2.66 | 1.5 | 4.0 | 0.3 |
| nodes_b | 2.6554 | 1.0 | 4.0 | 0.3 |
| nodes_min | 0.546 | 0.3 | 0.9 | 0.06 |
| trend_a | 0.7426 | 0.5 | 1.2 | 0.08 |
| trend_b | 0.048 | 0.0 | 0.15 | 0.008 |
| trend_min | 0.7214 | 0.4 | 1.0 | 0.08 |
| trend_max | 1.4031 | 1.0 | 2.5 | 0.15 |
| pv_a | 1.2881 | 0.8 | 2.0 | 0.12 |
| pv_b | 0.044 | 0.0 | 0.15 | 0.008 |
| pv_min | 0.716 | 0.4 | 1.0 | 0.08 |
| eval_a | 1.2664 | 0.8 | 2.0 | 0.12 |
| eval_b | 0.0416 | 0.0 | 0.15 | 0.008 |
| eval_min | 0.8642 | 0.5 | 1.0 | 0.08 |
| changes_a | 1.15 | 0.7 | 2.0 | 0.12 |
| changes_b | 0.2526 | 0.0 | 1.0 | 0.04 |

**Pairs that must stay ordered:** `multiplier_min < multiplier_max` and `trend_min < trend_max`
hold by their ranges. `soft_rise < soft_max` (a positive soft fraction at move 0) does not:
the time manager clamps the fraction at a small floor (0.005), so a perturbed point is never
nonsense.

### 6. Before it runs

- Every arena on the build that carries the time parameters (an older one would ignore `tm.*`
  keys, or refuse them, and play the defaults unnoticed: the plan says which, and the page's commit
  column shows it).
- The search's SPSA done, and its values in.
- The smithy's bench on an idle machine (smithy-bench.md, fixed 2026-11-13).

## Not in scope

- Tuning at 180+2 directly: about 18 times the hours of 10+0.1 for the same pairs.
- A UCI option for the time parameters: nothing outside the forge needs them.
- `CYCLIC_SOFT` and `CYCLIC_HARD`: only a repeating-control tune would move them; CCRL's 40/15
  is checked by smoke tests instead.
