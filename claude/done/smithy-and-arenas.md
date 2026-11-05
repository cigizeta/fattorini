# The smithy and the arenas

Status: design discussed with the developer on 2026-10-22; plan approved the same day. Step 1 (the
names, unit to task, `smithy.yaml`, `start|stop|status`, the `WAIT` reason) implemented the same day,
built, tested, run on the Lichess machine and committed. Step 2 (jobs from the page) implemented the same day,
to build and test. Settled while implementing: each job starts from a fresh state with a new token,
so the arenas say hello again and a task of an earlier job is turned away; the page's job controls
carry a control token that only the loopback page's state hands out, so another site open in the
same browser cannot start or stop a job; a job runs on its own thread, a panic in it ending the job,
not the smithy. Built, tested and a tune run from the page on the Lichess machine the same day. A training run,
as a job, is called a **forge** (the developer, 2026-10-22); "run" stays for its config and its
directory, `data/gen/<run>/`. The one-line refusal of another tune and the idle reason on the page
built, tested and committed the same day. To `done/` after a real forge started from the page. The first step of
browser-gui.md, on top of chores.md and status-page.md.

## Goal

The smithy starts with no job and stays up; jobs are started and stopped from its page, one at a
time, the next one without a restart. The command line shrinks to `start|stop|status`, and the
"server" and "client" names go: they are the smithy and the arenas.

## Terms

- **The smithy**: one `smithy` process, on the machine `data/smithy.yaml` names. It holds at most
  one job and deals it as chores.
- **The arenas**: one `arena` process per machine, the smithy's machine included. Each does chores
  with its **workers**, one thread per task in play.
- **job > chore > task**: a task is one game or one pair, what chores.md still calls a unit
  (decision 6).
- HTTP's own words stay where they mean HTTP (listener, request, reply).

## Decisions

1. **Commands:** `smithy start|stop|status` and `arena start|stop|status`, nothing else for this
   work. `--forge`, `--spsa`, `server` and `client` go.
   - `smithy start`: binds the port, serves its page, waits for a job. Only a stop ends it.
   - `smithy stop`: stops the job at its next safe point, if one runs, then the smithy.
   - `arena start|stop|status`: as `client` today.
   - Ctrl+C in the smithy's terminal is `smithy stop`; a second Ctrl+C ends it at once.
2. **Jobs from the page alone**, and only from the page opened on `127.0.0.1`, on the smithy's
   machine: it shows the buttons, the LAN's page stays read-only. A start or stop goes through the
   control, which answers on the loopback alone, as today.
3. **One job at a time, no queue**: a start while one runs is refused. A job that ends, fails or is
   stopped leaves its result on the page until the next starts. Arenas stay joined between jobs,
   dealt nothing.
4. **Starting a job:**
   - **Forge:** one of `configs/gen-*.yaml` and `configs/lc0-*.yaml`; one with a `state.txt` is
     offered as a resume.
   - **SPSA:** a player; first, fine from another player, or a number of pairs; depth or clock. A
     stopped tune is offered as a resume.
   - What the command line refuses today (a transformer that cannot self-play, a tune's spec
     changed, a player not found) comes back as an error in the form: a job's failure never ends
     the smithy.
   - **Stop job**: at its next safe point; the smithy stays up.
5. **Names in files:** `data/server.yaml` becomes `data/smithy.yaml`, still `machine: <name>`, read
   once from `server.yaml` when `smithy.yaml` is missing, with a line saying to rename it. The page
   says smithy and arenas.
6. **Unit becomes task**, in the same rebuild: `/unit` to `/task`; the protocol's `Unit` enum to
   `TaskKind { Game, Pair }`, its text field `unit` to `kind`; `units.rs` to `tasks.rs`;
   `UNITS_PER_WORKER` to `TASKS_PER_WORKER`; messages, comments, the page and DEV.md. The reply
   words stay. `progress::Unit` (a unit of measure) and "unit" in its ordinary sense stay. Nothing
   on disk names units, so no fallback.
7. **Later, not here:** the other commands of `smithy` and `arena` (lc0, check, calibrate,
   correlate, install; duel, match, sprt, replay, margin-search), one by one; running detached with
   log files; charts; starting a job from another machine.

## Implementation

1. **A job is a value the smithy holds**: `Job::Forge { config }` or `Job::Tune { args }`, run on
   its own thread, with its stop flag. Each start makes a fresh per-job state (work, view, tune,
   chore counter, pairs, finished); sessions and the listeners outlive it.
2. **No exit on a job's path**: every `fail()` reached from a job start or run returns an error to
   the job's thread, which records it as the job's result.
