# The test loss as the engine computes it

**Status:** done 2026-10-27, design agreed with the developer (option A below), implemented and
run the same day. Hypothesis 1 of loss-and-strength.md: **refuted.** The same file grew the later
diagnostics of loss-and-strength.md the same day; how to run each is below.

## How to run (Git Bash, from the repository root)

Each is an `#[ignore]` test of `crates/smithy/src/engine_loss.rs`, named exactly with `--exact`,
since the module's name matches all of them. It builds its own test binary, so it runs while a
smithy holds `smithy.exe`.

- **`engine_loss::tests::engine_loss`**, single-threaded, about a minute:
  `ROTOR_ENGINE_LOSS=<a lambda folder's candidate.nnue>`, optionally `ROTOR_ENGINE_LOSS_LAMBDA=<l>`
  to score it on another lambda's labels (compare networks on the same ones). Prints the trainer's,
  f32 and quantized losses by piece count; the pawn scale and the evaluations' spread; the engine's
  incremental path checked against the refresh; move agreement with Lc0's best moves at
  quiescence (20,000 quiet positions of Lc0's test games); the loss by kind of position.
- **`engine_loss::tests::search_agreement`**, every logical core but one, so with nothing else
  running: the same network, move agreement through the engine's own search at depths 1, 3, 5, 7
  on 8,000 of those positions.
- **`engine_loss::tests::duel_games`**, instant: `ROTOR_DUEL_PGN=<a match's games.pgn>`; each
  player's conversion and rescue by its own evaluations, and its drops of a pawn between two of its
  own moves.
- **`engine_loss::tests::duel_loss`**, about a minute: `ROTOR_DUEL_PGN=<a match's games.pgn>`; both
  players' networks, resolved from the PGN's names, scored quantized on every position of the match
  against each game's result, by band and by which player was to move, with an error over games
  (rotor-positions-loss.md). `ROTOR_DUEL_LOSS_NETWORKS="<a>,<b>"` scores two other networks.
- **`engine_loss::tests::sibling_reference`**, every logical core but one, nothing else running:
  `ROTOR_DUEL_PGN`; Stockfish (`data/engines.yaml`) scores every legal move of one in ten positions
  of the match, cached under `data/sibling-ranking/` (sibling-ranking.md).
- **`engine_loss::tests::sibling_ranking`**: `ROTOR_DUEL_PGN` and `ROTOR_DUEL_LOSS_NETWORKS` (any
  number); each network's ordering of those siblings against Stockfish's.

The command shape, for example:

`ROTOR_ENGINE_LOSS="G:/My Drive/rotor/data/gen/lc0-psq-threats-screlu/run-1/gen-1/attempt-1/lambda-0.02/candidate.nnue" cargo test -p smithy --release engine_loss::tests::engine_loss -- --ignored --exact --nocapture`

The networks dueled are each run's winner (`generations.tsv`): SCReLU lambda 0.02, pairwise 0.03,
pairwise_hidden 0.01. Every run's candidates are on Drive under `data/gen/<config>/run-1/gen-1/attempt-1/`.

## Why

The test loss is the trainer's, computed in float on the trainer's network. The engine plays the
exported file, quantized. If pairwise_hidden's 5.5% to 7.9% gain over SCReLU shrinks or vanishes once
measured on what the engine computes, quantization explains its lost duels after all.

## What it measures

For one exported network (a lambda folder's `candidate.nnue`, its lambda read from `candidate.txt`
beside it), over the Lc0 frozen test set (`data/lc0/frozen` on Drive, `configs/lc0.yaml`'s `test_positions`,
read by `freeze::read` as training reads it), three losses, overall and by piece count:

1. **The trainer's:** `candidate.txt`'s `test_loss`, and `pieces.tsv`'s row at its step.
2. **The file in f32,** through `rotor::nnue` (`PsqNetwork::evaluate_pawns`). Differing from 1 beyond
   rounding means the export, or the engine's reading of it, is wrong.
3. **The file quantized,** as the engine plays (`QuantizedPsqNetwork::evaluate_pawns`). Its gap from
   2 is what quantization costs, in loss.

The loss is the trainer's: `(score(pawns) - label)^2`, `rotor::labels::score`, the mean over the set.
Positions come back from `CanonicalPosition` to a `Board` through a FEN.

## Decisions (the developer, 2026-10-27)

- **A smithy test marked `#[ignore]`** (`engine_loss.rs`, test-only), not a page job nor a bench:
  it builds its own test binary, so it runs while a training holds `smithy.exe`, and smithy already
  reads the frozen set. The network named by `ROTOR_ENGINE_LOSS`, a path to a `candidate.nnue`.
- One thread, so a training on the same machine is not slowed.

## Steps

1. `engine_loss.rs`: the FEN rebuild (tested against `extract_canonical` round trips), the three
   losses, a table by piece count.
2. Run on SCReLU's and pairwise_hidden's winners (`lc0-psq-threats-screlu` lambda 0.02,
   `lc0-psq-threats-pairwise-hidden` lambda 0.03), then 512's once it ends.
3. Results here and in loss-and-strength.md.

## Results (2026-10-27)

Over the 4,194,304 positions of the Lc0 frozen test set, about 30 to 40 s each:

| network | trainer | f32 | quantized | quantized against f32 |
|---|---|---|---|---|
| `lc0-psq-threats-screlu`, lambda 0.02 | 0.065127 | 0.065127 | 0.065343 | +0.33% |
| `lc0-psq-threats-pairwise-hidden`, lambda 0.03 | 0.060969 | 0.060969 | 0.060983 | +0.02% |

- **The export is exact:** f32 equals the trainer's to six digits, overall and at every piece count,
  which also checks the FEN rebuild, the labels and the loss.
- **Quantization costs little:** SCReLU +0.33% (+0.8% to +5.4% at 3 to 5 pieces, +1.5% at 32,
  +0.1% to +0.5% between); pairwise_hidden +0.02% (its hidden layer in f32 since
  hidden-beside-f32.md, so this is the engine of today, not test 17's).
- **pairwise_hidden's gain survives whole:** as played, 6.7% below SCReLU, -5.5% to -11% at every
  count from 7 to 16 pieces, -2% to -4% from 17 up. Quantization does not explain its lost duels.
- **The spread, added the same day** (|evaluation| as played, pawns; SCReLU, then pairwise_hidden;
  both pawn scales 40%): medians 0.251 and 0.070 at 3 to 6 pieces, 0.446 and 0.313 at 7 to 16,
  0.210 and 0.161 at 17 to 32; p99 81.3 and 43.4, 80.0 and 47.8, 2.51 and 6.15. pairwise_hidden is
  compressed near zero and heavier in the opening's tail (loss-and-strength.md, hypothesis 3).
