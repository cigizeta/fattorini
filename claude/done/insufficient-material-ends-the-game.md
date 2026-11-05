# Insufficient material ends the game

**Status:** done 2026-10-15 (the developer: "let us start to fix this"): implemented, `cargo test
--release -p arena` passing.

## The gap

`smithy buckets amanda-0002` (king-distribution.md) counted 358,646 training rows at material 0,
king against king, 0.49% of the corpus, and more at material 3 (a lone minor against a bare king).
Neither game loop ends a game when neither side can mate:

- **Self-play** (`self_play.rs::play_one_game`): checkmate, stalemate, the fifty-move rule and
  twofold repetition only. A dead game plays on until a repetition or the fifty-move rule, every
  position searched and written as a training row.
- **Arena matches** (`game.rs::play_one_game`): the same with threefold repetition.
  `is_insufficient_material` is used only to score a time forfeit (FIDE 6.9).
- **Replay** is not affected: it follows the archived games, which their GUIs end at a dead
  position.

The engine's search already scores these positions as draws before evaluating
(`search::is_insufficient_material`): king against king, a lone knight or bishop against a bare king,
one bishop each on squares of the same colour.

## The fix

In both loops, right after the no-legal-move check: when `is_insufficient_material(&board)`, the
game ends a draw, `Termination::InsufficientMaterial` (`"insufficient-material"` in the PGN's
`[Termination]` tag). The dead position is the game's final one, labelled by the result as every
final position is; nothing after it is played.

- No label of an earlier position changes: only where games stop.
- Existing corpora keep their dead rows; `prepare` is not changed.
- Lands before generation 4's self-play.

## Tests

- `pgn.rs`: the new variant in the every-variant test.
- `self_play.rs`: a game from a dead position (king against king, and same-coloured bishops) ends at
  once, a draw by insufficient material, with no move played.
