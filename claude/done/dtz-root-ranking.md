# DTZ: ranking the root moves

**Status:** done 2026-10-14: the DTZ tables are fully downloaded on all three machines, and on
Lichess the bot now converts endgames it used to fail to. Earlier status: implemented 2026-10-07; all tests pass on the Lichess machine, including KQ vs K and KR vs K mated
from far starts within the fifty-move rule (both sides at depth 6). Left: lichess-bot's
`SyzygyPath` gains the two DTZ directories, and the Lichess machine's 6-piece DTZ download completes. On the label path where it touches search (see
"Labels"); built after betty and caroline finished training, which they have.

As built: `SyzygyTablebase::excluded_root_moves` returns the moves outside the best rank, computed
once per `go` in `choose_best_move_with_progress` and `choose_best_move_multipv_with_progress`
and passed as the root exclusion. One departure from Stockfish: a root move that mates or
stalemates is settled before any probe (DTZ 1 or 0), where Stockfish relies on its prober's DTZ for
a mated position and corrects it, which `shakmaty-syzygy` need not reproduce. The test positions
moved to `k7/7Q/1K6/8/8/8/8/8 w` (Qc7 stalemates; Qa7, Qb7, Qg8, Qh8 mate).

## The problem

Rotor probes only Syzygy WDL, and only right after a capture or pawn move (`tablebase.rs`,
`should_probe`, `probe_wdl_after_zeroing`). So once the last capture of a won ending is played,
nothing in Rotor knows how to convert it within the fifty-move rule: KQ vs K went unconverted on
Lichess on 2026-10-04 (TODO.md). The TT mate-distance fix (tt-mate-distance.md, done) removed one
cause; DTZ removes the other.

## What Stockfish does (fetched 2026-10-07, `search.cpp` and `syzygy/tbprobe.cpp`, master)

- **In the search, WDL only after a zeroing move**: the probe in `Search::Worker::search` requires
  `pos.rule50_count() == 0 && !pos.can_castle(ANY_CASTLING)`, the same as Rotor's `should_probe`.
  So TODO.md's "probe WDL at any clock" is not what Stockfish does, and this plan does not do it.
- **DTZ only at the root**, `Tablebases::root_probe`: for each root move, the DTZ of the position
  after it (from WDL if the move zeroes, 0 if it repeats or draws by the fifty-move rule, otherwise
  `-probe_dtz` moved one ply away from zero; a mate in one is DTZ 1). Then a rank, with `cnt50` the
  root's halfmove clock and `MAX_DTZ = 1 << 18`:
  - winning, `dtz + cnt50 <= 99` and no repetition so far: `MAX_DTZ` (minus `dtz` only under
    `rankDTZ`, off by default);
  - winning but too late for the clock: `MAX_DTZ / 2 - (dtz + cnt50)`;
  - losing, `-dtz * 2 + cnt50 < 100`: `-MAX_DTZ` (plus `-dtz` under `rankDTZ`);
  - losing but saved by the clock: `-MAX_DTZ / 2 + (-dtz + cnt50)`;
  - drawn: 0.
- **The root moves are stable-sorted by rank**, and iterative deepening searches only the group of
  equal rank at the front (`pvFirst`/`pvLast`): the best move is always one of the best-ranked.
- **With DTZ available, the in-search probes are switched off for that search** (`cardinality = 0`
  in `rank_root_moves`): the ranking already guarantees the result, and the search chooses among
  the safe moves by its own evaluation and mates. Rotor keeps them on, for its labels.
- If DTZ probing fails (a missing table), `root_probe_wdl` ranks by WDL alone instead.

Every win-preserving move that still fits the clock shares the top rank, so the search keeps its
own choice; as the clock runs, the top group shrinks to the moves that make progress, so a won
ending is always converted under the fifty-move rule.

## What `shakmaty-syzygy` 0.28.1 offers (read from its source)

- `probe_wdl_after_zeroing` (WDL tables only), what Rotor uses today.
- `probe_dtz(pos) -> MaybeRounded<Dtz>`, needing WDL and DTZ tables. DTZ may be rounded by one ply
  in some tables; the crate documents that following the tablebase line wastes at most one ply,
  harmless except right at the fifty-move edge.
