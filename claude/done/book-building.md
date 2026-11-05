# Rotor opening book: a Polyglot book from master games

**Status:** done 2026-10-14, all tests passing, installed on the Lichess machine for Lichess with amanda's
champion. The replay's use of the same graph is ccrl-sources.md.

`arena book` (`crates/arena/src/book.rs`) builds a Polyglot book from a PGN database of strong games:
every move played at least \\(N\\) times from its position, weighted by the points it scored.
`smithy install --book` puts it beside the engine as `rotor.bin`, where lichess-bot reads it. The
Polyglot key, move encoding and file bytes are `rotor::polyglot` (`crates/rotor/src/polyglot.rs`),
in the engine's crate because `smithy` needs the key too.

## Sources

Any PGN, plain or `.zst`. Each source has its own folder, `data/books/<source>/`, named after the
file's stem (`LumbrasGigaBase_OTB_ELO2400` for Lumbra's Elite file), holding its fit, its graph and
its books, all on the remote. Games with a `[FEN]` header are skipped.

## N from the database's size

\\[ N = \max\Bigl(3, \Bigl\lceil 10 \cdot \bigl(G / 1{,}000{,}000\bigr)^{\alpha} \Bigr\rceil\Bigr) \\]

- \\(G\\) is the number of games read.
- **10 at a million games is a fixed anchor**, a deliberate choice of how much a book move must weigh,
  the same for every source. 3 is a floor: a book move is never one or two games.
- **\\(\alpha\\) is data, measured per source by `fit`.** A growing database is also more diverse, so a
  theory position's share of it falls: a linear \\(N\\) (\\(\alpha = 1\\)) would slowly squeeze theory out,
  a square root (\\(\alpha = 0.5\\)) let the book grow by 40% over 2.5 times the games.

## `arena book fit --pgn <path>`

Measures \\(\alpha\\) from how the source grew, and writes `data/books/<source>/fit.txt`.

1. One pass for the games' dates. A game whose `[Date]` is not a full `YYYY.MM.DD` in digits
   (`2005.??.??`, `????.??.??`) cannot be placed between two dated games: it is counted and left out
   of every point.
2. **Date prefixes** where the dated games reach 40%, 55% and 70% of their total, cut by day, so a
   source spanning months (a Lichess export) cuts as well as one spanning centuries. A source whose
   dates span too few days to cut is refused. By date, not random samples: a random half has the
   whole's diversity, so its counts halve exactly and any \\(\alpha\\) near 1 would fit by construction.
3. A graph for each prefix and for all dated games, at \\(N = 3\\), as `graph` builds them.
4. Each graph's book size (moves with at least \\(N\\) games) at every whole \\(N\\), interpolated on a log
   scale in between.
5. \\(\alpha\\) by fixed point: from \\(\alpha = 1\\), the whole gets
   \\(N_{\mathrm{ref}} = 10 (G/10^6)^\alpha\\), real-valued; each prefix gets the \\(N\\) at which its book is as
   large as the whole's at \\(N_{\mathrm{ref}}\\); the least-squares slope of \\(\ln N\\) against \\(\ln G\\) over all
   points is the next \\(\alpha\\), until it settles.
6. `fit.txt`: source, games, games without a full date, the dates' span, the shares and cut dates,
   each point (date, \\(G\\), \\(N\\)), the anchor, \\(\alpha\\), today's \\(N\\), the date.

Caveat for a source spanning only months: it barely diversifies over that time, so its prefixes are
close to random samples and \\(\alpha\\) may come out near 1. Compare with Elite's before trusting it.

## `arena book graph --pgn <path>`

The positions reached by at least \\(N\\) games and the moves between them, as
`data/books/<source>/graph.txt`. \\(N\\) is the rule's, with \\(\alpha\\) from the source's `fit.txt`; the
graph refuses to run without one.

- **Identity:** the Polyglot key, so transpositions pool.
- **Support:** the number of games reaching a position at least once; a game repeating a position
  counts once.
- **Nodes:** positions with at least \\(N\\) games; the initial position always.
- **Edges:** a move some game played from one node to another, with its wins, draws and losses
  from the side that played it, once per game. Games without a result count in no edge's results.
- **A DAG:** an edge returning to a position earlier in the same game is dropped, and so is any edge
  that still closes a cycle across games, found by a depth-first walk from the initial position in
  move order. Only what that walk reaches is kept: a position can reach \\(N\\) games by transposition
  while each of its parents stays below \\(N\\).
