# LAN-only servers, and pairs streamed as PGN

**Superseded, 2026-10-22: see http-transport.md, orders.md and status-page.md**, which implemented
it, and smithy-and-arenas.md, which renamed the server and clients.

Status: design, agreed in discussion on 2026-10-21; orders with a counter, SPSA deferred, and the
corpus and test set added later the same day. Its transport, discovery by `server.yaml` and
loopback control are implemented by http-transport.md (2026-10-22), which supersedes "Transport"
below where they differ. Orders, pairs streamed as PGN, the corpus, and SPSA as a server mode are
implemented by orders.md (2026-10-22), which supersedes this document where they differ (self-play
stays single games; SPSA is not deferred). The `GET /` page is status-page.md's. The "Today" section describes the code before it.

## Goal

1. **Only power can stop a run.** The forge and SPSA servers and their clients must work with the
   Internet down, as long as the LAN is up. Nothing a run needs may go through Google Drive.
2. **Results arrive continuously.** A client reports every game pair as soon as it ends, not once all
   the pairs of its job are done, so tournament and round tables move pair by pair.
3. **One machine only may serve.** A local file on each machine names the server; no other machine
   can start one.

## Where we are

- Clients find the server through a server file on Drive: `data/loop/server.txt` (loop, gen-server)
  and `data/spsa/server.txt` (SPSA), holding `machine`, `port`, a run or tune description, a token
  fresh at every server start, and `state open|closed`. The server writes it at start; whichever
  machine starts a server becomes the server.
- The token tells server starts apart (a client holding an old one gets `TOKEN` and rejoins). It is
  not authentication: it travels in plain text and anyone reading the Drive file has it.
- `remote::check_mounted()` is called at start by `gen_server`, `loop_server`, `loop_client` and
  both SPSA roles: without `G:` none of them starts, even with a perfect LAN.
- The archiver's treasure copy of a gen run goes to Drive.
- Everything else of a run is already local or LAN: `data/gen/...`, the client's network cache
  (networks come by the `NETWORK` verb), the book and Syzygy under `%LOCALAPPDATA%\rotor`.
- `loop_client::pairs()` collects a job's results only after every pair is done, and sends one
  `PairsResult` (`r` in -2..=2 per pair, in job order). The games are written only locally, to
  `data/loop/gate-games.pgn`; the server never sees them. SPSA likewise sends `(k, r)` per batch and
  keeps its games in a local `games-<machine>.pgn`.
- Two ports: `SPSA_PORT = 47_820`, `LOOP_PORT = 47_821` ("so both can serve at once").

## Decisions

### Unit: the game pair

A single game is not a meaningful unit; only a pair is (one opening, two opponents, colours swapped).
Everything below reports, records, deduplicates and reissues whole pairs.

### Streaming pairs

- No jobs and no batches: work is assigned as **orders with a counter** (below); reporting is per
  pair.
- The client sends each finished pair at once: the PGN of its two games.
- The server appends the pair to the run's PGN file in one write, then flushes. A crash leaves at most
  one truncated pair at the end; resume drops it and the pair is reissued.
- The server derives `r` from the two `Result` tags and the colours; `r` is no longer sent.
- The server counts pairs that arrive, never pairs dealt: a pair lost with a client is simply not
  counted, so nothing is leased or reissued. A pair arriving twice is dropped by its identity.

### Orders with a counter (2026-10-21)

Pairs of one order differ only in the opening, which the client picks, so an order is a description
and a count, never a list of pairs:

- **Two players:** a network hash or `hand-crafted`, each with any engine options.
- **One limit:** a depth (self-play, round robin, gate: depth 9 today) or a time control (duels).
- **A count** of pairs.

Every worker thread of the client plays pairs of the current order back to back, decrementing the
count as each starts; the client never asks for more. The server's reply to every pair carries one
of:

| Reply | Effect |
|---|---|
| `OK` | nothing changes |
| `ADD k` | count += k |
| `STOP` | count = 0; pairs in flight finish and are sent |
| `ORDER ...` | a new description and count |

**The server tops the count up before it reaches 0,** so a client never starves and no CPU is lost.
A client whose count does reach 0 (a gap in the LAN, a phase with nothing to deal) returns to
`HELLO`, whose reply is its next order. Per mode:

- **Self-play:** top up while the positions received are short of the target; `STOP` at it. The
  overshoot is at most the pairs in flight.
- **Round robin:** top up while the pairing is short of its pairs, then `ORDER` the next pairing.
  Extra pairs of a full pairing are kept or dropped by the server.
- **Gate:** top up while the SPRT is undecided, then `STOP`; late pairs are ignored.
- **Phase changes:** pairs of an earlier order still arriving are accepted while the phase can use
  them (self-play positions always can), dropped otherwise.

A pair's identity is (order, client session, sequence number in the session), written in its tags.

### SPSA: deferred

