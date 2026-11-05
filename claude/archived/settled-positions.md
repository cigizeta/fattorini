# Training on the positions the search evaluates

**Archived 2026-11-02:** option 1, the quiet filter, failed and was removed (quiet-filter.md);
Lc0's networks train unfiltered, so options 2 and 3 are not pursued.

**Status:** draft for discussion, 2026-10-28. Option 1, the filter, is now its own experiment
(quiet-filter.md); options 2 and 3 wait for its result.

## Why

The network is trained on game positions, each labelled with its own score. The search almost
never evaluates a game position as it stands: it evaluates the leaves of its tree, after quiescence
has played the captures out, many of them lopsided (a piece just taken, the recapture not yet
possible, a promotion). The developer's observation (2026-10-28): the training set is the root, but
the network is never really asked about the root. A structural weakness the whole field should
share.

The measurements agree (sibling-ranking.md): pairwise_hidden orders a position's children better
than SCReLU by its static evaluation, and worse once quiescence has played the captures out; that
gap, not its fit, matches its lost duels. Concordance at quiescence ranks five networks in their
duels' order, about 15 Elo a point.

## What the field does (searched 2026-10-28)

- **Stockfish trains mainly on Lc0's data,** converted to its binpack format: its releases name over
  100 billion positions of Lc0 evaluation data, and nnue-pytorch's wiki finds Lc0-derived sets
  better than Stockfish-generated ones. The same choice as Rotor's.
- **Relabelled by tablebases, not by Stockfish's search.** Leela's data is rescored with Syzygy (6
  pieces, part of 7) before conversion (PR #4295, linrock's lc0-data-converter); the wiki's latest
  set is relabelled by a stronger Lc0 network. No relabelling by Stockfish's own search found:
  its search filters, it does not score.
- **The score blended with the result** by a lambda, as Rotor's direct blend (PR #4295:
  `--start-lambda=1.0 --end-lambda=0.75`).
- **Quiet positions only.** nnue-pytorch's `--smart-fen-skipping` skips positions in check or whose
  best move is a capture, 20% to 40% of the data by a forum report. PR #4295 filtered about half a
  dataset by a depth-6 MultiPV-2 search, dropping positions where either of the two best moves is a
  capture (its +4.7 Elo is the whole change's, not the filter's alone).
  **PR #4295 and the tools' PR #4324 read 2026-10-28:** no reason given for depth 6 rather than
  deeper (#4324's commit "Hardcode depth 6 and remove option to set depth"; only "the
  computationally expensive part of filtering is during search"), no cost figures, no ablation of
  the filter or of its depth. The author estimated it "throws away 25% more positions than
  smart-fen-skipping alone". The +4.7 +/- 3.1 Elo is a local test of the whole net at 25k nodes,
  besides its STC and LTC passes. Sopel97 proposed marking the filtered positions (`VALUE_NONE`)
  instead of removing them, a matter of the binpacks' compression only.
- **The leaf, once.** Nodchip's learner, Stockfish's first NNUE trainer, ran quiescence on each
  training position with the network being trained and trained on the leaf: option 2, circular. A
  talkchess thread asks why; Sopel's reply says that trainer is no longer used, Stockfish training
  only with nnue-pytorch. Why the leaf was not carried over: not found (the thread unreachable).
- No engine found training on positions sampled from its search tree.

So Stockfish lives with the gap by filtering, and judges by games only; nothing found measures the
gap itself. The first step here follows it: quiet-filter.md.

Sources: [nnue-pytorch, training datasets](https://github.com/official-stockfish/nnue-pytorch/wiki/Training-datasets),
[PR #4295](https://github.com/official-stockfish/Stockfish/pull/4295),
[PR #4314](https://github.com/official-stockfish/Stockfish/pull/4314),
[lc0-data-converter](https://github.com/linrock/lc0-data-converter),
[Stockfish releases](https://github.com/official-stockfish/Stockfish/releases),
[nnue-pytorch README](https://github.com/official-stockfish/nnue-pytorch/blob/0764091ec544446ae5852e3e7296f252a2046dd1/README.md),
[talkchess, stable positions](https://www.talkchess.com/viewtopic.php?p=940337),
[talkchess, qsearch in the trainer](https://talkchess.com/viewtopic.php?p=916873).

## Options

1. **Filter:** keep only quiet positions, not in check and with no capture winning by SEE
   (`engine_loss::kind_of`, about 71% of Lc0's test positions). Cheapest; the network then sees
   neither the roots it misjudged nor the leaves it should judge.
2. **The leaf in place of the root:** each position replaced by the end of its capture sequence,
   the label kept (negated when the side to move changes). The label stays right to the extent the
   sequence is forced; the input is now what the search evaluates.
3. **Both:** the leaves, and the quiet roots as they are (a quiet root is its own leaf, so this is
   option 2 with nothing to play out).

## Questions to settle

- **Whose capture sequence:** the leaf must not depend on the network being trained. A
  material-only resolution (SEE, or quiescence on a material evaluation) is independent and cheap;
  a network's own quiescence is circular.
- **Where it runs:** at Lc0's conversion (once, the corpus rewritten), or at the scatter into
  buckets (every forge). CPU work either way, over about 1B positions.
- **The test set:** transformed the same way, or not. Its loss then changes meaning, which matters
  less now that loss is known not to screen (loss-and-strength.md).
- **The judge:** concordance at quiescence against the cached Stockfish pass (`sibling_ranking`),
  then duels. The experiment: SCReLU's config, its data transformed, against
  `lc0-psq-threats-screlu-g1-r1`.
- **Order against width 512 and generation 2,** all three needing the training machine.
- **The label on Lc0's data:** keeping the root's label for its leaf assumes the label already
  reflects the capture sequence; true of a quiescence-based score, only roughly of Lc0's MCTS value.
