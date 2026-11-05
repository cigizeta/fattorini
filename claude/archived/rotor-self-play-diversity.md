# Self-Play Diversity for Rotor Training Data

**Archived:** a discussion only, built on the Lumbra Polyglot book and `rotor-curate`, both gone. `rotor-arena self` takes its openings from an EPD book, and most data now comes from replayed TCEC/CCRL chunks (corpus-windows.md).

**Note:** the Polyglot module and the `rotor-curate self-play`/`book-info` drivers this document
refers to were retired with `rotor-curate` (retire-rotor-curate.md); the module is last present at
commit `08afda6`. `rotor-arena self` draws its openings from an EPD book instead.

Resume with: "resume from rotor-self-play-diversity."

## Status

Discussion only -- nothing implemented yet from this document. This captures a broad research
and design discussion about how to get diverse self-play games out of a deterministic search,
grounded in what Stockfish (and, where relevant, Leela) actually does, researched via fetched
primary sources rather than recalled (CLAUDE.md rule 2). It follows on from, and assumes,
`rotor-curate-polyglot-book.md` (the polyglot book module and the `rotor-curate self-play` manual
test driver, both already implemented).

## The core problem

Rotor's search is deterministic: `choose_best_move(position)` always returns the same move for
the same position. Consequences:

- A polyglot book (or any seed-position source) only ever supplies randomness up to the point a
  game leaves book coverage. Once off-book, any two games that land on the *same* position play
  out identically forever after -- `choose_best_move` is a pure function of the position.
- Total achievable game diversity is bounded by (seed/book branching) x (departure timing), not
  by anything that happens later in the game.
- Seed-pool size and in-game randomization are **compounding, not substitutable**. A huge seed
  pool with a fully deterministic continuation still yields exactly one game per seed, no matter
  how long each game runs. A modest seed pool combined with genuine in-game randomization yields
  many distinct games per seed.

## 1. Seed / starting positions

- Original approach: a frequency-weighted polyglot book generated from SCID + Lumbra's Giant
  Master Database. Already built and measured: `lumbra.bin` (unfiltered) has 6,052,083 records /
  5,363,505 distinct positions; `lumbra10.bin` (`-min-game 10`) has 166,926 records / 122,153
  distinct positions.
