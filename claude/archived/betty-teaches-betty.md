# Betty generation 3 taught by betty-0001: does a Betty network teach better than an Amanda one?

Agreed 2026-10-11, conditional: run only if betty-0002 wins a depth-9 duel against amanda-0002. No
code needed; this is a choice of teacher and the usual pipeline.

**Not run (2026-10-11): the condition failed.** The depth-9 duel amanda-0002 vs betty-0002 was dead
even, +309 -304 =295, 50.3%, curtailed at 908 games, drawn. betty-0002 did not win, so Betty's
generation 3 is not taught by betty-0001. The three duels below agree: at the teaching depth a Betty
network searches no better than an Amanda one of the same generation.

Status: betty-0001's replay started on the Lichess machine on 2026-10-11, ahead of the duel, since replay is the
slowest step (`rotor-arena replay` takes the player itself and needs no `teacher.txt`). Step 2,
naming the teacher, still waits for the duel: `teacher.txt` is written once. If betty-0002 does not
win at depth 9, the replay is unused. It was not used, and its outputs (`data/games/replayed/{tcec,ccrl}/betty/`)
were deleted locally and from Drive on 2026-10-12.

Clocked duel, 2026-10-11 (10+0.1, not the deciding one): amanda-0002 is stronger, resolved at 976
games, +359 -261 =356, S 0.5502 (about +35 Elo). Expected: Betty's hidden layer costs depth under a
clock.

Depth-9 duel of the two teachers themselves, 2026-10-11: amanda-0001 vs betty-0001, +434 -349 =205,
amanda-0001 at 54.3% (about +30 Elo), curtailed at 988 games, formally drawn. At the teaching depth,
betty-0001 is not the better searcher; if anything amanda-0001 is.

Depth-9 duel of the generation-2 champions, 2026-10-11, the deciding one: amanda-0002 vs betty-0002,
+309 -304 =295, 50.3%, curtailed at 908 games, drawn.

## Why

Labels are made at a fixed depth (self-play and replay both search to depth 9), so how good a
teacher is depends on the quality of its search at that depth, not on its speed. A depth-9 duel
measures exactly that; the clocked duel answers a different question, which network plays better
under a clock, where Betty's hidden layer costs depth.

Betty generation 2 was taught by amanda-0001: it trained on `amanda/gen-0001`, the positions and
scores of amanda-0001's search, as Amanda's own generation 2 did. If betty-0002 wins at depth 9, a
Betty network searches better at the teaching depth, and the question is whether that carries over
to teaching.

## The experiment

Betty generation 3 is trained on data labelled by **betty-0001**, on purpose, not by her latest
champion betty-0002. Both of Betty's generations then have a generation-1 champion as teacher:

| | student | teacher |
|---|---|---|
| betty gen-0002 | Betty architecture | amanda-0001 |
| betty gen-0003 | Betty architecture | betty-0001 |

So the student's architecture and the teacher's generation stay fixed, and only the teacher's
architecture changes. betty-0002 taught nothing in either, so its own strength does not confound it.

### What decides it

Betty gen-0003's candidates go through `rotor-arena select` as usual: round robin, margin gauntlet,
and the gate against betty-0002, the current champion. Passing the gate means a Betty-taught Betty
beats an Amanda-taught Betty on equal footing (same student, same grid, same training fields).

What can differ besides the teacher, to keep in mind when reading the result:

- **The positions**: betty-0001 plays its own games, so the self-play positions come from a Betty
  search. That is part of what "teaching" means here, not a confound to remove.
- **The amount of data**: the dataset should be about as large as `amanda/gen-0001` (362M train
  rows), or a better result could come from more data rather than a better teacher.
- **Wall clock**: a Betty search costs more per node, so producing the same number of positions
  takes longer than it took amanda-0001. The labels are not worse for it.

## Steps

1. Run the depth-9 duel: `bin/rotor-arena duel amanda-0002 betty-0002 --depth 9`. If betty-0002 does
   not win, stop here and archive this doc with the result.
2. Name the teacher of Betty's generation 3: `bin/rotor-train teacher betty betty-0001`. It is
   written once to the remote `teacher.txt` and never overwritten.
3. Produce betty-0001's self-play and replay at depth 9, the same amounts as `amanda/gen-0001`.
4. `bin/rotor-train prepare betty`, then `bin/rotor-train train betty`, on betty.yaml's grid and
   training fields, unchanged.
5. `bin/rotor-arena select betty`; record the standings, margin and gate here.

## Open

- Whether Amanda's generation 3 is taught by amanda-0002 meanwhile, as the lineage's usual next
  step, or waits for this result.
