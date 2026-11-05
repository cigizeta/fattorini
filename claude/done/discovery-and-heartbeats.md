# Discovery and heartbeats: the smithy claims its arenas

**Its `--dev` superseded, see smithy-anywhere.md (2026-10-29):** no `--dev` nor smoke tree; every
smithy binds the LAN and claims `machines.yaml`' arenas on any machine (`smithy:` no longer read), a
scaled run asked from the page. The claim, the polls and the heartbeats below stand. **When a
smithy claims changed, see arena-contention.md (2026-10-29):** only while its job deals games, a
claim outranking another (full over scaled, then by rank) taking the arena.

**Status: design agreed with the developer, 2026-10-23; steps 1 to 7 done the same day: built, every
test passed, and the `--dev` checks on another machine worked (the developer). Step 8 next, on the training machine and
the Lichess machine.** Replaces how the smithy and
the arenas find each other and know each other alive: `data/smithy.yaml`, `SESSION_TIMEOUT` and its
expiry. The HTTP transport (http-transport.md) and the chores (chores.md) stay. `--dev` replaces
`smithy smoke` and the `--local` of TODO.md and smithy-window.md.

**Changed while implementing (Claude, 2026-10-23): `/hello` stays,** as the per-job join behind the
claim. The smithy rebuilds its sessions and token at every job (`run_job`), so the arenas already
say hello again at each job on `TOKEN`; keeping it left all of that untouched. The claim says which
smithy to talk to and keeps the arena alive; the hello still hands out the session, the token and
the store. So the claim carries no token or store, and a lost arena's session is kept (only its
chore closed), so a task it sends once back is still taken. `/alive` is a `POST` with the boot id.

## Requirements (the developer, 2026-10-23)

1. The smithy may be started from any machine, not only the training machine. Training still needs the training machine's GPU.
2. `--dev`: the smithy and one arena on the same machine, every config scaled down, everything kept
   on the local disk, never on `G:`.
3. Arenas may stay lazy: once started, they wait for a smithy to contact them.
4. At most one arena a PC.

## Lineage

Not Kubernetes, though its shape is close (a resident agent a machine, one control plane). The
nearest model is **HTCondor's claiming**: every machine runs an agent saying it is available, a
submitter claims the machine, which then serves that claimer alone while keep-alives flow, and
returns to the pool when they stop. The work side is the volunteer-computing pattern of BOINC,
fishtest and OpenBench: work dealt in small pieces, lost pieces dealt again, duplicates dropped by
identity. Differences from Kubernetes, on purpose: the smithy finds and polls the arenas (closer to
Prometheus scraping) rather than the arenas registering, and a claim is exclusive.

## Decisions (the developer, 2026-10-23)

1. **Arenas are started by hand** (`bin/arena start`), on each PC that may be used, and idle until
   claimed. Closing an arena withdraws its PC. No scheduled task, nothing listening unless asked.
2. **An arena is always available unless claimed**, with all its workers. No availability rules.
3. **One smithy at a time, real or `--dev`, is the developer's responsibility.** Nothing enforces
   it: a smithy finding an arena claimed by another simply does not use that arena.
4. **The smithy claims the arenas** of the machines `data/machines.yaml` lists. With none
   available it starts anyway, keeps probing, and says it has no arenas.
5. **`--dev` claims only the arena on `127.0.0.1`**, its store the local smoke tree, never `G:`.
6. **Liveness, driven by the smithy:** it polls each claimed arena every 10 s. An arena unanswering
   for 30 s is *late*, for 60 s *lost*: its chore is closed, so its work is dealt again. An arena not
   polled for 60 s releases its claim and idles, claimable again.
7. **A result arriving after its chore was closed is kept if the work still wants it**; one arriving
   twice is dropped by its tags, as today.
8. **An outbox in memory:** a finished task that could not be sent is kept and sent again once the
   arena reaches its smithy. An arena restart loses at most one task a worker.

