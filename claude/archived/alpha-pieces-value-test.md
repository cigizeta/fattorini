# alpha_pieces: can the value be learned?

**Superseded (2026-10-18), see transformer-streaming.md:** zelda's run on amanda-0002's dataset was
never done; experiment 5, `lc0-alpha-pieces` and `self-alpha-pieces`, asks the same question on the
streamed corpora.

**Status:** design, 2026-10-15, agreed with the developer step by step; the implementation plan
follows once this is reviewed. Implemented ahead of the network experiments (generation 3,
pairwise, stack), since it needs no the training machine time: it can train on the Lichess machine.

## The question

Can a small piece-centric transformer learn the value at least as well as a psq NNUE? The first
feasibility test of alpha_pieces (architectures.md's brainstorm, 2026-10-15): the value head alone,
trained on amanda's dataset and labels, its test loss against amanda's on the same test set. No
search, no policy, no engine: training only.

## Decisions

1. **The network is zelda**, the old transformer's name (the archived depth-0 experiment), at
   `configs/zelda.yaml`. A Z name keeps the transformers apart from the NNUE lineage, which runs
   from A upward.

2. **Small:** 33 tokens at most (`[CLS]` and up to 32 pieces), width 64, 4 layers, about 6.5M
   multiply-adds an evaluation. Proposed, open to change:
   - 4 attention heads, 16 wide each;
   - feed-forward 256 wide (four times the width);
   - full attention (kings-only stays available as a mask);
   - no dropout: the data is far larger than the model.

   The model is today's `transformer_network.rs`, unchanged. Its value head already outputs pawns
   with no sigmoid, so it shares every network's loss (architectures.md, "Output units and training
   loss").

3. **No padding: every batch holds positions of one length**, as the transformer track always did.
   The length-bucketed caches come back, for alpha_pieces only, in their existing format: one
   positions file and one label file per lambda for each piece count, 2 to 32, and each split. The
   reading side, the writers and the length picker still exist (`length_cache.rs`); only a builder
   is missing.

4. **`smithy regroup <teacher>`, the builder.** `smithy prepare` stays as it is for every
   architecture; `regroup` regroups the dataset `prepare` built, by piece count:
   - It takes the teacher (`bin/smithy regroup amanda-0002`), since a dataset is the teacher's,
     never a network's.
   - It reads `data/datasets/<teacher>`'s train and test halves and writes every row to its piece
     count's file, with **every label set** the dataset has, so no lambda is chosen here.
   - Output beside its source: `data/datasets/<teacher>/lengths/`.
   - Same positions, same labels, same test split as amanda's, only regrouped: the comparison is
     about the network alone. No games and no search, one pass over existing files.
   - Within a bucket, rows keep the dataset's shuffled order.
   - Not "re-prepare": it prepares nothing new.

   **Padding is avoided in training only, not at inference** (the developer, 2026-10-15): a GPU
   batch of MCTS leaves mixes piece counts, so inference pads to 33 tokens with a mask. That puts a
   requirement on the model, not on this test: padding tokens must not change any output, so a
   padded position, masked, gives exactly what it gives alone. A test of the model checks it now,
   so a network trained without padding is known to serve padded batches later.

5. **Zelda's config names its data:** the teacher whose regrouped dataset it trains on, and one
   lambda. This replaces the transformer track's `preprocessing.pgn` and `depth` and the
   stem-keyed readers in `main.rs` (`stem_dirs`, `open_readers_for_stems`), which point at caches
   nothing can build (retire-rotor-curate.md, "Left for a later step"). The lambda is amanda's
   generation-3 winner, known once generation 3's selection runs; the config takes it then.

6. **No generations for zelda.** It trains once, on one teacher's dataset; `promote` and `upload`
   keep refusing a transformer.

7. **Training reads the buckets as before:** at each step, the length to draw from is picked with
   probability proportional to the rows that bucket has left (`pick_length_index`), so a pass over
   the data visits every position once, mixed across lengths.

8. **The comparison: test loss against positions seen.** amanda's `history.tsv` records her test
   loss per round; zelda's run records its own. Zelda is compared with amanda **after the same
   number of training positions**, not only at the end: on the Lichess machine's CPU (an estimated 3,000 to
   5,000 positions a second, about a day per 300M), a full pass may take days, and the curves say
   early whether zelda is learning. Zelda's test loss is the mean over every bucket weighted by its
   rows, the per-position mean over the same test set as amanda's.

## Pass and fail

- **Feasible:** zelda reaches amanda's test loss at equal positions seen, or beats it. Piece
  attention learns chess at least as well as a psq NNUE, per position.
- **Not yet:** zelda learns but stays above amanda. Investigate (learning rate, depth, attack
  features), then perhaps the medium network (width 256, 10 layers) on the training machine.
- **Fails:** zelda's test loss does not fall meaningfully. Something is wrong with the data path or
  the model before the idea itself is judged.

## Open for the plan

- `regroup`'s resumability: one pass, so restarting from the beginning after an interruption may be
  enough; each bucket's `.count` sidecar already marks a finished file.
- The disk cost: a second copy of the dataset's positions and labels.
- What else of the transformer track is stale besides its data source (`transformer_round.rs` and
  `transformer_window.rs` looked near-identical in 2026-09).
- zelda's training fields: batch size, learning rate schedule, how often to test.
