# `rotor-arena self` in sync with `match`, `sprt` and `duel`

## Status

Implemented, not yet compiled or tested by the developer.

## Decided

```
rotor-arena self <player>
```

- **The player is positional**, named as the other commands name one: `hand-crafted` (was
  `--hand-crafted`) or `<network>` (was `--network <network>`), the network's latest champion,
  following the remote's pointer as before. A pinned generation, `<network>-NNNN`, is refused: self-play
  always plays the current champion, and a pinned one would fill the wrong generation's folder.
- **No `--threads`**: one game per logical core, as a fixed-depth `match` runs one pair per core.
  Single games, not colour-reversed pairs: self-play compares nothing.
- **No `--hash`**: every side's table is 16 MB, enforced. The memory guard (`memory.rs`) still runs,
  over `2 * cores` tables, before anything starts.
- **No `--depth`, no `--time`**: depth 9, enforced. A label is a Search(9) score and the ladder
  measures networks at that depth, so a dataset played at any other depth, or under a clock, would
  mean something else.
- **One usage**: `self` gets its line in `rotor-arena`'s main usage; its own usage goes.

Everything else is unchanged: exploration, segments, the remote, the lock, Ctrl+C.