9. **A machine that will not be claimed is retried with an exponential backoff (the developer,
   2026-10-23):** after a claim fails (no arena answering, held by another smithy), the next try
   waits `CLAIM_POLL`, then doubles each failure, up to `CLAIM_BACKOFF_CAP` (5 minutes); a different
   failure starts again from `CLAIM_POLL`, a claim taken forgets it. The page's Machines table shows
   the failure and when the next try is. Accepted: an arena released by another smithy may idle up to
   the cap before this one claims it.

10. **No commit check (the developer, 2026-10-23), replacing the refusal of an arena at another
    commit:** while developing every tree is `(modified)`, two modified trees compare equal however
    they differ, and squash merges rewrite hashes, so the check refused harmless arenas (another machine, the same
    day) and missed real differences. A protocol version was weighed and declined. Any arena is
    claimed; the Machines table shows an arena's commit when it differs from the smithy's, for the
    developer to judge. An incompatible arena fails on its first request instead.

## Design

### Two directions, two purposes

- **Smithy to arena: claim, poll, release.** The smithy initiates; this is the new part.
- **Arena to smithy: `/chore` and `/task`, as today.** A finished task goes at once, and the answer
  tops up, stops or replaces the chore. Moving results into the poll's reply would delay every
  top-up by up to 10 s and slow tunes; not worth it.

So every machine that may run the smithy or an arena accepts inbound connections on both ports.

### Ports

- `SMITHY_PORT` 47821, as today, now always bound on the LAN (any machine may run the smithy) and on
  `127.0.0.1`; under `--dev`, on `127.0.0.1` alone.
- `ARENA_CONTROL_PORT` 47822, as today: `127.0.0.1` alone, `arena stop|status`, and it keeps a second
  arena from starting.
- **`ARENA_PORT` 47823, new:** the arena's claim endpoints, on the LAN and `127.0.0.1`.

### `data/machines.yaml`

Gitignored, with `examples/machines.yaml` holding placeholders: the names of the machines a smithy
probes, as each machine's `data/machine.yaml` names it and as the LAN resolves it. Replaces
`data/smithy.yaml` and `examples/smithy.yaml`, which go, with `smithy_machine`, `smithy_host` and the
smithy's `lan_host`.

### The arena's endpoints, on `ARENA_PORT`

- **`POST /claim`** with the smithy's machine, host, port, token, boot id, store and whether it is
  `--dev`. Unclaimed, or claimed by the same boot id: `OK` with the arena's `Hello` (machine,
  workers, commit). Claimed by another boot id whose smithy has not gone silent: `CLAIMED` with that
  smithy's machine, and nothing changes. A new boot id from the *same* smithy machine replaces the
  old claim at once: a restarted smithy.
- **`GET /alive`** with the token: `OK` with each worker's state (idle, waiting for a network in the
  store, playing task `seq` since how long) and the outbox's size. Each poll is the claim's
  keep-alive. A wrong token: `UNCLAIMED`, and the smithy claims again.
- **`POST /release`** with the token: the arena drops the claim and idles. A smithy stopping says it
  to each of its arenas, so they need not wait 60 s.

### The smithy's side

- A **prober** thread: every 10 s, for each machine listed (only `127.0.0.1` under `--dev`) not yet
  claimed, `POST /claim`. `CLAIMED` is said once a machine. No commit is checked (decision 10).
