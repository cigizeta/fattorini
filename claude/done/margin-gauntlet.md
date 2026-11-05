# Margin gauntlet: 200 against 175, 150, 125 and 100, in turn

**Status:** done; generation 2's selection tuned its winner's margin with it (checked 2026-10-14).

Agreed 2026-10-11. Earlier status: implemented, awaiting its tests. Replaces the margin search's tree
(margin-search-command.md). Betty gen-0002's search was stopped partway through the tree (`200-vs-100`
won by 200, `200-vs-150` at 278 of 400 games) and resumed on the gauntlet, which reuses any match
both rules reach.

## Why

The search for a player's margin per ply (`margin_search.rs`) walks a fixed tree of best-of-400
matches on a 25 cp grid over 100 to 250: 200 against 100, then 150 against the winner, then a branch
on 125, 175, or 250 and 225. Three or four matches, depending on the results, and reading which ones
were played means following the tree.

The developer wants it simpler, even at four matches every time: a gauntlet.

## Design

### The rule

A king of the hill over a fixed list. The champion is 200; the challengers are, in order, 175, 150,
125 and 100. Each match is a best of 400 at the search's control, the champion against the next
challenger; the challenger takes the title only by winning it, and a drawn match keeps the champion,
as today. Whoever holds the title after the fourth match is the margin.

So a champion that keeps winning meets challengers ever further from it: 200 that holds plays 175,
150, 125 and then 100, the last 100 cp away, the tree's first match. A challenger that takes the
title then meets the next value down, closer again: 150 having won plays 125, 25 cp away.

Always four matches: at most one more than the tree.

### The range

100 to 200, against the tree's 100 to 250. Every search so far chose 125 (amanda gen-0001) or 150
(amanda gen-0002), and none ever went above 200; the tree's branch above it has never been taken.

### What stays

- **Resumable without state of its own**, as today: each match has a fixed folder,
  `<champion cp>-vs-<challenger cp>/` in the search's directory, and every start walks the gauntlet
  from its first match; a match whose recorded games resolve it replays them to its verdict and
  plays nothing, so the walk reaches the first unresolved match and resumes it there.
- `margin.txt` beside the player's network, `arena margin` and `select` sharing the search,
  the control (`STC` unless given), the best of 400, a draw keeping the champion.

### A search begun under the tree

A folder is named by its two margins, and a match is the same best of 400 under either rule, so a
folder both rules can reach (`200-vs-150`, `150-vs-125`, `200-vs-100`, ...) holds the same match
either way and is reused as it is. What must not happen is a search switching rules halfway: one
already finished keeps its `margin.txt`, and one interrupted under the tree is finished by the build
that started it, or its folder removed before the gauntlet runs.

## Implementation steps

One commit, tests passing.

1. **`search` becomes the gauntlet**: the champion 200, then a loop over `[175, 150, 125, 100]`,
   the champion carried forward as `play`'s result. The module comment describes the gauntlet in
   place of the tree, with the range and the reason for it. Tests replace the tree's: the four
   matches played in order for a champion that always holds (`200` against each, result 200), for
   one replaced at each step (result 100), and for a challenger that wins in the middle then holds
   (`150` takes over, then plays 125 and 100); an interruption stops the walk; a draw keeps the
   champion (kept as it is).
2. Update the usage line in `main.rs` if it describes the tree, and margin-search-command.md with a
   "superseded by margin-gauntlet.md" note on its tree.
