# Training on quiet positions only

**Status:** designed 2026-10-28; plan in quiet-filter-implementation.md. **Failed and dropped
(2026-10-30, confirmed 2026-10-31, below):** the filter costs our networks 100 to 190 Elo. Its code
was kept until the training pipeline was discussed (the developer, 2026-10-31). **Archived
2026-11-02:** the filter's code removed, the `-quiet` runs to go.

## The outcome (2026-10-31)

**The duels.** `-quiet` against `lc0-psq-threats-screlu-g1-r1`, both `@default`: about -160 Elo at
depth 9 and -108 at 10+0.1 (2026-10-30); played again with the new search (`corr` and `blend`,
search-vs-stockfish.md), about -190 and -137, decided in 12 and 20 pairs. The search changes did
not close the gap.

**No bug in the filter (2026-10-31, by reading the code):** the converter writes Lc0's best move
(`best_idx`) at the position before the played move, beside its score (`convert.rs`); `is_quiet`
judges that same board (`game_labels.rs`); positions, keep flags and labels share one ply index
(`pgn_stream.rs`, `freeze.rs`), so a dropped row takes its label with it. The data `-quiet` trained
on was not sampled (the developer: no data problem expected).

**Where it fails, measured (2026-10-31):** `nnue_bench --residuals` now splits its 10,000 Lumbra
positions into quiet and noisy (`is_noisy`: a legal capture winning material by SEE, or a
promotion; 2,335 noisy), the same split for both networks. At d = 0, `value - static` is what the
pending captures are worth beyond the static evaluation:

| | g1-r1 quiet | `-quiet` quiet | g1-r1 noisy | `-quiet` noisy |
|---|---|---|---|---|
| mean | +1.8 cp | +1.7 cp | +26 cp | +82 cp |
| p50 | 0 | 0 | 2 | 24 |
| p90 | 0 | 0 | 68 | 201 |
| p99 | 45 | 43 | 249 | 930 |
| p90, label space (1/1000) | 0 | 0 | 298 | 654 |

Equal on quiet positions: fewer training positions cost `-quiet` nothing where it was trained.
About three times worse on noisy ones: g1-r1, trained on labels that count the pending capture,
anticipates part of it; `-quiet` misses most. It also puts 292 positions past +/-300 cp against
g1-r1's 118, judging mid-exchange positions lopsided. Deeper searches keep the pattern.

**Why the premise fails for us.** The filter assumes the evaluation is used only where nothing is
pending. True of the static evaluations that become scores: quiescence replaces stand-pat wherever a
capture does better. False of the evaluations the search decides with: every node below the root
not in check is evaluated (`search.rs`, `raw_eval`) and read by reverse futility, razoring, null
move, futility, `improving` and the correction history, and quiescence evaluates stand-pat before
its captures. A network blind to pending captures misleads those decisions, which nothing corrects.
Stockfish's search reads its evaluation the same way; what lets the filter work for its trainers
(far larger corpora, larger networks, labels zeroed rather than dropped in older data) is not
measured here.

**What the field does (fetched 2026-10-31):** nnue-pytorch's loader skips, with `filtered` on,
positions in check or whose best move is a capture (`training_data_loader.cpp`); linrock's
`csv_filter_v6.py` and `v8.py`, behind Stockfish's Lc0-data networks, also skip a position when
either of Stockfish's two best moves is a capture or a promotion, or when only one move is good (the
two best moves' scores far apart). Their search's depth is not in the scripts read. Lc0's records
give the second move by visits for free (lc0-second-move.md), not its score: the one-good-move test
could only be approximated by the second move's visit share.

**Decided (the developer, 2026-10-31):** Lc0's networks train unfiltered. The second-move extension
(`Keep::Quiet` reading visit counts) and the full-corpus experiment of step 7 are dropped with it.

## The experiments (the developer, 2026-10-28)

1. **`lc0-psq-threats-screlu-quiet`:** SCReLU's config with Lc0's best-move filter, against
   `lc0-psq-threats-screlu-g1-r1`, both `@default`. Width 512's run stopped to make room.