SPSA is the one mode where pairs of an order are not alike: each plays θ + c·Δ against θ − c·Δ with
a fresh Δ, and θ moves after every result. Fitting it to orders needs Δ drawn by the client from a
seed in the order (the same RNG and draw order in arena and smithy, tested equal), θ refreshed in
the replies, and a check that pairs played at a stale θ are valid samples for `arena spsa`'s update
and its schedules. **The forge modes come first;** SPSA keeps its batches and its own port until a
design of its own moves it onto orders. SPSA and forge never serve at once, so the two ports do not
clash meanwhile.

### PGN tags

Both games of a pair carry custom tags:

```
[Order "o000123"]
[Session "<machine>-<n>"]
[Pair "17"]
[Challenger "<network hash>"]
[Teacher "<network hash>"]
```

- `Order` + `Session` + `Pair` (the pair's sequence number in the session) identify the pair; the
  order counter must stay unique across server restarts (persist it, or prefix the run).
- `Challenger` / `Teacher` give the pairing; `White` / `Black` stay readable names, `Round` keeps its
  standard meaning.

### Records

- `round-robin.tsv` / `gate.tsv` stay the record that resume and the SPRT replay read; the PGN is the
  full record of the games beside them.

### Who the server is: `data/server.yaml`

- New, local, gitignored, written once by hand on every machine, with a committed placeholder example
  (`examples/server.yaml`), as `data/engines.yaml` has `examples/engines.yaml`. The real machine name
  is never committed.
- One key: `machine: <name>`, validated as `machine::validate_name`.
- A server refuses to start unless `data/machine.yaml`'s `name` equals it. `machine.yaml` keeps its
  single key `name`: "am I the server" is the equality of the two, never a second flag.
- Clients connect to that machine on the LAN. Missing or broken file: refuse, saying how to write it.

### Server state: `server-state.yaml`

- Written by the server, local to the server only, for its own resume: `run`, `token`, `state`.
- Clients never read it.

### Token over the LAN

- The `HELLO` reply carries the current token with the session. A `TOKEN` reply sends the client
  back to `HELLO`, instead of to re-reading a file.
- No shared secret: the token only tells server starts apart, as today.

### One port

- SPSA and forge never serve at the same time (decided 2026-10-21), which reverses the earlier
  "so both can serve at once". One port constant for every mode.
- Since the one client plays any pair job, it joins whatever mode is serving; no client kind is
  needed in `HELLO`.

### Transport: HTTP/1.1 (decided 2026-10-22)

- The server speaks HTTP/1.1 on its one port, replacing the hand-made framing of `lan.rs` and
  `loop_protocol.rs`: `POST /hello` (the reply carries the session, the token and the order),
  `POST /pair` (the body is the pair's PGN; the reply is `OK`, `ADD k`, `STOP` or `ORDER ...`),
  `GET /network/<hash>` (a network's bytes). Plain request and response: every change reaches a
  client in a reply, so nothing needs a push.
- `GET /` serves a read-only page of the run's live state (phase, round table, standings, gate,
  clients), refreshed by the browser from a JSON endpoint. Controls may come later.
- Crates, not hand-rolled: a small synchronous server and client (`tiny_http` and `ureq` suggested,
  no async runtime), since the browser is a client we do not control.
- Not a step toward Linux: plain TCP through `std::net` was already portable.
- **One machine alone is a valid setup (the developer, 2026-10-22):** the server and a client on the
  same machine, the client connecting to `127.0.0.1`, need no LAN, router or firewall rule, and run the
  same code as a multi-machine run; more machines only add clients. The LAN guarantees speed, not
  correctness. Clients over the Internet stay out: the protocol has no authentication, so that would
  need TLS and real credentials, or a VPN that puts the remote machine on the LAN.

### Security (agreed 2026-10-22)

What an outsider reaching the port could do, worst first: poison the corpus through `POST /pair`
(silent, the worst); stop or start runs; serve a client a crafted `.nnue`, which the client loads and
runs (a client trusts its server); flood or read the traffic. So trust must go both ways.

- **Remote machines join through a VPN (Tailscale or WireGuard), never through an open port.** The
  VPN authenticates machines both ways and encrypts; Rotor writes no TLS code. Rotor's port is never
  forwarded to the Internet.
- **Built with HTTP, cheap and useful on the LAN too:**
  - **Control on loopback only:** `start`, `stop`, `status` and the GUI's actions answer on
    `127.0.0.1` alone, never on the port clients use; a remote stop is refused whatever the network.
  - **Validation of everything a client sends:** a pair's PGN must parse and match its order
    (players, limit, tags), within a size limit on every body, so not even a buggy client corrupts
    the corpus.
  - **Bound to a chosen interface,** the LAN's or `127.0.0.1`, not `0.0.0.0` by default, so a machine
    on a public network never exposes the port by accident.
- **Only if a VPN is ever impossible:** TLS through `rustls`, a self-signed server certificate whose
  fingerprint each client pins, and one random bearer token per client, revocable alone. Certificates
  and tokens only in gitignored local files, placeholders in `examples/` (CLAUDE.md).

### Clients and servers are independent

As in real client/server applications, neither's lifetime depends on the other's. A client never
exits because the server is closed, restarting or unreachable: it keeps polling the machine named in
`server.yaml` and joins whatever server starts there next. A server never waits for clients. The SPSA
client stops exiting on `CLOSED`, as the loop client already does.

### One client: it plays pairs, whatever the reason

A client knows nothing of gen runs, gates, round robins or tunes. It plays the pairs it is given and
streams them back; why they are played is the server's business alone. So:

- One client command, `arena client`, replaces `arena loop-client` and `arena spsa --client`.
- An order (above) describes its pairs fully: the client needs nothing else to play them.
- The opening stays the client's choice, as today: a random FEN from its local book, recorded in the
  PGN's `FEN` tag. Discussed 2026-10-21: Fishtest walks a seeded shuffle of its book, dealt by the
  server, to avoid repeats; with a book of about 1M openings, independent random picks repeat about
  k^2/2N times in k pairs (0.5 in 1,000, 50 in 10,000), which is negligible, and coordinating the
  same openings across candidates was not judged worth the complication.
- Tags that only the server understands (`Challenger`, `Teacher`, `K`) are given by the server in the
  job and copied by the client into the PGN untouched.
- **One kind of work: pairs.** Self-play is an order too (the teacher against itself, one opening
  with both colours). `Job::SelfPlay`, `Job::Gate` and `GamesResult` go.

### The corpus and the test set

- **The corpus is the server's appended PGN.** gen-server's one file per self-play job and its
  `.meta` go; positions are counted from the pairs received, and the Prepare phase (gen-prepare.md)
  scatters from this PGN. Scattering as each pair arrives is a possible later step.
- **The test set is held back by pair, never by game:** both games of a self-play pair start from
  one opening and are siblings, so one of them trained on and its twin tested would leak.
- **No self-play off the LAN (2026-10-21):** the outbox of an earlier design (since removed) is
  retired; a machine off the LAN contributes nothing.

### One server: `smithy server --<mode>`

One `server.rs` implementation, as there is one client: `smithy server --forge` (today's
`smithy gen-server`), `smithy server --spsa`, and any later mode. The client is `arena client`. This
supersedes the names `smithy forge-server` / `arena forge-client` of
self-training-simpler-attempt.md; the single-run `smithy forge` is retired at the clean-up, which is
near (2026-10-21), so the two uses of "forge" barely overlap.

The server owns the transport, sessions, token, jobs, leases, dedup by pair and the PGN appends; a
mode only decides which pairs to deal and what to make of their results.

- `loop_server.rs` is retired; `gen_server.rs` and the server half of `spsa_remote.rs` become modes.
- The SPSA server moves from `arena spsa --serve` to `smithy`. `bin/smithy` needs the `.rotor` venv,
  which the server machine has, since it is the one that trains.

### Drive out of runs

- Drop `remote::check_mounted()` from `loop_client`, the SPSA client and server, `gen_server`.
- The archiver's Drive copy becomes best-effort: Drive unreachable means skip and retry later, with a
  warning, never a stopped run.
- `data/loop/server.txt` and `data/spsa/server.txt` are retired.

### Offline checks outside the code

- `<server machine>` must resolve without upstream DNS: check once with the router's WAN cable
  unplugged; otherwise a hosts file entry, and a DHCP reservation for the server.
- The firewall rule for the port must allow the network profile Windows picks when offline.

## Open questions

1. **Self-play's settings.** Orders carry players, engine options and a depth or time control. To
   check in the code: can everything self-play needs (randomisation for diversity, adjudication) be
   set that way, or do some settings live outside UCI and need a field of their own?
2. **Which machine serves SPSA,** given that the forge server must be the training machine (the GPU) and
   an earlier design (since removed) had SPSA served by the Lichess machine.

## Files touched (expected)

- `crates/rotor/src/pipeline/loop_protocol.rs`: `ServerFile` removed; one job type (pairs); pair
  results as PGN; `HELLO` reply with token; one port.
- `crates/rotor/src/pipeline/lan.rs`: if the shared port and `server.yaml` loader live there.
- `crates/arena/src/loop_client.rs` becomes `arena client`: discovery by `server.yaml`; per-pair
  sends; no `check_mounted`; no self-play job.
- `crates/arena/src/spsa_remote.rs`: its client half folded into `arena client`, its server half
  moved to `smithy server --spsa`.
- `crates/arena/src/machine.rs` or a sibling: `server.yaml` loader.
- `crates/smithy/src/server.rs` (new): the one server, its machine check, `server-state.yaml`, PGN
  appends, dedup by pair, no `check_mounted`; `gen_server.rs` and SPSA's server become its modes;
  `loop_server.rs` deleted.
- `crates/smithy/src/archive.rs`: best-effort Drive copy.
- `examples/server.yaml`, README/DEV.md.
