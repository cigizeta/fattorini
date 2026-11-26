# The lease over HTTP, without the 60 s wait

Agreed with the developer, 2026-11-01. Design and implementation plan in one document.

## The problem

A job (a forge run, a tune, PeSTO's play, Lc0's Convert, a source's first freeze) took a lease on
Drive, `lease.txt` in its remote directory (`crates/smithy/src/lease.rs`), so that it never runs in
two smithies at once. Taking one wrote the lease, slept 60 s for Drive to sync it, and read it back.
The wait was annoying (TODO.md, "The lease's 60 s wait"), and the page showed the job as stalled
meanwhile. A stale lease (10 minutes without a heartbeat) needed a Take over by hand, yet a machine
that is off is the normal state on a home LAN (the developer, 2026-11-01), so a job left by a
machine switched off blocked for ten minutes and then asked.

The smithies already reach one another's port: every machine of `machines.yaml` runs a smithy on
`SMITHY_PORT`, which its arenas reach over the LAN.

## What was weighed

- **Leader election (ring, bully):** elects a coordinator among the machines that answer, assuming
  a silent machine crashed; a split network elects two. It solves nothing here.
- **Consensus (Paxos, Raft):** safe, but needs a majority of the machines up to act; with most
  machines off most of the time, no job could start. CAP's choice: availability.
- **Fencing tokens:** need storage with compare-and-swap. Drive has none.
- **A first-minute rule** (no wait, since a job would write nothing in its first minute): false.
  PeSTO's play, a source's freeze, a forge run and a tune all read or write Drive right after their
  lease (`settle_segments`, `freeze::ensure`, `sync_down` and the archiver, `Tune::open`).
- **Presence files on Drive** (each smithy's heartbeat, to tell a machine off from one on but
  unreachable), first beside `lease.txt`, then holding the leases themselves: implemented in a
  first version, then dropped (the developer, 2026-11-01). On a home LAN the connection does not go
  down, so a silent machine is off.
- **Periodic checks while a job runs** (over HTTP or Drive, to catch a double run): refused. The
  smithy that wants a job asks; nobody asks again afterwards.
- **Ranks, a handover by rank, and Take over:** dropped (the developer, 2026-11-01). A job taken is
  refused, whatever the machines; the developer stops it by hand where it runs, which is safer.

## Decisions

1. **No file on Drive for coordination.** No `lease.txt`, no presence, no heartbeat. A job is held
   by the smithy running it, in memory, and said over HTTP.
2. **A job's key** names it, the same on every machine: its former lease path under the remote's
   root (`data/lc0-counts/lease.txt`, a run's, a tune's). The callers keep their paths; no file is
   written there, and old `lease.txt` files are ignored (and still never copied by `tree.rs` and
   `tune.rs`'s filters).
3. **At a start,** the smithy asks every other machine of `machines.yaml` at once, `GET /jobs`, a
   timeout of 5 s and one retry, and decides once; it never asks again. Refused when another smithy:
   - holds the job, or is taking it: "runs in another smithy";
   - released it under 2 minutes ago (decision 4): "stopped in another smithy N s ago; Drive may
     still be syncing its last files: resume in a minute";
   - answers 404 or unreadably: "another smithy is too old to ask: rebuild it".

   A machine that times out, or refuses the connection, twice, holds nothing: it is off, or runs no
   smithy. Otherwise the job starts at once. This smithy itself refuses a job it already runs.
4. **The sync lag after a stop:** a smithy keeps each job it released for 2 minutes and lists it as
   released with its age in seconds, so a resume elsewhere waits for Drive to sync the job's last
   files (its stop's copy to Drive). An age, not a time, so clocks never compare. A smithy stopped
   or restarted meanwhile forgets them; accepted.
5. **No machine named** in any message or on the page.
6. **The residual risk, accepted:** a machine that is on and holds a job but fails to answer twice in
   5 s (a hung smithy, a name lookup failing, a missing firewall rule); nothing detects the double
   run that follows.

## The protocol

- `GET /jobs` on `SMITHY_PORT`, on the LAN, no token: `OK`, then one line per job,
  `taking<TAB><key>`, `held<TAB><key>`, or `released<TAB><key><TAB><seconds ago>`.

## Implementation plan

The first version (presence files beside `lease.txt`, no wait) is in the working tree; this revision
replaces it.

1. **`lease.rs`:** `lease.txt`, `Record`, the heartbeat thread, `Holder`, the stale rules, `Timing`
   and `take_over` go; the registry of keys keeps `taking` and `held`, and gains each released key
   with its `Instant`, kept 2 minutes; `take(path, job)` asks (decision 3) and returns a `Lease`
   whose drop marks the key released. The module's comment rewritten; tests over decision 3's rules
   with fake answers, and over the registry.
2. **`presence.rs`** deleted, with `smithy start`'s presence.
3. **`smithy.rs`:** the five callers lose their `take_over` and `TIMING` arguments, and the lost
   flags they passed; the run list's `lease` field and `run_lease_path`'s reader for the page go;
   the forge's `take-over` word (`TAKE_OVER`, its `job_name` case) goes.
4. **The page:** the run list's lease note and Take over button go, with their confirmation; a
   refusal comes back from Resume as any.
5. **Docs:** DEV.md's lease paragraph; TODO.md's item.
6. **Checks by the developer:** `cargo test -p smithy`, clippy; on two machines, a Resume refused
   while the job runs on the other, a Resume refused within 2 minutes of stopping it there and
   accepted after, and a Resume after the other machine was switched off (started at once).

## Progress

- 2026-11-01: the first version written (presence files, `lease.rs` rewritten, `JOBS_PATH`, the
  route in `handle`, the page, DEV.md). This revision designed, then written the same day, steps 1
  to 5: `lease.rs` rewritten, `presence.rs` out of the build (its file to delete by hand), the
  callers, the page, DEV.md, TODO.md. Not yet compiled nor tested.
