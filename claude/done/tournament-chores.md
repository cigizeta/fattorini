# Tournament chores

Status: implemented, built and tested 2026-10-23 (`smithy`, `arena` and the protocol's tests all
passing); not yet run for real, which moves it to `done/`. The protocol changes: every arena is
pulled and restarted with the smithy that uses it.

## The problem

`lc0-psq-r1`'s round robin (2026-10-23) played one pairing at a time: 752 games of 0.01 against 0.02,
none of anything else, its live standings meaningless until the end. `deal` picks the pairing furthest
behind, but a chore holds one pairing (`Dealt.pairing`, `Dealt.players`), and every top-up (`answer`'s
`ADD`) adds more of the same, so an arena plays its pairing to the end before another is dealt. The
comment at `deal` still assumes "arena's interleaved schedule", which chores (chores.md) lost.

Two smaller fixes were weighed and set aside: a chore closed at every top-up and dealt anew (churn,
networks evicted and reloaded at each switch), and a chore whose top-ups append entries (unbounded:
about 360 entries over a 14,400-pair round robin, scanned at every task and every eviction).

## Decisions (the developer, 2026-10-23)

1. **Four chore kinds, to stay safe.** First proposed as one kind, the tournament, with self-play and
   the gate as its special cases; revised the same day so that what works is not rewritten:
   - `game`: self-play, as today.
   - `pair`: the gate, as today.
   - `tournament`: new, the round robin, n players and their matchups, below.
   - `spsa`: the tune's chore, today a `pair` chore of many entries told apart by their `K` tags, under
     a kind of its own, its behaviour unchanged.
2. **Fixed quotas.** The smithy gives each matchup a quota of tasks and tops it up as the arena runs
   low; the arena never plays past it. With several arenas the smithy keeps the totals balanced, and
   a gate's SPRT wastes few pairs once it decides.
3. **A returning task is credited to its matchup by its players**, the networks its games name: each
   matchup of a chore has its own players. No matchup tag.
4. **No transition.** Every arena is updated with the smithy; an old arena is refused at its hello
   (the commit check).
5. **Live standings by score, not points.** While pairs arrive, the pairings are unevenly played, so
   points favour the lambdas that have played more; the table ranks by score (points over games)
   until every pairing is complete. Once balanced, both orders agree (`round_robin`'s module doc),
   and the winner is still named only then.

## The chore

First drafted as players listed once and `match` lines; built instead on today's entries, which
already carry two players and a count, so the format is unchanged but for the kind:

```text
chore     c000017
kind      tournament
limit     depth 9
entry     12
player    data/gen/lc0-psq/run-1/.../lambda-0.01.nnue   own
player    data/gen/lc0-psq/run-1/.../lambda-0.02.nnue   own
entry     12
player    data/gen/lc0-psq/run-1/.../lambda-0.01.nnue   own
player    data/gen/lc0-psq/run-1/.../lambda-0.03.nnue   own
```

- Each entry is one matchup and its quota, played in pairs, the first player White in the first game.
- A top-up (`ADD`) is entries as well; in a tournament each raises the quota of the entry with the
  same players and tags, or is added if the chore had no such matchup. A chore so never holds more
  entries than its round robin has pairings.
- Today the smithy deals a tournament for every `Work::Pairs` of more than one pairing (the round
  robin), and a `pair` chore for one (the gate).

## The arena

- Each free worker takes a task from the matchup with the most quota left, the first on a tie: the
  interleaving comes back at the grain of one task, whatever the top-ups' size.
- A task's players are its matchup's; the network cache keeps the chore's players, at most n networks,
  for the whole chore.
- A task given back (a stop, an unavailable network) returns to its matchup's quota.

## The smithy

- `Dealt` holds the chore's players and, per matchup, its dealt and arrived counts, instead of one
  `pairing` and one `players`. `outstanding(p)` sums matchup `p` across open chores.
- `deal` and `answer`'s top-up split a chore's size (`TASKS_PER_WORKER` per worker) over the matchups
  that want most, each never past what it wants (`wanted(p)`): the furthest behind first, as today's
  `deal` chooses, but every top-up choosing again.
- `take` finds the matchup by the games' White and Black (a pair's second game reversed), and refuses
  a task whose players are no matchup of its chore.
- A chore closes once none of its matchups wants more and none has tasks in flight, as today's
  "complete" check, per matchup.

## An arena lost and back

A task is never counted twice: `take` drops one whose `(chore, session, seq)` it has seen, a
restarted arena's old session is gone, and a restarted smithy's new token turns earlier sessions
away. But an arena lost (`lose`) keeps its session, so the pairs it played meanwhile are still taken
when it comes back, while its closed chore's tasks were dealt again: a pairing could end past its
`wanted`, unbalancing the round robin. So (the developer, 2026-10-23) `record` drops, unrecorded and
said, any pair of a pairing already at `wanted`: every pairing ends at `wanted` exactly, in a round
robin as in a gate. The cost: the few games arriving after their pairing filled.

## The other kinds

- `game` and `pair` keep today's format and paths; `pair` serves the gate alone.
- `spsa` is today's tune chore renamed: entries of one network at two parameter sets, each its `K`
  tag, matched as today. Only its `kind` line changes.

## The page and the terminal

- The round robin's live table ranks by score, its ±margin beside it, ties by points then the
  earlier lambda; the winner, once every pairing is complete, by points as today.

## Tests

- Protocol: a tournament's and an `spsa` chore's text and parse round-trip.
- Arena: tasks interleave across matchups by quota left; a top-up raises a quota; a given-back task
  returns to its matchup.
- Smithy: a round robin over two arenas keeps every pairing within one top-up of the others; a task
  credited to its matchup by its players; a task of players outside the chore refused; self-play, the
  gate and the tune unchanged in what they record; live standings ranked by score.
