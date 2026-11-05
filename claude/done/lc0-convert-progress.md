# Lc0's Convert progress on the page

Status: second design approved 2026-10-24 on the Lichess machine, after the first (a live box of indicatif
bars above the corpus, committed) was seen on the page during a dev Convert. Done the same day:
built, tested, and seen on the Lichess machine's dev Convert to its end. Replaced TODO.md's "Lc0's Convert
progress on the page".

## What the developer asked (2026-10-24)

- One graphical bar only: the corpus's bytes towards the cap.
- A status line for each download and for the conversion, no bar of their own.
- The two downloads above the conversion, and the status lines above the bar, as the training box's.
- A download not under way leaves its line blank, not removed, so the page keeps its height.
- Not a box of its own: in the Corpus section, below the "Running now" box with its Stop button.
- As the training box does it: plain numbers in `state.json`, worded by the page, no indicatif.

## The first design, seen and replaced

The child kept hidden indicatif bars in fixed places and reported them as `progress::LiveBar`s,
served in `/progress.json` as `{ bars, lc0 }` and drawn in the live box. It worked, but was a
second box above the corpus, and carried indicatif's strings where the training box carries
numbers. Kept from it: `progress::mirror` removed (lc0 was its only user), the board in fixed
places, the cap moving at each segment sealed.

## The design

1. **The child, no indicatif.** A static board (`Mutex`) of plain counts, `Flow { done, total,
   from, started }`: `downloads`, `CONNECTIONS` places (worker `i` writes place `i`, `None` while it
   downloads nothing), each with its tar's date; `conversion`, likewise; `cap`, the bytes the cap
   rule counts (the rows' `pgn_bytes` plus the converting tar's so far, moved at each segment
   sealed) of `cap_bytes`, `from` where this run started. A download sets `done` from its `.tmp`'s
   size and starts over on a curl retry; the conversion counts the bytes read through a counting
   `Read` in place of `wrap_read`. `MultiProgress`, `bar_in` and `say`'s indicatif branch leave
   `lc0/mod.rs`; `say` is a `println!`.
2. **The report:** `Lc0Live { downloads: [null | Line], conversion: null | Line, cap: Line }`,
   `Line { tar, done, total, rate, eta_seconds }` (`tar` empty for the cap), `rate` in bytes a second
   and both null before any progress this run, as the training's round. Printed as a `PROGRESS_LINE`
   every 250 ms as before.
3. **The smithy** keeps the last report (`lc0::keep`, cleared when the child ends) and serves it in
   `state.json` as `conversion.progress`, polled every 2 s like the training. `/progress.json` is a
   plain list again; `progress::snapshot` goes, unused.
4. **The page:** the Corpus section: its summary and button; while Convert runs, below them, a
   `div` redrawn at each poll (outside the section's own change check): the download lines, the
   conversion's (blank `&nbsp;` while none), the corpus's line, its bar. Worded as `roundLine`:
   `downloading 20260101-0117: 27.9 MiB of 823.5 MiB (3.4%), 1.8 MiB/s, ETA about 8m`, with a
   `bytes()` helper. The live box goes back above the job box as before this work.
5. **The summary line** (`lc0::summary`, the section's first line and the `lc0` box's) keeps only
   complete or not and the tars downloaded and converted: its bytes of the cap repeated the corpus's
   line (the developer, 2026-10-24, seen on the page).
6. **While no Convert runs,** the corpus's line and bar stay, from `tars.tsv` and the config's cap
   (`lc0::cap_bytes`, the catalog's lc0 entry's `cap`), with no rate or ETA: a finished corpus still
   shows how much of the cap it filled (the developer, 2026-10-24, after the run's end); above
   them the three places blank, so the section keeps its height when a Convert starts or ends.

## Steps

1. `progress`: `snapshot` and its test removed.
2. `lc0/mod.rs`: the board of counts, `Lc0Live`, the counting reader, indicatif out.
3. `smithy.rs`: `conversion.progress` in the state, `/progress.json` a list again, its test reverted.
4. `page.html`: the Corpus section's progress, `lc0Lines` and the live box's `lc0` part removed.
5. `cargo test -p progress`, `cargo test -p smithy`; on the Lichess machine `bin/smithy start --dev`, Convert:
   the section's height steady through a download ending, the second held back by the cap, and
   the run's end.
6. TODO.md's entry removed; this doc to `done/`.