- **Two passes:** the first counts each position's games, the second collects the edges between the
  positions that made it. Plies are read up to `--max-plies` (60), a bound on the first pass's
  memory; the record says how many positions at the cap still reached \\(N\\).

**Options, for experiments:** `--min-games <N>` overrides the rule; `--until <YYYY.MM.DD>` (or
`<YYYY>`, its last day) keeps fully dated games up to that day; `--output <path>` writes a local
file and leaves the remote graph alone.

**File:** a header of `name<TAB>value` lines (source, pgn, \\(\alpha\\), `min_games`, max plies, until,
the games read, used and skipped by reason, positions at the cap, cycle edges dropped, nodes, edges,
leaves, date), then `node` lines (key, games, leaf flag, FEN) by ply, then `edge` lines (parent,
Polyglot move, child, wins, draws, losses) by parent.

## `arena book write --source <name> [--weights points|counts]`

The Polyglot book over the source's graph, at the graph's own \\(N\\) (its `min_games` line), as
`data/books/<source>/book-n<N>-<weights>.bin` and its record `book-n<N>-<weights>.txt`, book first,
record last, both moved to the remote.

- **A move is kept** when it was played at least \\(N\\) times from its position. A graph edge alone is
  not enough: both its ends have \\(N\\) games, but the move between them may have fewer when the
  child is mostly reached by another order.
- **No min-score, no probability threshold.**
- **Weights:** `points` by default, \\(2W + D\\) from the mover's side, Polyglot's own convention;
  `counts`, \\(W + D + L\\), as the alternative. A move weighing 0 (under `points`, every game lost) is
  left out, since a weight is never written as 0, which readers take as "never play".
- **Encoding:** per position, the weights scaled to 16 bits by one factor when the largest does not
  fit (`scaled_weights`); entries sorted by key, then heaviest first, then by move.
- **Record:** the graph's header as `graph_*` lines, the weights, positions, moves, edges below
  \\(N\\), moves weighing 0, the date.

**Inspection:** Scid vs. PC (`winget install --id Stevenaaus.ScidvsPC`), **Windows → Book Window**.
A book open there cannot be replaced: Windows refuses the rename, so close the Book Window before
writing the same file again.

## `smithy install <network>|hand-crafted [--book <path>|none]`

- `--book data/books/<source>/book-n<N>-<weights>.bin`: a path under the remote root, which must
  exist; copied beside the engine as `rotor.bin`, its record carried into `rotor.txt` under `book_`,
  and `book` names the path.
- `--book none`: `rotor.bin` removed, `book none` recorded.
- No `--book`: `rotor.bin` stays, and the previous `rotor.txt`'s book lines are carried over, so
  installing a new champion never drops the book. `none` when no `rotor.bin` is there; `unknown`
  when one is there that no line describes.

**lichess-bot**, `polyglot` section of its `config.yml`: the book is `rotor.bin` beside the engine;
`normalization: "sum"` with `min_weight: 1` drops moves under 1% of their position (the root keeps
e4, d4, Nf3, c4, g3); `max_depth` counts moves, not plies (lichess-bot uses the book while the ply
is at most `max_depth * 2 - 1`), set to 20.

## Elite, 2026-10-14

- **`fit`:** 863,790 games, 75,328 without a full date (8.7%), left out; dates 1851.01.01 to
  2026.07.06; cuts at 2008.03.09 (315,395 games, \\(N\\) 3.864), 2012.09.08 (433,673, 4.946) and
  2016.12.29 (552,209, 6.051), the whole 788,462 (8.219); **\\(\alpha = 0.8252\\)**; 269 s.
- **`graph`:** \\(N = \lceil 10 \cdot 0.86379^{0.8252} \rceil = \lceil 8.86 \rceil = 9\\); 191,917 nodes,
  222,024 edges, 36,424 leaves peaking at plies 15 to 22, 15 at the 60-ply cap, 42 cycle edges
  dropped; 101 s.
- **`write --weights points`:** 145,189 positions, 196,306 moves (1.35 a position); 25,704 edges
  below \\(N\\) and 14 moves weighing 0 left out. In Scid vs. PC the root reads e4 43%, d4 37%, Nf3
  11%, c4 7%, g3 1%, then rare first moves below 0.5% each; after 1.e4, c5 45%, e5 28%, e6 11%, c6 8%.
