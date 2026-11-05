# The server's status page

Status: design agreed with the developer on 2026-10-22, plan written the same day; implemented, built
and tested the same day (smithy 328 tests), to see in a browser. `serde_json` was already in the lock file through another crate.
The last piece of lan-only-servers.md (its `GET /` page), on top of orders.md. browser-gui.md comes
after it and builds on it.

## Goal

What a run is doing, in any browser on the LAN, without a terminal on the server: `http://the training machine:47821/`.
Read-only: nothing on the page changes anything.

## Decisions

1. **Served by the training server itself**, on `SERVER_PORT`, on both listeners (`127.0.0.1` and
   the LAN's), so any browser on the LAN reaches it. `stop` and `status` stay on the loopback alone.
2. **Two paths, both `GET`, no token** (the browser has none; the page reveals nothing a client
   could not see):
   - `/`: one static HTML page, its CSS and a few lines of JavaScript inline, compiled into smithy
     with `include_str!`. No external script, font or CDN: it works with the Internet down.
   - `/state.json`: a snapshot of the run, which the page polls every 2 seconds and draws. Internal
     to the page: no other reader is promised its shape.
3. **What it shows:**
   - **Always:** the run's name; the clients, each with machine, workers, commit, current order
     (units dealt and arrived), units taken, and seconds since last seen.
   - **A training run:** generation, attempt and phase (`state.txt`), and the phase's own:
     - play: test set and corpus positions, got of wanted;
     - train: one row per lambda, the last `history.tsv` row (step, learning rate, train and test
       loss), each file's row cached by its modification time so a poll reads only what changed;
     - round robin: the standings, as the terminal prints them (rank, lambda, points of games,
       score with its margin), and pairings complete;
     - gate: the score from the teacher's side, games, the challenger's share, LLR and its bounds.
   - **A tune:** the player, pairs done of total, and theta's table (parameter, start, now).
4. **No charts** in this step: charting `history.tsv` is browser-gui.md's.
5. **The terminal's tables stay.** The page is beside them until the GUI replaces them.
6. **`smithy server status`** prints the page's address at its end, the LAN's when there is one.
7. **No login:** read-only, on a LAN, behind a firewall rule for the Private profile, and never
   forwarded (lan-only-servers.md, "Security").

## Implementation

1. **The snapshot:** a `Snapshot` built under the shared lock, from `Shared` (work, sessions,
   orders, counters, the tune) plus what the phases keep beside it: the run's `state.txt`, the
   round robin's lambdas and players, the gate's elo_d, the lambdas' directories for training. What
   `Shared` lacks today is added to it as the phases start (a `View` field the phases set), so the
   snapshot never reaches into a phase's locals. Serialized with `serde_json` (already a dependency
   of smithy, else added).
2. **The routes:** `handle` answers `GET /` and `GET /state.json` before the token check; both
   listeners serve them. Tests: both answer without a token, `/state.json` parses, and holds a
   session after a hello.
3. **The page:** `server_page.html` beside `server.rs`, plain HTML and CSS, light and dark from
   the system's setting, one table per section, redrawn from each poll; a line saying when the last
   poll answered, and that the server is unreachable when it does not.
4. **The training rows:** `history.tsv`'s last row per lambda, cached by path and modification time.
5. **Docs:** DEV.md's "Running a training run" names the address; TODO.md; lan-only-servers.md
   pointing here for its page.

## Revision: a finished run stays up

Proposed 2026-10-22, after the first tune seen in a browser: the server exited at the end, the page
froze at 96 of 100 pairs and said the server was not answering. The developer wants to look at a
finished run minutes later.

1. **Only a stop ends the server** (`smithy server stop` or Ctrl+C), approved the same day and
   implemented. A run that ends by itself (a tune with all its pairs done, a training run at its
   end) or fails prints its result, then
   `finished; the page stays up at <url> until smithy server stop or Ctrl+C`, and waits for that stop.
   A run that was stopped exits at once as before.
2. **Clients** get no chore (chores.md) while it waits (the work is idle) and keep waiting, as between phases.
3. **The snapshot** gains `finished`, the result's line, which the page shows instead of the phase.
   A finished tune still shows its table (all pairs done, the final theta) though its work is idle.
4. **The port stays held** until the stop: the next `start` says a server is already running here,
   as it does today.
5. **Test:** a snapshot after the tune ends holds the tune's view and `finished`.

## Open questions

None.
