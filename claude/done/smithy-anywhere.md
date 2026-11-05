# One smithy, on any machine

**Status:** designed 2026-10-29 with the developer; plan in smithy-anywhere-implementation.md. Supersedes the `--dev` smithy
(discovery-and-heartbeats.md) and TODO's `--local` item.

**Results (2026-10-29):** steps 1 to 6 built, tested and committed; scaled runs, the lease and the
resume from Drive checked by hand on one machine (a run's local folder moved away and resumed from
Drive). As built, beside the decisions below: the scale fixed at 1024, never chosen on the page; a
scaled run on a source trains on a 1/s share of its files, scattered into buckets of its own
(`-s<s>`), and reads a seeded subset of the source's shared frozen set; Take over on the page for a
forge run only. Left: reproducible buckets after `-quiet` ends, and the checks across two machines.

## Goal (the developer, 2026-10-29)

No `--dev` smithy. One smithy, started the same way on any machine, on the real Drive, its runs
real or scaled. Any number of smithies may run at once, on any number of machines with a GPU or
without; a job may stop in one smithy and resume in another, never run in two at once.

## Decisions

1. **No `--dev`:** the smoke tree (`data/smoke/s<s>/`), its stand-in for Drive, the loopback-only
   smithy and its own-arena-only claim all go (`layout.rs`'s smoke test, `scale::FLAG` passed to
   children). Nothing pins the smithy to one machine: `production_arenas`' refusal goes, the arenas
   claimed are still those `data/machines.yaml` lists.
2. **The GPU decides full-size training:** the smithy checks for CUDA through libtorch at start.
   Without it, every job runs but a full run's forge (a forge trains each generation): New run and
   a full run's Resume are greyed out on the page with the reason. **Scaled runs train on the CPU
   (the developer, 2026-10-29):** they exist to run exactly on CPU-only machines, so New scaled run
   and a scaled run's Resume stay enabled there.
3. **A lease a job, on Drive:** a job is anything writing shared files on Drive: a run, a tune, a
   Convert into a corpus, the first freezing of a source's test set. Its directory holds
   `lease.txt`: the smithy's boot token, its machine's random id (`data/smithy-id.txt`, never a
   name), when taken, a heartbeat refreshed about every minute. Removed when the job ends or stops
   cleanly. A lease left by this machine's own earlier boot is taken over at once: a machine runs
   one smithy at a time, its port held, so that boot is gone (a crash's restart needs no Take over).
   - **Taking it:** written, then read back after Drive has synced it (about a minute); another
     token there means another smithy took it, and this one backs off. Not a lock, Drive syncing
     eventually: two Resumes of one job within the same minute are the residual risk, accepted
     rather than a lock server.
   - **A live lease** (heartbeat recent): the page says the job runs elsewhere and offers no Resume.
   - **A stale lease** (no heartbeat for about 10 minutes): the page offers Take over, showing its
     age, on the developer's confirmation only, never by itself: a smithy cut off from Drive for a
     while may still be training.
4. **Arenas unchanged:** each serves the smithy whose claim it holds, so smithies share the arenas
   between them.
5. **A resume needs only Drive:** everything a resume reads is on Drive or rebuilt from it.
   **Checked 2026-10-29:** today a forge's resume reads only the local run directory, never Drive
   (`forge`, `runs_of`, `run_dir` in `smithy.rs`); training's live state (each lambda's `model.vs`,
   `optimizer.opt`, `round_progress.state`, `best.opt`, best checkpoint), `history.tsv` during
   Train, the generation's self-play segments and frozen set, and the round robin's and gate's
   `.pgn` reach Drive only when the mover archives the closed generation. Tunes already fall back to
   Drive (`Tune::open`). Approved by the developer, 2026-10-29:
   - **Fallback to Drive:** a resume whose local run directory is missing, or older than Drive's
     copy, copies it down first, as a tune does.
   - **Everything up on release:** a clean stop copies the run's living files to Drive (training
     state, histories, open segments, games) before removing the lease, so a planned move between
     smithies loses nothing.
   - **Training state every 30 minutes,** not every round (nine lambdas' weights and optimizers
     every couple of minutes is much for a home upload; sizes estimated, not measured): a crash
     loses at most that.
   - **Segments as they close,** as the PeSTO corpus job does; a generation's frozen set once made.
   - **A missing `state.txt` refused** when the run has files locally or on Drive. Today it silently
     restarts the run at generation 1, attempt 1, in place (`smithy.rs`, `forge`). A hazard today,
     so fixable on its own before the rest. **Fixed 2026-10-29 (`forge`):** a missing state refused
     for every run but a new one, naming the remote copy to restore; and a new run numbered past
     the remote's runs too (`next_run_number`), so a run whose local directory is gone never has
     its number taken.
