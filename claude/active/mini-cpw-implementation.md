# A mini CPW: implementation plan

Plan for mini-cpw.md. The page list is companion-book.md's table of contents, parts 2 to 5,
checked against the code on 2026-11-25: kept where it still holds, rewritten where the code
changed, dropped where nothing uses it, and new techniques added.

## Layout

```text
docs/
  .nojekyll
  index.html            home: one paragraph, the topics, a link to the README
  style.css
  network/   training/   search/   tuning/
```

**The hierarchy (written 2026-11-25):** two levels only. `index.html` holds the whole table of
contents, one section per chapter (Topics, below); the header's chapter
links point at those sections, so there are no topic index pages to keep in step. Technique pages
live in their topic's folder. A page not yet written is listed as plain, muted text
(`class="soon"`), becoming a link when its page exists; before the first merge to `main`, entries
still unwritten are removed or the merge waits, as mini-cpw.md's "never an index of placeholders"
asks. The logo lives in `docs/logo/`, the one authority (the developer, 2026-11-25): moved there
from `logo/`, since Pages serves only `docs/`; the README, the `.exe`'s icon, the smithy's favicons
and `bin/logo.sh` read it there.

**Layout (the developer, 2026-11-25):** mobile first, one column, the same on every screen (no
sidebar on wide screens). The contents in a `<details>` fold (open on the home page), each entry
a link at least 44px tall; no numbers on the contents or the chapter headings, as Wikipedia; a
"Top" link on each chapter heading. Fonts by generic family only: `system-ui, sans-serif` for
text, `monospace` for code.

Every page: the same header (site name, links to home and the four topics), the same footer (last
revised, the license), the shape of mini-cpw.md's conventions, links to related pages by relative
path.

## Pages

**Network** (`network/`)

1. `accumulator.html`: the sparse first layer, its incremental update, two perspectives; a
   perspective rebuilt through a refresh cache when its king changes bucket or side.
2. `piece-square-inputs.html`: one row per piece and square, as the first networks.
3. `halfk-inputs.html`: rows per king position, the board flipped for Black and mirrored when the
   king is on files e to h; king buckets (8); castling and en passant rows.
4. `threat-inputs.html`: rows for a piece attacking an opponent's piece, shared by every bucket,
   recomputed at each evaluation.
5. `factorization.html`: rows shared across king buckets in training, folded into each bucket when
   the network is exported.
6. `screlu.html`: clipped and squared clipped ReLU.
7. `pairwise.html`: pairwise products of clipped halves.
8. `hidden-layers.html`: the small layers after the transform (16, 32, 1).
9. `quantization.html`: integer weights, i16 accumulators, SIMD inference.
10. `centipawns.html`, "Expected score, expected value and centipawns" (renamed 2026-11-25, the
    developer): the two ranges of an outcome (mini-cpw.md), the conversions between them and
    centipawns (Lc0's tangent, the logistic and its scale, Elo's base 10), why training uses the
    logistic, the network's unit on Elo's scale (elo-scale.md), and the centipawns a GUI is shown.
10b. `pawn-scale.html` (added 2026-11-25, the developer; under Tuning, `tuning/`, before page 38,
    the developer the same day): what a pawn is worth in the network's
    unit, measured from the teacher's games (a pawn up scores about 60% in Lc0's, about 30 units);
    why it is not 100, and what uses it (the search's margins, page 38). **Merged with page 38**
    (the developer, 2026-11-25): one page, "The pawn scale", the measurement and the translation of
    the search's margins together; page 38 is gone.
11. `hand-crafted-evaluation.html`: PeSTO's tapered piece-square tables (the gate's opponent).

**Training** (`training/`)

12. `lc0-data.html`: positions and scores from Lc0's training games, converted.
13. `labels.html`: the blend of a position's score and its game's result.
14. `lambda-polyline.html`: the blend's weight as a function of a position's progress through its
    game, measured from the data.
15. `loss-and-optimizer.html`: cross-entropy on the win probability, AdamW, warmup.
16. `budget-schedule.html`: a fixed number of passes, the learning rate falling geometrically,
    checkpoints kept for the choice of a candidate.
