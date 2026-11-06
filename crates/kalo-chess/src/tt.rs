use crate::board::chess_move::{Move, MoveKind};
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use std::sync::atomic::{AtomicU64, Ordering};

/// Packs `mv` into 15 bits: 6 for `from`, 6 for `to`, 3 for a combined kind-or-promotion
/// code. Lives here, not on `Move` itself, since fitting a `Move` inside a shared atomic
/// word is `tt.rs`'s own storage need, not a general `Move` capability -- see
/// `threading.md`'s reasoning for why this stays a private free function in this file.
///
/// The 3-bit code exploits a fact every `Move` constructor already guarantees: `promotion`
/// is only ever `Some` alongside `MoveKind::Quiet` (a double pawn push, en passant capture,
/// or castle can never simultaneously be a promotion). That leaves exactly 8 real
/// combinations for a 3-bit field to cover, none wasted: no-promotion `Quiet` (`0`),
/// promoting `Quiet` to one of the four promotable pieces (`1..=4`, deliberately equal to
/// `PieceType::Knight as u8` through `PieceType::Queen as u8` -- `PieceType`'s own declared
/// order already groups the promotable pieces contiguously between `Pawn` and `King`, so
/// this reuses that discriminant rather than inventing a new numbering), and the three
/// non-`Quiet` kinds (`5`, `6`, `7`).
fn pack_move(mv: Move) -> u16 {
    let from = mv.from() as u16;
    let to = mv.to() as u16;
    let kind_code: u16 = match mv.kind() {
        MoveKind::Quiet => match mv.promotion() {
            None => 0,
            Some(piece) => piece as u16,
        },
        MoveKind::DoublePawnPush => 5,
        MoveKind::EnPassantCapture => 6,
        MoveKind::Castle => 7,
    };
    (from << 9) | (to << 3) | kind_code
}

/// The inverse of `pack_move`. Trusts that `bits` actually came from `pack_move` -- every
/// panic below (`expect`/`unreachable!`) documents a case that packing itself can never
/// produce, not a real runtime possibility once storage is wired up.
fn unpack_move(bits: u16) -> Move {
    let from = Square::try_from((bits >> 9) as u8 & 0x3f).expect("packed from is always a valid square");
    let to = Square::try_from((bits >> 3) as u8 & 0x3f).expect("packed to is always a valid square");
    let kind_code = bits & 0x7;
    match kind_code {
        0 => Move::new(from, to, None),
        1..=4 => {
            let piece = PieceType::try_from(kind_code as u8).expect("1..=4 always maps to a promotable piece type");
            Move::new(from, to, Some(piece))
        }
        5 => Move::double_pawn_push(from, to),
        6 => Move::en_passant_capture(from, to),
        7 => Move::castle(from, to),
        _ => unreachable!("kind_code is masked to 3 bits, so only 0..=7 is reachable"),
    }
}

/// Which side of the true score a stored `Entry` actually pins down. Alpha-beta's whole
/// point is not always finishing a node's full window, so an entry's `score` isn't always
/// the exact value of the position -- only sometimes, depending on why the search at that
/// node stopped where it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// `score` fell strictly between the search's `alpha` and `beta` at the time it was
    /// stored -- the true value, not merely a bound on it.
    Exact,
    /// `score` caused a beta cutoff: the true value is at least this large, but the search
    /// stopped before narrowing it any further.
    Lower,
    /// `score` never raised `alpha`: the true value is at most this large, but the search
    /// never found a move good enough to prove more than that.
    Upper,
}

/// A decoded snapshot of one transposition-table slot, as `TranspositionTable::probe` read it
/// at one instant. Fields are private, reached only through accessors -- same reasoning as
/// `Move` (see its own doc comment): the internal layout can change later without any caller
/// noticing.
///
/// Deliberately holds no `key` (unlike this struct before the atomic rewrite) -- `key` only
/// ever mattered for deciding whether a slot's contents actually answer the position asked
/// about, a decision `probe` already makes before handing an `Entry` back; nothing downstream
/// of that decision needs to see the key again.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    /// The move that produced `score` -- tried first the next time this position comes up,
    /// on the theory that whatever was best here before is likely still worth trying first.
    best_move: Move,
    /// Remaining depth-to-go this entry was searched with (`target_depth - ply` at the
    /// moment of writing, not `ply` itself -- the same quantity the history table's cutoff
    /// bonus already uses). A probe only trusts an entry for an immediate cutoff when this
    /// is at least as large as the current node's own remaining depth; a shallower entry is
    /// still useful as a move-ordering hint, just not as proof of anything.
    depth: u32,
    /// The stored value, side-to-move-relative like every other score in this engine's
    /// search.
    score: i32,
    /// Which side of the true score `score` actually represents -- see `Bound`.
    bound: Bound,
}

