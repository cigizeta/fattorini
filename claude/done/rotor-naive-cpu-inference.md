# Naive on the CPU: running a trained network inside the engine

**Done:** trained psq (formerly naive) networks load and evaluate in the engine with no libtorch (`PsqNetwork`, `src/nnue.rs`). The pawns conversion changed with the atan label scale (lc0-label-scale.md); threats came later (psq-threats.md). The "not yet implemented" status below is stale.

## Status

Design, not yet implemented. Prerequisite for the gate (challenger against champion), which is the
step the developer put first. The match runner itself is a separate document.

## Scope

In: exporting a trained naive checkpoint out of `tch`, a forward pass in the engine with no
`libtorch` anywhere, the raw-output-to-centipawns conversion, the UCI wiring that selects it, and
the tests that keep the two implementations honest.

Out, deliberately, each with its own reason:

- **Quantization.** int8/int16 weights are a speed project, measured in nodes per second. Nothing
  about the gate's question ("does this network play better chess") needs it, and fixed-nodes
  matches take speed out of the comparison entirely.
- **Incremental accumulator updates.** This is where NNUE implementations keep their bugs: an
  accumulator that drifts from the true one only in rare make/unmake orders produces an engine that
  is subtly, unreproducibly wrong. A full recomputation per evaluation is obviously correct and is
  the reference any later incremental version gets tested against.
- **SIMD.** Same reason. The plain loop is the reference implementation.
- **halfkp and halfka_v2_hm.** Naive is the cheapest first target: one perspective, no king-relative
  anchor, no king-file mirror. The export format and the module layout below are built so those two
  drop in later, but nothing here speculates about their shapes.

## The shape being implemented

Naive's forward pass, read off `nnue_network.rs::NetworkNaive` and `batch.rs::accumulate_naive`:

```
dense[204]  ->  transform (204 -> 512)  ->  relu
            ->  hidden0  (512 -> 256)   ->  relu
            ->  hidden1  (256 -> 128)   ->  relu
            ->  hidden2  (128 ->  64)   ->  relu
            ->  output   ( 64 ->   1)   ->  raw, unbounded
```

Widths come from `amanda.yaml` (`embedding_dim: 512`, `value_head.hidden_dim: [256, 128, 64]`), not
from code, so the engine reads them from the exported file rather than hardcoding them.

There is **one perspective**, not two. Naive expresses own/opponent through each piece's identity
(`piece_type * 2 + (0 if own else 1)`), so there is no second accumulator and no concatenation: the
value head reads a 512-wide vector, not a 1024-wide one. This differs from halfkp/halfka_v2_hm and
is the main reason naive is the cheap first target.

### The dense vector

204 rows, laid out by `accumulate_naive`:

| rows | meaning |
|---|---|
| `0..192` | `lane * 12 + identity`, where `lane` is `file` (`0..8`) or `8 + rank` (`8..16`) |
| `192..196` | own kingside, own queenside, opponent kingside, opponent queenside castling |
| `196..204` | en passant file |

Every piece, kings included, is oriented for the side to move (`oriented_square`, vertical mirror
only) and then **increments** two rows: its file lane and its rank lane, both at its own identity.
Increments, not sets. Eight pawns on one rank put `8.0` in that rank-lane row, and the network was
trained against exactly that. An implementation that sets `1.0` instead would be wrong in a way no
shape check catches, so the engine must accumulate the same way. Castling and en passant rows are
flags, set at most once, never summed.

So a real position activates at most `32 * 2 + 4 + 1 = 69` distinct rows out of 204, which is what
makes the gather cheap relative to the dense matrix multiply that follows it.

### Why this makes the transform a gather

Because the dense vector is sparse and integer-valued, the first layer is not a matrix multiply at
all:

```
accumulator = transform_bias + sum over active rows r of (count_r * transform_weight_row_r)
```

About 69 vector accumulations of width 512, against the 512 x 204 multiply the dense form would
imply. That is roughly 35k float adds, against 104k multiply-adds.

## Structural change: `features` and `labels` move into the lib

This is the part with real consequences, so it goes first.

