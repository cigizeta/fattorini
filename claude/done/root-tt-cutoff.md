# Root node resolving from a bare TT cutoff

## Symptom

Discovered via a real cutechess-cli match (Rotor vs. Stockfish, 5+3, 2 games, PGN output).
Both games ended with `[Termination "illegal move"]` -- Rotor reported `bestmove 0000`
mid-game despite the position having plenty of legal replies and not being in check.

Game 1: after `1.d4 d5 2.Qd3 c5 3.dxc5 Nf6 4.e4 Nxe4 5.Nc3 Nxc3`, White (Rotor) returned
`0000`. The resulting position (`rnbqkb1r/pp2pppp/8/2Pp4/8/2nQ4/PPP2PPP/R1B1KBNR w KQkq - 0
6`) has at least two immediate legal recaptures alone (`bxc3`, `Qxc3`), plus every other
ordinary legal move -- nothing about it is remotely terminal.

Game 2: after `1.e4 d5 2.exd5 e6 3.Bb5+ c6 4.dxc6 bxc6 5.Be2 e5 6.Nf3 e4 7.Ne5`, Black
(Rotor) returned `0000` the same way.

Both crash positions were captured directly from the match (PGN movetext plus an EPD dump
of the two final positions), giving exact, reproducible ground truth rather than a
description to work from.

## Root cause

`alpha_beta_with_buffer`'s transposition-table cutoff (an entry searched deep enough, with
a bound that resolves the node outright) applied uniformly at every `ply`, including `ply
== 0`, the root. `search_root` calls this function at `ply == 0` and then reads back
`self.pv[0]` to decide which move to actually report as `bestmove` -- but a bare TT cutoff
resolves the node by score alone, without ever running the move loop that writes `pv[0]`.

Depth 1's own `remaining` (`target_depth - ply`) is `1`, an extremely low bar for "searched
deep enough." Any position that had already been reached as a transposition inside an
earlier move's own deeper search tree -- entirely realistic mid-game, since the
transposition table persists for the whole game -- resolves the very first
`alpha_beta_with_buffer` call of a fresh `go` instantly, with an empty `pv` but a real
score. `choose_best_move_with_progress` cannot tell that apart from a genuinely
legal-move-free position (checkmate/stalemate) and breaks out of its loop with `best_move`
left at its initial `None`.

Every previous manual UCI test this project had run used a single `go` from the starting
position with no real move history and no real `wtime`/`btime` (`go infinite` or `go
movetime`), so the transposition table never had a chance to accumulate the kind of
same-game, cross-move-order overlap needed to trigger this. A real cutechess-cli match,
with real time control and a real, evolving move sequence, was the first thing that
actually exercised this path.

## Fix

Excluded `ply == 0` from the TT-cutoff branch: `if ply > 0 && entry.depth() >= remaining`.
The root now always falls through to a real move loop and always populates `pv[0]`; every
interior node is completely unaffected, since an interior node never needs to report its
own best move upward, only its score. `tt_move` is still extracted from the same probe
regardless of `ply`, so the root still benefits from the TT hit as a move-ordering hint.

Checked directly against Stockfish's own `search.cpp` for how a real engine avoids this:
its own TT-cutoff shortcut is gated on `!PvNode`, and the root is always a PV node, so it
never takes this shortcut there either -- only ever using a root TT hit as a
move-ordering hint, exactly as this fix now does.

## Tests

`a_deep_tt_hit_at_the_root_still_produces_a_real_move_not_an_empty_pv` -- reproduces the
exact game-1 crash position directly: primes the table with an artificially deep,
`Exact`-bound entry for that position's own hash (simulating it having already been
reached as a transposition), then asserts a fresh depth-1 `search_root` call still returns
a nonempty `pv` rather than resolving straight from the TT hit.

## Verification

Rebuilt and reran a live cutechess-cli/GUI rematch (Rotor vs. Stockfish, same 5+3 setup).
No further illegal-move terminations observed -- Rotor continued producing legal moves
throughout, including under real time pressure, confirmed by watching the match live
rather than only trusting the regression test in isolation.