impl Entry {
    /// The move that produced this entry's score.
    pub fn best_move(&self) -> Move {
        self.best_move
    }

    /// Remaining depth-to-go this entry was searched with.
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// The stored, side-to-move-relative score.
    pub fn score(&self) -> i32 {
        self.score
    }

    /// Which side of the true score `score` represents.
    pub fn bound(&self) -> Bound {
        self.bound
    }
}

/// Packs `bound` into 2 bits -- three variants need `ceil(log2(3)) = 2` bits, one code
/// (`3`) left unused. The exact numbering is this function's own business, not a fact
/// `Bound` itself needs to expose, so it doesn't derive any `*Primitive` trait -- unlike
/// `Square`/`PieceType`, nothing outside this packing needs `Bound`'s discriminant.
fn pack_bound(bound: Bound) -> u64 {
    match bound {
        Bound::Exact => 0,
        Bound::Lower => 1,
        Bound::Upper => 2,
    }
}

/// The inverse of `pack_bound`. `3` (and, in principle, any value outside `0..=2`) never
/// occurs in a `u64` this module itself produced -- reachable only if a caller handed
/// `unpack_entry_data` bits that didn't come from `pack_entry_data`, which nothing in this
/// file does.
fn unpack_bound(bits: u64) -> Bound {
    match bits {
        0 => Bound::Exact,
        1 => Bound::Lower,
        2 => Bound::Upper,
        _ => unreachable!("bound is packed into 2 bits by pack_bound, so only 0..=2 is reachable"),
    }
}

/// Packs everything an `Entry` holds besides `key` into one `u64`, laid out (high bits to
/// low) as `generation(8) | bound(2) | depth(8) | score(16) | best_move(15)`. This is the
/// word `threading.md` settled on sharing one `AtomicU64` for: two threads racing to store
/// the *same* transposing position can still tear this word, but never any worse than "the
/// fully-old write or the fully-new write" -- the same indivisibility guarantee a single
/// atomic load already gives for free (see `threading.md`'s TT-concurrency reasoning for
/// why that's the property that actually matters here).
///
/// `score` is a signed `i32` on `Entry` (matching every other score in this engine's
/// search) but stored here as its low 16 bits reinterpreted as unsigned -- lossless because
/// `score` is bounded by `MATE_SCORE = 32_000`, well within `i16`'s range, the same
/// reasoning `threading.md` gives for this field's 16-bit width.
///
/// `depth` is 8 bits (`0..=255`), matching Stockfish's own `depth8` field width rather than
/// whatever `search.rs`'s own iterative-deepening ceiling happened to be at the time --
/// tying the field's width to that would have made `depth` the bottleneck the moment it grew
/// past a narrower field's range, exactly the outcome search.rs's own iterative-deepening
/// ceiling being removed entirely (in favor of pure time management) later confirmed was the
/// right call to guard against (see `threading.md`).
///
/// `generation` wraps at 256 (an 8-bit field) rather than growing to avoid it -- a wraparound
/// only risks a suboptimal replacement decision (a very old entry briefly looking "current"),
/// never a wrong answer returned to the search, since `probe`'s own key check still guards
/// every real hit regardless. See `threading.md`'s bit-width table for the reasoning this
/// field's width was settled with.
fn pack_entry_data(best_move: Move, depth: u32, score: i32, bound: Bound, generation: u8) -> u64 {
    let move_bits = pack_move(best_move) as u64;
    let score_bits = (score as i16 as u16) as u64;
    let depth_bits = (depth as u64) & 0xFF;
    let bound_bits = pack_bound(bound);
    let generation_bits = generation as u64;
    (generation_bits << 41) | (bound_bits << 39) | (depth_bits << 31) | (score_bits << 15) | move_bits
}

/// The inverse of `pack_entry_data`.
fn unpack_entry_data(bits: u64) -> (Move, u32, i32, Bound, u8) {
    let best_move = unpack_move((bits & 0x7FFF) as u16);
    let score = (((bits >> 15) & 0xFFFF) as u16) as i16 as i32;
    let depth = ((bits >> 31) & 0xFF) as u32;
    let bound = unpack_bound((bits >> 39) & 0x3);
    let generation = ((bits >> 41) & 0xFF) as u8;
    (best_move, depth, score, bound, generation)
}

/// The `generation` value reserved to mean "this slot has never been written" -- see
/// `Slot::empty` and `TranspositionTable::probe`. Real per-`go` generations start at
/// `STARTING_GENERATION` and, thanks to `new_search`'s wraparound, never reach this value
/// again once the table is in use.
const NEVER_WRITTEN_GENERATION: u8 = 0;

