# Rotor Transformer: depth-0 (Search(0)) convergence experiment

**Archived:** never implemented, and built on `rotor-curate`, `prepare` and the Lumbra teacher, all gone. Its question (can the transformer learn the hand-crafted evaluation at depth 0) passes to the transformer's MCTS redesign; see rotor-transformer.md.

Resume with: "resume from rotor-transformer-depth0-experiment."

## Status

Design settled through discussion with the developer; not yet implemented. `data/networks/zelda/
net.yaml` exists but is now stale (see "Config: `zelda`" below -- it needs a `preprocessing`
section again, which it doesn't have yet). A prerequisite fix (see below) to `rotor_curate.rs` and
`labels.rs`, found during this session's own design discussion, needs to land first -- it also
means the existing `lumbra` teacher needs regenerating before `amanda` trains against it again.

**Revised, this session: the "no rotor-curate, new `depth0` subcommand, stream the PGN directly"
plan below this note (originally this document's whole design) is superseded.** It ran into a real
resumability gap -- streaming straight from a 729MB PGN with no persisted cache means every restart
re-does the whole expensive replay, and rebuilding the in-memory held-out test set has the same
problem. The developer proposed reusing `rotor-curate`/`teacher.yaml`/`prepare`'s existing,
already-resumable machinery instead, generalized along two independent axes (search depth, and
label-blending lambda) rather than bypassed. That generalized design is what's described below the
Goal/Label sections; the old plan is kept underneath, clearly marked, for the record rather than
deleted.

## Goal

Test whether the full-attention transformer (`rotor-transformer.md`'s shared encoder plus value
head, `rotor-transformer-implementation.md`'s implementation) can learn the hand-crafted
`evaluate()` function itself, at depth zero -- decoupled from search-label noise entirely, and (once
the prerequisite fix below lands) from game-outcome blending completely, with zero exceptions. If
the architecture can't even represent this simpler, noise-free target, there is no point chasing
convergence on the harder AB(lambda)/Search(9) label `amanda` trains against. `training.md` already
names this exact concept: "`Search(0)` (a single `evaluate()` call) is exactly a neural network's
forward pass in chess-engine vocabulary."

This is a separate, narrower diagnostic run from `amanda`'s own training -- not a replacement for
it, and not expected to produce a deployable network.

## Prerequisite fix: scoring off-by-one in `play_game`, and `compute_labels`'s hardcoded base case

Found this session while working through why `lambda = 0` didn't give a fully clean label, and
**settled as a fix to land before the depth-0 experiment itself, not specific to it** -- it changes
what `plies.score` means for every teacher, present and future.

**Bug 1: `rotor_curate.rs::play_game` scores the board *after* `board.make_move(mv)`, not before.**
Every other use of `choose_best_move` in this codebase (real UCI play included) searches the
position not yet moved from and returns a recommendation for what to play there -- scoring after
the move silently asks a different question ("what does search think one move later") than the one
actually wanted ("what does search think of the position the historical game was really at when
this move was chosen"). Concretely, this also means: the starting position of every game was never
scored or trained on at all (it's never `board` at the top of any loop iteration before a move has
been made), and the "terminal position, no legal moves, exclude it" check (`positions_excluded`,
`Option<i32>`, the `reports.last() -> None` branch) was catching the *wrong* position relative to
what a corrected scoring order actually needs.

**Bug 2: `labels.rs::compute_labels` hardcodes `labels[n - 1] = outcome`**, unconditionally, never
blended by lambda -- the "Known residual" this document originally accepted as a trade-off (see git
history of this file). Once Bug 1 is fixed, the natural fix for this one falls out directly, not as
a separate patch: a game with `n` real scored positions (`scores[0..n-1]`) has exactly one further,
*never-scored, never-emitted* position -- `position[n]`, the actual final board, whatever ended the
game (mate, stalemate, resignation, draw). Its value is `outcome` by definition (nothing to
bootstrap it from, no search was ever run on it), and every real position feeds off it through the
same recursive formula, with no hardcoded exception for the last real index:

```rust
pub fn compute_labels(scores: &[i32], outcome: f64, lambda: f64) -> Vec<f64> {
    let n = scores.len();
    assert!(n > 0, "compute_labels called with no scores");
    let mut labels = vec![0.0; n];
    let mut next = outcome;                 // the virtual labels[n], never itself emitted
    for t in (0..n).rev() {
        labels[t] = lambda * next + (1.0 - lambda) * score(scores[t]);
        next = labels[t];
    }
    return labels;
}
```

At `lambda = 0`, every real, trained-on position now gets `score(scores[t])` with zero exceptions --
including what used to be index `n - 1`. The "Known residual: last-ply contamination" this document
previously accepted is fully resolved by this fix, not just documented as a trade-off.

**Concrete changes, together:**
- `rotor_curate.rs::play_game`: move the scoring call to before `board.make_move(mv)`, for both the
  `depth > 0` case (`choose_best_move`) and the `depth == 0` case (`eval::evaluate(&board)` directly
  -- see "`rotor-curate generate --depth`, settled" below for why that's its own branch, not a value
  passed into `choose_best_move`). **`ancestor_hashes` must be seeded with the starting position's
  own hash (`vec![board.hash()]`), not left empty** -- caught during implementation, not part of the
  original plan: `is_repetition`'s own tests confirm a position's ancestor list must include *its
  own* hash at the moment it's checked, and `play_self_play_game` already seeds this way. Reordering
  the scoring call without also seeding this would leave every scored position missing from its own
  ancestor list, silently weakening repetition detection. `ancestor_hashes.push(board.hash())` still
  runs after `make_move`, unchanged, extending the list for the next iteration. Drop `Option<i32>`,
  `positions_excluded`, and the terminal-board `None` check entirely -- every scored position now
  has at least one legal move by construction (it's the position a move is about to be chosen from).
  `PlyRecord`/`PlayedGame` lose their `positions_excluded` field.
- `dataset.rs::GameSummary`: the doc comment explaining why `ply_count` differs from
  `games.ply_count` (the trailing-excluded-ply distinction) no longer applies -- the two collapse
  into the same number, always.
- `labels.rs::compute_labels`: the rewrite above.

**Blast radius: this isn't scoped to `lumbra-zero`.** It changes what `plies.score` means for every
teacher. The existing `lumbra` teacher (restored earlier this session from `lumbra.sql.gz`) was
built under the old, buggy scoring order -- it needs to be regenerated from scratch via
`rotor-curate generate` once this fix lands, not reused as-is. The developer has since renamed its
directory to `data/teachers/lumbra-rubbish/` to mark it clearly as stale; the real `lumbra` teacher
needs rebuilding fresh at `data/teachers/lumbra/` once the fix lands, under a `rotor-curate
generate` run with the corrected `play_game`. No `positions.cache`/label files exist yet against it,
so nothing downstream needs migrating, only the
teacher database itself.

## Label

`score(cp) = sigmoid(log(2.0) * cp / 100.0)` (`training.md`'s "Centipawn scaling," reused
unchanged) applied to a raw per-ply score. For this experiment that raw score is `Search(0)` --
literally `evaluate(board)` (`src/eval.rs`), no tree at all -- rather than `Search(9)`.
`evaluate()` already returns a side-to-move-relative value (`Color::White => balance, Color::Black
=> -balance`), matching the transformer's own own/opponent-relative token vocabulary directly.

## Two independent axes: search depth, and label-blending lambda

Settled this session, replacing the original "skip rotor-curate entirely" plan: depth and lambda
are two separate knobs, on two separate pipeline stages, and both should be generalized rather than
special-cased for this one experiment.

- **Depth** is a property of a *teacher* -- which raw per-ply score `rotor-curate generate` writes
  into `plies.score` (renamed `teacher.db` this session, see below). `rotor-curate generate` gets a
  new `--depth <N>` flag (default `9`, preserving every existing teacher's behavior unchanged);
  `--depth 0` writes `evaluate()`'s own raw output with no search tree at all. This experiment needs
  a new teacher, **`lumbra-zero`**, built via `rotor-curate generate --depth 0` against the full
  Lumbra corpus (`data/games/LumbrasGigaBase_OTB_ELO2400.pgn`) -- distinct from `amanda`'s own
  `lumbra` teacher (`Search(9)`). Both `lumbra` and `lumbra-zero` need building fresh via the fixed
  `rotor-curate generate` -- the database restored earlier this session now sits at
  `data/teachers/lumbra-rubbish/`, renamed by the developer to mark it stale, not at
  `data/teachers/lumbra/`.
- **Lambda** is a property of a *label file* derived from one teacher's raw scores via
  `rotor-train prepare` -- `labels.rs::compute_labels`'s own blending weight. Previously hardcoded
  (`DEFAULT_LAMBDA = 0.7` in `main.rs`, never exposed anywhere). Settled: `prepare` takes an
  explicit `--lambda <value>` flag. This experiment wants `--lambda 0` -- with the prerequisite fix
  above, this now means the raw score directly, at every position, with no exceptions (see "Last-ply
  contamination: resolved" below).

These compose freely: any teacher (any depth) can have label files built at any lambda, and a
teacher can accumulate label files for several different lambda values over time without
rebuilding anything depth-related.

## Cache format, split: positions vs. labels

Settled this session, replacing the original single-record `position + one label` cache format for
this pipeline (`cache.rs::write_record`) with two separate kinds of file, so that adding a new
lambda value never re-pays the cost of the expensive part (PGN replay, `extract_canonical`):

- **`positions.cache`**: one shared set of files per teacher (the existing `31`-bucket, `2..=32`
  piece-count convention, one file per bucket per `{train, test}` split), holding only
  `CanonicalPosition` data -- no label field. Variable-width records exactly as today (no padding),
  built once per teacher regardless of how many lambda values are ever explored against it. Keeps
  its own `.count` sidecar (still needed -- records are variable-width, so a row count can't be
  derived from file size alone).
- **Label files**: one set per lambda value actually built, fixed-width (`f32`, `4` bytes) per row,
  in exactly the same row order as `positions.cache`. Proposed layout (not yet implemented):
  `data/caches/<teacher>/positions/{train,test}/<piece_count>.cache` (+ `.count`) and
  `data/caches/<teacher>/labels/lambda-<value>/{train,test}/<piece_count>.labels`. A labels file's
  row count is derivable directly from its own byte length (`file_size / 4`) -- no sidecar needed,
  unlike `positions.cache`.

**`positions.cache` stays keyed per teacher, not per PGN-source/corpus** -- considered and declined
by the developer. Two teachers built from the exact same PGN file(s) (e.g. `lumbra` and
`lumbra-zero`) would in principle produce byte-identical `positions.cache` files, since position
content/bucketing/train-test split never depend on search depth -- but `teacher.yaml`'s own
`pgn_files` is genuinely per-teacher (two teachers can name different PGN lists), so sharing by
corpus identity would need its own derived key (e.g. a hash of the sorted file list) to detect when
two teachers actually do agree. Not worth the added complexity for a benefit that only applies when
they happen to match -- accepted as a real but occasional redundancy, matching this project's own
"don't design for hypothetical future requirements" convention (CLAUDE.md rule).

**No fingerprint or version guard ties a labels file to the `positions.cache` it was built
against** -- considered and explicitly declined by the developer. Row `i` of any labels file is
trusted to correspond to row `i` of `positions.cache` by construction (both walk the same
deterministic replay/bucketing order), not by a checked invariant. A change to that replay/
bucketing logic between building `positions.cache` and building a later labels file would silently
misalign every row, with nothing to catch it -- a known, accepted risk, not an oversight.

**Storage impact of this split** (why it matters, not just a stylistic preference): a cache record
today is `5 + 2k + 4m` bytes (`k` = piece count, `m` = labels per record). Storing many lambda
values in one interleaved record (an earlier idea this session, also superseded) would have meant
paying for the `5 + 2k` position bytes again for every lambda explored -- e.g. `21` lambda values
(`0.00` to `1.00` step `0.05`) would have grown the full-corpus cache from roughly `2.9 GB` to
roughly `8.6 GB`. Splitting positions from labels means the expensive `5 + 2k` bytes are paid once;
each additional lambda value only costs `4` bytes/record (roughly `280 MB` for the full `71M`-ply
corpus per lambda value), and only for lambda values actually explored.

## `prepare`, settled: idempotent per (teacher, lambda)

`rotor-train prepare <teacher-name> --lambda <value>` -- `--lambda` is **required, no default**.
Now that lambda genuinely varies per invocation (unlike today, where every run used the same
hardcoded `DEFAULT_LAMBDA`), a silent default risks silently building the wrong lambda's label
files. `DEFAULT_LAMBDA`/`main.rs`'s current hardcoded constant goes away.

- If `positions.cache` doesn't yet exist for this teacher, build it (one full PGN replay,
  `extract_canonical` per position, bucketed by piece count -- same work `build_length_caches` did
  before this split, just no longer writing a label alongside each position).
- If the requested lambda's label files don't yet exist for this teacher, build them. **Revised
  during implementation: this is its own full, independent PGN replay too, not the "no PGN replay
  needed" version originally planned here.** Routing each label to the right bucket needs that
  position's own piece count, which `teacher.db` never records (only `plies.score`/`games.outcome`)
  -- so without *some* persisted routing, a label-only build has no way to know which bucket a given
  `(game, ply)` lands in. A routing sidecar (one byte per scored position, per split, written
  alongside `positions.cache`) was considered and costed out (~71MB raw for the full Lumbra corpus,
  less compressed) but declined by the developer in favor of simplicity: every `build_labels` call
  just re-derives piece counts via `extract_canonical`, the same replay cost `positions.cache`
  itself pays. `length_cache.rs`'s own module doc comment covers this.
- Either step is skipped if its output already exists. A first-ever `prepare <teacher> --lambda 0.7`
  builds both `positions.cache` and the `lambda-0.7` label files; a later `prepare <teacher>
  --lambda 0.0` against the same teacher reuses the existing `positions.cache` and only adds the
  `lambda-0.0` label files.

No separate `depth0` subcommand, and no separate lighter `NetConfig` variant -- both are unneeded
once this reuses the ordinary teacher/`prepare`/`train` pipeline. `zelda` is an ordinary network
like `amanda`, just pointed at a different teacher and a different lambda.

**`prepare`'s `positions.cache` build is resumable** -- in scope for this plan, per the developer,
now implemented (`length_cache.rs`'s `PositionCacheBuilder`/`PositionsResumeState`). Checkpoint
cadence is time-based, sharing `main.rs`'s own training-loop `BACKUP_INTERVAL` (5 minutes, moved
into `length_cache.rs` so both use the literal same constant, per the developer) rather than a
per-game checkpoint like `rotor-curate generate`'s -- PGN replay here is cheap (no search), so
redoing up to `BACKUP_INTERVAL`'s worth of it on a crash is an acceptable cost, and checkpointing
every single game would mean far more, far smaller zstd frames (worse compression). A checkpoint
seals each bucket's current zstd frame (`Encoder::finish`) and reopens a fresh one appended after it
(zstd decodes concatenated frames transparently); `PositionsResumeState` (`data/caches/<teacher>/
positions/resume.state`, magic-checked) carries the PGN file index/byte offset, the next game
sequence number, each bucket's running row counts (carried into the final `.count` sidecars), and
each bucket's exact file length as of that checkpoint. That file length is what makes a crash
*between* checkpoints safe: `resume` truncates every bucket back to it before reopening in append
mode, discarding whatever a crash left dangling in an unsealed trailing frame -- without this, a
resumed build could silently corrupt a bucket by reopening past a partial, undecoded frame.
`build_labels` (one lambda's own label files) mirrors this with its own, simpler `LabelsResumeState`/
`LabelCacheWriter` -- no zstd frame-sealing dance needed (label files are fixed-width, uncompressed
`f32`, so a checkpoint is just a `flush()`, and a crash-safe resume just truncates each bucket to
`row_count * 4` bytes).

## Last-ply contamination: resolved, not accepted

Earlier revisions of this document accepted a small, known residual here (roughly one
outcome-contaminated ply per game at `lambda = 0`), after an initial proposed fix (blending
`compute_labels`'s base case by lambda) was declined. Further discussion found the actual root
cause -- `play_game`'s scoring off-by-one -- and the fix described in "Prerequisite fix" above
resolves this completely, not as a trade-off: at `lambda = 0`, every real training example gets a
pure `score(evaluate(pos))` label with zero exceptions.

## Config: `zelda`

`data/networks/zelda/net.yaml` exists (`embedding_dim: 256`, `num_layers: 6`, `num_heads: 8`,
`feedforward_dim: 1024`, `dropout: 0.1`, `value_head.hidden_dim: [128, 64]`, `attention: full`,
`learning_rate: 1.0e-4`, `batch_size: 1024`, `steps_per_eval: 8192` -- copied directly from
`examples/net_transformer.yaml`), but **is now stale**: it has no `preprocessing` section at all,
left that way from the now-superseded "no teacher" plan. It needs one again:

```yaml
preprocessing:
  teacher: lumbra-zero
  lambda: 0.0
```

`net_config.rs`'s `Preprocessing` struct needs a new `lambda: f64` field alongside `teacher:
String` -- settled this session, replacing the earlier "give `depth0` its own lighter config type"
plan (now moot, since `zelda` is an ordinary teacher-backed network again). `train` should fail
loudly (CLAUDE.md rule 4: assertions, not eyeballing) if the requested lambda's label files don't
exist yet for that teacher, rather than silently falling back to some other lambda or panicking
deep inside a reader.

Full attention only, per the developer -- kings-only (`betty`) is out of scope for this experiment.

## Success criterion

No new metric code. The developer will judge convergence by eye: watch `history.tsv`'s train/test
loss columns and the existing patience/early-stopping behavior (same mechanism every other
`rotor-train` run already uses), not a hardcoded threshold or baseline comparison.

## `rotor-curate generate --depth`, settled

Checked directly against `rotor_curate.rs`: `SEARCH_DEPTH` is a hardcoded `const SEARCH_DEPTH: u32
= 9`, with a doc comment explicitly framing it as "permanently, not a stand-in for something
fancier later" -- a deliberate prior decision, not an oversight, from before a depth-0 need existed.
`TT_SIZE_MB`'s own `64` MB is likewise sized specifically around depth 9's measured footprint (per
that same comment).

Settled, over leaving `generate` untouched and building a separate tool for depth 0: revise
`SEARCH_DEPTH` into a real `--depth <n>` flag (default `9`, every existing teacher's behavior
unchanged), updating the doc comment to reflect that the depth-0 need genuinely didn't exist when it
was written. `generate`'s current CLI shape (`generate --threads <n> <pgn-file> <database-path>`,
parsed in `run_generate`/`main.rs` around line 505-536) gets `--depth <n>` alongside `--threads`,
same parsing convention.

**`depth == 0` is its own special case inside `play_game`, not a value threaded into
`choose_best_move`.** Checked directly against `search.rs::choose_best_move_with_progress`: its
iterative-deepening loop is `for depth in 1..=max_depth.min(MAX_PLY)`, starting at `1`, never `0` --
`choose_best_move(&board, 0, ...)` would return an empty `reports` and `None`, for every position,
indistinguishable from the terminal-position case, not `evaluate()`'s raw output. `depth = 1` isn't
equivalent to `Search(0)` either (it searches one real ply with quiescence at the leaves, not a bare
static evaluation). So `Search(0)` needs its own path: when `depth == 0`, `play_game` calls
`eval::evaluate(&board)` directly and skips `choose_best_move` entirely -- no transposition table,
no `ancestor_hashes`, no tablebase, no `SELECT_MOVE`/PV machinery of any kind, since, per the
developer, **curation never needs the best move, only the evaluation** (`play_game` already
discards `_best_move` from `choose_best_move`'s own return today, at every depth, so this loses
nothing `play_game` was using). This also means the per-worker transposition table allocation is
genuinely unnecessary work at `depth = 0` -- skipped entirely for that case, not sized-but-unused.

## Labels-file directory layout, confirmed

`data/caches/<teacher>/positions/{train,test}/<piece_count>.cache` (+ `.count`) and
`data/caches/<teacher>/labels/lambda-<value>/{train,test}/<piece_count>.labels`, as proposed --
confirmed by the developer.

## Open design points (not yet settled)

None remaining -- the resume-state file's field layout (the last open point) is settled and
implemented (see "`prepare`, settled" above).

## Superseded plan (kept for the record, not deleted)

Everything below this line was this document's entire design before this session's "prepare after
all" pivot. No longer current -- kept so the reasoning that led away from it isn't lost.

---

**Original pipeline: new `depth0` subcommand, no rotor-curate, no teacher database.** Labels
computed inline during a direct PGN replay (`eval::evaluate(&board)` -> `score(cp)`, no database
read anywhere), reusing `pgn_replay`/`features::extract_canonical`/`transformer_vocab`/
`transformer_batch`/`transformer_network`/`adamw`/`early_stopping` unchanged. Train/test split by
game via `builder::is_train_game`, substituting a sequential in-PGN game index for the (nonexistent)
database `game_id`. In-memory batching by piece count, buffering at most `batch_size * 31`
positions at once, flushing a `TransformerBatch` the moment one bucket filled. Cycling back to the
start of the PGN once exhausted, rather than stopping after a single pass. Invocation: `rotor-train
depth0 <network-name> <pgn-file...>`.

**Why it was abandoned:** no persisted cache anywhere meant no resumability -- an interrupted run
(or even just building the held-out test set, which needed its own full PGN pass) had to redo the
entire expensive replay from scratch, with no analog to `rotor-curate generate`'s own proven
resume-offset mechanism. The developer's "we may need a prepare step after all" reopened this, and
the two-axis (depth, lambda) generalization above reuses proven, already-resumable machinery
instead of building a parallel resumability scheme just for this one experiment.

## Progress

**Prerequisite fix landed and verified** (`play_game`'s scoring order, `compute_labels`'s
lambda-blended base case, plus the `nnue_network.rs` perspective test hardened against a real,
independent flake it surfaced) -- 160 tests passing.

**Main implementation landed, not yet built/tested by the developer:**
- `net_config.rs`: `Preprocessing.lambda: f64` added (required field); `examples/net_transformer.yaml`
  (lambda `0.7`, a generic example, not depth-0-specific), `examples/net_naive.yaml`/`net_nnue.yaml`
  (lambda `0.7`), and `data/networks/zelda/net.yaml` (its `preprocessing` section restored: `teacher:
  lumbra-zero`, `lambda: 0.0`) all updated to match.
- `cache.rs`: `write_position`/`read_position` factored out of `write_record`/`read_record` (the
  position-only encoding `positions.cache` needs).
- `length_cache.rs`: rewritten around the positions/labels split -- `PositionCacheBuilder` (resumable,
  zstd) and `LabelCacheWriter` (resumable, uncompressed fixed-width) replace the old single
  `LengthCacheWriter`; `LengthCacheReader` now joins a positions bucket with its matching label
  bucket; `build_positions_cache`/`build_labels` replace `build_length_caches`. Directory layout
  matches the confirmed `positions/{train,test}/<n>.cache`+`.count` and `labels/lambda-<value>/
  {train,test}/<n>.labels` shape. `BACKUP_INTERVAL` moved here from `main.rs`, now shared.
- `main.rs`: `run_prepare` takes `--lambda <value>` (required, `parse_lambda_flag`), builds
  `positions.cache` then the requested lambda's labels; `run_train_nnue`/`run_train_transformer`
  resolve both a `positions_dir` and a `labels_dir` (from `config.preprocessing.teacher`/`lambda`)
  and pass both into `length_cache::open_readers`. `DEFAULT_LAMBDA` removed.
- `round.rs`: its own test fixtures (the only other compiled module touching the old
  `LengthCacheWriter`/`open_readers` API) updated to the new positions/labels directory pair.
  (`nnue_round.rs`, `transformer_round.rs`, `transformer_window.rs`, `transformer_config.rs` also
  reference the old API but are dead files, not declared as modules in `main.rs` -- confirmed,
  untouched.)

**Revised during implementation:** `build_labels` always redoes its own full PGN replay rather than
reusing a persisted routing table from `positions.cache`'s own build (see "`prepare`, settled"
above) -- the developer declined the routing-sidecar alternative.

**Not yet done:**
- Developer needs to run `cargo build --features training` and the test suite.
- Actually run `rotor-curate generate --depth 0` to build the `lumbra-zero` teacher, and regenerate
  the real `lumbra` teacher (depth 9) fresh, replacing the stale `lumbra-rubbish`.
- Run `rotor-train prepare lumbra-zero --lambda 0.0`, then `rotor-train train zelda`.