6. **Buckets rebuilt, never moved (the developer, 2026-10-29):** not on Drive (18 to 50 GB a folder,
   read whole every epoch; at the home line's speed an hour to fetch, against about 6 minutes to
   scatter again). Instead the buckets become a pure function of the corpus and the filter, so a
   resume on another machine scatters them again and lands on the exact record:
   - **The bucket by the position's identity:** a hash of its segment, game and ply, not of its
     arrival index (`buckets.rs`), which the PGN stream's unreproducible order changes.
   - **A fixed order before the shuffle:** each bucket's records sorted by their bytes as it loads,
     then the epoch's seeded permutation, so the order within a file, which still varies, never
     reaches training. The sort runs on the thread that loads the next bucket.
   - **A scatter version in `buckets.tsv`;** buckets without it are the old scatter.
7. **The old scatter kept only for the run in flight (the developer, 2026-10-29):**
   `lc0-psq-threats-screlu-quiet` resumes on its old buckets until it ends; then the old scatter's
   code goes, and buckets without a version are refused.
8. **Scaled runs:** at one fixed factor, 1024 (`scale::DEFAULT`, the developer, 2026-10-29: a
   debugging tool needs one; a field allowed a scale of 1, a full run on the CPU, and a bucket
   folder per factor typed), recorded in the run's config so a later change of the constant never
   changes a run started. A run is scaled or not, the scale frozen in its run config, shown on the page
   ("run 4, scaled 1/1024"). In `data/gen/<config>/` beside the others, numbered with them, saved to
   Drive as a full-size run is. They read the real corpus, a scaled share of it, so the scaled
   Convert goes. On a source, a scaled run reads a `1/s` share of its training files (the first ones, in their
   fixed order) scattered into buckets of their own (`<source><filter>-s<s>`, the developer,
   2026-10-29: a scaled run starts in minutes on any machine, never scattering the whole corpus),
   and a seeded subset of the source's shared frozen set, which the freezer already serves to a run
   asking fewer positions.
9. **Scaled networks do not play outside their run:** never installed, never offered as a side of
   a test; their own run's round robin and gate only. A scaled tune of one, at its run's scale, for
   debugging the tune alone (the developer, 2026-10-29), its parameters never used elsewhere.

10. **`data/gen` renamed `data/networks` (the developer, 2026-10-29),** locally and on Drive; the
    existing `data/networks` (the old pipeline's amanda to dorothy, read by no code, named only in
    `archived/configs/`' comments) renamed `data/networks-old` first. **Done 2026-10-29,** `-quiet`
    stopped for it and resumed after: `players::RUNS_ROOT` and every test, comment and DEV.md
    mention; the folders moved locally and on Drive; the paths written into files rewritten
    (`buckets.tsv` of self-play buckets, which would otherwise scatter every segment again, and
    `candidate.txt`'s record).

## Open

- **Saved to Drive (checked 2026-10-29):** a run's living records are copied at every phase
  boundary (`Run::mirror_living`), a closed generation moved whole by the mover (`archive.rs`);
  decision 5 adds to it. Scaled runs the same.
- **The last job's memory** (`LAST_JOB_PATH`): per machine as today, or the page listing every
  leased job on Drive.
- **Lease timings:** the heartbeat's period, staleness at 10 minutes, the read-back's wait; measured
  against Drive's sync delay.
- **Installs:** today refused to a dev smithy; with no dev smithy, allowed from any smithy, scaled
  networks excepted (decision 9).
