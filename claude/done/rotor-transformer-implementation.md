# Rotor Transformer Implementation (value head, convergence check)

**Done:** the value head, its batches, vocabulary and length-bucketed caches are implemented, and the code stays as is until the transformer redesign (MCTS, PUCT). References to `prepare`, `data/networks/<name>/net.yaml` and the amanda/betty runs are historical; those names now mean NNUE lineages. See rotor-transformer.md and architectures.md.

Resume with: "resume from rotor-transformer-implementation."

## Status

Implementation plan for the Rotor Transformer's **value head only** -- see `rotor-transformer.md`
for the settled architecture. Scope is deliberately narrow: prove the value head converges in
training, speed aside. The policy head and the play-time bitboard forward pass (required by
project rule 7 once anything here actually ships for real play) are both out of scope for this
first pass.

**The NNUE and transformer CLIs are allowed to drift apart, deliberately.** The existing
naive/HalfKP/HalfKAv2_hm networks keep their current flag-based invocation (`--lr`, `--lambda`,
`--features`) untouched. The transformer gets its own, different invocation style: a single
config-file argument (see "Config file" below) instead of CLI flags, and there is no expectation
the two ever converge back onto one shared interface.

**Config file, not CLI flags -- for the network's own shape and training recipe.** Every run
lives in a named directory, `data/networks/<name>/` (e.g. `data/networks/amanda/`,
`data/networks/betty/`), holding a `net.yaml` with all of that run's settings -- architecture
hyperparameters (embedding dimension, layer/head counts, feed-forward width, dropout, value-head
MLP shape), attention mode (full/kings-only), and training settings (learning rate, batch size,
`steps_per_eval`). Both `prepare` and `train`, for the transformer, take that config file's path
as their argument. Checkpoints and history are written next to `net.yaml`, inside that same run
directory -- but the length-bucketed cache files are *not* (revised below, "Cache files are
shared by teacher, not by run"). AB(lambda)'s own blending weight is *not* a `net.yaml` field --
hardcoded (`main.rs`'s `DEFAULT_LAMBDA`, shared with the NNUE track's own `--lambda` default) for
now, per the developer, since every run trained so far uses the same value anyway. **Job-scoped
stopping controls (`--patience`/`--min-steps`) are the one exception, and stay CLI flags rather
than `net.yaml` fields** -- settled later in the same session, once real use surfaced the
distinction ("Job-scoped stopping controls" in the Progress log below): they're properties of the
current running job, not of the network's own shape or training recipe.

**Cache files are shared by teacher, not by run (revised, settled during the first real smoke
test).** `data/networks/<name>/` was originally where a run's length-bucketed cache files lived
too, alongside its checkpoints -- but the cache files are a pure function of which PGN(s) a
teacher's own database was labeled from, never of a network's own `architecture`/`training`
settings. Once two networks (`amanda`: full attention, `betty`: kings-only) were set up training
against the exact same teacher, that meant redoing the same real PGN-replay pass and duplicating
the same `31`x`2` cache files twice for no reason. Settled instead: a teacher is its own
first-class, named thing, with its own directory (`data/teachers/<teacher>/`) holding its own
`teacher.db` (the `rotor-curate generate` output -- briefly renamed `labels.db` once it got a
directory of its own, then renamed back to `teacher.db`, per the developer) and its own
`teacher.yaml` (`pgn_files`, and nothing else -- schema in `examples/teacher.yaml`). `net.yaml`'s
own `preprocessing` section shrinks to just a `teacher` name; `main.rs`'s
`teacher_database_path`/`cache_dir` derive the real database path
(`data/teachers/{teacher}/teacher.db`) and the cache directory (`data/caches/{teacher}/`) from that
one name, and `teacher_name` recovers a teacher's own name from `teacher.yaml`'s containing
directory (never repeated inside the file itself, the same way a network's own name comes from
`net.yaml`'s containing directory, not a field). Every `net.yaml` naming the same teacher now
shares one `prepare` pass and one set of cache files automatically -- `data/networks/<name>/` holds
only what's genuinely per-run (checkpoints, `history.tsv`, model/optimizer/window-progress state),
never the caches.

**`prepare` takes a teacher config, `train` takes a network config -- two different config kinds
behind the same "one path argument" dispatch shape.** Once cache files moved to being keyed by
teacher, pointing `prepare` at any one network's `net.yaml` became genuinely arbitrary (`prepare`
never reads `architecture`/`training` at all, so `amanda`'s and `betty`'s `net.yaml`s would
produce byte-identical results) -- caught by the developer testing the pipeline for the first
time. Settled: `prepare <teacher.yaml>`, `train <net.yaml>`, both still exactly one path argument
(so `main.rs`'s existing "exactly one argument -> transformer track" dispatch rule needs no
per-subcommand special case), just pointed at the config kind each subcommand actually needs.

**Teacher config supports multiple PGN sources, resolved against one shared `data/games/`
directory.** `teacher.yaml`'s `pgn_files` is a list of bare filenames, not full paths (per the
developer, after `data/lumbra/` was renamed to `data/games/` -- every real PGN lives there now,
one shared pool every teacher draws from, so a teacher config only spells out the part that
actually varies). `length_cache::build_length_caches` replays them as one concatenated stream, in
list order, moving to the next file the instant the current one runs out (`Ok(None)`) so
`sequence`/`game_id` numbering stays contiguous across the boundary exactly as if every file had
been one long stream all along -- this only ever matters if a teacher's own database was itself
built by concatenating multiple PGN sources in that same order; `rotor-curate generate` itself
still only takes one PGN path per invocation, unchanged.

