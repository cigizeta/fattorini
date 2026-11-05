# Quiet filter: implementation plan

**Archived 2026-11-02:** the filter failed (quiet-filter.md) and its code is removed; `Keep::Quiet`
no longer exists.

**Status:** steps 1 to 7 written 2026-10-28, to build and test (step 8). `game_labels::Keep`
(`Searched`, `Quiet`, `Every`) decides which positions a stream and a freeze keep; `filter: quiet` in a
forge config. The transformer's stream keeps `Every` until its epochs are counted otherwise, and a
transformer config refuses `filter:`. **Self-play and PeSTO's corpus cannot freeze a test set on this
code until their test target counts kept positions** (Open): no self-play generation or PeSTO run
before that. The design: quiet-filter.md. This plan builds `-quiet`
(Lc0's best move) only; `-quiet-sf` gets its own plan if `-quiet` fails.

## To resume (2026-10-28)

1. **The real run** (step 10), on the training machine once free: pull, start the smithy, start
   `lc0-psq-threats-screlu-quiet` from the page. Expect the freeze to report about 6.0M quiet positions
   of the 66,542 test games, 4,194,304 frozen into `data/lc0/frozen-quiet`, then the scatter into
   `data/buckets/lc0-quiet`.
2. **Meanwhile, self-play's test target** (Open): design first. Proposed: self-play counts, game by
   game, the positions the run's `Keep` keeps, and stops its test games at `test_positions` kept, so
   the freezer always finds enough. Blocks every generation 2 and every PeSTO run.
3. **After the run:** its winner's duels against `lc0-psq-threats-screlu-g1-r1`, both `@default`,
   depth 9 then 10+0.1; what counts as success decided then (quiet-filter.md). Clearly loses:
   the filter dropped. Draws: `-quiet-lc0` only if the cheap check of quiet-filter.md's step 2
   passes. Succeeds: stop and think, then pairwise_hidden-quiet.

## What is built

A fix for every config, the game's final position no longer a training position, with the data
rebuilt; and a config key that drops non-quiet positions from a run's training set and test set alike, applied
where the PGN is read, with buckets and a frozen test set of its own.

## Decisions

1. **The predicate, `is_quiet(board, best)`:** false when the side to move is in check, or when
   `best` is a capture (en passant included) or a promotion. A small module of its own in smithy,
   with unit tests on each case.
2. **The final position is dropped from every config, quiet or not (the developer, 2026-10-28):**
   no move is played from it, so no search ever labelled it: no score, no best move, the result its
   only label. It carried the result back to the game's positions under AB(lambda); the direct
   blend labels each position with the result itself, so it has carried nothing since AB(lambda)
   went. A fix of the current implementation, under 1% of the data (Lc0's games about 120
   positions each). Book plies are never training positions, as before.
3. **The config key:** `filter: quiet` at the config's top level, absent meaning no filter (every
   existing config unchanged). Frozen in the run's config (`run_config.rs`), so a run cannot change
   its filter on resume. `quiet-sf` is refused until built.
4. **Where it applies:** in the reading of a game's labelled positions (`game_labels.rs`, beside
   `self_play_game_best_moves`), so the scatter and the freezer drop the same positions by the same
   code.
5. **Buckets of its own:** `data/buckets/<source>-quiet` instead of `data/buckets/<source>` for a
   sourced generation (`smithy.rs`, the bucket directory's choice); a self-play run's buckets are
   already its own.
6. **A frozen test set of its own:** `<source root>/frozen-quiet` beside `frozen`, drawn from the same
   test games, Algorithm R over the quiet positions only, exactly `test_positions` of them (4,194,304);
   an error, not a smaller set, if the test games hold fewer. The manifest records the filter.
7. **Position counts:** the segments' `.meta` count every position, filtered or not. Wherever a count
   sizes something (an epoch's length, a progress bar's total, the freezer's check that it read
   what `.meta` says), the filtered count is used or the check compares unfiltered reads. Each use
   found and listed in step 1.
8. **The data rebuilt (the developer, 2026-10-28):** the Lc0 buckets scattered again and the
   sources' frozen test sets (`data/lc0/frozen`, PeSTO's) frozen again, without final positions.
   Their manifests mark it; a set frozen before is refused with a message to delete it, as one
   holding K vs K is (`grouped_test_set.rs`). A finished generation's own test set stays as it is.
   Old networks stay, scored on the new set through decision 10 when a comparison needs it.
9. **The config:** `configs/lc0-psq-threats-screlu-quiet.yaml`, `lc0-psq-threats-screlu.yaml` with
   `filter: quiet` and its header comment saying so.
10. **Scoring an old network on a new test set:** `engine_loss.rs`'s `FROZEN_DIR` becomes a
   parameter (an environment variable, as its other diagnostics take), so the old SCReLU's loss is
   measured on the rebuilt set and on the quiet one.

## Survey (step 1, 2026-10-28)

**One choke point.** Every reader goes through `game_labels.rs`: `SelfPlaySegmentReader` reads a
game, `self_play_game_positions` replays its positions (the final one included, its best moves
already checked legal there), `self_play_game_labels` labels them, the final one with the outcome.
`position_count()` is moves after the book plus one.

**Readers of a game's positions:**

