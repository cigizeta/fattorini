# Move generation

## Design

Move generation produces, for a given `Board`, the moves the side to move may make. Like
every chess engine, this is split into two phases, built as separate increments:

1. **Pseudo-legal generation**: moves that obey each piece's own movement rules, without
   checking whether making the move would leave the mover's own king in check.
2. **Legality filtering**: removes any pseudo-legal move that leaves the king in check
   (e.g. moving a pinned piece off its pin). A later increment -- not started yet.

This document covers the first slice: the `Move` type itself, plus pseudo-legal
generation for knight and king moves specifically. Knight and king were chosen to start
with because neither needs sliding-attack machinery (magic bitboards) or any of a pawn's
four special cases (single push, double push, capture, en passant, promotion) -- both
have a fixed, board-position-independent set of destination squares per origin square
(modulo board edges), the simplest possible case to get the overall shape right on.
Pawns, sliding pieces (bishop/rook/queen, needing magic bitboards per CLAUDE.md's locked
architecture), castling, and check/pin legality are later increments.

### Move representation

`Move` is a struct with private fields and public accessor/constructor methods -- no
public field access, no field-exposing derives beyond what's needed for equality/debug
printing. This is deliberate, not incidental: chess engines conventionally pack a move
into a compact bitfield encoding (e.g. a 16-bit integer) because moves are generated and
copied by the million during search, but CLAUDE.md rule 5 rules out adopting that on an
unmeasured "should be faster" basis, and nothing has been profiled yet since search
doesn't exist. Keeping the fields private and access method-only means the packed-encoding
question stays genuinely open for later: switching the internal representation is then a
self-contained change inside `move.rs`, invisible to every caller (move generation,
make/unmake, search, UCI printing), exactly the same reasoning already used for
`Board`'s occupancy bitboards (derived on demand now, a pure implementation-detail change
to cache later if profiling justifies it).

Fields, kept deliberately minimal for this increment rather than shaped ahead for pawns'
near-certain future needs (an explicit choice, not the default -- see decisions below):

```rust
pub struct Move {
    from: Square,
    to: Square,
    promotion: Option<PieceType>,
}
```

- `promotion` is always `None` for every move this increment generates (knights and
  kings never promote -- only pawns do), but the field exists on `Move` from the start
  rather than being bolted on later, since promotion is a property of the destination
  square/piece pair, not of which piece type is moving.
- No `kind`/"special move" field yet (no `Capture`, `DoublePawnPush`, `EnPassantCapture`,
  `CastleKingside`/`CastleQueenside` distinction). Rejected for now: pawns (the next
  increment) will need exactly this, which made it tempting to design the field now by
  the same "near-certain second caller" reasoning that justified adding `Bitboard`'s
  operators up front rather than deferring them. Decided against it here specifically
  because, unlike `Bitboard`'s six operators (already fully known, small, and settled),
  the *shape* of that future field isn't settled -- e.g. whether promotion belongs inside
  `MoveKind` or stays a separate field, exactly which variants castling needs -- so
  designing it now risks guessing wrong and reshaping it anyway once pawns are actually
  built with full context. Knight/king moves don't need it at all: "is this a capture"
  can be read directly off `Board`'s mailbox at generation time, needing no storage on
  `Move` itself for this increment's purposes.
- `Move` derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq` -- needed to assert on
  generated move lists/sets in tests (rule 4) and to pass `Move` around by value cheaply,
  not because anything about the derives commits to a particular internal layout.

### Knight and king attack generation

Both are non-sliding: the set of squares attacked from a given square is a small, fixed
list of candidate offsets from that square (knight: 8 L-shaped offsets, `(±1,±2)`/
`(±2,±1)`; king: 8 one-step offsets, `(±1,0)`/`(0,±1)`/`(±1,±1)`), filtered against board
edges -- it never depends on the rest of the board's occupancy, only on which squares
exist.

Each is computed once, lazily, and cached forever, via `std::sync::LazyLock<[Bitboard;
64]>` -- not a true compile-time `const` table (`Bitboard`'s `BitOr` is a trait method,
and trait methods generally can't be called inside a `const fn` on stable Rust, so a real
compile-time table would need a second, `const`-fn-callable way to OR two `Bitboard`s
together just for this), and not recomputed from scratch on every call either. A private
`compute_*_attacks(square)` function holds the actual offset-checking arithmetic; the
`LazyLock` builds the full 64-entry table from it on first access; the public
`knight_attacks(square)`/`king_attacks(square)` functions just index into the cached
table.

This is a deliberate departure from the "correctness first, don't optimize without
measurement" stance used elsewhere (`Board`'s occupancy, `Move`'s packed-encoding
question) -- reached by explicit discussion, not by default, because the two reasons
that justified deferring optimization in those other cases don't hold here:

- Occupancy was rejected as a cached field specifically because caching it would create
  an *ongoing synchronization burden* against 12 mutable piece bitboards, updated on
  every move -- a real, permanent correctness-risk cost.
- `Move`'s packed encoding is deferred because whether it actually helps depends on
  genuinely uncertain factors (memory bandwidth, cache behavior under real search load)
  that can't be known without profiling -- it might not even help.

Neither applies to a knight/king attack table: it's a **pure function** over a **small,
fully-enumerable, unchanging domain** (64 squares; chessboard geometry never changes
while the program runs), certain to be called with the same 64 inputs repeatedly for as
long as the engine runs. Caching it isn't a bet on an uncertain payoff the way the other
two are -- there's no realistic scenario where computing each of the 64 values once and
reusing them is worse than recomputing on every call. The only real cost is the added
`LazyLock` machinery itself, not a risk of the optimization being premature or wrong.

### Board::occupancy and Board::pieces_of

Two small accessors added to `Board` (directly in `mod.rs`, alongside the struct, not a
separate file -- both are simple reads of `Board`'s own fields, not a distinct concern the
way FEN parsing is): `pieces_of(&self, piece: Piece) -> Bitboard` (a raw accessor, just
`self.pieces[piece as usize]`) and `occupancy(&self, color: Color) -> Bitboard` (every
square held by any of `color`'s pieces, OR-ing the 6 relevant piece bitboards together --
the actual implementation of the "derived on demand, not cached" design already recorded
on `pieces` itself).

`occupancy` loops the 6 `PieceType` variants and calls `Piece::new(color, piece_type)` for
each, rather than directly indexing `pieces[color * 6 .. color * 6 + 6]` via the same
arithmetic `Piece::new` uses internally. The two are provably equivalent given `Piece`'s
and `PieceType`'s current definitions (not a coincidence -- `PieceType`'s declared order
already puts `Pawn` through `King` at discriminants 0-5, so looping through `Piece::new`
computes exactly the same 6 indices the direct-arithmetic version would write by hand),
and a brief discussion of whether direct arithmetic might be measurably faster concluded
it very likely isn't, once inlining and dead-branch elimination happen in a release
build -- and, per rule 5, that it wouldn't be worth building and maintaining both versions
on that unmeasured a hunch anyway. The `Piece::new` version stays the only one, chosen
because it never needs to change if `Piece`'s numbering scheme ever did.

### Move extension for pawns

Pawns are the next increment after knight/king (not yet started as of this section --
design only, resuming here). They need `Move` to grow, which is exactly the extension
point deliberately left open above: a `kind: MoveKind` field, plus two new dedicated
constructors, added alongside (not replacing) the existing ones.

Five pawn move cases, and what each needs: single push and diagonal capture need nothing
beyond what `Move` already had (`from`/`to`, `Move::new`); promotion already has its
field. Double push and en passant capture are the two that forced this extension --
both are facts about *how* a move happens that `from`/`to` alone don't capture, and
critically, both are facts about the move itself (derivable from which piece is moving
plus its `from`/`to` squares), never about the position it came from.

```rust
pub struct Move {
    from: Square,
    to: Square,
    promotion: Option<PieceType>,
    kind: MoveKind,
}

