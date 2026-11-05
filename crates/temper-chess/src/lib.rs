/// `temper-chess bench`: the node count and speed an SPSA client reports, and its clock is scaled by.
pub mod bench;
pub mod bench_positions;
pub mod board;
mod core_pinning;
pub mod eval;
/// Feature extraction, shared by the training binaries and the engine alike. It lives here rather
/// than beside the training code because the engine's own inference path has to derive a
/// `CanonicalPosition` from a `Board` by exactly the rule the old prepare step used when it wrote the
/// training data. A second, engine-local copy would be free to drift from that rule, and the
/// natural test against it cannot catch the drift: comparing a trained network's own forward pass
/// against the engine's starts from a `CanonicalPosition` read off the cache, so the
/// `Board`-to-`CanonicalPosition` step is the one part such a test never exercises. Sharing the code
/// makes that step identical by construction rather than by assertion.
pub mod features;
/// The label scale (`score`/`pawns_from_score` and the ceiling around them) plus the direct blend
/// that builds on it. Here for the same reason as `features`: the engine clamps a
/// network's pawns to exactly the ceiling the labels were built with, and one set of constants in
/// one place is what keeps those two from drifting apart.
pub mod labels;
pub mod movegen;
/// What the search did with each quiet move, by history score and depth. Only under the
/// `move-stats` feature, for measuring move ordering; the engine never builds it.
#[cfg(feature = "move-stats")]
pub mod move_stats;
/// `node-stats` feature, for measuring what static evaluations cost a search; the engine never
/// builds it.
#[cfg(feature = "node-stats")]
pub mod node_stats;
/// A trained network as the engine holds and runs it, with no `tch`/`libtorch` anywhere in the
/// dependency chain -- the whole reason inference is reimplemented here rather than the engine
/// linking the training stack.
pub mod nnue;
pub mod perft;
pub mod pesto;
pub mod pipeline;
/// Polyglot keys, move encoding and book bytes. Here, not in `arena`, because `smithy` reads
/// master-game positions by the same key to find where a CCRL game's book ends.
pub mod polyglot;
pub mod search;
/// The keys a `psq_stack` network's head picks its weight sets by, shared by trainer and engine for
/// the reason `features` gives.
pub mod stack_buckets;
/// The AVX2/FMA3 slice kernels the network's forward pass is built on. Crate-private: it is an implementation
/// detail of how this engine evaluates, not part of its interface.
mod simd;
pub mod tablebase;
pub mod tt;
pub mod uci;
