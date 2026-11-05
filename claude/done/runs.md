# Runs: the same config done more than once

**Status: plan confirmed by the developer, 2026-10-22, proposals included; implemented
the same day, to build and test.** Replaces TODO.md's "The same job done many times". **Revised by
forge-configs.md (2026-10-22):** its runs, their folders and names stay; its form, the typed tune
network and the two frozen files go (a run freezes one `config.yaml`), and a tune is offered only
for a passed generation of an ended run, or hand-crafted.

Settled while implementing:
- A run is addressed by its name everywhere a run's name was used before (`players::run_dir`,
  `smithy check`, the archive, the arena's players), so nothing else changed shape.
- A run missing its frozen copies (moved by hand) reads `configs/` and is frozen as it starts.
- The job's words: `--forge <config> <n|new>` and `--tune <tune config> <player>`; `--spsa` and
  the form's length and limit fields are gone.
- The tune form's network is typed, suggested from every forge run's generation winners and
  `hand-crafted`, rather than picked in three steps; any player is accepted.
- A lambda's candidate (`<run>-NNNN-<lambda>`) is tuned in `tune-<lambda>/`, beside the winner's
  `tune/`.

## Why

Today a config is its folder: `data/gen/<config>/` for a forge, `data/spsa/<player>/` (or the
network's `selection/spsa/`) for a tune. Doing it again means resuming, or deleting the folder by
hand. A forge is worth repeating (run-to-run luck, a code change such as the full shuffle), and the
folder's name leaks into the players' names (`<config>-NNNN`).

## Agreed (the developer, 2026-10-22)

1. **A run** is one execution of a config: its own folder, its own frozen copy of the config, its
   own state and results. Starting a config again makes a new run; resuming continues one.
2. **Both jobs pick a config, then a run:** a dropdown of the config's active runs, plus "new run".
3. **A tune has a config too**, for symmetry with a forge: what a tune is (its length, its time
   control) moves from the form into a file.
4. **A tune belongs to a forge run:** its form also picks the forge config and the forge run whose
   network it tunes.
5. **No retuning, for now:** a network has at most one tune. An unfinished tune resumes; a new
   tune of a network whose tune is finished is refused. A wrong tune is deleted or moved by hand,
   on the command line.

## Proposed (to confirm)

1. **Layout:** `data/gen/<config>/run-<n>/`, numbered from 1, holding what `data/gen/<config>/`
   holds today plus `config.yaml`, the config as the run started. **Older runs are moved on disk by
   hand (the developer, 2026-10-22), not by the code:** a `data/gen/<config>/` still holding a
   run's files at its top (its `state.txt`) is refused, the message giving the move into `run-1/`
   and the copy of the config the run started from.
2. **Player names:** run 1 keeps today's, `<config>-NNNN` and `<config>-NNNN-<lambda>`, so every
   record stays valid; run n >= 2 is `<config>-r<n>-NNNN`. A config name ending in `-r<digits>` is
   refused, so a name is never ambiguous. **Revised 2026-10-22 (the developer): no backward
   compatibility; every run is numbered, run 1 included, `<config>-r1-NNNN`.** The old names no
   longer resolve anyway (the old `data/networks/` layout is not read), and a bare run 1 gave a new
   network an old network's name: `lc0-psq` run 1's winner is `lc0-psq-0001`, as the AB(lambda)
   network the records name.
3. **Active:** a forge run not finished (not `done`, its lineage not stopped by failed gates); a tune
   run whose pairs are not all played. Several active runs of one config are allowed; a finished
   run is closed, as today.
4. **The frozen config:** a run reads its own `config.yaml`, never `configs/<config>.yaml` again,
   so editing a config changes only new runs. Its network config is frozen alike, beside it.
5. **Which network a tune tunes:** a generation's winner of the forge run, the generation picked on
   the form, defaulting to the run's latest. Its tune lives in the generation's folder,
   `run-<n>/gen-NNNN/tune/`; its `parameters.txt` beside the winner's `.nnue`, where the engine and
   `smithy install` read it today.
6. **Tune configs:** `configs/tune-*.yaml`, as forge configs are `gen-*.yaml`:

   ```yaml
   length: first            # or: pairs: 4096, or: fine: <player whose parameters start it>
   control: 10+0.1          # or: depth: 9
   ```

7. **Networks from no forge run:** `hand-crafted` and the older networks (amanda to dorothy, the
   `lc0-*-0001` networks) stay tunable by name, listed after the forge runs on the tune form, their
   tunes where they are today.

## Steps

1. **Run folders:** `Dirs` rooted at a run; an old layout refused with the commands to move it;
   the run's frozen `config.yaml` and network config, read on resume. Tests.
2. **Player names with runs** (`players.rs`): `-r<n>` parsed and resolved, run 1 bare; the refused
   config names. Tests, including every existing name still resolving.
3. **Tune configs:** the loader, `TuneArgs` built from a config and a network; the tune's folder in
   the forge run's generation; the refusal of a finished network's new tune. Tests.
4. **The page:** `/control/choices` lists each config's active runs, and for tunes the forge runs
   and their generations; the form's fields (job, config, network for a tune, run); the job's words
   name the run; `data/smithy-last.txt` alike.
5. **Docs:** DEV.md's Jobs and Tuning sections; TODO.md.
6. Build, test, then on the Lichess machine: `gen-psq-smoke`'s folder moved into `run-1/` by hand, a second run started
   beside it, both on the form; a short tune of one of its winners from a tune config, then the
   refusal of a second tune.

## Open

- Proposal 2's spelling of a run in a name (`-r2-`), against another.
- Whether a finished forge run's page sections stay reachable from the form (a list of past runs),
  or only through the last job.
