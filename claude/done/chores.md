# Jobs and chores

Status: names agreed with the developer on 2026-10-22, plan approved and implemented the same
day, to build and test. A rename on top of
orders.md and status-page.md.

## Goal

Two plain words for the two levels of work, from largest to smallest:

- **job**: what a server does, one at a time: a forge run (`--forge`) or a tune (`--spsa`).
- **chore**: what a job deals one client, today's **order**: an id, a unit kind, a limit, entries,
  topped up (`ADD`) or ended (`STOP`).
- **unit**: one game or pair inside a chore, unchanged.

"Order" is odd for a piece of work, and collides with move ordering and sort order across the code.

## Decisions

1. **Order becomes chore everywhere it means the dealt work**: types (`Order` to `Chore`, the
   server's `Dealt` stays), fields, functions, messages, tests, the status page's column.
   "order" meaning a sort or move order is untouched.
2. **Protocol**: `/order` becomes `/chore`, the `ORDER` reply `CHORE`. Server and clients change
   together: every machine is rebuilt before the next job, an old client meets a 404 and says so.
3. **The PGN tag** `Order` becomes `Chore` on new games. Old segments keep `Order`: nothing reads
   the tag from disk (it serves only to drop a unit arriving twice, on arrival), so no fallback.
4. **Ids** become `c000000` instead of `o000000`.
5. **The counter file** `orders.txt` becomes `chores.txt`; a job with only `orders.txt` reads it
   once and goes on in `chores.txt`, so a resumed run never reuses an id.
6. **Job, in prose**: docs and comments say job where they mean a forge run or a tune in general.
   The code keeps `run` for a forge run (`data/gen/<run>/`, `run_generations`) and `tune` for a
   tune; the status page's `run` field becomes `job`.
7. **Docs**: DEV.md, orders.md (a note at its top: superseded in its names by chores.md),
   status-page.md, lan-only-servers.md, TODO.md. The old "LAN jobs" and "its job handed out again"
   wording becomes chore.

## Implementation

1. `protocol.rs`: `Order` to `Chore`, `ORDER_PATH`, `ORDER_TAG`, the reply word, parsing and text.
2. `server.rs`: dealing, the id prefix, `chores.txt` with the `orders.txt` fallback, status text,
   snapshot (`job`, a client's `chore`), tests.
3. `client.rs` and `units.rs`: the same names, messages, tests.
4. `server_page.html`: the column and the field names.
5. Docs as above.
6. Build, test, one short tune on the Lichess machine to see the new names end to end.

## Open questions

None.