2. **`lc0-psq-threats-screlu-quiet-lc0`, only if `-quiet` draws (revised 2026-10-28, the
   developer; Stockfish skipped the same day, lc0-second-move.md):** the same with a second line
   added: a position also dropped when Lc0's second choice, the policy target's highest entry other
   than the best move's, is a capture or a promotion. Lc0's records hold the target as raw visit
   shares, from the search that labelled the position, stronger than Stockfish's depth 6 and with no
   Stockfish to run. It drops 6.9% of the quiet test positions (2.5% when the second's share is at
   least 0.2), mostly middlegames, far below the "25% more" reported for Stockfish's MultiPV 2,
   whose second line by score includes captures Lc0's search hardly visits. Not run if `-quiet`
   clearly loses (removing more likely hurts more), nor if it wins (then step 3). Needs the 237 GB
   of raw tars downloaded again, so a cheap check gates it first, on the two test tars already
   downloaded: `-quiet`'s winner scored on the quiet test positions the second line drops against
   those it keeps, by piece count too so that a skew toward middlegames does not fake a gap. The
   dropped part's loss much higher (say over 1.5 times the kept part's): run it. About the same:
   the second line is closed. A high loss justifies the download only, it does not prove that
   dropping helps (hard positions may still teach the shared transform, the threats features
   especially); a duel still decides, and a 7% change may be beyond what a duel can see. **The raw tars
   downloaded again after the duels, whatever they show (the developer, 2026-10-28):** the
   conversion writes every legal move's visit count into each comment (mcts-nnue.md's format), not
   a filter, so the second line at any threshold is a config's choice over the corpus, and the raw
   tars are kept: never another download. The cheap check then needs no download either.
3. **If it succeeds:** stop and think; then `lc0-psq-threats-pairwise-hidden-quiet`.
4. **Old networks kept:** each filtered variant is a config of its own, `-quiet` or `-quiet-sf`;
   nothing existing is retrained or overwritten.
5. **Duels decide:** depth 9, then 10+0.1. Concordance at quiescence and test loss are reported,
   decide nothing. What counts as success (a draw included or not): decided after the duel.
6. **A test set of its own:** a fresh sample of exactly 4,194,304 quiet positions from the same
   held-out test games (Lc0's 66,542, unchanged), filtered as the training set is; about 40 to 50 a
   game, as today's 64 (the measurements below).

## The test set measured (2026-10-28)

Two diagnostics in `engine_loss.rs` over every position of Lc0's test games (7,976,467 in 66,542
games, about 120 a game), SCReLU's quantized loss at lambda 0.02:

- **`test_set_correlation`:** a game's positions hang together, correlation about 0.12, design
  effect about 15. The frozen set (about 64 a game) is worth about 500,000 independent positions;
  its mean loss's error by game about 0.33%, against 0.12% were its positions independent. One
  position a game would need 4.2M test games and still give 0.12%.
- **`paired_loss`:** two networks on the same positions share most of that noise. The error of
  their difference by game is about 0.09% (design effect about 6), a quarter of the absolute error.

Every network against SCReLU, all at lambda 0.02:

| network | loss against SCReLU | standard errors | duels against SCReLU |
|---|---|---|---|
| threats | +1.01% | 11.7 | about +12 Elo to SCReLU, drawn |
| pairwise | +0.30% | 3.1 | +38 Elo to SCReLU, won |
| material | -1.38% | 15.3 | drawn |
| phase | -1.66% | 17.2 | drawn |
| count | -0.73% | 8.4 | drawn |
| taper | -0.07% | 0.8 | drawn |
| pairwise_hidden | -7.27% | 59.2 | lost, -24 to -55 Elo |

Threats against pairwise: -0.70%, 7.8 standard errors. **The test set is not the trouble:** every
gap but taper's is real, and real loss gains became no Elo for material, phase, count and
pairwise_hidden alike. What the set holds (game positions, not the leaves the search ranks) is
the trouble, not its noise. Absolute losses across different test sets carry the full 0.33%
each, so a network is compared with another on one set (decision 10 of the plan).

## Why

pairwise_hidden fits Lc0's positions 7.8% better than SCReLU, orders a position's children best of
five networks by its static evaluation, and worse than SCReLU once quiescence has played the captures
out; that gap matches its lost duels (sibling-ranking.md). The network is trained on game positions,
roots, while the search asks it about the leaves of its tree (settled-positions.md). A position
with a capture pending carries a label that counts the capture, which the evaluation cannot see:
noise in exactly the positions the search resolves before evaluating.

Stockfish's answer is to train on quiet positions only (settled-positions.md, "What the field
does").

## The question it answers

First, whether the filter makes the best network stronger in play. Then, if it does, whether it
closes pairwise_hidden's gap between its static and its quiescence concordance. If neither Lc0's
filter nor Stockfish's helps SCReLU, the quiet idea has failed, and the approach is rethought
before anything else is trained.

## Decisions

1. **SCReLU first (the developer, 2026-10-28),** not plain psq: the best network, so a gain is
   also a release candidate. The experiments above.
2. **Promotions count as captures (the developer, 2026-10-28):** a promotion is hardly quiet.
3. **The criterion: the best move of the search that labelled the position (2026-10-28),
   Stockfish's rule.** A position is dropped when in check or when that best move is a capture or a
   promotion. Every training position already carries it beside its score,
   `{<score> <best>}`, so no source needs a pass over its data:
   - **Lc0's path:** Lc0's own search (`crates/smithy/src/lc0/convert.rs`). Strong, where Rotor's
     search at generation 1 is not (the developer, 2026-10-28).
   - **PeSTO's path:** Rotor's own depth-9 self-play search (best-move-label.md): PeSTO's evaluation
     at generation 0, generation i-1's network at generation i. Nothing from outside Rotor.