/// The generation a freshly built or freshly `clear`'d table starts at, and the value
/// `new_search` wraps back to after `u8::MAX` -- `1`, not `0`, since `0` is reserved as
/// `NEVER_WRITTEN_GENERATION`. Also what makes a `store` before the first real `new_search`
/// call (every existing test, and any use before `go`/`new_search` wiring exists) write
/// entries that read back as genuinely written rather than as the empty sentinel.
const STARTING_GENERATION: u8 = 1;

/// One raw storage cell backing a `TranspositionTable` slot: two independent atomics, per
/// `threading.md`'s TT-concurrency design -- `key` alone, and one packed `data` word holding
/// everything else (`best_move`/`depth`/`score`/`bound`/`generation`) together. Packing those
/// five fields into one atomic is what eliminates the worst tear case -- two threads racing to
/// store the *same* transposing position -- outright: a reader of `data` always gets either
/// the fully-old or the fully-new write of the entire word, never a mix of the two, the same
/// indivisibility guarantee a single atomic load already gives for free. The narrower residual
/// risk -- two different, index-colliding positions torn against each other -- rides on the
/// ordinary `key == hash` check `probe` needs regardless (see `threading.md`).
///
/// Not `pub`: nothing outside this module ever needs a raw `Slot`, only the decoded `Entry`
/// `probe` hands back.
struct Slot {
    key: AtomicU64,
    data: AtomicU64,
}

impl Slot {
    /// A slot that has never been written: both atomics zeroed, which decodes to `generation
    /// == NEVER_WRITTEN_GENERATION` -- the sentinel `probe` and `store` both check for, rather
    /// than trusting `key == 0` (see `threading.md` for why the sentinel lives in `generation`,
    /// not `key`: a real position's hash could in principle be exactly `0`, but `generation` is
    /// a table-internal counter with no such conflict).
    fn empty() -> Slot {
        Slot { key: AtomicU64::new(0), data: AtomicU64::new(0) }
    }
}

/// A hash table of previously visited positions, indexed by `Board::hash`. Lets the search
/// skip an already-solved subtree outright, or at least reuse its best move for ordering,
/// when the same position is reached again by a different sequence of moves ("transposing").
pub struct TranspositionTable {
    /// One slot per index, always present -- `Slot::empty()` (decoding to `generation ==
    /// NEVER_WRITTEN_GENERATION`) stands in for "never written" now that there's no `Option`
    /// wrapper to do it for free. Sized to a power of two (see `new`) specifically so a lookup
    /// can compute its index with a bitmask (`hash & mask`) instead of a modulo -- cheaper, at
    /// the cost of always rounding the requested size down to the nearest power of two rather
    /// than using the full byte budget requested. That rounding loss is accepted deliberately:
    /// a table sized generously enough for real play (the way strong engines actually
    /// configure `Hash`) makes it a non-issue in practice, and it buys every probe/store a
    /// bitmask instead of a division.
    entries: Vec<Slot>,
    /// `entries.len() - 1`, precomputed once in `with_slots` rather than recomputed on every
    /// probe/store -- a valid mask specifically because `entries.len()` is always a power of
    /// two.
    mask: u64,
    /// Which search is "current" -- every entry `store` writes gets stamped with this value,
    /// and `store`'s replacement policy treats a differently-tagged existing entry as stale
    /// regardless of its own depth (see `threading.md`'s replacement-policy decision). Bumped
    /// once per `go` via `new_search`, shared across every worker thread searching that `go`
    /// -- never per-thread, never per-depth (see `new_search`'s own doc comment for why).
    generation: u8,
}

impl TranspositionTable {
    /// Builds a table sized to fit within `size_mb` megabytes, rounded down to the nearest
    /// power of two number of slots (see `entries`'s own doc comment for why a power of two).
    pub fn new(size_mb: usize) -> TranspositionTable {
        let budget_bytes = size_mb * 1024 * 1024;
        let slot_bytes = std::mem::size_of::<Slot>();
        // `.max(1)`: a budget too small to fit even one slot still gets a working
        // (if useless) one-slot table, rather than a `1 << (0usize).ilog2()` panic
        // (`ilog2` of zero is undefined) or a zero-slot table that every probe/store would
        // then have to special-case for an empty `entries`.
        let desired_slots = (budget_bytes / slot_bytes).max(1);
        TranspositionTable::with_slots(desired_slots)
    }

