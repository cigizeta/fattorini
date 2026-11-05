# Measuring the quiet history distribution

Started 2026-10-03. Status: **done** (2026-10-03): implemented and run; its results chose the two
threshold candidates in move-picker.md. The bench stays, for remeasuring if history ever changes. Feeds move-picker.md's two quiet thresholds.

## Question

At a node of the main search, where do its quiet moves' `combined_history_score`s fall, per remaining
depth? And, for each score: how often is the move reached at all (not cut by late-move pruning), and
how often does it cause the cutoff? The last one tests the reading of an entry as a net cutoff rate:
if low scores rarely cut, a threshold there costs little.

## The search freeze

The engine must not change before betty's generation 1 is promoted. So the recording is compiled only
under a new cargo feature, `move-stats`, off by default: every recording line is behind
`#[cfg(feature = "move-stats")]`, and the engine, self-play, replay and the arena build exactly as
today. The fixed-depth test confirms it: it must pass unchanged, without the feature.

## What is recorded

In `alpha_beta_with_buffer`, at a node that is not in check, after move generation and before the
move loop: every pseudo-legal quiet move (not the TT move) with its score at that moment. This is
close to Stockfish's `QUIET_INIT`, which scores after the good captures were searched; the captures'
subtrees update history in between, so it is not exact. Each quiet move gets one of three outcomes
when the node ends:

- **not reached**: late-move pruning broke the loop, or a cutoff came first;
- **searched, no cutoff**;
- **caused the cutoff**.

Illegal moves are left out. A node's quiets are recorded only when the node returns normally (not on
a stop).

## Where it accumulates

A `MoveStats` table in `search.rs`, behind the feature: for remaining depth `1..=MAX_DEPTH_BUCKET`
(deeper nodes into the last bucket), score buckets of width 256 over `[-40960, 40960]`, and three
counts per bucket, one per outcome (four, as implemented: not reached, reached but pruned by
futility, late-move, history or SEE pruning, searched, cutoff). Plus, per depth: nodes recorded. It
is one global `Mutex`, locked once per node (the bench is single-threaded), read and reset by
`move_stats::take`.

## The bench

`benches/move_stats.rs`, `required-features = ["move-stats"]`, in-process like `nnue_bench`:

- the evaluator as in `nnue_bench`: a `.nnue` path, or `--big-five`;
- `--depth N` (default 9);
- positions: `nnue_bench`'s seven, or `--fens FILE`, one FEN per line;
- for each position, `choose_best_move` with a fresh transposition table, then one table is printed:
  per depth, quiets per node, the fractions below candidate thresholds, and the reached and cutoff
  rates per score band;
- the full table as CSV in a path given by `--csv FILE`, for plotting.

```
cargo bench --bench move_stats --features move-stats -- data/networks/betty/gen-0001/champion.nnue --depth 9 --csv history.csv
```

Since betty's generation 1 is not promoted yet, the first run uses `--big-five` (PeSTO); it reruns on
betty's champion once there is one.

## Tests

- With the feature off, the fixed-depth test passes unchanged.
- With the feature on, a depth-3 search on the start position records some nodes, and at every depth
  `cutoff + searched + not reached` equals the quiets recorded.
- The bucket function: `-40960` lands in the first bucket, `40960` in the last, `0` in the middle.

## First results (2026-10-03, the Lichess machine, 7 positions, depth 9)

Two runs, PeSTO's big five and the hand-crafted evaluation, give the same shape, so the evaluation
barely matters here. Hand-crafted figures:

- **The range is tiny**: about `-384` to `+1800`, against a possible `+-40960`. About 96% of quiets
  score below `+64`, 33% to 45% below `0`, 8% to 12% below `-64`, 3% below `-128`, and almost none
  below `-256`. Stockfish's thresholds (`-14000`, `-3560 * depth`) and their range-ratio scaling
  (`-3200`, `-800 * depth`) would never fire. Rotor's bonus is `d * d`, at most 81 per event at
  depth 9. Few events land on any one entry within a search, so entries stay small.
- **About 60% sit in `[0, 64)`**, most of them probably entries never updated, which read exactly
  `0`. Not yet separated.
- **The score predicts the cutoff, at shallow depth.** Cutoff rate among searched quiets, depths 1, 2
  and 3: `[64, ...)` about 50%, 20%, 15%; `[0, 64)` 37%, 11%, 3.5%; `[-64, 0)` 18%, 3.7%, 1.2%; below
  `-64` thin samples, mostly 0 to 4% from depth 2 on. From depth 4 on, almost no quiet causes a cutoff
  at all.
- **Most quiets are never reached**: 97% at depth 1, 86% at depth 4, 41% at depth 9.

The sample is small: nodes at depths 7 to 9 number tens, and the bands there are noise.

## Next (2026-10-03)

- Added: exact-zero quiets counted apart from the rest of `[0, 64)`, losing captures (SEE `< 0`) with
  the same outcomes, and `search_bench`'s Stockfish bench positions (`benches/stockfish_positions.rs`,
  shared by both benches) as the default set.
- The developer's condition: this line of work waits for amanda's generation 1 installed on the Lichess machine,
  and its numbers are read from runs with that network, not PeSTO's or the hand-crafted evaluation.

## amanda's generation 1 (2026-10-03, the Lichess machine, Stockfish's 49 bench positions, depth 9)

213,029 nodes at depth 1 down to 184 at depth 9: enough at depths 1 to 7.

- **Range** about `-640` to `+4400`. Below `0`: about 35%; below `-64`: 13%; below `-128`: 6%;
  below `-256`: 1%; below `-512`: none.
- **Exactly `0`: 54% to 58% of quiets**, entries never updated. They are the worst quiets of all:
  cutoff rate among searched, depths 1 to 4, `6.3, 2.6, 1.2, 0.8%`, against `27.5, 9.8, 5.1, 2.7%` for
  the rest of `(0, 64)` and `17.1, 4.3, 1.6, 0.9%` for `[-64, 0)`. A small negative score is a move
  the search found relevant somewhere; a zero is a move it never looked at.
- **Positive scores** (`>= 64`): about `32-40%, 15-22%, 9-19%, 4-7%`, rising slowly with the score.
- **Losing captures (SEE `< 0`)**: `15.4, 7.9, 3.3, 3.9, 3.3, 2.5, 1.2, 1.2%` at depths 1 to 8. From
  depth 2 on, about as good as `[-128, 0)` at depth 2, and **better than every non-positive quiet,
  zeros included, from depth 3 on** (`3.3%` against at most `1.7%`), and better even than `(0, 64)` from
  depth 4.
- At depth 1 alone, `[-192, -64)` cut about 40%, more than anything else but positive scores: depth 1
  differs from the rest, and is also where 96% of quiets are never reached.

Selection bias: a move searched later is searched only after the earlier ones failed, in nodes less
likely to cut at all. Today's order puts losing captures and non-positive quiets in the same stage
(after positive history, in generation order), so those two compare fairly; positive-history quiets,
tried earlier, are flattered.

## What it gives move-picker.md

The sort limit: the score below which quiets are mostly not reached, per depth. The good-quiet
threshold: the score below which a quiet's cutoff rate is lower than a losing capture's would be (a
second measurement, not this one, if it turns out to matter).
