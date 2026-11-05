# SPSA tuning plan: OpenBench + fastchess

**Superseded 2026-10-20, see distributed-spsa.md:** SPSA runs in Rotor's own `arena spsa`, and
OpenBench is retired. The tuning procedure and the references below still inform it.

## Goal
Tune 10 UCI options of my engine with plain SPSA (baseline).
Later: compare against my adaptive stage scheme (cosine-based learning rate).

**Where the adaptive scheme goes (the developer, 2026-10-20): a fork of OpenBench**, not Rotor's
own tuner. OpenBench's SPSA algorithm is a small part of it (the perturbations in
`OpenBench/spsa_utils.py`, and the server's update after each batch); the scheme replaces those
behind a selectable option, and everything else (clients, scaling, the page, the database) stays.
Both methods then run on the same games, engine and machines, differing only in how theta moves.
The fork is its own repository (OpenBench is GPLv3), outside Rotor's. To design first, with
Claude, before any code. `arena spsa` stays as it is.

## Hardware
- One game per physical core, on every machine
- Both engines of a game pinned to that core's two hyperthreads
- 1 thread per engine, pondering off
- No hyperthread sharing between games (it cuts search speed and adds noise)

## Setup
- OpenBench is distributed only via GitHub: `git clone https://github.com/AndyGrant/OpenBench`
- winget for prerequisites: Git, Python, GNU make, plus a C++ toolchain on every machine (the client
  builds fastchess-ob itself with `make` and `CXX`; Rotor's own build stays cargo, behind a makefile)
- Server (Django) on one PC: `python manage.py runserver 0.0.0.0:8000`, open port in firewall
- A client on every machine, pointed at the server's LAN address
- LAN traffic is negligible (small result reports per game pair)

## Time control and speed scaling
- STC: 10+0.1 (Fishtest convention; OpenBench's common default is 8+0.08)
- Reference speed = engine `bench` NPS measured on the slowest machine, under full load
- The slowest machine plays real 10+0.1; a faster one gets proportionally less time, same search effort
- Engine must support `bench` (nodes + NPS)
- Both engines of a pair run on the same PC, so speed differences hit A and B equally

## Game runner and pinning
- cutechess-cli: no built-in affinity
- fastchess: `-use-affinity [CPUS]`, e.g. "0-9"
- OpenBench client now uses fastchess (fastchess-ob binary)
- Checked 2026-10-20: `Client/worker.py` passes no affinity at all; pinning needs a patch to the
  client (`-use-affinity` on fastchess-ob's command line)
- TODO: verify both engines of a game land on sibling hyperthreads of one core
- Fallback: Rotor as game runner + small Python SPSA driver (Weather Factory style)

## What OpenBench requires of Rotor (checked in its source, 2026-10-20)

Read in OpenBench master at commit `876f5d7`: `Client/worker.py`, `Client/utils.py`,
`Client/bench.py`. Not from memory.

- **Always a build from source, never a prebuilt binary.** `prepare_engine` downloads the source zip
  at the test's commit and runs `make` in it; the only binary reused is one it built before, cached as
  `<engine>-<commit[:8]>-<net[-8:]>`. A "private" engine is the same build, its zip downloaded with a
  token from a local `credentials.<engine>` file: Rotor's GitHub repository, private today, works
  that way. The token file is never committed.
- **The build command is fixed:** `make -j EXE=<out_path> EVALFILE=<absolute network path>`, with
  `CC`/`CXX` added only for a custom compiler that is not `rustc` or `cargo`. On Windows the client
  looks for `<out_path>.exe`.
- **Networks fit the tune per network.** Each side of a test (base, dev) names its own `network`,
  uploaded to the server; the client downloads it to `Networks/<sha>`, checks the first 8 hex digits
  of its SHA-256, and passes it to the build as `EVALFILE`. A tune is tied to commit plus network,
  the network compiled in, as the release embeds the champion (tuned-margin-storage.md).
- **`bench`:** the client runs `./<binary> bench` and reads, last line first, `<n> nodes` and
  `<n> nps` (case-insensitive, punctuation ignored). The node count must equal the test's expected
  bench, so the bench must be deterministic.
- **SPSA values** reach fastchess as `option.<Name>=<value>`, formatted by Python's `str()` (a float
  as `0.123`), and fastchess sends them as UCI `setoption`.
- **Windows:** supported; the client runs `fastchess-ob.exe`.

**Rotor's work before a first tune:**
1. A `Makefile` at the repository root: `cargo build --release`, the network at `EVALFILE`
   embedded in the binary, the binary copied to `$(EXE).exe`.
2. A deterministic `rotor bench` command printing nodes and nps.
3. The ten parameters as UCI options (floats as strings, parsed by Rotor).
4. GNU make and a C++ compiler on every machine, for fastchess-ob.
5. Optional: the client patch for pinning.
6. Afterwards: SPSA's final values turned into the file beside the network, as `margin.txt` is
   today.

## SPSA input (one line per parameter)
name, int|float, value, min, max, C_end, R_end
- R_end = 0.002
- C_end ~ 1/20 of the parameter's reasonable range (~2-3 Elo effect)
- Integer params: OpenBench forces C >= 0.5; beware tiny-range "poisoned" params
- Floats: expose as UCI string option
- Defaults: alpha 0.602, gamma 0.101, A-ratio 0.1
- Batched reporting, Single distribution, 8 pairs per point

## Budget and run time
- Smoke test: 1,000 pairs (checks options, results, plots; not a real tune)
- Main run: N = 10,000 pairs = 1,250 iterations at 8 pairs/point
- Estimate: 20,000 games x ~28 s, divided by the games in parallel
- After the first hour: extrapolate from actual progress
- Budget increases = new runs (schedule is tied to N)

## What N = 10,000 delivers
- Moderate: fixes clearly mistuned parameters (where most Elo is)
- Near-optimal params: final values uncertain by ~ +/- 1-2 C_end (cheap in Elo)
- Weak params: wander harmlessly
- Still trending at the end -> second run of 10-20k from the new values

## Validation
- SPRT: tuned vs theta_0
- Plots: clear trends that flatten = real; aimless wandering = noise

## Retuning after a new network
- Eval-unit margins (futility, razoring, RFP, aspiration, SEE): strongly net-dependent
- Search-shape params (LMR, null move, extensions): mildly net-dependent
- Time management: mostly net-independent
- Procedure:
  1. If the net's eval scale changed, rescale eval margins first
  2. Warm start from previous tuned values
  3. Smaller C_end (~half)
  4. Budget 25-50% of full run (e.g., 3,000-5,000 pairs)
  5. SPRT: new net + retuned values vs new net + old values
- Track which params move each time -> eventually retune only net-sensitive ones
