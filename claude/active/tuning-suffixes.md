# Tuning suffixes

Status: **implemented 2026-11-13** with time-management-spsa-implementation.md
(`players::TuningSuffix`), waiting for its tests.

## Why

A network gains a second tune, its time management's (time-management-spsa.md), so a player has
three tuning states, and today's `@untuned` and `@tuned` cannot name them. The order (the
developer, 2026-11-13): a network trained, then its search tuned, then its time.

## Decided

**The suffixes (the developer, 2026-11-13: "verbose, but fair; internal use only"):** each names
what is tuned, so a later kind of tune adds a word rather than making an existing name wrong.

| state | suffix | search | time |
|---|---|---|---|
| untuned | `@untuned` | defaults | defaults |
| search tuned | `@search-tuned` | `search-parameters.txt` | defaults |
| search and time tuned | `@search-time-tuned` | `search-parameters.txt` | `time-parameters.txt` |

Rejected: `@search` (unclear), `@tuned` for both (misleading once a time tune exists), `@fully-tuned`
(wrong as soon as a third kind of tune exists).

Consequences: today's `@tuned` goes away; a snapshot's name carries the suffix
(`fattorini-<network>-search-tuned-<stamp>.exe`); the time tune's verdict duel is
`@search-time-tuned` against `@search-tuned`; a time tune always starts from `@search-tuned`.

**`@tuned` renamed once, no alias (the developer, 2026-11-13),** as `search-parameters.txt`: the
code reads the new names alone; stored rows that name `@tuned` (the smithy's tests list, chores)
are rewritten once to `@search-tuned`, which is what they meant. Match folders keep their names,
history only.

**Missing files refused (the developer, 2026-11-13),** as `@tuned` is today: `@search-tuned` needs
`search-parameters.txt`, `@search-time-tuned` both files; a name never quietly plays defaults.
`@search-tuned` ignores a `time-parameters.txt` that exists, playing the default clock: the time
tune's verdict duel needs it.

## Next

Folded into time-management-spsa-implementation.md, whose step 3 (`Tuning`) and the rename of
step 2 carry it, before that plan is agreed.
