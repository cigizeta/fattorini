# Self-play portion chosen at training time

**Status:** abandoned 2026-10-15, never implemented. Even cut to 20% against 50% on one network, the
experiment cost too much (the self-play alone four times 20%'s), and reasoning settled it instead
(the developer: "we stick with 20%"): psq networks are short of capacity, not data, so a larger set
adds little by size; the same teacher labels both parts, so only the mix changes; self-play matches
the search's own positions, but the archive shows where strong play goes; the cost is certain, the
gain likely below what a duel resolves. Kept for the reasoning. The one cheap piece worth having
some day, a replay/self-play flag per row for split test losses (decision 2 and 8), is in TODO.

## The question

Does more self-play on top of the archive pay? Shares 10, 20, 30, 40 and 50%, the replay's
570,432,102 positions always whole, so each share is a larger training set:

| Share | Self-play | Training set |
|---|---|---|
| 10% | 63.4 M | 633.8 M |
| 20% | 142.6 M | 713.0 M |
| 30% | 244.5 M | 814.9 M |
| 40% | 380.3 M | 950.7 M |
| 50% | 570.4 M | 1,140.9 M |

Size and mix change together, accepted: dropping replay positions is never a real option, so the
only practical choice is how many self-play games to play (the developer, 2026-10-15). This replaces
TODO's earlier fixed-total design.

The judge is Stockfish 19 level 10 at 10+0.1, as TODO records; test losses are a secondary signal.

## Decisions

1. **`corpus.yaml`'s `self.portion` is the ceiling: what is played and prepared.** Set to 50%
   (done, 2026-10-15): `arena self` plays until 570.4M self-play positions exist, and `prepare`
   builds **one** dataset at 50%. No share is prepared twice.

2. **A flag per training row: replay or self-play.** One bit a row, in its own file beside the label
   sets, in the train half's shuffled order, written by `prepare` in the pass that writes the
   positions. The position format is unchanged. About 1.14 billion train rows, so about 142 MB. The
   test half gets one too.

3. **The keep rule: a self-play row is kept when `row index mod 2520 < keep x 2520`**, replay rows
   always. The row index is the row's place in the shuffled train file, the cursor the reader
   already keeps, wrapping at the end. No hash and no seed:
   - **Sound because the train half is shuffled:** `prepare` scatters rows into buckets by hash and
     permutes each bucket (Fisher-Yates), so a row's index says nothing of its content, and any
     fixed rule on the index picks a random set. It rests on that shuffle alone, and on `prepare`
     refusing an unshuffled dataset, which it already does.
   - **2520** is the smallest number every integer from 1 to 10 divides, so the experiment's
     probabilities are exact (1/9, 1/4, 3/7, 2/3, 1: 280, 630, 1,080, 1,680, 2,520 of every 2520);
     others round to the nearest 1/2520.
   - **The shares nest:** a row kept at 10% is kept at every higher share.
   - **Sizes are inexact by about 0.01%** (self-play rows sit at random indices among replay rows,
     so their residues are binomial), accepted: the shares differ by 63M positions and more.
   - The drop happens as rows leave the file (`CacheFile::read_records_cyclic`), before the shuffle
     buffer, so batches stay 4,096 rows at every share.

4. **The share is decided at training time, never in a network's config:**
   `smithy forge <network> --self-portion <p>%`, **required**: without it `forge` refuses, so a run
   never trains on an implicit share. The keep probability is `p / (1 - p) x replay rows / self-play
   rows`, both counted from the flags; a share above what `prepare` built is refused, and so is a
   dataset without flags. Recorded in `run_config.txt` and frozen as the training fields are: a
   resume needs no flag, and one giving a different share is refused.

5. **Every run's folder names its share:** `gen-NNNN/self-<p>/lambda-<l>/`, for every network and
   every generation, the ordinary flow being one share (20% so far). Two shares never collide.
   Generations 1 to 3 are renamed by hand on disk into `self-20/` (the developer, 2026-10-15), so
   the code has one layout and no legacy path.

6. **Each share trains the whole lambda grid, then `arena select` picks its winner** (the developer,
   2026-10-15): the best lambda may move with the share, so a lambda fixed at 20%'s winner would
   favour 20%. Five forges, five selections, then the five winners against Stockfish level 10.

7. **Resume.** Today the cursor is `steps x batch_size` rows modulo the train rows (`main.rs`), exact
   only because every step reads exactly a batch. With drops, a resumed run walks the flags from
   row 0 to the row where `steps x batch_size` kept rows were read: 142 MB of bits, about a second,
   never the positions.

8. **Test losses:** on the test rows from replay months only, identical for every share, so their
   losses compare; and on self-play test rows separately (TODO's cheap first signal). Biased toward
   the archive, which is why Stockfish judges.

## What follows from 5

Every command that finds a run needs its share: `arena select`, `smithy promote`, `smithy check`,
`calibrate` and `correlate` (which already take `--lambda`), and arena's player names
(`amanda-0004-0.95` points at a lambda's candidate). Each takes the share or finds the single share
present, **refusing when there are several** rather than picking one. The plan settles the
spellings (a player name such as `amanda-0004-s30-0.95`, perhaps).

## Accepted caveat

Dropping rows scatters the kept self-play positions over all the self-play games, where really
playing fewer games gives fewer, whole games: a small share sees more varied positions than it would
in practice, which may flatter the small shares slightly. Accepted for the simplicity of one
dataset (the developer, 2026-10-15).

## Open

- **Generations 1 and 2's label.** Their corpus may have been self-play alone (amanda.yaml's header
  comment), so `self-20` would be a convention there, not a description; the developer's call when
  renaming.
- **The cost of the grid.** `forge` trains the lambdas in lockstep over one stream, so an extra lambda
  costs its own GPU work but no extra read; how 11 compare with 1 is unmeasured. Five forges of 11
  arms each on up to 1.14 billion rows is the experiment's the training machine bill.
- **Self-play time.** 570.4M self-play positions, four times 20%'s, on all three machines.

## In passing

`corpus_config.rs`'s comment on `SOURCES` says the test split takes its positions from the first
source; the build draws them from every segment (`dataset_cache::test_fraction`). Fixed with the
implementation.
