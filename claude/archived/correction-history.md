# Pawn correction history (B6 of search-optimizations.md)

**Status:** shelved 2026-10-07, the code reverted. Neither design showed a gain at 10+0.1 against
B1's search (both duels below). Neither was ever clearly behind, so a bug is unlikely; the likely
reading is that pawn correction, with magnitudes picked rather than tuned, is worth too little on
the hand-crafted evaluation to measure. Worth a retry with tuned constants (B9) or with a network
evaluation; this plan and the duels are the starting point. The first design as built: The first as built: a fresh
table per `go`, carried across its iterations through `search_root_pondering` as the other
histories are (the test-only `search_root` passes a fresh one and drops it, so its call sites are
unchanged); `Search::corrected_evaluate` for `node_eval` and the stand-pat; the update before the
TT store, with Stockfish's condition as `is_correction_informative` over Rotor's bound. On the label path: it changes the tree, so
it is judged by a duel under a clock (cutechess-cli, as B1), and lands before generation 2's
labelling.

## What it is

The static evaluation is wrong in a systematic way in some kinds of positions. After each search,
the difference between the search's result and the static evaluation is recorded in a table keyed
by the position's pawn structure, per side to move; the next time a position with that structure
is evaluated, the evaluation is corrected by the table's entry before any pruning reads it.

## Stockfish 16.1, the release that introduced it (fetched 2026-10-07, `search.cpp`, `movepick.h`)

- `CorrectionHistory = Stats<int16_t, 1024, COLOR_NB, 16384>`, indexed
  `pawn_key & (16384 - 1)`, per side to move.
- Correction: `v += cv * abs(cv) / 12475`, clamped inside the tablebase bands; at the limit 1024,
  about 84 of its units.
- In `search` and in `qsearch` alike, the static evaluation is the corrected one.
- Update at the end of `search`:
  `if (!inCheck && (!bestMove || !capture(bestMove)) && !(bestValue >= beta && bestValue <= staticEval) && !(!bestMove && bestValue >= staticEval))`,
  `bonus = clamp((bestValue - staticEval) * depth / 8, -256, 256)`, `staticEval` being the
  corrected one; the entry moves by `entry += bonus - entry * abs(bonus) / 1024`.

## Design (decided 2026-10-07)

1. **The pawn structure alone**, Stockfish's first version; minor-piece, non-pawn and
   continuation tables are later items, each with its own duel.
2. **A pawn key computed on demand** at each node that evaluates: the XOR of the existing Zobrist
   piece-square values of both colours' pawns. Incremental in `make_move` only if a profile shows
   it.
3. **The table**: `[[i16; 16384]; 2]`, Stockfish's shape, with the same lifetime and threading as
   Rotor's history tables.
4. **Magnitudes in Rotor's centipawns**, Stockfish's scaled by its pawn (about 208 units) to
   Rotor's 100: a correction of `cv * |cv| / CORRECTION_DIVISOR`, at most about 40 cp at the
   saturation 1024 (`CORRECTION_DIVISOR` = 1024 * 1024 / 40); a bonus of
   `clamp((best - eval) * remaining / 4, -256, 256)`, the same gravity update. Picked by
   magnitude, not tuned, as every search parameter.
5. **Read everywhere the static evaluation is**: the main search's `node_eval` (razoring, both
   futilities, the null-move gate and its margin term, `static_evals` for `improving`) and
   quiescence's stand-pat. **Updated in the main search only**, at the end of a node with an
   evaluation, under Stockfish's condition, with the node's remaining depth after IIR.

## First duel: a fresh table per `go` (2026-10-07)

As built above, against B1's search at 10+0.1: 152 - 141 - 207 after 500 games, 0.511,
+7.6 +/- 23.3 Elo, stopped there since 0.587 over the remaining 500 would have been needed to clear
0.549 at 1000. Never behind, so no sign of a bug; the likely cause is the table's lifetime. Stockfish
keeps it across moves and clears it on `ucinewgame`; a fresh table per `go` throws away what each
move learned, and at 10+0.1 a search is too short to fill one.

## Second design: the table lives for a game (decided 2026-10-07)

1. **A type**, `CorrectionHistory` in `search.rs`: the flat `Vec<i16>` with `new` and `clear`.
2. **Every `choose_best_move*` takes `&mut CorrectionHistory`** in place of making its own; the
   iterative deepening loop reads and writes it, so it carries over into the next call. The
   test-only `search_root` keeps its fresh table.
3. **One table per search thread, cleared with the hash**, wherever the hash is cleared:
   - `uci.rs`: `Engine` holds `Arc<[Mutex<CorrectionHistory>]>`, one per `Threads`, rebuilt when
     `Threads` changes, cleared on `ucinewgame`. Thread `i` of a `go` locks entry `i` for the whole
     search, uncontended; the `Arc` because the search's background thread outlives `handle`, as
     `tt`'s does.
   - `arena/in_process.rs`: one per player, cleared in `new_game`.
   - `arena/replay.rs`: one per worker, cleared per game, as its `tt`.
   - `arena/self_play.rs`: one per worker, shared by both sides as its `tt` is (the table is
     already split by side to move), cleared per game.
   - Benches and tests: a fresh one per call.
4. **Labelling keeps it within a game** (developer decision): the labeller then searches as the
   engine that plays does. A label stops being a function of the position and the hash alone, but
   the hash already makes it depend on the game's earlier searches.
5. **The multipv variant** keeps its per-pass clones, now taken from the persistent table, and
   writes the first pass's result back.

Verified as the first: a duel at 10+0.1 against `~/rotor-b1.exe`, by the duel rule, at most 1000
games.

## Second duel: a table per game (2026-10-07)

52 - 54 - 94 after 200 games, 0.495, -3.5 +/- 35.1 Elo, behind the first design's +14 at the same
point; stopped there, since 0.563 over the remaining 800 would have been needed. Shelved with the
first.

## Tests

- The pawn key: equal for two positions with the same pawns and different pieces, different for a
  moved pawn or the other colour's pawn on the same square.
- The update: the gravity bound (an entry never leaves `[-1024, 1024]`), a positive surprise
  raising the entry, the skip conditions (in check, a capture as best move, an uninformative
  bound).
- The correction's sign and cap.
- Node counts change; recorded as the new references, as B1's.

## Verification

A duel at 10+0.1 through cutechess-cli, the new search against B1's, both hand-crafted, stopped by
Rotor's duel rule (0.5 + 3.09 * 0.5 / sqrt(N)) or at 1000 games.
