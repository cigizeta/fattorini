# No attempts: one self-play set a generation, 1B positions

Status: done 2026-11-02. Code, configs and DEV.md written, tests passing, committed; the migration
below run on Drive and here, and checked by reading the files. No checks from the page: the
software is tested as it is used (the developer). Design and implementation plan in one doc, the change being mostly
removal.

## Why

A failed gate today plays another attempt: the same teacher, `training_positions` more games, the
generation trained again on all of them, up to `max_failed_gates`. `emily`'s generation 2 (TODO.md)
passed at 402.7M positions, against expectations; the 1B test was planned "with no attempt logic, a
failed gate ends it". The developer, 2026-11-02: no attempts at all, every self-play generation 1B
positions.

## Decided (the developer, 2026-11-02)

1. **1,073,741,824 positions a self-play generation** (1024 x 1024 x 1024), `training_positions` in
   every forge config that has one. A source's generation 1 trains on its corpus, as today.
2. **A failed gate ends the run,** self-play as a source's generation 1 already does. The lineage
   stops; a new run starts over. `max_failed_gates` leaves every config, and the config reader
   refuses it as it refuses any unknown key (so a stale config says so instead of being ignored).
   A state or `generations.tsv` from before is refused too, saying to migrate the run.
3. **The attempt leaves the layout and the names; old runs migrated by hand.**
   - Layout: `gen-<g>/lambda-<l>/` (was `gen-<g>/attempt-<a>/lambda-<l>/`); `round-robin.tsv` and
     `gate.tsv` in `gen-<g>/`.
   - Candidate names: `<config>-g<g>-l<lambda>-r<n>` (was `-g<g>-a<a>-l<lambda>-`). A name with
     `-a<a>-` is refused as any other misspelling, the message naming the new form.
   - `generations.tsv`: `generation winner gate_games gate_share passed`, no `attempt` column.
   - `state.txt`: no `attempt` and no `failed_gates` line. A stopped lineage needs its own mark:
     `phase stopped` (a new `Phase::Stopped`), in place of `failed_gates >= max_failed_gates`.
   - The code reads only the new layout: no fallback to `attempt-1`.

## The code (sites found 2026-11-02)

- **`smithy.rs`:**
  - `ForgeConfig`: `max_failed_gates` gone, with its validation and accessor.
  - `GenState`: `attempt` and `failed_gates` gone; `Phase::Stopped` added, read and written by
    name. Every `GenState { .. }` literal, the tests' included.
  - `Dirs`: `attempt()` gone; `lambda`, `round_robin`, `gate` take the generation alone.
  - The forge's loop (the gate's end, about line 4745 to 4897): a pass promotes as today; a fail, a
    generation that learned nothing, or a source's generation 1 failing writes `Phase::Stopped` and
    ends with one message ("generation <g> failed its gate: the run stops here"); the scaled run's
    promotion despite a fail stays. The early return for a stopped lineage reads the phase.
  - The training set: `training_positions`, not `attempt x training_positions` (the training target,
    the page's `train_wanted`, the log line).
  - `attempts_of`, `attempt_lambdas`, `attempt_view`: gone or by generation. The view's URL becomes
    `/view/<run>/<generation>`. `NetworkEntry`, `GenerationEntry`, the run's status lose `attempt`
    and `attempts`. The "lineage stopped" test (`state.failed_gates >= config.max_failed_gates()`,
    two sites) reads `Phase::Stopped`.
  - `learned_lambdas`, `train`'s cache name (`gen-<g>/attempt-<a>`) and the files list for the
    remote copy, by generation.
- **`page.html`:** the attempt selector gone; "generation <g>, <phase>" in every status line; a
  network's label "winner (l<lambda>)" or "lambda <lambda>"; the stopped phase named.
- **`players.rs`:** `network_player_name` and `parse_network_name` with `Option<f64>` for the
  candidate's lambda; `resolve_network_file_under` reads `gen-<g>/lambda-<l>/`; `run_winner`
  returns the lambda from the five-column `generations.tsv`. Tests rewritten to the new forms.
- **`generation.rs`, `candidates.rs`, `buckets.rs`, `tree.rs`, `archive.rs`:** doc comments and test
  paths to the new layout; `buckets.rs`' "attempts add only their new files" goes.
- **Configs:** `training_positions: 1073741824` in the self-play forges (both `lc0-` and `pesto-`),
  `max_failed_gates` removed everywhere, the comments naming attempts reworded. `configs/pesto.yaml`
  is a source's, not a forge's: left alone.

## The migration (the developer, by hand, after the code)

Every run directory under `data/networks/` (here and on Drive) with `gen-<g>/attempt-1/`: its
contents moved up into `gen-<g>/`, `generations.tsv`'s `attempt` column deleted, `state.txt`'s
`attempt` and `failed_gates` lines deleted (a run stopped by failed gates gets `phase stopped`),
and `config.yaml`'s `max_failed_gates` line deleted (the run's own config is read strictly, so it
would be refused). A run with `attempt-2` or later is not migrated: its later attempts have no place
in the new layout.

**Found 2026-11-02 on Drive:** nine runs, every one at attempt 1 with no failed gate, so no
`phase stopped` is needed: `emily`, `lc0-psq`, `lc0-psq-threats`, `lc0-psq-threats-pairwise`,
`lc0-psq-threats-pairwise-hidden`, `lc0-psq-threats-screlu-material`, `-phase`, `-count`,
`-taper` (each `run-1`). Here: `emily/run-1`'s top files and its `gen-3` (no attempt directory).
Two smoke runs, `smoke` and `gen-psq-smoke`, have an `attempt-2`: deleted, not migrated (the
developer to confirm).

## Open

- **Training's rounds and patience at 1B** (TODO.md, the 1B generation 2): 2.7 times the data an
  epoch; whether `steps_per_round` or `patience_rounds` change, to settle before the first 1B run.
- **Self-play's time:** about 24 h for 402.7M positions, so about 2.7 days for 1B.

## Steps

1. The code and its tests, in one change (the layout, the names and the state change together).
2. The configs.
3. The developer: `cargo test`, clippy, commit; then the migration; then a scaled run from the
   page to see a generation 1 and 2 go through.
