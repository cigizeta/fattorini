# Tablebases on both sides of every gate and match

**Done:** every UCI player in a match gets `SyzygyPath` (the data root's WDL tables), and the match loads them in process first, so missing tables stop it (`match_player.rs`).

Status: implemented 2026-09-27, awaiting a real match.

## Where things stand

- **gate**: both players are in-process (`Builtin` or `Network`), so `pairing::load_tablebase_if_needed`
  loads the data root's tables once and both sides probe the same `SyzygyTablebase`. Since
  data-root.md, missing tables stop the gate. Already fair; no change.
- **match**: every player is a UCI subprocess (`match_player.rs`), and none is sent `SyzygyPath`,
  so neither side has tables. Even, but not what we want: both should play with them.

## The change

1. `match_player::uci_entry` adds `SyzygyPath` to every player, beside `EQUAL_FOOTING_OPTIONS`: the
   two `data_root::syzygy_directories()` joined with `std::env::join_paths` (the OS separator, the
   one both Rotor's `parse_syzygy_path_value` and Stockfish split on). Rotor and Stockfish both
   accept the option, so every current player gets it.

2. A UCI engine given a bad `SyzygyPath` plays on silently without tables (Rotor by design, see
   `uci.rs`; Stockfish too). So `rotor-arena match` checks the tables itself before the first game:
   `load_tablebase_if_needed` becomes `load_tablebase`, always calling `data_root::load_tablebase()`,
   which exits naming the directory and `bin/syzygy.sh`. A UCI-only match then loads the tables
   in-process once, unused beyond the check; opening them only indexes the files, so it is cheap.

3. The `match_player.rs` module doc drops "Neither side loads tablebases" and says both do, through
   `SyzygyPath`, checked in-process first.

## Not changing

- Stockfish's other Syzygy options stay at their defaults (`SyzygyProbeLimit` 7, which covers our
  6-piece tables; `Syzygy50MoveRule` true, matching Rotor).
- We have WDL tables only, no DTZ, for both sides alike.
- Existing match folders: `spec.txt` does not mention tablebases, so a resumed match would mix games
  with and without them. Accepted: matches so far were exploratory; start new ones afresh.

## Tests

- `resolve_player` entries carry `SyzygyPath`, equal to the joined `syzygy_directories()`, for
  `hand-crafted`, `stockfish-<level>`, `stockfish-elo-<rating>` and `<network>-NNNN`.
