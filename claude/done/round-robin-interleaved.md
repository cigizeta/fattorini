# Round robin interleaved: one pair of every pairing, then the next

**Status:** done; generation 2's selection ran on it (checked 2026-10-14).

Agreed 2026-10-11, with players cached per worker (memory to be watched on the training machine). Earlier status: step 1
done (`runner::{Recording, play_games, write_pair, recorded_pairs}`); step 2 done
(`round_robin::interleaved_schedule`); steps 3 and 4 written together
(`round_robin_interleaved.rs`, called from `run_selection`), since the phase alone would fail the
dead-code build. Step 3's end-to-end test was not written: playing real games needs the opening
book and tablebases on disk, which no arena unit test reads; the first run on betty gen-0002 is the
check.

## Why

`arena select` plays the round robin one pairing after another (Berger order), each pairing's
`round_robin_pairs` pairs with every core busy on it (`main.rs`'s `run_selection`, `runner::play`).
With betty's eleven candidates that is 55 pairings of 100 pairs, 11,000 games, and the live table is
balanced only after each Berger round of five pairings: for most of the run some candidates have
played 200 games more than others, and the provisional ranking compares unequal samples.

The developer wants every candidate to have played about the same number of games at every moment,
with every core still busy: the same games, scheduled differently.

## Design

### The schedule

Passes, pair-major: pass `p` plays one pair of every pairing that still has fewer than `p + 1`
pairs, in Berger order within the pass; pass `p + 1` starts handing out pairs once pass `p`'s are
all handed out. One shared pool of workers, one per logical core at a fixed depth as today
(`Run::placement`), takes the next pair from that list. So at any moment the pairs in flight are a
window of about one pass (55 pairs against the training machine's workers), and every pairing's count is within
one or two pairs of every other's.

At the end every pairing has exactly `round_robin_pairs` pairs, as today, so the final ranking is
the one today's schedule would give from the same games (`round_robin.rs`: balance is what makes
points valid).

### Where it sits

A new phase at the start of `run_selection`, before today's pairing loop, which is left as it is:

1. **Interleaved phase** (new): brings every pairing to `round_robin_pairs` recorded pairs on the
   schedule above.
2. **Today's loop**, unchanged: for each pairing in Berger order, `play_duel` with
   `PairingRule::RoundRobin`. Every pairing is now complete, so `runner::play` rebuilds its record
   from its games and returns at once ("from the recorded games"), giving the `selection.txt` lines
   and the results the standings are computed from, exactly as today.

The margin search and the gate follow unchanged.

### Records stay where they are

Each pairing keeps its folder, `gen-NNNN/selection/<earlier>-vs-<later>/`, its `spec.txt`, `games.pgn`
and `results.db`, written exactly as `runner::play` writes them: whole pairs only, both games in one
transaction (`results_db::insert_pair`), PGN beside, pair indexes `2k` and `2k + 1`, challenger White
in game `2k`. So:

- the games already played in the stopped run count, whatever pairings they are in;
- a pairing's count is its whole recorded pairs (`runner::replay`), its next pair index one past the
  highest used, as today, so the gaps a Ctrl+C leaves are never reused;
- selection folders have no `spec.txt` (`play_duel` claims none), so there is nothing to claim.

### Workers and players

A worker takes a pair `(pairing, index)`, plays its two games with an opening drawn from the book,
records them under that pairing's lock, and takes the next. Pairs alternate between pairings, so
a worker needs whichever two candidates the pair names:

- **Each worker keeps one player per candidate it has met, started on first use** (in-process at a
  fixed depth, as `play_duel` builds them; `new_game` clears the table between games as today).
  Eleven candidates and the default hash are 11 x 16 MB per worker, about 3.5 GB on 20 workers;
  `check_memory` is told so. The alternative, rebuilding the two players for every pair, costs a
  network load and a table allocation per pair; caching is simpler to get right too.

### Stopping, faults, display

- **Ctrl+C**: workers stop at once, as today (`finish` aborts games within a move); pairs in flight
  are dropped whole, so every database still holds whole pairs only. A rerun recounts and goes on.
- **A fault** (a player that crashes or fails to start) stops the phase, as it stops a run today.
- **Live table** (on a terminal): redrawn after every recorded pair from every pairing's current
  record (`round_robin::standings`), provisional until the end, now with every candidate's games
  near equal. The message names the pass: `pass 37/100`.
- **In a log**: one line per recorded pair, `pass p: <a> vs <b> pair k, <record of that pairing>`.

## Implementation steps

Each step its own commit, tests passing.

1. **Make the per-pair pieces of `runner.rs` usable from a second caller**: playing one pair
   between two players (the body of `run_worker`'s loop), `replay`'s count and next index, and the
   whole-pair write. No behaviour change. Tests: today's runner tests pass unchanged.
2. **The schedule** as a pure function: from each pairing's recorded count and next index, the
   list of `(pairing, pair index)` in pass order. Tests: counts that differ (a resumed run) are
   evened out first; no pairing goes past `round_robin_pairs`; an index already used is never
   handed out.
3. **The interleaved phase**: the worker pool over the schedule, players cached per worker,
   recording per pairing, Ctrl+C and faults, the live table. Test: a small grid (three candidates,
   two pairs, depth 1) ends with every pairing at exactly two whole pairs, and today's loop then
   rebuilds every pairing from its games.
4. **`run_selection`** calls the phase before its loop, and the memory check counts the cached
   players. Then the developer resumes betty gen-0002's round robin with it.

## Decided

- **Player caching**: one player per candidate per worker, about 3.5 GB on the training machine, rather than
  rebuilding two players for every pair. The developer watches memory on the first run.

  Measured on betty gen-0002's round robin (2026-10-11): arena's private working set 3.75 GB,
  as estimated, with eleven candidates cached on every worker. The machine's total read 15.2 of
  15.4 GB with no paging: Windows' standby cache, not the arena. Resuming the run the old schedule
  had stopped worked as designed: the pairings evened out over the first passes, then rose together.
