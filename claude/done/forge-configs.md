# A configuration-driven smithy

**Status: plan, 2026-10-22, from the developer's redesign after the first runs check on the Lichess machine;
implementing ("try, then I will judge"). Step 1 done (the developer archived `configs/`, Claude wrote
the new one), step 2 done (tests passed), step 3 done (tests passed; `on_lc0(generation)`
picks Lc0's corpus and test set for generation 1 alone; a failed generation 1 gate writes
`failed_gates` at its maximum, so every "stopped" check reads it; `Done` is left to transformers;
an Lc0 run's generations are archived like any other), step 4 done (tests passed; below, "The page as
built"), step 5 done (DEV.md, runs.md's revision note, TODO.md); step 6, the Lichess machine check, to do, now with configs-with-tunes.md's names (`smoke`, `pesto`, Tune
without a tune config). Settled in step 2: the forge config drops
`label:` and `selection:` (the grid and the forge's gate replace them, and nothing read
`selection:`); `training_positions`, `gate_elo_d` and `max_failed_gates` are required of every forge
config, Lc0's included; a run frozen as two files today (runs.md) is not read: only smoke runs were.** Revises runs.md (its runs and frozen configs stay; its form goes) and
self-training-simpler-attempt.md's Lc0 route, which the forge had drifted from.

## Decisions (the developer, 2026-10-22, one question at a time)

1. **The page is driven by the configs:** every forge config, every run of it, a new run of it;
   inspecting a run never resumes it.
2. **Tunes hang off forge runs:** every generation of a run gets its own tune, once the run is
   finished, and only a generation whose challenger passed its gate. One tune a network, as
   before. `hand-crafted` is tunable, always, in its own section.
3. **An Lc0 run has generations:** generation 1 trains on Lc0's labels and is gated against PeSTO;
   from generation 2 on, self-play, as the PeSTO route (self-training-simpler-attempt.md, "Generation
   1 from Lc0 labels", which the forge had drifted from: today an Lc0 run ends at its round robin).
4. **A few configs, for now:** `smoke-psq`, `self-psq`, `lc0-psq`. More come back as more complex
   networks are tested.
5. **One file a forge config:** its forge keys and its network's sections together; no separate
   network config.
6. **The older networks** (amanda to dorothy, `lc0-*-0001`) may not be recoverable; training may
   restart from scratch, so nothing in the design keeps them alive.
7. **`configs/` is archived whole** into `archived/configs/` at the repository's root, and a new
   `configs/` is made.

## The new configs/

| File | From | What |
|---|---|---|
| `smoke-psq.yaml` | `gen-psq-smoke` + `self-psq-smoke` | PeSTO route, minutes |
| `self-psq.yaml` | `gen-psq` + `self-psq-direct` | PeSTO route, the real one |
| `lc0-psq.yaml` | `lc0-psq-direct-fine` + `lc0-psq-direct` | Lc0 generation 1, then self-play |
| `lc0.yaml` | as is | `smithy lc0`'s conversion, not a forge |
| `tune-first.yaml`, `tune-smoke.yaml` | as is | tune configs |

A forge config, one file:

```yaml
# forge keys
grid_center: 0.95
training_positions: 100663296
test_positions: 4194304
round_robin_pairs: 400
gate_elo_d: 35
max_failed_gates: 3
# the network, as a network config held it
labels: self
architecture: {...}
training: {...}
```

A run freezes this one file as `config.yaml`.

## Steps

1. **Archive** (the developer): `configs/` moved to `archived/configs/`; then the new `configs/`
   written (Claude). Tests and the bench that read archived configs point there, those about
   retired experiments' consistency deleted.
2. **One file a forge config:** the loader splits the file's keys, forge keys strictly into the
   forge config, the rest into the network config; `network:` is gone. Runs freeze the one file;
   a run frozen as two files (runs.md) is not read.
3. **The Lc0 route as designed:** an Lc0 config takes `training_positions`, `gate_elo_d` and
   `max_failed_gates`; generation 1 trains on Lc0's labels, its winner gated against
   `hand-crafted`, one attempt, a failure stopping the run with a message; generation 2 on,
   self-play taught by generation 1's winner.
4. **The page:** a "running now" panel with Stop; every forge config with its runs, each with View
   (its sections, read-only, from its files), Resume (unfinished, nothing running), and, when the
   run is finished, Tune for each generation that passed its gate (a tune config to pick; the tune's
   state and View under it once it exists); New run per config; `hand-crafted` with its Tune at
   the end. Read-only on the LAN. The form, "Job ended" and the typed network go.
5. **Docs:** DEV.md, runs.md's revision, TODO.md.
6. Build, test, then on the Lichess machine: `smoke-psq` new run to its end, View it, Tune a passed generation
   with `tune-smoke`, a second Tune refused; `hand-crafted` tuned or refused.

## The page as built (step 4)

- **A finished run's View:** a generation and attempt selector, on the latest by default (the
  developer, 2026-10-22: tunes are per generation, so the generation about to be tuned is seen).
- `GET /catalog.json`, every listener, read-only: each forge config, each run of it (where its state
  stands, whether it has ended), each generation with its attempts on disk, whether it passed its
  gate, and its winner's tune progress; hand-crafted's tune; the tune configs. Replaces
  `/control/choices`.
- `GET /view/<run>/<generation>/<attempt>`, every listener: that attempt's sections from its files
  (an earlier attempt's lambdas from its `lambda-*` directories), the running phase from memory
  when it is the running forge's own attempt.
- The page: the running job with Stop; the job's live sections and the arenas as before; then a
  section per config, its runs each with View and Resume (not ended, nothing running), New run;
  under an ended run, Tune with a tune config for each passed generation; hand-crafted's Tune; the
  View section, under the configs. Starting buttons only with the control token, disabled while a
  job runs.
- A tune is refused by the smithy too (`check_tunable`) unless hand-crafted, or the winner of a
  passed generation of an ended run.
- Gone: the form, "Job ended", the typed network, `Shared::words`. `data/smithy-last.txt` stays, so a
  smithy just started still shows the last job's sections.
