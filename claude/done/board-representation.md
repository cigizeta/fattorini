# Board representation

## Design

Rotor's board representation follows the architecture already locked in by CLAUDE.md:
bitboards as the primary representation, plus a mailbox array for O(1) piece lookup,
the two kept in sync on every move. This document is the design record for that work,
built up decision by decision.

### Square

A square is a `#[repr(u8)]` enum with all 64 variants named explicitly (`A1`, `B1`, ...,
`H8`), rather than a bare `u8` or a newtype wrapping one:

- Numbering follows LERF (Little-Endian Rank-File): `A1 = 0`, `B1 = 1`, ..., `H1 = 7`,
  `A2 = 8`, ..., `H8 = 63`. This is the near-universal convention in chess engines and in
  published magic bitboard tables, so adopting it now avoids a remap later.
- `#[repr(u8)]` pins the in-memory representation to exactly one byte, the same size as a
  raw `u8` -- the type safety costs nothing at runtime.
- Rejected alternative: a newtype `Square(u8)`. A newtype can wrap any `u8` for free
  (including invalid values like 200), whereas the 64-variant enum makes it impossible,
  in safe code, for a `Square` to ever hold anything but one of the 64 real squares. That
  guarantee, enforced by the compiler rather than by convention, was judged worth the
  small cost of writing an explicit conversion function (below).

Conversion between `Square` and `u8`:

- `Square` -> `u8` is free and built into the language (`square as u8`), since casting a
  fieldless enum down to its `#[repr(u8)]` discriminant needs no extra code.
- `u8` -> `Square` is not automatic, because the compiler cannot verify an arbitrary `u8`
  (which can be 0..255) falls in the 64-value range a `Square` allows. This direction is
  handled by a checked conversion function that validates the range and reports failure
  explicitly (e.g. returning `Option<Square>`) rather than by `unsafe` transmutation.
  `unsafe` was considered and deliberately deferred: CLAUDE.md rule 5 requires
  profiling before any optimization, and there is no measurement yet showing the checked
  path costs anything meaningful anywhere it's used -- including in bitboard-scanning
  code, where the bit index is mathematically guaranteed in range by construction, so
  even there the check simply never takes its failure branch today.

### File and Rank

Separate `#[repr(u8)]` enums, `File` (`A`..`H`, 0..7) and `Rank` (`One`..`Eight`, 0..7),
rather than plain arithmetic on a `Square`. Reasoning is the same as for `Square` itself:
the compile-time guarantee (a `File` can never hold an invalid value, and a `File` can't
be passed where a `Rank` is expected) costs nothing at runtime, and the risk it guards
against -- swapping file and rank at a call site -- is a realistic mistake in exactly the
FEN/algebraic-notation parsing code this representation exists to support. The "arithmetic
only, no dedicated type" alternative was rejected as an inconsistent application of the
reasoning already accepted for `Square`.

### Color, PieceType, and Piece

Three enums, all `#[repr(u8)]`, all using Rust's default sequential numbering (no explicit
discriminant values needed anywhere):

- `Color { White, Black }` -- `White = 0`, `Black = 1`.
- `PieceType { Pawn, Knight, Bishop, Rook, Queen, King }` -- `Pawn = 0` through `King = 5`.
  Unlike Stockfish's `PieceType`, nothing is reserved at 0 for a sentinel, since absence is
  handled by `Option`, not by a special value inside the enum's own range (see below).