- `probe_wdl(pos) -> AmbiguousWdl`, clock-aware, and `best_move(pos)`; neither needed here.
- `add_directory` adds `.rtbz` files by name, like `.rtbw`, so loading needs no new code: only the
  DTZ directories in the list.

## Design

1. **Loading: DTZ handled as WDL is (decided 2026-10-07).**
   - **The UCI engine: both optional.** It plays correctly with whatever `SyzygyPath` it is given:
     none, WDL only, or WDL and DTZ, since a CCRL tester may disable the tables or provide only
     some. No DTZ: no ranking, play as today. No tables: no probes. A DTZ table missing for one
     material: that root is not ranked (`None`), as a missing WDL table means no probe today.
     lichess-bot's `SyzygyPath` adds the DTZ directories (its config is outside this repository,
     updated by hand).
   - **Rotor's own tools (arena, self-play, replay): both required.** `syzygy_directories()` adds
     `3-4-5-dtz` and `6-dtz`, and `from_directories` refuses a missing or empty one as it does a
     WDL one, so games and labels never depend silently on which machine produced them. Each
     machine that runs them needs the full `bin/syzygy.sh` download, about 82 GB of DTZ.
2. **`SyzygyTablebase::rank_root_moves(board, moves, history) -> Option<Vec<i32>>`**: Stockfish's
   `root_probe` on Rotor's types, `rule50` on, `rankDTZ` off. `None` when the root has more pieces
   than the tables, castling rights, or any probe fails (a DTZ table missing, as 59 six-piece ones
   are on the Lichess machine today): then nothing changes from today. No `root_probe_wdl` fallback in this
   plan: WDL at the root already happens through the in-search probe after a zeroing move.
3. **The search at the root.** When ranks exist, every root move outside the best rank is
   excluded through the existing MultiPV exclusion (`excluded_root_moves`). The in-search WDL
   probe stays on (see "Labels"). MultiPV past the first group is not supported under a tablebase
   root: lines 2 and up show only the best group.
4. **The reported score.** Unchanged: whatever the search finds (a mate when it sees one, otherwise
   the evaluation). Stockfish reports a tablebase score instead; Rotor's `info` lines are read by
   no one who needs it, and an ordinary score keeps the TT and aspiration windows as they are.

## Labels (decided 2026-10-07)

Self-play and replay search at depth 9 and label each position with the search's score, blended
with the game's result. **The ranking applies everywhere, game play, self-play and replay alike,
and the in-search WDL probes stay on** (unlike Stockfish):

- Labels barely change. Today a tablebase position reached by quiet moves gets no probe anywhere
  in its tree, so its label is already the evaluation; the in-search probes, kept, still give the
  tablebase verdict wherever they gave it. What changes is only that root moves which lose the
  result under the fifty-move clock are no longer searched, which makes a label more correct.
- Self-play results improve: a won ending self-play fails to convert ends drawn by the fifty-move
  rule, and that wrong result is blended into the label of every position of the game. Ranked,
  self-play converts.
- Replay plays the recorded games' moves, so only its scores are affected, as above.

Still on the label path: it goes in between two generations' labelling, never during one.

## Tests

1. Ranks on known positions: KQ vs K with a mate in one ranks the mate in the top group with every
   other winning move, and a move dropping the queen below them.
2. The clock: the same winning position at halfmove clock 0 and at 95: at 95 only the moves with
   `dtz <= 4` stay on top.
3. A missing DTZ table gives `None`, and the search behaves as today.
4. Conversion: KQ vs K and KR vs K from a far start, played out by the engine through `bin/rotor`
   at `go movetime` 200 per move: mate within the fifty-move rule. Skipped when the tables are not
   in the data root.

## Verification

- `cargo test` on Windows, with the tables present.
- A KQ vs K and a KR vs K played out against Stockfish, no regression in `search_bench` (no
  position there reaches a tablebase root with castling rights gone and six pieces or fewer,
  except 48 and 49: their node counts may change).

## Open questions

None left: both answered in "Labels" (ranking everywhere, in-search probes kept).
