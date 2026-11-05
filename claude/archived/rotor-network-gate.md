# Gating a trained network against the hand-crafted champion

**Archived:** the match machinery and SPRT it describes were built (`src/arena`), but `rotor-evolve`, the epoch directories and its gate layout are gone. Selection is a depth-9 round robin then a gate: round-robin-selection.md and sprt.md.

## Status

Design, partly built. Depends on `rotor-naive-cpu-inference.md`, which is implemented: a trained naive
network loads into the engine and a `Search` can evaluate with it. Depends on `sprt.md` for the
decision rule, which this document deliberately does not restate.

Built: the `Player` trait with `UciPlayer` and `InProcessPlayer`, the `Control` grammar (`"3+2"` or
`"depth 9"`), the registry's `kind` field, `sprt.rs`, the 16 paired workers with their atomic pair
writes, and `decision: sprt`. Not yet run against a real network.

**Superseded 2026-09-26: the gate is no longer a tournament**, decided with the developer: a
tournament config for a two-player match was overkill, and the record belongs with the network.
`rotor-evolve gate <network> --threads <pairs>` plays the current generation's latest epoch's
challenger (`gen-NNNN/epoch-NNNN/challenger.nnue`) against the generation's champion
(`gen-NNNN/initial.nnue`, or the hand-crafted evaluation in `gen-0001`), at
`generation::SEARCH_DEPTH`, with the SPRT constants from the `sprt:` section of `net.yaml`, and
writes `gate.db` and `gate.pgn` into that epoch's directory. One gate per epoch. `decision: sprt`,
`match.sprt` and `match.threads` are gone from tournament configs, which are best-of only again and
refuse unknown keys. See rotor-train-generations.md. Everything below that describes the gate as a
tournament, `data/engines.yaml` entries or `data/tournaments/<id>/` is historical.

Three decisions taken during implementation that this design did not specify:

- **`engine_one` is the challenger**, so the order of `participants` is not cosmetic. The test is
  one-sided, and a gate with the champion listed first would read as a failure whatever happened.
- **Per-move console output is suppressed under `decision: sprt`.** Sixteen games writing move lines
  to one terminal is unreadable, so `game::play_one_game` takes a `print_moves` flag and the gate
  prints one line per completed pair: the pair's score, the running total, the LLR and both bounds.
  The PGN is unaffected.
- **A pair cut short by Ctrl+C or a fault is dropped entirely, including its completed first game.**
  That game is a real result, but keeping it would break the invariant that the database holds only
  colour-balanced pairs, and one game of information is not worth the property that makes resuming
  correct with no special cases.

A fault ends the whole gate rather than deciding it, unlike a best-of pairing. There is no score for
a fault to award here: the question is about an evaluation, and a crashed opponent is not evidence
about one.

## What the gate measures, and what it does not

**Does the trained network evaluate positions better than the hand-crafted evaluation that taught
it?** Both sides run the identical search at the identical fixed depth 9 -- the depth self-play
already uses -- so the only difference between them is the evaluation function. That is the standard
way to A/B an evaluation, and it is the question generation 0 exists to answer.

**It is explicitly not "is this engine stronger?"** At any real time control the network engine would
be crushed on speed alone: `benches/nnue_bench.rs` measures the search 29x slower at depth 9. A
clock would measure that handicap, not the evaluation, and we already know the handicap. Playing
strength at a real time control is a later question, and it only becomes worth asking if this one
comes back positive.

Worth stating plainly because it is the one way to misread the result: **a gate win does not mean the
engine got stronger today.** It means the evaluation carries signal worth making fast.

## What already exists

`rotor-evolve` is already a match runner, and a more complete one than this needs:

| piece | what it does |
|---|---|
| `pairing.rs` | the whole match format: best-of-N, first past the win threshold, tiebreak pairs until someone is strictly ahead. Resumable from `results.db`. |
| `pairing.rs::play_one_game` | the game loop: legal-move generation, checkmate, stalemate, fifty-move, threefold, clocks, PGN annotations |
| `results_db.rs` | per-game records, and the resume key `(round, engine_one, engine_two)` |
| `pgn.rs` | annotated PGN output, cutechess-shaped `{score/depth time}` comments |
| `book.rs` | the opening book, one position per game pair so both colours play it |
| `bracket.rs` | seeding, so a two-participant tournament is exactly one round-1 pairing |
| `engines.rs` | `data/engines.yaml`, resolving a participant name to a binary and UCI options |
| `uci_client.rs` | the subprocess half: handshake, `position`/`go`/`bestmove`, zero-tolerance faults |

So this is an extension, not a new binary. A gate is a tournament with two participants, which
`bracket::round_one_pairings` already turns into the single pairing `rotor-evolve` already plays.

Three things are missing: a player that runs in-process instead of over UCI, a fixed-depth control
(`MatchSpec` carries only Fischer time controls and `TimeControl::parse` only accepts `"3+2"`), and a
way for the registry to name a `.nnue` file.

## The cost, corrected

The projection printed by `nnue_bench.rs` needs one correction, and it changes the design rather than
just the schedule.