17. `transform-bound.html`: the transform's weights clamped in training, so quantization never
    overflows.

**Search** (`search/`)

18. `terminal-positions.html`: mate by distance, stalemate, repetition, the fifty-move rule,
    insufficient material.
19. `alpha-beta.html`: negamax, fail-soft alpha-beta, principal variation search.
20. `iterative-deepening.html`: with aspiration windows.
21. `transposition-table.html`: entries, bounds, replacement, mate scores, prefetch.
22. `quiescence.html`: stand pat, delta pruning, SEE, the move cap, check evasions.
23. `static-exchange-evaluation.html`: SEE itself, used by ordering, pruning and quiescence.
24. `move-ordering.html`: the staged picker, the TT move, MVV-LVA, killers.
25. `history.html`: history with gravity and malus, continuation history.
26. `correction-history.html`: the static evaluation corrected by past search results, keyed by
    pawns and by other pieces.
27. `pruning.html`: mate distance, reverse futility, razoring, futility, late move pruning (and the
    "improving" flag), history pruning, SEE pruning.
28. `null-move.html`: null move pruning, its reduction growing with the margin over beta.
29. `reductions-and-extensions.html`: late move reductions, internal iterative reduction, the check
    extension.
30. `lazy-smp.html`: several threads over one shared table.
31. `tablebases.html`: Syzygy WDL in search, DTZ at the root, the root's tablebase score.
32. `time-management.html`: soft and hard bounds, the soft bound scaled after each depth by five
    factors, move overhead, pondering.

**Tuning and testing** (`tuning/`)

33. `testing-games.html`: openings from an unbalanced book, colour-reversed pairs, adjudication.
34. `elo.html`: Elo from results, and its error.
35. `sprt.html`: the sequential test, and the duel's stopping rule per pair.
36. `round-robin-and-gate.html`: choosing a candidate among checkpoints, then a gate against a
    fixed opponent.
37. `spsa.html`: SPSA, as fishtest runs it, for search and time constants.
38. `network-units.html`: the search's margins expressed in the network's own unit.
39. `opening-books.html`: Polyglot books built from games, lines judged by search.

**Topics (the developer, 2026-11-25):** five chapters, in this order: Evaluation, Training, Search,
Tuning, Trials (`#evaluation`, `#training`, `#search`, `#tuning`, `#trials`; folders `evaluation/`,
`training/`, `search/`, `tuning/`, `trials/`). Time management sits under search, being part of
it. Tuning holds pages 37 and 38; Trials holds 33 to 36. Opening books (39) sit under search, last
(the developer, 2026-11-25: they have nothing to do with trials), their lines judged by search. The page lists below keep their numbers; the folders follow the chapters. The first
chapter, proposed as "Networks", is "Evaluation" (the developer, 2026-11-25): PeSTO is not a
network but must be described, so it joins the networks there, its page `pesto.html`.

**Dropped from companion-book.md:** output buckets (no current network uses them), the lambda
grid trained in lockstep, patience and learning-rate stages, tablebase adjudication of self-play
(self-play is paused).

## Steps

1. `docs/.nojekyll`, `style.css` (the logo's palette, light and dark, readable widths), the page
   template, `index.html`. Previewed locally by the developer. No notation or glossary page: terms
   are defined in context and kept consistent (mini-cpw.md).
2. The developer merges to `main` and turns Pages on (Settings, Pages, Deploy from a branch, `main`,
   `/docs`); the home page seen at the site's address.
3. Search (pages 18 to 32), one page at a time: drafted by Claude from the code, reviewed by the
   developer, previewed locally. Search first: the most stable part of the engine.
4. Network (1 to 11), then training (12 to 17), then tuning and testing (33 to 39), the same way.
5. The README links to the site once the four topics have their pages.

Each page is read against the code it describes before it is written, never from `claude/` alone:
the documents there record decisions, the code what is done.

No references or citations on any page (mini-cpw.md).
