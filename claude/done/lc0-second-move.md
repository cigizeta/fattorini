# Lc0's second move, a filter without Stockfish

**Status:** done 2026-10-28: implemented, tests passing, run on the two test tars (117 s). **Lc0's
second move replaces Stockfish's filter (the developer, 2026-10-28):** `-quiet-sf` becomes
`-quiet-lc0`, gated as quiet-filter.md's step 2 says; Stockfish is not run for filtering.

## Results (2026-10-28)

66,542 standard games (2,800 skipped), 7,910,812 records.

1. **The target is raw visit shares:** in every record, the nonnegative entries exactly the legal
   moves, summing to 1, and multiples of `1 / (visits - 1)` (only 1.7% also of `1 / visits`). The
   top entry is `best_idx` in 99.41% of records, `played_idx` in 60.84% (Lc0 plays with
   randomness; its target is the search's).
2. **The second move is real:** its share mostly 0.1 to 0.3 (0.0-0.1 37%, 0.1-0.2 34%, 0.2-0.3
   19%, 0.3-0.4 8%); the best move's lead under 0.1 in 34% of records, over 0.9 in 11%.
3. **What it drops:** 6,047,491 quiet positions, exactly the freeze's count, so this walk and
   `Keep::Quiet` agree position for position. Of them, those whose second move is a capture or a
   promotion:

   | second's share | 3-6 | 7-13 | 14-20 | 21-32 | all |
   |---|---|---|---|---|---|
   | over 0 | 1.54% | 4.95% | 8.49% | 8.62% | 6.93% |
   | 0.05 or more | 1.48% | 4.28% | 6.98% | 7.33% | 5.88% |
   | 0.10 or more | 0.85% | 3.28% | 5.59% | 5.96% | 4.68% |
   | 0.20 or more | 0.23% | 1.47% | 3.04% | 3.25% | 2.45% |

   Far below the "25% more" reported for Stockfish's depth-6 MultiPV 2 (settled-positions.md), a
   remark about another dataset: likely because MultiPV 2's second line, by score, is any move,
   bad captures included, while Lc0's search hardly visits a bad move. Not proved.

**Next:** the cheap check of quiet-filter.md's step 2, if `-quiet` draws, on these two tars (kept
in `data/lc0-raw/` until then): `-quiet`'s winner scored on the dropped part against the kept. `convert_game`'s loop now goes through `convert::replay` and
`convert::standard_start`, shared with the diagnostic; its own tests should show it unchanged.
The second move is the highest entry other than `best_idx`'s, a never-visited one never counted.

## Why

quiet-filter.md's step 2 would run `-quiet-sf`, Stockfish's depth-6 MultiPV-2 filter, only if
`-quiet` draws, gated by a cheap check. What Stockfish adds over `-quiet` is its **second** move: a
position whose best move is quiet but whose second is a capture or a promotion still has a capture
as a real alternative, a label resting on tactics the board does not show. PR #4295's author
estimated it drops about 25% more positions than smart-fen-skipping alone (settled-positions.md).

Lc0's own records may hold that second move already: every V6 record carries `probabilities[1858]`,
the training target, read by no one today (`lc0/record.rs`). Its second-highest move is Lc0's own
second choice, from the search that labelled the position, stronger than Stockfish's depth 6. If it
is usable, the second line comes with no Stockfish and no 280 core-hours. The raw tars are gone
(Drive keeps `tars.tsv` alone), so this is measured first on the two test tars, about 1.7 GB, that
hold all 66,542 test games.

## Decisions (the developer, 2026-10-28)

1. **Downloaded by plain `curl`** into `data/lc0-raw/` (gitignored), by the developer, not by the
   smithy's downloader.
2. **The current run is not touched:** the analysis is an `#[ignore]` test in `crates/smithy`, a test
   binary of its own; the running `smithy.exe` is never rebuilt or stopped.
3. **Read only:** nothing is converted, written or uploaded; the tables are printed.

## The questions, in order

1. **What the distribution is,** over every record of a standard game (the converter's
   `is_standard_start`, so the same games as the corpus):
   - the legal moves' probabilities summed (1 within 1e-4?), and whether every illegal entry is -1;
   - whether the values are multiples of `1 / (visits - 1)` within 1e-4 (raw visit shares, the root's
     own visit excluded) or not (a modified target: temperature, pruned visits);
   - how often the top entry is `best_idx`, and how often `played_idx`.
2. **How strong the second move is:** the second's share, and the gap between first and second,
   each as a histogram by tenths.
3. **What it filters:** among the positions `Keep::Quiet` keeps (not in check, `best_idx` neither a
   capture nor a promotion), the share whose second move is a capture or a promotion, when the
   second's share is at least 0%, 5%, 10% and 20%; overall and by piece count in the groups 3-6,
   7-13, 14-20, 21-32. Compared with the "25% more" above.

The loss of `-quiet`'s winner on the kept and the dropped parts is a later step, and only if
`-quiet` draws: it needs the frozen positions matched to records, by FEN.

## Steps

1. `Record::probabilities()`: the 1858 f32 from offset 8, and `Record::visits()` from 8340; the layout
   comment's "(the policy: not read)" updated. A unit test on a synthetic record.
2. In `lc0/convert.rs`, the walk over a game's records reused: each record's board replayed and
   checked as `convert_game` does, the policy entries decoded to legal moves with `decode`, so the
   second move is a real move on a checked board. A capture is a move onto an occupied square or en
   passant; a promotion as `Move::promotion`; in check from `shakmaty`.
3. `lc0::second_move::tests::second_move`, `#[ignore]`: reads the tars named by `LC0_RAW_TARS`
   (paths separated by `;`), every `training.<id>.gz` member as `convert_tar` does, and prints the
   three tables. Checked first on the real game in `tests/data/` by an ordinary test that the
   distribution decodes (its top entry legal, its sum near 1).
4. Run by the developer:
   `LC0_RAW_TARS="data/lc0-raw/training-run2-test91-20260101-0017.tar;data/lc0-raw/training-run2-test91-20260101-0117.tar" cargo test --release -p smithy second_move -- --ignored --nocapture`
5. The results recorded here and in quiet-filter.md's step 2: Lc0's second move as the preferred
   route for `-quiet-lc0`, Stockfish as the fallback if the distribution proves unusable.