pub enum MoveKind {
    Quiet,
    DoublePawnPush,
    EnPassantCapture,
}
```

Kept deliberately narrow -- two things this is *not*:

- **No `Capture` variant.** An ordinary capture isn't a move-intrinsic fact the way a
  double push or en passant is -- whether the destination square holds an enemy piece is
  discoverable by reading `Board` at the time (generation or make time), not something
  that needs storing on the move. Knight/king move generation already works this way
  (no capture flag anywhere), and pawns don't need to be treated differently for
  ordinary captures either.
- **No captured-piece identity, no prior en passant square, no prior castling rights.**
  This was explicitly discussed and rejected: `Move` is a value type -- every field is a
  plain fact about the move itself, decided once at construction, never something that
  depends on the board it came from or needs the board consulted again to make sense of.
  What a capture removed, or what the position looked like *before* this move, are facts
  about the *position*, not the move -- `Move` can't encode "what the board looked like
  before" no matter how it's shaped, since that information isn't the move's to carry.
  Restoring those on unmake will need a separate history/undo record later (real engines
  almost universally have one, often called something like `StateInfo`), entirely
  independent of `Move`'s own shape -- not something this increment needs to design.

Construction: `Move::new(from, to, promotion)` is unchanged (defaults `kind` to
`MoveKind::Quiet` internally), so `knight_moves`/`king_moves` and their tests need no
changes. Two new constructors, neither taking a `promotion` parameter (a pawn that just
left its own starting rank, or that just captured en passant, can never simultaneously
reach the last rank): `Move::double_pawn_push(from, to)` and
`Move::en_passant_capture(from, to)`.

### Square::try_offset -- shared by knight, king, and pawn

Knight and king each had their own inline "check an offset against board edges, then
build the target square" logic. Pawns need the same core pattern a third time (a single
directed offset, rather than a fixed list) -- a strong enough, non-speculative signal
(three real call sites, not a guess) to extract it as `Square::try_offset(self,
file_delta: i8, rank_delta: i8) -> Option<Square>`, a general-purpose `Square` method,
not movegen-specific. `compute_knight_attacks`/`compute_king_attacks` were refactored to
call it instead of duplicating the bounds-check-and-construct logic inline -- a pure
dedup, no behavior change, existing tests unchanged.

### Board additions for pawns

Two more small `Board` accessors, needed for the same reason `pieces_of`/`occupancy`
were: pawn generation needs to read state `Board` already stores but didn't expose yet.

- `Board::en_passant_square(&self) -> Option<Square>` -- a plain reader for the field
  that already existed.
- `Board::all_occupancy(&self) -> Bitboard` -- every occupied square regardless of
  color (`occupancy(White) | occupancy(Black)`), needed because a pawn push (unlike a
  capture) is only legal onto a square that's empty *at all*, not merely empty of the
  mover's own pieces. Added directly to `Board` rather than computed inline in
  `pawn.rs`, since sliding pieces (a near-future increment) will need the same
  combined-occupancy fact for blocker detection.

Also: `Color::opposite(self) -> Color`, needed to find the enemy occupancy for diagonal
captures -- a plain 2-arm match, added to `color.rs`.

### Pawn move generation

Implemented in `movegen/pawn.rs`. Two color-dependent helpers stay private to this file
rather than becoming `Color` methods, since no other piece's movement depends on a
"forward" direction: `forward(color) -> i8` (`+1` for White, `-1` for Black),
`starting_rank(color)` (double-push origin), `promotion_rank(color)` (last rank).

`pawn_moves(board, color)` loops `color`'s pawns (via `pieces_of` + `Bitboard::squares`,
same as knight/king) and, per pawn, calls two helpers:

- `generate_pushes`: the single-push target via `try_offset(0, forward(color))`, legal
  only if empty (`all_occupancy`, not `occupancy(color)` -- a push can't land on an
  enemy piece either); if blocked, returns immediately (a blocked single push blocks the
  double push too, so there's nothing more to check). If on `starting_rank(color)` and
  the double-push target is also empty, adds a `Move::double_pawn_push`.
- `generate_captures`: the two diagonal targets via `try_offset(±1, forward(color))`
  (either may not exist, at a board edge -- handled by `try_offset` returning `None`,
  the same as knight/king already rely on); legal as an ordinary capture if the target
  holds an enemy piece, or as `Move::en_passant_capture` if the target equals
  `Board::en_passant_square()` instead (mutually exclusive by construction: the en
  passant square is always the empty square a pawn passed over, never a currently
  occupied one).

Both funnel non-en-passant destinations through a shared `add_pawn_move` helper, which
expands into one `Move::new(from, to, Some(promotion))` per `PROMOTION_PIECES` entry
when `to` is on `promotion_rank(color)`, or a single ordinary move otherwise -- shared
between push and capture since either can reach the last rank.

Nine tests: single push off the starting rank, double push available, push blocked
directly ahead (blocks both), push blocked only two squares ahead (single push still
works), diagonal captures, en passant capture, promotion via both push and capture in
one position (8 moves total), a file-edge case confirming no wraparound, and a Black
pawn confirming the color-dependent direction logic generalizes.

### Sliding piece move generation (bishop, rook, queen)

Per CLAUDE.md rule 6 (updated during this increment): PEXT-based sliding-attack lookup
only, no magic-bitboard fallback -- this project currently targets only the developer's
own PC (confirmed BMI2/PEXT support), with portability (ARM, older x86) deliberately
deferred as a future extension, not built now. Verifying "PEXT is at least as fast as
magic bitboards" relies on well-replicated external evidence (Stockfish's own `USE_PEXT`
rationale, chess programming wiki) rather than an in-Rotor benchmark against a
magic-bitboard implementation that was never built -- a deliberate exception to
measuring everything internally, justified by the stakes being genuinely small
(microseconds) and the claim needed being the weak one ("not worse"), not "faster."

**The mechanism** (see the session's own walkthrough for the full explanation): a
sliding piece's attack set depends on board occupancy, unlike knight/king, so it can't
be one static table per square. PEXT bitboards solve this with, per square: a *relevant
occupancy mask* (every square that could block one of the piece's rays, excluding the
square itself and the far edge of each ray -- whether that exact edge square is
occupied never changes the attack set); `PEXT(occupancy, mask)` turns "which relevant
squares are occupied" into a small, dense table index with no hashing needed (unlike
classic magic bitboards, no magic-number search either); and a precomputed attack table,
one entry per possible masked-occupancy pattern, built once by walking each ray with
`PDEP(index, mask)` -- PDEP's exact inverse relationship to PEXT means it generates
every subset of a mask directly from a dense counter, with no separate bit-enumeration
trick needed.

**New, real `unsafe`.** `Bitboard::pdep`/`Bitboard::pext` wrap `std::arch::x86_64`'s
`_pdep_u64`/`_pext_u64`, which are `unsafe fn` in the standard library unconditionally --
this is structurally required to invoke the instructions at all, a different category
from the board-representation doc's earlier `unsafe` discussion (which was about
skipping a bounds check for speed, deliberately deferred). The safety obligation ("this
CPU supports BMI2") is satisfied by a new `.cargo/config.toml` setting
`target-feature=+bmi2`, compiling the whole binary on that assumption, with no runtime
capability check (per the "just for me, at this stage" decision) -- matching how a
program compiled this way would behave anyway (illegal instruction on unsupported
hardware) if a check were skipped. Both intrinsic calls stay fully contained inside
`Bitboard`'s own file, the same principle already used for the bitscan iterator: the one
type that owns the raw `u64` is the one place its bit-level tricks live.

**Shared machinery, not duplicated per piece.** Bishop and rook need identical
mask-building, table-building, and lookup logic, differing only in which 4 directions
they ray-cast along -- a real, already-known "two call sites" signal (not speculative),
so it lives once in `movegen/sliding.rs` as `SlidingAttackTable`, parameterized by a
`[(i8, i8); 4]` direction list. `movegen/bishop.rs`/`movegen/rook.rs` each just supply
their own directions, a `LazyLock<SlidingAttackTable>` built from it (same "pure
function over a small, fixed, never-changing domain" reasoning as `KNIGHT_ATTACKS`), and
thin `*_attacks`/`*_moves` wrappers identical in shape to `knight_moves`/`king_moves`.
`movegen/queen.rs` needs no attack table of its own at all: a queen's attacks are
exactly `bishop_attacks(...) | rook_attacks(...)`, since a queen simply combines both
pieces' movement.

`Bitboard::count()` (`u64::count_ones()`) was also added, needed to size each square's
attack-pattern array (`2^mask.count()` entries).

### Bitboard::squares

A bitboard's set squares are walked via the standard bitscan-and-clear-lowest-bit
technique -- `trailing_zeros()` to read the lowest set bit's position directly (a single
CPU instruction on most hardware), then `value & (value - 1)` to clear exactly that bit
(subtracting 1 flips the lowest set bit and every zero bit below it; ANDing with the
original leaves everything else unchanged), repeating until the value is zero. This is
real, near-universal practice in bitboard-based engines (Stockfish included), not a
one-off trick invented for this codebase -- and unlike the earlier `MoveKind` question,
every future piece type's move generation needs exactly this operation, not a
speculative maybe, which is why it's added now as a shared `Bitboard` method rather than
inlined per piece type or deferred.

Exposed as `Bitboard::squares(self) -> impl Iterator<Item = Square>`. The actual
bit-scanning struct (`Squares`, wrapping a plain `u64`, not a `Bitboard` -- the trick
needs raw integer subtraction, which `Bitboard` doesn't otherwise expose) stays private:
callers only ever need "something iterable over `Square`," never the concrete type, so
`impl Iterator<Item = Square>` is the public surface, keeping the internal struct free to
change later without being a public API.

### `Board::pieces_of` renamed to `Board::bitboard`

Renamed while discussing the castling design below, on the grounds that `pieces_of`
described the wrong thing: it returns a `Bitboard`, not "pieces." `bitboard(piece)` names
the return value directly, mirroring the field it reads (`self.pieces[piece as usize]`).
Pure rename, no behavior change -- every call site across `board/mod.rs` and
`movegen/*.rs` updated together. `cargo test` reconfirmed passing (69 tests) immediately
after.

### Attacked-square detection and castling

Castling turned out not to be self-contained: legality requires knowing whether the
opponent attacks certain squares (the king may not castle out of, through, or into
check), and nothing in Rotor answers "is this square attacked by color X" yet. That
question is also exactly what the still-pending legality-filtering increment will need
(does a candidate move leave the mover's own king in check) -- two concrete, already-known
callers, not a speculative one, so it's built now as shared infrastructure rather than
inlined into castling alone.

`is_square_attacked(board: &Board, square: Square, by_color: Color) -> bool`, in a new
`movegen/attacks.rs`. Technique: place a virtual piece of each type on `square` and check
whether it would attack a real `by_color` piece of that same type --

- Knight: `knight_attacks(square) & board.bitboard(Piece::new(by_color, Knight))`
- King: `king_attacks(square) & board.bitboard(Piece::new(by_color, King))`
- Bishop/queen: `bishop_attacks(square, occupancy) & (bishops | queens)` of `by_color`
- Rook/queen: `rook_attacks(square, occupancy) & (rooks | queens)` of `by_color`
- Pawn: needs a new `pawn_attacks(square, color) -> Bitboard` in `pawn.rs` (the two
  diagonal squares a `color` pawn on `square` attacks -- didn't exist yet, `pawn.rs` only
  had capture logic inline). Then: the squares a `by_color` pawn would attack *into*
  `square` are `pawn_attacks(square, by_color.opposite())` -- pawn attacks are diagonal
  and symmetric under a color flip, so asking "which enemy pawns hit me here" reuses the
  same table by flipping color once, rather than needing a second, inverted table.

`square` is attacked whenever any of the above intersections is non-empty.

`MoveKind` gains a `Castle` variant. No separate kingside/queenside variant -- the king's
own `to` square already disambiguates (e1->g1 vs e1->c1), same reasoning as not having a
separate `Capture` variant.

`castling_moves(board: &Board, color: Color) -> Vec<Move>` in a new
`movegen/castling.rs`. A small fixed table of the 4 (color, side) combinations, each
holding: the king's full path (from -> through -> to -- e.g. White kingside is
e1-f1-g1), the squares that must be empty (e.g. White kingside: f1, g1; White queenside:
b1, c1, d1 -- note b1 must be empty but the king never passes through it), and the rook's
from/to (not stored on the `Move` itself -- make/unmake, once designed, will infer the
rook's move from the king's destination, consistent with `Move` staying a value type that
doesn't reference the board it came from).

A castling move is generated only when: the relevant `CastlingRights` flag is set; every
square in that side's empty-squares set is actually empty; and no square on the king's
full path is attacked by the opponent. That last check elegantly folds in "the king isn't
currently in check" as just the first square of the same path being safe -- no separate
check needed.

`CastlingRights` is trusted as given, not re-derived by checking the rook is still on its
home square -- keeping the flag correct as rooks move or get captured is a make/unmake
concern for later, out of scope for generation off a already-given `Board`.

### Top-level `pseudo_legal_moves` combinator

Mechanical, not a design fork: each piece type already generates its own moves, so this
is just the union of all seven generators (pawn, knight, bishop, rook, queen, king,
castling) for one `color`, living directly in `movegen/mod.rs` rather than its own file --
there's no logic here worth separating out. `pub fn pseudo_legal_moves(board: &Board,
color: Color) -> Vec<Move>`, same `color`-as-explicit-parameter shape as every generator
it calls.

### Make/unmake

Legality filtering needs to answer "does this move leave my own king in check," which
means actually applying the move to see. Rotor had no way to apply a `Move` to a `Board`
at all, so that came first.

**Make/unmake vs. copy-make.** A real architectural fork, discussed explicitly: mutate
the `Board` in place and reverse it via a small undo record, or clone the whole `Board`
per move and never reverse anything. Settled on make/unmake. Unlike the earlier PEXT-vs-
magic-bitboards choice, this isn't a swappable internal detail -- it's the calling
convention every future piece of search code gets written against, so it was worth
deciding well up front. The speed argument for make/unmake rests on the same kind of
external, well-replicated evidence as the PEXT decision (every mainstream engine's own
extensive testing has already settled it) rather than an in-Rotor benchmark, since there's
no search loop yet to even run such a benchmark through.

**`UndoInfo`**: a pure opaque receipt (all fields private, no accessors -- callers only
ever hold it and hand it back), holding exactly what `make_move` overwrites that isn't
recoverable from the `Move` alone: `captured: Option<PieceType>` (color is always the
mover's opponent, so only the type needs storing), plus the prior `castling_rights`,
`en_passant_square`, and `halfmove_clock`. This is precisely the information `Move` was
deliberately kept free of, ever since the knight/king increment ("that information will
need a separate undo/history record later, not `Move`") -- this is that record, finally
built.

`Board::make_move(&mut self, mv: Move) -> UndoInfo` and
`Board::unmake_move(&mut self, mv: Move, undo: UndoInfo)`, in a new `board/make_move.rs`.
Two private sync helpers, `place_piece`/`remove_piece`, do the actual bitboard+mailbox
bookkeeping the `mailbox` field's own doc comment already promised ("kept in sync with
`pieces` on every move") -- this is where that promise is actually kept.

`make_move`'s shape: find the captured square (`mv.to()`, except
`MoveKind::EnPassantCapture`, where the captured pawn is never actually on `to`), remove
whatever's there, relocate the moving piece (or its promoted replacement), relocate the
rook too for `MoveKind::Castle`, update castling rights (king moved -> clear both of that
color's rights; a rook moved from, or was captured on, its own home square -> clear just
that side's right), set or clear the en passant square (only `Some` right after a
`DoublePawnPush`, computed as the midpoint rank between `from` and `to` -- pure rank
arithmetic, no color-dependent "which way is forward" needed), update the halfmove clock
(0 after a pawn move or any capture, else +1), flip `side_to_move`, and return everything
just-overwritten as the `UndoInfo`. `unmake_move` runs the same shape backward, flipping
`side_to_move` back first so the rest of the logic can treat "the mover" consistently.

**Layering fix: `Move`/`MoveKind` moved from `movegen` to `board`.** `make_move` needs the
`Move` type, but `Move` lived in `movegen`, which exists specifically to be built *on top
of* `board` (every `movegen` file imports from `board`, never the reverse). Discussed
explicitly as a real fork -- leave `Move` in `movegen` and let `board` reach upward into
it (works fine, Rust doesn't enforce single-crate modules as a strict DAG, but reads
backwards), versus relocate it. Chose relocation: `board/chess_move.rs` now holds
`Move`/`MoveKind` (unchanged otherwise), `movegen` still has all the piece-specific
generation logic that produces `Move`s, and every `movegen/*.rs` file's `use` line was
updated to import from `board` instead. Strict one-directional layering restored:
`board` never depends on `movegen`.

**Shared castling geometry.** `make_move`'s castling case needs "given the king's
destination, which rook moves where" -- geometry `movegen/castling.rs` already had, but
deliberately without `rook_from`/`rook_to` fields, since generation hadn't needed them yet
("moving the rook is a make/unmake concern"). Now it is. Discussed explicitly: share the
table (move it into `board`, the lower layer both `movegen::castling` and
`board::make_move` can depend on) or duplicate a small, self-contained four-arm match in
`make_move.rs` (the facts involved -- rook home/landing squares -- are literal, unchanging
board geometry, low risk to duplicate). Chose sharing: `CastlingSide`
(`Kingside`/`Queenside`) and `CastlingSquares` (`king_from`/`king_to`/`rook_from`/
`rook_to`) added to `board/castling_rights.rs`; `movegen/castling.rs`'s `CastlingPath`
now builds its `king_from`/`king_to`/`transit_square` from `CastlingSide::squares` rather
than its own literals -- and `transit_square` turned out to always equal `rook_to` (the
rook always lands exactly where the king crosses), a simplification that fell out of
having the shared table rather than being designed in from the start.

### Legality filtering

**When legality filtering applies, and when it deliberately doesn't.** Discussed
explicitly before designing any code: "attacks" (`is_square_attacked`, and everything
built on plain attack tables) must never be legality-filtered, on pain of circularity --
whether a square is attacked can't be defined in terms of whether the attacker could
*legally* move there, since a pinned piece still attacks/defends squares for check-
detection purposes even though it can't legally move off its pin line itself. Legality
filtering applies in exactly one place: producing the actual list of moves a player may
play (`legal_moves`, and later, search).

**Two ways to structure the check, only one of which is being built now.** A standalone
`legal_moves` filters every pseudo-legal move via its own make/check/unmake, producing a
`Vec<Move>` -- simple, reusable (perft, "what can be played here" for UCI), but pays for
make/unmake twice on every move that turns out legal (once here, once again when a future
search actually explores it). The efficient alternative, used by essentially every real
engine, fuses the check directly into the search loop's own single make/unmake (make it
once, check, either recurse the search from there or immediately unmake and move to the
next candidate without ever recursing). Rotor has no search loop yet to fuse anything
into, so the standalone utility version is what's being built now; the fused version is a
decision that belongs to the future search increment, not this one.

**`Board` derives `Clone`/`Copy`.** Every field already is one (arrays of `Copy` types,
plus `Color`/`CastlingRights`/`Square`/`u8`), so this states an already-true fact rather
than adding new capability. Lets `legal_moves` take `&Board` (consistent with every other
generator) while still getting a mutable scratch copy internally (`let mut scratch =
*board;`) to test candidates against via make/unmake, without needing a `&mut Board` just
to ask what's legal. One cheap copy per `legal_moves` call, not per candidate move --
doesn't reopen the copy-make-vs-make/unmake question (that was about paying a copy on
every move in a search visiting millions of nodes).

**`king_square`/`is_in_check`**, added to `movegen/attacks.rs` alongside
`is_square_attacked`: `king_square(board, color)` finds `color`'s (unique) king via its
own bitboard; `is_in_check(board, color)` is just `is_square_attacked` at that square by
the opponent. Small, but likely reused again once search exists (check-extension is
standard practice there).

**`legal_moves(board, color) -> Vec<Move>`**, in a new `movegen/legal.rs` (its own file,
unlike `pseudo_legal_moves` -- this one has real logic, not just a union): for each
`pseudo_legal_moves` candidate, `make_move` it on the scratch board, keep it if
`is_in_check(&scratch, color)` is false, `unmake_move` regardless.

## Decisions summary

- Pseudo-legal generation and legality filtering are separate increments; this document
  currently covers pseudo-legal only.
- First slice: `Move` type, then knight pseudo-legal moves, then king pseudo-legal moves
  (no castling yet). Pawns, sliding pieces, castling, and legality filtering are later
  increments.
- `Move`: struct with private fields (`from: Square`, `to: Square`,
  `promotion: Option<PieceType>`, `kind: MoveKind`), public accessor/constructor methods
  only -- no public field access. `Debug`/`Clone`/`Copy`/`PartialEq`/`Eq` derived.
  `MoveKind` (also `Debug`/`Clone`/`Copy`/`PartialEq`/`Eq`) has three variants: `Quiet`
  (includes ordinary captures -- discoverable from `Board` at the time, not stored),
  `DoublePawnPush`, `EnPassantCapture`. No captured-piece identity or prior
  position-history fields (prior en passant square, prior castling rights) -- `Move` is
  a value type describing only the move itself; that information belongs on a future,
  separate undo/history record. `Move::new(from, to, promotion)` unchanged (defaults
  `kind` to `Quiet`); `Move::double_pawn_push(from, to)` and
  `Move::en_passant_capture(from, to)` added alongside it, neither taking a `promotion`
  parameter (impossible for either case).
- Packed-bitfield `Move` encoding: deliberately deferred, not rejected -- revisit only if
  profiling against real search or perft shows the struct representation costs something
  measurable, at which point it is a self-contained change inside `move.rs`.
- Knight/king attack tables: `LazyLock<[Bitboard; 64]>`, computed once on first access,
  not recomputed per call and not a true compile-time `const` (blocked by `Bitboard`'s
  operators being trait-based). Adopted deliberately, as an explicit exception to the
  "no optimization without measurement" default -- justified because this is a pure
  function over a small, fixed, never-changing domain, not an uncertain-payoff guess.
- `Board::bitboard(piece)` (renamed from `pieces_of` -- see "Board::pieces_of renamed to
  Board::bitboard" above) and `Board::occupancy(color)`: added directly to `Board`'s
  `impl` block in `mod.rs`. `occupancy` loops `PieceType` variants through `Piece::new`
  rather than restating `Piece`'s `color * 6 + piece_type` arithmetic a second time.
- `Bitboard::squares()`: bitscan-and-clear-lowest-bit iteration, added as a shared
  `Bitboard` method (not per-piece-type inline code) since every future piece type needs
  it, not just knight/king. Public surface is `impl Iterator<Item = Square>`; the actual
  iterator struct stays private.
- `Square::try_offset(file_delta, rank_delta) -> Option<Square>`: extracted as a shared
  `Square` method once a third real call site (pawns) appeared, replacing knight's and
  king's own inline versions of the same bounds-check logic.
- `Board::all_occupancy()` and `Board::en_passant_square()`: two more `Board` accessors,
  same pattern as `bitboard`/`occupancy` -- exposing state `Board` already stores.
- `Color::opposite()`: a plain 2-arm match, needed to find enemy occupancy for captures.
- Pawn move generation: `Move`'s `kind`/`MoveKind` extension (above) plus
  `pawn_moves(board, color)` in `movegen/pawn.rs`, covering all five pawn cases (single
  push, double push, diagonal capture, en passant, promotion). See "Pawn move
  generation" above for the full structure.
- Sliding pieces: PEXT-only (CLAUDE.md rule 6 updated), no magic-bitboard fallback, no
  runtime capability check -- targets only the developer's own confirmed-BMI2 PC for
  now, with ARM/portability an explicit deferred future extension. "PEXT at least as
  fast as magic bitboards" relies on external published evidence, not an in-Rotor
  benchmark (no magic-bitboard implementation was built to compare against). Shared
  `SlidingAttackTable` machinery in `movegen/sliding.rs` (mask-building, PDEP-based
  table-building, PEXT-based lookup), parameterized by direction list; `bishop.rs`/
  `rook.rs` each supply their own 4 directions; `queen.rs` combines both via OR, with no
  attack table of its own. `Bitboard::pdep`/`Bitboard::pext` wrap `std::arch::x86_64`'s
  `_pdep_u64`/`_pext_u64` (genuinely `unsafe`, unconditionally, in std) behind a
  `target-feature=+bmi2` compile-time assumption (`.cargo/config.toml`), with the
  `unsafe` kept fully inside `Bitboard`'s own file. `Bitboard::count()` added alongside.
  See "Sliding piece move generation" above for the full reasoning.
- Attacked-square detection: `is_square_attacked(board, square, by_color)` in a new
  `movegen/attacks.rs`, built as shared infrastructure for both castling and the
  still-pending legality-filtering increment (two known, concrete callers). Uses the
  "place a virtual piece of each type on the square, check if it attacks a real enemy
  piece of that type" technique for knight/king/bishop/rook/queen, plus a color-flip
  trick for pawns (`pawn_attacks(square, by_color.opposite())`, new in `pawn.rs`). See
  "Attacked-square detection and castling" above for the full design.
- Castling: `MoveKind::Castle` (no separate kingside/queenside variant -- the king's `to`
  square disambiguates) plus `castling_moves(board, color)` in a new
  `movegen/castling.rs`, a fixed table of the 4 (color, side) combinations (king path,
  empty-squares set, rook from/to). Generated only when the `CastlingRights` flag is set,
  the empty-squares set is actually empty, and no square on the king's path is attacked --
  the last check also covers "not currently in check" for free, as the path's first
  square. See "Attacked-square detection and castling" above for the full design.
- Top-level combinator: `pseudo_legal_moves(board, color)` in `movegen/mod.rs`, unioning
  all seven per-piece-type generators. No separate file -- purely mechanical, no logic of
  its own worth isolating. See "Top-level pseudo_legal_moves combinator" above.
- Make/unmake: chosen over copy-make (relies on the same kind of external, well-replicated
  evidence as the PEXT decision, not an in-Rotor benchmark -- no search loop exists yet to
  run one through). `UndoInfo` (opaque receipt: captured piece type, prior castling
  rights, prior en passant square, prior halfmove clock) plus `Board::make_move`/
  `Board::unmake_move` in a new `board/make_move.rs`. `Move`/`MoveKind` relocated from
  `movegen/chess_move.rs` to `board/chess_move.rs` (restores strict one-directional
  layering -- `board` never depends on `movegen`). `CastlingSide`/`CastlingSquares` added
  to `board/castling_rights.rs`, shared by `movegen/castling.rs` (generation) and
  `board/make_move.rs` (application) rather than duplicated. See "Make/unmake" above for
  the full design.
- Legality filtering: attacks vs. legal moves kept strictly separate (attacks are never
  legality-filtered, on pain of circularity -- see "Legality filtering" above). Standalone
  `legal_moves(board, color)` built now (fused-into-search is a future search-increment
  decision, not this one). `Board` derives `Clone`/`Copy` (already true field-by-field),
  letting `legal_moves` take `&Board` and still get an internal mutable scratch copy.
  `king_square`/`is_in_check` added to `movegen/attacks.rs`. `legal_moves` itself in a new
  `movegen/legal.rs`. See "Legality filtering" above for the full design.

## Implementation progress

Done:
- New top-level `movegen` module (sibling to `board`), registered in `main.rs`.
- `Move`: struct with private `from`/`to`/`promotion` fields, `new`/`.from()`/`.to()`/
  `.promotion()`, living in `movegen/chess_move.rs` (named that, not `move.rs`, since
  `move` is a reserved keyword). `Debug`/`Clone`/`Copy`/`PartialEq`/`Eq` derived. Two
  tests: a plain move's accessors round-trip, and a promotion move's do too.

Done (continued):
- `knight_moves(board: &Board, color: Color) -> Vec<Move>` and
  `king_moves(board: &Board, color: Color) -> Vec<Move>`, in `movegen/knight.rs` and
  `movegen/king.rs` respectively. Both take `color` as an explicit parameter rather than
  reading `board`'s own side-to-move, since legality filtering (checking whether the
  *other* side attacks a given square) will need to generate moves/attacks for either
  color, not just whoever's turn it is -- deciding this now avoids two separate code
  paths later. Each: for every one of `color`'s knights/king (via `pieces_of` +
  `Bitboard::squares()`), destinations are that piece's attacks minus squares occupied by
  `color`'s own side (`attacks & !occupancy(color)`), each surviving destination becomes
  one `Move`. Tests use `Board::from_fen` to build small hand-constructed positions:
  own-piece exclusion + enemy capture for both piece types, plus a multi-knight test and
  a center-of-empty-board king test.

`cargo test`: 44 tests, all passing. The knight/king increment (`Move` type, plus
pseudo-legal knight and king move generation) is complete as scoped.

Now starting the pawn increment:

Done:
- `Move` extended with `kind: MoveKind` (`Quiet`/`DoublePawnPush`/`EnPassantCapture`),
  plus `Move::double_pawn_push`/`Move::en_passant_capture` constructors alongside the
  unchanged `Move::new`, in `movegen/chess_move.rs`. See "Move extension for pawns"
  above for the full reasoning, in particular what was deliberately kept off `Move`
  (captured-piece identity, prior position history) and why. Four tests: the original
  two (now also asserting `.kind() == Quiet`) plus one each for the two new
  constructors.

- `Square::try_offset`, `Board::all_occupancy`/`Board::en_passant_square`,
  `Color::opposite`, and `pawn_moves` itself, all implemented -- see "Square::try_offset",
  "Board additions for pawns", and "Pawn move generation" above for the full designs.
  `knight.rs`/`king.rs` refactored to use `Square::try_offset` instead of their own
  inline bounds-check logic (pure dedup, behavior unchanged). Nine tests in `pawn.rs`,
  one in `color.rs` (`opposite`), two in `square.rs` (`try_offset`).

`cargo test`: 58 tests, all passing. The pawn increment (`Move`'s `kind` extension, plus
pseudo-legal pawn move generation) is complete as scoped.

Now starting the sliding-piece increment:

Done:
- `.cargo/config.toml` (`target-feature=+bmi2`); `Bitboard::count`/`pdep`/`pext`;
  `movegen/sliding.rs` (`SlidingAttackTable`, `relevant_mask`, `ray_attacks`); `bishop.rs`,
  `rook.rs`, `queen.rs` with `*_attacks`/`*_moves`. See "Sliding piece move generation"
  above for the full design. Eleven tests: 3 on `Bitboard` (`count`, `pdep` scattering,
  `pdep`/`pext` round-trip), 4 on `SlidingAttackTable` (mask correctness, ray-blocking
  behavior, table-vs-direct-computation match, open-board square count), 1 each on
  `rook_moves`/`bishop_moves` (blocker + capture), 2 on `queen_moves` (combines
  bishop+rook attacks; blocks/captures in both move shapes).

`cargo test`: 69 tests, all passing. The sliding-piece increment (PEXT-based bishop,
rook, and queen pseudo-legal move generation) is complete as scoped.

Done:
- `Board::castling_rights()` accessor added (the field existed, nothing had exposed it
  yet). `Board::bitboard`/`pawn_attacks`/`is_square_attacked`/`MoveKind::Castle`/
  `castling_moves` implemented -- see "Attacked-square detection and castling" above for
  the full design. Fourteen tests: 2 on `pawn_attacks`, 1 on `Move::castle`, 4 on
  `is_square_attacked` (knight, pawn direction, sliding-through-vs-blocked, empty board),
  7 on `castling_moves` (kingside, queenside, no rights, blocked path, out of check,
  through an attacked square, Black symmetry).

`cargo test`: 83 tests, all passing. The castling increment (attacked-square detection
plus pseudo-legal castling generation) is complete as scoped.

Now starting the top-level combinator increment:

Done:
- `pseudo_legal_moves(board, color)` added directly to `movegen/mod.rs`, unioning all
  seven generators (pawn, knight, bishop, rook, queen, king, castling). See "Top-level
  pseudo_legal_moves combinator" above. Two tests: starting position's textbook 20 moves
  for White; a king/rook/castling-only position checking the combined count equals the
  sum of each generator's own count (no double-counting, nothing dropped).

`cargo test`: 85 tests, all passing. This increment is complete as scoped.

Now starting the make/unmake increment:

Done:
- `Move`/`MoveKind` relocated from `movegen/chess_move.rs` to `board/chess_move.rs`
  (unchanged otherwise), every `movegen/*.rs` `use` line updated. `CastlingSide`/
  `CastlingSquares` added to `board/castling_rights.rs`; `movegen/castling.rs`'s
  `CastlingPath` construction rewritten to build from them instead of its own literals
  (behavior unchanged, its 7 tests untouched). `UndoInfo`, `Board::make_move`,
  `Board::unmake_move`, and the private `place_piece`/`remove_piece` sync helpers, all in
  a new `board/make_move.rs`. See "Make/unmake" above for the full design. Ten tests,
  mostly structured as make-then-assert-then-unmake-then-assert-restored round trips
  (verified via `Board::to_fen`, since `Board` itself has no `PartialEq`): quiet move,
  ordinary capture, en passant capture, promotion, and castling, plus four more targeted
  at castling-rights bookkeeping specifically (king move clears both rights, rook move
  clears one, capturing a rook on its home square clears the opponent's, en passant square
  clearing on an unrelated move) -- one bug caught and fixed along the way: `make_move.rs`
  was written but never registered in `board/mod.rs`'s `pub mod` list, so its tests
  silently didn't run at all (`cargo test` reported 87, not the expected 96 -- the exact
  shortfall of what turned out to be its 10 tests) until `pub mod make_move;` was added.
  Two new tests in `castling_rights.rs` for the new `CastlingSide::squares` geometry
  itself.

`cargo test`: 97 tests, all passing. The make/unmake increment is complete as scoped.

Now starting the legality-filtering increment:

Done:
- Decided explicitly, before writing any code, exactly where legality filtering applies
  and where it deliberately doesn't (attacks, never; the actual playable-move list,
  always) -- see "Legality filtering" above. `Board` derives `Clone`/`Copy`. `king_square`/
  `is_in_check` added to `movegen/attacks.rs`, four tests. `legal_moves(board, color)` in a
  new `movegen/legal.rs`, four tests (pinned piece excluded, king move into an attacked
  square excluded, only check-resolving moves survive when already in check, legal and
  pseudo-legal agree exactly in a quiet position with no checks or pins). One test bug
  caught and fixed: a test FEN omitted a color's king entirely while calling `is_in_check`
  for that color, panicking `king_square`'s "exactly one king" assumption -- fixed by
  giving that FEN a king.

`cargo test`: 103 tests, all passing. The legality-filtering increment is complete as
scoped.

This closes out every increment this document originally scoped: all six piece types'
pseudo-legal generation, castling, the top-level `pseudo_legal_moves` combinator,
make/unmake, and legality filtering. No further increments are currently planned here --
further work (perft-based verification against published values, search, UCI) would begin
as new, separate work items rather than continuing this document.

## Progress log

- 2026-09-10: `movegen` branch created (from `main`, after the board-representation PR
  merged). Move generation's two-phase structure (pseudo-legal, then legality filtering)
  agreed, and the first slice scoped to `Move` plus knight/king pseudo-legal generation.
  `Move`'s representation agreed: private-field struct with accessor methods (not a
  packed encoding, deferred per rule 5), minimal fields for now (`from`, `to`,
  `promotion`, no special-move `kind` field yet -- pawns will need one, but its shape
  isn't settled enough to design now). `Move` implemented in `movegen/chess_move.rs` with
  two tests; `cargo test` confirmed passing (33 tests). Knight/king attack generation
  designed: `LazyLock<[Bitboard; 64]>` tables, computed once and cached, an explicit
  exception to "no optimization without measurement" justified by this being a pure
  function over a small, unchanging, repeatedly-queried domain (see "Knight and king
  attack generation" above for the full reasoning). Implemented in `movegen/knight.rs`
  and `movegen/king.rs`, each with a corner-square and a center-square test. `cargo test`
  confirmed passing (37 tests). `Board::pieces_of`/`Board::occupancy` designed and
  implemented directly in `board/mod.rs`: `occupancy` loops `PieceType` through
  `Piece::new` rather than restating `Piece`'s numbering arithmetic a second time, a
  choice settled after discussing (and rejecting, per rule 5) building both versions to
  profile a speed difference that inlining likely erases anyway. `Bitboard::squares()`
  designed and implemented: bitscan-and-clear-lowest-bit, the standard technique real
  engines use, added as a shared method (not per-piece code) since every future piece
  type needs exactly this, confirmed to be real, near-universal practice rather than a
  one-off trick. Three tests: empty bitboard, ascending-order check on a specific pair
  of squares, and a full-bitboard count. `knight_moves`/`king_moves` implemented,
  combining everything built so far; both take an explicit `color` parameter rather than
  reading `board`'s side-to-move, anticipating legality filtering's future need to
  generate the non-moving side's attacks too. Four tests total across both, built on
  `Board::from_fen` for small hand-constructed positions -- two initially failed from
  hand-typed "expected" square orderings that didn't match ascending-by-index order,
  fixed by sorting both sides of each comparison instead of hand-deriving the sorted
  order. `cargo test` confirmed passing: 44 tests. This increment (`Move` type, plus
  pseudo-legal knight and king move generation) is complete as scoped. Next increment,
  whenever picked back up: pawns, sliding pieces, castling, or legality filtering --
  not yet decided which.
- 2026-09-10 (continued): decided to pick up pawns next. Design conversation: pawns
  force the `Move`-extension question deferred during the knight/king increment. Agreed
  `Move` gains a `kind: MoveKind` field (`Quiet`/`DoublePawnPush`/`EnPassantCapture`),
  with two new constructors (`double_pawn_push`, `en_passant_capture`) added alongside
  the unchanged `Move::new` -- explicitly not a `Capture` variant (discoverable from
  `Board` at the time, same as knight/king already work) and explicitly not
  captured-piece identity or prior en-passant/castling-rights state, which was
  discussed and rejected as violating `Move`'s nature as a value type describing only
  the move itself, never the position it came from; that information will need a
  separate undo/history record later, not `Move`. See "Move extension for pawns" above
  for the full reasoning. Implemented in `movegen/chess_move.rs` with two new tests
  (plus the two existing ones updated to also check `.kind()`). `cargo test` confirmed
  passing (46 tests). Noticed knight/king each had their own inline offset-bounds-check
  logic, and pawns needed the same pattern a third time -- extracted
  `Square::try_offset(file_delta, rank_delta) -> Option<Square>` as a shared method and
  refactored both to use it (pure dedup, no behavior change). Added `Board::all_occupancy`,
  `Board::en_passant_square`, and `Color::opposite` -- the remaining small pieces pawn
  generation needed. Implemented `pawn_moves` in `movegen/pawn.rs`, covering single push,
  double push, diagonal capture, en passant, and promotion (one `Move` per promotion
  piece on any push/capture reaching the last rank), with nine tests. `cargo test`
  confirmed passing: 58 tests. The pawn increment is complete as scoped. Next increment,
  whenever picked back up: sliding pieces, castling, or legality filtering -- not yet
  decided which.
- 2026-09-10 (continued): decided to pick up sliding pieces (bishop/rook/queen) next.
  Design conversation revisited CLAUDE.md rule 6 itself: the developer's target is only
  their own PC (confirmed to have BMI2/PEXT) at this stage, not portability, so rule 6
  was updated to PEXT-only sliding-attack lookup, no magic-bitboard fallback, no startup
  capability check -- ARM/portability an explicit, deferred future extension. Discussed
  and accepted that "PEXT is at least as fast as magic bitboards" rests on external,
  widely-replicated evidence rather than an in-Rotor benchmark, given the stakes
  (microseconds) and the weaker claim actually needed ("not worse", not "faster").
  Designed the PEXT-bitboard mechanism (relevant occupancy mask, PEXT for lookup, PDEP
  for table-building -- its exact inverse) and agreed to share bishop/rook's nearly
  identical machinery in `movegen/sliding.rs`, parameterized by direction list, rather
  than duplicating it (two known call sites, same reasoning as `Square::try_offset`).
  Implemented: `.cargo/config.toml` (`target-feature=+bmi2`); `Bitboard::count`/`pdep`/
  `pext` (real `unsafe`, contained inside `Bitboard`'s own file); `movegen/sliding.rs`
  (`SlidingAttackTable`); `bishop.rs`/`rook.rs`/`queen.rs`. Eleven tests across all of
  it. See "Sliding piece move generation" above for the full design. `cargo test`
  confirmed passing: 69 tests. The sliding-piece increment is complete as scoped. Next
  increment, whenever picked back up: castling or legality filtering -- not yet decided
  which.
- 2026-09-10 (continued): decided to pick up castling next. `Board::pieces_of` renamed to
  `Board::bitboard` first (raised as a naming critique -- the old name described "pieces,"
  not the `Bitboard` it actually returns), every call site updated, `cargo test`
  reconfirmed passing (69 tests). Design conversation: castling isn't self-contained --
  it needs to know whether the opponent attacks certain squares, which nothing in Rotor
  answers yet, and that same question is exactly what the still-pending legality-filtering
  increment will need, so it's built now as shared infrastructure (two known callers, not
  a speculative one) rather than folded into castling alone. Agreed:
  `is_square_attacked(board, square, by_color)` in a new `movegen/attacks.rs`, using the
  "virtual piece of each type on the square, check if it attacks a real enemy piece of
  that type" technique, plus a new `pawn_attacks(square, color)` in `pawn.rs` and a
  color-flip trick for pawn attacks specifically. `MoveKind::Castle` added (no separate
  kingside/queenside variant). `castling_moves(board, color)` designed for a new
  `movegen/castling.rs`: a fixed table of the 4 (color, side) combinations, each checked
  against the relevant `CastlingRights` flag, an empty-squares set, and "no square on the
  king's full path is attacked" (which folds in "not currently in check" as the path's
  first square, no separate check needed). See "Attacked-square detection and castling"
  above for the full design. Implemented: `Board::castling_rights()` accessor,
  `pawn_attacks` in `pawn.rs`, `is_square_attacked` in a new `movegen/attacks.rs`,
  `MoveKind::Castle`/`Move::castle` in `chess_move.rs`, `castling_moves` in a new
  `movegen/castling.rs`. Fourteen tests across all of it. `cargo test` confirmed passing:
  83 tests. The castling increment is complete as scoped. Next increment, whenever picked
  back up: legality filtering, or the optional top-level `pseudo_legal_moves` combinator.
- 2026-09-10 (continued): decided to pick up the top-level `pseudo_legal_moves` combinator
  next -- mechanical, not a design fork, so implemented directly without a separate design
  round. `pub fn pseudo_legal_moves(board: &Board, color: Color) -> Vec<Move>` added to
  `movegen/mod.rs` itself (no new file -- there's no logic here worth separating out),
  unioning all seven generators. Two tests: the starting position's textbook 20 pseudo-
  legal moves for White, and a combined-count-equals-sum-of-parts check on a smaller
  king/rook/castling-only position. `cargo test` confirmed passing: 85 tests. This
  increment is complete as scoped. Next increment, whenever picked back up: legality
  filtering -- the last item on the pseudo-legal move generation list.
- 2026-09-10 (continued): decided to build make/unmake before legality filtering itself --
  checking whether a move leaves the mover's own king in check means actually applying the
  move, which Rotor had no way to do yet. Chose make/unmake over copy-make (real
  architectural fork, discussed explicitly -- see "Make/unmake" above for the full
  reasoning, including why this isn't the same kind of "assumed, not measured" claim rule
  5 warns against). Along the way, two further forks came up and were each discussed and
  resolved explicitly: relocating `Move`/`MoveKind` from `movegen` to `board` (restoring
  strict one-directional layering, since `make_move` needed the type but `board` must
  never depend on `movegen`), and sharing the castling king/rook geometry between
  `movegen/castling.rs` and the new `board/make_move.rs` via `CastlingSide`/
  `CastlingSquares` in `board/castling_rights.rs` rather than duplicating it. `UndoInfo`,
  `Board::make_move`/`unmake_move` implemented in a new `board/make_move.rs`, ten tests
  (mostly make-then-unmake round trips verified via `Board::to_fen`, plus targeted
  castling-rights-bookkeeping checks). One bug along the way: the new file was never
  registered in `board/mod.rs` (`pub mod make_move;` missing), so its tests silently
  didn't run -- caught because the reported test count (87) was short of the expected
  count by exactly 10, the number of tests in the missing file. `cargo test` confirmed
  passing once fixed: 97 tests. The make/unmake increment is complete as scoped. Next
  increment, whenever picked back up: legality filtering itself, now unblocked.
- 2026-09-10 (continued): legality filtering. Discussion first settled where legality
  filtering applies and where it deliberately doesn't -- attacks (`is_square_attacked`)
  are never legality-filtered, on pain of circularity (a pinned piece still attacks/
  defends squares for check-detection purposes, even though it can't legally move off its
  pin line); legality filtering applies only to producing the actual playable-move list.
  Also discussed the standalone-vs-fused-into-search question explicitly: a standalone
  `legal_moves` (built now) pays for make/unmake twice per legal move once search exists
  and calls it, versus fusing the check directly into search's own single make/unmake
  (the standard, efficient approach real engines use) -- deferred, since Rotor has no
  search loop yet to fuse anything into; that choice belongs to the future search
  increment. `Board` derives `Clone`/`Copy` (already true field-by-field, lets
  `legal_moves` take `&Board` like every other generator while still getting a mutable
  scratch copy internally). `king_square`/`is_in_check` added to `movegen/attacks.rs`;
  `legal_moves` in a new `movegen/legal.rs`. Eight tests across both. One test bug caught
  and fixed: a test FEN omitted a color's king while calling `is_in_check` for that color,
  panicking `king_square`'s "exactly one king" assumption. `cargo test` confirmed passing:
  103 tests. This closes out every increment this document originally scoped: all six
  piece types, castling, the top-level combinator, make/unmake, and legality filtering.
  Further work (perft, search, UCI) begins as new, separate work items, not a continuation
  of this document.
