# Sources and forges: lc0, pesto, lc0-psq, pesto-psq

**Status: design, 2026-10-22, from the developer's redesign of `configs/` after
old-networks-cleanup.md, one question at a time; confirmed, implemented the same day, every test
passed; DEV.md done. Beyond the plan: Convert runs as a child process (`smithy lc0-convert`, in no
usage line), stopped by `taskkill /T`; PeSTO's Play moves its finished segments to Drive at its start,
stop and end; a run records its generation 1 corpus in `corpus.txt`; a forge's name must match its
`labels:` source. The the Lichess machine and the training machine checks (step 8) to do.**

## The idea (the developer)

`configs/` holds, beside `smoke.yaml` (left out for now, unchanged), four configs of two kinds:

- **Sources**, each making one corpus, once, for every forge named after it:
  - `lc0.yaml`: Lc0's training tars converted, for every `lc0-` config.
  - `pesto.yaml`: the PeSTO evaluation's self-play, for every `pesto-` config.
- **Forges**, `<source>-<architecture>`: `lc0-psq`, `pesto-psq` (was `self-psq`). Generation 1 trains
  on its source's corpus; from generation 2 on, each run's own self-play, taught by its previous
  winner, as today.

The prefix names the source, the suffix the architecture: a future `pesto-threats` trains its
generation 1 on the same games as `pesto-psq`, so architectures are compared on equal data.

## Decisions (the developer, 2026-10-22)

1. **The prefix is the source:** `lc0` serves the `lc0-` configs, `pesto` the `pesto-` ones.
2. **Generation 1 has one attempt, on both routes:** its first attempt should pass its gate against
   PeSTO; a failure says the network is wrong, and the run stops (as the Lc0 route does today).
3. **PeSTO's corpus is its own job, from the `pesto` box:** Play, resumable, to `pesto.yaml`'s size.
   A `pesto-` run refuses to start until the corpus is complete, as an `lc0-` run does with Lc0's.
4. **Everything from the page:** `smithy lc0` goes; the `lc0` box has Convert, showing the tars
   converted and the bytes against the cap, resumable.
5. **Two jobs at most:** an Lc0 conversion (no arenas, no GPU) may run beside one arena job (a
   forge, a tune, or PeSTO's Play), each box with its own Stop. An `lc0-` forge still refuses to
   start until Lc0's corpus is complete.
6. **The source decides generation 1's data:** generation 1 trains on the whole corpus, `lc0.yaml`'s
   cap and `pesto.yaml`'s size the only knobs. A forge config says nothing about generation 1's data.
   Each run records the corpus size its generation 1 trained on.

## Defaults (Claude's, not objected to)

- The main page's boxes: `smoke`, `lc0`, `pesto`, `lc0-psq`, `pesto-psq`, then any other by name.
- `pesto.yaml`: the corpus's training and test sizes, and `pesto`'s `tune:`. The search depth stays
  the constant depth 9.
- PeSTO's corpus in `data/pesto/` with `train/` and `test/`, as Lc0's in `data/lc0/`; its frozen test
  set shared by every `pesto-` run, as Lc0's is.
- `smoke` unchanged: it plays its own generation 1 inside the run.
- `self-psq` becomes `pesto-psq`; its runs start again from scratch.

7. **PeSTO's corpus lives on the remote (Drive), as Lc0's:** both corpora side by side under the same
   root, read by the same code.

8. **The source fixes generation 1's test set too:** `lc0.yaml` and `pesto.yaml` each size their
   frozen test set, and every run's generation 1 is judged on all of it, so the test losses of one
   source's runs compare directly. A forge's `training_positions` and `test_positions` size
   generation 2 on only.

9. **Convert's Stop abandons the tar in flight,** within seconds, as Ctrl+C does today: that tar is
   redone from its download next time. The page shows the tars converted and the bytes against the
   cap; `configs/lc0.yaml`'s keys gain the frozen test set's size (decision 8), the rest unchanged.

## The two-job smithy (Claude's, settled)

- Two slots: the **arena slot** (forge, tune, PeSTO's Play, as the one job today) and the
  **conversion slot** (Lc0's Convert). `/control/job` starts a job in its slot, refused with 409
  when that slot is busy; each slot has its own stop flag, and `/control/job/stop` takes the slot's
  config, so a box's Stop stops its own job.
- `smithy stop` and Ctrl+C stop both, then the smithy.
- `Snapshot` keeps the arena job as today and gains the conversion's own part (running, stopping,
  tars and bytes, how it ended); a box is running when either slot's job is its config's.
- The conversion's lines go to the smithy's terminal, prefixed, beside the arena job's.

## Plan

1. **Configs:** `self-psq.yaml` becomes `pesto-psq.yaml` (`git mv`, the developer). `pesto.yaml`
   gains `training_positions` and `test_positions`; `lc0.yaml` gains `test_positions`. The forge
   configs' `training_positions` and `test_positions` comment as generation 2 on.
2. **A source in code:** `lc0` and `pesto` as the two sources, a forge's source from its name's prefix
   (`lc0-`, `pesto-`; `smoke` none, as today). `labels: lc0` and the route's `lc0` flag replaced by
   the source; a forge config with neither prefix nor `smoke` refused.
3. **PeSTO's corpus:** a `pesto` Play job dealing PeSTO's self-play to the arenas into
   `data/pesto/test` and `train` on the remote, to `pesto.yaml`'s sizes, resumable; its frozen test set
   in `data/pesto/frozen`; a `complete.txt` as Lc0's.
4. **Generation 1 from a source:** a `pesto-` run's generation 1 skips Play and trains on PeSTO's
   corpus, as an `lc0-` run's on Lc0's; both one attempt (decision 2); both judged on the source's
   whole frozen set (decision 8); the run records the corpus size it trained on (decision 6). A run
   refuses to start while its source is incomplete.
5. **Lc0's Convert as a job:** `smithy lc0`'s command goes; its loop runs in the conversion slot,
   stoppable between steps (decision 9), reporting to the page.
6. **Two slots** (above), and the page: the `lc0` box with Convert and its progress, the `pesto` box
   with Play and Tune, the order `smoke`, `lc0`, `pesto`, `lc0-psq`, `pesto-psq`.
7. **Docs:** DEV.md (sources, the page, no `smithy lc0`), forge-configs.md and configs-with-tunes.md
   noted as revised, TODO.md.
8. Build, test; on the Lichess machine: PeSTO's Play at a small size, a `pesto-psq`-shaped smoke of generation 1
   from it; on the training machine: Convert from the page, then `lc0-psq`.
