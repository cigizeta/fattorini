# The forges on data/lc0-counts

Status: designed 2026-11-02, to discuss. Step 0 of the halfk line (TODO.md): every Lc0 network from
here trains on `data/lc0-counts`, unfiltered; first `psq_threats_screlu` retrained on it, the
baseline the halfk steps are judged against. Replaces lc0-policy-implementation.md's step 4 for
value networks (the quiet buckets moot without the quiet filter).

## What the code does today (read 2026-11-02)

- **Two roots.** The forges read `preprocessing_config::LC0_ROOT = "data/lc0"`; the Convert writes
  `lc0::LC0_ROOT = "data/lc0-counts"`. `lc0::is_complete` reads `data/lc0/complete.txt`
  (`TRAINED_COMPLETE_PATH`), `lc0::converted_complete` the new one's.
- **The reader already takes the counts.** `game_labels.rs` parses a third comment field, checks its
  length against the legal moves, and keeps it only when asked: value networks read it and drop it.
- **The buckets rescatter by themselves.** `data/buckets/lc0` keeps the paths of the segments it
  scattered (`buckets.tsv`); the new corpus's paths differ, so the first run finds "the training
  set changed" and scatters everything again. Nothing old is read silently.
- **The frozen set is deterministic.** Algorithm R under a fixed seed over the test segments in
  order: the same test games give the same positions in the same slots.
- **`engine_loss.rs`** hard-codes `data/lc0` and `data/lc0/frozen` for its checks by hand.
- **No committed config has `filter:`**: the quiet filter's code stays, unused.

## What the new corpus holds (Drive, 2026-11-02)

- **Complete:** `data/lc0-counts/complete.txt`, 362 tars, 1,199,290,155 training positions in 1,397
  segments, 25.7 GB of PGN; the old corpus 275 tars, 999,893,639 positions.
- **Not a superset of the old one.** The Convert skips a slow tar for good (`skipped.tsv`), so about
  48 of the old corpus's 275 tars (5 to 12 January) are missing, and 135 later tars (12 to 20
  January) take their place: about 227 tars in common. TODO.md's "every tar of the old corpus and a
  fifth more" is wrong; to correct there. For a value network this changes nothing of substance (the
  same run of Lc0's games, a week later), but step 0's duel against `g1-r1` measures the new corpus as
  it is, tars included, not the counts alone.
- **The same test set.** Both corpora's test tars are the first two (1 January, 00:17 and 01:17),
  and the first test segment's games were checked equal by hand (2026-10-30). If the other test
  segments are equal too, the new frozen set is the old one position for position, and every old
  network's test loss stays comparable.

## Proposed

1. **The switch, one constant:** `preprocessing_config::LC0_ROOT = "data/lc0-counts"`;
   `lc0::is_complete` reads the new `complete.txt` (`TRAINED_COMPLETE_PATH` gone, `converted_complete`
   its only definition); `engine_loss.rs` reads `preprocessing_config::LC0_ROOT` and its frozen set,
   not its own constants. The rename back to `data/lc0`, once the old corpus is deleted, stays
   lc0-policy-implementation.md's step 7.
2. **The new frozen set checked against the old one:** frozen on the first run as any source's
   (`TestSet::of`, under the source's lease), then its `test.positions` and its labels at the
   run's lambdas compared byte for byte with `data/lc0/frozen`'s. Equal: the old test losses stay
   comparable, said so in the TODO. Not equal: the difference is found before anything trains on it.
   An ignored test, run by hand, as `a_segment_with_counts_holds_the_old_segment_s_games`.
3. **When:** after `lc0-psq-threats-screlu`'s generation 2 has ended its gate, so that no running
   run's sources change under it; nothing is deleted.
4. **Step 0 itself (decided 2026-11-02):** every Lc0 config keeps its name and moves to the new
   corpus; no config of its own. `lc0-psq-threats-screlu` trains again on it, unchanged, as a new
   run, and its winner replaces the old corpus's network whatever the result. The old network's
   only use left is one duel against the new winner, both `@default`, depth 9 and 10+0.1, to
   measure the difference. Then `lc0-halfk-threats-screlu` against step 0's winner.
5. **Stale data deleted:** once the frozen sets are compared (2), the old corpus, its Drive copy
   and the old buckets go, by hand and by the developer; the old network's file is kept until its
   duel is played.
6. **The old network kept as a record (decided 2026-11-02):** after today's experiment
   (generation 2 and its gate), the old corpus's `lc0-psq-threats-screlu` is renamed `emily`
   (`emily.yaml`) and kept as a historical record; `lc0-psq-threats-screlu` then names the new
   corpus's run alone.

## Open

- The scatter's cost: 1.2 billion positions, the visit counts parsed and checked against the legal
  moves of every position once. To time on the first run; a concern only if it takes many hours.
