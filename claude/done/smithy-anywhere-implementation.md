# One smithy, on any machine: implementation plan

**Status:** proposed 2026-10-29, to discuss. The design: smithy-anywhere.md (decisions numbered
as there). Decision 10 (`data/networks`) and decision 5's refusal of a missing `state.txt` are done.

## Order

Each step leaves a working smithy, tested and committed before the next. Steps 1 to 6 touch no
run's data and may land while `-quiet` trains, its smithy restarted on them only between phases or
at a stop. Step 7 changes the buckets: only after `-quiet` has ended.

## Survey (step 1, 2026-10-29)

- **The process-wide scale** (`layout::enter_smoke`, a `OnceLock`) is set by `start` and by the
  Convert's and the books' children, given `--scale` by `run_in_child`. What it changes:
  - **a forge:** `load_config` calls `scale_forge` (batch sizes, `steps_per_round`, training and
    test positions by `scale::volume`, the round robin's pairs by `scale::games`); `search_depth`
    (`scale::depth`) for self-play, tournament chores and two limits besides;
  - **a tune:** its pairs (`tune.rs`, `scaled_pairs`) and its clock (`scale::control`);
  - **sources:** the Convert's `cap_bytes`, test games, segment and test positions
    (`lc0::read_config`); PeSTO's training and test positions (`pesto.rs`);
  - **books:** nothing but the tree they are written in;
  - **paths:** every `layout::local` and `canonical` (runs, tests, buckets, installs, the PeSTO
    and Lc0 roots, the mover, tunes).
- **`start`:** `machine_name` reads the local `data/machine.yaml`; a real smithy binds its LAN
  address and claims `production_arenas`, which reads `data/machines.yaml` on Drive (`smithy:` and
  `arenas:`, `protocol.rs`) and refuses any machine but `smithy:`, its only reader; a dev one binds
  loopback and claims `127.0.0.1` alone.
- **The page** (`page.html`): `state.dev` hides the Install box, notes that a dev smithy installs
  nothing, and titles the page; the server refuses Install too. New run and Resume post
  `--forge <config> <new|n>` to `control/job`, run by `run_job`, `job`, `forge`.
- **libtorch:** the device is picked only inside training, `Device::cuda_if_available()`; nothing
  probes CUDA at start or tells the page. `build.rs` forces `torch_cuda.dll` to link, as without it
  CUDA silently falls back to the CPU.
- **Shared data:** source buckets `data/buckets/<source><filter>`, self-play buckets per run;
  a sourced generation's frozen set on Drive, `<source root>/frozen<filter>`, sized by the
  source's (scaled) `test_positions`; corpora; books on Drive, never scaled; `data/tests`.
- **`GenState`:** generation, attempt, phase, grid centre, failed gates; no counter or time.
- **The last job:** `data/smithy-last.txt`, local, the job's words; the page shows that run or
  tune when it opens.

**What follows for the steps:**
- **Sources are never scaled** once `--dev` goes: the Convert and PeSTO's Play always full-size, a
  scaled run reading a share of the full corpus; books stay unscaled. So `scale_forge`,
  `search_depth` and the tune's scaling become functions of the run's or tune's own scale, and
  `lc0::read_config`'s and `pesto.rs`' scaling go.
- **A scaled tune, for debugging only (the developer, 2026-10-29):** a scaled network always fails
  its gate, so tuning one has no sense but to debug the tune itself. A scaled network is tuned only
  by a scaled tune, at its run's scale, and a scaled tune is offered on a scaled network only; its
  parameters, like the network, never installed nor used elsewhere. The tune's scaling
  (`scaled_pairs`, `scale::control`) reads the network's run's scale instead of the process's.
- **A scaled run's frozen set** is drawn from the full source's test games at its scaled size:
  `<source root>/frozen<filter>-s<s>` on Drive.
- **`machines.yaml`' `smithy:`** loses its meaning; `arenas:` stays. `data/machine.yaml` stays,
  for the LAN address.
- **The CUDA probe** at start: `tch::Cuda::is_available()`, in the status as `gpu`, read by the
  page and by `forge`'s refusal.

## Steps

1. **Survey, listed here before any change:**
   - every use of `--dev` and the smoke tree: `layout.rs` (`enter_smoke`, `smoke_scale`, `root`,
     `local`, `canonical`), `scale::FLAG` passed to the Convert's and the books' children, the page's
     `dev` field, the install's refusal, `remember_job` and `last_job`, `scale_forge`, `search_depth`,
     `tune.rs`' scaled pairs and control, `pesto.rs`, `lc0::read_config`;
   - `production_arenas` and `machine_name`, and what `data/machines.yaml` says;
   - where libtorch is first touched, for the CUDA check;
   - the page's New run and Resume, and what a job tells the page;
   - which paths name a scale today (`data/smoke/s<s>/...`) and which shared data a scaled run
     would write: buckets, frozen sets, books.
2. **Written 2026-10-29, to build and test:** `has_gpu` (`tch::Cuda::is_available`, once),
   `NO_GPU` logged at start and refused by `forge`, `gpu` in the page's state; the page disables
   New run and Resume with the reason as a tooltip and a line atop Runs. No unit test: the probe
   is the machine's; checked by hand on a machine without CUDA.
   **The GPU check (decision 2):** CUDA probed once at start (libtorch's `Cuda::is_available`),
   kept in the status the page reads; a forge's New run and Resume greyed out with the reason
   without it, and refused by the control too, so no request bypasses the page. Tests: the
   refusal; the page's text.
3. **Written 2026-10-29, to build and test:** `GenConfig::scale`, appended to the run's own
   `config.yaml` by `freeze_run_config` and refused in `configs/`; `scale_forge` applied from it
   (or from `--dev`'s until step 5); `GenConfig::depth`, carried in `Shared::forge_depth` to every
   chore and view; `--forge <config> new <s>`, a scale refused for an existing run; the page's
   New scaled run with its `1/<s>` field, and "scaled 1/<s>" on each scaled run. **Simpler than
   below:** a scaled run shares the full runs' buckets (a function of the corpus and the filter
   alone, its scaled steps reading a share of them) and the source's frozen set, reading a seeded
   subset of it (`TestSet::frozen_rows` against `rows`), so nothing is suffixed `-s<s>`.
   **Planned:** the scale moves from `layout`'s process-wide
   `OnceLock` into the run's config (`run_config.rs`), frozen at New run, chosen on the page (none,
   or `1/<s>`). `scale_forge`, `search_depth` and the round robin's pairs read the run's scale. A
   scaled run's buckets and frozen sets are suffixed (`-s<s>`); everything else of it lives with the
   full-size runs, numbered with them and mirrored and moved as they are. The page marks it
   ("scaled 1/<s>") wherever it names the run or one of its networks. Tests: a scaled run's config
   round trip; its directories.
4. **Written 2026-10-29, to build and test:** `network_scale` and `unscaled` refuse a scaled run's
   network as an install (`ask_install`) or a test's side (`test_job`); the catalog's test and
   install list leaves scaled runs out; `TuneArgs::scale`, set by `tune_args` from the run's config,
   scales the tune's pairs and control. A run known only on Drive (its local directory gone) reads
   as full, its config unreadable here: acceptable until step 6's copy down.
   **Planned:** refused as an install or as a side of a
   test, by the control as well as left off the page's lists; tuned only by a scaled tune at their
   run's scale, for debugging (survey); their own round robin and gate unchanged. Tests: each
   refusal; a scaled tune's pairs and clock.
5. **Written 2026-10-29, to build and test:** `layout.rs` out of the module tree (the file itself
   for the developer to delete), every `layout::local`, `canonical` and `smoke_scale` replaced by
   the plain path or removed; `start` binds the LAN and claims `machines.yaml`' arenas on any
   machine (`arenas_to_claim`; `smithy:` kept in the file, no longer read); `--scale` to the Convert
   and books children gone, Lc0's and PeSTO's configs never scaled; the page's dev title, banner and
   Install hiding gone; DEV.md's "Scaled runs"; `examples/machines.yaml`. `remote::set_root` kept,
   unused by the smithy now.
   **Planned:** the smoke tree, its stand-in Drive, `scale::FLAG` to children
   (the scaled Convert goes; whether books keep a scale is the survey's), the loopback-only and
   own-arena-only start, and `production_arenas`' refusal of other machines. The install's dev
   refusal goes, replaced by step 4's. DEV.md's smoke-test section rewritten around scaled runs.
   `data/smoke/` deleted by the developer.
6. **Split in four (2026-10-29):** 6a the lease module and a forge run's lease; 6b the page's
   Running elsewhere and Take over; 6c the resume from Drive and the copies up; 6d the leases of a
   tune, a Convert and a source's first freeze.
   **6a written 2026-10-29, to build and test:** `lease.rs`, `lease.txt` in the run's remote
   directory (token, machine id, taken, heartbeat), the machine's id random in
   `data/smithy-id.txt`, never its name; a lease left by this machine's earlier boot taken over at
   once, a machine running one smithy; `forge` takes it before writing anything, waiting about a
   minute for the read-back, and a lease lost raises the job's stop. Tests on a temporary
   directory with short timings.
   **6a done 2026-10-29** (tests and clippy passing, checked by hand on a scaled run). **6b written
   2026-10-29, to build and test:** `RunEntry::lease` (`lease::holder`), the page's note for a live
   lease in place of Resume, Take over for a stale one behind a confirmation, the job's words
   `--forge <config> <n> take-over`.
   **6b done 2026-10-29.** **6c written 2026-10-29, to build and test:** `tree.rs` (`copy`, a tree
   copied one way, new or changed files, or all when forced; `in_open_generation`); `state.txt`
   gains `boundary`, counted by `write_state`; `forge` takes a run known on the remote alone, takes
   the lease first, then `sync_down` (the remote's top files and open generation, forced, when its
   boundary is past this machine's or this machine has none), and a missing state only then
   refused; `Run::copy_up` at the release and every 30 minutes of training through
   `train_in_lockstep`'s new `after_round`, between rounds. **Not yet:** self-play segments copied
   as each closes (a crash in Play loses the generation's games since its last boundary); left for
   later unless wanted.
   **6c done 2026-10-29.** **6d written 2026-10-29, to build and test:** leases on a tune (its
   remote folder; `Tune::open` fetches the remote's copy when it holds more pairs), PeSTO's play
   (`data/pesto`), the Convert (`data/lc0`, taken by the smithy around its child) and a source's
   frozen set (in its directory, only when something is to be written). Not on the page: a live or
   stale lease refuses these jobs with its message, a stale one removed by hand; a Convert's or a
   freeze's lost lease is only logged.
   **6d done 2026-10-29.**
   **Planned:**
   - a `lease` module: take (write, wait for the sync, read back), heartbeat (a thread, about a
     minute), release, and a reader the page uses (live, stale with its age, none); the timings
     constants, tuned in step 8;
   - leases on a forge run, a tune, a Convert into a corpus, and the first freezing of a source's
     test set;
   - the page: Resume, Running elsewhere, or Take over with a confirmation;
   - a forge's resume copies the run down from Drive when the local directory is missing or older
     (by `state.txt`'s content and time), as `Tune::open` does;
   - on release, the run's living files copied up (training state, histories, open segments and
     games); every 30 minutes, the training state; each self-play segment as it closes; a
     generation's frozen set once made.
   Tests: the lease's states and its back-off on another token, with a local directory for a
   remote; the copy down and up; a resume on a second tree landing on the same round.
7. **Reproducible buckets (decisions 6 and 7), after `-quiet` ends:**
   - each record carries its identity, its segment's name (not its path, which a rename changes),
     the game's index in it and the ply, and is scattered by a hash of it;
   - a bucket's records sorted by their bytes as it loads, before the epoch's permutation;
   - `buckets.tsv` gains `scatter\t2`; a state without it read as the old scatter while a run on it
     still trains, then refused once `-quiet` has ended and its code is removed;
   Tests: two scatters of the same files with different stream orders give identical buckets and
   identical epochs.
8. **Checks:** `cargo test`, `cargo clippy`; a scaled run started on one machine, stopped, resumed
   on another, its step count and losses continuing; a Resume refused while the lease is live; a
   machine without CUDA showing the reason; the lease timings measured against Drive's sync.
9. **Done 2026-10-29.** **Docs:** smithy-anywhere.md's results, DEV.md, TODO.md; discovery-and-heartbeats.md and the
   TODO's `--local` and `smithy --local` items marked superseded.

## Open

Settled 2026-10-29 (the developer, "continue" on the three proposals):

- **Books:** never scaled today but for their tree, so always full-size.
- **The last job's memory:** stays per machine, `data/smithy-last.txt`: the page of a smithy just
  started shows what this machine last ran; leases show what runs elsewhere.
- **"Older than Drive's copy" (step 6):** `GenState` gains a counter, `boundary`, incremented at
  every `write_state`; a missing one reads as 0.
