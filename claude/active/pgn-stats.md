# pgn-stats: how each player spent its clock

**Why (2026-11-28):** both dual networks draw their twins at fixed depths (screlu dual: 52.0% at
depth 9, 50.3% at depth 12, trials 26 and 30) and lose at 10+0.1 (trials 27 and 29, about 44%).
Their search is sound at depth, so the loss is on the clock: the time manager spending their time
differently, or their speed in real games. Every move of a trial's PGN carries its depth and time
(`{0.19/15 0.5s}`), so averaging them per player tells the two apart.

## The command

```
bin/arena pgn-stats <pgn>
```

In the arena (no libtorch, no venv), read-only. For each player (the PGN's `White` and `Black`
tags, a network's path or an engine's name), over every move with a comment:

- moves counted, mean and median time per move, mean depth;
- the same by phase: moves before full move 20, 20 to 39, 40 on;
- the mean of each game's total time.

A comment is `<score>/<depth> <time>s`, the score a number or `#<n>`; a move without one, or whose
comment does not read so, is skipped and counted apart. Who moved is told by alternation from the
side to move of the `FEN` tag (White without one). Plain text out, one block per player.

## Read

- The dual spends clearly less time a move: the time manager.
- The same time, a lower depth: its speed in games.
- The same time, the same or a greater depth: subtler (where the time goes).

## Tests

The comment's parser (a score, a mate, a malformed one); the alternation from a `FEN` with Black to
move; the per-player sums over a two-game PGN written in the test.
