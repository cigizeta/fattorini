# UCI

## Design

Move generation, make/unmake, legality, and perft are all done and verified (see
`move-generation.md` and `perft.md` in `claude/done/`). Rotor still has no way to actually
be played against, though -- `main.rs` is a `println!("Hello, world!")` placeholder. This
branch builds a minimal UCI protocol loop, the only external interface CLAUDE.md rule 6
allows, so Rotor can be pointed at from a real GUI or another engine.

This also removes the `perft`-only dev binary rule 6 carved out an exception for: perft
folds into UCI itself as `go perft <depth>`, the same convention Stockfish and Ethereal
both already use (confirmed during the `perft` branch's own cross-engine comparisons).
Once the binary is gone, that carve-out is no longer needed, so rule 6 reverts to its
original strict "UCI only, no custom CLI" wording.

### Commands supported

`uci`, `isready`, `ucinewgame`, `position [startpos | fen <FEN>] [moves <m1> <m2> ...]`,
`go` and `go perft <depth>`, `quit`. This is the minimal set a GUI actually needs to play
a game against an engine -- no `setoption` (nothing configurable yet), no `debug`, no
`ponderhit` (no pondering).

### State

One `Board`, held for the life of the process, replaced fresh whenever a `position`
command arrives, then advanced move-by-move through any trailing `moves ...` list via the
existing `make_move`.

### Move notation bridging

UCI moves are plain coordinate notation (`e2e4`, `e7e8q`) -- a fixed shape, always
unambiguous, nothing like SAN's contextual disambiguation (`Nbd2`) or PGN's much larger
surrounding grammar (headers, move numbers, comments, nested variations). Considered and
confirmed simpler than PGN support during design discussion; not pursued further since
UCI GUIs never send or expect PGN.

- **Parsing** (string -> `Move`): generate `legal_moves` for the current position, match
  a candidate by `from`/`to`/promotion letter. Reuses code already built and tested,
  rather than re-deriving `MoveKind` (castle, en passant, double push) from the string by
  hand -- the string alone can't distinguish an ordinary king step from a castle, or an
  ordinary pawn capture from en passant, but the legal-move candidate it matches against
  already knows.
- **Formatting** (`Move` -> string, needed for `bestmove` and `divide`'s per-move lines):
  squares via the existing `Square::file().to_char()`/`rank().to_char()` (already used by
  `to_fen`); promotion via a new lowercase-letter mapping, since UCI's promotion letter is
  always lowercase regardless of color -- a different convention from FEN's
  case-encodes-color scheme already noted in `fen.rs`'s own comment on
  `piece_from_fen_char`.

### `go` behavior

- `go perft <depth>`: divide format, Stockfish's own familiar shape. Generate root
  `legal_moves`, for each make it, recurse into the existing buffer-based `perft` helper
  at `depth - 1`, print `<move> : <count>` per root move, then `Nodes searched: <total>`.
  Reuses `perft` as-is -- no changes needed there.
- Plain `go` (no search engine exists yet): pick uniformly at random among `legal_moves`
  (via the `rand` crate -- generic infrastructure, allowed under rule 1) and reply
  `bestmove <move>`. A placeholder "engine" until real search lands in a future branch --
  lets a GUI actually play a full game against Rotor, badly, end to end, before search
  exists.
- No legal moves (checkmate or stalemate) when `go` is received: reply `bestmove 0000`.
  Confirmed against the UCI protocol spec itself (not any engine's implementation, per
  rule 1) -- `0000` is the spec's own defined null move for exactly this case. (Some
  engines reply `bestmove (none)` in practice, but that's non-compliant with the written
  spec; `0000` is correct.)

### Malformed or unrecognized input

Confirmed against the UCI protocol spec directly: *"If the engine or the GUI receives an
unknown command or token it should just ignore it and try to parse the rest of the
string,"* and *"If the engine receives a command which is not supposed to come... it
should also just ignore it."* So the loop never panics or exits on unexpected input --
an unknown command, a garbage `go perft` depth, an illegal move inside `position ...
moves ...` all leave state unchanged and move on to the next line, matching the spec's
own robustness requirement rather than an arbitrary local choice.

### Testability

A UCI loop is inherently about stdin/stdout, but rule 4 ("assertions, not eyeballing")
means the actual logic underneath shouldn't only be checkable by typing at a live
process. Split parsing/logic from actual I/O:

- `parse_command(line: &str) -> UciCommand` -- a plain enum, unit-testable directly
  against input strings, no I/O involved.
- `Engine::handle(&mut self, command: UciCommand) -> Vec<String>` -- returns response
  lines as data rather than printing them; unit-testable by asserting on what comes back
  (including, for the random `go`, asserting the returned move is a member of
  `legal_moves` rather than any exact move).
- Only `main` (or a thin `run` function) actually touches stdin/stdout: read a line,
  `parse_command`, `engine.handle(...)`, print each returned line, loop until `Quit`.

### File layout

New `src/uci.rs` (parsing, `Engine`, `handle`), registered in `lib.rs`. `src/main.rs`
shrinks to a thin wrapper calling into it. `src/bin/perft.rs` deleted entirely --
`perft()` itself, and its own fast `#[test]`s in `perft.rs`, are untouched.

### README

Gains a section with the six standard positions' FENs (already live-verified against the
Chess Programming Wiki during the `perft` branch) paired with copy-pasteable
`position fen ...` / `go perft N` command pairs -- replacing the dev binary's
named-position convenience, which was inherently a custom-CLI feature UCI itself has no
equivalent for (UCI's `position` command only ever takes `startpos` or a literal FEN, no
named positions).