`data/teachers/`, `data/networks/`, `data/caches/`, and `data/games/` are the four top-level
`data/` groups now, each named after what it holds, all still gitignored wholesale. `run_dir`,
`cache_dir`, and `teacher_name` (all in `main.rs`) dropped an earlier `_of` suffix (`run_dir_of`,
`cache_dir_of`, `teacher_name_of`) for naming consistency, per the developer -- a pure rename, no
behavior change.

## Grounding: how this fits the existing `rotor-train` codebase

Researched directly against the current code (`src/bin/rotor_train/`), not assumed:

- Raw positions are retained end-to-end as a variable-length `CanonicalPosition`
  (`features.rs::extract_canonical`) through the cache files (`cache.rs`, `reader.rs`) --
  reusable as-is, no upstream changes needed. Only the final `expand()`/`to_dense_batch` step
  (`batch.rs`) is specific to the three existing fixed-width networks (naive/HalfKP/HalfKAv2_hm);
  the transformer needs its own parallel step there (token-sequence batch construction), not a
  change to anything upstream of it.
- Labels (`plies.score`) come from a SQLite DB built by `rotor-curate` (the existing AB(9)/λ label
  generation), reused unchanged -- already in the sigmoid-bounded expected-score scale the whole
  project uses (see "Centipawn scaling" in the companion book), so no label-format conversion is
  needed.