- `Piece`, a single combined enum listing all six white pieces then all six black pieces
  (`WhitePawn, ..., WhiteKing, BlackPawn, ..., BlackKing`), giving values 0..11 with no
  gaps. Listing the variants in that order makes the numbering exactly
  `color * 6 + piece_type` automatically, since that's Rust's default enum numbering --
  no explicit `= N` annotations required, unlike Stockfish's gapped `color * 8 +
  piece_type` scheme, which needs them.

Two alternatives to `Piece`'s numbering were weighed:

- Stockfish's own scheme, `piece_type + color * 8` (with `piece_type` 1..6, 0 reserved for
  `NO_PIECE`), chosen there so that color and type can be recovered with pure bitmasks
  (`piece & 7`, `piece & 8`) -- important in C++, which has no equivalently idiomatic
  alternative. Rejected here: the dense `color * 6 + piece_type` scheme needs no explicit
  discriminants, wastes no array slots, and its extraction is no more expensive --
  division/modulo by the compile-time constant 6 compiles to a multiply-and-shift, not a
  real division instruction, and CLAUDE.md rule 5 rules out choosing a scheme on an
  unmeasured "should be faster" basis regardless. Extraction can equally well be done via
  a `match` over the named variants, which needs no arithmetic properties from the
  numbering at all.
- A `NO_PIECE` sentinel variant inside `Piece` itself, matching Stockfish. Rejected in
  favor of `Option<Piece>`: a sentinel is a workaround for C++ lacking a built-in
  "possibly absent" type, and reproducing it in Rust would give up, for no benefit, the
  compiler's guarantee that an empty square can never be mistaken for a real piece.
  `Option<Piece>` costs nothing extra here -- Rust's niche-filling optimization reuses one
  of `Piece`'s unused `u8` bit patterns to represent `None`, so `Option<Piece>` is still
  exactly one byte, identical in size to `Piece` alone.

### FEN scope: what Board carries

A FEN string is six space-separated fields: piece placement, side to move, castling
rights, en passant target square, halfmove clock, and fullmove number. `Board` carries
the first five. Reasoning: fields 2-5 are not derivable from a single position snapshot
(castling rights and the en passant square specifically depend on move history, not just
current geometry) and are read and updated by search on every move made, exactly like
piece placement -- so they belong on the same struct that's threaded through move-making,
not treated as separate FEN-parsing metadata. The fullmove number is the one field with
no effect on legal moves, draw detection, or search decisions; search never reads it, so
it does not live on `Board`. It's carried as a plain value wherever FEN parsing/printing
and UCI-facing code needs it -- not wrapped together with `Board` in a `Position`-style
type, since the two aren't used together at any single call site: `Board` travels through
search continuously, the fullmove number is only ever touched at the UCI/FEN boundary. A
wrapper wouldn't have cost anything extra at runtime (Rust doesn't add overhead for
grouping two values into a struct), so the reason to skip it is that the two values don't
represent one concept, not that a wrapper would be expensive.

Castling rights, once implemented, will need their own small type -- not an enum, since
the four rights (White/Black x kingside/queenside) are independent flags that can be true
or false in any combination simultaneously, not one-of-N mutually exclusive choices.

### Mailbox: synced, not derived on demand

`[Option<Piece>; 64]`, kept in sync with the 12 piece bitboards on every move, per
CLAUDE.md rule 6. Unlike occupancy, this isn't a free choice being made here -- it's
already locked in by that rule, and for a reason baked into the mailbox's own purpose,
not just a speed preference: the mailbox exists specifically to answer "what's on this
one square" in O(1), a single array read. Deriving that answer on demand instead would
mean scanning up to 12 bitboards checking each one's bit for that square -- exactly the
cost the mailbox exists to avoid. A "derived on demand" mailbox wouldn't be a slower
version of the same thing; it would fail to serve the purpose that justifies having one
at all. So, unlike occupancy, there's no working "derive on demand" alternative to weigh
here.

### Occupancy: derived on demand, not cached

Combined occupancy (all white pieces / all black pieces / all pieces) is not stored on
`Board`. Whenever it's needed, it's computed on the spot by OR-ing together the relevant
piece bitboards -- nothing is remembered between calls, so there's no extra state and
nothing to keep in sync.

This is a genuine cache: a stored occupancy bitboard would contain no information not
already fully implied by the 12 piece bitboards (the same relationship the mailbox has to
those bitboards, which is why the mailbox is a cache too, even though CLAUDE.md rule 6
doesn't use that word for it). The case for caching occupancy anyway is structural, not
a guess: occupancy will be *read* far more often than the pieces it's derived from
*change*, since a single move made during search triggers many sliding-piece move-generation
calls beneath it in the search tree, all wanting occupancy, while the underlying position
changes only once. That's the same lopsided read/write ratio that already justified the
mailbox in rule 6 without needing a profiler first.

Decided against caching regardless, because unlike the mailbox this isn't already locked
in by CLAUDE.md -- it's a free choice, so rule 5 (no optimization without measurement)
applies to it directly. Deriving on demand is the simpler, zero-invariant option, and it
is a pure implementation-detail change to switch to a cached field later if profiling
against real move generation shows it's worth the synchronization burden; nothing about
its callers would need to change.

### FEN parsing and printing

`Board::from_fen(&str) -> Result<(Board, u32), FenError>` and `Board::to_fen(&self,
fullmove_number: u32) -> String`, living in their own file, `board/fen.rs`, as an `impl
Board` block plus the `FenError` type -- kept out of `board/mod.rs` since FEN
serialization is a distinct concern from `Board`'s own definition, the same reasoning
that already put `CastlingRights` and `Bitboard` in their own files.

`Board` doesn't store the fullmove number (see "FEN scope" above), but a real FEN string
always carries all 6 fields, so the boundary functions deal with it without `Board`
itself needing to: `from_fen` returns it alongside the parsed `Board` as a plain `(Board,
u32)` pair, and `to_fen` takes it as a parameter to reassemble the full 6-field string. A
named wrapper struct for that pair was considered and rejected -- it would just be the
`Position`-style wrapper already rejected under "FEN scope", renamed.

Errors are a dedicated `FenError` enum, one variant per field that failed to parse
(`WrongFieldCount`, `InvalidPiecePlacement`, `InvalidSideToMove`, `InvalidCastlingRights`,
`InvalidEnPassantSquare`, `InvalidHalfmoveClock`, `InvalidFullmoveNumber`), so a future
caller (the UCI `position` command) can match on exactly what went wrong rather than only
displaying a message.

Three small additions to already-existing types, each needed by this parsing/printing
code specifically, not added speculatively:

- `Square::new(file: File, rank: Rank) -> Square` and `.file()`/`.rank()` accessors,
  mirroring `Piece::new`/`.color()`/`.piece_type()` exactly: `rank * 8 + file` arithmetic
  plus `TryFromPrimitive::unwrap()`, justified the same way (the range is always valid by
  construction). Needed to build squares while walking the piece-placement field rank by
  rank/file by file, and to decompose the en passant square back into file+rank
  characters when printing.
- `Bitboard::EMPTY` (the all-zero bitboard, to seed the 12-element piece array before
  OR-ing parsed pieces in) and `Bitboard::from_square(Square) -> Bitboard` (a single set
  bit, built only through a `Square`, never through a raw shifted `u64`).
- `CastlingRights::NONE` (all four flags clear), the same "start empty, then set flags
  one at a time" need as `Bitboard::EMPTY` -- the castling field is built up flag by flag
  from `-` or a subset of `KQkq`.
- `File::from_char`/`.to_char()` and `Rank::from_char`/`.to_char()`, added as public
  methods on `File`/`Rank` themselves rather than kept as private helpers inside
  `fen.rs`. Unlike the earlier "leave `File`/`Rank` bare until needed" default, this
  follows the same reasoning that turned around `Bitboard`'s operators: UCI's algebraic
  square notation (parsing `e2e4`-style moves, printing `bestmove`) is a near-certain
  second caller for exactly this conversion, not a speculative one.

Piece letter <-> `char` conversion (`P`/`p`, `N`/`n`, etc., case encoding color) stays a
private, unexported detail inside `fen.rs` rather than becoming a `Piece` method: unlike
`File`/`Rank`'s plain letter/digit mapping, this specific case-encodes-color scheme is
FEN's own convention, not one `Piece` itself or other future callers (UCI promotion
characters are a single, uncased letter with no color, a different mapping entirely) are
expected to reuse.

`to_fen`'s piece-placement output is read from the **mailbox**, not the piece bitboards
-- consistent with the mailbox existing specifically to answer "what's on this square" in
O(1), per the "Mailbox: synced, not derived on demand" section above.

Parsing is structural, not a full legality check: it validates that each field is
well-formed (right shape, valid characters, in-range numbers) but not chess-legality
properties a FEN could still violate (e.g. two kings of the same color, an en passant
square whose rank doesn't match the side to move) -- those are move-generation/legality
concerns, out of scope for `Board`'s own data representation. Two specific permissive
choices, called out so they don't look like oversights: a castling field with a flag
repeated (e.g. `"KK"`) is accepted (setting the same bit twice is harmless, not
malformed), and an empty-square run split across consecutive digits (e.g. `"44"` for a
fully empty rank) is accepted as long as the totals per rank still sum to exactly 8,
rather than being rejected for not using a single digit.

## Decisions summary

- `Square`: 64-variant `#[repr(u8)]` enum, LERF numbering, checked `u8` -> `Square`
  conversion.