- `-min-game N` semantics, confirmed against primary sources (`ddugovic/polyglot`'s own README,
  and `sshivaji/polyglot`'s `book_make.cpp` source, `if (entry->n < MinGame) return false;`):
  filtering is per `(position, move)` **entry**, not globally on move text and not per-position-
  as-a-whole. A specific move needs >= N occurrences *at that exact position* to survive as a
  book entry; a position stays in the book as long as at least one of its candidate moves clears
  the bar, even if others at the same position don't.
- Weight-0 entries are excluded by convention (confirmed via python-chess's own reference reader
  defaulting `minimum_weight` to `1`) -- already implemented in `polyglot_book.rs`.
- Real precedent for a **flat pool of complete starting positions**, structurally simpler than a
  weighted per-ply book: Stockfish's own real testing infrastructure (fishtest/cutechess) uses
  named EPD/FEN opening suites, e.g. `UHO_Lichess_4852_v1.epd` ("Unbalanced Human Openings", ~4852
  positions drawn from real Lichess games, deliberately biased to cut draw rate, used with
  `order=random, plies=16` -- one whole fixed-length entry per game, no per-ply weighting at all).
- 4852 is right-sized for SPRT-style engine-vs-engine testing (both engines see the same fixed
  positions, so opening luck cancels across the comparison) and far too small alone for self-play
  *training-data* diversity, a different purpose.
- Practical fix, not requiring any new tooling: sample directly from the already-downloaded
  `data/games/LumbrasGigaBase_OTB_ELO2400.pgn` (~5M real games), taking the position at a fixed or
  randomized ply from as many distinct games as wanted -- trivially reaches hundreds of thousands
  to millions of distinct seed positions, dwarfing 4852, with no scarcity problem and no polyglot
  weighting machinery needed for this specific purpose.

## 2. In-game move-selection randomization

- **Temperature/softmax sampling over near-best root moves** needs real MultiPV support --
  **not implemented in Rotor at all**: confirmed zero references to MultiPV anywhere in
  `search.rs`; `choose_best_move` only ever returns a single `(Vec<IterationReport>, Option<Move>)`
  pair. Implementing it (Stockfish's own root-move-exclusion-and-re-search technique: search
  normally, then re-search with the previous best root move(s) excluded, once per additional PV)
  is real, separate work, not a small addition to the existing single-PV loop.
- MultiPV cost is **not a fixed multiplier** -- per CLAUDE.md rule 5, worth measuring once
  implemented, not assuming. Reasoning for why it's sub-linear in practice: once the primary PV's
  search finishes, the TT is already densely populated across most of the tree at that depth, so
  searching for PV2 (root move excluded) mostly reuses that work via TT hits. Cost balloons only
  when the actual 2nd/3rd-best root move is genuinely close in score to the best -- a null-window
  test on a clearly-inferior alternative fails low quickly and cheaply; a close one doesn't get
  that shortcut and needs real, mostly-unreused search to resolve.
- **Fully random legal moves** -- a cruder, valid alternative: decouples diversity entirely from
  both the book and the eval, at some cost to game quality/realism.
- Real-world context (not to be over-indexed on): fishtest's own dominant diversity lever is
  actually starting-position variety (section 1 above), not in-search randomization -- Stockfish's
  own search is otherwise as deterministic, run-to-run, for a fixed position as Rotor's.

## 3. Stockfish's own real self-play diversity knobs (`gensfen`)

Documented directly in `nodchip/Stockfish`'s `docs/gensfen.md` (fetched, not recalled):

- `random_move_minply` -- minimum ply at which a random move may replace the search's own choice
  (default range ply 1-24, max 5 random moves per game).
- `random_multi_pv` -- number of PVs to pick that random move from; if unset, a truly random legal
  move is chosen instead.
- `book` -- optional starting positions, `.epd` format only (a flat list, not a weighted
  polyglot-style book).
- `depth` / `depth2` -- a **search-depth range**, not a fixed depth, for evaluating each position.
  A genuinely separate diversity lever from move-choice randomization: a shallower search
  sometimes finds a different "best" move than a deeper one would, purely from search-horizon
  differences, with zero explicit randomization logic needed.
- `seed` -- "can be either a number or a string... if not specified then the current time will be
  used" -- defaults to wall-clock time, so independent generation runs/threads don't silently
  duplicate each other's games.
- `eval_limit`, `detect_draw_by_consecutive_low_score`, `detect_draw_by_insufficient_mating_material`
  -- early-stopping/adjudication. A data-*quality*/efficiency concern (not wasting compute on
  already-decided games), not a diversity mechanism.
- `write_minply` / `write_maxply` -- controls which plies of a game actually get written out.
- `ensure_quiet` -- see section 5 below.

## 4. Reconciling "vary the move-selection depth" with "labels must stay Search(9)"

- Rotor's `SEARCH_DEPTH = 9` in `rotor_curate.rs` is a fixed, deliberate constant -- matches
  Stockfish's own real `gensfen` convention (depth 8-10 in practice), an odd depth specifically to
  dodge the Odd-Even Effect. It must not vary per position, or label quality/consistency breaks --
  this is a hard constraint the developer raised directly ("I must search at depth 9 to generate
  labels! I cannot generate labels at depth 3.").
- Resolution: decouple **which move gets played** from **what score gets stored**, exactly like
  `select_move`'s `search_fallback` closure already decouples them for the book. `play_game`
  already re-runs a fresh, fixed-depth `choose_best_move` on whatever position was actually
  reached, regardless of how the move leading there was chosen -- so a depth-varying mechanism can
  be scoped to *move selection only*, leaving the label pass untouched and still always at depth 9.
- **Best refinement found, strictly better than a separate cheap search**: `choose_best_move`'s
  iterative deepening already computes and stores one `IterationReport` per completed depth
  (`1..=9`) on the way to the depth-9 result used for the label (`search.rs:2825-2839`, `pv` field
  per report). Depth 8's own chosen move is already sitting at `reports[7].pv[0]` (0-indexed;
  `budget: None` so every depth up to 9 always completes, per `play_game`'s existing call shape) by
  the time the depth-9 search finishes. **Zero extra search cost** -- it's a byproduct of the
  search already being run to produce the label. Diversity comes from genuine cross-depth search
  instability (the same underlying phenomenon the Odd-Even Effect names), not from any new
  computation.
- Open, unmeasured question: how often depth-8 and depth-9 actually disagree on the root move in
  practice. With a good TT and aspiration windows, adjacent depths often converge on the same move
  well before depth 9, especially in quiet positions -- likely more divergence in sharp positions,
  less in quiet ones. Worth checking empirically (rule 5) once real self-play games exist, rather
  than assuming this alone is sufficient diversity.

## 5. Quiet-position filtering for training data

Two distinct, real Stockfish-ecosystem mechanisms exist; this discussion initially conflated them
before disentangling:

**a. `ensure_quiet`** (`gensfen`, data-*generation* time) -- documented exactly as: "this is a flag
option. When specified the positions will be from the qsearch leaf." Explored and **set aside**
for Rotor: quiescence is its own branching search over many candidate captures/checks, just like
the main search, returning one backed-up score via its own internal minimax -- there is no single
well-defined "the leaf" unless quiescence itself tracks and returns the actual best line it took.
Confirmed Rotor's `quiescence` (`search.rs:2325-2327`) returns only `i32`, no move, no PV; and the
main search's own PV construction doesn't reach into it either -- `alpha_beta_with_buffer` returns
`self.quiescence(...)` directly at depth 0 (`search.rs:1721`, `2241`), nothing appended to `pv`
beyond that handoff point. Getting the true quiescence-resolved position would require quiescence
to track and propagate its own local best-line moves the way the main search already does for
`pv` -- real, nontrivial plumbing through a hot path, not something already available.

**b. `--smart-fen-skipping`** (`nnue-pytorch`, data-*loading*/filtering time) -- confirmed directly
from the real, official `official-stockfish/nnue-pytorch` source
(`data_loader/cpp/training_data_loader.cpp`):

```cpp
if (config.filtered && (e.isCapturingMove() || e.isInCheck()))
    return true; // skip this entry
```

Skip a stored record if the move recorded with it is a capture, or the position has the side to
move in check. Much cheaper than (a): uses only data already in the record, no search or
quiescence call needed at filter time.

**Which move does `isCapturingMove()` check -- played, or a separately-tracked "true best"?**
Confirmed: the **played** move. `gensfen`'s own human-readable record format (`.plain`:
`fen <fen> move <move> score <score> ply <ply> result <result>`, confirmed from
`fairy-stockfish/variant-nnue-tools`' `convert.md`) carries exactly one move field per record,
populated from whatever was actually executed at that point in the self-played game (the search's
own normal choice, or a diversity-injected move) -- there is no separate "best move" annotation
anywhere in the format. The filter is a pure post-hoc check over a finished record; it needs
nothing known "a priori" before move selection happens.

**Practical implication for Rotor**: reuse the pattern `play_game`'s own `positions_excluded`
already establishes -- move selection and label-eligibility are two separate decisions. Whatever
move actually gets played (book draw, temperature sample, random move, or a depth-8 `pv[0]`
choice per section 4), only decide *after the fact* whether the resulting position qualifies as a
quiet training example: `is_in_check` (already `pub`, `movegen/attacks.rs:68`) plus a capture
check against the board the move was chosen from -- captures aren't stored on `Move` itself
(confirmed, `chess_move.rs:35-37`: "whether the destination holds an enemy piece is discoverable
from the board at the time, not a fact that needs storing on the move itself"), so this needs the
pre-move board, not just the `Move` value in isolation.

The earlier "store both the best move and the played move" idea has **no supporting precedent** --
Stockfish's own real format carries exactly one move per record. Dropped as unnecessary complexity
Rotor doesn't need either.

## 6. Scale reality check

- "Best nets trained with 16B depth-9-scored positions, training runs >200 epochs" sounded like an
  impossible bar. Clarified via `official-stockfish/nnue-pytorch`'s own `config.py`: `epoch_size`
  defaults to `100,000,000` -- one epoch is a fixed 100M-position slice, **not** a full pass over
  the entire 16B-position dataset.
- So 200 epochs x 100M = ~20 billion positions processed in total -- roughly **1.25 passes** over
  the full 16B pool, not 200 full passes (which would have been 3.2 trillion position-
  evaluations). Considerably less scary than it first sounded.
- Reaffirms CLAUDE.md's own project charter, already on the record: Rotor isn't trying to match
  this absolute scale. The actual question is how much of the strength gap to Stockfish is genuine
  design difference versus compute-scale difference, under Rotor's real (one machine, one GPU)
  budget -- not closing the absolute gap in dataset size.

## 7. Does Stockfish still use `gensfen` self-play at all, today?

No, not primarily anymore. The most current tracked dataset history (robotmoon.com's own NNUE
training-data page, the T80 period, 2022 onward) shows Stockfish's NNUE training data substantially
incorporating **Leela Chess Zero's own training data**, converted into Stockfish's own binpack
format -- not fresh `gensfen` self-play for that portion. The newest official nets (SFNNv10+)
reportedly document their datasets in a separate `threats.yaml` file rather than the old
gensfen-centric pipeline (the `nnue-pytorch` wiki's own "Training datasets" page is itself stale
past ~2021, with an explicit unresolved `TODO` acknowledging this).

Leela's self-play (MCTS + a policy network + Dirichlet noise injected at the root) is naturally
far more diverse than plain alpha-beta self-play ever could be. Stockfish's own practical answer
to "my self-play isn't diverse enough" ended up being largely "get diverse positions from an
entirely different engine's self-play," not "fully solve it in-engine" via `gensfen`'s own knobs
alone.

Not an option for Rotor -- CLAUDE.md rule 1 (no chess-crate/data dependencies beyond the one named
tablebase exception) rules out borrowing another engine's self-play output outright.

## 8. How Stockfish actually evaluates Leela-derived positions

Does it just copy Leela's own evaluation? **Evolved over time** -- confirmed from two different
primary sources, at two different points, not one clean unchanging answer:

- **The SF14 period (2021)**, Stockfish's own blog post, quoted verbatim: *"The LCZero team has provided
  a collection of billions of positions evaluated by Leela that we have combined with billions of
  positions evaluated by Stockfish to train the NNUE net that powers Stockfish 14."* At that
  point, part of the dataset genuinely carried Leela's own value-head numbers as training targets
  -- i.e., yes, at that time, they partly did use Leela's own evaluation.
- **The T80 period (2022+)**, confirmed from PR #4295's own description on `official-stockfish/Stockfish`:
  practice changed to **not** trusting Leela's own evaluation at all. Quoted verbatim: *"The new
  Leela T80 part of the dataset was prepared by downloading test80 training data from all of Oct
  2022 and Nov 2022, rescoring with syzygy 6-piece tablebases and ~600 GB of 7-piece tablebases,
  saving best moves to exported .plain files"* -- and, separately, other portions filtered using
  *"depth6 multipv2 search to throw away positions where either of the 2 best moves are
  captures."*

**Current, real mechanism (the T80 period and later)**: use Leela's self-play only as a source of diverse
*positions* -- the entire value of that data is the lines Leela's MCTS+policy-network self-play
visits that plain alpha-beta self-play never would reach -- then discard Leela's own scores
entirely and re-derive labels from Stockfish's own ground truth: exact tablebase probes wherever a
position is within tablebase range (better than any learned eval, since tablebases are exact), and
Stockfish's own shallow multipv search elsewhere, purely to decide keep/discard via the quiet-move
filter (here extended to check the top-2 candidates via multipv2, not just the single best move,
per section 5's `isCapturingMove()`/`isInCheck()` check generalized to more than one candidate).

This is the exact same principle already embodied in Rotor's own `play_game`: positions may come
from any external source (real PGN games today; hypothetically another engine's self-play, though
not an option here per rule 1), but labels always come from Rotor's own search, never borrowed
from wherever the position originally came from.

## Open questions, not yet decided

- Whether/how to combine seed pools: curated `lumbra10.bin`-style filtered polyglot book,
  additional positions sampled directly from the raw Lumbra PGN at varying plies, and/or a
  supplementary random-legal-move pool.
- Whether to implement MultiPV (needed for real temperature sampling over near-best root moves),
  and if so, at what cost -- needs measuring once built, not assumed (rule 5).
- Whether the free depth-8-vs-depth-9 move divergence (section 4) is, by itself, enough diversity,
  or needs combining with something else (book draws, random moves, MultiPV/temperature) --
  needs empirical measurement once real self-play games exist.
- Whether/how to implement quiet-position filtering for Rotor's own training data: the cheap
  `--smart-fen-skipping`-style check (in-check + played-move-is-capture, using data already
  available at label time) is the practical choice that emerged; the `ensure_quiet`/qsearch-leaf
  approach was explored and set aside as expensive, currently-unjustified plumbing.
- No decision yet on self-play's actual output schema (extending the existing SQLite `plies` table
  vs. something else) -- deferred. The earlier "store both best move and played move" idea was
  dropped, since no real precedent (Stockfish's own included) stores both.
- The ply-8/10%-book-departure policy's own fate (from `rotor-curate-polyglot-book.md`'s own open
  point D) is still separately open, and now sits alongside these newer levers (MultiPV/temperature,
  depth-8-divergence, random moves) as one of several possible diversity sources rather than the
  only one.

## Progress

Discussion only, this session. Nothing beyond the already-existing `polyglot_book.rs` module and
the `rotor-curate self-play` / `rotor-curate book-info` manual test drivers (see
`rotor-curate-polyglot-book.md`) has been implemented from any of the above.