| reader | what it does | what changes |
|---|---|---|
| `pgn_stream.rs`, `worker` | replays and labels each game, keeps each position by `keep_probability`, sends the kept ones; its `Tally` counts every position read | the drop goes beside the random keep: read and counted, not sent |
| `buckets.rs`, `prepare` (NNUE) | scatters what `PgnStream` sends; `buckets.tsv` records the records written | nothing: an epoch is the records scattered, so it shrinks by itself. Its bar's total is the `.meta` sum and ends short of it, cosmetic |
| `pgn_train_stream.rs`, `EpochRecords` (transformer) | an epoch is the `.meta` sum; a stream that runs dry ends the epoch early | still correct (its comment allows `.meta` overcounting); a resume's position inside an epoch becomes approximate, as its header already allows |
| `freeze.rs`, `sample` | reads every position, skips K vs K before the reservoir, checks that it read what `.meta` says | the drop joins K vs K: read and counted, never a slot, so the `.meta` check holds unchanged |
| `engine_loss.rs` (diagnostics, tests only) | reads best moves and the frozen set | the frozen set's path becomes a parameter (decision 10) |

**`.meta` counts stay "positions read", final position included.** They are written at conversion
(`lc0/convert.rs`, records plus one) and by self-play (`smithy.rs`), and nothing need change there:
the drop happens after reading, as K vs K's does, so every count check holds. `position_count()`
keeps its meaning (positions read); the kept count is a new function beside it.

**One real risk: a test set too small.** A freeze needs `rows` kept positions:
- **Lc0's test split:** 65,536 games of about 120 positions, about 7.9M read. Without final
  positions about 7.8M; with the quiet filter dropping 20% to 40%, about 4.7M to 6.2M, above the
  4,194,304 needed, but at 40% only by about 12%. The freezer's error says so if not; the split
  would then need more games (`lc0.yaml`, `test.games`), a reconversion of the test split.
- **Self-play generations (PeSTO's path, and every generation 2):** their test games are played
  until `test_positions` positions are read, final positions included, so a generation could not
  freeze 4,194,304 kept positions: it would fail on the final positions alone, and far more on a
  quiet config. Self-play's test target must count kept positions, or play a margin more. Not
  needed for `lc0-psq-threats-screlu-quiet`'s generation 1, but needed before any generation 2 or a
  PeSTO run after this change, the final-position drop included.

**Where the predicate needs its inputs.** `self_play_game_positions` already holds each position's
board and checks its best move legal there: the natural place to compute, beside the positions, a
keep flag for each (final position false; with the quiet filter, false in check or with a capture
or promotion as best move).

## Steps

1. **Survey:** every reader of a segment's positions and of `.meta` counts (the scatter, the freezer,
   training's stream, the inspectors), listed here before any change, with what each needs.
2. **The predicate** (decision 1) and its tests.
3. **The config key** (decision 3): parsing, the run's frozen copy, the refusal of `quiet-sf`.
4. **The final position dropped** (decision 2), for every config: the reading's tests updated, the
   freezer's marker and its refusal of an older set (decision 8).
5. **The filter in the reading** (decision 4), off unless the key is set; a test that an unfiltered
   read is unchanged but for the final position, and one that a filtered read drops exactly the
   positions the predicate rejects.
6. **Buckets and frozen set of their own** (decisions 5 to 7).
7. **The config file** (decision 9), and `engine_loss.rs`'s test set as a parameter (decision 10).
8. **Checks:** `cargo test`, `cargo clippy`; a dev smithy smoke run of the new config, its buckets
   and frozen set landing in their own directories; the share of positions dropped logged, to
   compare with the 20% to 40% reported for Stockfish's data.
9. **The rebuild** (decision 8), by the developer: the old Lc0 buckets and the sources' frozen sets
   deleted, rebuilt at the next run's start. **Done for Lc0 (2026-10-28):** `data/lc0/frozen` renamed
   `data/lc0/frozen-with-final` on Drive, kept so that a loss recorded before can be checked
   (`ROTOR_FROZEN_DIR`); the freezer refuses it, its manifest holding no keep. The Lc0 buckets scatter
   again by themselves (`buckets.tsv` holds no keep). PeSTO's frozen set waits for self-play's test
   target (Open). **Smoke-tested the same day (step 8):** a dev smithy's run kept 75.8% of both the
   test games' positions (3,023,639 of 3,986,540) and the training set's (26,397,152 of 34,827,162),
   in its own `lc0-quiet` buckets and `frozen-quiet` set; the full test split should give about 6.0M
   quiet positions for the 4,194,304 needed.
10. **The run:** generation 1 of `lc0-psq-threats-screlu-quiet` on the training machine (width 512's
   run stopped first, from the page); its winner's duels against `lc0-psq-threats-screlu-g1-r1`, both
   `@default`, depth 9, then 10+0.1; concordance and the loss of both networks on the quiet test set
   reported beside them.
11. **Docs:** quiet-filter.md's results, TODO.md, DEV.md if a config key is documented there.

## Open

- **Self-play's test target:** count kept positions, or play a margin more (survey, "a test set too
  small"). Before any self-play generation runs on the new code.

## Deferred

- **Width 512's resume:** its training started on the old buckets and test set; resumed on the
  rebuilt ones or started over, decided when its turn comes (the developer, 2026-10-28).