3. **The control:** `POST /control/job` with the job as JSON, `POST /control/job/stop`,
   `POST /control/stop`, `GET /control/status`. The page's buttons call them; the loopback page
   learns it may act from `/state.json` (`control: true` on the loopback listener alone).
4. **The page:** a job form when idle (the lists from `GET /control/choices`: configs, players,
   their resumable state), the job's view and a stop button while one runs, the last result after.
5. **Renames:** `smithy server` to `smithy`, `arena client` to `arena`, `data/server.yaml` to
   `data/smithy.yaml` with the fallback, `server.rs` to `smithy.rs` and `client.rs` to `chores.rs`
   (open question 1), `bin/smithy` and `bin/arena` skipping the build for `stop|status` as today;
   unit to task (decision 6).
6. **Docs:** DEV.md's "Running a training run" and "Tuning search parameters" become one section
   ("Jobs"); examples/server.yaml becomes examples/smithy.yaml; lan-only-servers.md,
   browser-gui.md, TODO.md.
7. Build, test, then on the Lichess machine: `smithy start`, a short tune from the page, stopped, resumed,
   then a second job without a restart.

## Settled

1. The source files: `server.rs` becomes `smithy.rs`, `client.rs` becomes `chores.rs`.
2. An arena dealt nothing prints one line per wait, as today, its reason carried by the `WAIT`
   reply: `waiting: the smithy has no job`, or `waiting: <job> is training` (or between phases).

## The training table matches the terminal (approved 2026-10-22)

The page's training table showed step, learning rate, train and test loss from each lambda's last
`history.tsv` row; the terminal's round line shows round, rate, train, test, best @ round, stage
and patience. The page gets the terminal's columns:

| lambda | round | learning rate | train loss | test loss | best | stage | patience |
|---|---|---|---|---|---|---|---|
| 0.91 | 3 | 3e-4 | 0.179318 | 0.297187 | 0.295519 @ 3 | 3 of 5 | 2 of 4 |

1. **One source, revised while implementing:** the terminal already replays a finished lambda's
   stages from its `history.tsv` alone (`stage_after`). The page does the same,
   `lockstep::latest_round` with the forge's rate schedule, so both read the same file through the
   same code, no plumbing out of the training loop, and the table survives a restart.
2. **Finished lambdas** greyed, "finished" as their stage, patience blank; rows sorted by best
   loss, as on the terminal.
3. **It does not disappear (the developer, 2026-10-22):** the job's latest training is its own
   section, under the round robin and the gate, titled with its generation and attempt, until the
   next training replaces it or another job starts.
4. **An ended job is acknowledged (the developer, 2026-10-22):** when a forge or a tune ends, the
   controls show "Job ended" and a centred "Start new job" button, which opens the form. A smithy
   that has had no job shows the form at once. Page only.
5. Build, test, then on the Lichess machine a smoke forge, the page beside the terminal.

## Every phase stays on the page, across restarts (approved 2026-10-22)

The developer: a forge's page shows all its phases, play, training, round robin and gate, not the
running one alone; and after a restart, the last job as it ended.

1. **From the files, like the training table:** the current attempt's sections are rebuilt at each
   poll from the run's files: self-play's test and training positions from the segments' `.meta`
   files against the config's sizes; training from the lambdas' `history.tsv`; the round robin
   from `round-robin.tsv`, its pairings fixed by the lambda grid (i < j); the gate from `gate.tsv`
   and the config's SPRT. A section with no file yet is not shown; an Lc0 forge has no play.
2. **The running phase live:** while self-play, the round robin or the gate runs, its section is
   the live one (counts in memory, not yet in sealed segments), in its place among the others.
3. **The last job, across restarts:** the smithy writes the words of each job it starts to
   `data/smithy-last.txt` (local, ignored by git), a tune's without its length, so that reopening
   it resumes rather than starts. On start it shows that job, ended, under "Job ended": a forge's
   sections from its files, a tune's parameter table from its folder (nothing if it cannot be
   opened). The status line says "last job, before this smithy started".
4. The `Training` kept in memory goes: the forge's directories and schedule are enough.
5. **A tune too (the developer, 2026-10-22):** its parameter table already stayed after it ended,
   and comes back after a restart (3). Its bench, the reference speed under a clock, is shown in its
   section; it is not kept on disk, so a restarted smithy's page has no bench line.
6. **The form opens on the last job (2026-10-22):** its config selected for a forge, tune and its
   player for a tune; before, the first config in the list, which started the wrong forge once.

## Open questions

None.