    /// The actual constructor, taking a slot count directly rather than a byte budget --
    /// `new` is the only real caller, but tests use this too, to build small tables with an
    /// exact, known slot count (independent of `Slot`'s real byte size) so index collisions
    /// are reachable deterministically instead of by chance.
    fn with_slots(desired_slots: usize) -> TranspositionTable {
        let slots = 1usize << desired_slots.max(1).ilog2();
        let entries = (0..slots).map(|_| Slot::empty()).collect();
        TranspositionTable { entries, mask: (slots - 1) as u64, generation: STARTING_GENERATION }
    }

    /// How many slots this table actually has -- always a power of two (see `entries`'s own
    /// doc comment). Not needed by `probe`/`store` themselves (they only ever need `mask`),
    /// but a plain, honest way for a caller to confirm a resize actually took effect, rather
    /// than inferring it indirectly from probe/store behavior.
    ///
    /// `#[cfg(test)]`: every real caller is a test -- this module's own size tests and `uci.rs`'s
    /// `setoption name Hash` resize test. Not moved into one file's own `mod tests`, since
    /// `uci.rs` needs to call it as an ordinary method on a value it holds, from its own separate
    /// module -- `#[cfg(test)]` gets the same "doesn't exist outside a test build" result while
    /// staying an ordinary, crate-visible method during one.
    #[cfg(test)]
    pub fn slots(&self) -> usize {
        self.entries.len()
    }

    /// Marks the start of a new search: every entry `store` writes from here until the next
    /// call to this method (or to `clear`) is tagged with one shared generation, so `store`'s
    /// replacement policy can recognize an entry from a previous, no-longer-relevant search as
    /// stale regardless of its own depth.
    ///
    /// Call exactly once per `go`, before any worker thread starts probing or storing -- never
    /// per-thread, never per-depth. Threads share one table specifically so each can profit
    /// from the others' fresh writes; bumping per-thread would make every other thread's
    /// entries from the very same search look stale to each other, throwing away the entire
    /// benefit of sharing a table in the first place (see `threading.md`'s generation-bump-
    /// timing decision).
    ///
    /// Wraps back to `STARTING_GENERATION` after `u8::MAX` rather than reaching
    /// `NEVER_WRITTEN_GENERATION` (`0`) again -- see that constant's own doc comment for why
    /// `0` must stay unreachable here. A wrapped-around generation coinciding with some
    /// genuinely ancient entry's own tag is a real, deliberately accepted risk (see
    /// `threading.md`'s wraparound-frequency correction): at worst a stale entry survives
    /// replacement for one extra cycle, never a wrong answer reaching the search, since
    /// `probe`'s own `key == hash` check guards every real hit regardless of `generation`.
    pub fn new_search(&mut self) {
        self.generation = if self.generation == u8::MAX { STARTING_GENERATION } else { self.generation + 1 };
    }

    /// Looks up `hash`. Returns `None` when the slot has never been written, and also when it
    /// holds a different position that merely happens to share `hash`'s low bits (an index
    /// collision) -- the slot's own stored key (kept internally, never exposed on `Entry`) is
    /// what tells these "nothing usable here" cases apart from an actual match, so a caller
    /// never needs to check anything itself.
    pub fn probe(&self, hash: u64) -> Option<Entry> {
        let slot = &self.entries[(hash & self.mask) as usize];
        let key = slot.key.load(Ordering::Relaxed);
        let (best_move, depth, score, bound, generation) = unpack_entry_data(slot.data.load(Ordering::Relaxed));
        if generation == NEVER_WRITTEN_GENERATION || key != hash {
            return None;
        }
        Some(Entry { best_move, depth, score, bound })
    }

