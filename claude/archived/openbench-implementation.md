# OpenBench for SPSA: implementation plan

**Superseded 2026-10-20, see distributed-spsa.md:** OpenBench retired, its scripts, `Makefile`,
`rotor parameters` and UCI tuning options removed; only `rotor bench` stays.

**Status: implemented 2026-10-20, tests passing on the Lichess machine; `rotor bench` deterministic
(hand-crafted: 3,116,586 nodes twice, its own reference, below search_bench's 3,139,294 for its
16 MB table and no repetition history).** OpenBench set up on the Lichess machine (server and client), and a
smoke SPSA tune on hand-crafted (125 iterations, 1,000 pairs) playing, 2026-10-20: private
source by token, Makefile build, bench and the ten options all worked. Learned on the way: Git
Bash's older MinGW DLLs break fastchess (fixed in `bin/openbench-client`); the engine config's
current format is `test_presets`/`tune_presets`/`datagen_presets` (`import_engines`); a tune
needs its Info field filled (an OpenBench bug reads `base_branch` otherwise); the client logs in
with a registered account, never the superuser.
**Early stop through OpenBench (2026-10-20):** early-stop.md's three rules, decided for SPSA alone,
as the tune preset's fastchess settings (`win_adj` `movecount=3 score=600`, `draw_adj`
`movenumber=34 movecount=8 score=20`, `syzygy_adj` `6-MAN`), with `syzygy_wdl` `OPTIONAL` so the
engines probe as they play; `bin/openbench-client` passes the data root's WDL tables (`--syzygy`).
The tune preset also defaults to 1,250 iterations (10,000 pairs). As
built: `rotor bench` and `rotor parameters <network.nnue>` (stdin) are subcommands of the engine
binary, in place of a `bin/openbench-parameters` script; the local settings are
`data/openbench.conf` (example in `examples/openbench.conf`); `Network::from_bytes` loads the
embedded network; the bench's determinism is checked by running it twice by hand (step 1), since a
depth-9 bench is too slow for a debug test. MultiPV searches keep the network's own parameters. Implements open-bench-spsa.md:
SPSA out of the box first (the developer, 2026-10-20), OpenBench with fastchess, rather than
`arena spsa` (spsa-implementation.md). What OpenBench requires of an engine was read in its source
(open-bench-spsa.md, "What OpenBench requires of Rotor").

## Scope and order (the developer, 2026-10-20)

- **Now: develop the OpenBench setup only.** Smoke tests, no real tune.
- **Before the first real tune: the pawn scale `c` replaces `p`** (TODO, Evaluation, the BIG item;
  human-rotor.md), so the margins are tuned in the unit they will keep.
- **Machines.** the Lichess machine is always the server. For now it is also the only client; the training machine may join as
  a client, and any client may join or retire at any time. Outside development, the Lichess machine's client runs
  only while the Lichess machine is not playing Lichess.

## What already exists

- The ten parameters are `SearchParameters`' fields (`search.rs`), all integers, with ranges and
  `c_end` in spsa.md's table. No float option is needed.
- `parameters.txt` beside a network (`nnue::read_tuning_beside`) holds a tune's values; a tune's
  result only has to be written into it.

## Changes in Rotor

1. **Ten UCI spin options**, one per `SearchParameters` field, CamelCase names (`RazoringMargin`,
   `ReverseFutilityMargin`, `FutilityMargin`, `NullMoveMargin`, `DeltaMargin`, `SeePruneMargin`,
   `SeeMargin`, `AspirationDelta`, `HistoryPruningMultiplier`, `GoodQuietThreshold`). Advertised with
   the loaded network's values (its `parameters.txt` or `margin.txt`, else `DEFAULT`) and spsa.md's
   ranges as `min`/`max`. A `setoption` overrides one field for the next searches; loading another
   `EvalFile` resets all ten to that network's tuning.
