# Orders with a counter, units streamed as PGN, SPSA as a server mode

Names superseded by chores.md (2026-10-22): an order is now a chore, and training runs and tunes
are jobs. The text below keeps the old names.

Status: design agreed with the developer on 2026-10-22, plan written the same day. Steps 1 to 3
built, tested and run on the Lichess machine the same day; steps 4 (SPSA) and 5 (docs) built and tested the
same day. To do before `done/`: a real tune over the server. In step 4 the server resolves the tuned player from its own files
(`RotorPlayer::resolve`, as `smithy install` does): an older network's champion must be local. Settled in the code: a
game unit is played to a depth with its player's own parameters, as self-play always was (the client
refuses any other); the dedup set lives in memory only, since a restarted server's new token turns
every earlier session away; the `.tsv` lines carry the identity for the record.
It follows http-transport.md and implements the rest of lan-only-servers.md, except its `GET /`
page. Where the two differ, this document supersedes lan-only-servers.md: self-play stays single
games, and SPSA is no longer deferred.

## Goal

- One kind of work for `arena client`: **orders**. Each order is a description of its units (games
  or pairs) and how many to play. The client plays its order and sends **each unit as soon as it
  ends**, as PGN.
- No jobs, no leases, no batches. The server counts the units that arrive, and tops up, stops or
  replaces a client's order in its reply to each unit.
- SPSA becomes a mode of the one server, `smithy server start --spsa`. Its batches, port, server file
  and `arena spsa --serve|--client` go.

## Decisions

1. **Two units.**
   - `game`: one game, the player against itself. Used by self-play, which stays single games: a
     random book opening, then MultiPV exploration (decided 2026-10-22: pairs add nothing there).
   - `pair`: one opening played with both colours, two players. Used by the round robin, the gate and
     SPSA.
2. **An order** is an id, a unit, a limit and its entries:
   - **Limit:** `depth <d>` or `clock <base+inc>`. The clock is written per session, so SPSA keeps
     scaling each client's clock to its speed.
   - **Entry:** one player (game) or two (pair, challenger first) and a count. Each player is a
     network (its hash, or `hand-crafted`) and its search parameters: `defaults`, or every value
     explicit (`name=value,...`). The server resolves a tuned player's `parameters.txt` into explicit
     values, so the client reads no file for them and the order records what was played.
   - **Tags:** server-only tags (`Challenger`, `Teacher`, `K`), which the client copies into the PGN
     untouched.
   - The client plays the entries in order, decrementing an entry's count as each unit starts.
     Self-play, a round-robin pairing and the gate have one entry with a count. SPSA has one entry per
     k, each with a count of 1 and its own θ + c_k Δ_k and θ − c_k Δ_k: one pair per Δ, as today.
3. **Endpoints**, replacing `/work` and `/results`; `/hello` and `/network/<hash>` stay:
   - `POST /order` with the session: `ORDER` with an order, `WAIT`, or `UNKNOWN`.
   - `POST /unit` with the session, a newline and one unit's PGN (one game or two). The reply is
     one of:

     | Reply | Effect on the client's order |
     |---|---|
     | `OK` | nothing |
     | `ADD` with entries | the entries are appended (`ADD` of the same entry is a top-up) |
     | `STOP` | every count to 0; units in flight finish and are sent |
     | `ORDER` with an order | replaces the current one; units in flight finish and are sent |
     | `UNKNOWN` | say hello again |

   - A client whose entries are all played asks `POST /order` again, `WAIT` meaning "ask again in a
     few seconds".
4. **A unit's identity** is (order, session, sequence number in the session), written as PGN tags
   `Order`, `Session`, `Seq` in each game. Order ids are `o<NNNNNN>`, from a counter the server
   persists in its run directory, so ids stay unique across restarts. The server drops a unit whose
   identity it already holds.
5. **The server counts arrivals only.** Its estimate of a session's units left is dealt − arrived −
   workers (the ones in flight). Below 2 × workers it replies `ADD` with 4 × workers more, while
   its mode still wants units. A unit lost with a client is never counted, so nothing needs to be
   leased or reissued. The exception is SPSA, whose k's are each dealt once: the k's a session holds
   go back to the pool, smallest first as today, when the session is dropped (a new hello from its
   machine, or 30 minutes without a request).
6. **Validation of every unit:** its PGN must parse, its tags must match an order dealt to that
   session, its players and limit must be the order's, and it must be one game or two. Within the
   64 MB body limit. A bad unit gets a 400 and is not recorded.
