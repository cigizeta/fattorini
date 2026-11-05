# lc0-progress-bars

## Goal

`smithy lc0` shows up to three bars at once: one per download running ahead
(`DOWNLOADS_AHEAD`, two) and the conversion. You can then see whether downloads keep
ahead of the conversion, which tells you whether `DOWNLOADS_AHEAD` could be 1, and
spot a curl retry (see lc0-download-retry.md) as a bar that stalls and then recovers.

## Design

- **Polling.** `download` starts curl with `spawn()` instead of `status()` and, until curl
  exits, sets its bar to the `.tmp` file's size every 500 ms. The total is the
  `Content-Length` the queue already reads. curl stays silent (`-s`) and no
  dependency is added. Because of `-C -`, a retry carries on from the size already
  shown.
- **`progress` crate.** It gets two new functions:
  - `multi(hidden)`: a `MultiProgress` drawn to stderr, or nowhere under test.
  - `bar_in(multi, len, unit, label)`: a bar in the usual style, built with a hidden
    target and only then added to `multi`, so none of its frames is drawn outside it.
- **Labels.** Each bar shows `downloading <date>` or `converting <date>`, padded to the
  same width so the bars line up.
- **Lifetimes.** A download bar is cleared as soon as its tar is complete. The
  conversion bar is cleared when its tar is done, and the summary line printed for
  that tar takes its place, so no more than three bars are shown at once.
- **Printing.** While the bars are drawn, every line `run` prints goes through
  `MultiProgress::println`, otherwise the bars would draw over it. `fail`'s `eprintln!`
  stays, since the process exits right after it.

## Risk

On NTFS, the size reported for a file that is still open for writing can lag behind.
If a download bar sits at 0 or moves in jumps, read the size from an open handle
instead.

## Test

- `cargo test -p progress`: a bar from `bar_in` on a hidden `multi` is hidden.
- The first real run of `bin/smithy lc0`: two download bars move while the
  conversion bar runs, and each one disappears when its download or conversion is done.