4. **No outside engine in a path's training data (the developer, 2026-10-28):** using Stockfish is
   fair on Lc0's path, never on PeSTO's, where neither Lc0 nor Stockfish may be used. The rule in
   decision 3 keeps both clean: each path filters by its own labeller.
5. **Where: at the scatter.** A bucket record holds the position and its labels at lambda 0 and 1,
   no best move (`buckets.rs`), so the filter applies where the PGN is read: a filtered run has its
   own buckets, a scatter of about 6 minutes on Lc0's corpus. A config key turns it on.
6. **Tablebase labels computed once, applied separately (the developer, 2026-10-28):** the tablebase
   result of every position of 6 pieces or fewer, from a probe-only pass (no search), stored and
   applied by a config key of its own, so its effect is measured apart from the filter's. The first
   runs use the filter alone.

## Fallbacks

- **SEE, not planned:** in check, or a capture winning material by SEE (`engine_loss::kind_of`, about 71% of
  Lc0's test positions kept). Needs no best move.
- **Superseded 2026-10-28 by Lc0's second move (step 2), kept for the record; Stockfish not run.**
  **Depth 6, MultiPV 2, Lc0's path only, `-quiet-sf`:** dropped when either
  of the two best moves is a capture, Stockfish's filter of 2022 (PR #4295). Adds the second-best move to Lc0's best. **Run by
  Stockfish** if ever run: strong and independent of every network compared, already driven by
  `sibling_reference`; Rotor's generation 1 search is weak, and would bias the filter toward its
  network. Stockfish's depth 6 is not Rotor's (below). The tablebase probes would go in the same pass. PeSTO's path never needs it: its labels' own depth-9 search already
  gives a stronger best move.

Rotor's own costs, measured for this on 2026-10-28 by `rotor bench <depth>` (the 49 bench positions,
one thread, a fresh 16 MB table each):

| depth | nodes a position | time a position | 1B positions |
|---|---|---|---|
| 9 | about 63,600 | about 14.5 ms | about 4,000 core-hours |
| 6 | about 10,900 | about 3.2 ms | about 890 core-hours |

Stockfish's, by its own `bench 16 1 <depth> default depth` (51 positions, one thread, MultiPV 1),
the same day:

| depth | nodes a position | time a position |
|---|---|---|
| 9 | about 5,650 | about 3.3 ms |
| 6 | about 1,450 | about 2.0 ms |

Depth 9 is 3.9 times depth 6's nodes but 1.6 times its time: fitted to both runs, about 3.2M nodes/s
of search plus about 1.6 ms a position of fixed cost, the bench setting each position up afresh.

**Measured in one process (2026-10-28):** the 9,298 roots of the sibling-ranking cache (game
positions, 7 pieces up, out of check, in game order), one Stockfish fed `position fen` and `go depth
6` for each, a 16 MB hash kept across positions, no `ucinewgame`; every search completed.

| MultiPV | total | a position | 1B positions |
|---|---|---|---|
| 2 | 12.35 s | about 1.33 ms | about 370 core-hours |
| 1 | 7.39 s | about 0.79 ms | about 220 core-hours |

The second line costs about 70% more. Lc0's corpus has more endgames (cheaper), and every core busy
at once slows each process somewhat (dearer): about 370 core-hours stands, about 12 to 15 hours of
wall-clock with every machine idle. Run only over the positions `-quiet` keeps (758,359,347 of
999,893,639, 75.8%, its scatter of 2026-10-28), about 280 core-hours.

## Judging

**The duels decide:** depth 9, then 10+0.1, both sides `@default`. Reported beside them, deciding
nothing:

- Concordance at quiescence against the cached Stockfish pass (`sibling_ranking`): SCReLU's 80.95%
  the baseline, about 15 Elo a point; the gap between the static and quiescence columns is what to
  watch.
- Test loss on the quiet test set, and the old network's loss on it too, so both are compared on
  the same positions. A sanity check only, loss being known not to screen (loss-and-strength.md).

## Open

- **Tablebases on PeSTO's path:** Syzygy is computed by retrograde analysis, learned from no engine,
  and Rotor probes it in play; whether its labels are allowed there.
- **The judge on PeSTO's path:** `sibling_ranking` scores against Stockfish. Measuring puts nothing
  into a network, but deciding which PeSTO-path network goes on would let Stockfish's judgement
  steer the line by selection: whether that path is judged by games alone.
- **The tablebase label:** a win, draw or loss mapped to Rotor's mate scale or to a fixed score;
  and rescoring those positions against dropping them from training, the engine probing them anyway.
- **Success:** whether a drawn duel counts, decided after it ends.
- **What else carries over:** Stockfish also drops a share of positions at random
  (`--random-fen-skipping`); not proposed here.
