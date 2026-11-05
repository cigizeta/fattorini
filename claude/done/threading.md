# Multi-threaded search

## Design

Per the developer's own roadmap (see `tt.md`, now done): after a transposition table, the
next step is multi-threaded search. This document captures that design discussion as it
happens -- see the "Progress log" section for exactly what's settled and what's next.

### Threading model: Lazy SMP, shared transposition table

The real, modern standard (Stockfish's own approach, confirmed against
[the Chess Programming Wiki's Lazy SMP page](https://www.chessprogramming.org/Lazy_SMP)):
every worker thread runs the same full search on the same position, all sharing one
transposition table. The wiki page describes the shared table as the actual mechanism, not
an add-on to it -- threads "profit from probing hash entries written by other instances of
the search."

The alternative considered and rejected: **root-move splitting** (dividing the root's legal
moves across threads, each searching its own slice with a fully private TT/killers/history,
merging scores at the end). A real historical technique, and simpler -- no shared mutable
state at all, so no thread-safety design needed. Rejected because it doesn't help when there
are few legal moves to split (forced lines, check evasions, endgames), exactly the case Lazy
SMP's shared table still helps with (every thread can still usefully dig into the one
remaining line together).

### Scope: synchronous multi-threaded search only, no async `go`/`stop`

`go` spawns N worker threads, joins all of them, then replies -- the same blocking shape
`uci.rs::run()` already has today, just more search per `go`. Real `stop` (an async
background-thread search the main loop can interrupt mid-flight) is a separate, related but
distinct restructuring of `uci.rs`'s whole interface layer, not bundled into this work --
the developer's own explicit call. Tracked as its own future item in `TODO.md`, not here.

### `Threads` UCI option: real convention, not the actual spec

Checked directly against
[the real UCI protocol specification text](https://backscattering.de/chess/uci/): `Threads`
is **not** one of the spec's own enumerated predefined options (that list is `Hash`,
`NalimovPath`, `NalimovCache`, `Ponder`, `OwnBook`, `MultiPV`, and the `UCI_*`-prefixed ones
-- the spec predates multi-threaded engines being the norm). `Threads` is a de facto
convention every multi-threaded UCI engine has independently converged on regardless,
because GUIs (Arena, CuteChess, Banksia, etc.) look for that literal option name to expose a
thread-count control. Confirmed against the developer's own real Stockfish 17.1 binary
(`echo "uci" | stockfish`): `option name Threads type spin default 1 min 1 max 1024`.

Rotor follows the name (for GUI compatibility) but not the max: `1024` reflects Stockfish's
own willingness to run on server/cluster-class hardware, which CLAUDE.md's "one commodity
machine" scope explicitly excludes. Deliberately not tied to the developer's own current
12-core machine either (`nproc` = 12) -- that's today's specific hardware, not a property of
"a single machine" as a category. Settled: **default `1`, min `1`, max `128`** -- generous
enough to cover even a 64-core/128-thread single-socket HEDT chip (Threadripper-class) with
headroom, without reaching into server/datacenter territory.

Same override mechanism as `Hash`, confirmed explicitly rather than left implicit: Rotor
starts at the default (`1` thread, identical behavior to today's single-threaded search) and
only switches to N threads once a real `setoption name Threads value <N>` actually asks for
it -- no separate mechanism to design here, just `UciCommand::SetOptionHash`'s existing
pattern applied a second time (`parse_setoption` already anticipated a second option needing
this treatment, in its own doc comment).

### Prerequisite discovered: aspiration windows don't exist in Rotor yet

Stockfish's actual, verified thread-diversity mechanism (`search.cpp`):

```cpp
delta = 5 + threadIdx % 8 + std::abs(rootMoves[pvIdx].meanSquaredScore) / 10193;
Value avg = rootMoves[pvIdx].averageScore;
alpha     = std::max(avg - delta, -VALUE_INFINITE);
beta      = std::min(avg + delta, VALUE_INFINITE);
```

`threadIdx % 8` gives different threads different aspiration-window widths for nominally the
same position/depth. A wider vs. narrower window hits fail-high/fail-low at different points,
so threads visit genuinely different nodes and write genuinely different entries into the
shared table -- that's the actual source of useful diversity, not injected randomness.

Two other claims about Stockfish's diversity mechanism were floated during this discussion
and turned out to be wrong on checking the real source, corrected in place rather than left
standing:
- **No per-thread depth skipping.** `search.cpp`'s outer loop increments one shared
  `rootDepth` (`while (rootDepth + 1 < MAX_PLY && ...) { rootDepth++; ...}`); every thread
  runs the identical loop.
- **No per-thread root move reordering.** `thread.cpp` assigns the identical root move list
  to every thread unconditionally (`th->worker->rootMoves = rootMoves;`), no shuffling or
  thread-index-based variation.

The real problem this creates for Rotor: `search_root` currently calls
`alpha_beta_with_buffer(board, 0, depth, -INFINITY, INFINITY)` -- full window, every depth,
always. There is no window width to vary per thread, because there is no narrowed window at
all yet. Without some diversity mechanism, N threads running the identical deterministic
full-window search on the identical position would explore in lockstep and provide
approximately zero benefit over one thread. **Aspiration windows have to exist first**, both
as a real, independently valuable search improvement on their own, and as the substrate
Stockfish's own real diversity trick depends on. This is the very next design topic.

### Thread diversity: per-thread aspiration-window delta

A real gap surfaced only once implementation reached the thread-spawning piece, not caught
during the design phase itself: aspiration windows landed (`aspiration-windows.md`, in `done/`)
as a genuine single-threaded search improvement, but nothing was ever added to vary the window
*per thread* -- exactly the mechanism this document's own "Prerequisite discovered" section
above says aspiration windows exist to enable. Every function in the aspiration path
(`search_root`, `choose_best_move`) computes the identical `delta` for the identical
position/depth regardless of which thread calls it. Spawning threads on top of this as-is would
reproduce the exact lockstep failure mode the prerequisite section warned about.

**`meanSquaredScore`, checked and excluded.** Verified directly against Stockfish's current
`search.h`/`search.cpp` (an earlier passing mention of this term in conversation wasn't already
confirmed against real source, so it was checked before use here): it's a per-root-move
exponential moving average of that move's own score, signed-squared (`value * abs(value)`) to
grow quadratically with magnitude while keeping sign, with an adaptive weight itself derived
from how many nodes were just spent on that move relative to its historical "effort." It feeds
a *separate* term in Stockfish's real formula (`delta = 5 + threadIdx % 8 +
abs(meanSquaredScore) / 10193`) that widens the window based on a move's own score volatility --
identical across every thread examining the same move (`thread.cpp` assigns the identical
`rootMoves` list to every worker, already confirmed above), since it isn't a function of
`threadIdx` at all. A real, separate refinement to aspiration windows generally (Rotor's own
`search_root` only ever sees one plain `previous_score`, not a persisted per-root-move EMA
across iterations), but orthogonal to the thread-diversity gap this section is actually about --
excluded from this decision rather than folded in.

**Settled formula:** `delta(thread_idx) = ASPIRATION_INITIAL_DELTA + STEP * (thread_idx % 8)`,
with `STEP = 5`. Thread `0` keeps exactly today's proven `ASPIRATION_INITIAL_DELTA` unchanged as
its baseline; each higher thread index up to `7` widens the window by another `5` centipawns,
then wraps -- threads `8`-`11` (for a 12-thread example) exactly repeat threads `0`-`3`'s deltas,
saturating at 8 distinct widths no matter how many threads run, the same behavior Stockfish's own
real formula has at any thread count.

`STEP = 5` (rather than Stockfish's implicit `1`, i.e. `threadIdx % 8` added unscaled) is
reasoned, not measured: Rotor's own hand-crafted evaluation (material/PST/mobility/pawn
structure/king safety, all cheap integer arithmetic) is coarser-grained than Stockfish's NNUE, so
a step as small as `1` centipawn wouldn't reliably push different threads to fail high/low at
different points -- the eval's own granularity would swallow it. This is the same kind of claim
as `ASPIRATION_INITIAL_DELTA` itself, and is recorded with the same status: a directionally-
justified starting value, not yet validated, revisit once the self-play/strength-testing harness
(still not built, see `TODO.md`) can actually measure whether threads end up behaving differently
enough to matter.

The `% 8` saturation itself was a real choice, not an unexamined carry-over from Stockfish: at
Rotor's own settled `Threads` ceiling of `128`, linear growth with no wraparound
(`delta(127) = ASPIRATION_INITIAL_DELTA + 5*127`, comfortably under 3% of `MATE_SCORE = 30_000`
regardless of `ASPIRATION_INITIAL_DELTA`'s own current value) would never actually degrade toward
a full-window search the way it eventually would on Stockfish's much larger thread ceilings -- so
Rotor doesn't strictly *need* the saturation trick the way Stockfish does. Kept anyway, as a
matter of principle: matching Stockfish's real formula shape where nothing about Rotor's own
situation demands deviating from it, rather than diverging just because the lower thread ceiling
happens to make either choice survivable.

**`ASPIRATION_INITIAL_DELTA` itself was revisited and lowered, `50` to `20`, after this section
was already settled.** Working through the window's own shape concretely (`[center - delta,
center + delta]`) surfaced what `50` actually means stated plainly: a *total* window of
`2 * 50 = 100` centipawns -- a full pawn's worth of slack -- which is more margin than there's
any real reason to open with. `20` (a `40`-centipawn total window) reverts to the same older
Stockfish reference value `ASPIRATION_INITIAL_DELTA`'s own doc comment already cited, undoing the
earlier, since-reversed reasoning for widening past it. Still the same epistemic status as
before: a directionally-reasoned placeholder, not measured, revisit once real depth-to-depth
score data or a self-play strength-testing harness can actually calibrate it.

### TT concurrency: lock-free, two atomics per entry, no checksum

The underlying problem, argued from first principles before picking a mechanism: `probe`
takes `&self`, `store` takes `&mut self` -- Rust's exclusivity rules mean "many threads, one
shared `&mut TranspositionTable`" doesn't compile at all, full stop. Some form of interior
mutability is mandatory before Lazy SMP can exist as compiling Rust, not an optional
hardening step. Without any protection at all, two threads writing the same slot
concurrently could produce a *torn* entry -- some words from one write, some from another
(e.g. one position's key paired with a different position's score) -- silently wrong data
masquerading as a valid hit, not a crash.

**Correction, caught and fixed mid-discussion:** an earlier version of this document claimed
Stockfish's real technique is a `key XOR data` checksum (the classic Hyatt/Crafty "lockless
hashing" trick). Checked directly against Stockfish's actual current `tt.h`/`tt.cpp` rather
than trust that claim, and it's wrong -- there is no checksum anywhere in real Stockfish's TT.
Its actual `TTEntry` is six independent fields, each its own small atomic
(`RelaxedAtomic<u16> key16`, `RelaxedAtomic<u8> depth8`, `RelaxedAtomic<u8> genBound8`,
`RelaxedAtomic<Move> move16`, `RelaxedAtomic<i16> value16`, `RelaxedAtomic<i16> eval16`),
each read and written completely independently with no cross-field guarantee at all. The
source says so explicitly: "This gap is the main place for read races. After `read()`
completes that copy is final, but may be self-inconsistent." Stockfish ships this
deliberately, at far higher node throughput than Rotor will ever reach, validated by
`fishtest`'s SPRT self-play testing over years -- not because torn reads can't happen, but
because they're rare enough and cheap enough when they do happen that paying for a checksum
on every probe and store isn't worth it in practice.

**What forcing atomicity without a lock actually means, mechanically:** a naturally-aligned
load or store of up to the machine's native word size (8 bytes on any 64-bit target this
project runs on) is already indivisible at the hardware memory-bus level -- no thread can ever
observe a value that's half the old bytes and half the new ones within that *one* word. Rust's
`AtomicU64`/`AtomicU32`/etc. (`Ordering::Relaxed`) mostly just stop the compiler from breaking
that guarantee (splitting the access, reordering it); the indivisibility itself is a physical
property of the CPU, not something Rust or a lock imposes. A lock is a completely different
mechanism: it serializes access to an arbitrarily large critical section via its own atomic
flag underneath, at the cost of threads actually blocking. A bare atomic protects exactly one
word, instantaneously, and nobody ever waits -- but two *separate* atomic fields read at
"the same time" can still each be individually torn-free while disagreeing with each other,
since nothing ties the two reads together as having happened at the same instant. That's
the real, narrower risk once a lock is off the table -- not "corrupted bits within a field,"
but "two genuinely different fields, each internally fine, paired together wrong."

**Quantifying how often that risk actually fires, rather than asserting "rare":** modeled as
a Poisson process (all threads' stores combined, rate `R`/sec, uniformly hitting one of `N`
slots), the expected number of same-slot near-simultaneous store collisions over a search of
length `T` seconds is `R^2 * dt * T / N`, where `dt` is the write's own overlap window. For a 1
GB table (`N ~= 2^25 ~= 33.5 million slots` for any plausible `Entry` byte size, since the
power-of-two rounding lands in the same bracket regardless), the developer's real 12-core
machine, a 5-second search, and a 50ns `dt`: at `R = 1,000,000` combined stores/sec the answer
is under 1% chance of even one collision in the whole search; at `R = 10,000,000` it's ~75%.
That's a two-order-of-magnitude swing driven entirely by Rotor's own combined store
throughput, a number that doesn't exist yet (the same unmeasured NPS hypothesis already
flagged in `TODO.md`) -- this is a real case where CLAUDE.md rule 5 applies directly: the
actual collision rate is something to measure once threading exists, not settle by reasoning
about which illustrative `R` feels right.

**Severity isn't uniform across bound kinds, and that mattered to the decision.** Checked
Rotor's own probe logic (`alpha_beta_with_buffer`) against Stockfish's real cutoff condition
in `search.cpp`: a `Lower` or `Upper` hit is only trusted if the stored score actually
satisfies the *current* node's own `beta`/`alpha` -- a torn `Lower`/`Upper` entry has a real
chance of simply failing that check and being discarded as unhelpful, an accidental
self-correction. An `Exact` hit has no such check, in Rotor or in real Stockfish -- it returns
immediately as the position's proven value, and a corrupted `Exact` hit propagates upward with
nothing downstream ever positioned to catch it. Real Stockfish accepts exactly this
concentrated risk today; Rotor doing the same is a real, informed precedent, not a shortcut.

**Settled: two atomics per `Entry`, `key: AtomicU64` and one packed `AtomicU64` holding
everything else (`best_move`/`depth`/`score`/`bound` together) -- no `Mutex`, no checksum.**
This lands between Stockfish's six-independent-atomics approach and a full checksum, and it
does more than just "fewer atomics, fewer chances": the two contention patterns that can hit
one slot behave completely differently under it.
- **Two threads storing the *same* transposing position concurrently** -- Lazy SMP's actual
  normal case, the whole reason the table is shared -- never changes `key` between the two
  writes at all; the only prior danger was `depth`/`score`/`bound`/`move` tearing against each
  other (up to `2^4 = 16` inconsistent combinations across four independent atomics). Packing
  those four into one atomic word **eliminates this failure mode completely**: a reader gets
  either the fully-old write or the fully-new write of that word, nothing in between, the same
  hardware guarantee that makes any single atomic load indivisible.
- **Two threads storing genuinely different, index-colliding positions** -- here `key` does
  change between writes, so a reader could still see an old key paired with a new packed word
  or vice versa. But this only matters if the probing call's own target hash happens to equal
  one of those two exact key values at the moment of the tear -- otherwise `probe`'s ordinary
  `entry.key == hash` check rejects it exactly as it already rejects any ordinary index
  collision. This residual risk rides on a check `probe` needs regardless, not a new
  corruption class needing new defenses.

### Packed `Entry` layout: bit widths, and where the packing logic lives

Reasoned bit-widths, from Rotor's own real value ranges rather than borrowed from Stockfish's:

| Field | Bits | Why |
|---|---|---|
| `key` | 64 | Stays whole, its own atomic -- truncating it is exactly what would make an index collision indistinguishable from a real match. |
| `best_move` | 15 | 6 `from` + 6 `to` + 3 combined kind-or-promotion (see below). |
| `depth` | 8 | See below -- widened from an initial `4` once implementation exposed the reasoning as too narrow. |
| `score` | 16 | Bounded by `MATE_SCORE = 30_000`, fits an `i16` exactly, the same width Stockfish's own `value16` uses for the same reason. |
| `bound` | 2 | Three variants (`Exact`/`Lower`/`Upper`) need `ceil(log2(3)) = 2` bits. |
| `generation` | 8 | See below -- a gap in this table discovered only once implementation reached it. |

Everything but `key` totals `15 + 8 + 16 + 2 + 8 = 49` bits, comfortably under the second `u64`.

**`depth`'s width, revisited during implementation.** The original `4` bits was sized directly
from today's `MAX_ITERATIVE_DEPTH = 10` -- but that constant is exactly the kind of thing this
project expects to grow as search improves (deeper iterative deepening, extensions), the same
way `MAX_PLY` itself (`20` today) is expected to. Baking a field's width to "however large the
current constant happens to be" would have made `depth` the bottleneck the moment
`MAX_ITERATIVE_DEPTH` grew past `15`, the exact kind of unjustified narrowness the dropped
Stockfish `4`-ply tolerance constant was rejected for earlier in this document. Caught by the
developer directly (not something implementation would have caught on its own), rather than
silently shipped and revisited later.

Settled: **8 bits**, matching real precedent rather than a fresh guess -- Stockfish's own
`TTEntry::depth8` field is a full `u8`. `255` is far past any plausible Rotor iterative-deepening
depth for the foreseeable future, and the extra 4 bits (versus the originally proposed 4-bit
field) cost nothing given how roomy the packed word already is. Two alternatives were
considered and declined: `6` bits (`0..=63`, comfortably covers strong-engine-scale depths
without going as far as a full byte, but saves nothing meaningful given the free bit budget) and
tying the width to `MAX_PLY`'s own bit-width via a const expression (would auto-grow if `MAX_PLY`
does, but adds a layer of indirection for a problem `8` fixed bits already solves with headroom
to spare).

**Gap caught during implementation, not before:** this table originally stopped at `bound`,
even though the replacement policy settled above (`existing.generation != current_generation`)
depends on every `Entry` carrying its own generation -- there was simply nowhere in the packed
word for it. Caught once the packing code actually needed a field the table never budgeted,
rather than at design time. Settled: **8 bits**, giving 256 raw values, one of which (`0`) is
separately reserved as the empty-slot sentinel (see below) -- leaving 255 real generation values,
wrapping back to `1` after the 255th. Plenty of headroom over Stockfish's own real convention
(generation and bound share one byte in `genBound8`, leaving ~6 generation bits there). 16 bits
(wraparound practically unreachable) was considered and rejected as an ungrounded choice in the
other direction -- picking it "because it's free" isn't a different kind of justification than
picking any other number without a reason, per CLAUDE.md rule 5.

**Wraparound's actual frequency, corrected:** an earlier version of this document framed
wraparound as "after 256 `go` calls in one process lifetime" -- phrasing that reads as a rare,
almost-never event. Caught as misleading once the developer worked the actual arithmetic: at
one bump per `go`, the 255 usable values wrap every **255 of Rotor's own moves**, a threshold an
ordinarily long game can reach on its own (500 total plies puts Rotor's own move count at or past
that cap, depending on which side it played), not something requiring "a process lifetime" of
many games. The underlying safety argument is unchanged -- `probe`'s `key == hash` check is what
actually guarantees a returned entry belongs to the requested position, so a wrapped generation
can never produce a wrong answer, only a missed opportunity to evict one genuinely stale entry
for one extra cycle -- but the frequency claim needed correcting, not just the safety claim.

**Empty-slot sentinel: `generation == 0`, reserved rather than checking `key == 0`.** Replacing
`Vec<Option<Entry>>` with raw atomics removes `Option`'s free "nothing written here yet" signal,
so something in the raw bits has to mean it instead. Checked Stockfish's real technique rather
than invent one: `TTEntry::is_occupied()` checks `bool(depth8)`, a *data*-field sentinel, not the
key. That's deliberate on Stockfish's part, not incidental -- `key` is a uniformly-distributed
hash across the full 64-bit space including zero, so a real position could in principle hash to
exactly `0`; treating `key == 0` as "empty" would make that one real position silently
unstorable forever. `generation` is Rotor's own equivalent: a table-internal bookkeeping counter,
never a fact about the position, so reserving `0` to mean "never written since the table was
built or last `clear()`'d" and starting real per-`go` bumps at `1` (wrapping through `1..=255`,
skipping `0`) costs nothing but one generation value out of 256 -- smaller than the wraparound
cost already accepted above. A dedicated occupied bit in the packed word's remaining spare bits
was considered and declined: it would spend a bit on a fact `generation == 0` already gives for
free.

**`clear()` (i.e. `ucinewgame`) resets the generation counter too, not just the entries.**
`ucinewgame` already means "forget everything, start fresh" for the entries themselves; resetting
generation alongside them keeps that consistent, and gives every new game its own full 255-move
budget before its first wraparound rather than inheriting wear accumulated across however many
prior games were played in the same process/GUI session. Leaving generation to climb
indefinitely across games was considered and declined -- it would make wraparound creep closer
with every game played in a long-running session, a real behavioral difference from a fresh
per-game budget, not merely an implementation detail neither choice would notice.

**`Move`'s packed encoding exploits its own constructors, not an arbitrary new scheme.** Every
`Move` constructor documents that `promotion` is only ever `Some` alongside
`MoveKind::Quiet`, and every other `MoveKind` always carries `promotion: None` -- the two
fields are mutually exclusive by construction, so one 3-bit field covering both needs only as
many codes as real combinations exist (5 promotion states + 3 non-quiet kinds = 8, exactly
`2^3`). The promotion codes `1`-`4` aren't an arbitrary new numbering either -- they're exactly
`PieceType::Knight as u8` through `PieceType::Queen as u8`, since `PieceType`'s own declared
order already places the four promotable pieces contiguously between the two that never
promote (`Pawn = 0`, `King = 5`). `Square` already has a `#[repr(u8)]` plus `TryFromPrimitive`
(`square as u8` / `Square::try_from(byte)` both already exist and are already used elsewhere),
so `from`/`to` need no new conversion at all.

**Settled: this packing logic lives entirely in `tt.rs`, as private free functions, not as
methods on `Move` itself.** `Move` already exposes everything a packer needs through its
existing public API (`from()`, `to()`, `promotion()`, `kind()`, and the four constructors to
unpack back into) -- nothing new needs to be added to `chess_move.rs`, an already-stable,
already-tested file. The packing need is entirely `tt.rs`'s own (fitting a `Move` inside a
shared atomic word), not a general `Move` capability anything else in Rotor wants today; adding
`pack`/`unpack` methods to `Move` for the benefit of one specific caller would be exactly the
kind of speculative generality this project already avoids elsewhere (the dropped replacement-
policy tolerance constant, the deferred `[from][to]` history table). The one consequence:
`tt.rs` gains its own `use` of `MoveKind` and `PieceType`, and the "promotion is only ever
`Some` alongside `Quiet`" reasoning lives as a doc comment on `tt.rs`'s own packing helper,
referencing `Move`'s publicly documented constructor behavior rather than its private fields.

### Replacement policy: "always replace" is no longer acceptable

Verified directly against Stockfish's real `tt.cpp`, rather than assumed: even Stockfish's
own *single-threaded* replacement policy was never "always replace" --

```cpp
void TTEntry::save(Key k, Value v, bool pv, Bound b, Depth d, Move m, Value ev, u8 curr_generation)
...
if (b == BOUND_EXACT || u16(k) != key16 || d - DEPTH_NONE + 2 * pv > depth8 - 4
    || relative_age(curr_generation))
```

(`depth8` is the struct field holding the *existing* stored, encoded depth; `d`/`pv`/`b` are
the incoming write's parameters; `DEPTH_NONE = -3`, `DEPTH_UNSEARCHED = -2`, `DEPTH_QS = 0`,
confirmed in `types.h`.)

Decoded: overwrite if the new value is `Exact` (strongest bound, always worth keeping), or
the slot actually holds a different position (`key16` mismatch -- a real collision), or the
new depth (with a small tolerance, and a PV-node result counted as 2 plies deeper than it
literally is, since a PV result is worth more) exceeds the old one, or the old entry is
simply stale (`relative_age`, a different search generation entirely).

Two things confirmed about that formula that matter for adapting it:
- The `- DEPTH_NONE` term cancels out algebraically once both sides are decoded (it's purely
  a byte-packing artifact of storing `depth` in a `u8`), leaving a real, unit-invariant
  condition: `d + 2*pv > old_depth - 4`. The `4`-ply tolerance is a genuine, meaningful
  constant in real ply units, not an implementation detail -- but tuned for Stockfish's own
  routine depths (20-40+ plies), not Rotor's (`MAX_ITERATIVE_DEPTH = 10`). Borrowing `4`
  unchanged would mean "anything within 40% of Rotor's entire depth range still overwrites,"
  which isn't a small grace window at Rotor's scale, it's most of the comparison being
  meaningless. **Dropped for Rotor's first landing** rather than replaced with a smaller but
  equally unjustified guess -- the one real practical reason tolerance exists (letting a
  fresher-but-shallower write through) is already covered by the separate generation-staleness
  branch, and any further softening should come from actual measurement, not a guess.
- The `2 * pv` bonus assumes a PV/non-PV node distinction, which comes from **PVS**
  (principal variation search: every move but the first searched with a cheap null window,
  re-searched with a full window only if it fails high) -- infrastructure Rotor doesn't have.
  `alpha_beta_with_buffer` currently gives every child the same full `(-beta, -alpha)`
  window; there is no "was this a PV node" fact to feed the term honestly. **Dropped**,
  revisit once/if PVS exists.

Rotor's adapted condition, settled:

```
if existing.key != new.key {
    true  // a different, unrelated position landed in this slot -- no comparison applies,
          // nothing here had a claim to the slot to begin with, just take it
} else {
    new.bound == Bound::Exact
        || new.depth > existing.depth   // strict -- a tie keeps the existing entry
        || existing.generation != current_generation
}
```

Written as this explicit two-tier `if`/`else`, not as one flat boolean expression the way
Stockfish's source reads -- the key check is a different *kind* of justification than the
other three (which genuinely compare new vs. old for the *same* position; the key check says
there's no comparison to make at all, so writing it as a separate first branch says what's
actually happening instead of relying on `||`'s short-circuiting to paper over two different
kinds of reasoning in one line.

### Generation bump timing: once per `go`, shared across all worker threads

Bump the table's generation counter once per `go` -- i.e. once per real search, before any
worker thread starts -- not once per thread and not once per depth. Threads share one TT
specifically so each can profit from the others' fresh writes; if a thread bumped its own
generation the instant it started, it would treat every other thread's entries from that same
`go`, written moments earlier for the same search, as already stale. That throws away exactly
the cross-thread benefit Lazy SMP exists for. One bump per `go`, visible to every thread, is
the only shape consistent with "generation" meaning "which search this entry belongs to," not
"which thread wrote it."

### Result selection: deepest completed depth wins, then raw score

Checked Stockfish's real `ThreadPool::get_best_thread()` (`thread.cpp`) rather than assume a
simple rule exists. It doesn't compare depth at all: every thread casts a "vote" for its own
top move, weighted by how far above the pool's *worst* thread's score it sits
(`score - minScore + 14`), so several threads independently agreeing on one move outweighs a
lone dissenting thread even if that thread's own nominal depth is one ply deeper. A separate
special case (`is_decisive`/`is_win`/`is_loss`) then overrides the vote entirely once either
thread reports a forced mate, preferring the *shorter* mate between two mate-reporting
threads.

Not adopted for Rotor's first landing. The vote system is a hedge against threads genuinely
disagreeing, which only pays for itself statistically with the double-digit thread counts
Stockfish routinely runs -- building that hedge now would mean guessing at a problem with no
evidence yet that it's real at Rotor's own (likely much smaller, single-machine) thread
counts. It also needs infrastructure Rotor doesn't have: a `RootMove` list with `pv.size()`
and an inexactness flag tracked per root move, not the single flat `(pv, score)` pair
`search_root` returns today.

The mate-distance special case turns out to need no separate handling in Rotor at all, rather
than being simplified away: `MATE_SCORE - ply` already makes a nearer mate a strictly larger
raw score than a farther one, from the one shared perspective every thread's result is scored
in (the root position, and therefore its side to move, is identical across every thread in one
`go` -- there is no cross-thread perspective mismatch to reconcile, unlike the sign flip that
happens ply to ply inside the tree itself). Plain score comparison already prefers the shorter
mate for free.

**Settled: the worker that completed the greatest depth wins; ties at the same depth broken by
comparing raw score directly (`newThread.score > bestThread.score`), no sign adjustment, no
voting.** An explicit placeholder in the same category as `ASPIRATION_INITIAL_DELTA` -- a
directionally-reasoned simplification to revisit if real multi-threaded self-play data ever
shows threads genuinely disagreeing often enough for voting to matter.

"Depth" here means the nominal iterative-deepening depth each thread's own `choose_best_move`
loop last completed -- the same integer UCI's `info depth N` reports -- never `seldepth`. A
thread's `seldepth` measures how far quiescence chased one particular forcing line past the
nominal depth, which varies node to node and says nothing about how thoroughly the rest of
that thread's search covered the position; comparing it would reward a thread that happened to
stumble into one long tactical sequence over one that searched more broadly but hit no long
tactics, not a meaningful notion of "which thread did more or better work."

## Not yet settled / deferred

An earlier version of this line claimed "nothing" -- itself wrong, caught only once
implementation reached the thread-spawning piece and found the per-thread aspiration-window
delta had never actually been designed (see "Thread diversity" above, now settled). Recorded
here as a reminder that "every fork raised is settled" is a claim about forks *raised so far*,
not a guarantee against gaps the design phase itself missed -- this document has now caught
three such gaps during implementation (generation's missing bit-width, `depth`'s width tied to
a constant expected to grow, and this one), each caught and settled in place rather than
papered over.

Genuinely nothing outstanding as of now: every fork raised, including this one, is settled.

## Progress log

Design discussion in progress, following the project's established methodology (verify every
real-engine claim against actual source rather than memory; never invent an unjustifiable
number; land the naive version first). Verified against: the developer's own real Stockfish
17.1 `echo "uci" | stockfish` output (`Threads` default/min/max), the real UCI protocol spec
text at backscattering.de (`Threads` is convention, not spec), Stockfish's real
`search.cpp`/`thread.cpp`/`tt.cpp`/`types.h` source (aspiration-window thread diversity,
absence of per-thread depth-skipping or root-move reordering, the real `TTEntry::save`
replacement condition and depth encoding), and the Chess Programming Wiki's Lazy SMP page
(the shared table as Lazy SMP's actual mechanism, not an add-on). Two misremembered claims
about Stockfish's own diversity mechanism (per-thread depth skipping; per-thread root move
reordering) were floated and then corrected once checked against real source, rather than
left standing.

Settled so far: threading model (Lazy SMP, shared TT, root-splitting considered and
rejected); scope (synchronous only, `stop`/async tracked separately in `TODO.md`); the
`Threads` option's shape (default 1, min 1, max 128); that aspiration windows are a hard
prerequisite; the adapted replacement condition (key-mismatch always wins; otherwise `Exact`
bound, strictly greater depth with no tolerance, or stale generation); generation bump timing
(once per `go`, shared across all worker threads); and result selection (deepest completed
depth wins, ties broken by raw score, voting rejected as unjustified complexity at Rotor's own
scale).

Aspiration windows (the prerequisite identified above) are now implemented and confirmed
working (`aspiration-windows.md`, moved to `done/`). Checked Stockfish's real
`ThreadPool::get_best_thread()` (`thread.cpp`) for the result-selection question rather than
guess -- its vote-based scheme and mate-distance special case were both examined and
deliberately not adopted, with reasoning recorded above.

TT concurrency and the packed `Entry` layout, the two remaining forks, are now both settled
together, having turned out to be one real design question rather than two sequential ones.
An earlier claim in this document that Stockfish uses a `key XOR data` checksum was checked
against Stockfish's actual current `tt.cpp` and found wrong -- corrected in place, with the
real mechanism (six independent per-field `RelaxedAtomic`s, deliberately racy, validated by
`fishtest` rather than provably safe) now recorded instead. Worked out, from first principles
and with real numbers rather than assertions: what a bare atomic actually guarantees versus a
lock; a back-of-envelope collision-probability model (`R^2 * dt * T / N`) showing the honest
answer swings two orders of magnitude on Rotor's own unmeasured store throughput, flagged for
real measurement later rather than settled by picking a comfortable illustrative number; and
that `Bound::Exact` hits carry the entire risk (no downstream alpha/beta corroboration exists
for them, unlike `Lower`/`Upper`), a real asymmetry Rotor shares with Stockfish's own accepted
design. Settled on a middle ground neither pure Stockfish-style nor a checksum: two atomics per
entry (`key: AtomicU64`, one packed `AtomicU64` for `best_move`/`depth`/`score`/`bound`
together), which eliminates the single most common contention pattern (two threads storing the
same transposing position) outright, leaving only the narrower index-collision case that rides
on `probe`'s existing key check regardless. The packed bit layout itself (15 bits for `best_move`,
reusing `PieceType`'s own discriminant for promotion codes rather than inventing new numbering;
4 for `depth`; 16 for `score`; 2 for `bound`) is settled too, along with where the packing logic
lives: entirely inside `tt.rs` as private free functions using `Move`'s existing public API,
deliberately not new methods on `Move` itself, consistent with this project's standing
practice of not growing a foundational type for one caller's storage-format need.