Today `src/features.rs` and `src/labels.rs` are not library modules. They are `#[path]`-included
into the `rotor-prepare` and `rotor-train` binary crates (see `prepare.rs`'s own module list), and
each binary compiles its own copy. `src/lib.rs` exposes only `board`, `eval`, `material`, `movegen`,
`perft`, `phase`, `search`, `tablebase`, `tt`, `uci` -- the engine has no access to either.

The engine needs `extract_canonical`, `naive::expand`, and `clamped_pawns_from_score`. So both
modules move into the lib, as `rotor::features` and `rotor::labels`, and the binaries drop their
`#[path]` copies in favour of `use rotor::features` / `use rotor::labels`.

**Why moving rather than reimplementing in the engine.** The obvious alternative is a second,
engine-local feature extractor, kept honest by the agreement test. It does not work, and the reason
is worth writing down because it is not obvious: the agreement test compares the two forward passes
starting from a `CanonicalPosition` read off the cache. The engine, in real play, starts from a
`Board`. The `Board -> CanonicalPosition` step is therefore *not covered* by that test, and it is
exactly the step a duplicate would get wrong -- a mis-oriented square or a dropped castling right
produces a network that silently plays worse and no test fails. Sharing the code makes the step
identical by construction instead of by assertion.

Neither module pulls in `tch`. `features.rs` depends on `rotor::board` alone; `labels.rs` adds
`rotor::search::{TB_WIN_SCORE, MAX_PLY}`. Both become `crate::` references once moved. No new
dependency reaches the engine, and `rotor`/`rotor-curate` still build with no `libtorch`.

`labels.rs` moves whole rather than being split into "the engine's half" and "training's half".
`SCALE` and `CLAMP_EPS` are shared by both halves, and splitting the module creates two places for
one constant to drift -- the same class of bug this whole exercise just finished paying for.

### Mechanical consequences

Items currently `pub(crate)` are used across what becomes a crate boundary, and have to widen:

| item | used by | today | becomes |
|---|---|---|---|
| `features::oriented_square` | `transformer_vocab.rs` | `pub(crate)` | `pub` |
| `labels::score` | `length_cache.rs::is_trainable` | `pub(crate)` | `pub` |
| `labels::CLAMP_EPS` | `length_cache.rs` | `pub(crate)` | `pub` |
| `labels::is_decisive` | `length_cache.rs` | `pub(crate)` | `pub` |

Each keeps its existing doc comment explaining why it is visible at all; only the level changes.

## The export file

A new `rotor-train export <network-name>` subcommand. It takes **neither a step nor a rate**, and
both omissions are decisions rather than conveniences.

**The step is the arm's own best test loss**, read via the existing `best_recorded(history_path)`.
Exporting anything else would ship a network the run itself measured as worse, and no invocation
should be able to ask for that by typo.

**The arm is the one with the lowest `best_test_loss` in `grid.tsv`**, decided with the developer.
That is the same arm `grid.tsv` already highlights as champion, so the exported network is always
the one the standings name, with no second selection rule to keep consistent with the first.

The known weakness of that statistic is worth recording where the code lives rather than losing it:
`best_test_loss` is a minimum over every round an arm ever ran, so it is biased downward, and the
bias grows with the number of rounds. Ranking arms by it therefore rewards whichever arm is
noisiest. On amanda's five-epoch run the top two are `1e-3` at `0.0370733` and `3e-4` at `0.0372033`,
a gap of `0.00013` against a round-to-round band of about `0.0008` -- indistinguishable, and the
last-20-round means (`0.037523` against `0.037555`) agree that they are. So the rule picks a genuine
co-champion here rather than a wrong arm, and if a future grid is ever decided by a margin smaller
than its own noise band, that is a signal to re-examine the selection rather than to trust it.

It writes `<run_dir>/checkpoints/step_<step>.nnue` beside the `.vs` it was read from. **Pruning needs
no change**: `checkpoints/` is pruned down to its few best rounds by test loss, and the step being
exported is by construction the best one, so the source weights can never be deleted out from under
an exported net.

Little-endian throughout, `f32` weights:

```
magic        8 bytes   "ROTORNNU"
version      u32       1
feature_set  u32       0 = naive, 1 = halfkp, 2 = halfka_v2_hm
embedding_dim u32
hidden_count u32
hidden_dims  u32 * hidden_count
step         u64       provenance, reported by the engine, never read as logic
learning_rate f64      provenance
then, in order:
  transform weight, TRANSPOSED: [rows][embedding_dim] row-major
  transform bias:              [embedding_dim]
  for each hidden layer i:
    weight: [out_i][in_i] row-major
    bias:   [out_i]
  output weight: [1][last_hidden]
  output bias:   [1]
```

**The transform is stored transposed, and that is the one performance decision in the format.**
`tch`'s `nn::Linear` holds `ws` as `[out, in]`, so the transform sits as `[512, 204]`. The gather
above wants, for one active row `r`, the 512 contiguous floats of that row's contribution -- which
in `[512, 204]` layout is a stride-204 column walk, one cache line touched per float. Transposing
once at export turns 69 cache-hostile column walks per evaluation into 69 contiguous 2 KB reads.
The value-head layers stay `[out, in]`, which is already the right orientation for a dense
matrix-vector product.

`feature_set`, `embedding_dim` and `hidden_dims` are in the header rather than assumed so that the
engine refuses a mismatched file outright instead of reading garbage, and so halfkp/halfka_v2_hm
need a new variant rather than a new format.

The exporter reads weights out of the `VarStore` by the names `vs.variables()` reports, and asserts
the exact expected name set. A rename on the training side then fails the export loudly rather than
silently exporting a zero tensor.

## The engine-side forward pass

New lib module `rotor::nnue`, no `tch`:

```rust
pub struct NaiveNetwork { /* dims, transform (transposed), biases, layers */ }

impl NaiveNetwork {
    pub fn load(path: &str) -> Result<NaiveNetwork, String>;
    /// Side-to-move relative, in the same [-1, 1] frame the labels were trained in.
    pub fn evaluate_raw(&self, board: &Board) -> f32;
}
```

`evaluate_raw` runs `extract_canonical`, then `naive::expand`, then the accumulate-and-multiply
above. Scratch buffers are allocated per call in the first version: about 960 floats, under 4 KB,
against tens of microseconds of arithmetic. Hoisting them into thread-local storage is a later,
measurable change, not a thing to assume up front.

**No sign handling anywhere.** The network's output is already side-to-move relative -- that is what
the label-frame fix established -- and `eval::evaluate`'s contract is already "positive means the
side to move is ahead". The two conventions coincide exactly, which is the payoff for having fixed
the labels rather than patching the sign at inference.

## Raw output to centipawns

`cp = round(100 * clamped_pawns_from_score(raw))`, the same conversion `check` already uses. Nothing
new is invented here, and `clamped_pawns_from_score` is already `pub`.

**The clamp is a numerical guard, not a decision about the evaluation's range.** The output layer is
a plain `nn::linear` with no squashing, so `raw` is unbounded -- `CLAMP_EPS`'s own doc comment
records a real trained checkpoint measured at `1.6781` -- while `atanh` is infinite at `+/-1` and
undefined past it. The clamp is what makes the inversion defined on an arbitrary network output.
Removing it would move the bound from `0.99` to `1.00`, which is the same place; there is no version
of this conversion that does without one.

`tanh` itself loses nothing. It is invertible: `score` maps a queen to `(2^9-1)/(2^9+1) = 0.9961` and
a queen and rook to `0.99988`, and `atanh` recovers both exactly. The closed form is worth recording
because it makes every number here checkable by hand -- `2x = ln2 * pawns`, so
`score(p) = (2^p - 1) / (2^p + 1)`, giving the documented `score(1) = 1/3` directly.

What limits the evaluation far from equality is precision, not the clamp and not the transform.
Those two labels differ by `0.004`, against a network test RMSE of `sqrt(0.0375) = 0.194`, so the
absolute centipawn number in a lopsided position sits well below the network's own error.

That is a weaker statement than it first sounds and is not a reason to change anything on this path.
Search compares sibling positions, where the network's errors are correlated, so the local ordering
can be right where the absolute value is not -- and local ordering is all alpha-beta consumes.
Whether it actually is right is a question the gate answers in games rather than by argument. If it
turns out to matter, the lever is `SCALE` in `labels.rs`: a training change that redistributes
resolution between equal and lopsided positions, paid for by compressing it where the overwhelming
majority of real positions live.

## Engine integration

**The gate runs in-process, not over UCI** (decided with the developer). Two `Search` instances in
one process play each other, the way `rotor-self` already drives games, rather than two child
processes speaking the protocol. UCI support for a network is wanted eventually but is not on this
path, and everything below follows from that.

- **`Search` carries its own evaluator**, set at construction:

  ```rust
  pub enum Evaluator {
      HandCrafted,
      Naive(Arc<NaiveNetwork>),
  }
  ```

  with a private `Search::evaluate(&self, board: &Board) -> i32` dispatching on it. The reason is the
  second gate, not the first: generation 1 against generation 0 is two different networks alive at
  once, which no process-global can express, and the refactor only gets more expensive once code has
  been built on top of the global.
- **`eval::evaluate` does not change at all.** It stays the free, hand-crafted function it is today,
  and `Evaluator::HandCrafted` calls straight through to it. That is what keeps the roughly two dozen
  tests in `search.rs` and `uci.rs` that call `evaluate` directly working untouched.
- **Five call sites move** from `evaluate(board)` to `self.evaluate(board)`: the `MAX_PLY` cutoff in
  `alpha_beta_with_buffer`, reverse (static) futility pruning, the shared futility precomputation,
  `quiescence`'s own `MAX_PLY` cutoff, and the stand-pat. All five are already inside `Search`
  methods, so `self` is in scope and no signature outside `Search` changes.
- **`Arc`, not a bare value**, so both sides of a match can share one loaded network when a generation
  plays itself, and so cloning a `Search` per thread costs a refcount rather than a copy of the
  weights.
- **UCI (`setoption name EvalFile`) is deferred**, along with the question of what the engine should
  do about a file that fails to load. In-process, a bad file is an ordinary `Result` handed back to
  the match runner before any game starts. Over UCI it becomes a protocol question -- a silent
  fallback to the hand-crafted evaluation would let a whole match run and report a meaningless result
  -- and that is worth deciding when UCI support is actually built, not now.

## Tests

In rough order of how much they are worth:

1. **The agreement test, `tch` as the oracle.** In `rotor-train`'s own module tree, since only it has
   `tch`: build a `NetworkNaive` with random weights, export it to a temporary file, load it through
   `rotor::nnue::NaiveNetwork`, and compare both forward passes over a set of positions. This is the
   test that makes a second implementation safe to have at all. Tolerance is relative, around `1e-4`
   on the raw value: the two paths sum in different orders and libtorch may vectorize, so bitwise
   equality is not the right assertion. At the clamp boundary `1e-4` of raw output is on the order of
   a centipawn and a half, which is the figure to state in the test rather than leave implicit.
2. **Round trip through the file format.** Random dimensions, random weights, write then read, assert
   every tensor comes back identical. Catches layout and transpose errors on their own, without the
   forward pass in the way.
3. **Header rejection.** Wrong magic, unknown version, a `feature_set` the build does not implement,
   and a truncated file each produce an error rather than a panic or garbage.
4. **The accumulate rule.** A position with several pawns on one rank must put a count greater than
   `1.0` in that row. This is the "increments, not sets" invariant, and it is the one that a
   reimplementation gets wrong most naturally.
5. **The centipawn conversion.** `evaluate` on a loaded network stays well inside the mate band, and
   saturates at the documented `+/-764` rather than running away.
6. **Negamax symmetry.** For a position and its colour-swapped mirror, the loaded network's
   `evaluate` must negate. This is a structural property of the whole path -- orientation, identity
   assignment and conversion together -- and it holds independently of what the weights are.

## Performance

Measured by `benches/nnue_bench.rs` against the real exported network (277,505 parameters,
`embedding_dim 512`, hidden `[256, 128, 64]`), seven positions from the opening to bare kings:

```
  hand-crafted evaluate            19.1 ns
  feature extraction only          44.8 ns
  network evaluate_raw          11616.4 ns  (11.6 us)
    of which arithmetic         11571.7 ns  (100%)
    of which extraction            44.8 ns  (0%)

  network costs 608x the hand-crafted evaluation
  86085 evaluations/second single-threaded
```

That is after vectorizing the value head. The first version, a scalar loop accumulating into a single
`sum`, measured `60.9 us` -- and the 5.2x came from removing a loop-carried floating-point dependency
rather than from any algorithmic change: `simd.rs::dot_product_avx2` keeps four independent `__m256`
accumulators, so FMA latency is covered instead of serializing the loop.

Where that leaves the headroom, because it decides whether more f32 work is worth doing: 380k
floating-point operations in 11.6 us is about **33 GFLOP/s**, roughly a quarter of one core's AVX2 FMA
peak -- but it also reads the value head's 688 KB of `f32` weights every evaluation, which is **59
GB/s**. If those weights sit in L3 rather than L2, that is close to the per-core limit and no amount
of further f32 tuning will move it. Cutting the bytes is what would, which is the argument for int8
quantization and not for a better f32 kernel.

Two results worth stating plainly, because both contradict what this document assumed before the
bench existed.

**Feature extraction is free, and an incremental accumulator would buy almost nothing.** Extraction
is 45 ns of 11,616 -- under half a percent. Even counting the transform gather itself (about 35k of
the ~380k floating-point operations), everything an incremental update could remove is under 10% of
the cost. That is the opposite of the usual NNUE situation, and for a structural reason: a real
engine's feature transformer is enormous and its value head is tiny, so the accumulator update is the
whole game. Naive inverts that. Its transform is `204 x 512` while its value head's first layer alone
is `512 x 256` -- 131k of the 172k multiply-adds, 76% of the total. **The value head is the cost, not
the accumulator.** Incremental updates, recorded here as the obvious later optimization, are in fact
close to pointless for this architecture.

### At the search level, which is what actually matters

The per-evaluation ratio is the wrong number to plan with, and by a wide margin. The same bench with
`--depth 9`, the depth self-play already uses:

```
                  position   hand (s)   hand nodes    net (s)    net nodes
  rnbqkbnr/pppppppp/8/8/8/      0.014         9265      0.745        35204
  r3k2r/p1ppqpb1/bn2pnp1/3      0.065        41127      1.349        73529
  4rrk1/pp1n3p/3q2pQ/2p1pb      0.034        18749      1.148        69570

  search is 29x slower with the network, against 608x per evaluation.
```

**29x, not 608x.** Everything else a node does -- movegen, ordering, make/unmake, the transposition
probe -- is unchanged, and it dominates a hand-crafted node. (The per-evaluation ratio also moves run
to run, because it divides by a ~20 ns quantity near timer resolution. It does not matter; 29x is the
number that does. Before the value head was vectorized it was 137x.)

**The network searches 1.8x to 3.8x more nodes at the same depth, and that is a real finding, not
noise.** Null-move pruning, razoring and both futility prunings all compare a *static evaluation*
against centipawn margins tuned against the hand-crafted evaluation's own scale. The network's output
has a different distribution -- bounded to `+/-764` cp, and regressed toward a blended AB(0.7) target,
so plausibly compressed near zero -- and a compressed evaluation clears fewer margins, so less gets
pruned. That is a hypothesis with an obvious test (histogram the two evaluations over real
positions), and it is worth running, because recovering that factor is 2-4x for no loss of quality.

### The two consequences, which point opposite ways

**The gate is cheap.** At 29x and the measured 3 games/s hand-crafted at depth 9 on 16 threads: one
game every 10 s, so 100 games in 0.3 h and 500 games in **1.3 h**. Before the value head was
vectorized the same run was 6.4 h.

**Generation-1 self-play is still the binding constraint.** 29x applied to data generation, at the
measured 106.5 plies per game, is about 11 positions/s against the hand-crafted 320. A dataset the
size of the current one -- roughly 14M positions -- takes about **15 days**, down from 70 but still
far too long. So the gate is affordable and the generational loop is not, which is the right order:
the gate decides whether this network is worth quantizing.

## Open questions for the developer

1. **The node budget for the gate.** It sets the wall-clock cost per game, which is the whole
   practical constraint on how many games the gate can afford. Belongs to the match runner's own
   design rather than this one.

## Settled with the developer

- **The exported step is the arm's own best test loss, and the arm is `grid.tsv`'s lowest
  `best_test_loss`.** `export` takes neither a step nor a rate.
- **`export` stays a separate, manual invocation.** `train` never writes a `.nnue`, so it needs no
  knowledge of the file format and an epoch boundary stays purely a training concern. `export` is
  then idempotent and rerunnable against any finished run, including past ones, without retraining.
- **The gate runs in-process rather than over UCI**, so a `Search` carries its own `Evaluator`
  instead of reading a process-global network. A `OnceLock` was settled on first and then reversed
  once in-process pitting was stated: its whole justification was one process per engine, which no
  longer holds, and generation 1 against generation 0 needs two networks alive at once.
- **UCI support for a network is deferred**, and with it the question of how the engine should react
  to an `EvalFile` that fails to load.
