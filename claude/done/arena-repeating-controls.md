# Arena: repeating time controls

Status: **design settled** with the developer 2026-11-13; steps 1 to 9 done the same day
(`cargo test -p arena -p smithy --release` and `cargo clippy --workspace --all-targets` clean).
Step 10, the Reckless smoke test at 40/20, done the same day: no forfeit, the clocks refilled
(reckless-time-management.md). The duel's start-up line prints the control as Rust's debug form
(`Fischer(TimeControl { ... })`), cosmetic, for the rename in TODO.md. Kept for now: the names `Control::Fischer` and `MoveLimit::Fischer`,
which now cover repeating clocks too (13 uses in 6 files); a rename to `Clock` is in TODO.md's "Code to clean up".

## Why

reckless-time-management.md's step 10, a smoke test at a repeating control, needs one, and the arena
plays Fischer controls only (`<seconds>+<seconds>`). CCRL's 40/15 is a repeating control, and the
`movestogo` path of every time management has never been played by the arena. The developer,
2026-11-13: the arena should support more time controls, increments with a repeating control
included, even if unused.

## Design

- **Notation:** `<moves>/<seconds>` or `<moves>/<seconds>+<seconds>`: `40/900` is CCRL's 40/15,
  `40/20` a short smoke test, `40/60+0.5` a repeating control with an increment. Seconds only, as
  the Fischer form; `<moves>` a whole number from 1. The Fischer form `<seconds>+<seconds>` is
  unchanged. `40/900` is also the PGN standard's form for the `TimeControl` tag; the form with an
  increment has none in the standard and is written as given.
- **The clock:** each side counts the moves it has left before the control. After each of its
  moves, the elapsed time is spent and the increment added, as today; then the count goes down by
  one, and on reaching 0 the base is added back and the count restarts at `<moves>`. Unspent time
  carries over, as on a real clock and at CCRL.
- **UCI:** `go` adds `movestogo <n>` under a repeating control, `<n>` the side to move's own
  count (the only one UCI carries); nothing changes under Fischer.
- **The in-process engine** (in_process.rs) passes the same count to `time_limit` as `movestogo`.
- **Bench scaling** (`ControlSpec::scaled`): the seconds scaled, the move count kept.
- **The smithy's page needs no notation (the developer, 2026-11-13):** the notation is for the
  command line. The New test form's clock becomes labelled number fields, in seconds:

  ```
  Clock    (o) Fischer      Base [ 10  ] s   Increment [ 0.1 ] s
           ( ) Repeating    [ 40 ] moves in [ 900 ] s   Increment [ 0 ] s
           [10+0.1] [180+2] [40/15]
  ```

  The page builds the notation from the fields and sends it in the job's words as today, so the
  text stays the one form between the page, the smithy and the arena. A wrong value is refused by
  the page itself, said beside Start: the smithy does not depend on the arena crate and passes the
  clock's text on unread (`Limit::parse`), so only the arena would otherwise catch it, game by game. Three preset buttons fill the fields, which stay editable:
  10+0.1 (the tune and the gate duel), 180+2 (the verdicts), 40/15 (CCRL); no preset for one-off
  controls, no dropdown with a "custom" mode. Defaults: Fischer 10 + 0.1; repeating 40 moves in
  900 s, no increment.
- **Unchanged:** Fischer controls, fixed depth, match folder names (engines and date, not the
  control), the smithy's tune, which plays Fischer only and keeps rejecting any other
  `TimeControl` tag.

## Steps

1. **`TimeControl`** (clock.rs) gains `moves: Option<u32>`; `parse_checked` reads the two new
   forms and rejects `0/...`, a non-integer move count, and a missing base.
2. **`Clock`** keeps `base`, `moves` and `moves_left`; `spend` refills as above;
   `moves_to_go()` returns the count, `None` under Fischer.
3. **`MoveLimit::Fischer`** (player.rs) gains `moves_to_go: Option<u32>`, filled by
   `play_one_game` (game.rs) from the side to move's clock.
4. **`uci_client::go`** takes `movestogo: Option<u32>` and sends it when `Some`; its caller passes
   `moves_to_go` through.
5. **`in_process.rs`** passes `moves_to_go` to `time_limit`.
6. **`ControlSpec::scaled`** keeps `<moves>/` and writes the increment only when there is one,
   so the text parses back; `ControlSpec::time`'s doc names the new forms.
7. **The smithy:** its job parsing (`matches::Spec::parse`) checked to accept the new forms, as it
   reads the same control text; the New test form in page.html as in "Design", replacing the free
   `clock` text field.
8. **Unit tests:** parsing (the three forms, the rejects); the clock refilling after the
   `<moves>`-th move with carry-over, and with an increment; `movestogo` sent only under a
   repeating control; scaling `40/900` at 1.12 to `40/1008`.
9. **`cargo test`** of the arena and the smithy, and **`cargo clippy --workspace --all-targets`**,
   all clean; the form tried on the page.
10. **The smoke test** of reckless-time-management.md, at `40/20`.
