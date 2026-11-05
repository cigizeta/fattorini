# Engine registry: every external engine from data/engines.yaml

**Status:** done; verified by amanda's matches against Stockfish through `data/engines.yaml`, whose
results are in the README (2026-10-14).

## Goal

The arena finds every external engine through one per-machine file, `data/engines.yaml`, the way
cutechess keeps its `engines.json`: each engine named once, with the absolute path to its
executable. Nothing is hard-coded and nothing is searched for. A developer on any Windows machine
copies a committed example and fills in their own paths.

Today `stockfish-<level>` and `stockfish-elo-<rating>` spawn the bare name `stockfish`
(`match_player.rs`), which only works with a `stockfish.exe` on the `PATH`. winget's package
names its executable `stockfish-windows-x86-64-universal.exe`, and a Bash alias never reaches a
process the arena spawns, so the duels against Stockfish cannot start on a fresh machine.

## Decisions

- **Per machine, not versioned.** `data/engines.yaml` stays where it is, gitignored: a path to an
  executable is the developer's own business (`engines.rs` already says so). Only Windows is assumed.
- **A committed example.** `examples/engines.yaml` shows the format with placeholder paths; the
  README says to copy it to `data/engines.yaml` and fill in the paths, as `data/machine.yaml` is set
  up by hand.
- **No GUI.** Editing the YAML is cutechess's settings dialog.
- **No downloads, no version pinning.** Which build is at a path is the developer's choice.

## Format

The existing registry format (`engines.rs`), unchanged:

```yaml
engines:
  stockfish:
    path: "C:/Users/<you>/AppData/Local/Microsoft/WinGet/Packages/Stockfish.Stockfish_Microsoft.Winget.Source_8wekyb3d8bbwe/stockfish/stockfish-windows-x86-64-universal.exe"
  reckless:
    path: "C:/engines/reckless/reckless.exe"
```

`kind` defaults to `uci`. `options`, when given, are sent to the engine.

## Changes

1. **`stockfish-<level>` and `stockfish-elo-<rating>`** take the executable of the registry's
   `stockfish` entry, then add their handicap (`Skill Level`, or `UCI_LimitStrength` and `UCI_Elo`)
   on top of the entry's own options.
2. **Every `kind: uci` entry is a player by its name** in `match`, `sprt`, `duel` and `margin`:
   `./bin/arena duel reckless amanda-0002 --time 10+0.1`. Such a player is external, so the whole
   run is over UCI (`is_external`, `plays_in_process`).
3. **Equal footing still wins.** `Hash`, `Threads 1` and `SyzygyPath` are set over whatever an entry
   says, as `uci_entry` does today, so no entry can give one side more.
4. **Clear refusals, before any game:**
   - no `data/engines.yaml`, when a run needs an external engine: name the file and the example;
   - no `stockfish` entry, for a `stockfish-...` player;
   - a `path` that does not exist;
   - an entry whose name a built-in player already has, or shadows: `hand-crafted`,
     `stockfish-...`, `<network>-NNNN[-<lambda>]`. `stockfish` itself is allowed: it is the entry the
     handicapped players read, though not a player itself (see "The bare `stockfish`" below).
   - a name containing `@`, which a margin suffix would take apart.
5. **The registry is read once per run**, so every game of a run uses the same executable. A run
   that needs no external engine does not need the file at all: Rotor against Rotor keeps working
   with no registry.
6. **The tournament mode** keeps reading the same file, unchanged.
7. **`examples/engines.yaml`**, committed: Stockfish's winget path with `<you>` in place of the
   user, and one commented example of another engine.
8. **README**: the "Stockfish's skill levels as ratings" section loses the `cp` step; a short
   "Engines" section says to copy `examples/engines.yaml` to `data/engines.yaml` and edit the paths,
   with forward slashes in double quotes (a backslash starts a YAML escape). The match section's
   list of players gains "any engine named in `data/engines.yaml`".
9. **Tests**: the handicapped players read the path from a registry built in memory; each refusal
   of point 4; a registered engine is external; equal-footing options override an entry's.

## The bare `stockfish`

Decided 2026-10-13: the `stockfish` entry only says where Stockfish is, and is not a player. As a
player name, `stockfish` is refused with a pointer to `stockfish-20`, full strength, which is the one
spelling: two spellings of the same player would split its records between two `data/matches/`
folders.

## Checks

```bash
cargo test --release -p arena
```

Then, on the Lichess machine, with `data/engines.yaml` written from the example:

```bash
./bin/arena duel stockfish-0 amanda-0002 --time 10+0.1
```

## Later, not in this plan

- Recording each UCI engine's `id name` in a run's `spec.txt`, so the records say which build
  played, and refusing to resume a run whose engine changed.
