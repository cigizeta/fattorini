# Pondering

Implementation plan. Branch: `alessio`. Pondering is for Lichess only: `rotor-arena` never ponders.
Settled in discussion with the developer.

## Who can ponder

The GUI decides: Rotor ponders only on `go ponder`. There are two ways to turn pondering off, and
it is off by default:

- the `Ponder` option, `type check default false`. While it is false, `bestmove` carries no ponder
  move, so a GUI or lichess-bot has nothing to ponder on;
- not sending `go ponder`.

`rotor-arena` does neither: it never sends `go ponder` or `setoption name Ponder`, and its
`bestmove` parser reads the first token only (`uci_client.rs`). So arena games are unchanged by
construction, with no guard needed.

## Design

Only `uci.rs` changes; `search.rs` is untouched.

1. **`Ponder` option.** `uci` advertises `option name Ponder type check default false`;
   `setoption name Ponder value true|false` sets `Engine::ponder`.
2. **Ponder move.** When `Engine::ponder` is true, `bestmove X ponder Y`, with `Y` the second move
   of the selected thread's last report (`reports.last().pv[1]`); no ponder move when that PV has a
   single move. When false, the output is exactly today's.
3. **`go ponder ...`.** Parsed as a `ponder: bool` field of `UciCommand::Go`. Starts an infinite
   search (no clock) on the given position, keeping the time arguments for `ponderhit`. A ponder
   search never prints `bestmove` on its own: if it finishes early (a forced mate, `MAX_PLY`), its
   thread waits until `ponderhit` or `stop`.
4. **`ponderhit`.** A new `UciCommand::PonderHit`. Stops the ponder search silently (no
   `bestmove`), joins it, then starts an ordinary `go` with the kept time arguments. The
   transposition table is warm, so iterative deepening reaches the pondered depth again quickly.
   The clock starts at `ponderhit`, when ours really starts running, so the budget is computed as
   for any other move.
5. **`stop` during a ponder** (the opponent played another move). Stops the search and prints its
   `bestmove`, as the protocol requires; the GUI ignores it.

`ponderhit` with no ponder search running is a silent no-op, like `stop`.

### How the ponder thread knows what to do

`RunningSearch` gains a ponder outcome shared with the thread, an `AtomicU8`: pending, hit,
stopped. `Stop` sets stopped, `PonderHit` sets hit; both also set `stop_signal`. When the search
returns, a ponder thread waits while the outcome is pending (sleeping briefly between checks),
then prints `bestmove` only if it is stopped. A non-ponder search has no outcome and prints as
today. `RunningSearch` also keeps the ponder search's time arguments, for `PonderHit` to start the
real search with.

### Why restart at `ponderhit`

An iteration started while pondering has no deadline inside the tree. Switching the running search
to a clock would need a deadline it can see change mid-iteration, a `search.rs` change in the hot
path. Restarting keeps almost all of the gain (the pondered tree is in the TT) for a few fast
re-iterations. A live switch can come later if Lichess games show it matters.

Superseded on 2026-10-03: the live switch will be done regardless of what Lichess games show, as
soon as the search freeze ends (`TODO.md`).

## Tests (`uci.rs`)

- `parse_command` reads `go ponder wtime ...` (ponder set, the time fields kept), `ponderhit`, and
  `setoption name Ponder value true|false`; `uci` advertises the option.
- With `Ponder` false, a search's `bestmove` has no ponder move; with it true, a search deep
  enough for a two-move PV ends with `bestmove X ponder Y`, `Y` legal after `X`.
- A ponder search on a position that finishes at once (a mate in one, `go ponder depth 1`) prints
  no `bestmove` until `stop`, then exactly one.
- `ponderhit` during a ponder search prints exactly one `bestmove`, from the timed search.
- `ponderhit` and `stop` with nothing running are silent.

## As implemented

- Found on the command line: a search answered from the transposition table (the same position
  searched again) reports a one-move PV, so it offered no ponder move. `ponder_move` then takes the
  move the table holds for the position after the best move, matched against the legal moves by
  squares and promotion (Stockfish's `extract_ponder_from_tt`). Read after the search, so the
  search is unchanged. Two tests, one of them the command-line case: `go depth 6` twice.

- Verified by hand in `bin/rotor` (2026-10-01): the option advertised, no ponder move while it is
  off, one after `setoption`, also from the table, a ponder hit and a miss each answered once, a
  finished ponder search waiting for `stop`, `ponderhit` with nothing pondering silent. Left: a
  Lichess game, once the CPU is free.

## Lichess

lichess-bot ponders when its `config.yml` has `ponder: true` under `engine:`. That is the
developer's setting, not changed here. Note it beside `rotor-train install hand-crafted`'s
instructions in `README.md`.

## Verification

```
cargo test --release
```

Then a Lichess game with `ponder: true`: the engine's log shows `go ponder`, then `ponderhit` or
`stop`, and the bot never loses on time.
