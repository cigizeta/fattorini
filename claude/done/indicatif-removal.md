# indicatif out of the workspace

**Status:** approved and implemented 2026-10-27, steps 1 to 3 and 5 done, `smithy`'s tests and
clippy passing; the build after the 512 run (step 4). `LiveBar` keeps `length`, `percent`
and `eta` optional, so the page is untouched, though every bar now has a length. The last step of "indicatif goes, graphical bars take its
place" (TODO.md): every bar is the page's since terminal-log.md and no-text-bars.md, drawn on no
terminal, so indicatif is kept today only as a counter the page reads (`progress::live`).

## What uses it today (read 2026-10-27)

- **`progress`:** every bar is an indicatif `ProgressBar`, registered weakly for `live`, drawn on
  stderr unless `log_events` or `hidden`. Its public functions: `bar`, `resumed`, `counter`, `multi`,
  `bar_in`, `style`, `style_under`, `register`, `live`, `log_events`, `eta_text`, `rate_text`.
- **Their callers, all in `smithy`:** `progress::bar` in `buckets.rs` (the scatter) and `freeze.rs`
  (three bars), `progress::resumed` in `rounds_display.rs`, and one test in `smithy.rs`. On the
  result they call `inc`, `set_position`, `finish` and `finish_and_clear` alone. `counter`, `multi`,
  `bar_in`, `style`, `style_under` and `register` have no caller outside the crate's tests.
- **`rounds_display.rs`:** its `PassBar`, an indicatif bar for a pass with no round suffix, is
  never made: both callers of `print_progress` (`lockstep.rs`) pass a suffix, `round 0` or
  `round N`, so every pass is `note_pass`'s plain record. `clear_progress_line` only ever clears that
  absent bar.
- **`arena`:** declares the dependency and uses none of it.

## Decisions (proposed)

1. **`progress::Bar`, the crate's own counter:** an `Arc` of the label, the unit, the length (or
   none), the start position, when it was made, and an atomic position and finished flag; `live`'s
   registry holds it weakly, as today. Methods: `inc`, `set_position`, `finish`, `position`. No
   `finish_and_clear`: with nothing drawn it is `finish`, and its two callers call that.
2. **Nothing draws on a terminal, ever:** the `hidden` argument, `UNDRAWN` and the draw targets go.
   `log_events` keeps its logger only.
3. **The unused API goes:** `counter`, `multi`, `bar_in`, `style`, `style_under`, `register`, and
   `Unit` stays. `bar` and `resumed` keep their names, minus `hidden`.
4. **indicatif's two formatters, written here:** the elapsed time and ETA as `HH:MM:SS` (a day or
   more as `Nd HH:MM:SS`, as indicatif's `FormattedDuration`), bytes in binary units (`512 B`,
   `3.00 MiB`, as its `HumanBytes`), so `LiveBar`'s texts and the page are unchanged.
5. **`rounds_display.rs` without its dead bar:** `PassBar`, `PASS_BAR`, `needs_new_bar` and
   `clear_progress_line` go, with `clear_progress_line`'s calls in `lockstep.rs`; `print_progress`
   is `note_pass` alone. Its doc comments no longer speak of a terminal bar.
6. **indicatif leaves every `Cargo.toml`:** the workspace's, `progress`'s, `smithy`'s, `arena`'s.

## Steps

1. `progress`: `Bar`, the formatters, the registry on `Bar`; its tests kept where they still apply
   (the ETA and rate, a resumed bar, `live` until finished or dropped) and the formatters tested.
2. `smithy`: the callers on `Bar` (`finish_and_clear` to `finish`), `rounds_display.rs` and
   `lockstep.rs` as in decision 5.
3. The `Cargo.toml`s, then `cargo test -p progress` and `cargo test -p smithy`, and clippy.
4. **Built only once the 512 run has ended:** `bin/smithy` replaces the running `smithy.exe`. Then
   the page's bars checked at the next scatter or frozen set's read.
5. Docs: TODO.md's indicatif item, DEV.md if it names indicatif.
