# Polyglot Opening Book Support

**Archived:** `src/polyglot_book.rs` was deleted with `rotor-curate` (last present at commit `08afda6`). Rotor reads no book; the competition book is built by `rotor-arena book` and read by lichess-bot (opening-book.md).

**Retired, to be restored** (retire-rotor-curate.md): `src/polyglot_book.rs` was deleted with
`rotor-curate`, its only user. It is last present at commit `08afda6`, with its tests. The
`self-play` and `book-info` drivers below went with `rotor-curate`.

Resume with: "resume from rotor-curate-polyglot-book."

## Status

Design settled through discussion; not yet implemented. Training/tooling-side capability, used by
whatever drives self-play games (`rotor-curate`, and eventually `rotor-evolve` -- not yet built,
mentioned by the developer as a second, future consumer) -- explicitly *not* a shipped-`rotor`-
engine feature. That conclusion took two real turns to reach, both grounded in fetched fact
rather than assumption (rule 2), not just developer preference:

1. UCI does define a standard `OwnBook` option, but real Stockfish -- the actual comparison point
   this whole project measures itself against -- carries no book-related UCI option at all
   (confirmed against Stockfish's own official UCI docs, full option list quoted below). The real,
   dominant convention is an *external* driver (a GUI's own book manager, or a tool like
   `cutechess-cli -book <file>`) that owns the book and only ever hands the engine a finished
   position via `position ... moves ...` once it's out of book -- never a UCI option on the engine
   itself.
2. Since the engine is never the one consulting the book under that convention, there's also no
   reason for the module to live in the shared `rotor` library crate the way `tablebase.rs` does
   (tablebase support earned that spot because it's a genuine search-time concern, wired directly
   into `alpha_beta_with_buffer`/`quiescence`). It belongs wherever the self-play driver(s) live
   instead.

## Grounding

- **Stockfish's real, fetched UCI option list** (official docs, not recalled): `Threads`, `Hash`,
  `MultiPV`, `NumaPolicy`, `Clear Hash`, `Ponder`, `EvalFile`, `UCI_Chess960`, `UCI_ShowWDL`,
  `UCI_LimitStrength`, `UCI_Elo`, `Skill Level`, `SyzygyPath`, `SyzygyProbeDepth`,
  `Syzygy50MoveRule`, `SyzygyProbeLimit`, `Move Overhead`, `nodestime`, `Debug Log File` --
  nothing book-related. `OwnBook` (type `check`) is real, defined in the base UCI spec (fetched
  from backscattering.de/chess/uci, maintained by niklasf -- the same author as `shakmaty`), and
  was implemented by some older engines (Fruit, Toga, Crafty), but not Stockfish.
- **Where this lives**: a shared file, `#[path]`-included into every binary that needs it --
  exactly the existing `pgn_replay.rs` pattern (already included into both `rotor_curate.rs`'s
  and `rotor_train/main.rs`'s own crate roots for the same reason: training-tooling code shared
  across multiple binaries, never linked into the shipped `rotor` engine). `rotor-evolve` doesn't
  exist yet, so nothing needs wiring into it now, but the module should be structured for sharing
  from the start (its own file, not code inlined directly into `rotor_curate.rs`), not something
  to retrofit later. Unlike `pgn_replay.rs`, this isn't forced by CLAUDE.md rule 1 (the polyglot
  parser is entirely hand-written, no chess-crate dependency, so it could technically live in the
  `rotor` lib crate without any rule-1 tension) -- the reason is purely architectural: keeping the
  shipped engine's own module surface (`board`, `eval`, `material`, `movegen`, `perft`, `phase`,
  `search`, `tablebase`, `tt`, `uci`) limited to what the engine itself actually uses, matching
  point 2 in Status above.
