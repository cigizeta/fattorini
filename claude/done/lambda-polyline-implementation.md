# Lambda by progress: implementation plan

**Status: step 1 written (2026-11-04): `polyline.rs` (measure, clamp, `Polyline::at`, `ensure`,
`polyline.txt`); `scores_pawns()` no longer test-only; `lambda_experiment` unhooked, its file to be
deleted by the developer. Two temporary `#[allow(dead_code)]` (on `mod polyline` and on
`scores_pawns`), dead code being denied: step 5 removes them. Step 2 begun: `ArmLambda` in
`preprocessing_config` (`Constant`, `Polyline`, `parse`, `Display`); `lambda_text`, `lambdas_text`,
`lambda_dir_name`, `label_set_dir_name`, `generation::lambda_dir` take it (the developer chose the
type over a reserved value). Step 2 done (2026-11-04, tests and release build passing): players,
lockstep, candidates take `ArmLambda`; `freeze` wraps constants until step 4; smithy's `Dirs::arm`
takes it while `Dirs::lambda` keeps the grid's `f64`; the page lists constant winners only until
step 6. Step 3 done (2026-11-04, tests and release build passing): `CacheRecord.progress`
(`cache::Progress`, i and n as `u16`, the developer chose the field over extra label columns),
filled by `pgn_stream`; bucket records end with i and n; `buckets.tsv`'s labels line ends with
`progress`; `buckets::Labelling` (arms and an optional polyline) opens the stream, smithy passing no
polyline until step 5. Step 4 done (2026-11-04, tests and release build passing): `freeze` takes a
`Labelling`; `lambda-polyline-direct` holds each position's blend at its own lambda, header lambda
-1, and `polyline.txt` (the ten values to 6 decimals), a mismatch refused until the set is deleted.
Step 5 done (2026-11-04, tests and release build passing): `lambda: grid|polyline` in `GenConfig`, `grid_center` optional and
refused under the polyline, a transformer refused under it; the polyline measured on
`polyline::sample` (every k-th training file by path, about 10M positions, Claude's choice: a
whole corpus at a few thousand positions a second takes days, and a scaled share must give the
source's polyline, shared with its frozen test set), kept at `<source root>/polyline.txt` on the
remote or `gen-<g>/polyline.txt`; one `ArmLambda::Polyline` arm, straight to the gate; the
`dead_code` allow on `mod polyline` removed. Step 6 done (2026-11-04, tests and release build
passing): `ArmLambda` serialized as a number or `"polyline"`; the page's training rows, network
list (a polyline winner included) and arm directories carry it; a polyline run's Training box shows
the ten values (`polyline_path`, shared with training). Step 7: the tests were written with each
step. Verified (2026-11-04) by a scaled 1/1024 run of `pesto-psq-threats-screlu`: generation 1 on
PeSTO's partial corpus (a scaled run may start on an incomplete source, its polyline and test set
then kept in its own generation), polyline 0.111 to 0.972, gate failed and promoted as scaled;
generation 2 on its own self-play, polyline 0.071 to 0.508, gate passed 65.6%. Done.** Implements lambda-polyline.md. On `fabio`, beside the
training corrections (the developer): one duel judges both, and the polyline is behind a config
switch, so it needs no separate revert. The grid stays, selectable per config.

## What the code does today (mapped 2026-11-04)

- **Buckets** store per position the label at lambda 0 (`f32`) and at lambda 1 (`i8`)
  (`buckets.rs` `write_record`), and blend per lambda at batch time in `read_records`,
  `(1 - lambda) * at_zero + lambda * at_one`. No magic in the bucket files: `buckets.tsv`'s `keep`
  and `labels` lines force a rescatter on a mismatch.
- **i and n** are known in `pgn_stream`'s per-game loop: i the ply index among the searched
  positions, n `scores_pawns.len()` (private; its accessor is test-only).
- **Frozen test sets** key each label set by an `f64` lambda: the directory `lambda-<l>-direct`, the
  `LabelHeader`, the check in `read`.
- **Arms** are keyed by an `f64` lambda everywhere: `lambda-<l>` directories, `run_config.txt`,
  `candidate.txt`, `generations.tsv`'s winner (read by `players::run_winner`, `run_winners`), the
  page's `LambdaView`. One learned arm already skips the round robin and goes to the gate.