Implementation has started in `tt.rs`. Landed so far: `pack_move`/`unpack_move` (the 15-bit
`best_move` encoding, private free functions, tested against all 8 kind-or-promotion codes),
and `pack_bound`/`unpack_bound` plus `pack_entry_data`/`unpack_entry_data` (the full
`best_move`/`depth`/`score`/`bound` word, tested including negative and mate-magnitude scores
and the deepest representable depth). Each landed with round-trip tests before moving to the
next piece, per this project's small-pieces methodology.

Implementing `pack_entry_data` surfaced a real gap the design phase missed: no bits had been
budgeted for `generation`, even though the replacement policy needs it on every `Entry`. Caught
and settled before writing more code rather than papered over -- see the bit-width table above
(now `generation: 8 bits`, appended). Also caught during this same stretch: `depth`'s original
4-bit width was tied to today's `MAX_ITERATIVE_DEPTH = 10` rather than a durable justification,
widened to 8 bits (matching Stockfish's own `depth8`) once the developer flagged it; the
wraparound-frequency framing for `generation` was corrected (wraps within a single long game,
not merely "across a process lifetime"); the empty-slot sentinel question (`generation == 0`,
checked against Stockfish's real `is_occupied()` technique rather than a naive `key == 0`) was
raised and settled; and `clear()`/`ucinewgame` resetting the generation counter (not just the
entries) was raised and settled. `pack_entry_data`/`unpack_entry_data` (now carrying
`generation`) and their tests are landed and passing.

The atomic `Entry`/`TranspositionTable` rewrite has now landed in full: `Slot` (two atomics,
confirmed exactly 16 bytes) replaces `Vec<Option<Entry>>`; `Entry` itself dropped its `key`
field, now a pure decoded snapshot `probe` hands back by value rather than something living
permanently in the table; `NEVER_WRITTEN_GENERATION`/`STARTING_GENERATION` realize the
empty-slot-sentinel decision; `probe`/`store` implement the real two-tier replacement policy;
`new_search` (bump, wrapping `u8::MAX` back to `STARTING_GENERATION`) and `clear` (now also
resets `generation`) are both new. One call site in `search.rs` needed fixing to compile
(`probe(...).copied()` -> `probe(...)`, since `probe` no longer returns a reference). Full test
suite (330 tests) passes.

`new_search` is now wired in: `choose_best_move` (the one function representing "a real
search," already holding `tt: &mut TranspositionTable`) calls it once at its own top, before
the iterative-deepening loop -- the correct "once per go, before any worker thread starts"
moment even today with no threads yet, and stays correct once Lazy SMP spawns them as long as
spawning happens after this line. One call site in `search.rs` (`probe(...).copied()`) needed
fixing to compile after the rewrite. Full suite (330 tests) passes unchanged before and after
this wiring -- expected, since `probe`'s hit/miss decision never consulted `generation` (only
`key` and the empty-slot sentinel), so this could only activate `store`'s previously-dormant
stale-generation replacement branch, never change search outcomes or node counts on today's
single-threaded path.

Deliberately not done yet: `probe`/`store`/`clear` still take `&self`/`&mut self` as before the
rewrite -- no thread-sharing wiring (e.g. `Arc<TranspositionTable>`) yet.

Before starting the thread-spawning piece itself, one more real gap surfaced: the per-thread
aspiration-window delta -- the actual diversity mechanism this document's own "Prerequisite
discovered" section says aspiration windows exist to enable -- had never been designed.
`meanSquaredScore` was checked directly against Stockfish's current `search.h`/`search.cpp`
(rather than trust an earlier passing mention of it in conversation) and confirmed to be a
separate, per-root-move refinement, not a function of `threadIdx`, so excluded from this
decision. Settled: `delta(thread_idx) = ASPIRATION_INITIAL_DELTA + 5 * (thread_idx % 8)` --
`STEP = 5` reasoned from Rotor's own coarser-grained hand-crafted eval (same unmeasured-
placeholder status as `ASPIRATION_INITIAL_DELTA` itself), and `% 8` saturation kept deliberately
(Rotor's own 128-thread ceiling doesn't strictly need it, unlike Stockfish's much larger one, but
nothing about Rotor's
situation argues for diverging from the real formula's shape either). See "Thread diversity"
above for the full reasoning. Not yet implemented -- this landed as a design decision only, no
code written for it yet.

The per-thread delta itself is now implemented: `ASPIRATION_THREAD_STEP`/
`ASPIRATION_THREAD_DIVERSITY` constants and `initial_aspiration_delta(thread_idx)` landed next to
`ASPIRATION_INITIAL_DELTA` in `search.rs`; `search_root` and `choose_best_move` both gained a
`thread_idx: u32` parameter (the latter passing it straight through to the former, unchanged
depth to depth); every call site (`uci.rs`'s one real caller, and every test call site in
`search.rs`) now passes `0`, reproducing today's exact single-threaded behavior unchanged, since
`initial_aspiration_delta(0) == ASPIRATION_INITIAL_DELTA`. Three new unit tests cover the formula
directly (thread `0` matches the plain default; each thread index up to the diversity count
widens by one step; the pattern wraps every 8 threads). Full suite (333 tests) passes.

One near-miss caught before it landed: a first draft of one new test's comment referenced this
very document (`threading.md`) by name -- caught and rewritten to be self-contained (the actual
reasoning inlined instead) before being left in place, per this project's own standing rule that
source code must never point at a `claude/` document.

`ASPIRATION_INITIAL_DELTA` was revisited and lowered, `50` to `20`, once its window's own shape
(`[center - delta, center + delta]`) was worked through concretely and `50` turned out to mean a
full-pawn-wide (`100`-centipawn) total window -- more slack than there's any real reason to open
with. See "Thread diversity"'s own new note above for the full reasoning; every place this
document quoted the literal value `50` was updated to reference the constant by name instead, so
it can't silently drift out of sync with the code again.

**The thread-spawning mechanism itself was settled, and it's simpler than the "likely `Arc`" note
above assumed.** `probe`/`store` don't actually need exclusive access at all -- both already only
touch `self` through `Slot`'s atomics and plain reads, so `&mut self` on `store` was purely a
leftover from before the atomic rewrite. Only `new_search`/`clear` genuinely mutate
(`self.generation`, a plain field), and both only ever need to run *before* any worker thread
exists, never concurrently with them. Given the already-settled scope ("`go` spawns N worker
threads, joins all of them, then replies"), `std::thread::scope` matches that shape exactly and
lets the borrow checker enforce the ordering for free: call `tt.new_search()` (needs `&mut`) with
no borrows outstanding, then hand a shared `&TranspositionTable` to every thread spawned inside
the scope, which can only reach `&self` methods. No `Arc`, no `Mutex`, no atomic `generation`
needed. `search_root`/`choose_best_move`/`Search`'s own `tt` field all change from `&mut
TranspositionTable` to `&TranspositionTable` throughout as a result -- a real, if mechanical,
ripple through every call site.

A non-winning thread's `IterationReport`s are simply discarded once result selection picks a
different thread's result -- matches how `go_search` already works today (info lines only ever
get built after `choose_best_move` fully returns, never streamed live per-iteration), so no new
"live multi-thread reporting" design is needed.

**Time management: settled on mirroring Stockfish's real model, not the "every thread
independently checks the same budget" simplification first considered.** Checked Stockfish's own
model directly (`thread.cpp`/`search.cpp`) rather than assume: only the main thread (`threadIdx ==
0`, itself nothing more than "whichever thread was constructed first when the pool was built,"
confirmed via `ThreadPool::set()`'s `threadId = threads.size()`) ever reads the clock; it alone
decides when the budget is exhausted (both between iterations and, via a periodic mid-search
`check_time()`, within one) and sets one pool-wide `threads.stop` atomic, which every thread
(main included) only ever polls, never writes to itself.

Settled: Rotor's thread `0` plays the same role. A single `AtomicBool`, scoped to one
`choose_best_move` call (created fresh per `go`, dropped when it returns) -- not the pool-wide,
process-lifetime kind Stockfish's `threads.stop` is, just scoped to one search. Thread `0` keeps
doing exactly what `choose_best_move`/`alpha_beta_with_buffer` already do today (the between-
depths headroom gate, the `CHECK_INTERVAL`-throttled mid-search clock check) unchanged, except it
now also writes `true` to the shared flag whenever either decides to stop, not just its own
private `Search::stopped`. Every other thread never computes a deadline or reads a clock at all
-- its own periodic check just polls the shared flag instead.

This does **not** pull forward the separately-deferred async-UCI-`stop` work from `TODO.md`,
despite both involving a shared atomic -- that deferred item is about *external* interruption (a
GUI's `stop` command arriving while `go` blocks, needing `uci.rs`'s stdin-reading loop itself
restructured to stay non-blocking). This mechanism is entirely *internal*: it lives and dies
inside one still-fully-synchronous `choose_best_move` call, never touches `uci.rs`, and matches
the already-settled scope ("`go` spawns N worker threads, joins all of them, then replies")
exactly. An earlier version of this document conflated the two -- corrected here once the actual
mechanism was worked through concretely rather than assumed to require the same restructuring.

The `&mut self` -> `&self` signature refactor is now landed in full: `TranspositionTable::store`
(mechanically -- everything it touches already worked through shared access), `Search`'s own `tt`
field, `new_search`, `search_root`, and `choose_best_move` all changed from `&mut
TranspositionTable` to `&TranspositionTable`, rippling through every call site (`uci.rs`'s one
real caller, every test in `search.rs`, and `tt.rs`'s own tests, several of which also dropped a
now-unnecessary `mut` on their table bindings to avoid an unused-`mut` warning). One real
consequence, not just mechanical: `choose_best_move` could no longer call `tt.new_search()`
itself once `tt` became a shared reference, so that responsibility moved to the caller --
`go_search` in `uci.rs` now calls `self.tt.new_search()` explicitly, once, immediately before
`choose_best_move`. This is the correct shape regardless of threading: `choose_best_move`
represents *one worker thread's own* search and has no way to tell whether it's thread 0 of 1 or
thread 7 of 12, so only code that knows "this is the start of a new `go`" can call `new_search`
correctly. Full suite (333 tests) passes, unchanged count, confirming this was purely mechanical.

The shared-stop-flag time management is now landed in full: `Search` gained a `stop_signal: &'a
AtomicBool` field alongside `deadline`/`stopped`; both `CHECK_INTERVAL` sites
(`alpha_beta_with_buffer`, `quiescence`) now check the shared flag first (stopping immediately if
another thread already tripped it) before falling through to the thread's own `deadline`, writing
`true` to the shared flag itself when its own deadline trips. `new_search`, `search_root`, and
`choose_best_move` all gained a `stop_signal: &AtomicBool` parameter threaded straight through;
`choose_best_move`'s between-depths loop now branches on `thread_idx`: thread `0` alone computes
elapsed time and headroom exactly as before (now also storing `true` to the shared flag when it
decides to stop), every other thread skips deadline computation entirely and simply polls the flag,
breaking out of the loop if it's already set. `go_search` in `uci.rs` creates one `AtomicBool`
scoped to its own `choose_best_move` call, exactly matching the settled "one flag per `go`" scope.
Every call site across `search.rs`'s test suite (dozens of `new_search`/`quiescence_search`/
`search_root`/`choose_best_move` calls, and several explicit `Search { ... }` literals) now passes
a `stop_signal` argument, almost always a fresh local `AtomicBool::new(false)`. Full suite (333
tests) passes, unchanged count, confirming this is behaviorally neutral today: `thread_idx` is
always `0` at every real call site, so the new flag starts false and is never observed to change
anything about single-threaded behavior.

The `Threads` UCI option is now landed, mirroring `Hash`'s existing pattern exactly:
`DEFAULT_THREADS`/`THREADS_MIN`/`THREADS_MAX` (`1`/`1`/`128`, matching the values settled above)
next to `DEFAULT_HASH_MB`/`HASH_MB_MIN`/`HASH_MB_MAX`; `UciCommand::SetOptionThreads(u32)`;
`parse_setoption` extended with a second exact-shape arm (`["name", "Threads", "value", n]`); a
`threads: u32` field on `Engine`, defaulted to `DEFAULT_THREADS` in `Engine::new()`; the `uci`
reply gained a second `option name Threads type spin default 1 min 1 max 128` line; and a
`SetOptionThreads(n) => { self.threads = n; ... }` case in `handle()`. Deliberately inert so far:
`self.threads` is only stored, not yet read anywhere, since `go_search` doesn't spawn worker
threads yet. Full suite (337 tests) passes -- 333 before, plus the 4 new tests covering this
option's parsing and storage, matching exactly.

Thread spawning and result selection are now both landed, the last two pieces of this design.
`select_best_result` (in `search.rs`, next to `choose_best_move`) implements the settled rule
directly on the `(Vec<IterationReport>, Option<Move>)` pairs `choose_best_move` already returns:
the result whose last completed report has the greatest `depth` wins, ties broken by comparing
`score`, an empty `reports` (no legal moves, or stopped before depth `1`) never beats a nonempty
one. `go_search` in `uci.rs` now actually spawns `self.threads` workers inside
`std::thread::scope`, one call to `choose_best_move` per `thread_idx` in `0..self.threads`, all
sharing `&self.board`/`&self.ancestors`/`&self.tt`/`&stop_signal`; every handle is joined before
the scope returns, and `select_best_result` picks the reply from the joined `Vec` of results.

One real gotcha surfaced only at compile time, not during design: the first draft moved
`stop_signal` (the `AtomicBool` value itself, not a reference) into the per-thread `move` closure
-- compiles fine for `board`/`ancestors`/`tt` since a `&T` is `Copy` and can be moved into a
closure called repeatedly by `.map`, but `AtomicBool` itself isn't `Copy`, so the compiler
correctly rejected moving it more than once across multiple loop iterations. Fixed by reborrowing
`stop_signal` as a plain `&AtomicBool` right alongside `board`/`ancestors`/`tt`, before the
`.map`, so what actually gets moved into each thread's closure is a reference, not the atomic
itself.

Full suite (341 tests) passes -- 337 before, plus 4 new tests covering `select_best_result`
directly (greater depth wins regardless of order; a depth tie is broken by score; any completed
result beats an empty one; all-empty stays empty). Every existing test still passes unchanged too,
confirming the default `Threads = 1` case reproduces the old single-threaded behavior exactly: one
worker, `thread_idx = 0`, and `select_best_result` trivially returns that one result.

This closes out the multi-threaded search design as originally scoped: Lazy SMP with a shared,
lock-free transposition table; per-thread aspiration-window diversity; `std::thread::scope`-based
synchronous spawn-join-reply; the `Threads` UCI option; thread-0-decides time management; and
depth-then-score result selection. Nothing left outstanding from this document's own design
section. Real multi-threaded strength testing (does this actually play better with `Threads > 1`)
is a self-play/SPRT-harness question, tracked in `TODO.md`, not something this document itself
needs to resolve further.

Before closing this out, a real manual UCI test (`go movetime 3000` from the starting position,
`Threads = 4`) was run rather than trusting the test suite alone -- and it surfaced a real bug:
several reported PVs contained illegal moves (a move from a square its own piece had already
left, the same move appearing twice). Reproducing the identical corruption with `Threads = 1`
(no concurrency at all) confirmed it as a pre-existing, single-threaded bug entirely unrelated to
this design -- a stale, shared `Search::pv` row leaking between unrelated nodes at the same ply,
whenever one of them resolved via an early return (a TT cutoff, a repetition, and others) that
skipped the PV bookkeeping. Found and fixed as its own piece of work; see `pv-row-corruption.md`
for the full root cause and fix. Re-running the identical manual test after the fix confirmed
every PV at every depth is now legal and self-consistent, with the threading work itself
unchanged.
