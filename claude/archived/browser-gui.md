# A browser GUI for Rotor on localhost

**Archived 2026-11-04: superseded, see smithy-pages.md and page-screens.md. The smithy's own page
became the GUI.**

Status: draft for discussion, proposed 2026-10-22. Not agreed, not implemented. After the forge
clean-up (forge-cleanup.md) and the move to HTTP (lan-only-servers.md, "Transport").

First step, 2026-10-22: smithy-and-arenas.md (jobs started from the smithy's own page, plain
`start|stop|status`). It settles open questions 1 and 2 below: the smithy serves the GUI.

## Goal

Watch and drive Rotor's long work from a browser instead of a terminal: no column limit (a page
scrolls sideways and lays tables out to fit), tables and charts that a terminal cannot draw, and one
place for what is today spread over several Git Bash windows.

## Where it stands against the server's page

lan-only-servers.md already gives `smithy server` a read-only page on its LAN port, `GET /`, for
anyone on the LAN to watch a run. This GUI is the other half: **local, on `127.0.0.1` only**, for
the developer at the machine, and allowed to act (start, stop), which a LAN page must never do
without authentication. Both are served the same way, so they share their page code and JSON.

## Proposal

- **One process serves it:** `smithy gui` (or `arena gui`, open question 1) on
  `http://127.0.0.1:<port>`, a fixed port beside the server's (`47_822` suggested), bound to the
  loopback interface only, so no other machine can reach it and no firewall rule is needed.
- **Same stack as the server:** `tiny_http`, one static page with its JavaScript compiled into the
  binary (`include_str!`), JSON endpoints read by the page. No framework, no Node, no build step.
- **What it shows, first:**
  - The run served on this machine, if any: the same JSON as the LAN page (phase, round table with
    every column, standings, gate and its SPRT, clients and their speed).
  - `history.tsv` of any lambda run as a chart: test loss by round, the rate switches marked.
  - The records already on disk: `generations.tsv`, `round-robin.tsv`, `gate.tsv`, duels.
- **What it does, later and only locally:** start and stop a server run (stop at the next safe point,
  as Ctrl+C does), start a duel, open a run's folder. Each action runs the same code path as its
  command, never a second implementation.
- **The terminal stays.** Every command keeps working and printing as today; the GUI is a viewer
  and a launcher over the same files and processes, never the only way to do something.

## Server and client as background processes (the developer, 2026-10-22)

The server and the client lose their terminal output as their interface: each runs in the
background, and its command line only has three verbs.

- `start`: launches the process detached (on Windows, a new process group with no console), so
  closing the terminal, VS Code or the browser never stops it; prints where its page is and returns.
  Refuses when one is already running on this machine.
- `stop`: asks the running process, over its HTTP API on `127.0.0.1`, to stop at its next safe point,
  as Ctrl+C does today, and waits until it has; a second `stop --now` ends it at once, as a second
  Ctrl+C does.
- `status`: one line or a few, read from the same API: running or not, phase, progress, the page's
  address. Exit status 0 when running, so a script can test it.

For example `smithy server start --forge <config>`, `smithy server stop`, `smithy server status`,
and `arena client start|stop|status`. Everything else a run shows today (the round table, the
standings, the gate, the clients) moves to the page.

- **Logs:** what the process prints goes to a log file beside its run (`server.log`, `client.log`),
  rotated by size, so a failure is still readable afterwards; the page can show its tail.
- **One process per role per machine:** a lock file with the process id, checked by `start` and
  cleared at exit; a stale one (its process gone) is taken over.
- **This answers open question 2:** the GUI never parents a run; its buttons call the same API as
  `start`, `stop` and `status`.

## Open questions

1. **Which binary serves it.** `smithy` has libtorch and the training; `arena` plays games and has no
   libtorch, so it starts faster and runs on every machine. A viewer of files needs neither. Perhaps a
   small third binary, or `arena`.
2. **Settled 2026-10-22:** launching from a page goes through the server's and client's
   `start`/`stop`/`status` (above), so the GUI never parents a run.
3. **Charts:** hand-drawn SVG in the page, or a small charting library compiled in.
4. **Remote machines:** the training machine's run seen from another machine is the LAN page's job. Whether the local GUI also
   lists the LAN's servers (reading their JSON) or stays strictly local.
5. **Port and conflicts:** a fixed port, or the next free one printed at start.
