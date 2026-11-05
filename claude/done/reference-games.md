# The reference games in the repository

**Status:** design, 2026-10-13. Nothing implemented.

## Why

`nnue_bench --residuals` measures a network's evaluation error on held-out human games, the
5,000-game reference `LumbrasGigaBase_OTB_ELO2400-5000.pgn.zst` in `data/lumbra/` on Drive, cut once
from Lumbra's Elite file by `--cut-reference` (evenly spaced games of 40 plies or more). The engine's
pawn scale (`src/pawn_scale_positions.txt`, pawn-scale-positions.md) is 10,000 positions of the same
reference, two per game.

The reference must not depend on Lumbra's releases (the developer, 2026-10-13): a cut repeated on a
new release, which inserts, removes and corrects games, picks other games. So the sample is defined
by its content, frozen in the repository, almost source code. Lumbra's games are not training data
(lumbra-book.md, only the book), so the reference stays held out with no exclusion needed; one was
planned while they briefly were, and dropped the same day.

## Decided (the developer, 2026-10-13)

- **The existing 5,000-game cut is the reference, frozen**, byte for byte: 1,038,490 bytes as
  `.pgn.zst`. Not a new sample: the pawn-scale positions come from it, and every `--residuals`
  number so far was measured on it, so measurements stay comparable across generations.
- **Checked into git** beside the bench, `crates/rotor/benches/reference-5000.pgn.zst`, with a short
  note crediting Lumbra's GigaBase and its licence (CC BY-NC 4.0, accepted by the developer).
- **`--residuals` and `--pawn-scale` read it from there**, not from `data/lumbra/`, with no copy down
  from Drive. `--per-game` stays.
- **`--cut-reference` stays, for developers only** (the developer, 2026-10-13): the way to cut a new
  sample, should one ever be committed. It writes into the repository beside the bench, not to
  `data/lumbra/` and no longer to Drive, and never over an existing reference, since a cut from a
  different release picks different games: a new sample is a new file, committed by hand.
  `--reference-games` stays with it, naming the size of the cut and of the reference read. Its doc
  comment says that the committed reference was cut from the 2026-07-08 Elite file and cannot be
  reproduced from a later release.
- **The `-50000` cut** stays on Drive, unread: it served only the archived margin measurements
  (expected-score-margins.md).

## Steps

1. The developer copies the file into the repository (one command, below the steps).
2. `nnue_bench`: `REFERENCE_DIR` points beside the bench; the copy down from Drive removed;
   `--cut-reference` writes there, refuses an existing file and no longer copies up; the doc
   comments updated. `--residuals` gives the same numbers as before on amanda-0001.
3. DEV.md documents the bench's two **freezing modes**, `--cut-reference` and
   `--write-pawn-scale-positions`: files written once and committed by hand, never part of a normal
   run. No wrapper in `bin/`: they run only as an explicit `cargo bench` with their flag, and
   `--cut-reference` refuses to overwrite an existing reference, so neither can run by chance. The
   top doc comment of `nnue_bench.rs` lists them together under that name.
4. The note crediting Lumbra beside the file, with its provenance: the source (Lumbra's OTB Elite,
   the file dated 2026-07-08 on Drive), the rule (`PlyCount` of 40 or more, every stride-th eligible
   game, verbatim), and its size in bytes.

Step 1 is done (2026-10-13): the file is committed, 1,038,490 bytes, checked with `wc -c`.
