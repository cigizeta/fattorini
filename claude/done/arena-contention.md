# Arena contention: two smithies at once

**Status:** designed 2026-10-29 with the developer; built, tested and checked on two machines the
same day (the developer: "it seems to work"). Beyond the decisions: Install claims the arena it
installs on and keeps it until the install ends; what a machine has installed is shown only for an
arena held, never remembered; Install offers PeSTO and full runs' generation winners alone. Follows smithy-anywhere.md, which
lets several smithies run at once on any machines, and discovery-and-heartbeats.md, whose claim
and polls stay.

## The problem

Today a smithy claims every arena of `machines.yaml` at its start and polls them until it stops,
with or without a job; an arena serves one smithy, and turns every other claim away while its
holder polls (60 s unpolled releases it). So the first smithy started holds every arena, idle or
not, and a second one's job gets no games at all, its own machine's arena included.

## Decisions

1. **Arenas claimed only while a job plays.** A smithy claims its arenas when a job needs games (a
   forge's play, round robin and gate, a tune, a test, PeSTO's play) and releases them when the job
   ends or stops, and while it is in a phase that plays nothing (training, a scatter, a freeze). An
   idle smithy holds no arena. This alone settles the usual case: a full run training leaves the
   arenas to another smithy, and claims them back for its round robin.
2. **Full before scaled.** A claim says whether its job is full or scaled (a tune or a test is full,
   unless a scaled network's tune). An arena held for a scaled job gives way to a full job's claim:
   its chores dropped, dealt again by their smithy as a lost arena's are.
3. **Between two jobs of the same kind, the smithies' ranks (the developer, 2026-10-29).** Smithies
   are ranked in `machines.yaml`; an arena held for a job gives way to a claim of the same kind from
   a smithy ranked higher, and turns away one ranked lower. A smithy not ranked comes after every
   ranked one; two unranked smithies, first come keeps. Rare enough not to need a fair split: a split
   by workers was considered and rejected as too complex.
4. **What gives way to what, in order:** a full job over a scaled one, whatever the ranks; then the
   higher-ranked smithy; then whoever holds the arena.
5. **The page** shows each arena as held here, held by another smithy (its rank, never its machine's
   name), or free; a job waiting for arenas says so. `arena status` says which rank holds it.

6. **One list (the developer, 2026-10-29):** `machines.yaml` holds `machines: [<name>, ...]`, every
   machine, highest rank first. A smithy claims the arena of every listed machine (one running none
   just does not answer, retried as a lost arena is) and ranks by its own machine's place, as
   `data/machine.yaml` names it; an unlisted smithy ranks last. `smithy:` and `arenas:` go: a file
   still holding them is refused at start, the message showing the new form.

Plan: arena-contention-implementation.md.