2. **An embedded network.** `ROTOR_EVALFILE` set at build time makes `build.rs` embed that `.nnue`
   (`include_bytes!`); unset, nothing is embedded and the binary is as today. Precedence at startup:
   `rotor.nnue` beside the binary, else the embedded network, else hand-crafted; a GUI's `EvalFile`
   overrides all. The release needs the same (tuned-margin-storage.md). An embedded network has no
   `parameters.txt` beside it: it plays `DEFAULT`, or what `setoption` sends.
3. **`rotor bench`:** a command-line subcommand, not UCI: search_bench's 49 positions at a fixed
   depth, one thread, a fresh table per position, the embedded network (or hand-crafted). Prints a
   last line `<nodes> nodes <nps> nps`, the form OpenBench's `bench.py` parses. Deterministic for a
   given binary, as OpenBench requires the node count to match the test's.
4. **A root `Makefile`** for OpenBench's `make -j EXE=<path> EVALFILE=<path>`: runs
   `cargo build --release -p rotor` with `ROTOR_EVALFILE=$(EVALFILE)`, then copies
   `target/release/rotor.exe` to `$(EXE).exe`. Called from Python, not Git Bash, so it must work
   under GNU make for Windows; to check which shell its recipes get.
5. **`bin/openbench-parameters`:** turns a tune's final values (pasted from the server's page) into
   the network's `parameters.txt`, refusing a name or value out of range.

## OpenBench, outside the repository

- A clone at a path of the developer's choice, run as is; the pinning patch (open-bench-spsa.md)
  later, if games prove noisy without it.
- **`bin/openbench-server`** (the Lichess machine) and **`bin/openbench-client`** (any machine): thin Bash
  wrappers over `manage.py runserver 0.0.0.0:<port>` and `client.py`, reading the clone's path, the
  server's address and the client's account from a gitignored local file, with a committed example
  holding placeholders (as `data/engines.yaml`).
- **Joining and retiring** is OpenBench's own: a client polls the server for work, so starting
  `bin/openbench-client` adds a machine and stopping it retires it; its unfinished share is handed
  to others.
- **Never committed:** the server's address and port, OpenBench usernames and passwords, the GitHub
  token in `credentials.rotor`. The firewall rule for the port is opened by hand on the Lichess machine, on the
  private network only.
- **Rotor's engine entry** on the server (OpenBench's `Config/`): its build path, `bench` expected,
  and the ten parameters' SPSA lines from spsa.md's table, kept in the clone, not here.

## Settled (the developer, 2026-10-20)

- **Every tested commit is pushed to GitHub first:** OpenBench downloads Rotor's source at the test's
  commit, the token reaching the private repository.
- **No provenance kept (the developer, 2026-10-20):** a tune is run, its values decided on, and
  written into `parameters.txt`; no record of the test, no tag. A tested commit lost to a squash
  merge changes nothing.
- **Bench depth 9**, search_bench's (about a second on the Lichess machine). The bench is not how the tune's
  games are played, which stay STC (10+0.1): OpenBench runs it on each client before playing, its
  node count proving the build matches, its nodes/s scaling the time control to the machine.
- **A client uses half its machine's logical cores**, as the arena's placement: `bin/openbench-client`
  computes it, nothing in the local file.

## Tests

- Each UCI option parses, clamps nothing (the GUI's job, as `Hash`), and reaches the search.
- `rotor bench` gives the same node count twice.
- The embedded network loads, and a `rotor.nnue` beside the binary wins over it.
- `openbench-parameters`' output reads back through `nnue::read_tuning_beside`.

## Steps for the developer

1. Build and test (`cargo test -p rotor`).
2. Install GNU make and a C++ compiler on the Lichess machine (winget), clone OpenBench, fill the local file.
3. `bin/openbench-server`, then `bin/openbench-client` on the Lichess machine; a smoke SPSA test of a few hundred
   pairs on hand-crafted, to check the options, the results and the plots.
