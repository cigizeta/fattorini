# Lc0's data for value and policy networks: implementation plan

**Archived 2026-11-04 with lc0-policy.md. Phase A (visit counts kept by the reader and the Convert)
is done and in use; step 4 is moot (lc0-counts-forges.md); phase B waits for a new design.**

**Status:** proposed 2026-10-29, to discuss. The design: lc0-policy.md. Steps 1 to 3 do not depend
on `-quiet`'s result; step 4 and after assume it succeeds.

## Two phases

- **Phase A, the corpus (now):** the reader accepts the counts, the converter writes them, Lc0's
  275 tars reconverted (about 16 h unattended). Every later network needs it, and the download is the
  long part.
- **Phase B, the consumers (when the first network with a policy head is designed):** `lc0-policy`'s
  buckets and record, a frozen set with counts, the transformer's stream passing them. Built against
  that network's needs, not guessed now: no policy network exists to test them end to end.
  `lc0-value`, the one consumer that needs no counts, is in phase A (step 4).

## Decisions

1. **The reader first:** `game_labels.rs`'s comment parser today refuses a third field ("holds more
   than a score and a best move"). It learns the counts before any file holds them, so no corpus is
   ever unreadable. Old comments, a score alone or with a move, still read.
2. **Counts checked on reading:** their number must equal the position's legal moves, a free check
   of record against position; a mismatch fails the read, as an illegal best move does.
3. **Revised 2026-10-29: every reader keeps the counts,** the quiet filter reading the second move
   from them (the developer: the quiet filter changed, no threshold). Was: **Value-only readers pay
   nothing more than parsing:** the counts are kept in the collected game
   only when a reader asks for them; the scatter into `lc0-value` and the freezer skip them.
4. **The converter always writes counts** from the reconversion on: `convert_game`'s flag set by
   the Convert job, no config key (one corpus, decision 1 of the design).
5. **`lc0-quiet` renamed `lc0-value`** by the developer, `buckets.tsv` unchanged (it names no
   directory), and the forge's choice of directory follows (design, decision 5).
6. **`filter:` until `lc0` goes:** a value-only config without `filter: quiet` still reads `lc0`, so
   old configs run unchanged; the filter becomes implied, and its absence refused, only when `lc0`
   is deleted (design, decisions 7 and 8). Until then a new value-only config states `filter: quiet`.

## Open, before phase A starts

- **The new corpus beside the old (decided, the developer, 2026-10-29):** `data/lc0-counts/` with its
  own `tars.tsv`, the old `data/lc0/` kept until the new one is verified; the frozen sets and the
  scatters read the new one from then on. Its segments' positions are the old ones' exactly (same
  tars, same games, same order), so `lc0-value` and the frozen sets stay valid without rebuilding.
  To check in step 3: the old and new segments give the same positions and labels.
- **When:** after `-quiet`'s duels (TODO), or now, the training machine busy anyway and the download
  running on another machine.

## Steps, phase A

1. **The reader:** `CollectedSelfPlayGame` carries each move's counts when asked; tests: an old
   comment, a comment with counts, a wrong number of counts refused.
   **Written 2026-10-29, to build and test:** `CollectedSelfPlayGame::counts`, parsed always (a
   malformed one fails any reader), kept only by `SelfPlayGameCollector::with_counts` (test-only
   until a policy reader exists), their number checked against the legal moves in
   `self_play_game_kept`.
2. **Written 2026-10-29, to build and test:** the Convert's corpus a hard-coded constant (the
   developer: no setting), `lc0::LC0_ROOT = "data/lc0-counts"`, its `tars.tsv`, `complete.txt` and
   staging under it, every game converted with counts; `lc0::is_complete` still `data/lc0`'s, the
   corpus the forges read, `converted_complete` the new one's; the Convert's lease on the new root.
   `lc0-value` and `lc0-policy` are to be hard-coded constants too (step 4, phase B).
   **Planned:** the Convert job writes counts (`convert_game(..., true)`), into the corpus
   decided above; a test that a converted game reads back with its counts, as
   `a_converted_game_reads_back_as_a_self_play_game` does without.
3. **Step 2 done 2026-10-29.** **Written 2026-10-29:** the ignored test
   `a_segment_with_counts_holds_the_old_segment_s_games` (`game_labels.rs`), the same tar's segment
   from each corpus compared game by game. **Deferred (the developer, 2026-10-29):** the Convert and
   the comparison run in full, the reconversion with them (step 5), once the current experiment
   (`-quiet`) ends. **Checks:** `cargo test`, `cargo clippy`; a dev smithy's Convert at a small scale, its segments
   read back with counts; the new segments' positions and labels compared with the old ones' on the
   first test tar. **Passed 2026-10-30** on the first test tar's first segment
   (`training-run2-test91-20260101-0017-00`), read from Drive while the Convert went on.
4. **Written 2026-10-29, to build and test:** `LC0_VALUE_BUCKETS = "lc0-value"` and
   `source_buckets`: Lc0's quiet buckets are `lc0-value` once `preprocessing_config::LC0_ROOT` is the
   corpus with counts, `lc0-quiet` until then; no rename by hand (the old `lc0-quiet` buckets stay
   `-quiet`'s, the new ones scattered fresh, the filter having changed). **Done 2026-10-29.** Left
   for after the reconversion and the comparison: `preprocessing_config::LC0_ROOT` set to
   `data/lc0-counts`, the one line that moves the forges. **Planned:** `lc0-value`: the forge's bucket directory by the network's kind; the rename by the developer.
5. **The reconversion,** by the developer: about 16 h, the raw tars kept. **Cap 32 GiB (the
   developer, 2026-10-29),** `configs/lc0.yaml`'s 5 GB holding only about a quarter of the old
   corpus's positions once comments carry counts (21.45 bytes a position against 4.85). 32 GiB goes
   past the old corpus's tars (about 22 GB would match them), since value networks filter: the new
   corpus starts with the old one's tars, in the same order, so step 3's comparison still holds on
   the tars both have. **Lowered to 24 GiB (the developer, 2026-10-31),** at 44.9% of 32 GiB: about
   1.2 billion positions, still every tar of the old corpus (about 1.0 billion) and a fifth more,
   and the converting machine free about 9.5 hours sooner. A network trained on it has about 20%
   more positions than `-quiet` had: a narrow win over `-quiet` would need that ruled out, as by a
   run on the old corpus's tars alone.
6. **Docs:** lc0-policy.md, TODO.md, DEV.md's corpus section if it names the directory.
7. **`data/lc0-counts` renamed `data/lc0` (the developer, 2026-10-29),** once the old corpus is
   deleted, `lc0-counts` being a name for the transition only. With no Convert or forge running: the
   old `data/lc0/` deleted on Drive, then `data/lc0-counts/` renamed `data/lc0/` there, its frozen
   sets moving with it. In one commit: `lc0::LC0_ROOT` back to `data/lc0`, `TRAINED_COMPLETE_PATH`
   gone, `source_buckets`' `lc0-quiet` fallback gone. Locally, `data/buckets/lc0/` and
   `data/buckets/lc0-quiet/` deleted on every machine; `lc0-value` keeps its name.

## Phase B, sketched

- `lc0-policy`'s record: a type of its own, the position, its two labels, the counts as varints.
- The scatter's choice of record by the network's kind; the gather's shuffle over variable-length
  records.
- A frozen set with counts (`frozen-policy`, or counts added to `frozen`).
- `pgn_train_stream.rs` passing the counts.
- Planned in a document of its own once the first policy network's design says what it reads.