7. **Self-play games** land in today's segments, so `freeze` and training read them unchanged:
   - The server appends each game, in one write followed by a flush, to an open text file in the
     test or corpus directory, `<stamp>.pgn.open`.
   - It closes the file at 500,000 positions (plies + 1 per game, counted by the server from the
     moves): compressed to `<stamp>.pgn.zst` with its `.meta`, then the open file removed. The phase
     closes the open file when its target is met.
   - On resume, an open file is read, a truncated last game dropped, and appending continues.
   - Positions count toward a target as each game arrives, so the targets and `positions_in` keep
     their meaning, with the open file's positions added.
8. **Pairs' games are recorded beside their results:** `round-robin.pgn` and `gate.pgn` in the
   attempt's directory, appended in one write per pair. The server derives `r` (-2..=2) from the two
   `Result` tags and the colours. The `.tsv` lines gain the identity, so a resume rebuilds the dedup
   set from them. The client's `data/client/games.pgn` goes.
9. **Phase changes:** a self-play game is accepted while its generation's play phase runs; a pair
   while its pairing's round, gate or tune is open. Any other unit gets `OK`, is dropped, and the
   reply to the next unit is `ORDER` or `STOP`.
10. **SPSA as `smithy server start --spsa <player> --first|--fine <from>|--pairs <n>`:**
    - The method in `spsa.rs` is unchanged and moves to `rotor` (pure functions, so both binaries
      may reach it; only smithy needs it after the move).
    - The tune's state and folder stay where they are today, under `data/spsa/`, on the server
      machine; its pairs go to a `pairs.pgn` there instead of each client's `games-<machine>.pgn`.
    - Each order holds the next few k's (2 × workers of the client), each with its explicit θ ± c_k Δ_k
      at the θ of the moment. θ moves as each pair arrives; pairs played at an older θ are valid, as
      today's batches already allow.
    - The clock is scaled per client, from its pace, as `spsa_remote` does today.
    - `arena spsa --serve`, `arena spsa --client`, `spsa_remote.rs`, `SPSA_PORT` and
      `data/spsa/server.txt` go. A one-machine tune is a server and a client on `127.0.0.1`.
    - `stop` and `status` come with the server's control, as for `--forge`.
11. **SPSA's details (agreed 2026-10-22, before step 4):**
    - **Early stop:** an order carries `adjudicate`, true only for SPSA: its pairs end early by the
      tablebase, win and draw rules, as `arena spsa`'s always did; round robins and gates never.
    - **Clock scaling, SPSA only:** the server benches its machine with the tune's network once at
      the tune's start; each SPSA order carries that reference speed. A client benches each network
      once, loaded and pinned as it plays, scales the clock itself (`clock_factor`, with its clamps
      and dead band), and writes the clock it played in each game's `TimeControl` tag; the server
      refuses a clock outside the clamps.
    - **No local tune:** `arena spsa` goes whole. One machine is a server and a client on
      `127.0.0.1`.
    - **Resolving the player** (its network, `parameters.txt`, tune folder, a `--fine` start) moves
      from arena's `match_player` into `rotor::pipeline::players`, where both binaries reach it.
12. **One server at a time** on a machine: SPSA and forge share `SERVER_PORT`, so a second one fails
    to start, as it does today for two forge servers.

## Implementation

1. **Protocol** (`rotor::pipeline::protocol`): `Order`, `Entry`, `Player`, `Parameters`
   (`Defaults` or explicit), `Limit`, `Unit`; the replies; text round trips and refusals tested. `Job`,
   `GamesResult`, `PairsResult` and `results_body` go.
2. **Client** (`arena client`): the workers play the order's entries, a game through
   `self_play::play_game_pgn`, a pair through `runner::play_games` with explicit parameters per
   player; each unit is sent as it ends, its reply applied to the shared order. The PGN tags.
   Tested against a fake server on `127.0.0.1:0`.
3. **Server, forge modes:** `Work`, `Out`, leases and `next_job` give way to per-session orders and
   the top-up rule; the self-play appender with its close and resume; `round-robin.pgn`, `gate.pgn`,
   the identity in the `.tsv`; the persisted order counter; validation. Tests: dedup, top-up, a
   truncated open file resumed, a phase change dropping old units.
4. **SPSA mode:** `spsa.rs` moved; `spsa_command.rs`'s tune state and resume moved to smithy as a
   mode; the k's dealt per session and returned when it is dropped; per-client clocks; `arena spsa`'s
   serve and client removed, and `lan.rs` reduced to the helpers still used.
5. **Docs:** DEV.md ("Running a training run", "Tuning search parameters", the SPSA firewall rule
   gone), README if it names `arena spsa`, TODO.md, lan-only-servers.md pointing here.

## Open questions

None. To check in the code at step 4: whether `arena spsa` keeps any local-only use worth a
command, or goes whole.