- `File`, `Rank`: dedicated `#[repr(u8)]` enums (8 variants each), not raw arithmetic.
- `Color`: `{ White, Black }`. `PieceType`: `{ Pawn, Knight, Bishop, Rook, Queen, King }`,
  0-indexed.
- `Piece`: 12-variant enum, white pieces then black pieces, giving `color * 6 +
  piece_type` numbering for free. Absence is `Option<Piece>`, not a sentinel variant.
- `Board` carries FEN fields 1-5 (placement, side to move, castling rights, en passant,
  halfmove clock); the fullmove number (field 6) lives outside `Board`, wherever
  FEN/UCI-facing code needs it.
- Occupancy bitboards: derived on demand, not cached.
- `Bitboard`: newtype over `u64`, with `BitAnd`/`BitOr`/`BitXor`/`Not`/`Shl<u32>`/`Shr<u32>`
  implemented now rather than deferred, since move generation's need for exactly these six
  operations is already known, not speculative.
- `CastlingRights`: `CastlingRights(u8)` bitset (low 4 bits only, `xxxxBBBB`), hand-rolled
  rather than the `bitflags` crate, bit order matching FEN's own `KQkq` field order (bit 0
  = White kingside .. bit 3 = Black queenside). Public API is per-side query/set methods
  taking `Color` (`.kingside(color)`, `.queenside(color)`, `.set_kingside(color, value)`,
  `.set_queenside(color, value)`), plus a `NONE` constant (all flags clear).
