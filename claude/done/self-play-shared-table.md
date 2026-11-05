# Self-play: one transposition table per game, shared by both sides

## Status

Decided and switched, not yet compiled after the switch. The benchmark ran on one machine, 256 hand-crafted games each way, from the same openings:

| | split | shared | change |
|---|---|---|---|
| nodes / position | 15,947 | 10,195 | -36% |
| positions / s | 344.8 | 542.1 | +57% |
| nodes / s | 5.50 M | 5.53 M | +0.5% |
| positions | 26,581 | 27,883 | +5% |

The same nodes per second, so the whole gain is search saved. `rotor-arena self` now plays one table
per game, and its memory guard counts one table per core. The benchmark was removed afterwards (the
developer's call: keeping it for the record is not worth its code); its description below is the
record of how the numbers were measured.

## The idea (the developer's)

Today every self-play game has two 16 MB tables, one per side, cleared between games. Instead: one
table per game, used by both sides, still cleared between games. Two hoped-for gains:

1. **Speed**, to be verified: after a move, the opponent's search starts in a subtree the mover has
   just searched, so more probes should hit, fewer nodes per move, more positions per hour.
2. **Better labels**, speculative: consecutive scores computed from one shared memory of the game
   should be more consistent, which is what AB(lambda) averages over. Only a full generation and a
   duel could show it; not tested.

The decision rests on speed: if the benchmark shows a clear gain, the idea is kept.

Why it is sound: self-play is one network playing both sides, so one engine's memory is the natural
model; the per-side argument (two real engines never share a table) belongs to the ladder, which
keeps one table per player. Games stay isolated: the table is cleared between games. Switch at a
generation boundary, so one dataset never mixes games played both ways.

## The benchmark

```
rotor-arena self-bench <player> <games> --tt split|shared
```

- `<player>` as for `self`: `hand-crafted` or a network (its latest champion).
- Plays exactly `<games>` games, one per logical core at a time, at depth 9 with 16 MB tables, with
  the exploration of `self` unchanged. **Writes nothing**: no staging file, no segment, no remote.
- **The same openings both ways**: drawn from the book with a fixed seed, game `i` always starting
  from the same position, and its exploration seeded by `i`. The games still diverge once the table
  changes a search, as they must.
- Reports games, positions (scored plies), total nodes, wall time, positions per second and nodes
  per position.

Run it twice on the same machine, same `<player>` and `<games>`, idle otherwise:

```
./bin/rotor-arena self-bench hand-crafted 256 --tt split
./bin/rotor-arena self-bench hand-crafted 256 --tt shared
```

Nodes per position is the direct measure (machine load cannot touch it); positions per second is the
one that pays.

## After the decision

- Kept: `self` switches to one table per game; the memory guard counts one table per core; the
  benchmark's `--tt split` goes, or the benchmark goes entirely.
- Dropped: the benchmark goes, and `self` stays as it is.