- **The transformer** labels per lambda in `pgn_stream`, not from buckets; no transformer is trained
  now.

## Decisions taken for the plan (Claude, to confirm)

- **An arm's lambda becomes a small type,** `ArmLambda::Constant(f64)` or `ArmLambda::Polyline`, with
  a text form, `0.05` or `polyline`, used wherever the `f64` was written or read: directories
  (`lambda-polyline`), `run_config.txt`, `candidate.txt`, `generations.tsv`, the page. Constant
  arms keep exactly today's text, so every existing run reads as before.
- **The polyline is NNUE only:** a config choosing it with a transformer architecture is refused.
- **The config key,** in `GenConfig`: `lambda: grid` (the default, today's behaviour) or
  `lambda: polyline`. A polyline run's every generation trains one polyline arm; `grid_center` is
  then refused as meaningless.

## Steps

1. **The measurement,** `polyline.rs` (smithy), the experiment's pass made production code: the
   ten bucket lambda* of a corpus's training games, s through the teacher's curve
   (`Source::curve`, `LOGISTIC` for self-play), mates and tablebase scores left out, each clamped
   to [0, 1]. `Polyline::at(i, n)`: linear between bucket centres, flat beyond the end centres.
   `ensure(path, files, curve)`: read `polyline.txt` if there, else measure and write it (the ten
   values and the positions measured). Kept beside a source's corpus on the remote, in a self-play
   generation's directory. `lambda_experiment.rs` deleted. Tests: the bucket arithmetic in
   integers, the clamp, the polyline at centres, between them and beyond the ends, the inverse-
   variance fixture.
2. **`ArmLambda`** (rotor `pipeline`, beside `lambda_text`), its text form and parse, and every
   reader and writer of an arm's lambda moved to it; `run_winner` and `run_winners` parse it.
   Tests: round trips, every existing text form still read as a constant.
3. **Progress in the buckets:** `write_record` appends i and n as two `u16` after the labels;
   `read_records` reads them and blends a polyline arm at `Polyline::at(i, n)`, a constant arm as
   today; `BucketStream::open` takes `&[ArmLambda]` and the polyline. `buckets.tsv`'s `labels` line
   gains `progress`, so every existing cache is scattered again once. `CollectedSelfPlayGame` gets
   a production accessor for n. The tablebase override unchanged (it sets both ends). Tests: an
   epoch at the polyline equals the hand blend at each position's own lambda; constant arms equal
   today's.
4. **The frozen test set:** a polyline label set, `lambda-polyline-direct`, labelled in `sample` at
   `Polyline::at(i, n)` (i and n known in its loop); its manifest records the ten values, so a
   re-measured polyline is refused until the set is deleted. The label sets' key becomes
   `ArmLambda`; `LabelHeader` stores a constant's lambda as today and a reserved value for the
   polyline. Tests: freezing a polyline set beside constant ones, reading it back, a mismatched
   polyline refused.
5. **The generation loop:** with `lambda: polyline`, `train` measures (`polyline::ensure`) after
   choosing the corpus and its curve, trains one arm `ArmLambda::Polyline`; one learned arm already
   goes straight to the gate; `generations.tsv` records `polyline`; promotion copies
   `lambda-polyline`'s candidate. `next_grid` not called. With `lambda: grid`, nothing changes.
6. **The page:** a polyline arm shown as `polyline`, its ten values beside the run.
7. **Tests** written with each step; the existing grid tests unchanged and passing.

## The experiment, once built

**First, a scaled generational run on PeSTO (the developer, 2026-11-04):** `pesto-psq-threats-screlu`
with `lambda: polyline`, scaled, over a few generations, to check the pipeline end to end; Lc0 waits
for its corpus rewrite to finish.

A new `lc0-psq-threats-screlu` run with `lambda: polyline`, after the Lc0 rewrite and with its
frozen test set deleted; its generation 1's network duelled against emily's generation 1.

**It replaces the training-corrections reality check (the developer, 2026-11-04):** no grid run on
Lc0 under the new curve first; the curve and the polyline are judged together by this one duel.

## Open

Nothing. The three decisions above, Claude's, stand unless the developer says otherwise.