- A **poller** thread: every 10 s, `GET /alive` to each claimed arena; late at 30 s, lost at 60 s
  (decision 6). Losing an arena is today's `drop_session`: its chore closed, which returns its work
  (pairs are counted from open chores only, self-play by positions arrived, a tune's `k`s given back).
- A session is now a claim: its id `machine-n` as today, its `Hello` from the claim's reply.
  `last_seen` is set by every answered poll and every `/chore` and `/task`.
- The page (and later the window) shows an arenas table: machine, workers, state (claimed, late,
  lost, refused, claimed by another), each worker's state, games an hour, last heard.

### The arena's side

- `bin/arena start` binds its two ports and idles: "waiting to be claimed". No `smithy.yaml`, no
  `--local`: it serves whichever smithy claims it, the `--dev` one on its own machine included.
- Claimed, it plays as today (`/chore`, `/task`), towards the host and port the claim named.
- Unpolled for 60 s, or released, it stops dealing, lets each worker finish its current task into the
  outbox, and idles. The outbox is sent to the next claim of the same boot id; a different boot id
  (another smithy start) discards it, since its chores' ids mean nothing there.
- A `/task` that fails to send goes to the outbox; the main loop sends the outbox first whenever it
  reaches the smithy.

### `--dev`

`bin/smithy start --dev [--scale <s>]`: `start` under `layout::enter_smoke(s)` (1024 by default),
bound to `127.0.0.1` alone, probing only `127.0.0.1`, its store the smoke tree's stand-in for Drive.
Jobs are started from its page as for a real smithy, which says clearly that it is a dev smithy.
`smithy smoke <config>` goes. The window (smithy-window.md) inherits `--dev` when built.

### What goes

`SESSION_TIMEOUT` and `expire`'s 30 minutes, `data/smithy.yaml` and `examples/smithy.yaml`,
`smithy smoke`. Not `/hello` (see the status above).

## Plan

1. **Protocol** (`protocol.rs`): `ARENA_PORT`, `CLAIM_PATH`, `ALIVE_PATH`, `RELEASE_PATH`; `Claim`
   and the alive reply as `name<TAB>value` texts with `text`/`parse` and round-trip tests; the
   module's doc rewritten for the two directions. `/hello`, `HelloReply`, `SmithyConfig` and its
   helpers removed. `machines(path)` reading `data/machines.yaml`, its errors saying how to write it.
2. **Arena** (`chores.rs`): the claim listener on `ARENA_PORT`; claim state (idle, claimed by boot id
   and token, last polled); the main loop idle until claimed, then dealing as today towards the
   claim's host; release on 60 s without a poll or on `/release`; the outbox. `arena status` says
   idle or claimed by which machine. Tests: claim, refusal by another boot id, takeover by a new boot
   id of the same machine, release on silence (the 60 s injectable), outbox kept and discarded.
3. **Smithy** (`smithy.rs`): always bound on the LAN (loopback alone under `--dev`); the prober and
   poller threads; sessions from claims; late and lost; `/release` to every arena at stop; `lan_host`,
   `/hello` handling and `expire`'s timeout removed. Tests over loopback: a fake arena claimed,
   polled, lost, its chore closed and its pairs dealt again; a late task kept, a duplicate dropped.
4. **`--dev`:** `start --dev [--scale <s>]`, `smoke` removed, its job setup moved behind the page's
   job start under the smoke layout; the page marked dev.
5. **The page:** the arenas table.
6. **Files and docs:** `examples/machines.yaml` added, `examples/smithy.yaml` removed; DEV.md
   (starting arenas by hand, `machines.yaml`, the firewall rules for ports 47821 and 47823 on every
   machine, `--dev`); TODO.md (the `--local` item, the statements that the smithy runs on the training machine only);
   smithy-window.md (`--local` becomes `--dev`).
7. **Checks, on one machine first:** `cargo test`; `bin/arena start` and `bin/smithy start --dev` in
   two terminals; a scaled forge to its gate; the arena stopped mid-chore (lost after 60 s, work dealt
   again once it restarts); the smithy stopped and restarted (the arena released, then claimed again,
   its outbox sent).
8. **Checks, on several machines:** `machines.yaml` listing them, a real smithy on one, arenas on the
   others, an arena at another commit claimed and its commit shown, a smithy started on a machine
   other than the training machine.

## Open, to decide while implementing

- Whether the smithy's machine also runs an arena of its own: decision 4 says yes when listed, which
  makes the training machine's arena and its training share the CPU during a phase that does both. Today it does.
- The worker count in `Hello`: as today (the arena's own default), no change planned.
