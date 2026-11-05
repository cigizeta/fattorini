# Distributed `arena spsa`: implementation plan

**Superseded, 2026-10-22: see orders.md**, where a tune became a job of the server (now the
smithy's, smithy-and-arenas.md) and `arena spsa` was removed.

**Status: approved 2026-10-20; steps 1 to 6 implemented, not yet built; step 7 after the first
distributed tune.** Implements distributed-spsa.md, whose decisions it
takes as given. Read from the code as it is: `spsa_command.rs` (`tune_in`, `run_worker`, `Shared`),
`spsa.rs`, `rotor::openbench::bench`, `config::ControlSpec`, `clock::TimeControl`.

## Revision: the LAN transport (approved 2026-10-20; implemented, not yet built)

distributed-spsa.md's revision moves the transport to TCP. In `spsa_remote.rs`:

1. **Messages.** A request is a verb line, the token line, then a body; a reply is a status line
   then a body. Verbs and replies:
   - `HELLO` with the `hello` text: `OK` with the session id and `spec.txt`'s text, or `REFUSED`
     with the reason (commit, network hash). A machine's new `HELLO` drops its older session's batch.
   - `WORK` with the session id: `OK` with the `work` text, `WAIT` (no `k` free now; ask again in
     5 s), `CLOSED`, or `UNKNOWN` (no such session: a restarted server; the client says `HELLO`
     again).
   - `RESULTS` with the session id, the batch id and the `results` text: `OK`, or `IGNORED` (the batch
     expired or was dropped).
   - A wrong token: `REFUSED`, with nothing about the token in the reply or the server's output.
2. **The server.** A listener thread on `SPSA_PORT` (a constant), one request at a time, each a
   call of `handle(request, now) -> reply` over the dispatch state, which the tests call without a
   socket. Expiry checked at every request and every 5 s by the dispatcher loop. `server.txt` on
   Drive at start (machine, port, token, player, `open`), rewritten `closed` at the end.
3. **The client.** Reads `server.txt`; refuses to start when it says `closed` or is absent. `HELLO`,
   then a loop of `WORK`, play, `RESULTS`. A connection error is retried every 5 s, printed once;
   `UNKNOWN` sends `HELLO` again. Its games go to its own machine's `games.pgn` in the tune's local
   folder, not to Drive.
4. **The token:** 128 random bits in hex (`rand`), regenerated per server start.
5. **Removed:** the client folders, `open.txt`, `status.txt`, `done` markers, the heartbeat, and the
   Drive scanning that found open tunes.
6. **Tests:** every message round trips; `handle` over the scenarios the Drive dispatcher test
   covered (join, apply a partial batch, the unreported `k` first, expiry, another commit, a wrong
   token, a restarted client); one loopback test with a real listener on `127.0.0.1`.

The developer adds a Windows firewall rule for `arena.exe` on the Lichess machine once.

## What changes in today's tune

1. **The `k` pool.** Today `Shared::next_k` counts up and the run ends when it reaches `pairs`. With
   batches that can be dropped, a dropped batch's `k`s must be played again, or `k_done` never
   reaches `pairs`. `Shared` gets `returned: Vec<u32>`: a dropped batch puts its `k`s there, and the
   next `k` handed out, local or remote, is the smallest returned one before `next_k`. Delta is
   `draw_delta(seed, k)`, so a replayed `k` plays the same Delta, as a resume already does. The run
   ends when `k_done == pairs`; results arriving later are ignored and said so.
2. **Remote pairs update theta like local ones:** under the same lock, `spsa::update` with each
   pair's own `k`, `state.tsv` rewritten, one `pairs.tsv` row each, with a `source` column (`local`
   or the client's machine name). The header changes; an old `pairs.tsv` without the column is
   refused at resume, with the message to move it away (no tune has a `pairs.tsv` worth keeping).
3. **One more thread on the server**, the dispatcher, beside the local workers: every 30 s it reads
   the clients' folders, applies finished batches, drops expired ones, writes new ones, and rewrites
   `status.txt`. It stops with the run.

## Files on Drive

The tune's folder is `tune_dir(player)`, as today, and its Drive mirror
`remote::remote_path(tune_dir)`:

- `open.txt` (server): the player and the reference nps, written when `--serve` starts, and kept;
  `closed.txt` (server) marks the end, naming the final `parameters.txt`.
- `spec.txt` (server): copied up at start, so a client checks the run it joins.
- `status.txt` (server): one line per live client: last batch, pairs, score, pace, last seen.
- `clients/<machine>/<session>/` (one per client start; `<session>` the client's start time,
  `YYYYMMDD-HHMMSS`):
  - `hello.txt` (client): machine, workers, loaded nps, commit, network hash. A newer session of the
    same machine makes the server drop the older one's open batch at once.
  - `refused.txt` (server): why the client was refused (commit or hash), which the client prints and
    exits on.
  - `work-<n>.tsv` (server): a header line (`n`, the scaled control, the time it was written), then
    one line per pair: `k`, the plus parameters, the minus parameters. Deleting it is how the server
    cancels it; the client checks before each new pair and drops the batch when it is gone.
  - `results-<n>.tsv` (client): one line per pair, `k` and `r`, then the batch's play time; then
    `done-<n>` (client), empty, written last: the server reads a batch only once its `done` is there.
  - `games-<n>.pgn` (client): the batch's games, for inspection, as `games.pgn` locally.

One writer per file, as designed. A file is written whole through `.tmp` and a rename; the `done`
marker covers Drive delivering the rename before the content.

## The client, `arena spsa --client [<player>]`

1. Finds the tune: the `open.txt` files under Drive's `data/networks/*/gen-*/selection/spsa/` and
   `data/spsa/hand-crafted/` without a `closed.txt`. None: says so and exits. Several: lists them and
   asks for the player. The player named: that one.
2. Resolves the player as `arena spsa` does (`match_player::resolve_player`, which completes a
   champion from Drive; a candidate through `generation_sync::pull_candidate`), loads the network,
   checks `spec.txt` against its own `Settings`.
3. Measures its loaded nps: `rotor::openbench::bench` on the network, run at once on every worker
   thread (`Run::placement`'s, pinned the same way), the mean of their nps.
4. Writes `hello.txt`, then loops: waits for a `work-<n>.tsv`, plays its pairs on its workers (one
   pair a worker at a time, as `run_worker`, the parameters from the file instead of theta), and
   writes `results-<n>.tsv`, `games-<n>.pgn`, `done-<n>`. Polls every 10 s while idle.
5. Exits on `closed.txt` ("the tune is over") or `refused.txt` (its reason). Ctrl+C stops at once;
   the batch is dropped, as designed.

## The server, `arena spsa <player> --serve`

Today's `tune` plus the dispatcher. Without `--serve` nothing changes.

- **At start:** measures the Lichess machine's loaded nps the same way (or reads it from `open.txt` on a resume,
  so it is measured once per tune), copies `spec.txt` up, writes `open.txt`. A resume drops every
  open batch: they are not in `state.tsv`, so their `k`s go back to the pool.
- **A new session:** checks `hello.txt`: commit equal to the server's, network hash equal. Else
  `refused.txt`.
- **A batch:** sized `workers * pairs_per_worker`, `pairs_per_worker` from the client's last batch
  time to come to 5 minutes (`BATCH_SECONDS = 300`), clamped 1 to 16, 2 for the first batch. The
  control is the tune's scaled by `reference_nps / client_nps`, base and increment alike, to the
  millisecond. Both parameter sets from theta as it is when the batch is written.
- **Expiry:** a batch without `done` 30 minutes after it was written (`BATCH_TIMEOUT_MINUTES = 30`,
  by the server's own clock) is deleted, its `k`s returned.
- **A finished batch:** each pair applied in file order, one line printed:
  `spsa: the training machine batch 12, 64 pairs, 31.5/64, 1834/2500 done`.
- **The end:** when `k_done == pairs`: `parameters.txt` as today, `closed.txt` on Drive, open batches
  deleted.

## Smaller pieces

- **The commit**, for `hello.txt`: `arena`'s `build.rs` sets `ROTOR_COMMIT` from `git rev-parse HEAD`,
  `+dirty` when `git status --porcelain` is not empty. A dirty build is refused as a client and
  allowed as a server only with a warning.
- **The network hash:** FNV-1a 64 over the `.nnue` file's bytes, a few lines, no new dependency;
  hand-crafted's is the empty string.
- **`ControlSpec::scaled(factor)`:** a Fischer control with base and increment times `factor`, its
  text as `10.00+0.100 (x1.12)`; a fixed depth unchanged (a depth tune needs no scaling).
- **Machine name:** `data/machine.yaml`'s (`machine.rs`), the name self-play already publishes
  under; nothing else about the machine goes on Drive.
- **The server plays nothing itself** (the developer, 2026-10-20, after the first trial): `--serve`
  runs the dispatcher only, and the Lichess machine's cores join as a client beside it, like any machine's. The
  reference speed is still the server machine's, measured before its client starts.
- **A heartbeat** (added while implementing): the client rewrites `alive.txt` every minute, and a
  client whose heartbeat has not changed for 5 minutes gets no new batch, so a machine switched off
  does not hold `k`s for 30 minutes after every batch.

## Tests (pure, no Drive, no games)

- The `k` pool: returned `k`s handed out first, smallest first; the end at `k_done == pairs` with
  `k`s returned late.
- `work`, `results` and `hello` texts round trip; a malformed one is an error naming the file.
- Batch sizing: first batch, from a last batch time, the clamps.
- Expiry: a batch past 30 minutes returns its `k`s; a newer session drops the older one's batch.
- `ControlSpec::scaled`: 10+0.1 at 1.12 is 11.2+0.112; a depth unchanged.
- The FNV hash against known values; tune discovery over a temporary directory tree (none, one,
  several, closed).
- The dispatcher's step as a function of (the folders' contents, `Shared`, now): what it applies,
  writes, deletes. The thread around it is only a loop.

## Steps

1. The `k` pool and the `source` column in `spsa_command.rs`; today's tune unchanged otherwise.
2. `ControlSpec::scaled`, the hash, `ROTOR_COMMIT`, the loaded bench.
3. The file formats, their texts and parsers, in a new `spsa_remote.rs`.
4. The dispatcher and `--serve`.
5. The client, `--client`.
6. Help text and `arena`'s module comment.
7. **Done 2026-10-20, during the first real tune** (the developer trusting `arena spsa` by then;
   `openbench.rs` became `bench.rs`, keeping only the bench): retire OpenBench (the `Makefile`,
   `bin/openbench-server`, `bin/openbench-client`, `examples/openbench.conf`, `bin/rotor parameters`,
   `openbench::parameters_from_tune`, the ten UCI tuning options and their test lines), keep
   `bin/rotor bench`. TODO's OpenBench notes go with it.

## Steps for the developer

- Tests after each step.
- A first trial: a short tune at `--depth 2` with the Lichess machine serving and the training machine as client, then one at
  10+0.1.