The measured baseline -- 3 games/s hand-crafted at depth 9 -- is a **16-thread** figure. So dividing
it by 137 gives a 16-thread figure too: 0.022 games/s, one game every 46 s of *throughput*. Per game
in isolation the numbers are:

```
hand-crafted, one thread:   3 games/s / 16  =  5.3 s per game
network, one thread:        5.3 s x 29      =  154 s per game  (2.6 minutes)
network, 16 concurrent:     154 s / 16      =  9.6 s per game of throughput
```

**500 games is 1.3 hours with 16 games running at once, and 21 hours sequentially.** Parallel game
execution is therefore a requirement of this design, not an optimisation to add later. That is the
single most consequential thing the measurement says.

### The unit of work is a game pair, one per thread

Decided with the developer: **each thread plays one opening, then the same opening with the colours
reversed, and applies both games' updates when the pair finishes.** So a thread owns a pair, not a
game, and colour balance holds by construction rather than in expectation -- no run can end having
given the challenger one more White than Black.

This fits what `pairing.rs` already does. It computes `pair_index = games_played / 2` and gives both
games of a pair the identical `starting_fen`, alternating `engine_one_is_white` on
`games_played % 2`. The pair is already the natural unit; today it is simply played sequentially by
one loop instead of owned by a worker.

So the concurrency unit is 16 pairs in flight, 32 games, and a pair costs about 5.1 minutes of thread
time: **16-way parallelism retires a pair every 19 s.**

## Determinism, and what it does to the match format

At fixed depth, with one search thread, **both players are completely deterministic**. There is no
clock to jitter and no Lazy SMP nondeterminism. Three consequences, and the third is the one that
matters:

1. **The whole match is reproducible.** Same config, same result, bit for bit. That is a real
   property to have in a gate -- a surprising outcome can be re-examined rather than re-rolled.
2. **Search must stay single-threaded per game.** Lazy SMP is nondeterministic by construction, so
   parallelism has to come from playing many games at once rather than many threads per search. That
   is the same shape self-play already uses, and it is what the 3 games/s figure was measured under.
3. **The opening book is the only source of variety, so a pair is the sample and there is nothing
   else to average over.** Re-running gains nothing -- the score is a deterministic function of which
   openings were used. The only way to tighten the answer is more pairs, meaning more openings.

That third point is what makes a sequential test the right stopping rule rather than a fixed game
count: there is no noise to beat down by repetition, only information to accumulate by drawing more
openings, and a test that stops as soon as the answer is clear is worth a great deal when a pair costs
5.1 minutes of thread time.

## The decision rule: see `sprt.md`

**The test is not to be invented here, and not restated here either.** `sprt.md` is the single
specification: the three chosen constants, the derived chain, the score-linear LLR, why the error
bounds hold without a draw-rate estimate, the opening-pair decision, and the cost.

One thing recorded there that this document must respect rather than re-derive: a completed pair
applies **two ordinary per-game updates**, one per game. There is no pair-level observation, so
pairing below is a scheduling and colour-balance decision and nothing in the statistic changes.

An earlier draft of this document reinvented the test as a normal approximation with a variance
estimate, which is exactly the substitution `sprt.md` warns against. Deleted. That is why the test now
has a document of its own.

## The shape of the change

### A `Player` trait

`play_one_game` today calls `UciClient` four ways: `new_game`, `set_position`, `go`,
`wait_for_bestmove`. That split is UCI's shape, not a game loop's. Collapsing it to one call per move
lets an in-process player be simple instead of pretending to be a protocol:

```rust
pub trait Player {
    fn new_game(&mut self) -> Result<(), Fault>;
    fn choose_move(&mut self, request: &MoveRequest, stop: &Arc<AtomicBool>) -> Result<MoveOutcome, Fault>;
    fn shutdown(&mut self);
}

pub struct MoveRequest<'a> {
    /// What a UCI player needs: it replays the game from a FEN plus a move list.
    pub starting_fen: &'a str,
    pub moves_uci: &'a [String],
    /// What an in-process player needs: the position itself, and the ancestor hashes its own
    /// repetition detection reads.
    pub board: &'a Board,
    pub ancestors: &'a [u64],
    pub control: Control,
}
```

Both halves of the request are already in hand at the call site -- `play_one_game` tracks `board`,
`ancestors`, `starting_fen` and `moves_uci` today -- so each implementation takes what it needs and
neither has to reconstruct anything.

### `UciPlayer` and `InProcessPlayer`

`UciPlayer` is a thin wrapper over the existing `UciClient`, doing `set_position` then `go` then
`wait_for_bestmove` inside one `choose_move`. No behaviour change; external engines keep working
exactly as they do.

`InProcessPlayer` owns an evaluator and **its own transposition table**:

```rust
pub struct InProcessPlayer {
    evaluator: Option<Arc<NaiveNetwork>>,   // None = hand-crafted
    tt: TranspositionTable,
    tablebase: SyzygyTablebase,
}
```

**The separate table is a correctness requirement, not a fairness nicety.** A transposition entry
stores a score produced by one evaluation. Two players sharing a table would read each other's
scores, and the resulting engine would be neither of the two things being compared. Cleared on
`new_game`, same as a real engine clears its hash between games.

