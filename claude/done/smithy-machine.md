# The production smithy on one machine, named on Drive

Status: implemented 2026-10-23, to build and test; not compiled when written.

## What (the developer, 2026-10-23)

The smithy never runs in production mode on the Lichess machine or another machine, only on the training machine. `machines.yaml` says so,
and lives on Google Drive, so every machine reads the same copy and the rule is enforced.

## Decisions

1. **The file is on Drive:** `data/machines.yaml` at its remote counterpart
   (`remote::remote_path`), no longer a local, gitignored file. A local copy is no longer read.
2. **Its shape, no backward compatibility:**
   ```yaml
   smithy: the training machine
   arenas: [the training machine, the Lichess machine, another machine]
   ```
   `arenas` replaces `machines`: the machines whose arenas the smithy claims. `smithy` is the one
   machine a production smithy may run on; it need not be among the arenas.
3. **`smithy start` enforces it,** before binding any port: Drive mounted, the file there and well
   formed, and this machine (`data/machine.yaml`) the one it names; otherwise it refuses, saying
   which machine runs the smithy and where the file is.
4. **`smithy start --dev` is unaffected:** it reads no `machines.yaml` and never touches Drive, so it
   runs on any machine, claiming its own arena alone.
5. **The arenas do not read it:** they are claimed, as today.

## The work

- `protocol::machines` returns both, `Machines { smithy, arenas }`, each name validated, the arenas
  at least one and none twice; its errors say how to write it.
- `smithy::start` (production only): `remote::check_mounted`, then the file at
  `remote::remote_path(MACHINES_PATH)`, then the machine's check.
- `examples/machines.yaml` in the new shape, its comment naming Drive.
- To do by hand once: write the file on Drive, delete the local `data/machines.yaml`.
