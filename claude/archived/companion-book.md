# The companion book

Status: **dropped (the developer, 2026-10-31),** `companion/` deleted; archived. Was: TOC adopted
into `_quarto.yml` (the developer, 2026-10-24); no chapter drafted yet.

## What (the developer, 2026-10-24)

The book in `companion/`, published with Quarto (`companion/_quarto.yml`), title "Introduction to
Chess Programming". Written for humans, but not a CLRS: every technique Rotor uses, each briefly
and concisely, readable without getting lost in narrative or endless examples.

## Decisions (the developer, 2026-10-24)

1. **Scope: Rotor's techniques only.** A technique gets a section if the engine or its training
   pipeline uses it today. Techniques Rotor does not use are left out, even famous ones (B*, SSS*,
   MTD(f), YBWC, magic bitboards), and so are architectures written but never played (halfkp,
   halfka, the stack, the transformer).
2. **Length: whatever the content needs,** not a page budget.
3. **Techniques only, no Rotor data:** no measurements, no Elo of Rotor's experiments, no history of
   its decisions. Those stay in the repository.
4. **Code: short pseudocode,** language-neutral, only where it says more than a formula or a
   sentence. No Rust listings.

## Conventions (proposed)

- **One section per technique, the same shape each time:** what it is (one or two sentences), why
  it works, the idea itself (a formula or a few lines of pseudocode), its parameters and pitfalls,
  and where it comes from (a citation). No worked examples beyond one small one where the idea is
  otherwise hard to see.
- **A chapter is a group of related techniques,** a short introduction, then its sections.
- **Citations:** the original paper or the reference implementation, in a `references.bib` read by
  Quarto (`[@knuth1975]`); the Chess Programming Wiki where no paper exists.
- **No em or en dashes** (CLAUDE.md), Markdown files as `_quarto.yml` already names them.
- **Notation fixed once** in the introduction: depth, ply, alpha and beta, side to move, centipawns,
  score in `[0, 1]`.

## Table of contents (proposed, from a survey of the code)

**Introduction:** what an engine is (board, search, evaluation, protocol), notation.

**Part 1, Board representation**
1. Bitboards and mailbox: twelve piece bitboards beside a square-indexed array.
2. Move encoding: from, to, kind (promotion, castling, en passant).
3. Move generation: leapers by table, sliders by PEXT over precomputed attack tables, pseudo-legal
   generation then legality from pinned pieces and checkers, without making the move.
4. Making and unmaking moves, incremental state.
5. Zobrist hashing.
6. Perft.

**Part 2, Evaluation**
7. Terminal positions: checkmate scored by distance, stalemate, repetition, the fifty-move rule,
   insufficient material.
8. Hand-crafted evaluation: material and piece-square tables tapered by game phase (PeSTO).
9. NNUE, the accumulator: a sparse first layer, its incremental update, two perspectives.
10. NNUE features: piece-square inputs, threat inputs.
11. NNUE heads: clipped ReLU, pairwise products, squared clipped ReLU, output buckets by material.
12. Quantization and SIMD inference.
13. From network output to centipawns: the win-probability scale.

**Part 3, Search**
14. Negamax and alpha-beta (fail-soft), principal variation search.
15. Iterative deepening and aspiration windows.
16. The transposition table: entries, bounds, replacement, mate scores.
17. Quiescence search: stand pat, delta pruning, SEE, the move-count cap, check evasions.
18. Move ordering: the staged picker, TT move, MVV-LVA, static exchange evaluation, killers, history
    with gravity and malus, continuation history.
19. Pruning: mate distance, reverse futility, razoring, null move, futility, late move pruning,
    history pruning, SEE pruning.
20. Reductions and extensions: late move reductions, internal iterative reduction, check extension.
21. Parallel search: Lazy SMP over a shared table.
22. Time management: the clock target, falling score, single-move and stop rules, pondering.

**Part 4, Training a network**
23. Training data: converting Lc0's games, self-play with exploration, tablebase adjudication.
24. Labels: score and result, the direct blend and its lambda.
25. Training: the loss, AdamW, the learning-rate stages and patience, warmup, a sparse transform.
26. Many networks at once: a grid of lambdas trained in lockstep.

**Part 5, Measurement and tuning**
27. Testing games: openings, colour-reversed pairs, adjudication.
28. Elo from game results, and its error.
29. Sequential tests: SPRT, and the duel's stopping rule.
30. Selection: a round robin, then a gate.
31. SPSA tuning of search parameters.

**Part 6, External data and protocol**
32. Opening books: Polyglot.
33. Endgame tablebases: Syzygy WDL and DTZ, in search and at the root.
34. The UCI protocol: commands, options, MultiPV, pondering.

## To decide

- The TOC above: anything missing, merged, or out.
- The order of writing: front to back, or the parts most worth having first (search, NNUE).
- Whether `scratch.md` is kept as notes or dropped once its material is in the chapters.

## Plan (once the above is agreed)

1. `_quarto.yml` rewritten to the agreed TOC, `references.bib` started, the introduction written.
2. One chapter at a time, each surveyed from the code first (what Rotor actually does), drafted,
   reviewed by the developer, then the next.
