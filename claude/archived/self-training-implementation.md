# Self-training loop: implementation plan

**Superseded, 2026-10-22: see self-training-simpler-attempt.md and
self-training-simpler-implementation.md**: generational forges replaced the continuous loop.

**Status: agreed 2026-10-21 (the four points under "To agree", as written); implementing.** The design is self-training.md, "Design decisions"; this
plan does not reopen it. Nothing implemented.

## Shape

**Renamed while implementing (the developer, 2026-10-21):** `smithy loop-server` and `arena
loop-client`, so the command says which side it runs; below, `smithy loop` and `arena loop` mean
them.

Two new commands, beside the old pipeline, which stays as it is (decision 10):

- **`smithy loop <config>`: the server.** It owns the run: the window, the students' training,
  the CUSUM gate, the rungs, the rate. It runs on whichever machine has the GPU (the training machine), or on
  the Lichess machine's CPU for a test (decision 7). It plays nothing itself; the same machine runs a client
  beside it.
- **`arena loop`: a client.** It finds the open loop through its server file, fetches jobs, plays,
  returns results. Any number, joining and leaving freely.

Why the server is smithy: it trains, and only smithy links libtorch. Why the client is arena: it
plays, with self-play's and the matches' code. The transport is shared through the `rotor` crate
(step 2), since smithy does not depend on arena.

## The round

Strict alternation, one phase at a time, clients waiting (`WAIT`) between phases:

