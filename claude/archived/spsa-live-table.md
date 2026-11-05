# `arena spsa`: a live table and a progress bar

**Superseded, 2026-10-22: see orders.md**: `arena spsa` was removed, and the tune's table and bar
are the smithy's, on its terminal and its page (smithy-and-arenas.md).

**Status: design agreed 2026-10-20 (the developer); implemented, awaiting the by-eye test.**

## Design

On a terminal, a served or local tune draws one block, redrawn in place after every recorded pair:
the parameter table (start and now, as today's every-100-pairs table) and an `indicatif` bar under
it: pairs done of the tune's, pairs an hour and the ETA of this run (pairs replayed from `pairs.tsv`
not counted). The pattern is the round robin's `LiveTable` (`round_robin.rs`): one `ProgressBar`
whose prefix is the whole table, so an update is one redraw and does not flicker.

- **Batch lines go**, and so does the local workers' line per pair: the bar carries their progress
  and pace.
- **Events stay**, printed above the block with the bar's `println`, so they scroll while it stays:
  a client joined or restarted, a batch dropped, every WARNING.
- **Not a terminal** (stderr redirected): no block, the output exactly as today, the table every 100
  pairs and the batch lines included.
- The client's output does not change.

## Plan

1. **`spsa_command.rs`: a `Console`**, beside `theta_table`:
   - `Console::new(specs, theta, k_done, pairs)`: drawing when stderr is a terminal, hidden under
     `cfg(test)` (as `LiveTable`); the bar's style `progress::style_under(k_done, Unit::Count("pairs"))`.
   - `update(&self, theta, k_done)`: drawing, the prefix redrawn and the position set; not drawing,
     `theta_table` printed every `THETA_EVERY` pairs, as `record_pair` does now.
   - `say(&self, line)`: an event; `bar.println` when drawing, else `println!`.
   - `progress(&self, line)`: a batch or pair line, printed only when not drawing.
   - `finish(&self)`: the bar left complete, before the final parameters are printed.
   - The table's text shared with `theta_table`: one function for the rows, the blank lines and the
     `spsa:` prefix only in the printed form.
2. **`Shared` holds the `Console`**, so `record_pair` (local and remote alike) calls `update` in
   place of its `THETA_EVERY` print, and `print_progress` goes through `progress`.
3. **`spsa_remote.rs`, the server:** every `println!`/`eprintln!` from `dispatch` on (the WARNINGs,
   joined, restarted, dropped) through `state.console.say`, the batch line through `progress`. The
   two lines before the tune starts (measuring, serving) stay as they are, printed before the block
   exists.
4. **`tune_in`:** the run's first line printed before the block; `finish` before the closing
   parameters.
5. **Test:** the table's rows for `SPECS` at their starts and after one change, and a `Console`
   built under test (hidden) taking `update`, `say` and `progress` without panicking.

The developer tests by eye on the running `--first` itself: the server stopped, rebuilt and
restarted, resuming; its client left playing, since a client survives a server restart. A separate
trial tune would collide with it (one port, one `server.txt`, and a tune folder per player).