## Decisions summary

- Minimal command set: `uci`, `isready`, `ucinewgame`, `position`, `go` (plain and
  `perft`), `quit`. No `setoption`/`debug`/`ponderhit` -- nothing to configure yet.
- Move notation parsed by matching against `legal_moves`, not by re-deriving move kind
  from the string; formatted via existing square-char helpers plus a new UCI-specific
  lowercase promotion-letter mapping.
- `go perft <depth>` replies in Stockfish's own divide format (per-move breakdown, then
  total); plain `go` replies with a uniformly random legal move (`rand` crate) as a
  placeholder until real search exists; no legal moves replies `bestmove 0000`, per the
  UCI spec's own defined null move.
- Malformed/unrecognized input is always ignored, never panics or exits -- directly
  matches the UCI protocol spec's own stated robustness requirement.
- Parsing/logic (`parse_command`, `Engine::handle`) kept fully separate from actual
  stdin/stdout, so all of it is unit-testable without a live process.
- `src/bin/perft.rs` deleted; CLAUDE.md rule 6 reverts to its original strict "UCI only"
  wording (the perft-binary carve-out is no longer needed).
- README gains the six standard FENs plus paired UCI commands, replacing the dev binary's
  named-position lookup.

## Implementation progress

Done, in the planned order:
1. Move notation (`parse_move`/`format_move` in `src/uci.rs`), tested independently
   (plain moves, promotions, castling and en passant resolved purely by matching against
   `legal_moves`, malformed text rejected).
2. `UciCommand`/`parse_command`, tested directly against input strings (fixed commands,
   `position startpos`/`fen`/`moves`, `go`/`go perft`, malformed input all falling back to
   `Unknown`).
3. Along the way: `src/lib.rs` removed. It only existed so `src/bin/perft.rs` (a second
   binary) could share compiled modules with `main`; once that binary is gone, one binary
   needs no library split, so `main.rs` reverted to private `mod` declarations directly,
   same shape the project had before the `perft` branch introduced the split. Confirmed
   no other file referenced the crate externally (`rotor::...`) before removing it.
4. `Engine`/`handle` (`uci`/`isready`/`ucinewgame`/`position`/`go perft` divide/plain
   `go`'s random move and `bestmove 0000`), each tested by asserting on the returned
   `Vec<String>` -- including a stalemate FEN whose zero-legal-moves precondition is
   itself asserted in the test, not just assumed.
5. `run()` added as the one function that actually touches stdin/stdout, reading
   `parse_command`/`Engine::handle` results and printing them; `main.rs` now just calls
   `uci::run()`. `src/bin/perft.rs` deleted (required once `lib.rs` was gone, since it
   depended on the crate being a library).
6. CLAUDE.md rule 6 reverted to its original strict "UCI only" wording.
7. README gained the six standard FENs paired with copy-pasteable `position fen ...` /
   `go perft N` UCI commands, replacing the deleted dev binary's named-position lookup.

`cargo test`: 133 passed (122 before `Engine` -- move notation and command parsing --
plus 11 new `Engine`/`handle` tests).

Manual smoke test: `cargo run --release`, typed by hand. `uci` replied with `id
name`/`id author`/`uciok`; `isready` replied `readyok`; `position startpos` + `go perft
1` produced all 20 divide lines and `Nodes searched: 20`; `go` replied a legal
`bestmove`; `position fen <kiwipete>` + `go perft 5` produced `Nodes searched:
193690690` -- an exact match to the value already cross-verified against Stockfish and
the published Chess Programming Wiki figure during the `perft` branch, this time
produced through `go_perft`'s new divide/formatting path rather than the deleted dev
binary. `quit` exited cleanly.

Nothing left pending -- this branch's goal (a minimal, testable UCI loop replacing the
custom perft CLI) is complete.

## Progress log

- 2026-09-10: `uci` branch created (from `main`, after the `move-buffer` branch merged).
  Design discussed and settled as described above. Two points were checked against the
  UCI protocol spec itself rather than assumed: malformed/unrecognized input handling
  (ignore and continue -- the spec's own explicit requirement) and the reply when no
  legal moves exist (`bestmove 0000`, the spec's defined null move, not the non-compliant
  `bestmove (none)` some engines use in practice).
- 2026-09-10 (continued): implemented in the planned order (move notation, command
  parsing, `Engine`/`handle`, the stdin/stdout loop). Along the way, dropped `src/lib.rs`
  entirely -- it only existed to let the now-deleted `perft` dev binary share modules
  with `main`, so with a single binary again it reverted to `main.rs`'s own private `mod`
  declarations, the same shape the project had before the `perft` branch. `cargo test`:
  133 passed. Manual smoke test via `cargo run --release`, typing real UCI commands by
  hand: identity/handshake commands replied correctly, `go perft 1` from startpos gave
  all 20 divide lines and the right total, `go` gave a legal `bestmove`, and `go perft 5`
  from Kiwipete gave exactly 193,690,690 -- matching the value already independently
  verified (published figure, Rotor's own prior perft, and Stockfish) back in the
  `perft` branch, now reached through UCI's own `go perft` instead of the deleted dev
  binary. This branch's goal is complete.