1. **Play.** The server hands out self-play jobs with the current teacher until the round's games
   reach N positions (a batch overruns by at most the games in flight, as self-play's segments do).
   Games are written as one file, `rounds/round-NNNN.pgn.zst`, with a `.meta` holding its
   positions.
2. **Slide.** The window is the latest round files totalling at least W positions (so the
   bootstrap's files, of any size, start it, decision 9). Files falling out move to `staging/`
   for the archive.
3. **Train.** Both students, stacked as two arms, train one shuffled pass over the window at the
   current rate, each with its own lambda, the direct blend (decision 5).
4. **Gate.** Each student is exported to `.nnue`; the server hands out gate jobs, 16 pairs a
   student against the teacher (decision 3). Each pair's result `r` in -2..=2 is its pentanomial
   score `(r + 2) / 4`.
5. **Decide.** Each student's `S` is updated; promotion, the pair's move, patience and the rate
   follow the rules of "Claude's rewrite of the idea" and decisions 2 and 4.
6. **Record.** The round's state is written (decision 8), then the next round starts.

## Steps

### 1. Pure logic, in `rotor` (no I/O, unit-tested first)

`crates/rotor/src/pipeline/self_training.rs`:

- **The ladder:** `rung_lambda(k, gamma)`: rung 0 is 1.0, rung `k >= 1` is
  `1 / (1 + 99 * gamma^(k-1))`, computed exactly; `rung_name(k)` rounds to two decimals only for
  names. Tests: rungs 1 to 11 against the table in continuous-self-play.md; never 0, never negative.
- **The CUSUM:** `Cusum { s }`, `update(&mut self, pair_scores: &[f64], delta) -> f64`, `s = max(0,
  s + mean - 0.5 - delta)`. Tests: a run of 0.5 pairs stays at 0; a gain drifts up; a loss resets.
- **The controller:** `LoopState { round, rungs: [u32; 2], s: [f64; 2], stage, rounds_since_promotion,
  teacher_round }` and `decide(state, s_after, h, patience, stage_count) -> Decision` with
  `Decision { promoted: Option<Student>, rungs, rate_step, stop }`:
  - the first student over `h` wins; both over: the larger `S`;
  - the winner keeps its rung, the other moves to the far side; rung 0 cannot go higher;
  - any promotion zeroes `rounds_since_promotion` and both `S`;
  - `patience` rounds without one: the rate steps down, both students restart from the teacher;
    past the last rate: `stop`.

  Tests for each rule, and for the starting rungs 2 and 3 (decision 6).
- **The state file:** `LoopState` as `key\tvalue` lines, as `spsa_remote`'s messages, written
  `.tmp` then renamed. Round trip test.

### 2. The transport, lifted out of `spsa_remote` into `rotor`

`crates/rotor/src/pipeline/lan.rs`: the pieces spsa_remote already has that are not SPSA's:
`request`/`request_text`/`first_line`, the `fields`/`field` parser, `new_token`, `fnv1a` and
`network_hash`, `ServerFile` (machine, port, token, state) with its read and write on Drive, and the
accept loop. `spsa_remote` then calls them: a move, not a change of behaviour; its tests must pass
untouched. Two additions, which spsa does not use:

- **Binary bodies:** a reply may carry bytes after its text header (`length\t<n>` then the bytes),
  for games and networks.
- **The server file's path** as a parameter: `data/loop/server.txt` beside spsa's
  `data/spsa/server.txt`. Open: both may run at once, so a separate port too (`LOOP_PORT`, the next
  free one after `SPSA_PORT`).

### 3. The client, `arena loop`

`crates/arena/src/loop_client.rs`, on `spsa_remote::Client`'s state machine (`Standing`, retry on
`TOKEN` or an unreachable server, a new `hello` dropping the old session's job):

- **`hello`:** machine, workers, commit, bench nodes (warned on mismatch, as spsa).
- **`network <hash>`:** downloads a network the client does not have into
  `data/loop/cache/<hash>.nnue`, checked by `fnv1a`. Unlike spsa, networks travel: the loop makes
  a new one every round.
- **`work`:** one of
  - `selfplay teacher=<hash> positions=<n>`: plays games as `self_play::play_one_game` does
    (depth 9, UHO, MultiPV for 6 plies, the same PGN annotations), sized to about 5 minutes of the
    client's workers, as spsa's batches;
  - `gate student=<hash> teacher=<hash> pairs=<n>`: plays pairs through `runner::play_games`,
    depth 9, no adjudication, as the gates;
  - `WAIT`, `CLOSED`.
- **`results`:** the games as one zstd-compressed PGN body, or the pairs' `r` values.

To check while implementing: `self_play::play_one_game` and the worker around it are callable with
a network loaded from any path and a sink other than the staging file, without changing `arena
self`'s behaviour; if not, the per-game function is extracted, `arena self` calling it as before.

### 4. The trainer, in smithy

`crates/smithy/src/self_training.rs`, reusing forge's parts, not its run layout:

- **The window through forge's own stream, one shuffling code (the developer, 2026-10-21).** The
  window is a list of `StreamFile`s (the round files and their `.meta` counts), which
  `PgnStream`/`PgnTrainStream::open` already accept from any source. With `shuffle_buffer` at
  least the window's positions, its random draws are a full shuffle of the window, so decision 1's
  shuffle needs no code of its own, and forge's and the loop's shuffling stay one. Each round
  opens a fresh stream over the new window, with the two students' lambdas as its label columns
  (direct blend), and trains `window positions / batch` steps: one epoch, one pass. Since the
  stream is reopened every round, a move of the rungs costs nothing.
  **Found while implementing (2026-10-21):** forge's buffer refills each drawn slot with the next
  record, so even holding the whole window it is not a full shuffle: over one pass of W draws a
  position comes out with probability about 1 - 1/e, some twice, some never. So the stream gained
  `PgnTrainStream::open_one_pass`, the same buffer drained without refilling (a Fisher-Yates
  shuffle as it goes), forge's own behaviour unchanged.
- **Restarts use a fresh optimizer (implemented 2026-10-21),** not the winner's state: simpler, and
  AdamW's bias correction makes a fresh start sound; to revisit if the first steps after a
  promotion prove unstable.
- **Two arms:** the existing `Stack` with `StackedAdamW`, label columns from the two students'
  lambdas, one rate for both. Each round: `train_step` until the pass ends.
- **Restart from the teacher:** both slots loaded from the winner's `.vs` and optimizer state; at a
  rate step, from the teacher's.
- **Export:** `write_engine_network` for each student every round (the gate) and for a promoted one
  (the teacher).
- **Monitoring only, deciding nothing:** each student's loss on the new round's games before it
  trains on them (fresh data), beside its training loss: the overfitting watch of decision 1.
- **Device:** `cuda_if_available`, as forge (CPU on the Lichess machine).

To check while implementing: whether `Stack`, `StackedAdamW`, `write_engine_network` and the reader
are usable from a new module inside smithy without forge's `SelfPlayInputs`; smithy stays a binary
crate, the new module living in it.

### 5. The server, `smithy loop`

`crates/smithy/src/loop_server.rs`: the round of "The round" above, the transport of step 2, the
trainer of step 4, the controller of step 1.

- **Run directory** `data/loop/<config>/` (local to the server, never Drive):
  `state.txt`, `history.tsv` (one row a round: positions, the window's size, each student's pair
  counts by score 0/0.25/0.5/0.75/1, `S`, losses, promotion, rungs, rate), `teacher/` (`.nnue`,
  `.vs`, optimizer), `students/<a|b>/` (`.vs`, optimizer), `rounds/`, `staging/`.
- **Resume (decision 8):** at start, `state.txt` and the latest complete files; a round without
  its state written is replayed from its start, its partial games discarded.
- **Jobs:** a self-play job names the round; games of an earlier round arriving late are dropped
  (strict alternation keeps it simple). A job unfinished after 30 minutes is handed out again.
- **Stop:** Ctrl+C finishes the current phase's jobs, writes nothing partial, closes the server
  file.

### 6. The bootstrap: the old pipeline, then an import (decision 6)

**The bootstrap is a forge as on Lc0's labels, with one lambda (the developer, 2026-10-21):** a
fixed teacher, one training to convergence, no generations; only the labels differ, PeSTO's
self-play instead of Lc0's games, and the lambdas, 0.98 alone instead of ten. So it runs on the
existing commands, not as new code:

1. **Play:** `arena self hand-crafted` until 96 x 1024 x 1024 positions.
2. **Train:** `smithy forge` with a config of `labels: self`, `blend: direct`, `lambda: [0.98]`:
   today's rule as it is (patience on test loss, rollback to the best), right for a fixed
   teacher. One lambda, so no select.
3. **It must beat PeSTO before it teaches (the developer, 2026-10-21; almost certain to):**
   today's gate against `hand-crafted`, then promotion.

The loop then imports it: `smithy loop <config> --from <network>-0001` takes the champion as the
first teacher (its `.nnue`, `.vs` and optimizer state) and copies the segments holding the
corpus's latest W positions from Drive into `rounds/`, once, as the first window. The students
start on rungs 2 and 3.

To check while implementing: `self.yaml`'s `train.positions` is global today, and this lineage
needs 100.7M (the open "corpus size" of continuous-self-play.md); and whether a one-lambda forge
with `labels: self` goes straight from candidate to gate without a select.

### 7. Configs

`configs/loop-psq.yaml`, its own small schema (`loop_config.rs`), refused by forge and vice versa:

```yaml
architecture:          # as lc0-psq's
  kind: psq
  embedding_dim: 256
  value_head:
    hidden_dim: []
window:
  positions: 16777216  # W, 4096 x 4096
  new_positions: 4194304 # N, 1024 x 4096
training:
  batch_size: 4096
  learning_rate: [3.0e-3, 1.0e-3, 3.0e-4, 1.0e-4, 3.0e-5]
  patience_rounds: 36  # placeholder, decision 4
ladder:
  gamma: 2.0
  start_rungs: [2, 3]
gate:
  pairs: 16
  elo_delta: 30
  h: 0.55              # placeholder: decision 3's 1.09 at 8 pairs and d = 0.5, halved at 16
search:
  depth: 9
```

`configs/loop-psq-cpu.yaml`: the same, everything small, for the Lichess machine's mechanics test: W 256k,
N 64k, patience 4, gate 4 pairs. Its bootstrap may be any existing promoted psq network with
self-play games (for mechanics, its strength does not matter), so the test waits for no new
forge.

**A test start, `bootstrap: hand-crafted` (the developer, 2026-10-21),** with `architecture:` naming a
forge config: no promoted psq network with its games exists on the Lichess machine (amanda's generation 1 has
neither its `.vs` nor its corpus), so for the mechanics test the hand-crafted evaluation teaches, the
students start from random weights, the window grows from the loop's own games, and the first
promotion is a student beating it. A test path only: the real run starts from self-psq-direct
(decision 6), its bootstrap run on the training machine.

Plus the bootstrap's forge config, `configs/self-psq-direct.yaml`: `lc0-psq-direct.yaml` with
`labels: self` and `lambda: [0.98]`.

### 8. Steps for the developer

1. `cargo test -p rotor -p smithy -p arena`.
2. On the Lichess machine, the server and a client, in two Git Bash windows:

```bash
bin/smithy loop loop-psq-cpu
```

```bash
bin/arena loop
```

3. Watch `history.tsv`: rounds closing, the window sliding, the gate's pair counts, a promotion
   moving the rungs.

## Not in this plan

- The h and P simulation (decision 3): once the first rounds' pair counts exist.
- The transfer of `staging/` to another machine (decision 9): a `bin/` script with `rclone move`, written once
  the loop runs.
- Gate games into the window, relabelling, skipping MultiPV positions: later experiments.

## To agree

1. The split: server in smithy, client in arena, transport in `rotor`.
2. The bootstrap through the old pipeline (`arena self hand-crafted`, forge, gate), then imported
   by `--from`.
3. Strict alternation, late games dropped.
4. The order of work: steps 1 to 7, the CPU test as soon as steps 1 to 5 run.