- FEN: `Board::from_fen(&str) -> Result<(Board, u32), FenError>` and
  `Board::to_fen(&self, fullmove_number: u32) -> String`, living in their own `fen.rs`.
  Fullmove number travels as a plain value alongside `Board`, never inside it. Errors are
  a dedicated `FenError` enum (one variant per field). Parsing is structural only, not a
  legality check (see "FEN parsing and printing" above for the full reasoning and the two
  deliberately permissive edge cases).

## Implementation progress

All eight type files exist on the `board` branch, one type per file (except `Board`
itself, which lives directly in `mod.rs` as the module's hub type): `square.rs`,
`file.rs`, `rank.rs`, `color.rs`, `piece_type.rs`, `piece.rs`, `bitboard.rs`,
`castling_rights.rs`, plus `fen.rs` for FEN parsing/printing.

Done:
- `Square`: enum, checked `u8 -> Square` conversion via `TryFromPrimitive`, plus
  `new(file, rank)`/`.file()`/`.rank()` (mirroring `Piece::new`/`.color()`/`.piece_type()`
  exactly). Five tests: numbering spot-check, exhaustive 0..64 round-trip, out-of-range
  rejection, and the file/rank round-trip.
- `Color` and `PieceType`: plain enums with `TryFromPrimitive`.
- `Piece`: `new(color, piece_type)`, `.color()`, `.piece_type()` via `color * 6 +
  piece_type` arithmetic plus `TryFromPrimitive`/`.unwrap()` (justified by construction,
  not an unchecked assumption). Round-trip test covers all 12 `Color` x `PieceType`
  combinations.
- `File`, `Rank`: `TryFromPrimitive` plus `.from_char()`/`.to_char()` (`'a'..'h'`,
  `'1'..'8'`), each with a round-trip test and an invalid-input rejection test.
- `Bitboard`: newtype over `u64`; `BitAnd`/`BitOr`/`BitXor`/`Not`/`Shl<u32>`/`Shr<u32>`,
  plus `EMPTY` and `from_square(Square)`. One test per operator/constructor.
- `CastlingRights`: `CastlingRights(u8)` bitset, `NONE`, `.kingside()`/`.queenside()`/
  `.set_kingside()`/`.set_queenside()`. Tests: bit-layout-matches-FEN-order, flag
  independence, set/clear round-trip.
- `Board`: struct defined with all agreed fields (12 piece bitboards, mailbox, side to
  move, castling rights, en passant square, halfmove clock). No derives, no constructor
  outside `from_fen` yet -- not needed by anything built so far.
- `fen.rs`: `FenError` (7 variants), `Board::from_fen`/`Board::to_fen`, plus private
  piece-letter <-> `char` helpers. 10 tests: starting-position round-trip, a second
  round-trip exercising en passant + no castling rights, and one rejection test per
  `FenError` variant.
- Explicit `return` on every value-returning function throughout the codebase (not just
  this module) -- now a project-wide convention, recorded as its own rule in the
  top-level CLAUDE.md, not specific to board representation.
- `.vscode/settings.json`: disabled inlay hints and sticky scroll (personal editor
  preference, not committed -- `.vscode/` is gitignored).
- `cargo test`: 31 tests, all passing.

Nothing left pending for this increment. The milestone the whole board-representation
work was scoped around -- parsing a FEN and printing it back out unchanged -- is done and
tested, for both the starting position and a position with en passant set and no castling
rights. What's left (occupancy-deriving helpers, move generation, `Board` derives like
`Clone`/`Debug`, castling-rights updates during move-making) belongs to future work, not
this document -- each is deferred until something concrete needs it, per the reasoning
already recorded above (see "Occupancy", and the `Bitboard`/`File`/`Rank` entries in "FEN
parsing and printing").

## Progress log

- 2026-09-09: `Square` design agreed (64-variant `#[repr(u8)]` enum, LERF numbering,
  checked `u8` -> `Square` conversion, no `unsafe`). `File`, `Rank`, `Color`,
  `PieceType`, and `Piece` designs agreed as described above. FEN scope settled (`Board`
  carries fields 1-5; fullmove number lives outside it). Occupancy settled as derived on
  demand, not cached. All open design questions for this increment are now resolved --
  see Decisions summary. Nothing implemented in code yet; next step is writing the
  enums, on the `board` branch.
- 2026-09-10: `Piece` round-trip test added; `cargo test` reconfirmed passing. `Bitboard`
  designed and implemented (newtype + all six operators, added now rather than deferred
  since move generation's need for them is already known). `CastlingRights` designed
  (bitset, FEN-order bits, per-side accessor methods) and implemented. `Board` struct
  written with all five agreed fields. FEN parsing/printing designed and implemented in
  `fen.rs`, including three small supporting additions to existing types
  (`Square::new`/`.file()`/`.rank()`, `Bitboard::EMPTY`/`.from_square()`,
  `File`/`Rank::from_char`/`.to_char()`, `CastlingRights::NONE`). All 31 tests pass. A
  project-wide "explicit `return`" convention was agreed during this session and applied
  throughout the codebase (recorded in the top-level CLAUDE.md, not here, since it's not
  board-representation-specific). The board-representation work as originally scoped is
  complete.
