# Each champion stored in the generation that won it

**Done:** the champion N_k lives in `gen-k/champion.{vs,nnue,txt}`, and `promote` writing `champion.txt` closes generation k (`generation.rs`). Only `gen-0001/initial.vs` remains, as the shared random start.

## Why

The numbering is already right in the code's model (`generation.rs`): generation k trains on the
games of N_{k-1} and produces N_k, and N_0 is the hand-crafted evaluation. Self-play sources
(`data/self/amanda/gen-0001` = N_1's games), gate labels and match names (`amanda-0001` = N_1)
follow it.

Storage does not. `promote` writes N_k into the generation that *starts* from it:
`gen-(k+1)/initial.{vs,nnue,txt}`. So everything that names a champion by its directory is off by
one: the winner of generation 1 is `gen-0002/initial.nnue`, the remote's `CURRENT` says `gen-0002`,
and `install` reports "generation 2 champion". The file tree, which is what gets browsed on disk and
in the Drive UI, says the opposite of the model.

Fix: N_k lives in `gen-k/champion.*`, the generation that produced it, and every pointer names it
by k.

## Decisions

### Layout

- `gen-k/champion.vs`, `gen-k/champion.nnue` (naive networks only), `gen-k/champion.txt`: N_k,
  written by `promote` into the finished generation k. `champion.txt` is today's `initial.txt`
  (generation, learning rate, step, test loss, checkpoint), plus the epoch.
- A generation's starting weights are the previous champion's, read in place:
  `gen-(k-1)/champion.vs`. No copy in `gen-k`.
- `gen-0001/initial.vs` stays, and only there: the shared random initialisation that
  `seed_new_arms` writes for the first generation. It is not a champion and never uploaded.

### The current generation

`current_generation(network_dir)` = the latest champion + 1, or 1 with none, where the latest
champion is the highest `gen-k` holding `champion.vs` or `champion.nnue`. (On the training machine
both exist; a generator downloads only the `.nnue`.)

So `promote` no longer creates `gen-(k+1)/`: writing the champion into `gen-k` is itself the
promotion, and `train` creates `gen-(k+1)/` when it starts it, as it already creates `gen-0001/`.
The commit point is `champion.vs` renamed into place last (after `.nnue` and `.txt`, each via a
staging name); a crash before it leaves no champion, and the rerun rewrites the other two.
`begin_generation`/`commit_generation` and the `.partial-gen-` staging then serve only
`migrate_old_layout`.

A `gen-` directory above `current_generation` means someone put it there by hand; commands that
read the current generation refuse it rather than guess, naming the directory.

### Every reader, rewritten

| Where | Today | After |
|---|---|---|
| `promote` | builds `gen-(k+1)/initial.*` | writes `gen-k/champion.*`, refuses if `gen-k` already has one |
| `train`, `seed_new_arms` | `gen-k/initial.vs` for k > 1 | `gen-(k-1)/champion.vs` for k > 1; `gen-0001/initial.vs` as today |
| `export_challenger` message, gate (`run_gate`) | champion = `gen-k/initial.nnue` | `gen-(k-1)/champion.nnue` for k > 1 |
| `self_play_source` | unchanged | unchanged: generation k trains on `{network}/gen-(k-1)` |
| `rotor-arena self`, `champion_generator` | `gen-current/initial.nnue`, plays into `gen-(current-1)` | latest champion k: `gen-k/champion.nnue`, plays into `gen-k` |
| `match_player`, `amanda-NNNN` | `gen-(NNNN+1)/initial.nnue` | `gen-NNNN/champion.nnue` |
| `upload` / `upload_or_exit` | `gen-current/initial.*`, `CURRENT = gen-current` | the latest champion's `gen-k/champion.*`, `CURRENT = gen-k` |
| `follow_pointer` | downloads `gen-p/initial.*` into a staged new generation | downloads `champion.nnue` and `champion.txt` of `gen-p` into the local `gen-p/`, `.nnue` last |
| `install` | `CURRENT` p, `gen-p/initial.nnue`, reports p | `CURRENT` k, `gen-k/champion.nnue`, reports k |

`CURRENT` changes meaning from "the generation now training" to "the latest champion". Its
spelling (`gen-NNNN`) stays. `rotor.txt`'s `generation` is then the champion's own, so the
`champion_` prefixed copy of the record drops its now-redundant `champion_generation`.

`follow_pointer`'s "remote behind local, trust local" rule compares the pointer with the local
latest champion instead of the local current generation.

### Migration

Existing data has champions in `gen-(k+1)/initial.*`. One-time, like `migrate_old_layout`:

- **Local, automatic.** `generation::migrate_champions(network_dir)`, called wherever
  `migrate_old_layout` is today and by the arena's readers: for every `gen-j` (j >= 2) holding
  `initial.*`, move each `initial.X` to `gen-(j-1)/champion.X` (`.vs` last), then remove `gen-j` if
  that left it empty. A `gen-j` that also holds training (`lr-*/`, `epoch-*/`) stays, now without
  `initial.*`, and is the current generation by the new rule. Idempotent, and refuses a
  `gen-(j-1)` that already has a different `champion.X`. Deletable once every machine has run it.
- **Remote, by hand, once**, from the training machine, since the remote is write-once and nothing
  in Rotor renames on it: copy each `gen-j/initial.X` to `gen-(j-1)/champion.X`, rewrite `CURRENT`
  to the latest champion's `gen-k`, then delete the old `initial.*` in the Drive UI. The exact
  commands go in the progress log for the developer to run, after checking the remote with `ls`.
- Every machine runs the new binaries before the remote's `CURRENT` is rewritten. An old generator
  reading `gen-0001` would take it as behind its local `gen-0002` and keep playing N_1 locally,
  which is harmless, but nothing should run mixed versions for long.

### Out of scope

- The `<network>-NNNN` naming of match players is already right and stays.
- Existing self-play games and datasets are keyed by `self_play_source`, which does not change, so
  none of them move.

## Steps

1. `generation.rs`: `champion_*_path(generation_dir)`, `latest_champion(network_dir) ->
   Option<u32>`, the new `current_generation` with the refusal above, `migrate_champions`, and
   tests for each (including a crash left after `.nnue`/`.txt` but before `.vs`, and migration of a
   `gen-0002` holding both `initial.*` and training). `initial_network_path` and
   `initial_record_path` go; `initial_weights_path` stays for `gen-0001` only.
2. `rotor-train`: `promote`, `seed_new_arms`, `export_challenger`'s message, `upload`,
   `UPLOADED_GENERATION_FILES`, `install`, `install_record`, usage text, tests.
3. `rotor-arena`: `run_gate`, `champion_generator`, `follow_pointer`, `match_player` and its test.
4. `uci.rs:1308`'s test string (`gen-0002/initial.nnue`) becomes a `champion.nnue` path; it is only
   a parse test.
5. README: the training flow and "Playing on Lichess"; module docs in `generation.rs`,
   `match_player.rs`, `self_play_command.rs`.
6. Verify: `cargo test`; local migration on the training machine (`ls data/networks/amanda/*`
   before and after); the remote migration by hand; then `rotor-train install amanda` reports
   generation 1, and `rotor-arena self --network amanda` plays N_1 into `data/self/amanda/gen-0001`.

## Progress

- Design agreed (A). remote-only.md committed and pushed first.
- Steps 1-5 written on the older PC, not yet built or tested. Deviations from the design above:
  - The marker of a complete champion is `champion.txt`, not `champion.vs`: a generator downloads
    only `.nnue` and `.txt`, so the marker has to be one it has. `promote` writes `.nnue`, `.vs`,
    then `.txt`; `follow_pointer` downloads `.nnue` then `.txt`, one `copy_down` each; migration
    moves `.txt` last and writes a minimal `generation<TAB>k` record for a champion that had none.
  - `latest_champion` runs `migrate_champions` first, so every reader migrates on first use;
    `match_player` calls it explicitly.
  - `begin_generation` is gone; `commit_generation` is private to `migrate_old_layout`.
  - `seed_new_arms` takes the starting path and the generation, and asserts the champion's
    `.vs` exists for any generation after the first. `export_challenger` takes the opponent path.
  - `champion.txt` gains an `epoch` field; `rotor.txt` drops the redundant `champion_generation`.
- Next: developer builds and tests on the older PC, then runs the remote migration below, then
  `./bin/rotor-train install amanda`, which should report generation 1.
- The remote's amanda, checked 2026-09-28 from the older PC: `CURRENT` plus `gen-0002/initial.{nnue,txt,vs}`
  only (N_1, one promotion so far). The older PC has no local `data/networks/amanda`; the GPU PC's
  local layout is still to be checked.
- Remote migration for amanda, to run once every machine has the new binaries:
  `mkdir "/mnt/g/My Drive/rotor/data/networks/amanda/gen-0001"`, then `cp` each of
  `gen-0002/initial.nnue`, `initial.txt`, `initial.vs` to `gen-0001/champion.nnue`, `champion.txt`,
  `champion.vs` (the `.txt` last, being the marker), then `printf 'gen-0001\n' > .../amanda/CURRENT`, then delete
  `gen-0002/` in the Drive UI.
