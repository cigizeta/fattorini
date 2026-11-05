# The best-move label: replay and self-play keep the move their search found

**Status (2026-10-14):** implemented (steps 1 to 3, the reader, the replay and self-play), tests
passing; decided for generation 4 (the developer): its replay and self-play PGNs carry the best
move. Earlier status: design only (2026-10-12). Not to be implemented while generation 3's replay runs: any
machine that restarts its replay rebuilds `arena` from its working copy, and a format change
mid-generation would mix two formats in one corpus. Implement once generation 3's replay and
self-play are finished on all three machines, or on a branch none of them has checked out.

## Why

psq_threats_move (architectures.md, best-eval-together-with-best-move.md) trains its move head
against search(9)'s best move. Today no corpus stores it: replay labels each position with a score
only, and the move after it in the PGN is the game's move (a CCRL or TCEC engine's, or self-play's
exploration move), not Rotor's best move.

## What exists

- **Replay** (`arena/replay.rs::search_game`) calls `choose_best_move`, which returns
  `(reports, best_move)`, and keeps only `reports.last().score`. The move is found and discarded:
  keeping it costs no search.
- **Self-play** (`arena/self_play.rs::play_one_game`): past `EXPLORATION_PLIES` (6) the played move
  is the best move. In the first 6 plies `choose_exploration_move` may play a lower-ranked move on
  purpose, but already holds `best_move`, and already annotates the best move's score, not the
  played one's.
- **Tablebase positions in replay** (`tablebase_label`) get a label with no search, so no move.
- **One reader for both**, `train/length_cache.rs::SelfPlayGameCollector::comment`, parses the
  whole comment as a score and panics on anything else.

## The format

Each score comment after a move gains the best move of the position *before* that move, the
position the score belongs to, in UCI notation, separated by a space:

```
4... Nc6 {0.31 g8f6} 5. c4 {#-2 d8h4}
```

- **UCI, not SAN**: no check suffix, no disambiguation, and self-play already keeps its moves as
  UCI (`MoveAnnotation::uci`). The reader resolves it against the board it already replays.
- **The move is optional**: a comment with a score alone, every corpus up to generation 3, reads as
  before with no move label. One reader serves old and new files; nothing is rewritten.
- **Tablebase positions in replay carry a move of the DTZ ranking's best rank** (`root_ranks`, no
  search): the moves play itself chooses among (`excluded_root_moves` keeps only those), so labelling
  and play match (the developer, 2026-10-14). The game's own move when it is among them, the choice
  of a strong engine, otherwise the first in generation order. No preference by DTZ: as in play and
  in Stockfish (its `rankDTZ` off), every win within the fifty-move clock is alike. Never a move
  that spoils a win or loses a draw: a move head that points to a drawing move keeps it out of the
  losing lines. (A first version took the smallest DTZ, a style play does not have; dropped the same
  day.) The final position of every game has no move, as it has no score.
- **Self-play writes the best move at every ply**, exploration plies included; past ply 6 it is
  also the played move. The position's label is its best move, as its score is its best score.

## What changes

1. **Reader first** (`length_cache.rs`): split the comment at the first space, parse the score as
   today, keep the optional move as a string, resolve it to a legal move when the game is replayed.
   An unresolvable move fails loudly, as an unparsable score does now. Only after this is in every
   working copy do the writers change, so no machine can write what an older reader cannot read.
2. **Replay** (`search_game`): return `(score, Move)` per position, the tablebase branch taking its
   move from `root_ranks` with the tie-break above (a `SyzygyTablebase` method returning the result
   and the move together, so the ranking runs once); `format_output` appends the move. Output tests
   updated.
3. **Self-play** (`play_one_game`, `choose_exploration_move`): return the best move beside the
   played one; `annotate` writes it.
4. ~~`prepare`~~ **Left out** (the developer, 2026-10-14): the move's encoding, its cache file and
   the loss belong to psq_threats_move's own design, still to come (its input and its loss). The
   cache can be built from the PGNs at any time; only the PGNs had to carry the move from
   generation 4. psq, psq_threats and the others build the same cache as today.

The node counts and labels of replay and self-play do not change: the same search, the same score,
one more field written.

## Open

1. ~~UCI or SAN~~ **UCI** (the developer, 2026-10-14).
2. ~~The move index file~~ **In psq_threats_move's own plan** (the developer, 2026-10-14), with the
   network's input and loss.

## Tests

- The reader: a score alone, a score and a move, a mate with a move, an illegal move (fails).
- Replay's `format_output`: a move after every position after the book.
- The tablebase move: in a drawn position with losing alternatives (e.g. a KRvKR position where
  some moves lose the rook), the move is a drawing one; the game's move is kept when it is among
  the best ranked, a slower win included; a stalemating move never is.
- Self-play: past the exploration plies, the comment's move equals the played move.
- Node counts at depth 9 unchanged (`nnue_bench --search-only`), on an idle the Lichess machine.