    /// Asks the CPU to start loading the slot `probe(hash)` will read, without waiting for it: a
    /// hint with no effect on what any probe or store sees. Called once a move is made, so the
    /// likely cache miss overlaps the work the search does before the child's probe.
    pub fn prefetch(&self, hash: u64) {
        #[cfg(target_arch = "x86_64")]
        {
            use std::arch::x86_64::{_mm_prefetch, _MM_HINT_T0};
            let slot = &self.entries[(hash & self.mask) as usize];
            unsafe {
                _mm_prefetch::<_MM_HINT_T0>(slot as *const Slot as *const i8);
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = hash;
        }
    }

    /// Records `hash`'s result -- but only if the replacement policy says this write should
    /// win over whatever the slot already holds: a key mismatch (a different position
    /// entirely, or a slot that was never written) always replaces; otherwise only an `Exact`
    /// bound, a strictly greater depth, or a stale generation (from a previous `go`, see
    /// `new_search`) does. A same-position write that improves on none of these is silently
    /// dropped, leaving the existing, at-least-as-good entry in place.
    ///
    /// `&self`, not `&mut self`: everything this method touches (`entries[i]`'s two atomics,
    /// `self.generation`'s plain read) already works through shared access, the same as
    /// `probe`. This is what actually lets multiple worker threads hold and use the same table
    /// concurrently with no `Arc`/`Mutex` at all -- only `new_search`/`clear` genuinely mutate
    /// (`self.generation`, a plain field), and both only ever need to run before any thread
    /// shares this table, never concurrently with one that does.
    pub fn store(&self, hash: u64, best_move: Move, depth: u32, score: i32, bound: Bound) {
        let slot = &self.entries[(hash & self.mask) as usize];
        let existing_key = slot.key.load(Ordering::Relaxed);
        let (_, existing_depth, _, _, existing_generation) = unpack_entry_data(slot.data.load(Ordering::Relaxed));

        // Two-tier, not one flat `||` expression -- per threading.md's own reasoning, a key
        // mismatch is a different *kind* of justification (there's no comparison to make at
        // all; nothing here had a claim to the slot to begin with) than the other three, which
        // genuinely compare new vs. old for the *same* position.
        let should_replace = if existing_key != hash {
            true
        } else {
            bound == Bound::Exact || depth > existing_depth || existing_generation != self.generation
        };
        if !should_replace {
            return;
        }

        slot.key.store(hash, Ordering::Relaxed);
        slot.data.store(pack_entry_data(best_move, depth, score, bound, self.generation), Ordering::Relaxed);
    }

    /// UCI's own `info hashfull` value: how full the table is with entries from the *current*
    /// search, in permille (`0..=1000`) -- confirmed directly against Stockfish's own `tt.cpp`
    /// (`TranspositionTable::hashfull`), which samples only the first 1000 slots rather than
    /// scanning the whole table (a full scan would cost real time on every `info` line, for a
    /// number that's only ever an approximate, human-facing progress indicator) and counts only
    /// entries tagged with the *current* generation, not "ever written" across any past search
    /// -- a stale entry from a previous `go` is already eligible for replacement, so counting it
    /// here would overstate how full the table actually is for the search in progress.
    ///
    /// `NEVER_WRITTEN_GENERATION` (`0`) can never equal `self.generation` (`new_search` only
    /// ever wraps back to `STARTING_GENERATION = 1`, never to `0`), so a plain equality check
    /// against `self.generation` already excludes untouched slots -- no separate "is this
    /// occupied at all" check is needed, unlike Stockfish's own two-part condition (its
    /// generation byte shares bits with the bound, so it needs a mask *and* a nonzero-depth
    /// check where Kalo Chess's own separate, unmasked `generation` field needs neither).
    pub fn hashfull(&self) -> u32 {
        let sampled_slots = self.entries.len().min(1000);
        let occupied_this_generation = self.entries[..sampled_slots]
            .iter()
            .filter(|slot| {
                let (_, _, _, _, generation) = unpack_entry_data(slot.data.load(Ordering::Relaxed));
                generation == self.generation
            })
            .count();
        (occupied_this_generation * 1000 / sampled_slots) as u32
    }

    /// Forgets every stored entry and resets the generation counter, without changing the
    /// table's own size -- for `ucinewgame`: a new game means every remembered position is
    /// from a different game entirely and must not be trusted, but the `Hash` size itself is a
    /// separate, unrelated setting that a fresh game has no reason to disturb. Resetting
    /// `generation` alongside the entries (rather than leaving it to keep climbing across
    /// games) gives every new game its own full generation budget before its first
    /// wraparound, instead of inheriting wear accumulated across however many games were
    /// already played in this process (see `threading.md`'s generation-reset decision).
    pub fn clear(&mut self) {
        for slot in &self.entries {
            slot.key.store(0, Ordering::Relaxed);
            slot.data.store(0, Ordering::Relaxed);
        }
        self.generation = STARTING_GENERATION;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::square::Square;

    fn some_move() -> Move {
        Move::new(Square::E2, Square::E4, None)
    }

    fn other_move() -> Move {
        Move::new(Square::G1, Square::F3, None)
    }

    #[test]
    fn pack_move_round_trips_a_quiet_non_promoting_move() {
        let mv = Move::new(Square::E2, Square::E4, None);
        assert_eq!(unpack_move(pack_move(mv)), mv);
    }

    #[test]
    fn pack_move_round_trips_every_promotion_piece() {
        for piece in [PieceType::Knight, PieceType::Bishop, PieceType::Rook, PieceType::Queen] {
            let mv = Move::new(Square::E7, Square::E8, Some(piece));
            assert_eq!(unpack_move(pack_move(mv)), mv);
        }
    }

    #[test]
    fn pack_move_round_trips_a_double_pawn_push() {
        let mv = Move::double_pawn_push(Square::E2, Square::E4);
        assert_eq!(unpack_move(pack_move(mv)), mv);
    }

    #[test]
    fn pack_move_round_trips_an_en_passant_capture() {
        let mv = Move::en_passant_capture(Square::D5, Square::E6);
        assert_eq!(unpack_move(pack_move(mv)), mv);
    }

    #[test]
    fn pack_move_round_trips_a_castle() {
        let mv = Move::castle(Square::E1, Square::G1);
        assert_eq!(unpack_move(pack_move(mv)), mv);
    }

    #[test]
    fn pack_move_fits_in_15_bits() {
        // `from`/`to` can each reach 63 (Square::H8), the highest values pack_move handles --
        // confirms the layout never spills into the 16th bit that best_move's field width
        // (15, per threading.md) doesn't budget for.
        let mv = Move::castle(Square::H8, Square::A1);
        assert!(pack_move(mv) < (1 << 15));
    }

    #[test]
    fn pack_entry_data_round_trips_a_positive_score() {
        let (mv, depth, score, bound, generation) = unpack_entry_data(pack_entry_data(some_move(), 7, 250, Bound::Lower, 3));
        assert_eq!(mv, some_move());
        assert_eq!(depth, 7);
        assert_eq!(score, 250);
        assert_eq!(bound, Bound::Lower);
        assert_eq!(generation, 3);
    }

    #[test]
    fn pack_entry_data_round_trips_a_negative_score() {
        // The one place sign handling could quietly break: score is stored as reinterpreted
        // unsigned bits, so a negative value round-tripping correctly is what actually proves
        // the i16 sign-extension on unpack, not just the positive case above.
        let (_, _, score, _, _) = unpack_entry_data(pack_entry_data(some_move(), 0, -12_345, Bound::Upper, 0));
        assert_eq!(score, -12_345);
    }

    #[test]
    fn pack_entry_data_round_trips_a_mate_score() {
        let (_, _, score, _, _) =
            unpack_entry_data(pack_entry_data(some_move(), 0, -crate::search::MATE_SCORE, Bound::Exact, 0));
        assert_eq!(score, -crate::search::MATE_SCORE);
    }

    #[test]
    fn pack_entry_data_round_trips_every_bound() {
        for bound in [Bound::Exact, Bound::Lower, Bound::Upper] {
            let (_, _, _, unpacked, _) = unpack_entry_data(pack_entry_data(some_move(), 0, 0, bound, 0));
            assert_eq!(unpacked, bound);
        }
    }

    #[test]
    fn pack_entry_data_round_trips_the_deepest_representable_depth() {
        // 8 bits: 0..=255 is the whole representable range -- threading.md's own justification
        // for the width (matching Stockfish's depth8, not search.rs's own iterative-deepening
        // ceiling of the day) depends on 255 actually being reachable, not silently truncated
        // away.
        let (_, depth, _, _, _) = unpack_entry_data(pack_entry_data(some_move(), 255, 0, Bound::Exact, 0));
        assert_eq!(depth, 255);
    }

    #[test]
    fn pack_entry_data_round_trips_the_highest_representable_generation() {
        // 8 bits: 0..=255 is the whole representable range -- threading.md's own wraparound
        // reasoning depends on 255 actually being reachable, not silently truncated away.
        let (_, _, _, _, generation) = unpack_entry_data(pack_entry_data(some_move(), 0, 0, Bound::Exact, 255));
        assert_eq!(generation, 255);
    }

    #[test]
    fn with_slots_rounds_down_to_a_power_of_two() {
        assert_eq!(TranspositionTable::with_slots(1).entries.len(), 1);
        assert_eq!(TranspositionTable::with_slots(4).entries.len(), 4);
        assert_eq!(TranspositionTable::with_slots(5).entries.len(), 4);
        assert_eq!(TranspositionTable::with_slots(8).entries.len(), 8);
    }

    #[test]
    fn with_slots_never_produces_an_empty_table_even_for_a_zero_request() {
        // `new`'s own `.max(1)` is what actually stops a zero-byte budget from reaching here,
        // but `with_slots` guards the same case independently -- `ilog2(0)` is undefined, so
        // this is what stands between a degenerate caller and a panic, not just belt-and-braces.
        assert_eq!(TranspositionTable::with_slots(0).entries.len(), 1);
    }

    #[test]
    fn new_sizes_the_table_from_a_megabyte_budget() {
        let slot_bytes = std::mem::size_of::<Slot>();
        let table = TranspositionTable::new(1);
        let budget_slots = 1024 * 1024 / slot_bytes;
        assert!(table.entries.len().is_power_of_two());
        // The largest power of two not exceeding the budget -- doubling it must overshoot.
        assert!(table.entries.len() <= budget_slots);
        assert!(table.entries.len() * 2 > budget_slots);
    }

    #[test]
    fn slot_is_exactly_sixteen_bytes() {
        // Two AtomicU64s, back to back: the standard library guarantees each has the same
        // size and alignment as u64, so this is 8 + 8 with no padding between them -- checked
        // directly rather than just asserted in prose, since a future field added to Slot
        // could silently change it.
        assert_eq!(std::mem::size_of::<Slot>(), 16);
    }

    #[test]
    fn slots_reports_the_actual_power_of_two_size() {
        assert_eq!(TranspositionTable::with_slots(8).slots(), 8);
        assert_eq!(TranspositionTable::with_slots(5).slots(), 4);
    }

    #[test]
    fn probe_returns_none_for_a_hash_never_stored() {
        let table = TranspositionTable::with_slots(8);
        assert!(table.probe(42).is_none());
    }

    #[test]
    fn store_then_probe_returns_what_was_stored() {
        let table = TranspositionTable::with_slots(8);
        table.store(42, some_move(), 3, 100, Bound::Exact);

        let entry = table.probe(42).expect("just stored this hash");
        assert_eq!(entry.best_move(), some_move());
        assert_eq!(entry.depth(), 3);
        assert_eq!(entry.score(), 100);
        assert_eq!(entry.bound(), Bound::Exact);
    }

    #[test]
    fn probe_returns_none_on_an_index_collision() {
        // A 2-slot table masks every hash down to its lowest bit, so 0 and 2 land in the same
        // slot despite being different positions.
        let table = TranspositionTable::with_slots(2);
        table.store(0, some_move(), 1, 10, Bound::Exact);
        assert!(table.probe(2).is_none());
    }

    #[test]
    fn probe_returns_none_for_hash_zero_on_an_empty_table() {
        // The one edge case the generation sentinel exists to guard, since key alone can't: an
        // empty slot's own key is also 0, so a naive key-only "is this occupied" check would
        // wrongly treat hash 0 as a match against an empty slot.
        let table = TranspositionTable::with_slots(8);
        assert!(table.probe(0).is_none());
    }

    #[test]
    fn a_freshly_built_table_has_every_slot_at_the_never_written_generation() {
        let table = TranspositionTable::with_slots(8);
        for slot in &table.entries {
            let (_, _, _, _, generation) = unpack_entry_data(slot.data.load(Ordering::Relaxed));
            assert_eq!(generation, NEVER_WRITTEN_GENERATION);
        }
    }

    #[test]
    fn store_replaces_an_index_collision_regardless_of_depth_or_bound() {
        let table = TranspositionTable::with_slots(2);
        table.store(0, some_move(), 5, 10, Bound::Exact);
        // Same slot as hash 0 (see the collision test above), but a shallower search and a
        // different move -- a key mismatch always wins regardless, per the replacement
        // policy's first tier: there's no comparison to make when the slot never belonged to
        // this position to begin with.
        table.store(2, other_move(), 1, -10, Bound::Upper);

        assert!(table.probe(0).is_none());
        let entry = table.probe(2).expect("just stored this hash");
        assert_eq!(entry.best_move(), other_move());
        assert_eq!(entry.depth(), 1);
    }

    #[test]
    fn store_refuses_to_replace_a_same_position_entry_that_improves_on_nothing() {
        let table = TranspositionTable::with_slots(8);
        table.store(42, some_move(), 5, 100, Bound::Lower);
        // Same position, shallower depth, not Exact, same generation -- none of the
        // replacement conditions hold, so the stronger existing entry must survive.
        table.store(42, other_move(), 2, -50, Bound::Upper);

        let entry = table.probe(42).expect("still occupied");
        assert_eq!(entry.best_move(), some_move());
        assert_eq!(entry.depth(), 5);
        assert_eq!(entry.score(), 100);
        assert_eq!(entry.bound(), Bound::Lower);
    }

    #[test]
    fn store_replaces_a_same_position_entry_when_the_new_bound_is_exact() {
        let table = TranspositionTable::with_slots(8);
        table.store(42, some_move(), 5, 100, Bound::Lower);
        // Shallower depth, but Exact always wins -- the strongest bound is always worth
        // keeping regardless of depth.
        table.store(42, other_move(), 1, -50, Bound::Exact);

        let entry = table.probe(42).expect("still occupied");
        assert_eq!(entry.best_move(), other_move());
        assert_eq!(entry.bound(), Bound::Exact);
    }

    #[test]
    fn store_replaces_a_same_position_entry_when_the_new_depth_is_strictly_greater() {
        let table = TranspositionTable::with_slots(8);
        table.store(42, some_move(), 3, 100, Bound::Lower);
        table.store(42, other_move(), 4, -50, Bound::Upper);

        let entry = table.probe(42).expect("still occupied");
        assert_eq!(entry.best_move(), other_move());
        assert_eq!(entry.depth(), 4);
    }

    #[test]
    fn store_does_not_replace_a_same_position_entry_on_an_equal_depth_tie() {
        // Strict inequality: a tie keeps the existing entry rather than replacing it.
        let table = TranspositionTable::with_slots(8);
        table.store(42, some_move(), 3, 100, Bound::Lower);
        table.store(42, other_move(), 3, -50, Bound::Upper);

        let entry = table.probe(42).expect("still occupied");
        assert_eq!(entry.best_move(), some_move());
    }

    #[test]
    fn store_replaces_a_stale_generation_entry_even_at_a_shallower_depth() {
        let mut table = TranspositionTable::with_slots(8);
        table.store(42, some_move(), 5, 100, Bound::Lower);
        table.new_search(); // a new go begins: the entry above now belongs to a previous search
        table.store(42, other_move(), 1, -50, Bound::Upper);

        let entry = table.probe(42).expect("still occupied");
        assert_eq!(entry.best_move(), other_move());
        assert_eq!(entry.depth(), 1);
    }

    #[test]
    fn new_search_bumps_the_generation_by_one() {
        let mut table = TranspositionTable::with_slots(8);
        assert_eq!(table.generation, STARTING_GENERATION);
        table.new_search();
        assert_eq!(table.generation, STARTING_GENERATION + 1);
    }

    #[test]
    fn new_search_wraps_back_to_the_starting_generation_after_the_maximum() {
        let mut table = TranspositionTable::with_slots(8);
        table.generation = u8::MAX;
        table.new_search();
        // Must land back on STARTING_GENERATION, not NEVER_WRITTEN_GENERATION (0) -- the whole
        // point of wrapping to 1 instead of 0 is keeping the empty-slot sentinel unreachable
        // once the table is in use.
        assert_eq!(table.generation, STARTING_GENERATION);
    }

    #[test]
    fn clear_resets_the_generation_to_the_starting_value() {
        let mut table = TranspositionTable::with_slots(8);
        table.new_search();
        table.new_search();
        table.clear();
        assert_eq!(table.generation, STARTING_GENERATION);
    }

    #[test]
    fn hashfull_is_zero_for_a_freshly_built_table() {
        let table = TranspositionTable::with_slots(8);
        assert_eq!(table.hashfull(), 0);
    }

    #[test]
    fn hashfull_reflects_the_fraction_of_sampled_slots_written_this_generation() {
        // 8 slots total, none colliding at this size (distinct low 3 bits) -- 4 stored is
        // exactly half, so hashfull should read 500 (permille), not some other rounding.
        let table = TranspositionTable::with_slots(8);
        for hash in [0u64, 1, 2, 3] {
            table.store(hash, some_move(), 1, 0, Bound::Exact);
        }
        assert_eq!(table.hashfull(), 500);
    }

    #[test]
    fn hashfull_is_1000_when_every_sampled_slot_is_written_this_generation() {
        let table = TranspositionTable::with_slots(8);
        for hash in 0u64..8 {
            table.store(hash, some_move(), 1, 0, Bound::Exact);
        }
        assert_eq!(table.hashfull(), 1000);
    }

    #[test]
    fn hashfull_excludes_entries_from_a_previous_generation() {
        // A full table from generation 1, then a new search begins (generation 2) without
        // storing anything new yet -- every existing entry is now "stale," so hashfull must
        // report the table as empty *for this search*, not still full from the last one.
        let mut table = TranspositionTable::with_slots(8);
        for hash in 0u64..8 {
            table.store(hash, some_move(), 1, 0, Bound::Exact);
        }
        table.new_search();
        assert_eq!(table.hashfull(), 0);
    }

    #[test]
    fn hashfull_samples_only_the_first_1000_slots_of_a_larger_table() {
        // 2048 slots, only the first 100 written -- if hashfull scanned the whole table, this
        // would read roughly 49 (100/2048 permille); sampling only the first 1000 gives 100
        // instead (100/1000 permille), matching Stockfish's own real, fixed sampling window.
        let table = TranspositionTable::with_slots(2048);
        for hash in 0u64..100 {
            table.store(hash, some_move(), 1, 0, Bound::Exact);
        }
        assert_eq!(table.hashfull(), 100);
    }

    #[test]
    fn clear_forgets_every_entry_without_changing_the_size() {
        let mut table = TranspositionTable::with_slots(8);
        table.store(42, some_move(), 3, 100, Bound::Exact);
        table.clear();

        assert!(table.probe(42).is_none());
        assert_eq!(table.entries.len(), 8);
    }
}