`choose_move` is then `choose_best_move(board, depth, None, ancestors, &tt, &tablebase, evaluator, 0,
stop)` -- already possible, since that signature took the evaluator in the last change.

**Fault handling becomes vacuous for it**, and that is worth writing down rather than leaving as a
puzzle: an in-process player cannot crash the way a subprocess can, cannot time out (no clock), and
cannot return an illegal move (it generates legal moves). Its `Result` is `Ok` except for
`MoveOutcome::Interrupted`. The zero-tolerance fault machinery stays for UCI players, where it earns
its keep.

### `Control`: Fischer or fixed depth

```rust
pub enum Control {
    Fischer(TimeControl),
    FixedDepth(u32),
}
```

Parsed from the same config string `MatchSpec` already carries: `"3+2"` stays Fischer, `"depth 9"`
becomes `FixedDepth(9)`. One field, one grammar extension, no new config concept -- and both controls
work for both player kinds (UCI gets `go depth 9`; in-process passes the depth and a `None` budget).

With a fixed depth there is no faster tiebreak control to fall back to, so `tiebreak_time_control`
simply carries the same depth. Tiebreak pairs are then just more games at the same depth, which is
what they should be.

### The registry gains a `kind`

```yaml
engines:
  hand-crafted:
    kind: builtin
  amanda-e5:
    kind: network
    net: data/networks/amanda/lr-1e-3/checkpoints/step_000080896.nnue
  stockfish:
    kind: uci
    path: stockfish
    options:
      Threads: 1
```

`kind` defaults to `uci`, so every existing `engines.yaml` keeps working unchanged. `builtin` and
`network` both build an `InProcessPlayer`; only the evaluator differs.

## Configuration

```yaml
name: amanda generation 0 gate
format: single-elimination
participants: [amanda-e5, hand-crafted]
match:
  base_time_control: "depth 9"
  tiebreak_time_control: "depth 9"
  decision: sprt
  sprt:
    elo_d: 35.0
    alpha: 0.05
    beta: 0.05
  threads: 16
```

The three `sprt` values are exactly the three `sprt.md` says are chosen; everything else in the test is
derived from them at construction, never written here as a decimal.

`decision` defaults to `best-of` so every existing tournament config keeps working, and `best_of` is
simply unread when `decision: sprt`. Two participants, so `bracket::round_one_pairings` yields exactly
one pairing and the existing `rotor-evolve <tournament-id>` entry point plays it -- no new binary and
no new command.

`threads` is how many pairs run at once, 16 here. In the config rather than defaulted to the core
count, because the per-game cost differs by 29x between the two player kinds, so oversubscribing
would leave the wall-clock estimate meaningless even though correctness is unaffected.

## Resume

`results_db.rs` already keys games on `(round, engine_one, engine_two, game_index)`, so resuming after
Ctrl+C means replaying the SPRT over the games already recorded and continuing from the next pair.
Two things follow, and both are properties worth having rather than work to do:

- **The LLR is recomputed from the database, never persisted.** It is a pure function of the recorded
  results, so there is no second piece of state that could disagree with the games.
- **A pair must be recorded atomically.** A crash between a pair's two games would otherwise leave a
  colour-unbalanced fragment that the recomputation would have to special-case. Writing both games in
  one transaction, or discarding an odd trailing game on resume, keeps the invariant that the database
  only ever holds whole pairs.

## Loose ends found while reading, both minor

- **`book::opening_fen` rescans the file per call**, and its own doc comment justifies that with "a
  handful of pairs at most". At 100 pairs that assumption no longer holds: each call walks a 2.6M-line
  file. It is still only a few minutes against a 6-hour run, so not worth fixing for correctness --
  but `RandomBook` in the same module already keeps a byte offset per line for exactly this reason,
  and switching is cheap if the overhead ever shows up.
- **`pairing.rs` has one `#[allow(clippy::too_many_arguments)]` already**, and batching plus a
  `Player` generic will not make that better. Worth bundling the per-pairing constants into a struct
  while touching the file anyway, rather than adding a ninth positional argument.

## Deliberately not in this

- **No Elo point estimate, no cap, no draw-rate estimate.** All three are `sprt.md`'s own decisions,
  with the reasoning there.
- **No UCI `EvalFile`.** Deferred with the rest of the UCI work. The gate drives `Search` directly, so
  nothing here needs the protocol; when UCI support does land, `UciPlayer` will already be able to
  point at a network through the registry's existing `options` map.
- **No optimisation of the network.** 29x is affordable for a gate and still prohibitive for
  generation-1 data (~15 days for a comparable dataset, recorded in `TODO.md`). The AVX2 pass already
  took the evaluation from 60.2 to 11.6 microseconds; the next real step is int8 quantization, since
  the value head streams 688 KB of `f32` weights per evaluation and is bandwidth-bound rather than
  arithmetic-bound. The gate is what decides whether the network is worth that work, so it runs first.

## Nothing left open

`elo1 = 35` was the last open question and it is settled, recorded with its reasoning in `sprt.md`.