- **Crate vs. hand-written, settled**: a real crate exists (`polyglot-book-rs`, docs.rs/
  crates.io, source at github.com/hedgeg0d/polyglot-rs) with exactly the right shape
  (`PolyglotBook::load`, `get_all_moves_from_fen(fen) -> Vec<PolyglotEntry { weight: u16, ...
  }>`, so a caller does its own weighted draw rather than being stuck with the crate's own "best
  move" pick). Rejected: one published version (0.1.0), ~1,900 downloads, single contributor, no
  track record -- a silently-wrong hash or move decode wouldn't crash anything, it would just
  quietly feed self-play games a subtly wrong opening move. The format itself (fixed 16-byte
  records, sorted by an 8-byte hash, found by binary search) is small enough that hand-writing it
  is comparably little work, and comes with our own verifiable tests instead of trusting an
  unaudited crate. No new `Cargo.toml` dependency at all, therefore -- `rand`/`rand_distr` are
  already unconditional dependencies (not gated behind `training`), which is everything this
  needs beyond the hand-written parser itself.
- **The polyglot format, researched, not recalled (rule 2)**: a `.bin` book is a big-endian
  array of 16-byte records (8-byte hash, 2-byte move, 2-byte weight, 4-byte unused "learn"
  field), sorted ascending by hash, looked up via binary search (multiple consecutive records
  can share the same hash -- every legal book move at that position). The hash is polyglot's
  *own* Zobrist-style scheme: a fixed, published table of 781 pseudorandom 64-bit constants (64
  per piece-per-square x 12 piece/color combos, 16 for castling rights, 8 for the en passant
  file, 1 for side to move), XORed together for whichever of those facts are true of the
  position -- unrelated to Rotor's own internal `Board::hash()`. The move encoding packs
  from-square/to-square/promotion into 16 bits with one well-known quirk: castling is encoded as
  "king captures its own rook" (e.g. e1h1 for White kingside), not the destination square the
  king actually ends up on -- a common source of bugs in naive implementations. None of these
  specifics are being taken on faith here -- fetching the real spec (or a well-regarded reference
  implementation's own worked examples/test vectors) is step 1 of the plan below, not assumed
  now.
- **Existing pieces to build on**: `rotor::board::Board::from_fen`/`to_fen` and its bitboard/
  mailbox accessors for computing the polyglot hash, `rand`/`rand_distr` (already a plain
  dependency), the TT's own `pack_move`/`unpack_move` in `tt.rs` as a precedent for "a small,
  well-tested, hand-written binary encode/decode" in this exact codebase (not reused directly --
  polyglot's move bits are a different, incompatible encoding -- but the same kind of thing, done
  once already, done well), and `pgn_replay.rs` as the direct precedent for this module's own
  placement and `#[path]`-inclusion shape.

## Settled (from discussion with the developer)

- **Selection uses raw frequency (`weight`) only.** Never any win/loss/draw-rate annotation,
  even if some derived book variant happens to carry one. A weighted random draw across every
  book entry at a position, proportional to `weight` -- not always the highest-weight move.
- **Weight-0 entries**: open point, see below -- common tool convention treats `weight: 0` as
  "don't actually play this," but the format itself doesn't enforce that; needs a real decision
  once there's a real book file to check against.
- **RNG is seeded**, not OS entropy.
- **Open again**: the ply-gated, per-ply "10% chance of calling a real search instead of the
  book, starting at ply 8" override was originally motivated entirely by self-play training-data
  diversity. Whether it still belongs, and whether `rotor-evolve`'s own use of self-play wants the
  same policy or a different one, hasn't been revisited since the scope moved off "dataset
  curation only" -- still open, see below.

## Plan

1. **Research the real polyglot hash table and move encoding from an authoritative source**
   (the original polyglot documentation/reference implementation, or a well-regarded engine's
   own real source, e.g. Stockfish's or another established open engine's book-probing code) --
   fetched, not recalled (rule 2). Needs: the exact 781-constant random table (or a documented,
   reproducible way to derive it), the exact XOR-combination rule (which facts contribute which
   constants), and worked example hash values (starting position's own polyglot hash is commonly
   cited in the spec -- to be confirmed against a real source, not assumed) to serve as golden
   test values.
2. **`polyglot_hash(&Board) -> u64`** -- computed directly from `Board`'s existing accessors
   (piece placement, castling rights, en passant file *only when actually capturable*, per the
   real spec -- a common subtlety worth confirming in step 1, side to move). Tested against the
   golden values from step 1, not just internal self-consistency.
3. **Record parsing**: a `PolyglotEntry` (hash, decoded move, weight) and a function reading one
   16-byte big-endian record. Tested against hand-built byte arrays (the same style `tt.rs`'s own
   `pack_entry_data_round_trips_*` tests already use), not a real file.
4. **Move decoding**: polyglot's 16-bit move -> Rotor's own `Move`, including the castling
   special case, which needs the `Board` for context (to recognize "from == this side's own king
   start square, to == this side's own rook start square" as the castle signal, then produce
   Rotor's own `Move::castle(...)`). Tested against hand-built cases for every encoding shape
   (quiet, promotion, en passant, both castle sides).
5. **`PolyglotBook`** (new shared, `#[path]`-included file, mirroring `pgn_replay.rs`):
   `load(path) -> io::Result<PolyglotBook>` (parses the whole file into an in-memory,
   already-sorted `Vec<PolyglotEntry>` -- simple first, per rule 5 measure-before-optimizing; a
   memory-mapped/lazy version is a later concern only if a real book's size ever makes eager
   parsing actually a problem) and `moves_at(&self, hash: u64) -> &[PolyglotEntry]` (binary
   search to the matching run of same-hash records, empty slice if none).
6. **Weighted selection**: a function drawing one `PolyglotEntry` from a slice, proportional to
   `weight`, via `rand`/`rand_distr` (already a dependency -- likely `WeightedIndex`). Tested with
   a seeded RNG against hand-built weight distributions (both a rough frequency check over many
   draws, and the weight-0 exclusion decision once settled).
7. **The ply-gated selection policy**, once its own open point (below) is resolved: the small
   orchestration function combining "does the book have an entry here" with whatever override
   rule is settled on, calling out to a caller-supplied fallback (a closure, not a direct search
   call) rather than owning a `TranspositionTable`/threading dependency itself.

Wiring this into an actual game loop (`rotor-curate`'s own self-play mode, and later
`rotor-evolve`) is separate, later work -- not part of this plan.

## Open design points

- **A. Weight-0 entries** -- exclude from the weighted draw entirely (common tool convention:
  "don't play this"), or include at their own vanishingly small probability (the format itself
  doesn't mandate exclusion)? Deferred until there's a real book file to inspect for how often
  this actually appears and what it seems to mean in practice.
- **B. In-memory vs. lazy/mmap book loading** -- unmeasured; starting with eager, whole-file
  parsing into a `Vec` per rule 5, revisited only if a real book's size makes that a real problem.
- **C. Multiple simultaneously loaded books** -- not discussed yet; assumed one book per run for
  now, since nothing so far has suggested a need for more.
- **D. The ply-8/10%-real-search override's fate** -- still open; needs re-discussion now that
  the scope includes `rotor-evolve` as a second, not-yet-designed consumer alongside
  `rotor-curate`, which may or may not want the identical policy.

## Progress

Steps 1-6 of the plan are implemented in `src/bin/polyglot_book.rs`, `#[path]`-included into
`rotor_curate.rs` alongside `pgn_replay.rs`. `cargo test` passes, including the golden
`polyglot_hash` values (starting position, after 1.e4, the en passant case, the castling-rights
case), the record parser, move decoding (quiet, castling, promotion, no-legal-match), weighted
selection, and the ply-gated `select_move` policy.

Settled along the way:

- **Weight-0 entries (open point A)**: excluded, matching python-chess's own reference reader
  default (`minimum_weight: int = 1`).
- `select_move`'s policy uses `MIN_PLY = 8` and `SKIP_BOOK_PROBABILITY = 0.10` as named constants,
  checked before consulting the book at each ply (memoryless, no "left the book" state), calling
  a caller-supplied `search_fallback` closure rather than owning any search dependency itself.

Still not started: the real self-play/dataset-generation loop (`rotor-curate`'s own self-play
mode, later `rotor-evolve`) -- deliberately out of scope for this plan, per the developer's own
instruction. Open design points B, C, D remain open, to be revisited when that game loop is
designed.

A minimal manual test driver was added on top, purely to actually watch a real book being
followed/departed from in a real game -- `play_self_play_game` and the `rotor-curate self-play
[book.bin] [seed]` entry point in `rotor_curate.rs`. It plays one game from the start position,
choosing every move via `select_move` with `Search(SEARCH_DEPTH)` as the fallback, and prints
each move plus the end reason (checkmate, stalemate, fifty-move rule, threefold repetition, or
the `SELF_PLAY_MAX_PLIES` safety cap). It stores nothing and has no threading of its own -- not
the real self-play loop above, just enough to see the book in action.
