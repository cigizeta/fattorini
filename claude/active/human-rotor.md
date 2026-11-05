# Rotor: Human-Level Playing Strength Through Inference-Time Tuning

## Goal

Create a tunable family of weaker Rotor levels that play naturally at a range of human ratings, without injecting deliberate blunders. Nothing is discarded: the final user sets alpha-beta depth, MCTS budget, MCTS temperature and PeSTO blend as engine options, and tunes them online. Explore whether different configurations also produce distinct playing styles.

## Core idea

Train one strong engine per search, once:

- **Alpha-beta:** the evaluation network, trained with the existing AB(λ) pipeline at **λ = 0.98**, which has experimentally produced the stronger Rotor.
- **MCTS:** a network with a value head and a policy head, trained fully, with the sole aim of a strong MCTS engine.

Then tune playing strength entirely at inference time, without retraining for every variant.

## Two search alternatives

**1. Alpha-beta**

- Keep the trained evaluator fixed.
- Limit search depth to control playing strength.
- Deterministic: the same position and options always give the same move.

**2. MCTS**

- Replace alpha-beta with Monte Carlo Tree Search (PUCT), guided by the trained policy head's move priors.
- Use a very small simulation budget to limit search effort.
- A temperature option τ randomizes play: the move is sampled from the root visit counts N with probability proportional to N^(1/τ). τ = 0 (the default) plays the most visited move. With a tiny budget the visit counts are coarse, so useful values likely lie below 1.

These are alternative search architectures to experiment with, not necessarily components to combine.

## Evaluation blending

At inference time, blend the network's evaluation with PeSTO's handcrafted evaluation. PeSTO speaks centipawns; the network's own space is [-1, +1]. Both are from the side to move. They are blended in [-1, +1], through the label curve the networks are trained with.

### The label curve

