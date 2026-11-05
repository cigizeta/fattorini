# An explicit teacher per generation

**Status:** done 2026-10-07: implemented, tested, and used for generation 2 (`rotor-train teacher
amanda amanda-0001`, `rotor-train teacher betty amanda-0001`). As built:
- `generation::Teacher` (`parse`, `name`, `source`), `champion_source`, `teacher_path`,
  `read_teacher`/`write_teacher`; `self_play_source` is gone.
- `rotor-train teacher` finds the next generation from the remote's pointer (the latest
  champion's, plus one; 1 without a pointer), so it answers the same on every machine.
- `prepare` reads the remote `teacher.txt` and copies it into the local generation directory;
  `train` reads the local copy, so its tests never touch the remote.
- `rotor-arena self` takes a `Teacher` too. It resolves its player once and returns when the
  games are complete, for a champion as for hand-crafted: the wait for a promotion and the
  pointer following are gone. It loads the champion through `replay::load_champion`, which
  downloads it if it is not local.

## Why

Generation 2 is labelled by amanda's generation-1 champion for both amanda and betty (decided
2026-10-07; caroline, amanda's shape, is retired): amanda, caroline and betty drew at depth 9, so
separate teachers would double the replay and self-play for labels no better, and a shared corpus
keeps the amanda-betty duel a test of the hidden layer alone. Per-lineage teachers come back once a duel
shows one lineage stronger. So the teacher must be choosable per generation, not implied.

Today it is implied: `generation::self_play_source(network, generation)` returns `hand-crafted`
for generation 1 and the same network's previous champion otherwise, and `rotor-train prepare`
and `rotor-train train` both call it. Nothing records the teacher except the path.

## What already fits

Labelled data is filed by its teacher, not by the network that trains on it:
`data/games/replayed/<archive>/<teacher>/`, `data/games/self/<teacher>/`,
`data/datasets/<teacher>/`, `<teacher>` being `hand-crafted` or `<network>/gen-NNNN`. Data
labelled by `caroline/gen-0001` is readable by any network, so the layout does not change.

## Design (decided 2026-10-07)

1. **`teacher.txt` in the generation it trains**, on Drive:
   `data/networks/<network>/gen-NNNN/teacher.txt`, one line, a player name: `hand-crafted` or
   `<network>-NNNN`, a promoted champion named by network and generation. Never "the latest":
   that would be ambiguous.
2. **Written by a command**, `rotor-train teacher <network> <teacher>`: it checks that the teacher
   is `hand-crafted` or a promoted champion that exists, writes the file for `<network>`'s next
   generation (the one `prepare` would build), and refuses to overwrite an existing one. A new
   lineage's generation 1 gets `hand-crafted` the same way.
3. **Read by `prepare` and `train`**, in place of `self_play_source`: a missing `teacher.txt`
   refuses, naming the file to write. No fallback to the old rule.
4. **`rotor-arena self` takes the teacher by name**, `<network>-NNNN`, like `replay` already does,
   and refuses a bare network name. Following a promotion during a run goes: a new champion is a
   new `self` command.
5. **Existing generations are left as they are.** Generation 1 is trained; nothing reads its
   teacher again.

## Steps

1. `generation.rs`: the teacher's path, read and write, parsing the player name into its folder
   (`hand-crafted` or `<network>/gen-NNNN`); `self_play_source` goes once nothing calls it.
2. `rotor-train teacher`, with its checks.
3. `prepare` and `train` read `teacher.txt`.
4. `rotor-arena self <network>-NNNN`; `replay` unchanged.
5. Docs: corpus-windows.md's "labelled by the previous champion" and the per-network configs'
   comments.

## Tests

- The command: writes the next generation's file; refuses an unknown or unpromoted teacher, a
  candidate (`<network>-NNNN-<lambda>`), a bare network name, and an existing file.
- `prepare`/`train` refuse without `teacher.txt` and name the file.
- A player name maps to the folder the replay and self-play write (`caroline-0001` to
  `caroline/gen-0001`).
- `rotor-arena self` refuses a bare network name.

## Open questions

None left.
