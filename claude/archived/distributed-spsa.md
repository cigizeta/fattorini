# Distributed `arena spsa`: a server and clients through Google Drive

**Superseded, 2026-10-22: see orders.md**, where a tune became a job of the server (now the
smithy's, smithy-and-arenas.md) and `arena spsa` was removed.

**Status: design agreed 2026-10-20, every point settled; plan not yet written.** A simplified OpenBench of our own, for
SPSA only. OpenBench itself (openbench-implementation.md) stays usable meanwhile.

## Revision, 2026-10-20: LAN instead of Drive (the developer)

After the first trial on the Lichess machine, the transport moves from Drive files to the local network, both
machines being on the same network whenever a tune is distributed. What changes:

- **Messages over TCP**, one connection per request, as plain HTTP without HTTP: the client connects,
  sends one message, reads one reply, closes. Messages are the same texts as the files were
  (`hello`, `work`, `results`). No state lives in a connection, so a server restart costs a client
  only a retry.
- **Pull, not push:** a client asks for work, so no batch goes to a machine that is gone; the
  heartbeat and its 5-minute rule go. A batch still expires after 30 minutes, for a client that dies
  in one; a client's new `hello` (a restart) drops its old session's batch at once.
- **Finding the server:** `--serve` writes `data/spsa/server.txt` on Drive at start: its machine name
  (`data/machine.yaml`), port, a fresh random token, and the player; at the end it marks it closed.
  Clients read it, connect by the computer name, and send the token with every message. The token
  is regenerated at every start, lives only in that Drive file and in memory, and is never printed
  or logged.
- **Spec over the wire:** the client gets `spec.txt`'s text from the server's `hello` reply.
- **No `status.txt`:** the server prints a line per batch; Drive carries only `server.txt`.
- **Windows' firewall** must let the arena listen on the Lichess machine: a one-time rule, the developer's.

- **The reference speed is the server machine's client's** (found in the LAN trial: a bench at the
  server's start ran 10% slow beside a starting client, and the Lichess machine's own client got 1.803+0.018 for
  2+0.02). That client plays the tune's clock as it is, and its measured speed is the one every
  other machine's clock is scaled to; the server's own bench serves only until it joins.

- **Another build only warns** (the developer, 2026-10-20: development moves too fast to hold every
  machine at one commit). A client's commit and its bench node count (the same for one search,
  evaluation and network) are compared with the server's, and a difference is printed on the
  server; the client plays. Another network's hash is still refused, being another tune, and so is
  another spec (seed, ranges, `c_end`, `r_end`).

Unchanged: the server plays nothing itself, the clock scaled by bench with its 5% dead band, batches
of about 5 minutes, the `k` pool.

## Why

A first tune of 32,768 games is about 32 h on the Lichess machine alone, 14 h with the training machine (TODO.md). `arena spsa`
already tunes in-process, with adjudication and resume; what it lacks is a second machine.
OpenBench's other features are not needed: builds per commit (a tune is tied to a network, not a
commit) and a web page.

## Decisions (the developer, 2026-10-20)

- **Transport: Google Drive files.** No server process, no listening port, no address in a config.
- **Scope: SPSA only.** SPRTs stay on one machine.
- **Speed: the clock scaled by bench**, as OpenBench does: a client plays 10+0.1 times the Lichess machine's
  loaded bench nps over its own, on the tune's network.
- **Math: `spsa.rs` as it is.** Distribution only; an adaptive scheme comes later as its own change,
  measured against this one.
- **Commands:** `arena spsa <player> --serve` on the Lichess machine, `arena spsa --client` elsewhere. A client
  names no player: it joins the one tune open on Drive, and refuses when there are several.
- **Batches of about 5 minutes** of the client's play; a batch unfinished after **30 minutes** is
  dropped, its pairs redrawn.

## Design

1. **One writer per file.** Drive has no atomic claim, so no file is written by two machines. Each
   client has `<tune>/clients/<machine>/` on Drive:
   - the client writes `hello.txt` (its bench nps on the tune's network, its commit, the network's
     hash), and per batch `results-<batch>.tsv`, then a `done-<batch>` marker last;
   - the server writes `work-<batch>.tsv`: each pair's `k`, Delta, and the two parameter sets.
2. **The server is today's `arena spsa`** with remote workers beside its local ones: the Lichess machine plays its
   own pairs in-process as now. Every 30 to 60 s it reads the clients' folders, applies finished pairs
   to theta in the order they arrive, each with its own `k` (as local pairs already are), and writes
   the next batch to any client without one.
3. **Clients join and retire freely.** A new `hello.txt` is a new client. A batch past its 30 minutes
   is dropped: its pairs never reach theta and are drawn again, as a resume already does. No state of
   its own; SPSA tolerates a lost pair.
4. **Speed.** The Lichess machine's reference nps is measured once per tune, recorded in `spec.txt`. Each client
   measures its own at joining, on the tune's network, loaded (its workers busy, as in play).
5. **Same build, same network.** `hello.txt` carries the commit and the network's hash; the server
   refuses a client that disagrees and says why in the client's folder. The network comes from Drive,
   as the arena already pulls candidates.
6. **Batch size** from the client's workers and the 5 minutes, about 8 pairs a worker: theta is only
   slightly stale within a batch, as OpenBench's `pairs_per`.
7. **Nothing identifying on Drive:** folders named by the machine names the project already uses,
   no addresses, no paths beyond the data root.

8. **A client restart drops its batch.** The client starts fresh and writes a new `hello.txt`; the
   server, seeing it, abandons that client's open batch at once instead of after 30 minutes, and
   sends a new one. No local state on the client.
9. **Progress.** The local progress lines stay as today. Each arriving batch prints one line: the
   client, its pairs, its score, and theta's pair count. The server keeps a `status.txt` on Drive,
   each client's last batch and pace, readable from any machine.
10. **The end.** When the pairs are done the server writes `closed.txt`, naming where the final
    `parameters.txt` went; each client, seeing it, says the tune is over and exits.
