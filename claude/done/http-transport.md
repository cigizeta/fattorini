# HTTP transport, and start/stop/status for the server and the client

Status: design agreed 2026-10-22 (the developer); implemented the same day, built and tested (every test passes but one
unrelated flaky export test, see TODO.md); a real run over it still to do.
`tiny_http` and `ureq` live in `rotor` (`pipeline/http.rs`), both binaries reaching them there. The first step of
lan-only-servers.md: its transport, discovery and loopback control. Its orders with a counter, pairs
streamed as PGN and the corpus as the server's appended PGN come after, on top of this.

## Goal

- The training server and its clients speak HTTP/1.1, not `lan.rs`'s hand-made framing.
- Nothing in the protocol goes through Google Drive: the server file `data/loop/server.txt` goes.
- `smithy server start|stop|status` and `arena client start|stop|status`, `stop` and `status`
  reaching the running process over `127.0.0.1` only.
- One machine alone works with no setup: server and client on `127.0.0.1`.

## Decisions

1. **Crates:** `tiny_http` 0.12 for the server, `ureq` 2.x for the client, both synchronous, no async
   runtime. `tiny_http` is also what the control listeners use, on both sides.
2. **Endpoints of the training server**, the verbs of today's `loop_protocol.rs`, unchanged in
   meaning:
   - `POST /hello`, body the client's `Hello`; reply `OK` with the session **and the token**.
   - `POST /work`, body the session; `OK` with a `Job`, `WAIT`, `UNKNOWN`.
   - `POST /results`, body the session line and the results; `OK`, `IGNORED`.
   - `GET /network/<hash>`; `OK` with the bytes, `MISSING`.
   - The token in a header, `X-Rotor-Token`, on every request but `/hello`. A wrong one is answered
     `TOKEN`, and the client says hello again (no file to re-read any more).
   - Every protocol answer is HTTP 200, its body starting with today's status word line, so the job
     logic in `server.rs` and the client barely changes. A malformed request is a 400, a body over
     the size limit a 413, an unknown path a 404.
   - A size limit on every body: 64 MB (a job's compressed self-play games are a few MB).
3. **Two listeners on the server, never `0.0.0.0`:**
   - `127.0.0.1:SERVER_PORT`: the clients' endpoints and the control endpoints.
   - The LAN address of the machine `data/server.yaml` names, `SERVER_PORT`: the clients' endpoints
     only. Opened only when `server.yaml` names this machine.
   - So a stop or a status can only come from the server's own machine, by construction.
4. **`data/server.yaml`**, local and gitignored, `machine: <name>` (validated as machine names
   are), with a placeholder `examples/server.yaml`:
   - A client connects to that machine; **without the file, to `127.0.0.1`.**
   - A server refuses to start when the file names another machine than `data/machine.yaml`'s
     `name`: one machine only may serve. Without the file it serves on `127.0.0.1` alone.
5. **Ports:** `SERVER_PORT = 47_821`, the training server's (clients and control);
   `CLIENT_CONTROL_PORT = 47_822`, the client's control, on `127.0.0.1` only. SPSA keeps `47_820`.
6. **The server's control**, on its loopback listener:
   - `GET /control/status`: the run, phase, generation and attempt, the work being dealt, the clients
     (session, machine, workers, job out). `smithy server status` prints it, or says no server is
     running on this machine.
   - `POST /control/stop`: what the first Ctrl+C does, a stop at the next safe point. `smithy server
     stop` sends it and says so; the server's own terminal shows the stop as it happens.
7. **The client's control**, on `127.0.0.1:CLIENT_CONTROL_PORT`:
   - `GET /control/status`: the server it is joined to (or waiting for), its session, its workers, the
     job it plays.
   - `POST /control/stop`: what Ctrl+C does today, the job in play abandoned (the server hands it out
     again) and the client gone.
   - A second client on the same machine cannot start: the control port is taken, and it says so.
8. **Foreground for both:** `start` runs in its terminal and prints what it prints today (the server's
   tables, the client's bars). `stop` and `status` come from another terminal. Detached processes come
   with the web page (browser-gui.md), which replaces the tables.
9. **Renames, the one reserved for this move:** `arena loop-client` becomes `arena client start`;
   `loop_protocol.rs` becomes `protocol.rs`; `LOOP_PORT` becomes `SERVER_PORT`; `loop_client.rs`
   becomes `client.rs`, its `PREFIX` `client`. The client's cache moves from `data/loop/cache/` to
   `data/client/cache/`, its gate PGN to `data/client/games.pgn`.
10. **No Drive:** `ServerFile`, `SERVER_FILE` and every `check_mounted()` of the server and the client
    go. An Lc0 run still reads `data/lc0` on `G:`, and fails with that read's own message without it.
11. **`lan.rs` stays for SPSA,** which keeps its framing, port and Drive server file until its own
    design moves it onto this (lan-only-servers.md, "SPSA: deferred"). The text helpers
    (`fields`, `field`, `first_line`, hashing, `write_whole`) stay shared.

## Implementation

1. **Dependencies:** `tiny_http` in `rotor` (both sides' control and the server live on it) or in
   each binary that serves; `ureq` in `arena` and `smithy` (`smithy server stop|status` is a client of
   its own control). Each version exact, as `Cargo.toml` already pins.
2. **`rotor::pipeline::protocol`** (from `loop_protocol.rs`): `Hello`, `Job`, `GamesResult`,
   `PairsResult`, `results_body` unchanged; `ServerFile` and `SERVER_FILE` out; `SERVER_PORT`,
   `CLIENT_CONTROL_PORT`, the paths, the token header's name, the body limit; the `server.yaml`
   loader (`server_machine()`: `Some(name)` or `None` without the file); `HelloReply` (session and
   token). Round-trip tests as today.
3. **`rotor::pipeline::http`** (new, small): the shared pieces both sides use, so neither binary
   hand-writes HTTP: a blocking `serve(listener, handler, stop)` loop over `tiny_http` that reads a
   body within the limit and answers `(status, bytes)`; `get` and `post` over `ureq` with a timeout,
   returning the status word line and the body. Tested against a server on `127.0.0.1:0`.
4. **`smithy` server:** `handle` keeps its shape, taking `(method, path, token, body)` instead of
   parsing verbs; the `/control/*` paths answered by the loopback listener only; `run` binds the
   listeners, checks `server.yaml` against `machine.yaml`, no server file; `stop` and `status`
   subcommands. The status text built from `Shared` and the run's `GenState`, shared with the
   handler through the `Run`.
5. **`arena client`:** `client.rs` from `loop_client.rs`, finding the server by `server.yaml`,
   saying hello again on `TOKEN`; its control listener; `start|stop|status`. The usage text updated.
6. **Tests:** the server's `handle` on paths and tokens; a server on `127.0.0.1:0` and a client
   joining, getting a job, sending results, the token changing on a restart; `/control/*` refused on
   the LAN listener; `server.yaml` absent, naming this machine, naming another.
7. **Docs:** DEV.md's "Running a training run" (`arena client start`, `server.yaml`, `stop`,
   `status`), the firewall note for `SERVER_PORT` on the LAN; TODO.md; lan-only-servers.md pointing
   here for its transport.

## Open questions

None; to settle in the code: whether `tiny_http` lives in `rotor` or in each binary (it adds no
dependency the engine binary links if only the pipeline module uses it, which is to be checked).
