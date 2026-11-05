# Storing a network's tuned margin

**Status:** done 2026-10-08, tests passing, except the release, which waits for GitHub releases
(TODO.md, "UCI and release"). `Network::load` reads `margin.txt` beside the file
(`nnue::margin_file_beside`, `read_margin_file`) into `Network::margin_per_ply`; the search's margin
is `search::margin_per_ply_for`: the caller's override (an arena `@<cp>`), else the network's, else
`MARGIN_PER_PLY`. Every caller but the arena's in-process players passes no override, so UCI,
self-play and replay search at a network's tuned margin. amanda-0001's `margin.txt` (125) labels
generation 2's replay; it reaches Drive and the other replay machines by hand, once.

## Why

`rotor-arena margin` (margin-search-command.md) finds a network's margin per ply and writes it to
`data/margins/<player>/margin.txt`, which nothing reads. The pipeline of margin-round-robin.md
needs the tuned margin to travel with the network: into the challenge against the champion, into
labelling once promoted, and into a release.

## Design (the developer, 2026-10-08)

- **A `margin.txt` beside the network**, read by code, one line `margin_per_ply\t<cp>`, in
  hand-crafted's pawn (`search::MARGIN_PER_PLY`'s unit). **Not `champion.txt`**, which is a record
  for people only (where the champion came from), never read for behaviour. For a champion:
  `gen-NNNN/margin.txt` beside `gen-NNNN/champion.nnue`. A directory without one is a network never
  tuned. Drive mirrors it with the champion (`UPLOADED_GENERATION_FILES`).
- **Who reads it, in development:** the engine on loading a network (`EvalFile`, or `rotor.nnue`
  beside the binary) reads `margin.txt` in the same directory, if present; the arena's in-process
  players likewise. `lambda-<l>/candidate.nnue` has none beside it and keeps `MARGIN_PER_PLY` times
  its measured pawn scale, as the lambda round robin needs. A malformed file is an error, not a
  silent default.
- **Labelling at the tuned margin too (the developer, 2026-10-08):** replay and self-play search a
  champion at its `margin.txt` like play does, not at 200, although at a fixed depth a tighter margin
  only saves nodes (faster labels) and earns no depth back (slightly less accurate ones).
- **Precedence:** an `@<cp>` player name over the file, so the margin search can run on a tuned
  champion; the file over `MARGIN_PER_PLY`. Always scaled by the measured pawn scale, as today.
- **A release** embeds the network and its margin at build time: testers (CCRL) run the released
  binary as it is. `EvalFile` stays for development.

- **Who writes it (the developer, 2026-10-08):** `rotor-arena margin` itself, beside the player's
  network: `data/networks/amanda/gen-0001/margin.txt` for amanda-0001; for a candidate,
  `lambda-<l>/margin.txt` beside its `candidate.nnue`, so it challenges the champion at its tuned
  margin. Hand-crafted, with no network, in `data/margins/hand-crafted/`. Implemented
  (`margin_search::margin_path`).

## Promotion carries it (the developer, 2026-10-08)

No manual copying after amanda:

1. The margin search tunes the selected candidate before the challenge: `lambda-<l>/margin.txt`.
2. **The challenge refuses a candidate with no `margin.txt`** (the developer: the duel "cannot
   run without margin.txt"). A champion with none is played at the default: hand-crafted, or a
   champion promoted before tuning existed.
3. **`rotor-train promote` refuses one too**, and otherwise copies `lambda-<l>/margin.txt` to
   `gen-k/margin.txt` beside `champion.nnue`, through a staging name, before `champion.txt`, which
   stays last since it is what makes the champion.
4. `margin.txt` joins `UPLOADED_GENERATION_FILES`, so Drive gets it with the champion.
5. `match_player`'s `is_promotion_file` takes it too, so every machine completing a promotion from
   Drive (a new generation: no `champion.txt` locally) fetches it.

**Implemented 2026-10-08 (3 to 5, not yet compiled or tested):** `run_promote` refuses an engine
network whose `lambda-<l>/margin.txt` is missing or malformed, before writing anything, and copies it
beside `champion.nnue`; `MARGIN_FILE` in `UPLOADED_GENERATION_FILES`; `pull_promotion` and
`replay::load_champion` download `champion.nnue`, `margin.txt`, then `champion.txt`, one at a time
(name order would put `margin.txt` after `champion.txt`). Also `rotor-train install` puts the
champion's `margin.txt` beside `rotor.nnue` in `bin/`, or removes a stale one, so lichess-bot plays at
the tuned margin. `upload_or_exit` uploads one file at a time too, `champion.txt` last (the developer,
2026-10-08), so Drive never shows a promoted champion without its margin.

**amanda-0001, by hand, once:** her `margin.txt`, written on the Lichess machine by the margin search, copied to
Drive and to every replay machine's `data/networks/amanda/gen-0001/` before the replay restarts.

## Inside `rotor-arena select` (the developer, 2026-10-08)

`select` runs the margin search itself, between the lambda round robin and the challenge: the round
robin (depth 9, every candidate at the default margin), then the winner's margin search
(`margin_search`, best of 400 at 10+0.1, writing `lambda-<l>/margin.txt`), then the challenge (depth
9, the candidate at its tuned margin), each stage resumable as today. Mixed controls within one
`select`. `rotor-arena margin` stays as a command of its own, though perhaps never used in practice
once `select` runs it.

**Implemented 2026-10-08, tests passing; not yet run on a real generation:** `margin_search::search_margin`, shared by
the command and `run_selection`, which calls it on the round robin's winner after `selection.txt`
names it, at `config::STC` (fixed in code, the developer 2026-10-08: 10+0.1, fishtest's short time
control), then refuses to gate a winner without `margin.txt`. **The matches' folder follows the
player** (the developer, 2026-10-08): a generation's network under `gen-NNNN/selection/margin/`,
beside the round robin and the gate, for both commands; hand-crafted under `data/margins/`. On
the training machine, where `select` runs, STC is a slightly longer control in effect than on the Lichess machine: accepted.
amanda-0001's own search, run before this, stays in `data/margins/amanda-0001/`.

## Tests

- `nnue`: a network loads with the margin of the `margin.txt` beside it, none without one, and
  refuses six malformed files.
- `search`: `choose_best_move_with_margin` at `MARGIN_PER_PLY` is `choose_best_move` node for node,
  and half the margin searches a different tree; a requested margin beats the default.
- `match_player`: a promotion completed from Drive brings its `margin.txt`.

## The release

No GitHub releases exist yet: `rotor-train install` is the only install, and it puts `margin.txt`
beside `rotor.nnue`. Embedding the network and its margin in a release binary is recorded in TODO.md
for when releases come.