`labels::score(pawns) = atan(pawns / A) / K`, clamped to [-1, +1], and its exact inverse `labels::pawns_from_score(s) = A × tan(K × s)`, finite on all of [-1, +1] and reaching ±32 pawns (`CEILING_PAWNS`) at ±1. Every label is built in this space (the teacher's score through `score`, blended with the game result by λ), so a network's prediction in this space is what it was trained to predict.

### The network's side: `s_net`

- **MCTS network:** its value head outputs [-1, +1] directly: `s_net = V_value`.
- **Alpha-beta network:** it outputs its own pawns, clamped to ±32 (`labels.rs`), so `s_net = score(V_net)`.

No constant is needed here: a network's pawn is not PeSTO's pawn (about 0.55 to 0.6 of it near equality, which is why the pawn scale `p` exists), but `s_net` is the network's own label space, correct whatever its pawn is worth.

### Step 1: PeSTO into [-1, +1]

`s_PeSTO = score(V_PeSTO / 100 / c)`

`/ 100` turns PeSTO's centipawns into pawns; `c` turns PeSTO's pawn into the network's. `c` is one constant per network, fitted when the network loads, on the 10,000 compiled-in positions that `nnue::measure_pawn_percent` already uses (`pawn_scale_boards()`): the `c` that makes `s_PeSTO` closest to `s_net` by least squares, in label space, on every position (not only those within ±300 cp, as `p` is). It absorbs the unit mismatch that `p` absorbs today.

The fit, when a network loads:

- For each of the 10,000 positions, compute PeSTO's `h_i` (centipawns) and the network's `s_net,i` once; only the cheap curve is reevaluated per candidate `c`.
- Minimize `Σ ( score(h_i / 100 / c) − s_net,i )²` over `c`. Unlike `p`'s closed form (`Σhn / Σh²`), `c` sits inside `atan`, so there is none: a golden-section search over `c` in [0.25, 4], about 30 to 40 steps of 10,000 `atan` each, negligible at load time.
- Every position counts: no ±300 cp window, and `h = 0` is kept (it pulls `c` toward the network also reading the position as about equal).
- Round `c` to a fixed step, so that networks of nearly the same scale search the same trees (for example, `100 / c` to 5%, `p`'s own granularity), and clamp it to the bracket against a degenerate network.
- Sanity check before anything else: on amanda, betty and caroline, `100 / c` should land near today's `p` (55 to 60%). Not exactly, since `c` also sees lopsided positions, but a large gap means the curve does not fit PeSTO's shape.
- Until the TODO item replacing `p` with `c` is done, the engine keeps `p` for its search; `c` is fitted beside it and used only by the blend (alpha-beta at `α < 1` runs with `pawn_percent = 100`, as step 3 says) and the MCTS UCI score.

### Step 2: the blend

`s = α × s_net + (1 − α) × s_PeSTO`

- `α = 1`: pure neural evaluation.
- `α = 0`: pure PeSTO evaluation.
- Intermediate values combine the two.

Both terms are bounded before mixing, so neither takes over in lopsided positions, which a linear blend in centipawns would allow.

### Step 3: back to centipawns, for alpha-beta

**MCTS** uses `s` as it is. **Alpha-beta** inverts step 1:

`V_cp = 100 × c × pawns_from_score(s)`

in PeSTO's centipawns, so its search runs with `pawn_percent = 100` at every `α`. `pawns_from_score` is finite at ±1 (±32 pawns, far below the mate and tablebase bands), so no extra clamp is needed. One `atan` and one `tan` per evaluation, negligible next to the network.

### Example

With a made-up `c = 1.7`: PeSTO says +170 cp, so 170 / 100 / 1.7 = 1.0 pawn and `s_PeSTO = score(1.0) = 0.5`. The network says `s_net = 0.3`. At `α = 0.5`, `s = 0.5 × 0.3 + 0.5 × 0.5 = 0.4`. Back: `pawns_from_score(0.4)` ≈ 0.72, so `V_cp` ≈ 100 × 1.7 × 0.72 ≈ 122 cp.

### UCI scores

An MCTS engine still reports `score cp` in UCI. It uses step 3 on its root value (the best move's mean `s`): `100 × c × pawns_from_score(s)`, in PeSTO's centipawns, the same unit alpha-beta reports. So both searches print comparable scores, at every `α`, and `c` is needed by the MCTS network even at `α = 1`. Proven wins and losses (certainty propagation, tablebases) report `score mate` or the tablebase band as alpha-beta does, not through the curve.

`c` replaces `p` throughout the engine, not only here: a TODO item of its own (TODO.md).

## Experimental procedure

1. Train the two strong networks once each: the alpha-beta evaluation network and the MCTS network (value and policy heads).
2. Generate variants by changing alpha-beta depth or MCTS simulation budget and the evaluation blend coefficient.
3. Run matches to measure the relative playing strength of each configuration.
4. A configuration's Lichess rating is what a Lichess bot running it earns; it is not measured here.
5. Compare games from similarly rated variants to determine whether the parameters produce distinct playing styles.

## Constraints

- No deliberate blunder injection.
- No retraining for each inference-time configuration.
- Keep the trained network fixed during the initial experiments.
- Stockfish `UCI_Elo` levels and Lichess ratings are different scales; neither is converted into the other.

The central hypothesis is that limited search and a tunable blend of neural and handcrafted evaluations can produce naturally weaker play, with a family of configurations that may differ in playing style as well as strength.

## Decisions

- The rating measurement protocol, settled (the developer, 2026-10-16): two separate scales, never converted into each other.
  - **Engine scale, Stockfish's `UCI_Elo`: the only one measured here.** The arena's Stockfish ladder (`stockfish-elo-<rating>`, 1320 to 3190), as README's Strength section did for the strongest Rotor. Configurations below 1320 are placed by arena matches chained down from the lowest level.
  - **Human scale, Lichess: not measured here.** A Lichess rating exists only for a Lichess bot playing that configuration; it is whatever the bot earns there.
  - **Ratings are Stockfish `UCI_Elo` levels, stated as such (the developer, 2026-10-16):** "plays like Stockfish at `UCI_Elo` N", which is more precise than claiming a CCRL rating. Stockfish's README says `UCI_Elo` is calibrated at 60s+0.6s and anchored to CCRL 40/4; that is Stockfish's business, not a claim of ours.
  - **Time control: 60s+0.6s, on whatever hardware is available, unscaled.** No hardware scaling: Rotor and Stockfish share the machine, so the comparison is fair on any of them.
    - CCRL Blitz (2'+1" on an i7-4770K, scaled by a Stockfish 10 bench whose reference figure CCRL does not publish) was considered and dropped.
  - **Games per configuration: not fixed (the developer, 2026-10-16).** Each measurement is a duel against a Stockfish level under the arena's duel rule: an upper bound on games and its stopping rule, stopping as soon as the result is resolved either way.
  - **Which configurations: exploratory (the developer, 2026-10-16).** Try many configurations and see what happens; no grid fixed in advance.

Settled: evaluation and search are separate settings by construction. Alpha-beta depth, MCTS budget and MCTS temperature are search options; the blend `α` is an evaluation option. The user tunes each independently.