- `tch` 0.24 (`Cargo.toml`) has no built-in transformer/attention module (`nn::TransformerEncoder`
  or `nn::MultiheadAttention` don't exist in this version). Attention must be hand-built from
  `nn::linear` plus tensor ops (matmul/softmax) -- the same way `tch`'s own `vision/dinov2.rs`
  hand-builds a ViT internally, confirming this is the intended pattern for this library version.
  `tch` does expose the raw `Tensor::scaled_dot_product_attention` ATen op as a lower-level
  building block, which supports an attention-mask argument -- settled below (point C) as the one
  to use for the core attention computation, rather than hand-rolling matmul/softmax.
- `Network` / `FeatureSet` / `DenseBatch` are all small enums, one variant per feature set,
  dispatched via `match`, with a `feature_set.state` marker file persisting the choice across
  resumed runs. **Revised, after the config-file/no-padding/length-bucketing decisions below
  settled: the transformer does *not* get a fourth variant on any of these three.** Their whole
  shape exists to serve `main.rs`'s CLI-flag parsing (`FeatureSet::from_cli_str`) and
  `window::run_window`'s single-cursor, single-train-cache-file loop -- machinery the transformer
  bypasses entirely (own config file, 31 length-bucketed cache files with per-length cursors, own
  training loop). `DenseBatch`'s existing variants are all fixed-width dense tensors; the
  transformer needs variable-length token/position/mask tensors, nothing shaped like the existing
  variants. Adding `Transformer` arms to these three enums would only add dead surface area with
  no shared caller, not real reuse -- so the transformer gets its own standalone types instead
  (a `TransformerNetwork` struct, its own batch struct), and `main.rs` branches to entirely
  separate `run_prepare_transformer`/`run_train_transformer` functions on argument shape (a single
  config-file path vs. the existing flag list), never touching `FeatureSet`/`Network`/`DenseBatch`
  at all.

## Plan

1. **Vocabulary & token-sequence construction.** New module implementing the settled 569-token
   piece-centric vocabulary and the `CanonicalPosition` -> relative-coordinate token-index sequence
   conversion (own/opponent split, `K < Q < R < B < N < P` ordering, square ordering, king
   castling-rights suffix, pawn en passant star). Unit-tested against known positions (project
   rule 4: correctness via assertions, not eyeballing).
2. **CLI dispatch, standalone types.** `main.rs` branches to new `run_prepare_transformer`/
   `run_train_transformer` functions on argument shape (a single `net.yaml` path vs. the existing
   flag list) -- no `FeatureSet`/`Network`/`DenseBatch` variant added (settled below: those three
   enums' whole shape serves machinery the transformer bypasses entirely). The transformer gets
   its own standalone `TransformerNetwork` struct (step 4) and its own batch struct (step 3),
   neither nested inside the existing enums.
3. **Batch construction.** A new function, parallel to `to_dense_batch`, turning a batch of
   `CanonicalPosition`s (all the same piece count, per the no-padding design below) into token-
   index tensors, positional-index tensors, and attention-mask tensors (open design points below
   cover the mask construction).
4. **Model.** Learned token embedding + learned positional embedding, hand-built multi-head
   self-attention and feed-forward transformer blocks (BERT-like, LayerNorm, GELU), a `[CLS]`-read
   value head MLP ending in sigmoid. Full and kings-only attention share this same implementation,
   differing only in the mask passed in.
5. **Training loop integration.** Wire into the existing `rotor-train train` CLI, reusing
   checkpointing conventions where they still make sense (open design point below).
6. **Run the convergence check.** Train against the existing cached data, watch the loss curve.
   No play-time bitboard forward pass yet -- that's separate future work, contingent on this
   convergence check actually succeeding.

## Open design points

- **A. Settled: two separately trained models, sharing code.** Same parameter count either way
  (the attention mask is a forward-pass detail, not a weight-shape one), but full and kings-only
  are trained as two independent runs, producing two separate checkpoint files -- not one set of
  weights with the mask as a post-hoc switch. Step 2's enum needs two variants (or one variant
  plus a mode field that also selects which checkpoint namespace/CLI value is used), not a single
  shared checkpoint.
- **B. Settled: one cache file per piece count, mixed at read time -- never touching the
  database.** Training in plain PGN order (today's behavior for naive/HalfKP/HalfKAv2_hm too) is a
  poor fit for variable-length inputs: it's both non-i.i.d. (consecutive plies from the same game)
  and gives no control over per-batch piece-count spread.

  The `rotor-curate` database can't help with this regardless of whether it's busy right now: it
  only ever stores `plies.score` labels, never positions. Positions exist only as transient
  PGN-replay state inside `builder.rs`, or once captured into `CanonicalPosition` in the cache
  files `prepare` builds -- so piece count (`CanonicalPosition::pieces.len() + 2`) has to come from
  there, not the database, permanently, not as a stopgap.

  Storage: instead of one train-cache file, `prepare` writes `31` of them, one per piece count
  (`2..=32`) -- it already computes each position's exact piece count via `extract_canonical` on
  the very pass it writes the cache today, so this just changes which file a record is appended to,
  no second pass and no in-memory index of any kind, at any dataset size. A batch is built by
  reading the next `N` records from one length's file.

  **Settled: row count lives in a small sidecar file, not the cache's own header.** Unlike the
  existing single-file NNUE cache, a length file's row count genuinely can't be known before the
  pass that writes it starts (piece count only comes from replaying the game, never from
  metadata `count_rows_per_split` could read up front) -- so `cache::CacheHeader` (row count
  written first, inside the zstd stream itself) doesn't fit here at all. Rather than a real second
  pass just to pre-count (doubling the PGN replay, `extract_canonical`, and per-game SQL work) or a
  scratch-then-recompress finalization step (doubling every record's own write), each length file
  gets a tiny separate `<path>.count` sidecar (`length_cache.rs`, magic-checked like every other
  format in this codebase) written once, after the single real pass finishes and the true count is
  already sitting in memory. The `.cache` file itself carries no header at all any more, just a
  zstd-compressed stream of `cache::write_record` calls -- reusing that function, and `read_record`,
  completely unchanged. A genuine improvement over `build_caches`'s own row-count pre-pass, not just
  a workaround: this need never touch `game_summaries`' metadata trick at all.

  **Settled: no padding, ever -- every batch is exactly one length, not "minimally padded."**
  Since a batch only ever draws from one length-file, there's no length variation within a batch to
  pad away in the first place -- earlier phrasing here ("padded dynamically to that batch's own
  max") was stale, left over from the in-memory-permutation design this superseded, and is wrong:
  fixed now. Real consequence for the reader: it must **not** reuse `read_batch_cyclic` as-is,
  since that function always wraps mid-batch to return exactly `batch_size` records, silently
  mixing the tail of one pass with the head of the next -- which would pad a short remainder out to
  full size using wrapped-around (partially repeated) data. Instead, a length-file's batch read
  must stop short at that file's own `row_count` rather than wrapping within the call: **at most
  `31` batches may come back short (one per length-file, at most) within any given dataset
  window**, exactly when that file's cursor happens to reach the end of a pass during that window;
  the next call for that file then wraps its cursor to `0` and continues normally. A new reader
  function, not `read_batch_cyclic` itself -- left for step 3.

  **Training must still mix lengths, not consume the files in length order** -- a per-length split
  fixes padding waste but says nothing about training order on its own; visiting file `2` to
  completion, then file `3`, and so on would recreate the exact monotonic
  all-endgames-then-all-middlegames-then-all-openings sweep length-bucketing was meant to avoid.
  So: which length-file to read the *next* batch from is itself randomized throughout training, not
  fixed or sequential -- batches stay length-homogeneous internally, but consecutive batches jump
  between lengths. **Settled: randomized length pick, not deterministic round-robin** -- a real
  break from the existing single-file cyclic reader's own stated "no RNG, no seed" design
  (`reader.rs::read_batch_cyclic`'s doc comment), allowed per the drift note above.

  Each length keeps its own independent `read_batch_cyclic` cursor (unchanged mechanism, just `31`
  of them instead of `1`). **Resume needs `31` persisted cursors, not `1`** -- `WindowProgress`
  (`window_progress.rs`), which today saves a single `cursor: u64`, needs an array/map of cursors
  instead, one per length, so a resumed run continues every length-file exactly where it left off.
  The length-*picking* RNG itself needs no such persistence, unlike the cursors: picking which
  length to read from next is stateless and history-independent (today's next pick doesn't depend
  on yesterday's), so a resumed run can just start a fresh random stream -- nothing about it needs
  to be reproduced bit-for-bit the way cursor position does.

  **Settled: weighted by each file's own remaining-until-wraparound count, not total `row_count`
  and not uniform.** Weight for length `k` at a given step is `row_count_k - cursor_k` -- recomputed
  fresh from the already-persisted cursors each time, no new state needed beyond the `31` cursors
  above. This is exactly equivalent to picking a uniformly random *record* from the union of every
  length's currently-unread remainder: as a file's cursor advances toward its own wraparound, its
  share of the pool (and thus its pick probability) shrinks accordingly; the moment it wraps, its
  weight resets to the full `row_count_k`, refilling its share. So this approximates a true global
  shuffle-without-replacement over the whole dataset -- length-partitioning aside -- using only the
  already-existing sequential per-file cursors, no permutation, no extra bookkeeping.

  **Settled: no within-file shuffling.** Records inside a single length-file are read straight
  through sequentially, same cheap access pattern as today's single-file reader -- cross-length
  mixing already does most of the decorrelation work, and the residual within-file correlation
  (a handful of consecutive same-length records from one game) isn't worth paying for random
  access into an otherwise sequential, compressed stream.

  Transformer-only, per the drift note above: naive/HalfKP/HalfKAv2_hm keep training in their
  current single-file PGN order, entirely unaffected by any of this.
- **C. Settled: use the built-in `scaled_dot_product_attention` ATen op.** Rule 3 ("every line
  understood, not just compiling") is about this project's own chess-engine internals -- bitboard
  tricks, borrow patterns, chess logic -- not about re-deriving a well-established, generically
  documented ML primitive from scratch just to avoid a library call. Multi-head attention is
  standard ML literature, not something specific to Rotor; using the built-in op is in scope.
- **D. Revised: attention mode is a `net.yaml` field, not a CLI flag.** Superseded by the config-
  file decision above -- no `--attention` flag; `net.yaml` has a `full`/`kings-only` field instead,
  and since each named run directory (`data/betty/`, etc.) is already one specific attention mode's
  own config, the run directory itself is what separates the two checkpoint namespaces (per A: two
  separately trained models), not a CLI-level switch. Mask construction itself (combining the
  padding mask with the full/kings-only content mask into one tensor per batch item) is an
  implementation detail under this, not a further open question.
- **E. Settled (revised): its own separate transformer module, not nested under `features.rs`.**
  `features.rs`'s existing `halfkp`/`halfka_v2_hm`/`naive` submodules are all NNUE-style networks;
  the transformer is a different kind of thing entirely and gets its own top-level module rather
  than living inside the NNUE feature-set module -- still unit tested against known positions the
  same way those three already are, just not organizationally nested under them.
- **F. Settled: same `prepare`/`train` subcommand names.** `main.rs` branches internally on
  argument shape (a single config-file path vs. the existing flag list) rather than getting new,
  separate subcommand names for the transformer.
- **G. Settled: `net.yaml` schema drafted, single example at `examples/net.yaml`.** A
  two-subdirectory version (`data/example/full/net.yaml` + `data/example/kings-only/net.yaml`) was
  tried and dropped again in the same session: since every value except `attention` is identical
  between the two modes, one file with the `# full | kings-only` comment already documents the
  schema and both valid values, and two near-duplicate files would just be redundant -- those two
  directories are superseded, pending manual removal (no delete tool available). **Also moved out
  of `data/` entirely, later in the same session**: `data/` is gitignored wholesale (`.gitignore`),
  so `data/example/net.yaml` was never actually going to be checked in despite standing as the
  schema reference -- `examples/net.yaml` (a new top-level directory, tracked normally) is its
  real, committed home; `data/example/net.yaml` itself is now stale and also pending manual
  removal, alongside the two superseded subdirectories above. Three top-level
  groups matching the actual boundary between the two subcommands and the model itself:
  `preprocessing` -- originally everything `prepare` reads, now down to just `teacher` (renamed
  from a literal `database` path once cache-sharing-by-teacher was settled, and `pgn_file` moved
  out entirely into the teacher's own `teacher.yaml`, once `prepare` stopped taking a network's
  `net.yaml` at all -- see "Cache files are shared by teacher, not by run" and "`prepare` takes a
  teacher config" above). `net.yaml`'s own `preprocessing` section is read by `train` now (to know
  which teacher's cache directory to open), not by `prepare`. `architecture`
  (embedding dim, layer/head counts, feedforward dim, dropout, nested `value_head` with hidden
  dim/depth), `training` (everything `train` reads: `attention` [`full`/`kings-only`, moved here
  from top-level in a later edit], `learning_rate`, `batch_size`). **Revised: parsed via `serde`
  + `serde_yaml` (both added to `Cargo.toml`, gated behind the `training` feature like `tch`/
  `zstd`), not a hand-rolled parser.** Unlike FEN (this project's own domain-specific format,
  CLAUDE.md rule 1's exact reason to hand-write that one), YAML is a generic, well-established
  format with a mature library already available -- no reason to reimplement it, so this doesn't
  fall under this project's hand-rolled-parser convention at all. **Revised again: `lambda`
  dropped from `preprocessing` entirely, hardcoded instead** (`main.rs`'s own `DEFAULT_LAMBDA`,
  shared with the NNUE track's own `--lambda` default) -- per the developer, not exposed in
  `net.yaml` for now.

  **Chosen values** (not placeholders anymore, sized for one consumer GPU): `embedding_dim: 256`, `num_layers: 6`, `num_heads: 8`, `feedforward_dim: 1024`,
  `dropout: 0.1`, value head `hidden_dim: 256`/`depth: 2`, `learning_rate: 0.0001` (lowered from an
  initial `0.001`, which is on the high side for AdamW training a transformer from scratch with no
  warmup schedule -- flat `1e-4` for now, to be swept over later once tuning starts rather than
  solved with scheduling machinery today), `batch_size: 1024`. Same values regardless of attention mode -- at this sequence length (max
  `33` tokens), attention cost itself (`992` pairs full vs. `62` kings-only, at the 32-piece max)
  is dwarfed by the FFN/projection cost either way, so there's no real headroom argument for
  scaling one differently from the other; identical architecture and batch size also make the
  full-vs-kings-only comparison a fair one later, with `attention` as the only varying factor.
- **H. Settled: no `Transformer` variant on `FeatureSet`/`Network`/`DenseBatch` -- standalone types
  instead.** Revisits the Grounding section's original assumption once B/D/F above had all
  settled: those three enums' entire shape (CLI-flag parsing, `window::run_window`'s single-cursor
  loop, fixed-width dense tensors) serves machinery the transformer bypasses completely, so a
  fourth variant on any of them would be dead surface area, not real reuse. The transformer gets
  its own `TransformerNetwork` struct (step 4) and its own batch struct (step 3); `main.rs`
  dispatches to separate `run_prepare_transformer`/`run_train_transformer` functions based on
  argument shape (a lone `net.yaml` path vs. the existing flag list), never constructing a
  `FeatureSet`/`Network`/`DenseBatch` value for the transformer path at all.

## Progress

Plan settled (all open design points A-H resolved), `examples/net.yaml` written as the schema
reference (moved out of the gitignored `data/` directory later in the session -- see point G).
Step 1 (vocabulary & token-sequence module) is complete. Step 2 (CLI dispatch, standalone types)
underway:

- `transformer_config.rs`: `NetConfig`/`Preprocessing`/`Architecture`/`ValueHead`/`Training`/
  `Attention`, deriving `serde::Deserialize` (added `serde`+`serde_yaml` to `Cargo.toml`, gated
  behind the `training` feature) and a `load(path) -> NetConfig` function -- done, tested against
  `examples/net.yaml` directly.
- `main.rs`: dispatch on argument shape -- `prepare`/`train` given exactly one argument route to
  new `run_prepare_transformer`/`run_train_transformer` (which load the config and `todo!()` past
  that point, left for steps 3-5); anything else still goes through the existing `run_prepare`/
  `run_train`, whose signatures changed from `env::Args` to `impl Iterator<Item = String>` (a pure
  signature change, no behavior change, needed since `main` now has to inspect argument count
  before deciding which track to hand them to) -- done.
- Still to do in step 2: the transformer's own standalone `TransformerNetwork` struct (step 4)
  hasn't been written yet. `run_train_transformer` is still a stub past the config-load point.

Step 3 (batch construction) underway, starting with the storage half (design point B's `31`
length-bucketed cache files) before the batch-reading half:

- `length_cache.rs` (new module): `MIN_PIECE_COUNT`/`MAX_PIECE_COUNT`/`PIECE_COUNT_BUCKETS`
  (`2..=32`, `31`), `write_row_count`/`read_row_count` (the magic-checked `.count` sidecar format,
  see design point B's latest update above), `LengthCacheWriter` (owns all `31` open `.cache`
  encoders for one split, `write` routes by piece count, `finish` closes every stream and writes
  its sidecar), and `build_length_caches` (the orchestration function -- one real PGN-replay pass,
  reusing `builder::is_train_game` [bumped to `pub(crate)`], `dataset`, `labels::compute_labels`,
  `pgn_replay`, and `features::extract_canonical` exactly as `builder::build_caches` already does,
  just routing into two `LengthCacheWriter`s instead of two fixed `CacheHeader`-led files) -- done,
  tested (routing correctness, untouched-bucket sidecars, out-of-range rejection, and an end-to-end
  two-tiny-games round trip mirroring `builder.rs`'s own fixture).
- `main.rs`: `run_prepare_transformer` now calls `length_cache::build_length_caches` for real
  (opens the database, reads `game_summaries`, derives the run directory from `net.yaml`'s own
  path via the new `run_dir_of` helper) -- no longer a stub. `run_train_transformer` still is.
- `length_cache.rs`: `LengthCacheReader` (the read-side counterpart to `LengthCacheWriter`, same
  sequential-decode/`seek_to` shape as `reader::CacheFile`, `row_count` from the `.count` sidecar
  rather than an in-stream header) and `read_batch_stop_short` (the no-padding detour's own new
  reader function -- stops short at `row_count` instead of wrapping mid-batch like `reader::
  CacheFile::read_batch_cyclic` does, returning a short batch and a `next_cursor` of `0` whenever
  the pass runs out, including the exact-boundary case) -- done, tested (full batch, short batch,
  exact-boundary wrap, a bucket permanently smaller than one batch, cross-call continuation without
  reopening, resume at a nonzero cursor, out-of-range cursor rejection).
- `length_cache.rs`: `pick_length_index` -- the weighted-random length pick itself, `weight[i] =
  row_counts[i] - cursors[i]` recomputed fresh every call (no extra persisted state beyond the
  cursors), via `rand`'s own `WeightedIndex` (`rand::distr::weighted`, `rand = "0.10.2"`'s renamed
  `distributions` module) -- done, tested (single-nonzero-weight determinism, an exhausted bucket
  never picked, an all-empty panic, and the cursor-invariant guard).
- `length_cache.rs`: `TransformerWindowProgress` -- same shape and save/load discipline as
  `window_progress::WindowProgress`, but `train_cursors: [u64; PIECE_COUNT_BUCKETS]` instead of a
  single `cursor` (a distinct type, not an extension of the NNUE-only original -- design point H);
  the held-out test pass still needs no persisted cursor of its own, matching the existing
  convention (one full ordered sweep per window, never resumed mid-pass) -- done, tested (full
  round trip with distinct per-index cursor values, bad-magic rejection). **Revised during step 5:
  added a `rows_done: u64` field.** `window_progress::WindowProgress` never needed one because
  `read_batch_cyclic` always returns exactly `batch_size` rows (`steps_done * batch_size` is
  always the right denominator for `loss_sum`); `read_batch_stop_short` can return a short batch,
  so the true row count has to be tracked and persisted explicitly instead.
- `transformer_vocab.rs`: `is_king_token(vocabulary_index)` -- whether a vocabulary index names a
  King token (either side), needed by the mask-builder below and kept here since only this
  module's own layout (`KING_OFFSET..QUEEN_OFFSET`) can answer it -- done, tested.
- `transformer_batch.rs` (new module): `TransformerBatch` (`tokens: [batch, seq_len] Int64`,
  `attention_mask: Option<[batch, 1, seq_len, seq_len] Bool>`, `label: [batch, 1] f32` -- the
  transformer's own standalone batch type, design point H, no `DenseBatch` variant) and
  `to_transformer_batch(records, attention)`. No positional-index tensor: sequence position is
  just `0..seq_len`, trivially `Tensor::arange`-able from `seq_len` alone at forward-pass time
  (step 4), not something worth building or storing here. **Found during implementation, not a
  pre-existing open question: the kings-only mask can't be one `[seq_len, seq_len]` matrix shared
  across a batch.** Buckets are keyed by *total* piece count, but individual records split that
  total between own/opponent differently (`12` own `+ 8` opponent and `6` own `+ 14` opponent both
  total `20`) -- the own king is always at sequence position `1`, but the opponent king's position
  (`1 +` own side's own token count) varies per record even within one bucket. So
  `build_kings_only_mask` computes the `is_king(i) || is_king(j)` rule genuinely per record, not
  once per batch. Full attention needs no mask tensor at all (`None`, not an all-`true` tensor) --
  done, tested (tensor shapes for both attention modes, a length-mismatch guard, label pass-
  through, and the mask's actual boolean values checked against the starting position by hand).

**Step 3 complete.**

Step 4 (model, new module `transformer_network.rs`) complete:

- `Embeddings`: `nn::embedding` token lookup (`VOCAB_SIZE` rows) plus a learned positional
  embedding -- a plain sliced parameter tensor, not a second `nn::Embedding`, since every real
  sequence's positions are always the contiguous prefix `0..seq_len` (never offset partway into
  the table) -- summed, with embedding dropout applied once at the end.
- `SelfAttention`: multi-head self-attention via the built-in `Tensor::scaled_dot_product_
  attention` (design point C), following `vision::dinov2::Attention`'s own qkv-projection-then-
  split shape adapted from 4D (image patches) to 3D (token sequences); `attention_mask` passed
  straight through from `TransformerBatch`. Two dropouts, both BERT-standard: on the attention
  probabilities themselves (`scaled_dot_product_attention`'s own `dropout_p`, manually zeroed when
  not training since that op has no eval-mode awareness of its own) and on the output projection.
- `FeedForward`: the standard two-linear-layers-with-GELU-between sublayer, dropout after `fc2`.
- `TransformerBlock`: self-attention then feed-forward, each wrapped in its own residual connection
  and followed by `LayerNorm` -- classic *post*-norm BERT (`LayerNorm(x + Sublayer(x))`), not the
  pre-norm ordering newer architectures (and `vision::dinov2::Block`) use instead, since
  `rotor-transformer.md` calls for "standard BERT-like encoding" specifically.
- `ValueHead`: an MLP (configurable `hidden_dim`/`depth` from `net.yaml`) applied to `[CLS]`'s own
  final hidden state, ending in `sigmoid` -- a real expected score in `[0, 1]`, matching every
  other network in this project.
- `TransformerNetwork`: ties the above together (`Embeddings` -> `num_layers` `TransformerBlock`s
  -> `[CLS]`'s own row (`Tensor::select`) -> `ValueHead`), reading its shape entirely from
  `net.yaml`'s `Architecture`. Shared unchanged between full and kings-only attention (design point
  A) -- only which mask `TransformerBatch` carries ever differs.
- **Dropout retrofit, mid-step:** `net.yaml`'s `architecture.dropout` was sitting unused until a
  deliberate retrofit threaded a `(dropout: f64, train: bool)` pair through every `forward` already
  built (`Embeddings`/`SelfAttention`/`FeedForward`/`TransformerBlock`), applying it at the three
  standard BERT points named above; `TransformerNetwork::forward` itself only takes `train`, and
  reads its own stored `dropout` (captured from `Architecture` at construction). All pre-existing
  tests updated to pass `(0.0, false)` (exactly inert, no behavior change); new tests confirm
  eval-mode is a genuine no-op and train-mode actually randomizes across repeated calls.
- Tested throughout with shape checks, mask-changes-output / dropout-changes-output behavioral
  checks (matching `network.rs`'s own existing testing style), and an end-to-end
  `TransformerNetwork::forward` check (both attention modes) producing a valid `[batch, 1]`
  probability in `[0, 1]`.

**Step 4 complete.**

Step 5 (training loop integration, new module `transformer_window.rs` plus wiring
`run_train_transformer` in `main.rs`) complete:

- `transformer_window.rs`: `run_training_steps` -- the training half of one window, mirroring
  `window.rs::run_training_steps`'s own resume/checkpoint/stop contracts exactly, but picking a
  length bucket via `length_cache::pick_length_index` each step instead of reading one cyclic
  cursor, and accumulating a genuinely row-weighted `train_loss` (`loss_sum / rows_done`, using
  the `rows_done` field added to `TransformerWindowProgress` above) since `read_batch_stop_short`
  can return a short batch. `rng: &mut impl RngExt` is a parameter (a real `rand::rng()` in
  production, a seeded `StdRng` in tests) -- needs no resume bookkeeping of its own, since
  `pick_length_index`'s weights are recomputed fresh from the persisted cursors every call
  regardless of the RNG's own history.
- `transformer_window.rs`: `run_test_pass` -- the held-out half, mirroring `window.rs::
  run_test_pass`: a full, deterministic, no-grad sweep over every one of the (up to) 31 test-split
  length files, each read from row `0` through exactly one wraparound of `read_batch_stop_short`
  (which is precisely one full pass of that bucket), no cursor or resume support, `on_progress`
  reporting against the grand total across every bucket combined.
- `transformer_window.rs`: `run_window` -- composes the two above, same shape as `window::
  run_window` (returns `Some((train_loss, test_loss, next_cursors))`, or `None` and skips the test
  pass entirely if `stop_requested` fired mid-training).
- `length_cache.rs`: `open_readers` (opens all `PIECE_COUNT_BUCKETS` length files for one split at
  once, the read-side counterpart to `LengthCacheWriter::create`) and `row_counts_of` (gathers
  every reader's own `row_count` into the fixed-size array `pick_length_index` takes) -- done,
  tested.
- All three `transformer_window.rs` functions tested: finite-loss/cursor-advance shape checks,
  loss actually falling over repeated calls on a fixed target, `stop_requested` cutting a pass
  short and checkpointing the stopped point, full coverage across multiple buckets, no-mutation-
  during-eval, and (for `run_window`) that a mid-training stop skips the test pass outright.
- `main.rs`: `run_train_transformer` no longer a stub -- opens the 31 train and 31 test
  `LengthCacheReader`s, builds `TransformerNetwork`/`AdamW` from `net.yaml`'s `Architecture`/
  `learning_rate`, loads or starts fresh exactly like `run_train` does (`model.vs`/`optimizer.opt`/
  `early_stopping.state`), installs the same `Ctrl+C` handler, and loops `transformer_window::
  run_window` windows using `TransformerWindowProgress` in place of `WindowProgress` -- same
  archiving (`checkpoints/step_NNNNNNNNN.vs`, `history.tsv`), same `STEPS_PER_EVAL`/
  `CHECKPOINT_INTERVAL`/`PATIENCE` constants reused as-is (no transformer-specific copies). No
  `FeatureSet` marker check needed here (unlike `run_train`): `net.yaml`'s own `training.attention`
  is fixed per run directory by construction (design point A), so there is nothing that could
  mismatch the way `--features` could against an existing checkpoint directory. Builds cleanly;
  no dedicated unit test of its own, same as `run_train` itself.

**Step 5 complete.** Step 6 (run the convergence check against real cached data) is next.

Step 6 underway: running the pipeline for real, on a small (771-game, 80,218-ply) smoke-test
teacher (`lumbra-zero`), surfaced several real design and correctness issues, none of them
hypothetical:

- **Progress reporting fix.** The train pass's `on_progress` callback originally reported rows
  processed against a fabricated total (`steps * batch_size`), unrelated to the real dataset size
  or to the run's own actual, early-stopping-governed length. True of both `window.rs` (NNUE) and
  `transformer_window.rs`, not something specific to the transformer track: a pre-existing NNUE
  characteristic, mirrored faithfully into the new module without question until the developer
  caught it against real output. Fixed in both: the train pass now reports a plain step count
  (`steps_done` against `steps`, a real, fixed quantity known in advance), and `main.rs`'s
  `print_progress` shows a bare running step count for `"train"`, with no bar, no percent, and no
  ETA. The `"test"` pass keeps its own percentage bar unchanged, since its total (the real
  held-out row count) is genuinely meaningful.
- **Early-stopping resume bug.** Both `run_train` and `run_train_transformer` loaded `vs`/
  `optimizer`/`early_stopping` all under one `model_path.exists()` branch, but the periodic
  mid-window checkpoint only ever saves `model.vs`/`optimizer.opt`/`window_progress.state`, never
  `early_stopping.state` (nothing new to record for it until a whole window first completes and
  `observe()` runs). A crash or interruption during a network's very first, still-incomplete
  window, after at least one periodic checkpoint but before that window's own completion, left
  `model.vs` behind with no `early_stopping.state` alongside it: a real, resumable state, not a
  corrupt one. Caused a genuine panic on the developer's first attempt at `train amanda`. Fixed in
  both functions: `early_stopping` loading is now gated independently on
  `early_stopping_path.exists()`, not on `model_path.exists()`.
- **`steps_per_eval` moved into `net.yaml`, per network.** Originally the shared NNUE constant
  `STEPS_PER_EVAL = 8192`, reused unchanged for the transformer track. On a CPU-only
  machine, one window at that size (times `batch_size = 1024`) means training over
  8 million rows, more than 100 passes over the current 77K-row smoke-test dataset, before the
  first test-set evaluation, checkpoint archive, or early-stopping decision ever happens. Settled:
  `training.steps_per_eval: u64`, a required `net.yaml` field, read per network rather than
  shared. `amanda`/`betty` both set to `512` for now (a placeholder for this small dataset, freely
  adjustable); `examples/net.yaml` and its own parsing test updated to match. The NNUE track's own
  `STEPS_PER_EVAL` constant is untouched.
- **Job-scoped stopping controls: `--patience`/`--min-steps` as CLI flags, not `net.yaml`
  fields.** Discussed at length with the developer: patience (how many evaluation windows without
  improvement before stopping) is a property of the current running job, not of the network's own
  shape or training recipe the way `architecture`/`learning_rate`/`batch_size`/`steps_per_eval`
  are, so it doesn't belong in `net.yaml` alongside them. A third candidate, `--max-steps` (a hard
  step ceiling regardless of patience), was considered and dropped: its only real use is bounding
  an unattended run, and every run so far has been interactive, always stoppable by Ctrl+C with no
  loss (the existing resumable checkpoint/window-progress state already covers that case
  completely), so it would be built for a mode of use that doesn't exist yet, against this
  project's own "don't design for hypothetical future requirements" convention.

  Settled: `train <network-name> [--patience <n>] [--min-steps <n>]`, defaulting to `10`
  (matching the NNUE track's own long-standing patience) and `0` (no floor, matching today's
  unconstrained behavior) respectively, when omitted. `--min-steps` only suppresses the
  *automatic* stop-because-patience-exhausted decision until the job's own cumulative step count
  (`optimizer.step_count()`) reaches it; it never blocks a manual Ctrl+C, and patience bookkeeping
  (`best_loss`/`steps_since_improvement`) keeps accumulating underneath it regardless.
  `--patience` replaces the shared `PATIENCE` constant at the two call sites
  `run_train_transformer` already has (`EarlyStopping::new`/`EarlyStopping::load`); no change
  needed inside `early_stopping.rs` itself, since defaulting to the same value (`10`) on every
  invocation of an existing job, fresh or resumed, means its own existing mismatch assertion only
  ever fires when the developer deliberately passes a different value than the job was started
  with, exactly its intended purpose. Scoped to the transformer track's own bare-name `train`
  dispatch only, for now; the NNUE track's `run_train` keeps its hardcoded `PATIENCE` untouched,
  since only the transformer track is actually being run today. **Implemented**: `parse_job_flags`
  (`main.rs`) parses the two flags for the transformer's own bare-name `train` dispatch;
  `run_train_transformer` threads `patience`/`min_steps` through in place of the old hardcoded
  constant.
- **The window-relative step counter went stale the moment `steps_per_eval` became a `net.yaml`
  field, and was replaced with the real, always-consistent `optimizer.step_count() %
  steps_per_eval`.** Found via the developer's own first resumed run after lowering `amanda`'s
  `steps_per_eval` from `8192` to `512`: the leftover, separately-persisted "steps into this
  window" counter (`589`, accumulated under the old, larger size) was already past the new
  boundary, so the resumed window "completed" for free with zero further training. Fixed by
  dropping that counter entirely -- `transformer_window::run_training_steps` (and, for
  consistency, its NNUE counterpart, unaffected by the underlying bug since `STEPS_PER_EVAL` never
  changes there) now checks the real, persisted `optimizer.step_count()` directly against
  `steps_per_eval`'s current value on every batch, which can never desynchronize regardless of
  what the config said on an earlier run. Also found and fixed in the same pass: `duration_secs`
  (both tracks) only ever measured the current process's own elapsed time, silently undercounting
  a round resumed after a `Ctrl+C` -- the transformer track now accumulates it across resumes the
  same way `rows_done`/`loss_sum` already did; the NNUE track times its own two phases separately
  but does not (yet) accumulate across resumes, since `STEPS_PER_EVAL` never changes there and the
  gap has no evidence of mattering. `history.tsv`'s single `duration_secs` column split into
  `train_duration_secs`/`test_duration_secs` (both tracks), since the two phases' very different
  row counts make a combined figure useless for computing either one's own throughput.
- **Renamed throughout, per the developer: "window" to "round", and the periodic mid-round save
  ("checkpoint") to "backup".** "Window" collided with this codebase's own domain (attention
  window, context window -- a real, different concept in a Transformer specifically) and was
  never standard ML terminology for "steps between evaluations" in the first place (industry
  practice mostly just says "every `eval_steps` steps" and leaves the span itself unnamed).
  "Checkpoint" was already overloaded three ways in this codebase (the periodic mid-round save;
  the single resumable `model.vs`/`optimizer.opt`; the archived, immutable `checkpoints/step_N.vs`
  history) -- renamed only the first of those three to "backup" (`on_backup`, `BACKUP_INTERVAL`,
  "periodic backup saved"), since it's the one that never actually touches the `checkpoints/`
  directory at all; that directory keeps its name, since "checkpoint" is already standard
  terminology for an archived weights snapshot (matching Hugging Face's own `Trainer`, PyTorch
  Lightning, etc.) and renaming it away from that would trade a correctly-named thing for an
  incorrectly-named one to fix a collision that wasn't actually there.

  Mechanically: `window.rs`/`transformer_window.rs`/`window_progress.rs` became
  `round.rs`/`transformer_round.rs`/`round_progress.rs` (new files written with the renamed
  content; the three old files are now orphaned, unreferenced by any `mod` declaration, and are
  pending manual deletion -- no delete tool available). `run_window`/`WindowProgress`/
  `TransformerWindowProgress` became `run_round`/`RoundProgress`/`TransformerRoundProgress`;
  `window_progress.state` became `round_progress.state` (existing files under the old name are
  simply never looked at again under the new path, no migration needed); `TransformerRoundProgress`
  additionally got a fresh magic (`RTTR`, was `RTTW`) and format version reset to `1`, purely for
  cleanliness (nothing depends on reading an old file under the new path regardless). A stale
  `window_progress.state`/`history.tsv` (old header) from before this rename needs deleting by the
  developer before the next real run.

Step 1 (vocabulary & token-sequence module, `src/bin/rotor_train/transformer_vocab.rs`):

- `VOCAB_SIZE` (`569`) and the per-piece-type size constants -- done, tested.
- `file_major_index` (re-indexes `Square`'s native rank-major LERF numbering into the file-major
  order `a1 < a2 < ... < b1 < ...` our token order actually needs) and `king_token_index` (King's
  `256`-token block: square outer axis, castling-rights suffix inner axis, kingside-then-queenside
  bit order per FEN's `KQ` convention) -- done, tested.
- `oriented_square` bumped to `pub(crate)` in `features.rs` so this module can reuse it directly
  for the own-forward-is-increasing-rank relative-square conversion, instead of reimplementing it.
  Confirmed in the same session: none of `halfka_v2_hm`'s horizontal-mirror ("hm") logic applies
  here -- that exists purely to shrink an NNUE accumulator table's king-bucket width, which this
  vocabulary has no equivalent of.
- Queen/Rook/Bishop/Knight token indices (plain square, no suffix) -- done, tested.
- Pawn's index (the `48 + 8` grammar, en passant star legal only on relative rank 5 -- a pawn can
  only ever be captured en passant the move immediately after its own double step, which always
  lands it on rank 5) -- caught mid-implementation that the code and this doc both had it wrong as
  `48 + 16` across ranks 4-5; corrected in both places (`VOCAB_SIZE` `577` -> `569`).
- `side_block` (one color's full token list, `K < Q < R < B < N < P` order; the en passant star is
  set only on the opponent's own pawn matching `en_passant_file`, never on the side to move's own
  pawns) and `token_sequence` (`[CLS]` + own side's block + opponent's, the module's public entry
  point) -- done, tested.

**Step 1 complete.** `transformer_vocab.rs` goes straight from `CanonicalPosition` to `Vec<usize>`
vocabulary indices, no string/tokenizer step anywhere (confirmed against the settled "no FEN, no
tokenizer" design point). Step 2 (enum plumbing: `FeatureSet::Transformer`, `Network::Transformer`,
`DenseBatch::Transformer`) is next.
