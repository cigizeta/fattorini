# TODO

Loose, running list of outstanding action items -- not full designs (those live in
`active/`/`done/`/`archived/`), just things not to forget.

**The project was called Rotor until 2026-11-04.** It is now temper-chess, displayed as Temper
(temper-chess-rename.md). `claude/` keeps its history as written, so older entries here and in the
documents use the old name: `rotor`, `ROTOR_*`, `.rotor`.

**Temper Games from 2026-11-05 (temper-games-restart.md, read it first).** The repository is
`temper-games`, the engine Temper Chess; `data/` and `configs/` gained a `chess/` level, the roots and
the venv are named `temper-games`, and older entries here use the older paths and names.

**Kalo Games from 2026-11-06 (kalo-games-rename.md).** The repository, roots and venv are now
`kalo-games`; the engine stays Temper Chess. The opening positions and the tablebases moved to the
repository's `data/` (data-root-in-repository.md). Since the same day nothing uses
`%LOCALAPPDATA%`: snapshots go to Drive's `data/chess/snapshots/` (no-localappdata.md, snapshots.md).

**Kalo Chess from 2026-11-06 (kalo-chess-rename.md):** the engine is `kalo-chess` (crate, binary,
`bin/kalo-chess`, `KALO_CHESS_*`, UCI name Kalo Chess), the network magic `KALO`, and the Lichess
account `kalo-chess`. Older entries say Temper or `temper-chess`.

## Next (the developer, 2026-11-06)

In this order:

1. **Tune the lc0 network** (`lc0-psq-threats-screlu-g1-r1`), SPSA from the smithy's page.
2. **Train and tune a PeSTO network:** a forge run on PeSTO's played corpus, its winner tuned.
3. **Lichess, the tuned lc0 network:** a `@tuned` snapshot (`bin/arena snapshot`), lichess-bot's
   `engine.name` pointed at it. Ready since 2026-11-06: the bot `kalo-chess` (account and token
   done), lichess-bot's config on a Drive snapshot, games in `data/chess/lichess/`; it plays the
   untuned snapshot until then. `temper-chess` to close.
4. **The first release:** binary-release-implementation.md, at a tag; a draft first, its open
   points (notices, notes, version) settled on it when the time comes (the developer, 2026-11-06).
5. **CCRL testers:** tell them of the release.

## Status (2026-11-05, read this first)

**Changed since the list below:** the Lc0 rewrite is done (`curve.txt` says so, and moved with the corpus).
`fabio` is merged. Old networks no longer load in the new binaries: they stay in the
old Drive root and play from a build of the archive, so step 4's duel against emily is a CuteChess
match. A clear loss cannot be reverted, the new repository having one commit: the old training comes
back by hand from the archive. Atan support stays until that verdict.

**The experiment's verdict (2026-11-05):** `lc0-psq-threats-screlu-g1-r1` passed its gate, 36.5 to 3.5
against PeSTO, 173 of 179 rounds in 1h08m, test loss 0.624559, labels and frozen set by the tables
(scatter-tablebase-speed.md). Against emily's generation 1, CuteChess, 10+0.1, one thread, 64 MB, UHO
book, both untuned, different builds: stopped by the developer at 375 games, 114 - 103 - 158, 51.5%,
+10.2 +/- 26.8 Elo, LOS 77%, llr 0.278. Not a win and no SPRT result: about the same strength, and
the new pipeline stays, one arm trained in about an hour against nine and a round robin. It says
nothing about lambda, the two being almost alike on Lc0 (polyline-calibration.md). The build check
of PeSTO against PeSTO was skipped. Atan support and lockstep training of several arms are deprecated
by this, and stay in the code for now (the developer). Next, not decided: generation 2 or a tune.

Branch `fabio` carries two experiments, judged together by one duel:

- **The training corrections** (training-corrections.md): Lc0's Q read through Lc0's own curve, the
  network's output as `sigma(net / S)` with S = 70.37 cp, the binary cross entropy; PeSTO and
  self-play labelled `sigma(cp / S)`; old (atan) networks play only. Done and tested.
- **The lambda polyline** (lambda-polyline.md, lambda-polyline-implementation.md, both in done/):
  per position, lambda by progress through its game, ten bucket values measured once per corpus;
  `lambda: polyline` in a forge config trains one arm, straight to the gate. Done, tested, and
  verified by a scaled PeSTO run (generation 2 beat generation 1, 65.6%).
  `configs/pesto-psq-threats-screlu.yaml` already says `lambda: polyline`.

Commits may be waiting for a push: GitHub's Git Operations were degraded on 2026-11-04.

**Next, in order (the developer, 2026-11-04):**

1. **Finish the Lc0 conversion:** `bin/smithy lc0-rewrite` (10 of 1,405 segments done, resumable)
   on the machine that started it, kept on until the Drive client has uploaded everything; no other
   machine reads Lc0's corpus before. Check the done-file lists all 1,405 segments.
2. **Clean what the old curve made:** on the training machine, stop the smithy,
   `rm -rf data/networks/lc0-psq-threats-screlu/run-1` (its remote copy already deleted),
   `rm -rf data/buckets/lc0*`, check out `fabio`; on the remote, delete Lc0's frozen test set
   (`data/lc0/frozen`), only after step 1, so it is frozen again under Lc0's curve. Optional: the
   rewrite changed every segment, so the training machine downloads the corpus once either way (the
   old run's scatter was slow for that download); marking the folder "Available offline" early only
   starts it sooner, overlapped with other work, and may be faster than the scatter's on-demand reads
   (unmeasured).
3. **Done 2026-11-04:** `configs/lc0-psq-threats-screlu.yaml` says `lambda: polyline`.
4. **The experiment:** a full New run of `lc0-psq-threats-screlu` on the training machine. Its
   polyline is measured on the whole training set, from the finished buckets, after the scatter and
   before the frozen labels (polyline-from-buckets.md; expect about 0.01 to 0.03 early, 0 from
   mid-game) and kept at `data/lc0/polyline.txt` on the remote. Then duel its generation 1 winner
   against emily's generation 1, before any SPSA. A clear loss: revert the squash merge, delete the
   frozen test sets again, and Lc0's corpus needs a reverse rewrite or a new download, unless backed
   up.
5. **Meanwhile, PeSTO's corpus is being played from scratch** (below); a PeSTO run waits for its
   `complete.txt`.

6. **After the experiment, is Lc0's late lambda of 0 right?** (polyline-calibration.md, proposed
   2026-11-05): the clamp hides a model that does not apply to a squeezed teacher. A read-only
   measurement first, slope and bins per bucket of progress, then a design if the squeeze is real.

## Now

- **The polyline's measurement has a live bar, and Stop ends it (polyline-from-buckets.md,
  2026-11-04):** written, tests and the page's bar to check at the first real pass, which is timed.
- **In progress (training-corrections.md, training-corrections-implementation.md, 2026-11-04): the
  logistic curve at S = 70.37 cp with the binary cross entropy, an experiment on branch `fabio`.**
  Code written and tested (steps 1 to 8). What remains is the Status section's steps 1 to 4.
- **In progress (2026-11-04): PeSTO's corpus replayed from scratch with the current search.** The
  lambda experiment's sample is done and `lambda_experiment.rs` deleted; its polyline,
  lambda-polyline-implementation.md, verified by a scaled run (generation 2 passed its gate); a scaled run may start on an incomplete
  source, keeping its polyline and frozen test set in its own generation so no later run inherits
  them. The
  old partial corpus (about 1M positions, test games from an older search), its buckets and the
  scaled test run were deleted, here and on the remote. Play is filling the test games, then the
  training games, until `complete.txt`. **Still to do:** delete any local `data/pesto` and
  `data/buckets/pesto*` on the training machine before it trains on PeSTO.
- **To design: a scaled run's share of a partial source.** `scaled_share` takes 1/s of the positions
  that exist, so on an incomplete corpus a scaled generation 1 trains on almost nothing (the 1/1024
  test: about a thousand positions, 3 of 34 against PeSTO). Proposed: 1/s of the full corpus's size,
  capped at what exists. Not decided (the developer, 2026-11-04).
- **Done 2026-11-03: the label scale is too timid for our self-play (found 2026-11-03,
  continuous-training.md).** In emily-g1's depth-9 self-play a pawn up ends at 0.71 on average where the atan scale
  says 0.54; from 1.5 to 4 pawns the scale needs about 2.5 times the pawns (Lc0's corpus: about 0.7
  to 1). The direct blend therefore averages a timid s and a steep z. Options to discuss: refit the
  curve's constants to self-play results, calibrate s per generation before blending, or leave it
  to lambda. Measured by the ignored test `lambda_star_of_corpus` (`lambda_star.rs`), which also
  gave lambda_0 = 0.036 on Lc0's corpus. **Designed and planned 2026-11-03 (label-scale.md,
  label-scale-implementation.md):** f at one pawn 0.50 with a ceiling of 128, g fitted per teacher
  (about 0.62 for emily-g1), Lc0's files decoded with their frozen curve. **Implemented and tested
  2026-11-03** (`scale_fit.rs`, each corpus's `scale.txt`). Lc0's own one pawn score, measured the
  same day: 0.50 or a little below, Q calibrated as it is. Still to do: show the one pawn score on
  the run's page. Then continuous training (continuous-training.md, designed, not planned).
- **Superseded 2026-11-04 by training-corrections.md (no fitted scale; Lc0's corpus rewritten into
  Lc0's own curve instead): Lc0's one pawn score fitted, not hard-coded (lc0-fitted-scale.md).** One ceiling everywhere, 128: Convert writes Q through `NETWORK`, Lc0's corpus is
  re-encoded once from its files (old curve to Q to `NETWORK`, no download) by an ignored test the
  developer runs, then its scale fitted against the results like any teacher's. **Step 1 done**
  (`train` fits Lc0's scale), clippy passing. **Next: step 2**, then the reconversion. **No Lc0 run
  until the reconversion is done.** The tablebase rescoring's run on the training machine (below)
  waits for it too.
- **Done 2026-11-02, against expectations: `lc0-psq-threats-screlu`'s generation 2 passed its gate**
  against g1-r1, 918.5 of 1,720 games, 53.4%, about +24 Elo at depth 9 (LLR 3.07); won by lambda
  0.06, the round robin flat (49.0% to 50.7%, +/- 0.9%). **No generation 3 (the developer):** the
  run becomes `emily` (both winners kept, `archived/configs/emily.yaml`), `data/lc0-counts` is
  renamed `data/lc0` (the code reads `data/lc0` again), and training starts over on it from
  `lc0-psq-threats-screlu`'s run 1. The record of the test follows.
- **Attempts removed (no-attempts.md, 2026-11-02, the developer):** every self-play generation
  plays 1,073,741,824 positions once, a failed gate ends the run; `max_failed_gates`, the
  `attempt-<a>/` directories and the `-a<a>-` in names gone. Done the same day, every run on Drive
  and here migrated. **No self-play for a while (the developer, 2026-11-02):** training restarts
  from scratch on Lc0's data (`data/lc0`), generation 1 only; the 1B generation 2 waits until
  self-play comes back. Training's settings stay as they are: a round is a fixed number of steps,
  so patience does not depend on the training set's size.
- **The scatter, possible optimizations (the developer, 2026-11-02, not designed).** Lc0's first
  scatter on `data/lc0` took about 1h30m, bound by Drive's download (Wi-Fi flat at about 45 Mbps,
  the CPU at 8%, no core pinned): 25.7 GB streamed on demand. Paid once a machine, the buckets then
  reused by every `lc0-` run. **The first try failed after 79 minutes (2026-11-02):** one segment's
  read from Drive's streamed `G:` failed ("Incorrect function", os error 1; the cause, the developer
  2026-11-04: a Google Drive sync not yet complete), the stream worker panicked, and the whole
  scatter was lost. Worked around by making `data/lc0` available offline before resuming. **A
  failed read is now waited out (segment-read-wait.md, implemented 2026-11-04, tests and clippy
  passing, a real wait not yet seen):**
  every minute, with no limit, until it succeeds or the job is stopped, the wait shown on the page.
  Two ideas:
  - **Resumable: done 2026-11-03 (resumable-scatter.md, in batches of 50M positions, a commit
    after each), tests and clippy passing; a real scatter not yet seen resuming.** As first asked: not needed while the buckets
    stay on this machine, but required before any future scatter of a large corpus. Today `buckets.tsv` records the scattered files only at the end
    (`buckets::prepare`), so a scatter stopped partway starts over. Record them every N segments
    with each bucket's committed length; a restart truncates to the last record and goes on.
  - **Faster:** only the download bounds it now. A wired line, or a local copy of the corpus, helps
    a second scatter alone. If the download ever stops binding, the single scattering thread
    (compression and writes, `buckets.rs`) is next.
- **Done 2026-11-02, gate passed (above): `lc0-psq-threats-screlu`'s generation 2 (started
  2026-10-31), the fair test of self-play after Lc0.** Kept as a record. `-quiet-r1`'s generation 2 lost (gate -10 Elo, about -94 with the new search) but
  confounded several changes at once: 100M positions against generation 1's 1B, the quiet filter,
  and a coarse grid (0.10 to 0.90) that never reached generation 1's lambda 0.02 (lambda weighs the
  game result, `labels::compute_labels_direct`: 0.02 is 98% the search's score, 0.30 70%). This one:
  teacher g1-r1, no filter, 4 x 96 x 1024 x 1024 = 402,653,184 positions an attempt (the run's
  config raised before play, and `configs/lc0-psq-threats-screlu.yaml`), the fine grid 0.01 to 0.09
  (its state predates the coarse grid), today's search. Self-play about 24 h at the rate first
  seen. The developer's reasoning for the low grid: a depth-9 search's score improves on the
  network's own static evaluation whatever its quality. Attempt 2, if it comes, adds another
  402.7M (805.3M in all). **Gated against g1-r1: if it fails, self-play after Lc0 is ruled out.**

- **The training design (the developer, 2026-10-31): two lines, as at the start.** Discussed after
  Stockfish's networks were found to start on non-Lc0 data and then train in many Lc0 stages:
  - **Identity: PeSTO, then self-play generations,** nothing from outside Rotor; strength from
    Rotor's own budget, expected well behind Lc0's.
  - **Lc0: one stage, from scratch, unfiltered.** The strongest; more Lc0 stages, and a self-play
    start before Lc0 (Stockfish's order), dropped: one large Lc0 stage most likely washes out any
    start, and Lc0 is one stage (the developer). Whether self-play after Lc0 helps: the running
    generation 2 above.
  - **Open:** which line Rotor releases (CCRL, Lichess), to record as a decision so that an Lc0
    network is never swapped in silently. (The cap on a generation's attempts, once open here, is
    moot: attempts were removed 2026-11-02, no-attempts.md.)

- **Next, after generation 2's gate (passed 2026-11-02, against expectations): the halfk line (the developer, 2026-10-31).**
  On psq inputs the line ends at `psq_threats_screlu` (architectures.md); halfk (halfkav2_hm) is
  next, Lc0's labels only, one stage, each step one change from the previous winner, duelled at
  depth 9 then 10+0.1, both `@default`. Threats are a must, so no bare halfk:
  1. **`halfk_threats_screlu`** against `lc0-psq-threats-screlu-g1-r1`: psq inputs swapped for
     halfk, the threats shared across king buckets as in psq, SCReLU, no hidden layer. Do halfk
     inputs help? If it loses or draws, bare halfk against bare psq is the diagnostic.
     **Designed and written 2026-11-02** (halfk-threats-screlu.md, its implementation plan): the
     32 buckets factorised, engine tag 20 with a refresh cache, no measurable cost at depth 9;
     configs `lc0-halfk-threats-screlu` and `pesto-halfk-threats-screlu` (lambdas 0.91 to 0.99,
     after the Lc0 run). The old halfkp and halfka_v2_hm code removed.
  2. **`halfk_threats_screlu_hidden`** against step 1's winner: does a hidden layer help on halfk,
     where Stockfish's hidden layers pay? **Designed 2026-11-02** (halfk-threats-screlu-hidden.md):
     Reckless's head, 512 -> 16 -> 32 -> 1, clipped, no skip, end to end, from scratch, the first
     layer in int16 and int8 both, chosen on measurements; no weight bound (one scale per row).
     **Written 2026-11-02** (its implementation plan): engine tag 21, config
     `lc0-halfk-threats-screlu-hidden`; the speed measurements done 2026-11-02 on random networks
     (int8 about 8% faster, both kept until the trained winner decides).
  3. **`halfk_threats_pairwise_hidden`** against step 2, only if step 2 wins or draws: pairwise in
     place of SCReLU, both with the hidden layer. Pairwise only with a hidden layer: its gain is
     halving the layer's input, and without one SCReLU beat it on psq (+38 Elo). **Designed
     2026-11-02** (halfk-threats-pairwise-hidden.md): step 2's network with pairwise in place of
     SCReLU, width 256 kept, step 2's arithmetic inherited. **Written 2026-11-02**, ahead of step
     2's duel (the developer): engine tag 22, config `lc0-halfk-threats-pairwise-hidden`; trained
     only if step 2 wins or draws.
  **The new corpus, the old one deleted (the developer, 2026-10-31):** every Lc0 network from here
  trains on `data/lc0-counts`, renamed `data/lc0` on 2026-11-02 (about 1.2B positions,
  unfiltered), so a step 0 comes first, the next thing to run, on the training machine:
  `psq_threats_screlu` retrained on it, the baseline steps 1 to 3 are judged against, its duel
  against today's g1-r1 measuring the new corpus alone. Before it, the forges must read
  `data/lc0-counts`: every searched position, the visit counts ignored by value networks
  (lc0-policy-implementation.md's step 4 planned quiet `lc0-value` buckets, moot without the
  filter); a small design change to agree on first. **Designed and written 2026-11-02**
  (lc0-counts-forges.md, its implementation plan): the forges read `data/lc0-counts`; every config
  keeps its name and moves to it, step 0 being `lc0-psq-threats-screlu`'s next run, whose winner
  replaces the old network whatever its duel says. **The old run became `emily`** after generation
  2's gate (done 2026-11-02, `archived/configs/emily.yaml`), kept as a record, its
  winners duelled against step 0's. **The same test set:** `data/lc0/frozen` was found gone from
  the remote, but the new corpus's test games draw the old set exactly (all 4,194,304 positions and
  their labels, checked 2026-11-02), so every old network's test loss compares. **`data/lc0`
  deleted** after the rename, by hand, here and on Drive; nothing waits on it any more.
  **To design first:** the engine's halfk work (a far larger first layer, the accumulator refreshed
  when a king crosses buckets, Stockfish's cache of refreshes); training time and evaluation speed.

- **The search closer to Stockfish's (search-vs-stockfish.md, search-vs-stockfish-implementation.md,
  the developer, 2026-10-31):** where the search reads the static evaluation, five candidates (no
  pruning at PV nodes, none at the root, reverse futility without a quiet TT move, a correction
  history, blended returns), each a switch duelled at 10+0.1 and depth 9 with g1-r1 and `-quiet`,
  untuned; then `-quiet`'s duels and `-quiet-r1`'s gate played again; then the full quiet filter's
  experiment on `data/lc0-counts`. **Done 2026-10-31:** every duel drawn but `corr` (+18 and +17
  Elo on the clock) and `blend` (won both at depth 9, flat on the clock); `DEFAULT` keeps those two,
  `pv`, `root` and `ttquiet` reverted (kept as switches, off). The verdicts played again stand,
  wider: `-quiet` about -190 and -137 Elo, `-quiet-r1`'s generation 2 about -94 (the gate said
  -10). Step 7 dropped with the quiet filter (quiet-filter.md). Left: `blend` as two weights tuned
  by SPSA, when SPSA comes. **The switches removed 2026-11-02** (search-switches-cleanup.md): `corr`
  and `blend` plain code, no `@default+<switch>` any more.
- **To measure, on an idle machine: today's search against the old one with a network (2026-10-31).**
  Generation 2's self-play plays more games a second than `-quiet-r1`'s did; `corr` saved 4.65%
  nodes and `blend` cost 11% at depth 9, both with PeSTO and apart. Nodes at depth 9 on the bench
  positions with g1-r1, all switches off against `DEFAULT`; and positions per game in the two runs'
  segments, in case games got shorter.

- **Done 2026-10-31, duel-per-pair.md: the pair the unit, `sigma_hat` measured, C 3.09 kept, capped
  at 400 pairs.** Drawn duels now end near 370 to 390 pairs (about 750 games, 37 minutes at 10+0.1);
  `sigma_hat` about 0.22 on the clock, 0.25 to 0.28 at depth 9. The discussion that led there, kept:
- **To discuss: drawn duels end too late (2026-10-31, the developer: "winners are already detected
  early; what is annoying is having to wait 900-1000 games for a duel that is drawn").**
  Curtailment is exact, so it can only stop once the verdict cannot change, near 900 games for an
  even score. Stopping sooner means stopping on evidence, which changes some verdicts against the
  full duel (2026-10-08: a projection rule lost about a sixth of a +40 Elo engine's wins).
  **Idea, no new parameter:** a drawn duel means no gap the cap could detect (0.5 +/- 0.049 at
  1,000 games, about 34 Elo), so declare the draw once the data shows that at the duel's own
  confidence: `|S - 0.5| + C * sigma_hat / sqrt(N) < 0.049`, `sigma_hat` the measured standard
  deviation of a game (about 0.37 at today's 40% to 48% draws, not the 0.5 the threshold assumes).
  An even duel would end near 550 games; a lean like `root`'s 51.4% would still run to
  curtailment. Open for it: how often a real 25 to 34 Elo gap is then drawn, by simulation as in
  October; a floor on `sigma_hat` while it is noisy. **Not** a variance-measuring win threshold
  (`C * sigma_hat / sqrt(N)` for the winner): it finds winners sooner, not needed, and curtails
  draws about 3% later.
  **The developer's preferred direction (2026-10-31, not today): a lower `C` and a lower cap
  together,** the rule unchanged otherwise (no `sigma_hat`). Fewer checks let a lower `C` keep the
  5% false winners; a drawn duel ends near `M - C * sqrt(M)` games for a cap `M`, the resolution at
  the cap `C * 0.5 / sqrt(M)` (cap 500, `C` about 2.95: about 434 games, about 46 Elo; cap 600:
  about 527, about 43 Elo; `C` only illustrative). Changing `C` alone does not shorten drawn duels:
  a lower one curtails them later. **Optionally the standard deviation in the formula** (the
  developer, 2026-10-31: "to make it more standard"), `0.5 + C * sigma_hat / sqrt(N)`: `C` then in
  real standard errors, so calibrated lower and at several draw rates, the worst-case 0.5's margin
  gone; with `sigma_hat` there, the early draw above comes almost free. **Per pair, not per game**
  (the developer, 2026-10-31): a pair's two games share a UHO opening, so they are not independent;
  the unit is the pair score (0 to 2 points, halved), its standard error `sigma_pair / sqrt(P)`.
  Today's rule assumes independent games: conservative when pairs tend to split 1-1 (UHO's aim),
  more than 5% false winners if they tended to go 2-0 or 0-2. Worst case per pair, no estimate:
  `0.5 + C * 0.5 / sqrt(P)`, stricter than today; measured, `sigma_hat_pair`, catching both the
  draws and the pairs' cancelling (fishtest's pentanomial model). To do first: today's duels' split
  of the five pair results, from their recorded games; then a simulation sweep **of pairs** with
  that split, over caps, with and without `sigma_hat_pair`, giving each cap its `C` for 5% false
  winners, its chance to win at +20, +30, +40 and +60 Elo, and its mean length.

- **Closed 2026-11-02: the quiet filter removed from the code, the `-quiet` runs to go (the
  developer).** Was next on 2026-10-28 (quiet-filter-implementation.md, "To resume"). The quiet run of SCReLU
  (`lc0-psq-threats-screlu-quiet`): **generation 1 done 2026-10-30,** won by lambda 0.02 (best
  test loss 0.050556 on its own quiet test set, not comparable with SCReLU's), the round robin flat
  (every lambda 48.7% to 51.1% +/- 1.0%), gate against PeSTO passed 63 to 29 in 92 games. **Both
  duels lost** against `lc0-psq-threats-screlu-g1-r1`, both `@default`: test 19, depth 9, 16 of 56,
  28.6%, about -160 Elo; test 20, 10+0.1, 38.5 of 110, 35.0%, about -108 Elo. By experiment 1's
  rule the filter is dropped. **Explained (2026-10-30):** `sibling_ranking` on the 2026-10-27 18:25
  duel's games (SCReLU's own, biased its way): quiet orders siblings *better* at quiescence (83.81%
  against 80.90%, +/- 0.34) but worse statically (62.75% against 68.31%, +/- 0.38). The pawn scale
  level (`100 / c` 38.5% against 37.9%, both searched at 40%), but its residual against PeSTO's
  quiescence 0.2718 against 0.2250: it misjudges positions with a capture pending. Likely the
  search's static uses (futility, razoring, null move, stand-pat) on non-quiet nodes, which
  quiescence never corrects. **So concordance at quiescence alone is no screen:** the static view
  must not fall behind either (sibling-ranking.md and loss-and-strength.md amended 2026-11-04). Lc0's labels on
  tactical positions keep our static evaluation honest; Stockfish's filter does not carry over.
  Meanwhile, self-play's test target to design, which blocks every generation 2 and PeSTO run.
  **Confirmed 2026-10-31 (quiet-filter.md, "The outcome"):** with the new search still about -190
  and -137 Elo; the residuals split by kind show `-quiet` equal to g1-r1 on quiet positions and about
  three times worse on noisy ones, which the search evaluates at every node. **Lc0's networks train
  unfiltered (the developer, 2026-10-31);** the filter's code, first kept until the training
  pipeline was discussed, is removed (2026-11-02).

- **From generation 2 on, training is fine-tuning: start at 3e-4 (the developer, 2026-10-30).**
  A warm-started generation trains from its teacher's weights, so it fine-tunes rather than learns
  from scratch, as Stockfish's retrains do (about half its from-scratch rate). `-quiet-r1`'s
  generation 2, the five stages 3e-3, 1e-3, 3e-4, 1e-4, 3e-5: at 3e-3, 7 rounds, no lambda beat its
  step 0 (the teacher's weights), its test loss 5 to 20% worse after round 1 (0.10: 0.027334 against
  0.022726); at 1e-3, none either; at 3e-4, every lambda beat step 0 from the stage's first round,
  by about 1.5% each (0.10: 0.022374; 0.50: 0.078666 against 0.079873; 0.90: 0.207323 against
  0.210484). The two first stages, about 14 rounds and 21 minutes, only led back to where training
  began, the rollback undoing them. To do: a warm start's stages begin at 3e-4 (3e-4, 1e-4, 3e-5);
  generation 1, from random weights, keeps all five. A second warm start to confirm first, if
  generation 2's later stages or a later generation say otherwise. **Not built yet (2026-10-31):**
  `lc0-psq-threats-screlu`'s generation 2, started that day, trains from 3e-3 again; its first two
  stages are that second warm start's confirmation.

- **A full run's non-training phases on a machine without CUDA (the developer, 2026-11-01, an
  optimization; done 2026-11-03, cpu-phases.md, tests and clippy passing; a non-training phase
  not yet seen resuming without CUDA).** `forge` refuses a full run's Resume wherever CUDA is missing
  (`lacks_gpu`, right after the lease), and the page disables the button (`forgeDisabled`), whatever
  the phase; yet self-play, the round robin and the gate are played by the arenas, and only training
  needs the GPU. Wanted: a Resume allowed there outside training, the job ending cleanly when
  training comes ("waits for Resume on a machine with CUDA"), a Resume landing in training refused
  with the same words. Would have let generation 2's self-play start on any machine and move later.

- **A run on Drive alone viewed, tuned and installed (remote-runs.md, done 2026-11-03):** the View,
  a winner's Tune and Install read a run started in another smithy from the remote, its frozen
  config included (a scaled run there showed as full). Tests and clippy passing; a run
  started on the training machine viewed on another one, checked the same day.

- **Lc0's Convert: skip a slow tar only while the line is fine (2026-10-30).** A tar slow over its
  first two minutes is skipped (lc0-skip-slow-tars.md), which assumes the slowness is the file's. A
  hard drop (no route, no name) is a curl failure, not slowness: five in a row, 10 s apart, end the
  run, nothing skipped. But a stall, the line up and almost nothing getting through, reads as
  slowness: each worker skips its tar every two minutes and takes the next, stalled too, about one
  tar a minute with two workers, an hour's stall some 60 gaps (retried next run, gaps for good if the
  cap comes first). Fix: skip only when another worker received at normal speed over the same
  window, as `20260104-1817` was found slow beside fast neighbours; all slow together is the line,
  and every worker keeps resuming until it recovers. Sooner rather than later: a stall is not rare.

- **Lc0's Convert: a maximum of skips per tar (the developer, 2026-10-30).** A tar slow over its first
  two minutes is skipped and recorded in `data/lc0-counts/skipped.tsv`, and every later Convert tries
  it again (lc0-skip-slow-tars.md). On 2026-10-30 `20260104-1817` and `20260105-0017` were each
  skipped twice, on two machines and runs, always at 25 to 45 KB/s against 1.75 MB/s for their
  neighbours, slow now on any machine. Yet both are in the old corpus's `data/lc0/tars.tsv` (rows 92
  and 98), downloaded whole around 2026-10-17: the slowness is not the file's for good, so retrying
  is worth it (Lc0 may have moved older tars to slower storage since, or they were slow then too and
  went unnoticed, the old code having no speed check). Each retry costs only two minutes a run, so
  this waits until slow files multiply: a tar with N rows in
  `skipped.tsv` (3, say) is no longer tried, counted from the file, and said at the run's start.

- **Moot 2026-11-02, the quiet filter removed (self-play keeps every searched position):** a quiet
  self-play's kept count after a restart (2026-10-30), kept as a record. At a restart, `play()`
  counts the kept positions again by parsing every game of every segment (`kept_in`): minutes at
  120M positions. Meanwhile there is no live work, and the run's box falls back to the file view,
  which counts every position read (`positions_in`, from the `.meta`s): `-quiet-r1`'s generation 2
  showed 5.5M of 4.2M and 120.5M of 100.7M, both "100%", an hour before its end. Fix: a `kept` line
  in each segment's `.meta`, written when it is sealed; the file view shows it, and the recount reads
  the `.meta`s alone. A segment sealed before the change counts as today, parsed.

- **`-quiet-r1`'s generation 2, attempt 1: failed its gate (2026-10-30).** Self-play with generation
  1's winner, depth 9, the quiet filter (about 73% of the training positions kept), 100M kept, the
  coarse grid 0.10 to 0.90. Training: no lambda beat step 0 at 3e-3 nor 1e-3; from 3e-4 all did,
  most at low lambdas (0.10: 0.022726 to 0.021764, -4.2%; 0.50: -2.2%; 0.90: -1.8%), the gap between
  train and test loss small (about 1%): not data-bound. Round robin not flat: low lambdas ahead (0.10
  to 0.40 about 57 to 58% at a sixth of the games, 0.60 to 0.90 at 38 to 46%), winner 0.30. Gate
  against generation 1: 48.5% over 372 games, about -10 Elo, level with the teacher. So self-play at
  depth 9 with the quiet filter does not improve on an Lc0-trained teacher; the duel against
  `lc0-psq-threats-screlu-g1-r1` is not worth playing. Attempt 2 waits for Resume, not resumed:
  more of the same labels is unlikely to help, the loss being label-bound. Next, proposed: the same
  generation 2 unfiltered, every searched position kept, to test whether the quiet filter is what
  holds self-play back (it drops the positions where the search disagrees with the evaluation).
  Working explanation (the developer, 2026-11-02): the quiet filter made the target too easy and
  cost the Elo; the unfiltered generation 2 (400M positions) confirms it if its gate beats
  generation 1. **If that gate fails narrowly (the developer, 2026-11-02):** a fairer test, one
  generation 2 on 1B self-play positions, with no attempt logic (a failed gate ends it, no
  retry); its rounds and patience to be reconsidered for the larger set first. Run it on the new
  Lc0 data (`data/lc0-counts`, to be renamed `data/lc0`): generation 1 trained on that, then the 1B
  generation 2 from its winner. **Whatever this gate says, the 1B test runs (the developer,
  2026-11-02, for peace of mind):** a pass at 400M is one result, a fail may be data size; at 1B
  generation 2 matches generation 1's corpus size, so neither verdict hangs on it.

- **After `-quiet`'s generation 2, whichever its result: rethink the training order (the developer,
  2026-10-30).** **Settled 2026-10-31 by the training design (above): Lc0 one stage, no self-play
  start before it.** Generation 2 trains the `-quiet` network on Rotor's self-play (Rotor's labels and
  best move), to gate against generation 1 and duel `lc0-psq-threats-screlu-g1-r1`. Stockfish does
  the reverse: nnue-pytorch's wiki says it is usually best to train first on Stockfish-generated
  data (its first nets: self-play at fixed depth 9), then retrain that network on Lc0 data, and that
  Lc0 data alone gave weaker nets. Its retrain is a fine-tune: the earlier net's weights, a learning
  rate about half the from-scratch one, decaying, Lc0's evaluations as targets (lambda 1.0 in
  several runs), in chains, each step from the previous step's best epoch. Its filters on Lc0 data
  are not the capture filter alone: best move a capture skipped, a depth 6 multipv 2 search of
  Stockfish's own dropping captures *and positions with one good move*, positions whose evaluation
  disagrees with the result dropped at random, early plies skipped, endgames rescored by
  tablebases, batches rebalanced by piece count. To weigh: self-play first then Lc0, our retrain's
  starting learning rate against the teacher's, and the one-good-move filter. **What our own +70 Elo
  shows, and does not:** `lc0-psq-threats-0001` beat `caroline-0001` (2026-10-19), but caroline's
  corpus was mostly replayed CCRL and TCEC games (about 80%, self-play the rest), labelled by Rotor's
  depth 9 search, while the Lc0 network's labels are Lc0's `best_q`. So Rotor's depth 9 labels lost
  to Lc0's, positions differing too; neither order, self-play then Lc0 nor Lc0 then self-play, was
  tried. Generation 2's teacher differs from caroline's: a network trained on Lc0.

- **Why a lower loss does not make a stronger engine (loss-and-strength.md): closed 2026-10-28.**
  pairwise_hidden orders a position's children best statically and worse than SCReLU once
  quiescence has played the captures out, which matches its lost duels (sibling-ranking.md); its fit
  on Rotor's own positions is level with SCReLU's (rotor-positions-loss.md). Why its architecture is
  weaker there is not known. Kept: concordance at quiescence (`sibling_ranking`) screens every new
  network before its duels.

- **The experiments planned, in order (revised 2026-10-28):**
  1. **Quiet positions only (quiet-filter.md, quiet-filter-implementation.md, the developer,
     2026-10-28):** `lc0-psq-threats-screlu-quiet`, SCReLU on a corpus filtered as Stockfish's is
     (in check, or Lc0's best move a capture or promotion), with a quiet test set of its own, against
     `lc0-psq-threats-screlu-g1-r1` by duels. If it clearly loses, the filter is dropped; if it
     draws, `-quiet-lc0` (Lc0's second choice a capture or promotion also dropped, about 7% more;
     Stockfish skipped, lc0-second-move.md) only if a cheap check on the test set finds the extra
     positions hard (quiet-filter.md, step 2); if it succeeds, stop and think, then
     pairwise_hidden-quiet. Width 512's run stopped.
     **The test set measured (2026-10-28, quiet-filter.md):** a game's positions correlate (design
     effect about 15, the frozen set worth about 500,000 independent positions), but two networks
     on the same positions differ with an error of about 0.09%: every loss gap behind the earlier
     verdicts but taper's is real. Not the trouble; the quiet set drawn from the same test games.
     The final position, never searched, dropped from every config, the data rebuilt.
  2. **Width 512, generation 1** (psq-threats-screlu-512.md): training, stopped 2026-10-28 for the
     quiet experiment, resumed from the page after it (with the filter if it wins); screened
     by `sibling_ranking`, then its duels against `lc0-psq-threats-screlu-g1-r1`, both `@default`,
     depth 9 then 10+0.1, which decides.
  3. **Generation 2 of the winner:** self-play, its training, a duel against its own generation 1.
     The psq line's last check, and the first generation labelled by Rotor's own search.
  4. **SPSA tune of the best network,** then `0.1.0` and the version bumped to `0.2.0`.
  - **Done:** the loss on the duel games' positions (rotor-positions-loss.md, 2026-10-28).
  - **Dropped (2026-10-28):** pairwise_hidden's generation 2. Its condition, pairwise_hidden clearly
    worse than SCReLU on Rotor's own positions, was not met (level), so self-play has no shift to
    fix; its weakness is in ordering the leaves, which self-play's root positions do not target.

- **Tablebase rescoring: done 2026-11-03 (tablebase-rescoring.md), tests and clippy passing.** Every
  NNUE scatter labels the positions the tables answer, at every lambda, 1, 0 or -1; no config key,
  the tables required, old buckets scattered again by themselves. **Next, on the training machine
  when it is available:** pull; stop step 0; delete `lc0-psq-threats-screlu`'s run 1 there and on
  Drive (the developer, 2026-11-03: a safe network, not a test); check `bin/syzygy.sh` has run
  there; New run from the page, its scatter rescoring. The original entry:
- **Tablebase rescoring of the Lc0 corpus, up to 6 pieces (the developer, 2026-10-28):** Lc0's
  labels in few-piece positions are its own values, never rescored (lc0-data.md left them, Rotor
  probing Syzygy in search); Stockfish rescores Leela's data with Syzygy before training. A wrong
  label there still trains the shared transform, and quiescence leaves fall into the tables. A probe
  a position, no search. Computed once by a probe-only pass, applied by a config key of its own, so
  its effect is measured apart from the quiet filter's (quiet-filter.md, decision 6). Open: the label of a win, draw or loss (Rotor's mate scale or a
  fixed score), and rescoring against dropping those positions from training.

- **The final position omitted everywhere (final-position-omitted.md, 2026-10-29):** `Keep::Every`
  deleted, the transformer's stream on `Keep::Searched`, its epoch the `.meta` positions less the
  games. Done the same day, tests and clippy passing.

- **Lc0's second move (lc0-second-move.md, done 2026-10-28):** Lc0's policy target is raw visit
  shares; its second choice drops 6.9% of quiet positions. Replaces Stockfish's filter, which is not
  run. The two test tars stay in `data/lc0-raw/` for the cheap check.

- **Designed and its code written 2026-10-29, see the lc0-policy entry below.** **Lc0's corpus
  converted again, with the policy (the developer, 2026-10-28), after `-quiet`'s duels:** the 275 tars downloaded again (about 16 h unattended) and kept; each comment carries
  every legal move's visit count (mcts-nnue.md's format), for MCTS's generation 1 policy teacher
  and any second-line filter. To design first: the converter's comment, the PGN reader accepting
  it, and whether the existing segments are replaced or a new corpus sits beside them.

- **Lc0's data for value and policy networks (lc0-policy.md, designed 2026-10-29, no plan yet):**
  assuming `-quiet` succeeded (it failed, and the filter is removed since 2026-11-02, so the quiet
  `lc0-value` buckets are dropped and value networks read `data/lc0-counts` unfiltered,
  lc0-counts-forges.md), buckets by the reader's kind: `lc0-value` (today's `lc0-quiet`
  renamed, quiet positions) for NNUEs without a policy head, `lc0-policy` (every position, with
  visit counts) for NNUEs with one, `lc0` kept temporarily; transformers stream the corpus. Sizes
  measured 2026-10-29 (corpus 4.85 to 21.45 bytes a position); plan, lc0-policy-implementation.md.
  Phase A's code done 2026-10-29: the reader keeps visit counts when asked, the Convert fills
  `data/lc0-counts` with them (the forges read it from 2026-11-02). **The full Convert started
  2026-10-30, during `-quiet`, resumed 2026-10-31, restarted the same day with the cap lowered to
  24 GiB** (complete: 362 tars, 1,199,290,155 training positions; not a superset of the old
  corpus's 275 tars, about 48 of them, 5 to 12 January, skipped as slow and 135 later ones in
  their place, the test tars the same); the by-hand comparison of the first tar's first segment with the old
  corpus's passed the same day. `lc0-value` (step 4) dropped with the filter (2026-11-02).

- **After `-quiet` ends, if it works (the developer, 2026-10-29):** **`-quiet` failed (2026-10-30,
  confirmed 2026-10-31):** the entries below assumed it worked. **Moot 2026-11-02: the quiet
  filter, `Keep::Quiet` and its second move removed from the code;** kept as a record. Only the
  configs' clean-up survives, without the filter as a default.
  - **Configs cleaned up:** settled experiments' configs retired (`configs/retired/`, their runs
    kept on Drive), the quiet filter the default for Lc0 value networks (no `filter:` key), and the
    line's winner a base config under a short name, each new experiment the base plus one change.
    Which configs are settled, and the base's name, are the developer's to say then.
  - **The quiet filter takes the second move (the developer, 2026-10-29; written the same day, to
    build and test):** `Keep::Quiet` itself, not a new filter, also drops a position whose search's
    second move, the most visited after the best by the comment's visit counts, is a capture or a
    promotion; no threshold, any visit counts (about 6.9% more dropped, lc0-second-move.md). Without
    counts the best move alone decides, so `-quiet` on `data/lc0` is unchanged. **Before a forge
    reads `data/lc0-counts`:** the quiet buckets must not be `data/buckets/lc0-quiet` (the old rule's):
    `lc0-value`, a new hard-coded constant, as planned (lc0-policy-implementation.md, step 4); the
    frozen set lives under the corpus's root, so the new corpus freezes its own. (Moot since
    2026-11-02: the forges read `data/lc0-counts` unfiltered, lc0-counts-forges.md.)

- **One smithy, on any machine (smithy-anywhere.md, smithy-anywhere-implementation.md):** steps 1
  to 6 done 2026-10-29: no `--dev`; the GPU decides full runs, scaled runs (1/1024) on any machine
  from the page, saved to Drive and marked scaled, their networks never installed nor tested;
  `data/gen` renamed `data/networks`; leases on Drive (forge runs with Take over on the page, tunes,
  PeSTO's play, the Convert, a source's first freeze); a run resumed from Drive in any smithy, its
  files copied up at release and every 30 minutes of training. Left: step 7, reproducible buckets,
  no longer waiting on `-quiet` (removed 2026-11-02); step 8, checks across two machines and the lease timings against Drive's sync.
  **The lease's 60 s wait: superseded 2026-11-01 by lease-over-http.md** (no wait and no file on
  Drive: the other smithies asked over HTTP once at a start, a job refused while held elsewhere or
  for 2 minutes after its stop there; no take over; written, to compile and check). The history
  below kept.
  **The lease's 60 s wait (the developer, 2026-10-29, annoying):** to discuss, not designed. Proposed:
  the read-back in the background, the job started at once and stopped if another token is found (a
  resume writes nothing to Drive in its first minute); no wait when the lease found is this
  machine's own earlier boot's. The wait narrows the race, not closes it (Drive's sync may take longer).
  **Meanwhile the page says nothing (the developer, 2026-10-30):** for the 60 s the job runs with no
  work, so its box reads as stalled. The wait is `lease::take_as`'s `sleep(timing.sync)`, behind all
  five callers (a forge run, a tune, PeSTO's play, the Convert or Book slot, a source's first freeze).
  **Done 2026-10-30, to build and check:** `take_as` records in a static of `lease.rs` the job's name
  and when the read-back is due, set before the sleep and cleared after; the status JSON carries it
  (`lease_wait: {job, seconds_left}`); the "Running now" box says "Waiting for the lease of <job> to
  sync on Google Drive: N s left", counting down at each poll. No caller changes. Moot if the wait goes into
  the background.

- **The Convert's slow downloads reconnected (slow-download-reconnect.md, implemented 2026-10-30):**
  tests, clippy and a Convert run to check. storage.lczero.org sometimes serves a connection at about 40 KiB/s for hours while a new
  one gets 1.5 MiB/s (2026-10-29 and 30: Wi-Fi switched off and on, the tars started over and got
  fast). Proposed: curl's `--speed-limit 262144 --speed-time 60`, its retries moved into `download`'s
  own loop, a slow abort (error 28) restarted without limit, other errors keeping the 5 retries; no
  byte ranges, so each restart begins the tar at 0, still far quicker. Open: the threshold, a
  reconnect count on the page. Fix beside it: the comment at `fetch_tars`'s spawn saying the next run
  continues its `.tmp`, untrue without byte ranges.

- **Arena contention (arena-contention.md, done 2026-10-29):** `machines.yaml` one ranked list;
  arenas claimed only while a job deals games; full over scaled, then the higher rank; Install claims
  its arena. Built, tested, checked on two machines; in `done/` with its plan.

- **`rotor bench <depth>` (2026-10-28):** the bench to any depth, the default still 9 (3,117,647
  nodes unchanged), tests and clippy passing; committed.

- **The 512 run resumed 2026-10-27** on a smithy built with indicatif's removal (indicatif-removal.md,
  step 4): whether the frozen set's read showed as a live bar on the page at the resume, to confirm.

- **The repository went public 2026-11-05 (temper-games, one first commit).** What remains below is
  the first binary release's.
- **Going public, not yet (the developer, 2026-10-24); not before the width 512 experiment
  (psq-threats-screlu-512.md) has settled which network ships, nothing wider tried, and that
  network is SPSA-tuned; then `0.1.0`, and the version bumped to `0.2.0` (the developer,
  2026-10-27):** the first binary release, a `.exe` on
  GitHub, makes the repository public. The new repository, restarted from one first commit with the
  old one archived privately (its history keeping the reasoning behind the decisions), is this one,
  still private. Before it goes public:
  - **The working tree clean** (CLAUDE.md, "Never leak secrets"): no schedules or habits of the
    household (a first sweep by text search done 2026-10-24, two archived designs removed and every
    hit reworded; a reading pass over `claude/` done, since a search misses rewordings), no paths
    beyond the data root (the TODO names Drive paths), no credentials or addresses in tracked
    files, the gitignored files' committed examples holding placeholders; the commit's author email
    chosen on purpose. A read-only search of the tree first, every passage listed for the
    developer to decide. **Search done 2026-10-25:** no machine names, addresses, e-mails or
    credentials tracked; the hardware and machine-count passages already written stay as they are
    (the developer, 2026-10-25: relaxed for the past only, CLAUDE.md's rule still binding every new
    writing). The text the first sweep's replacement had mangled reworded the same day, with no
    machine name back, even as a common word. The reading pass was done before the repository was
    restarted from its first commit (the developer, 2026-10-27).
  - **`claude/` public too,** design notes included (decided, the developer, 2026-10-25: the
    project cannot be developed without it).
  - **`.gitattributes` in the first commit,** so line endings are stored right from the start.
  - **AGPLv3 for a binary:** `LICENSE` in the release, the release pointing to its exact source (a
    tag), and the notices of every dependency built into the `.exe`. **Audited 2026-10-25 from
    `Cargo.lock` and the registry's license fields:** every crate of the workspace, the dev crates
    included, is AGPLv3-compatible (no GPL-2.0-only, nothing proprietary); the public repository
    holds no third-party code, so `LICENSE` suffices for it. The `.exe`'s own tree: `shakmaty` and
    `shakmaty-syzygy` GPL-3.0-or-later (both credited, not the prober alone); the ICU stack
    Unicode-3.0; `tracing`, `tracing-core`, `unsafe-libyaml` MIT only; the rest MIT or Apache, plus
    Zlib, BSL and Unlicense options needing nothing. `pgn-reader` (GPL) is a dev-dependency of the
    engine only. The notices generated at each release by `cargo-about` (an `about.toml` of accepted
    licenses), not kept by hand. The HTTP code is in its own crate since 2026-10-25
    (lan-http-crate.md), so `tiny_http`, `ureq`, `url` and the ICU crates are out of the engine's
    tree. The C runtime static in the release build alone (binary-release.md, decision 2).
  - **The release itself: binary-release.md (plan, 2026-10-25),** debugged as a draft in this
    repository, which is already the new one meant to go public, still private. Its history was
    squashed again, so the passages reworded on 2026-10-25 are gone from it (the developer,
    2026-10-27; the plan's step 6).
  - **The embedded network's data: checked 2026-10-25.** Lc0's training data is under ODbL 1.0, its
    records under DbCL (`storage.lczero.org/files/training_data/LICENSE.txt`, no licensor named;
    lczero.org says nothing on networks trained on it). Read cautiously, a network is a Produced Work
    (ODbL 1.0, 4.5(b)): it may ship under any terms, AGPLv3 included, but needs a notice (4.3). Since
    it comes from our converted corpus, a Derivative Database, 4.4(c) and 4.6 also apply: the corpus
    falls under ODbL and recipients must be offered it or "the method of making the alterations",
    which the public repository's converter is (`crates/smithy/src/lc0/`, run from the page; named by
    its source, not by a command, since the command line goes). At the release, beside the
    `cargo-about` notices and in README, a notice such as: "Rotor's network was trained on positions
    from Leela Chess Zero's training data (test91, storage.lczero.org), made available under the
    Open Database License (ODbL) 1.0; the conversion is Rotor's own, in `crates/smithy/src/lc0/`."
    **The DbCL read 2026-10-27:** its one condition (2.2) is to comply with the ODbL, so it adds
    nothing of its own. The notice is in README's Acknowledgments since 2026-10-27; the release's
    notices carry it through `about.hbs`. No note to the Lc0 team (the developer, 2026-10-27): the
    notices are what the license asks for.
  - **CPU support: one build, decided (the developer, 2026-10-25):** no support for older CPUs;
    the requirement named x86-64-v3 and the build compiled for it, and a clear refusal at startup
    instead of an illegal-instruction crash (x86-64-v3.md, implemented 2026-10-25, the bench's nodes
    unchanged, the refusal accepted untested).

- **Loss on Rotor's own positions (rotor-positions-loss.md, 2026-10-28): the distribution shift
  likely, not proved.** pairwise_hidden, 7.8% below SCReLU on Lc0's test positions, is 2.4% +/- 2.1%
  above it on the positions of their own depth 9 duel from 7 pieces up (`engine_loss::tests::duel_loss`,
  labelled by the games' results). But the control, plain pairwise (SCReLU +36 Elo, shown stronger),
  moved the same way, by about 2 points against pairwise_hidden's 10: scored on its own duel's games, a
  network is biased toward the winner. **The clean test (same day), on a phase against count duel's
  games:** both pairwise networks level with SCReLU from 7 pieces up. pairwise_hidden's Lc0 lead does
  not carry over, but fit explains neither lost duel: the cause is in the search (sibling ranking,
  loss-and-strength.md). **Calibrated the same day:** threats 3.2% +/- 0.8% below psq on the same
  games, so the measure sees about +51 Elo and "level" is level. **Then the cause, found
  (sibling-ranking.md, 2026-10-28):** ordering every move's child as Stockfish does, at quiescence,
  ranks SCReLU, threats, pairwise, pairwise_hidden and psq in their duels' order, about 15 Elo a
  point. pairwise_hidden orders best statically, worse after the captures. **Width 512 is screened by
  it first** (`sibling_ranking`, about 18 s against the cached Stockfish pass), then its duels.
  **settled-positions.md** (training on the positions the search evaluates): what Stockfish does
  checked 2026-10-28, recorded there; it filters to quiet positions, its first step here
  (quiet-filter.md), the leaf options waiting for its result. The Stockfish cache lives only on the machine that made it (`data/sibling-ranking/`,
  gitignored): another machine reruns `sibling_reference` first.

- **`lc0-psq-r1`, generation 1, done (2026-10-23):** moved into `run-1/` by hand and resumed on the
  new code at round 104 of its training, which read the old build's state as it was. Round robin won
  by 0.03, flat within noise; gate against PeSTO passed in 70 games at 75.8% (direct-blend.md,
  "Results of the fine grid"). The run waits at generation 2's self-play, phase `play`. **Next:** the
  duel of its winner against `lc0-psq-0001-0.15`, if ever, by the deprecated `arena duel`: the
  page's tests offer the smithy's networks only, and keep doing so (the developer, 2026-10-23);
  `lc0-psq-threats` runs (above). Generation 2 only
  if wanted: its Resume plays self-play now.

- **`lc0-psq-threats-r1`, generation 1, training (started 2026-10-23, on the training machine):** the first run
  on the tournament chores and the event log below. At round 60 every lambda was in stage 2 of 5;
  training estimated to end 1 to 1.75 h later, then the round robin (about 65 min) and the gate.
  **Next:** its winner's duel against `lc0-psq-r1`'s, from the page's tests, then
  `lc0-psq-threats-pairwise`.

- **The terminal is a log of events (terminal-log.md, done 2026-10-23):** `smithy start` and `arena
  start` log through `env_logger` to stdout, UTC timestamps, no bars, tables or gauges; their bars
  are still made, drawn nowhere, for the page. The direction (the developer, 2026-10-23): the page
  does everything, install included, and every other command is deprecated, left as it is until the
  page replaces it. **Next:** install from the page; no detach mode until a log file is wanted.

- **Books and install from the page (page-install-and-books-implementation.md, paused 2026-10-26
  after step 7):** the Books screen makes a source's book end to end (tested on a 1,500,000-game cut
  of Lumbra; `fit` needs about a million games or more). The Install screen asks a claimed arena to
  build and install; `smithy install` is gone. Tested on a dev smithy: only `pesto` installed.
  **Open, before it is trusted:**
  - **A dev install writes the real install.** The arena installs into the data root's `bin/`,
    lichess-bot's, whichever smithy asks: a `--dev` smithy's install replaces the real engine,
    network and book. **Refused (the developer, 2026-10-27):** a dev smithy installs nothing, the
    smithy answering its Install with a 403 and the page leaving the Install box out; installs are
    tested on a real smithy only.
  - **Why only `pesto`:** find out whether a network install fails on a dev smithy alone (its runs
    in `data/smoke/`, its store a stand-in) or on any.
  - **The user experience:** the page said little while the install ran; the click's answer and the
    `running` line came first in this pass, more to do (the build's progress, a clearer end).
  - Then steps 8 (usage texts, DEV.md's book and install sections, this item) and 9 (the checks,
    lichess-bot starting from an install made from the page). Arena's `zstd` dependency's comment
    still names `book.rs`, which moved to `smithy`.

- **indicatif goes, graphical bars take its place (the developer, 2026-10-23, general direction):**
  gradually, every progress shown as a bar on the page, none on a terminal. Today the page's live
  bars are indicatif bars drawn nowhere (`progress::register`, `progress::live`), so the first step
  is a counter of the `progress` crate's own (position, length, start, elapsed) that the page reads,
  in place of `ProgressBar`; then each deprecated command's bars go with the command, as the page
  takes its job over; indicatif leaves the workspace with the last. **Done so far:** the round
  robin's text bar (pairs-bar.md); training's, a round's two halves and the frozen test set's read,
  now a plain record the training box draws (round-in-flight.md, 2026-10-23); then every text bar
  left on the page (no-text-bars.md, 2026-10-23): self-play's and the tune's gone, their boxes
  saying it, every other live bar (lc0's conversion, scattering, the frozen set's writing) drawn
  graphically from its `LiveBar`. **indicatif out of the workspace (indicatif-removal.md,
  2026-10-27):** `progress::Bar`, the crate's own counter behind the same `LiveBar`, no bar drawn on
  any terminal, the training's dead terminal bar gone. Tests and clippy passing; built once the 512
  run ends, then the page's bars checked at the next scatter or frozen set's read.

- **The frozen test set's read in a box of its own (the developer, 2026-10-27):** a resumed
  training showed it as a line of the training box, a starting one as a live bar of its own (the
  sample's). Now `freeze::read` reports through a live bar too, "reading the frozen test set", and
  the training box's `reading` case is gone. Done, to check at the next resume.

- **Tests from the page (tests.md, built and tested 2026-10-23):** a duel, a match or
  an SPRT between any two of the smithy's networks or PeSTO, as a job, at a depth or under a clock,
  each side its tune or the defaults (`mixed` marked); the latest 20 kept in `data/tests/` with their
  games. The gate's SPRT is now the general elo0/elo1 form at `(0, elo_d)`.

- **Tournament chores (tournament-chores.md, built and tested 2026-10-23, not yet run for real):** the
  round robin played one pairing at a time on each arena; now a `tournament` chore spreads its pairs
  over the pairings behind and the arena interleaves them, the tune's chore is `spsa`, the live
  standings rank by score, and no pairing is recorded past its `wanted` (an arena lost and back).
  The gate shows its LLR as a gauge between its bounds, on the page. A protocol change: every arena
  is pulled and restarted with the smithy before the next run. First real use: `lc0-psq-threats-r1`'s
  round robin.

- **The smithy claims its arenas (discovery-and-heartbeats.md, the developer, 2026-10-23):** the
  smithy on any machine, arenas started by hand and claimed from `data/machines.yaml`, polled every
  10 s (late 30 s, lost 60 s), an outbox in memory; `--dev` itself superseded 2026-10-29 by scaled
  runs on the page (smithy-anywhere.md); `smithy start --dev [--scale <s>]` replaced
  `smithy smoke` and the `--local` item below. Built, tested, and `--dev` checked on another machine
  (2026-10-23); the training machine and the Lichess machine claimed and played together in `lc0-psq-r1`'s round robin the
  same day, the Lichess machine once its firewall rules were in (DEV.md). Another machine not checked yet. `data/smithy.yaml` is gone: every item below naming it predates
  this.

- **The forge clean-up, then HTTP, both on 2026-10-22 (the developer, branch `dario`):** first
  forge-cleanup.md (`smithy server --forge` replaces the old pipeline, cut by reachability; steps 1
  to 5 done, step 6 the docs, then the real run of `lc0-psq` through the server on the training machine, see its
  "Progress"). **Every item below that names `smithy forge`, `teacher`, `freeze`,
  `promote`, `gen-server` or `loop-server`, `arena self` or `select`, or AB(lambda) and its cut
  predates the clean-up,** as does every config other than those in `configs/` (`lc0-psq`,
  `pesto-psq`, `lc0`, `pesto`, and since 2026-10-22 `lc0-psq-threats` and
  `lc0-psq-threats-pairwise`; the rest are in `archived/configs/`): those commands are gone, a run goes through `smithy server start --forge
  <gen-config>` with `arena client start`, and every label is the direct blend. The configs that
  nothing reads any more carry a RETIRED header, each to be reviewed by the developer. HTTP's first
  step is built and tested (http-transport.md): HTTP/1.1 over `tiny_http` and `ureq`, discovery by
  `data/server.yaml` or `127.0.0.1`, `start|stop|status` for server and client, no Drive in the
  protocol. Then orders.md, the same day: work dealt as orders with a counter, every game or pair
  sent as it ends as PGN, self-play appended into segments, and SPSA as `smithy server start --spsa`
  (`arena spsa` removed); built and tested, orders and resume run on the Lichess machine. **Every item below that
  names `arena spsa`, `--serve`, `--client` or `data/spsa/server.txt` predates it.** Next: the real
  run on the training machine, and a first tune over the server. The read-only status page (status-page.md) is
  served at `http://<server>:47821/`, and stays up after a job ends until a stop. Orders are now
  chores, and training runs and tunes jobs (chores.md): `/chore`, the `Chore` tag, `c` ids,
  `chores.txt`. Then smithy-and-arenas.md: `smithy start|stop|status` and `arena start|stop|status`,
  jobs (a forge or a tune) started and stopped from the smithy's page, tasks instead of units.

- **The Lc0 runs, one at a time (the developer, 2026-10-22):** on the current forge, each
  generation 1 alike (direct blend, lambdas 0.01 to 0.09, Lc0's 1B positions), each compared with the
  one before by a duel of the winners: `lc0-psq` (generation 1 done 2026-10-23, winner 0.03), then `lc0-psq-threats` (threats against
  psq, the old pipeline's +70 Elo to confirm), then `lc0-psq-threats-pairwise` (the pairwise products
  against threats, its only fair baseline). Threats beat psq in their duel (2026-10-23, test 2:
  268 to 200 of 468 games at 10+0.1, about +51 Elo, a stopping rule's estimate, so likely high);
  pairwise started the same day, its rounds about 7% slower than threats', its test loss about 1%
  lower at round 67. **Pairwise against threats (2026-10-23): no gain, dropped.** Its winner,
  lambda 0.03 (gate passed in 42 games, 88.1%), had a test loss about 0.5 to 0.7% below threats'
  winner (0.02), the grid's gain growing with lambda (0.6% at 0.01, 1.1% at 0.08). Test 3, duel at
  10+0.1: 452.5 of 972, 46.6%, about -24 +/- 20 Elo, drawn (curtailed). Test 4, duel at depth 9:
  451.5 of 924, 48.9%, about -8 +/- 20 Elo, drawn (curtailed). At equal depth no measurable gain; on
  the clock its speed costs about 16 Elo. **Threats stays the line**, `lc0-psq-threats-r1-0001`
  the best network. Lesson: a 5% lower loss gave +50 Elo (threats against psq), a 0.6% one nothing
  visible; small loss gains are no evidence of strength.

- **Tomorrow, in order (the developer, 2026-10-23):** the renaming of networks and players
  (network-names.md), then SCReLU (below).

- **Network names renamed (network-names-implementation.md, 2026-10-24, on another machine):** names are now
  `<config>-g<g>[-a<a>-l<lambda>]-r<n>`, a player always followed by `@default`, `@t1` or `@<cp>`.
  Generation folders are `gen-<g>/`, unpadded. `smithy migrate-names` run on every machine,
  then deleted (2026-10-24). Every item below naming a network `-NNNN` or a `gen-NNNN/` predates
  this.

- **The new pipeline against the old networks, by SPRT with cutechess-cli (the developer,
  2026-10-23, later that day or the next):** `lc0-psq-threats-r1-0001` (lambda 0.02, on Drive:
  `G:/My Drive/rotor/data/gen/lc0-psq-threats/run-1/gen-0001/attempt-1/lambda-0.02/candidate.nnue`)
  against caroline-0001 (`data/networks/caroline/gen-0001/champion.nnue`), then amanda-0002
  (`data/networks/amanda/gen-0002/champion.nnue`). arena and the smithy cannot name the old networks
  since old-networks-cleanup.md, but the engine still loads them through `EvalFile` (their `psq` and
  `psq_threats` architectures are still supported), so cutechess-cli plays them: `rotor.exe` on both
  sides, a different `EvalFile` each, 10+0.1, one thread, 64 MB hash, the UHO book from the data root,
  `-sprt elo0=0 elo1=10 alpha=0.05 beta=0.05`, colour-reversed pairs (`-repeat -games 2`),
  concurrency 6 on the Lichess machine. **Caveats:** both sides play the default search parameters (the old
  networks' `margin.txt`, caroline's tuned 100 cp, is retired and ignored; no `parameters.txt` on
  either side); an idle machine (the Lichess machine when Lichess leaves it idle, or the training machine after its training);
  threats on Lc0 against caroline was already about +70 Elo by duel (2026-10-19), so this mostly
  checks that the new pipeline kept it. **Against caroline, done on the Lichess machine (2026-10-24):** both
  untuned, the files copied into `data/sprt/threats-vs-caroline/`. A fixed match stopped after 20
  games, 13-1-6, +241 +/- 159; then `-sprt elo0=0 elo1=35`, **H1 accepted** in 47 games, 23-5-19,
  +140 +/- 80, LOS 100%. The pipeline kept it. Its 5 "No result" games were no crash: the last five
  in the PGN, 0 to 26 s long, the games in flight (concurrency 6) cut off when H1 was accepted.
  **Done:** amanda-0002 not played, weaker than caroline (the developer, 2026-10-24). **Deployed on
  Lichess the same day:** `smithy install lc0-psq-threats-g1-r1` on the Lichess machine, from Drive (lambda
  0.02), untuned, the book kept (`LumbrasGigaBase_OTB_ELO2400/book-n9-points.bin`).

- **SCReLU, an experiment after pairwise (the developer, 2026-10-23):** the squared clipped ReLU,
  each element squared on its own, the usual activation of single-layer networks (bullet's engines).
  Pairwise pays in Stockfish partly by halving the input of its first hidden layer; ours has none
  (`hidden_dim: []`), so only its quadratic terms help, and SCReLU gives such terms too. A config
  differing from `lc0-psq-threats` by its activation alone, dueled against threats (pairwise
  dropped, 2026-10-23). Needs the activation in the architecture (training and the engine's
  inference); the delicate part is the integer inference, a squared value having another scale.
  **Still worth a try after pairwise's null result (2026-10-23), expectations lowered:** unlike
  pairwise it has a record in single-layer networks (bullet's engines), squares each value on its
  own rather than multiplying two, and should cost less in the engine (the output reads the same 512
  values; value times weight times value). **Judged as pairwise was**, its loss alone no evidence:
  a duel at depth 9 against threats (the evaluation alone), then, if at least level, one at 10+0.1
  (its speed cost). **Designed 2026-10-24 (screlu.md):** psq_threats with no hidden layer only,
  `psq_threats_screlu`, its integer form pairwise's arithmetic with a lane times itself; the head
  benched before training. **Implemented 2026-10-24** (engine, bench, trainer, config
  `lc0-psq-threats-screlu`); the bench found no measurable cost. **Done 2026-10-24, kept:**
  generation 1 won by lambda 0.02 (test loss 0.065127, about 1% below threats' 0.06592, the gap
  opening only at the last rates), gate against PeSTO passed 38.5 to 5.5 in 44 games. Against
  `lc0-psq-threats-g1-r1`, both `@default`: test 5, depth 9, 488.5 of 940, 52.0%, drawn (curtailed);
  test 6, 10+0.1, 480.5 of 932, 51.6%, drawn (curtailed). Both lean the same way, about +12 Elo
  together, at the edge of significance; no SPRT (the developer). SCReLU is the line; its
  generation 2 waits for Resume.

- **SCReLU with material sets (psq-threats-screlu-material.md, the developer, 2026-10-24):**
  `lc0-psq-threats-screlu-material`, SCReLU's output replaced by the 78 material sets of
  pairwise-material, factorised. Implemented the same day (engine tag 13, bench, trainer, config),
  tests passing; the bench found no measurable cost. Generation 1 beat SCReLU by about 1.3% in test
  loss, but its winner (lambda 0.07) drew both duels against `lc0-psq-threats-screlu-g1-r1`, both
  `@default`: test 7, depth 9, 451.5 of 934, 48.3%; test 8, 10+0.1, 452 of 908, 49.8%. Generation 2
  parked. Still open: the lambda 0.02 candidate's duel, in case lambda hid a gain.

- **SCReLU with phase sets (psq-threats-screlu-phase.md, the developer, 2026-10-24):**
  `lc0-psq-threats-screlu-phase`, the material sets' 78 buckets replaced by PeSTO's game phase, 25
  buckets, on the hypothesis that 78 are too many. Engine tag 14; the batches carry the phase as a
  fourth bucket column. Implemented the same day, tests passing, no measurable cost in the bench.
  **Generation 1 done 2026-10-25:** won by lambda 0.01, the grid's edge (test loss 0.063836, about
  2% below SCReLU's 0.065127 and below material's too; the three lowest losses at 0.01, 0.02, 0.03,
  in order), the round robin flat (0.01 50.9% +/- 1.0%, every other lambda 48.9% to 50.1%), gate
  against PeSTO passed 37 to 5 in 42 games. Against `lc0-psq-threats-screlu-g1-r1`, both
  `@default`: test 9, depth 9, 488 of 940, 51.9%, drawn (curtailed), about +13 Elo, leaning the
  same way as SCReLU's own tests. Test 10, 10+0.1: 451.5 of 942, 47.9%, drawn (curtailed), about
  -15 Elo. The two together about level: like material's, its lower loss shows no measurable
  strength. **Its loss by piece count (`pieces.tsv`, lambda 0.01, each run's last round), against
  SCReLU's:** -9% to -34% at 3 to 6 pieces, -2% to -4% at 7 to 13, -0.5% to -1.5% from 14 up, -2.2%
  overall; at round 19 it was still 1% to 2% worse from 14 up (each bucket sees part of the data),
  and it trained 217 rounds to SCReLU's 146. A gain of a few Elo, mostly in endgames, would explain
  the drawn duels: about +/- 20 Elo cannot see it, only an SPRT with narrow bounds can. Published
  evidence is thin (searched 2026-10-25): output buckets by piece count are common on 768-input
  single-layer networks, but the only isolated A/B found is NoaChess's, on halfk, +20 +/- 14 Elo;
  Ransom's (768 inputs) found their gain mostly in endgames. Whether buckets help halfk as much, or
  less in endgames since halfk sees the kings, needs its own experiment.

- **SCReLU with count sets (psq-threats-screlu-count.md, the developer, 2026-10-25):**
  `lc0-psq-threats-screlu-count`, the phase's 25 buckets replaced by Stockfish's key, `(count - 1) /
  4` over every piece, 8 buckets. Engine tag 15; the batches carry the count as a fifth bucket
  column. Implemented the same day, tests passing, no measurable cost in the bench, a dev smithy
  smoke run clean. **Generation 1 training since 2026-10-25,** about 105 s a round at the start, as
  phase's. At round 19 (lambda 0.01, rate 3e-3), against SCReLU: +0.38% overall, its endgames ahead
  (-6% to -36% at 3 to 6 pieces) but behind phase from 5 to 13 pieces, where its coarse buckets lump
  every ending together. **Generation 1 done 2026-10-25, parked:** won by lambda 0.03 (test loss
  0.064706, about 0.6% below SCReLU's, phase 2.0% below), the round robin flat (0.03 51.1% +/- 0.9%,
  every lambda 49.3% to 51.1%), gate against PeSTO passed 42.5 to 9.5 in 52 games. **Its loss by
  piece count (`pieces.tsv`, each run's last round), against SCReLU's:** -26% to -36% at 3 and 4
  pieces, -9% to -11% at 5 and 6, -3.5% at 7, within noise from 8 up; lambda 0.01 the same within
  noise. Phase is as good or better at every count but 4, clearly at 8 to 13 (-1% to -5% against
  count's nothing): count's coarse buckets (5 to 8 pieces in one) cannot tell endings apart. Stockfish's
  choice of count says little here: its bucket picks a layer stack that can read material from the
  accumulator, on halfka inputs that already see the kings; ours has no hidden layer, so the bucket
  is the only place the stage enters. Both `@default`: test 11, against SCReLU, depth 9, 452 of
  906, 49.9%; test 12, 10+0.1, 452 of 936, 48.3%; test 13, against phase, depth 9, 476 of 928,
  51.3%; test 14, 10+0.1, 463 of 916, 50.5%; all drawn (curtailed). SCReLU, phase and count are
  within about +/- 10 Elo of one another, consistently: buckets on this architecture add nothing a
  duel can see. Worth retrying against phase only on halfk inputs or with a hidden layer.

- **SCReLU with a taper (psq-threats-screlu-taper.md, the developer, 2026-10-25):**
  `lc0-psq-threats-screlu-taper`, two output sets, middlegame and endgame, blended by PeSTO's phase
  instead of one set picked per bucket; not factorised. Engine tag 16, about 3% per node with a fused
  kernel; the batches' phase column, no new one. Implemented the same day, tests passing, a dev
  smithy smoke run clean. **Next (count parked 2026-10-25), on the training machine:** generation 1, then its winner against
  `lc0-psq-threats-screlu-g1-r1` and the phase winner, both `@default`, at depth 9, then 10+0.1 if
  at least level. **Predicted (2026-10-25), from phase's loss by piece count:** between SCReLU and
  phase; no early lag from 14 pieces up (both sets see every position), less gain than phase at 3
  to 8 (one endgame set for every ending); its 3% a node about 2 to 3 Elo, beyond what a duel can
  see. Judged by its loss by piece count against SCReLU and phase, then an SPRT if worth keeping.
  **A follow-up if it does well (the developer, 2026-10-25):** the taper blended by material on the
  78 scale (1/3/3/5/9, pawns included, clamped at the top) instead of PeSTO's 24: pawn endgames,
  which PeSTO's phase puts all at the endgame end, then graded by their pawns; finer steps; the
  same two sets and the same cost. **Generation 1 training since 2026-10-25:** round 1 (lambda 0.01)
  92.6 s, as SCReLU's 91.4 (phase 98.8); test loss 1.7% below SCReLU's, phase 5.6% below; -14% to
  -19% at 3 to 5 pieces, level with SCReLU from 14 up, as predicted. **Round 24 (2026-10-25), still
  at 3e-3:** test loss 0.19% above SCReLU's (mean of rounds 22 to 24), phase 0.16% below; the
  endgame gain gone, level with SCReLU at every piece count within noise (3 pieces -5.5%, 4 +3.9%),
  where phase keeps -13% to -27% at 3 to 5. Heading for a null; the verdict waits for the end.
  **Done 2026-10-26, a null, parked:** lambda 0.03 promoted (round robin flat, gate 36.5 to 3.5
  in 40 games); best test loss 0.065053, SCReLU's within 0.1%; by piece count -8% and -5% at 3 and
  4, nothing from 5 up, where phase keeps -2% to -5% to 13 pieces. Both `@default` against
  `lc0-psq-threats-screlu-g1-r1`: test 15, depth 9, 452 of 914, 49.5%; test 16, 10+0.1, 451.5 of
  922, 49.0%; both drawn (curtailed). Hypothesis: SCReLU's squares can build a term linear in the
  phase, so a linear blend adds only a head start; the material-blended follow-up is linear too,
  dropped. Next on the training machine: kings.

- **SCReLU with king-bucketed hidden layers (psq-threats-screlu-kings.md, the developer,
  2026-10-25):** `lc0-psq-threats-screlu-kings`, the stack's head over SCReLU's squares, without
  output buckets: 512 -> 16 by our king -> 16 by their king -> 1. Isolates the stack's untested
  idea, the king in the head, now that pairwise and output buckets drew; also settles the hidden
  layer of 16. Runs only if the taper too shows nothing a duel can see; with a tapered output it
  would be `psq_threats_screlu_stack`. Designed and planned 2026-10-25; engine done (tag 17), the
  bench about 1.5x SCReLU's cost per node, kept unoptimized until the depth 9 duel says the
  evaluation is worth speeding up; trainer, export and config done 2026-10-25, tests passing.
  **Next:** a dev smithy smoke run, then generation 1 once the taper's run frees the training
  machine; its winner against `lc0-psq-threats-screlu-g1-r1`, both `@default`, at depth 9 first.
  **Dropped at round 10 (2026-10-26):** test loss 7.7% above SCReLU's, the gap growing; -14% to
  -54% at 3 to 6 pieces, +5% to +25% from 8 up: the 16-wide clipped layer likely a bottleneck for
  middlegames. Its trainer's speed tried and kept as it is (kings-trainer-speed.md, archived).
  **Successor: kings2 (psq-threats-screlu-kings2.md, the developer, 2026-10-26),** each side's 256
  squares -> 16 by its own king, both activations, -> 1, plus SCReLU's 512 -> 1 as a linear skip.
  Designed, not planned. **It takes kings' name, config and tag 17 (the developer, 2026-10-26):**
  the old kings' code removed in its plan, the dropped run's data deleted first. **Implemented
  2026-10-26** (engine, bench, trainer, export, config), tests and clippy passing; the bench 2.2x
  screlu's head, about 2.0x a node. **Next:** a dev smithy smoke run, then generation 1.
  **Generation 1 (2026-10-26):** the first try trailed SCReLU by 2% to 2.7% from round 4, the
  king layers' gradient pulling the shared transform their way; stopped at round 8. Retried with the
  squares detached before the king layers (training only, the network unchanged): 1.9% below SCReLU
  at round 4, below it at every piece count, about 159 s a round. Waiting for rounds 10 and 20, then
  the end and the duels.
  **Idea, after kings' duels (the developer, 2026-10-26): kings in two stages.** SCReLU trained
  first (its champion, `lc0-psq-threats-screlu-g1-r1`, reused), then frozen, and only the king layers
  trained on top: "the champion plus a correction", much cheaper per round (no transform backward,
  no optimizer on the transform). Needs a run started from another run's checkpoint and parameters
  frozen by name, neither in the trainer today. Worth it only if the detached kings wins its duels,
  as the cheaper way to train later generations; if it draws, the two-stage form, more constrained,
  very likely draws too. Open: whether the skip's 512 weights stay frozen or adjust.

- **SCReLU with a detached hidden layer (psq-threats-screlu-hidden.md, the developer,
  2026-10-26):** `lc0-psq-threats-screlu-hidden`, kings without the king: 512 -> 16, clipped and
  squared, -> 1 beside SCReLU's 512 -> 1 as a skip, the 16's input detached in training. Tells
  whether kings' gain comes from the detached layer alone. Implemented 2026-10-26 (engine, bench,
  trainer, export, config, file tag 18), tests and clippy passing; the bench 1.7x SCReLU a node.
  **Next:** generation 1, compared round by round with kings' and SCReLU's losses; if it matches
  kings, a duel decides which of the two to keep.

- **Pairwise with a detached hidden layer (psq-threats-pairwise-hidden.md, the developer,
  2026-10-26):** `lc0-psq-threats-pairwise-hidden`, screlu_hidden with pairwise in place of screlu:
  256 products -> 16, clipped and squared, -> 1 beside pairwise's 256 -> 1 as a skip, detached.
  Pairwise drew SCReLU with no hidden layer; with one, it halves the layer's input, its gain in
  Stockfish. Implemented 2026-10-26 (engine, bench, trainer, export, config, file tag 19), tests and
  clippy passing; the bench 1.4x pairwise a node, 15% cheaper than screlu_hidden. **Next:**
  generation 1 after screlu_hidden's, compared round by round with it, pairwise's and SCReLU's.
  **Generation 1 (2026-10-26), round 55:** 5% to 6.5% below SCReLU at the same round and lambda
  (lambdas 0.03 to 0.09, both at 1e-3), round 26 -5.9% at 0.02; lambda 0.03 at 0.0652 at its first
  rate drop, SCReLU's final 0.06513 at 0.02 the bar. Ended 5.5% to 7.9% below SCReLU at every
  lambda, gate passed, **but both duels lost** (depth 9 42.4%, 10+0.1 42.0%, tests 17 and 18).
  **The checks in psq-threats-pairwise-hidden.md's "To resume", done 2026-10-27:** quantization's
  bulk as SCReLU's, its tail about four times heavier, unlikely the cause; the pawn scale clean
  (40.5% against SCReLU's 37.9%). So betty's lesson, likely. **One last check (the developer,
  2026-10-27):** screlu-hidden's generation 1 and its duels against `lc0-psq-threats-screlu-g1-r1`;
  if it loses too, every hidden layer is dropped (the grid below, kings2) and the release ships
  SCReLU. **The quantization tail fixed the same day (hidden-beside-f32.md):** the hidden layer
  beside the output now in f32 over the whole products, the tail halved, the head about 33% slower;
  replayed at depth 9 on a dev smithy, pairwise_hidden 44.9% against test 17's 42.4%, still lost.
  screlu_hidden's run plays on the fixed engine. **SCReLU against pairwise, directly (the
  developer, 2026-10-27, dev smithy at `--scale 1`, its test 2):** `lc0-psq-threats-screlu-g1-r1`
  against `lc0-psq-threats-pairwise-g1-r1` (lambda 0.03), both `@default`, depth 9: 462 of 834,
  55.4%, about +38 Elo, won by SCReLU. With no hidden layer, SCReLU is the better quadratic
  activation, as the indirect evidence through threats suggested (+12 and -8 at depth 9).
  **The psq line stops at `lc0-psq-threats-screlu` (the developer, 2026-10-27):** no further
  architecture on psq inputs. The one check left: whether a generation 2, trained on its own
  self-play, beats `lc0-psq-threats-screlu-g1-r1` (its Resume, which plays self-play first); on
  the training machine. screlu_hidden's generation 1, the last check decided earlier the same day,
  dropped with it (the developer): every hidden layer on psq inputs is dropped, the grid below and
  kings2 included.

- **The hidden-layer grid (the developer, 2026-10-26), once pairwise_hidden's generation 1 ends,
  and only if it succeeds:** four experiments, the hidden layer beside the skip over pairwise's
  products or SCReLU's squares, its input detached or not:

  | | detached | not detached |
  |---|---|---|
  | pairwise | lc0-psq-threats-pairwise-hidden (run) | to build |
  | screlu | lc0-psq-threats-screlu-hidden (ready) | to build |

  Settles which input makes the better hidden layer, and whether the detach helps once a skip carries
  the transform's own gradient (kings without it trailed SCReLU by 2% to 2.7%, but with king buckets).
  The detach is training only: engine, file and export unchanged (tags 18 and 19). Open, to discuss
  first: how a config turns it off, a `value_head.detach: false` field (proposed, defaulting to true
  for the hidden heads, usable by kings too) or kinds of their own. Judged by loss at the same rounds
  and lambda, then duels for the best.

- **Rust 1.99.1 (rust-1.99.md, the developer, 2026-10-26):** the pin moved from 1.98.1, to share
  one toolchain with another project. Tests and clippy clean, no new lints; the network bench level
  or a little faster (incremental update about 5% to 10%, at the noise's edge). **Next:** the next
  forge run's round times against the last run's on the same config.

- **One checkpoint a lambda (best-checkpoint-only.md, the developer, 2026-10-25):** only the best
  survives each round, read from the stages, no ranking by loss; the stage bests' protection, kept
  for a round robin selection no longer plays, removed. About 45 MB an attempt instead of 300.
  Done the same day, tests passing, a dev smithy smoke run of lc0-psq leaving one checkpoint per
  lambda, the candidate's.

- **Every gate ends the job (the developer, 2026-10-22; implemented the same day in
  `run_generations`, its tests passing 2026-10-23, not yet seen in a real run):** after a gate the
  forge went on by itself
  today (`forge`'s loop), into the next generation's self-play on a pass, into another attempt on a
  fail. Instead: the decision as today (promotion, or the next attempt), the state saved at the next
  phase, and the job ends, saying what waits; the page's Resume starts the next generation or
  attempt. A fail that stops the lineage ends the run as today. With the run numbering below, after
  `lc0-psq` run 1's gate: that run, on today's build, goes into generation 2 by itself, stopped there
  by hand.

- **Every run numbered, run 1 included (runs.md, decision 2 revised, the developer, 2026-10-22):**
  `<config>-r1-NNNN`, no backward compatibility: `run_name` and `parse_run` in `players.rs`, their
  tests, and the docs naming players. Not while `lc0-psq` run 1 runs; check first whether anything
  in a run's folder records its players by name, which a rename would orphan.

- **Runs: the same config done more than once (runs.md, implemented 2026-10-22, to build and
  test):** a run is one execution of a config, in `data/gen/<config>/run-<n>/`, picked on the page
  with its config; a tune has a config (`configs/tune-*.yaml`) and a network; no retuning for now.
  Old runs at a config's top are moved into `run-1/` by hand.

- **A configuration-driven smithy (forge-configs.md, implemented 2026-10-22, tests passed, the
  the Lichess machine check to do):** `configs/` archived whole into `archived/configs/`; three forge configs,
  `smoke-psq`, `self-psq`, `lc0-psq` (since revised, below), each one file of forge keys and network sections; an Lc0 run's
  generation 1 gated against PeSTO, one attempt, then self-play; the page lists every config and
  run with View (generation and attempt), Resume, New run, and Tune for each passed generation of
  an ended run and for hand-crafted. Revises runs.md's form.

- **An analysis tool, for Mentes, a separate project (the developer, 2026-10-22):** labelling any PGN with a
  player's search, as `arena replay` did before old-networks-cleanup.md removed it.

- **Configs with their tunes, and pesto (configs-with-tunes.md, implemented 2026-10-22, tests
  passed, the Lichess machine check to do):** `configs/` is two runs, `pesto-psq` (was `self-psq`) and
  `lc0-psq` (from `lc0-psq-direct-fine` and `lc0-psq-direct`: generation 1 on Lc0's labels, nine
  lambdas 0.01 to 0.09, gated once against PeSTO, then self-play), each with `tune:`, and two
  sources, `pesto` and `lc0`; `smoke` is gone; the tune configs go; a tune's job is `--tune <player>`; `hand-crafted` renamed `pesto`
  everywhere, no backward compatibility.

- **"Corpus" becomes "training set" (the developer, 2026-10-22):** it pairs with the test set. The
  page, the log lines, the forge's banner and DEV.md done 2026-10-22; the key `training_positions`
  (`corpus_positions` still read), the `train/` folders (renamed by a forge as it starts) and the
  smithy's names the same day (config-cleanup.md). Lc0's converted games stay a corpus.

- **`gen-lc0-psq-smoke` could not run (seen 2026-10-22), fixed the same day:** every Lc0 forge
  shares the frozen test set in `data/lc0/frozen`, frozen at 4,194,304 positions, and the smoke asks
  for 32,768. A run asking fewer now reads a seeded random subset of the frozen set (`freeze.rs`,
  `subset`), not its first positions, which lean toward the first games read; asking more is still
  refused. To smoke-test on the Lichess machine: Lc0's first scatter, 5.9 minutes and 23.2 GB on the training machine
  (2026-10-22, 999,893,639 positions into 1024 buckets).

- **A running client or server blocks every other build (seen 2026-10-22):** Windows refuses to
  replace a running `arena.exe` or `smithy.exe`. `stop` and `status` now skip the build in
  `bin/arena` and `bin/smithy`, but any other command after a source change still fails while a
  client or server runs, as does `cargo build`. If it bites: build to a copy that is swapped in.

- **The scatter shows no progress (seen 2026-10-22, `lc0-psq` run 1 on the training machine):** `buckets::prepare`
  prints one line at its start and one at its end, and the page stays at "phase train" for all of
  Lc0's first scatter (5.9 minutes, 23.2 GB on the training machine), so it looks frozen; only `du -sh
  data/buckets/lc0` showed it alive. Minor at that length, but a page that says nothing for minutes
  invites a stop. With it, for the same GUI pass (the developer, 2026-10-22): the Training box has no
  bar either. Its total is unknown (patience ends each stage), so a bar over the current round's
  steps (of `steps_per_round`), the stage and patience columns saying how far each lambda is. To add:
  a block on the page for this phase with a progress bar over the positions read (rate, ETA). The
  scatter's bar (`progress::bar` in `buckets.rs`) is registered, so the page's live bars may show it
  already (2026-10-23): to check at the next scatter. Also: the scatter commits only at its end,
  so a stop part-way starts it over from nothing; committing now and then would keep its work.

- **A self-play generation's buckets outlive it (the developer, 2026-10-22):** a source's buckets
  (`data/buckets/lc0/`) are kept, shared by every forge of the source. A run's
  `data/buckets/<run>/gen-NNNN/` go only when the next generation starts training, and the last
  generation's never. To delete them once the generation is decided (its gate passed, or its lineage
  stopped after `max_failed_gates`) and at the run's end, never while an attempt may still add to
  them.

- **Superseded by smithy-anywhere.md (2026-10-29): scaled runs from the page on any smithy, no
  local-only tree.** **`--local` replaces `smithy smoke` (the developer, 2026-10-22):** `smithy --local`, on any
  machine the training machine included, is the same smithy and window with every config, scaled down (1024 unless
  `--scale <s>`; `--scale 1` a full-size debug run), on `127.0.0.1` alone, stored locally only, in
  `data/smoke/s<s>/` with its stand-in for Drive, never on Drive. A real run uses Drive and runs only
  on the machine `data/smithy.yaml` names. `arena --local` connects to `127.0.0.1` whatever
  `smithy.yaml` says, so a machine's own arena plays its local run. One smithy a machine: local and
  real share the port. Open: whether a headless form stays for scripted checks. Today `lan_host`
  refuses even the smoke on the Lichess machine and another machine, whose `smithy.yaml` names the training machine. **Done with the window
  (smithy-window.md), not on today's web page:** the window says clearly that it is local.

- **The smithy as a window (smithy-window.md, the developer, 2026-10-22, not urgent):** `smithy`
  alone starts it and opens its window, `smithy start` gone; closing the window stops the job;
  ~~`smithy --local` replaces `smithy smoke`~~ (superseded by scaled runs, smithy-anywhere.md). With
  the GUI pass above.

- **A browser GUI on localhost (browser-gui.md, draft, the developer, 2026-10-22):** after HTTP, a
  local page on `127.0.0.1` that watches runs, charts `history.tsv` and later starts and stops runs,
  beside the server's read-only LAN page and sharing its code. Five open questions, which binary
  serves it first.

- **The forge client's workers ask for their own jobs (the developer, 2026-10-21):** a client
  returns a job's results once every pair in it is played, so workers done early idle until the
  slowest pair ends. Pair jobs of 4 pairs a worker (`PAIRS_PER_WORKER`) shrink the waste; removing the
  barrier ends it: each worker its own loop, asking for a small job, playing it, sending it, asking
  again, results streaming to the live table and the gate. Not one job for the whole round robin:
  then only one client plays, nothing shows until the end, and a long job is handed out again on
  timeout.

- **Redesign `data/` (the developer, 2026-10-21):** its layout grew pipeline by pipeline. Until then
  the forge server's own files, under `data/gen/`, are the only ones on Drive.

- **Simplified self-training replaces the continuous loop (2026-10-21,
  self-training-simpler-attempt.md, self-training-simpler-implementation.md):** generational
  `smithy gen-server` (now `smithy server start --forge`), direct blend, 9 lambdas 0.01 apart centred on the last best, SPRT gate at
  depth 9, retries with more data after a failed gate. PeSTO route smoke-tested end to end; next the
  medium CPU experiment on the Lichess machine, after moving `gen-server`/`loop-*` data from the data root to
  `data/`. Generation 1 may instead train on Lc0's labels, its grid centre waiting for the
  `lc0-psq-direct` select. Meanwhile `smithy forge lc0-psq-direct` resumed on the training machine (2026-10-21),
  its select planned the same day ("How to blend" below).

- **Plan (the developer, 2026-10-21; its run is now the `lc0-psq` config's generation 1):**
  2026-10-22, the code clean-up; then `lc0-psq-direct-fine`: `lc0-psq-direct` with lambdas 0.01 to 0.09 (gen-server's generation-1
  grid), on today's stream for comparability, its winner dueling `lc0-psq-0001-0.15`. The
  hypothesis: AB(lambda)'s label noise spreads its runs, and best-of-ten harvests that luck; nine
  direct tickets near the peak buy the same luck without the noise. Not the finer AB grid rejected
  on 2026-10-18, which would have measured only run-to-run variance: here the variance is bought on
  purpose. `select lc0-psq-direct` and its duel (drawn, S 0.4994 over 904 games) are recorded
  in direct-blend.md.

- **Full shuffle (gen-prepare.md): done and committed 2026-10-22.** Every NNUE forge trains on
  compressed buckets in `data/buckets/`, shared across attempts; smoke-tested on the Lichess machine with
  `gen-psq-smoke` (a scatter, then an attempt's games added). Lc0's first scatter, and a second
  start skipping it, to see on the training machine with `lc0-psq`.

- **The tracks ahead, order not decided (the developer, 2026-10-18):**
  - **psq experiments:** experiments 1 to 4 below, on Lc0's labels, then the hidden-layer twin of
    the best.
  - **halfk experiments:** `halfk_threats`, its engine inference first (none exists), then the
    speed benchmark against the stack and the stack against it under a limited data budget
    (architectures.md).
  - **alpha experiments:** experiment 5 below, `lc0-alpha-pieces`, the transformer's value head
    (transformer-streaming.md).
  - **SPSA on improved search code:** several search parameters per network (spsa.md), with
    Rotor's own distributed `arena spsa` (distributed-spsa.md); OpenBench, tried first
    (openbench-implementation.md), was retired on 2026-10-20 once `arena spsa` ran a real tune.
    The developer's adaptive stage scheme comes later, measured against today's schedule.
    **Tune sizes, proposed 2026-10-20, at STC:** a **first tune** of 2,048 iterations x 8 pairs =
    **16,384 pairs, 32,768 games**, generous on purpose (the margins start untuned at 200, and
    correlated); a **fine-tune per new network** of 25-50% of that, warm-started at half `C_end`
    (open-bench-spsa.md's procedure): **4,000 to 8,000 pairs, 8,000 to 16,000 games**. For
    comparison, Stockfish's own search tunes (commit messages, checked 2026-10-20): about 57
    parameters in 44k games at 120+1.2 (`36db936`), about 27 in 35k at 180+1.8 (`0f01a51`), about
    7 in 149k at LTC (`c14297a`); hundreds of parameters only in their network-weight tunes (800k
    games at 5 s). **Estimated hours**, from about 28 s a game and one game a client thread (the Lichess machine
    8, the training machine 10), neither measured yet, adjudication not counted (it shortens games):

    | Tune | Games | the Lichess machine + the training machine | the Lichess machine alone |
    |---|---|---|---|
    | first tune | 32,768 | about 14 h | about 32 h |
    | fine-tune | 8,000 to 16,000 | about 3.5 to 7 h | about 8 to 16 h |

    Tunes resume after a stop, so the hours add up across sessions. To redo once the Lichess machine's games
    per hour is measured (the smoke tune) and the reference speed is fixed. Once the
    search code they tune has been improved. The pawn scale `c`, which had to come first, is in
    (2026-10-20), and so is the distributed `arena spsa` (distributed-spsa.md): `--serve` on
    the Lichess machine, `--client` on every machine, `--first` (16,384 pairs from the defaults) or
    `--fine <from>` (8,192 pairs from another player's `parameters.txt`). **The plan (the
    developer, 2026-10-20):** `--first` on hand-crafted, then `--fine hand-crafted` for every other
    network. **Revised the same day:** the real `--first` runs on psq, the root every network is
    built on, since the evaluation-based margins follow the evaluator (hand-crafted's first 824
    pairs: razoring, futility, reverse futility and null move rising, delta and both SEE margins
    falling), then `--fine` from it for the rest. Hand-crafted's tune runs meanwhile, until the cut's
    duel (`lc0-psq-cut` against `lc0-psq-0001`) decides which psq network it is; the winner is named
    `lc0-psq`, renames being free. `r_end` is Stockfish's 0.002 for every tune (adopted 2026-10-20). **To revisit after the
    first tunes:** whether SPSA supersedes `arena margin` (kept until it is proved, the developer,
    2026-10-20: then retired, and experiment duels compare both sides `--fine`-tuned or both
    `@default`); `c_end` and the pairs, from the trajectories (the developer, 2026-10-20: a later `--first` tries
    double the budget, 32,768 pairs, against this one; expected to pay off, since `--first` runs
    once and every network's `--fine` starts from it); and whether `--fine` should probe
    at half `c_end`, as open-bench-spsa.md's procedure has it (today it probes at the first tune's
    `c_end`).
  - **The move head, `_move`:** a best-move head on the best psq or halfk shape, a move-ordering
    hint for alpha-beta (best-eval-together-with-best-move.md, architectures.md).
  - **The policy head and MCTS, `_mcts`:** a policy head on the best psq or halfk shape, searched
    with MCTS (mcts-nnue.md, architectures.md). On Lc0's data it needs their policy, so the tars
    converted again with their visit distributions (the Lc0 policy item below). **Space (the
    developer, 2026-10-27):** a position with its visit distribution is many times one with a score,
    so the corpus, its scatter and their copies need far more room than today's; to size, against
    the data root's disk and Drive, in its plan.
    **GPU engines play on the GPU machine's arena alone (the developer, 2026-10-25), so no bench
    scales their clock.** To design then: the arena skips today's bench (`speed`, run for any
    clocked task carrying `reference_nps`, on the challenger's network), the smithy routes GPU
    chores only to an arena that says it can play them, how many workers that arena runs beside
    a GPU engine (a CPU opponent shares its cores), and a GPU bench of its own (playouts or
    evaluations a second) should a second machine ever play them.

- **The next network experiments, in order (the developer, 2026-10-17)**, **on Lc0's labels only
  (the developer, 2026-10-18)**, `data/lc0`. Self-play (caroline-0001's games, toward 1B positions)
  is for afterwards: the best `lc0-` shape then trains on it as `self-`, and the other `self-`
  configs stay unused. First planned on both labels, so that every shape would be ranked twice
  (the Lc0 ranking check, Evaluation). **Open (the developer, 2026-10-18): restart self-play from
  hand-crafted**, so that rotor-alone trains really alone: caroline-0001's lineage learned from
  replayed CCRL and TCEC games, which her self-play carries second-hand. Generation 1 would then be
  taught by PeSTO (`arena self hand-crafted`), and caroline-0001's games in `data/self` would not
  serve rotor-alone. To decide before her self-play takes more machine time and Drive space. **They wait for the 5 GB cap:**
  `smithy lc0` running on the training machine, 2.5 GB on 2026-10-18, expected full about 7 h
  later.
  1. `psq`: amanda's shape, for completeness. `lc0-psq`, `self-psq`: ready.
     **1b. `psq768`** (psq768.md, implemented 2026-10-18): psq without castling rights and the en
     passant file. `lc0-psq768`, `self-psq768`: ready, smithy's tests passing (the training machine, 2026-10-18). Speed
     does not decide (below the benchmark's noise); about equal strength keeps psq's 780 rows, for
     the NNUE MCTS experiment (mcts-nnue.md). **Done 2026-10-19: drawn, psq keeps its 780 rows;** psq768's code removed 2026-10-20
     (psq768-removal.md).
     `lc0-psq-0001-0.15` against `lc0-psq768-0001-0.55`, untuned, 10+0.1: 906 games, +304 -306
     =296, S 0.4989 [0.4487, 0.5513], curtailed (neither could pass its bound by 1000). Neither
     tuned for a rematch: tuning would add each run's luck to a question about the inputs alone.
  2. `psq_threats`: caroline's shape. `lc0-psq-threats`, `self-psq-threats`: ready. **Done
     2026-10-19: threats are stronger.** `lc0-psq-threats-0001-0.25` against `lc0-psq-0001-0.15`,
     untuned, 10+0.1: 228 games, +96 -49 =83, 137.5 - 90.5, S 0.6031, above the duel's bound
     (0.6023) and so resolved early. Training costs about 1.5 times psq's a round, and the engine
     plays about 22% fewer games a second (select's 15.0 against 19.2), both paid for.
     **Against caroline-0001, the same shape on self-play labels (2026-10-19):** a first duel,
     untuned at the default 200 cp a ply against her tuned 100, was stopped part-way, not read.
     `arena margin lc0-psq-threats-0001-0.25` then chose **100** (200 held against 175, lost to
     150; 150 held against 125, lost to 100), the same as caroline's, so the rematch differs in the labels alone:
     `arena duel lc0-psq-threats-0001 caroline-0001 --time 10+0.1`. **Result: Lc0's labels are
     stronger (the developer's hypothesis confirmed).** 220 games in 11 min, +99 -53 =68, 133.0 -
     87.0, S 0.6045, above the bound (0.6042): about +70 Elo for the same shape and margin, despite
     caroline's larger self-play corpus and a lineage of generations.
  3. `psq_threats_pairwise`: the pairwise products over caroline's inputs.
     `lc0-psq-threats-pairwise`, `self-`: ready (psq-threats-pairwise.md, bench to run). **Its duel
     against `lc0-psq-threats-0001`, which plays its tuned 100 cp margin (2026-10-19), only after
     `arena margin lc0-psq-threats-pairwise-0001`,** so both sides are tuned alike; or both at
     `@default`. Never a tuned side against an untuned one.
     **3b. `psq_threats_pairwise_material`** (architectures.md, the developer, 2026-10-18): 3 with
     its output layer one of 78 weight sets by material (1/3/3/5/9, pawns included), no king
     buckets. It isolates the material buckets from the stack's four changes, and decides whether
     halfk embeds them. **Design agreed 2026-10-20 (psq-threats-pairwise-material.md):** factorised
     as the stack, `lc0-` only; implemented the same day, to build and test, then
     `smithy forge lc0-psq-threats-pairwise-material` on the training machine after pairwise's run. `self-`
     networks only in a distant future, after the best `lc0-` network has emerged (the developer,
     2026-10-20).
  **Expected (the developer, 2026-10-18):** each step improves up to 3b; 4 is open.
  **The hidden layer of 16, to verify again (the developer, 2026-10-18):** amanda against betty and
  caroline against dorothy suggest the psq networks are stronger without it (architectures.md).
  Every experiment above trains without it. **Once, after them (the developer, 2026-10-18):** the
  best shape without a hidden layer gets one twin with `hidden_dim: [16]` on the same corpus,
  judged by games, so one network more settles it rather than a twin per shape. If the best is the
  stack (4), which has its own two hidden layers, the question is moot. **If pairwise (3) ties
  threats (2) (2026-10-19):** the twin is pairwise's, not threats', since pairwise's gain in engines
  is speed, halving the first dense layer's input, and with no hidden layer it has none to halve
  (its one output reads 128 products instead of 256 values, about the same work). A pairwise twin
  with `[16]` then settles both questions at once: the hidden layer, and pairwise where it can pay.
  4. `psq_threats_stack` (`psq_threats_pairwise_stack` in architectures.md): the stacked head over caroline's inputs.
     **Its question is the stack against `halfk_threats` under a limited data budget (the
     developer, 2026-10-18):** both bring the king's square in, the stack in the head (factorised,
     a few thousand weights a bucket), halfk_threats in the input (45k rows a perspective, each king
     square seeing about 1/64 of the data). Which pays at a given corpus size, and the stack was
     devised to be the faster. A must-do speed benchmark of the two (architectures.md) needs halfk
     inference in the engine first, which does not exist. `lc0-psq-threats-stack`,
     `self-`: ready (psq-threats-stack.md, bench to run).
  5. The transformer, value head only, no policy, zelda's shape as `lc0-alpha-pieces` and
     `self-alpha-pieces`: streams (transformer-streaming.md, implemented 2026-10-17). No unit test
     covers a streamed transformer's whole `forge`: its first real run is the check.
  **Names (the developer, 2026-10-17):** an experiment is named by its labels, `lc0-` or `self-`,
  then its architecture; nothing is renamed, so amanda, caroline and zelda keep theirs. Every
  `self-` network's generation 1 is taught by caroline-0001, from random weights; its own
  `self-psq-threats` beside caroline's generation 2, which trains the same shape from her champion.
  greta (psq_pairwise) and hilda (psq_stack) were never trained and are dropped, folded into 3 and
  4; zelda never ran, replaced by 5; betty and dorothy stay. Every experiment rounds as zelda: 4096
  steps a round, patience 6. Every other evaluation idea waits for the experiments' results, among
  them **king buckets in the input** (discussed 2026-10-15): strong engines use uneven layouts, e.g.
  Stormphrax's 16 mirrored buckets, every square alone on ranks 1 and 2, coarser above; HalfKA is
  64 x 704 = 45,056 rows. Mirroring is doubtful for Rotor (castled kings sit on g1 and c1, which the
  mirror does not pair).

- **A separate data store across the machines is retired (the developer, 2026-10-21):** runs go
  over the LAN only (lan-only-servers.md), and the store's designs are removed.

- **Settled 2026-10-20 (the developer):** an `lc0-` network has no generations; a network trained
  on another corpus is simply another network, with its own name, compared by a duel. The `-0001`
  suffix stays, being cosmetic. The stage-by-stage plan below remains open.
  **Rethink generations for Lc0-trained networks (the developer, 2026-10-18).** An `lc0-` network
  is trained once on Lc0's labels and has no generations, yet the arena names it `lc0-psq-0001`
  and keeps it in `gen-0001/`: every network player name ends in `-NNNN`
  (`match_player::parse_network_name`), and a bare `<network>` is refused so that a record's name
  never depends on when a command ran. To rethink together with **the plan to pit networks against
  each other at each stage, before the lambda tournament** (the developer, 2026-10-18), which may
  change what a stage, a generation and a selection are. Until then the `-0001` names stand
  (`lc0-psq-0001`, `lc0-psq768-0001`).

- **Done 2026-10-18: the stage pre-tournament dropped from `lc0-psq`'s select.** All ten of
  `select lc0-psq768`'s stage round robins were monotonic, the last stage winning, and test loss
  picked the same stage everywhere (stage-tournament.md, "Result"). Still open from it: monotonic
  tables say forge stopped while still improving; weigh longer runs.

- **How to blend, on Lc0's labels (2026-10-20).** Results so far:
  - **The cut** (temperature-cut.md): `select lc0-psq-cut` won by 0.05 (52.0%); the long horizons
    improved (0.95 from 45.5% to 48.8%, 0.85 from 47.0% to 49.2%) and their test loss fell sharply
    (0.95: 0.108 to 0.083), but no rise toward them. Its duel not run: only worth it if the others fail.
  - **Lambda 0** (lambda-zero.md): the lowest loss of the uncut arms, yet weaker than
    `lc0-psq-0001-0.15` (+41 -62 =65, about -44 Elo, stopped at 168 games). Test loss on a network's
    own labels does not rank strength across label sets; some blending helps.
  - **The direct blend** (direct-blend.md): `label = (1 - lambda) * score + lambda * result`, Lc0's own,
    `forge lc0-psq-direct` running on the training machine (stopped and resumed with the same command), then
    its select and its winner's duel against `lc0-psq-0001-0.15`. Both predictions are in the doc.
    **Planned for 2026-10-21 (the developer):** finish the training, then
    `arena select lc0-psq-direct`. The question: does the direct blend give a smooth curve over
    lambda, unlike AB(lambda)'s zigzag? Read against the 1 to 2 points of training luck. Its peak is
    the endpoint the self-play lambda ladder should approach (self-training.md).
  - **Next, proposed:** AB(lambda) without the result, the chain anchored on the game's last score
    instead of the outcome, to tell smoothing (the next scores averaging `best_q`'s jitter) from the
    result's information. With lambda 0, AB(lambda) and the direct blend it completes a table of
    one ingredient at a time.
  - **For self-play** (continuous-self-play.md): the developer's hypothesis that the best lambda falls
    as the teacher improves, so a continuous lineage's lambda follows a falling schedule.

- **Under question (2026-10-19): the flat lambda on Lc0's labels may be an artefact.** Lc0's
  training games play deliberate non-best moves (temperature), which AB(λ) propagates backward; the
  "deep scores need no averaging" reading below was never tested (dynamic-lambda.md). Next, after
  `lc0-psq-threats-pairwise`'s run: `lc0-psq` retrained with the cut (temperature-cut.md), the old
  method kept. **2026-10-20:** pairwise's training suspended, `forge lc0-psq-cut` ended; next its
  select, then the duel against `lc0-psq-0001`. The select waits for forge-upload.md
  (implemented, to build and test), so that it can run on the Lichess machine from Drive.
- **Decided 2026-10-18: no finer lambda grid for Lc0's labels; the ten lambdas stay.** The lambda
  round robin of `select lc0-psq768` (36,000 games, +/-1.0% a lambda): 0.55 52.5%, 0.05 51.0%,
  0.35 50.7%, 0.15 50.5%, 0.65 50.4%, 0.85 50.1%, 0.25 50.1%, 0.75 49.2%, 0.45 48.3%, 0.95 47.2%.
  Flat from 0.05 to 0.85, zigzagging (0.45, beside the winner, ninth), so the winner's lead is mostly
  its own run's luck, each lambda being one training run; only 0.95 is clearly worse. A grid at
  0.01 or 0.05 would measure run-to-run variance, not lambda. The developer keeps the ten lambdas
  0.05 to 0.95: ten independent runs, the best kept, and a watch over the whole range should another
  architecture or label source make lambda matter again. **Why Lc0 differs from self-play:** a label
  trusts its position's own score by 1 - lambda; self-play's shallow-search scores were weak, the
  game's end carried the information and lambda 0.98 won, while Lc0's deep-search scores are already
  good estimates, so short horizons do as well and only the longest (0.95) suffers from single game
  results' noise. The winner, `lc0-psq768-0001-0.55`, meets `lc0-psq-0001` untuned
  (`arena duel ... --time 10+0.1`) after `forge` and `select lc0-psq`, before any tuning.
  **`select lc0-psq` (the training machine, 2026-10-19, 36,000 games, +/-1.0% a lambda, no stage
  pre-tournament):** 0.15 51.8%, 0.35 51.6%, 0.25 51.5%, 0.05 50.8%, 0.75 50.8%, 0.65 50.7%, 0.45
  50.3%, 0.55 50.0%, 0.85 47.0%, 0.95 45.5%. The same picture as psq768's: flat up to 0.75, the
  longest horizons clearly worse (0.85 too this time), and psq768's winner 0.55 eighth here, more
  sign that the winner is run-to-run luck. The winner is `lc0-psq-0001-0.15`; its duel against
  `lc0-psq768-0001-0.55` was drawn (1b above).
  **`select lc0-psq-threats` (the training machine, 2026-10-19, 36,000 games in 40 min, +/-1.0% a lambda):** 0.25
  52.6%, 0.15 50.9%, 0.45 50.9%, 0.05 50.8%, 0.35 50.5%, 0.75 50.1%, 0.85 50.1%, 0.55 49.5%, 0.65
  49.4%, 0.95 45.1%. The winner is `lc0-psq-threats-0001-0.25`, the only lead of the three selects
  beyond its margin, though a lambda one run each. **The three selects together (the developer,
  2026-10-19):** a little of the game's future helps, 0.05 (the position's own score, near enough)
  competitive every time but never first; too much hurts, 0.95 last every time; and no fixed lambda
  is best, the winner changing from run to run (0.55, 0.15, 0.25). The ten lambdas stay.

- **Self-play segments by time, not by positions (the developer, 2026-10-18): after caroline's
  self-play ends.** Today a batch closes at `segment` positions (1,000,000), and each machine checks
  the corpus target only between batches: when the training machine's segments complete the corpus, another machine (4
  threads) may be hours into its own batch, so the end waits for it or its staging is left unused.
  **The change:** a batch stops starting games after `segment_minutes` (10) of play, in `self.yaml`
  in place of `segment`, and is sealed once the games in flight finish (none is ever cut short).
  Every machine then stops within about 10 minutes of the target, losing nothing, and publishes
  every 10 minutes instead of every few hours. Checked: training is unaffected by unequal segment
  sizes, since each epoch reads every position once, the stream weighting each open file by the
  positions it has left (`pgn_stream.rs`), and the target counts positions from the `.meta` files.
  To design first: the batch's elapsed play surviving a restart (beside `{stem}.count`), the many
  more remote files (about 140 a day from the training machine alone, for Google Drive's sync), the tests. Rejected alternative: keep positions and seal a partial batch early once the
  target is met, which fixes the end only.

- **Lc0's policy too, when we move to the transformer (the developer, 2026-10-17).** `smithy lc0`
  keeps only `best_q` and the best move today; a transformer with a policy head would also train on
  Lc0's visit distribution, the 1858 `probabilities` of every V6 record. It means converting the
  tars again (they are not kept: about 217 GB downloaded per 1B positions, about 15 hours at 4 MB/s)
  into a format that carries a policy per position, which PGN cannot. To decide then: how to store
  it within Drive's space (the full 1858 floats are 7.4 KB a position; only the legal moves', or the
  top few, are far smaller), and whether the same tars and window are reused, so value and policy
  come from the same games. **Storage (the developer, 2026-10-17):** an external disk if Drive
  is not enough; the training machine's RAM is enough for every plan so far (forge streams; watch
  Task Manager during the next forge). Lc0's data needs no backup, the tars staying on
  storage.lczero.org, so it can live on that disk alone; Rotor has no path for a second local data
  root yet. That disk is also the backup disk (the developer, 2026-10-18).

- **Rollback to the best checkpoint at every rate switch: done (2026-10-16, rollback-to-best.md).**
  Step 0, the previous champion, is the first best; when patience runs out an arm rolls back to its
  latest best, then moves to the next rate. Every training since uses it. Generation 3's retraining
  of betty, caroline and dorothy under it was dropped with the archive corpus (generation-3.md,
  archived): amanda's was the last.

- **Lc0's corpus, `test91/` (lc0-data.md, done).** https://storage.lczero.org/files/training_data/test91/,
  hourly tars (ODbL, `LICENSE.txt` beside them), converted by `smithy lc0` into PGN on Drive, value
  target `best_q`. **No generations on Lc0's labels (the developer, 2026-10-15): one long training
  per architecture**, ended as today when patience runs out after the last rate. Still to confirm:
  which networks play `test91` and at how many nodes, so one slice of it is a consistent teacher.

- **The long-term future: four ways to use Rotor, on two axes (the developer, 2026-10-16).** Not
  necessarily four binaries: rotor-alone-cpu, rotor-alone-gpu, rotor-lc0-cpu and rotor-lc0-gpu. One
  axis is the label source: Rotor's own labels (self-play from the PeSTO bootstrap) or Lc0's data.
  The other is the hardware: CPU (NNUE and alpha-beta) or GPU (presumably the transformer track and
  MCTS, rotor-transformer.md). Extends "perhaps two releases" (Evaluation, the Lc0 item). Details to
  figure out later, among them how a GPU network runs in a release without Python or libtorch, and
  where a GPU variant is rated.
  **Consequence: CCRL and TCEC games are no longer training data under either axis.** CCRL as a
  rating list is unaffected. **Replay is to be rethought around a generic PGN file:** label any
  PGN's positions with Rotor's search, for uses such as a fixed outside test set catching
  rotor-alone's blind spots, the Lc0 ranking check, or Rotor's own Lichess losses. **The CCRL and
  TCEC specifics are removed (rotor-alone-and-lc0.md, done 2026-10-17);** what is left of the
  archive path waits for the streaming transformer (archives-removal.md).

- **The move head (best-eval-together-with-best-move.md), when the developer starts it.** The
  design and the 2026-10-14 measurements are there: a move branch beside the value, computed only
  from about 3 plies left, about +2% a node. Its benches are done (2026-10-15, the last one: 70.9% of
  the waste comes before a quiet cutoff, so the hint goes after the good captures). It was to train
  on generation 4's corpus, which will not exist: its corpus is now Lc0's or self-play's.

- **The reference judge: Stockfish at the right level (the developer, 2026-10-15).** Outside every
  lineage and fixed across generations, so it also measures progress. **Stockfish 19 level 10 at
  10+0.1, one thread each, as README's Strength ladder**, where amanda-0002 scored 54.6% over 186
  games (2026-10-13), close to the 50% where a match is most sensitive. `Skill Level` blunders at
  random from a MultiPV search, but it is calibrated, already measured and recorded, and continuity
  with that ladder outweighs a cleaner knob; moved to the level nearest 50% as Rotor improves. The
  same book and number of games for every variant.

- **Lumbra's GigaBase is not a corpus source: considered and dropped (the developer,
  2026-10-15).** Over 12 million OTB games, about a billion positions (estimated). Two problems for
  AB(lambda) and MCTS(lambda), which back values up along the played line: unsound results
  (timeouts, resignations, agreed draws), curable by bootstrapping from the teacher's last score;
  and blunders, which corrupt every term between a position and the end, curable only by replacing
  the human continuation with Rotor's playout. Then humans would supply only variety, and **variety
  must come from self-play: humans learn from engines, not the reverse.** Confirms 2026-10-13's
  "human games are not training data" (lumbra-book.md).

- **The README needs revision at the first real release (readme-for-humans.md).** Rewritten
  2026-10-13 for readers first, development at the end. At the release: Strength and Inside get the
  released network's figures (the best architecture of the experiments, not necessarily amanda's
  shape), the release is one plain `rotor.exe`, and its version is the workspace's, `0.1.0` (decided
  and in `id name` 2026-10-14). README's "Download `rotor.exe`" links to the release page
  (https://github.com/cigizeta/rotor/releases). `DEV.md` (split off 2026-10-13, dev-md.md) may also need the winget packages for `7z`
  and `zstd`.

## Evaluation

- **Make the threats incremental (the developer, 2026-10-26).** Today a psq_threats evaluation
  recomputes every threat row and adds them, saturating, to a copy of each piece-square accumulator,
  dozens of rows a node from a 2 MB table, likely cache misses. Incremental: the threat rows kept in
  the per-ply accumulator itself, a move adding and subtracting only the rows it changed, found by
  diffing each piece's attacked opponent squares before and after. Stockfish keeps its far larger,
  piece-typed threats incremental; square pairs are simpler to diff (no piece identity, x-rays and
  discovered lines already in the bitboards). The obstacle is quantization: saturation cannot be
  undone by a subtraction, so the threats must sit inside the i16 bound, which `quantize`'s
  worst-case bound refused (dorothy, 2026-10-16); a bound from real positions with margin, or a
  training-time bound on the threat weights, as Stockfish keeps its weights in range in practice.
  **First:** a bench mode timing the threat step alone (rows found, copy, adds) against the head, so
  the gain is known before the work: worth it if the threats are a large share of the head's 140 to
  220 ns. **Designed 2026-10-29: threats-timing.md.**

- **X-rays and pins in the threats (the developer, 2026-10-29), not now:** a slider's attack through
  the first piece, and a pinned piece's attacks dropped or kept to its line; a features experiment of
  its own, its inference cost timed with it. Rotor's threats are bare square pairs today.

- **Done 2026-10-20: the pawn scale from the label-space scale `c`** (label-scale.md,
  label-scale-search.md). `c` is fitted at load against PeSTO's quiescence on the 10,000
  compiled-in positions, and the search scales by `100 / c` rounded (35% on all four champions);
  the static slope `p` is gone, and so is `margin.txt`. Linear only: evaluations still reach the
  search in the network's own centipawns. **Left open:** the nonlinear form, every evaluation as
  `100 × c × pawns_from_score(s)` in PeSTO's centipawns (human-rotor.md), or margins defined in label
  space and applied at the bound, tried first on reverse futility and futility as an A/B; the same
  `c` for the MCTS network's `score cp` and the PeSTO blend.

- **The accumulator's width: try 512, 768 and 1024 against today's 256 (the developer,
  2026-10-15).** `embedding_dim`, 256 in amanda's shape (780 -> 256 per side, 512 concatenated).
  Psq networks are short of capacity, not data, so width is the most direct lever. Costs grow with
  it: the accumulator's update and the head, both about proportional to d; the transform's weights
  (780 x d x 2 bytes: 0.8 MB at 512 fits a core of the Lichess machine's 1 MB L2, 1.6 MB at 1024 does not, perhaps
  5 to 10% more); and the training machine's training time, about proportional. **Estimated nodes/s on the Lichess machine
  (2026-10-15),** from amanda's 3.32M at 256 (about 300 ns a node; update about 65 ns, head about
  40 ns): if evaluation is a third of a node, 2.5M at 512 (-26%), 2.0M at 768 (-41%), 1.6M at 1024
  (-51%); if a tenth (TODO's older figure, from the training machine), -9%, -17%, -23%. A halved speed costs roughly
  50 to 70 Elo at fast controls (rule of thumb), which the wider network must earn back. To measure:
  `nnue_bench` with a random network at each width. **Judged under a clock, not at fixed depth:** test
  loss and a depth-9 duel both favour the widest for free, since neither prices speed. **Staged:**
  512 against 256 first, test loss then a duel at 10+0.1; 768 and 1024 only if 512 wins. Suggested
  after the network experiments, on the winning shape (a wider version of a shape about to be
  replaced says little), on the same corpus.

- **Attention in NNUEs beyond HalfK: four directions to discuss one by one (2026-10-14).** HalfK is
  attention with one fixed, rarely changing query, the king square, which keeps the accumulator
  linear and incrementally updated. Any idea must keep that: a sparse linear accumulator, then a
  small nonlinear stage. Psq networks are limited by capacity, not data (train and test loss nearly
  equal), so representation is the lever, at a cost in nodes/s judged by games.
  1. **Products of accumulators.** Two accumulators `a = Ux`, `b = Vx` and `sum(a_i * b_i)`: a
     rank-r bilinear form, every pair of pieces at O(r) per evaluation, both updated
     incrementally. Stockfish multiplies the two halves of its accumulator elementwise (to check
     exactly, fetched, before designing). The cheapest of the four, a small change to a psq network.
     **Implemented (2026-10-15, pairwise-products.md, pairwise-products-implementation.md):**
     `kind: psq_pairwise`, halves in `0..=255` and `>> 9` as Stockfish. Head as fast as psq's on a
     random network (40.7 against 39.0 ns, another machine). greta never trained: the products are tested over
     caroline's inputs instead, experiment 3 (`psq_threats_pairwise`).
  2. **Soft HalfK: a few learned queries.** K accumulators (4 to 8), each linear in psq features,
     mixed by weights `g(context)` from something that changes rarely (king squares, castling,
     pawn structure, phase). HalfK is K = 64 one-hot on the king square; a small K lets every
     accumulator see all the data. Cost: K updates a move.
     **Postponed for thinking (the developer, 2026-10-14).** It is a mixture of experts: hard
     selection is top-1 with a fixed router (HalfK is K = 64), soft mixing is a dense MoE. The
     archived moe.md designed one with the experts in the head, after one shared accumulator
     (cheap); here they sit in the feature transformer (richer, but K updates or a refresh per
     context change). With experts in the head and a fixed router, 2 becomes 4.
     **Softness dropped (the developer, 2026-10-15): the phase buckets of 4 are already a mixture
     of experts.** Soft mixing must update all K accumulators every move: about K times an
     evaluation, about 1.6 times a search node at K = 4 (evaluation about 0.1 of another machine's 0.51 us a
     node), more if K transforms (400 KB each in `i16`) overflow a core's L2. Stockfish's buckets
     are all hard (king buckets in the input, layer stacks in the head). Kept from it: Stockfish's
     feature factorization, a shared component trained beside every bucket and folded in at export,
     so rare buckets learn from the rest; a candidate for the phase buckets.
  3. **Relations as features.** Threats already turn "A attacks B" into a sparse feature; extend to
     defenders, pins, x-rays, lines to the king. Recent Stockfish added threat inputs too.
     **Waits for games (the developer, 2026-10-14): a refinement, so first check that threats
     work**, by caroline against amanda and dorothy against betty in generation 3's duels. Today's
     threats (psq-threats.md): own attacks on opponent pieces only, square pairs without piece
     types, no defences, x-rays or pins. Candidates, once they pay: defences (Stockfish includes
     them), piece types in the row (attacker type on `i`, target type on `j`), x-rays and pins,
     attacks on the king's zone.
  4. **Output buckets by context.** A head chosen by piece count or phase (Stockfish's layer
     stacks): attention at the head rather than the input.
     **Discussed 2026-10-14, open:** also a mixture of experts (head, fixed router, top-1), the
     cheapest corner of direction 2's family. Stockfish's 8 buckets, `(pieces - 1) / 4`, came in
     2021 bundled with HalfKAv2 and the PSQT buckets; no separate Elo found. Free at inference, so
     test loss can judge it (amanda's shape plus 8 buckets on generation 3's corpus), the duel only
     checking nothing broke: a gain of 10 to 20 Elo is below what the duel rule confirms. Open
     question to resume with: postpone with 2 as one MoE question, or keep as the cheap first
     experiment (suggested).
     **Bucket by phase, not piece count (the developer, 2026-10-15):** Fruit's game phase, as
     PeSTO's and `pesto.rs`'s taper, knight and bishop 1, rook 2, queen 4, pawns and kings 0, 24 at
     the start, capped there for promotions. It weighs material by what it changes, unlike
     Stockfish's `(pieces - 1) / 4`, and moves only on captures and promotions. **One bucket per
     phase value, 25 (2026-10-15):** `smithy phases amanda-0002` (phase-distribution.md) found no
     value starved, the rarest 871k rows (phase 23), 1,700 a parameter of amanda's head. Stockfish
     master has 32 stacks, 8 piece counts by 4 queen states; the table shows queens matter at phases
     8 to 16, where one phase holds queenless and queened positions alike (12: 1.45M without, 1.14M
     with both). A queen split is a later refinement, judged after the 25.
     **Material 1/3/3/5/9 too (the developer, 2026-10-15):** `smithy phases` also counts it, 0 to 78,
     pawns included. It resolves the endgame the phase lumps together (phase 0's 1.02M rows spread
     over material 0 to 8 by pawns), but does not separate queens either (40: 285k rows without, 300k
     with both; 21 to 60 mixed throughout), and its tail is thinner (1 to 3: 72k to 157k rows). 78
     buckets, 1 to 78: material 0 is K vs K, which the search scores as a draw before evaluating
     (`is_insufficient_material`), as it does K+N or K+B vs K at 3.
     **After pairwise (the developer, 2026-10-15): `psq_pairwise_24` (phase) and `psq_pairwise_78`
     (material)**, each named by its scale's full value rather than its bucket count (the phase has
     25 buckets, 0 to 24, material 78, 1 to 78), compared by test loss on one corpus, then the better played against plain
     `psq_pairwise`, a duel resolving only large gains (25 to 35 Elo).
     **psq_stack implemented ahead of those experiments (2026-10-15, psq-stack.md,
     psq-stack-implementation.md):** `(128 + 128) x 2 -> 256 -> 16 -> 16 -> 1`, L1 by our king (64),
     L2 by their king (64), output by material (78), factorised in training; config hilda. Head 103 ns
     against betty's 121 on the Lichess machine (random weights; 136 before the second layer went eight rows at a
     time). hilda never trained: the stack is tested over caroline's inputs instead, experiment 4
     (`psq_threats_stack`). `_78` became experiment 3b, `psq_threats_pairwise_material`
     (2026-10-18): the stack changes four things at once (products, two hidden layers, king
     buckets, material buckets), and 3b isolates the material buckets. `psq_pairwise_24` (phase)
     is kept only to explain a failure.

- **Training on Lc0's evaluations, to test architecture ideas faster (the developer, 2026-10-15).**
  The aim is architecture comparison, not a network to ship: train candidate shapes on positions
  labelled by Lc0 instead of by Rotor's own champion, so a comparison no longer waits for a
  generation's replay and self-play, and the labels' noise does not hide the gaps between shapes.
  **Taken at face value:** Lc0's labels cannot be checked against anything stronger here, so their
  errors and biases pass straight into the comparison, and a shape that ranks well on Lc0's labels
  is only presumed to rank well on Rotor's. Open, to discuss before designing: where the labels
  come from (Lc0's published training data or running Lc0 ourselves, and its license against
  AGPLv3); no centipawn conversion (the developer, 2026-10-15): Lc0's Q, already in [-1, 1], is
  the target in label space as it is, the same for every shape, blended with the game result by
  lambda as Rotor's labels are (the developer: of course, and cheap, Lc0's chunks record the
  result); whether the ranking
  carries over, checked once by training the same shapes on both and comparing their order.
  **Perhaps two releases (the developer, 2026-10-15):** `rotor`, networks from Rotor's own labels
  only, and `rotor-lc0`, a network trained on Lc0's labels; the same engine code, only the
  embedded network differs. This makes the license of Lc0's data a question for now, not later
  (redistributing a network trained on it), and also how rating lists such as CCRL treat
  networks trained on another engine's data (both to check, fetched). `rotor-lc0` needs no
  generations: its labels come from a fixed outside corpus, not from Rotor's champion.
  **"Alone" would mean alone (the developer, 2026-10-15):** replay of outside games might give way
  to self-play only, which also raises the self-play openings, the Lichess book and the PeSTO
  bootstrap. **Not decided:** the binaries are reconsidered only if the Lc0 test succeeds, and no
  code is deleted either way (replay and the rest stay). **What "alone" means, if they come (the
  developer, 2026-10-15):** the PeSTO bootstrap is accepted; CCRL and TCEC games must go, so
  `rotor`'s networks train on self-play only. Each binary states its source; `rotor-lc0` names
  Lc0's data (ODbL's attribution notice too). Not cheating, as long as it is disclosed.

- **mcts-nnue.md drafted together (2026-10-04).** The design of psq_threats_mcts: generation 1
  taught by alpha-beta on pesto with MultiPV(3), MCTS visit counts from generation 2. Its open
  points (the mass split first) are listed there.

- **The hand-crafted bootstrap is PeSTO's material and tables only**, tapered mg/eg, with no other
  term: weak but sound, and playing strength comes from the trained networks (architectures.md).
  The big five, the classical terms as a network initialized from PeSTO, was retired and its code
  removed. Tuning the hand-crafted weights is dropped; textbook-evaluation.md and
  automatic-tuning.md (archived) record what remains of those ideas.

- **Won endgames: does a trained network still shuffle once well ahead?** The old naive output
  label space and saturated at +7.64: against Stockfish skill 3 and 5 (2026-09-27) it shuffled
  until a mate came into the horizon, and underpromoted since every move scored alike. Networks now
  output pawns with a ceiling of 32, so this may be gone. Check by playing today's network (amanda's
  or caroline's champion) once a CPU is free.

- **No mop-up term (decided 2026-10-04).** On Lichess (2026-10-04, probably under `hand-crafted`)
  Rotor failed to convert KQ vs K: PeSTO's tables are nearly flat there, so every quiet queen move
  scores alike. The hand-crafted evaluation stays PeSTO as is, being only a bootstrap, and the NNUEs
  get no hand-written term. Conversion of won endings comes from search and tablebases instead: the
  TT mate-score fix (Search) and DTZ (UCI and release). Get the game's PGN for a regression position.

- **The label curve sets the resolution far from equality.** `labels.rs::score` is fixed by
  `CEILING_PAWNS = 32`; far from equality, positions crowd near `±1` of label space, where the
  test RMSE is larger than the gaps between them. Not a defect of the conversion, and changing the
  curve invalidates every label (a `prepare` rebuild and a retrain). Search consumes the order of
  sibling positions rather than absolute values, so it may cost nothing in games: wait for match
  results before touching it (rule 5).

- **Do the pruning margins fit a network's scale?** A network searched 1.8x to 3.8x more nodes than
  the hand-crafted evaluation at the same depth (old naive, label space). Null move, razoring and
  both futilities compare a static evaluation against margins set for the hand-crafted
  distribution; a network regressed toward a blended target is plausibly compressed toward zero
  and clears fewer margins. **Measured (2026-10-03, search-simplification.md, `nnue_bench
  --eval-scale`):** amanda's network is compressed near equality (its pawn about 0.6 of the
  hand-crafted one at p50 and p75) and stretched far from it (p99 ratio 1.89), so every margin is
  more conservative under the network than intended. The margins merge into one at 2 pawns/ply
  first; scaling it to the evaluation's own pawn is a separate, later question.

- **The transformer still outputs label space**, regressed with MSE against the label (its own
  network and rounds, outside `nnue_network::Network`). Move it to pawns, for one loss and one unit
  across every architecture, as part of its redesign below.

- **Redesign the transformer track, later.** Not before the NNUEs are strong and serve as the
  baseline it is measured against. Its search is MCTS, as rotor-transformer.md already pairs it
  (transformers are too slow for alpha-beta; the NNUEs use it, except psq_threats_mcts, see
  architectures.md). Decided 2026-10-01: start from the standard solution, PUCT with certainty
  propagation (proven wins and losses backed up as exact minimax values, as Lc0 does), and invent
  only once it works. Open, for then: the policy head's first targets, bootstrapped from alpha-beta
  (best move, or MultiPV scores as a softmax) or MCTS visit counts from a uniform policy;
  mcts-nnue.md answers the same question for psq_threats_mcts first. Until then its code stays as
  it is. It has no data source left since `rotor-curate` went. The redesign decides the fate of
  `preprocessing.pgn`/`depth`, the stem-keyed readers, `positions_dir`/`labels_dir`, and the
  transformer rules in `net_config.rs`.

- **Do not `smithy install` a network whose `.nnue` the current engine cannot load:** the
  engine exits at startup when its `EvalFile` fails to load. `smithy install hand-crafted`
  installs the engine with no network.

## Training

- **Google Drive: the free 15 GB, no paid plan until forced (the developer, 2026-10-17).** Drive
  showed 15 GB, 4.0 GB used, on 2026-10-17. The 100 GB plan of 2026-10-13 (Google One Basic) was
  never taken. Budget: 5 GB for Lc0 (`lc0.yaml`'s `cap_bytes`), 5 GB for self-play, the rest for
  networks and frozen sets. **Self-play fits its share (measured 2026-10-18):** 5.12 bytes a
  position (caroline-0001's 1.85 GB for 362M positions) against Lc0's 4.95 (2.90 GB for 585M), so
  1B positions take about 5.1 GB. The earlier estimate of 6.5 to 7 bytes (1.5 GB at about 215M
  positions, 2026-10-17) was wrong. Projected total about 10 to 10.5 GB of 15. If space gets tight, Lc0's
  corpus moves to the external disk first, since it can be downloaded again. If that runs out, the external disk comes first (the Lc0 policy item
  above). The `rotor` folder is kept offline on every machine, so whatever Drive holds must also
  fit every machine's local disk.
- **`champion.txt` records no learning rate (decided 2026-10-08, the developer: dropped).** A run
  steps down through every rate of `training.learning_rate`, so the rate of the round that produced
  the champion said little; `history.tsv` keeps every round's. Older `champion.txt` files keep
  their `learning_rate` line as written. `candidate.txt` dropped it too, the same day; the rate
  still goes into the `.nnue` header.

- **Rotor's trainer stays Rotor's own (decided 2026-10-06).** No third-party NNUE trainer (bullet,
  nnue-pytorch or the like): Rotor must remain the developer's own implementation, and
  lockstep lambdas with selection by games, not by loss, are Rotor's own anyway. libtorch through
  `tch` only supplies the tensor arithmetic.

- **Lockstep lambdas (lockstep-lambdas.md, implemented 2026-10-03, merged to `main`)**: every
  lambda of a generation trained over one shared batch stream, as the learning-rate grid's arms once
  were; batches built once for all lambdas. Done: run for real by caroline's and betty's generation 1
  (2026-10-06) and every run since.

- **Psq training is fast (measured 2026-10-03, amanda's generation 1 `history.tsv`).** Lambda 0.85,
  one lambda at a time, before lockstep: about 15.5 s of training per 4M-position round (about 270k
  positions/s) and 6.8 s of test pass, about 55 min per lambda. Caroline, in lockstep at batch
  4096: an estimated 3 to 5 h for six lambdas. The replay and the self-play, not training, set the
  pace of a generation. Measure her first rounds to confirm, and her GPU utilization (the reader
  thread and all-core batch building, parallel-batches.md, run for real there for the first time).

- **A seekable train cache: done (implemented 2026-10-08, seekable-train-cache.md; tried and
  working, the developer, 2026-10-12). The original item:** a resume reaches its
  row by decoding everything before it (2026-10-06).
  Each cache file (positions and every lambda's labels) is one zstd stream of variable-length
  records, so `CacheFile::seek_to` reopens it and discards `target` records: resuming betty at step
  192,895 sat silent before `shuffle buffer ... filled` while 9 streams decoded in lockstep up to
  the cursor, up to a whole pass of a 358M-row cache. Fixed-width records alone would not help: a
  compressed stream has no offset for row `i` either way, and dropping compression costs several
  times the disk. The fix: write each file as independent zstd frames of a fixed row count (say
  2^20 rows), plus a sidecar index of each frame's first row and byte offset; a seek jumps to its
  frame and decodes at most one frame's rows. Records stay variable-length, compression barely
  changes. Every file of a cache shares the same frame boundaries, so the lockstep labels stay
  aligned. Changes the cache format (a `prepare` rebuild). Also print a line before the fill, so
  the wait is not silent. Not urgent: at today's 358M rows the skip took little time (the
  developer, 2026-10-06), but it grows linearly with the cache, so it matters once the corpus is
  much larger. To design first.

- **Stack lockstep arms into one batched network (stacked-lockstep-arms.md): done.** Betty gen-0002
  resumed on it at step 140368 and finished on it (2026-10-11): train seconds per round from about 40
  to 12.7 for eleven arms, GPU at 90%.

- **Sparse transform for psq: the next training speed-up, now that the GPU is the limit.** With the
  arms stacked the GPU runs at 90% (2026-10-11), so a round is bound by arithmetic, and most of it is
  the dense `[batch, 780] x [780, arms * 256]` transform, forward and backward. At most 37 of the 780
  inputs per perspective are active, about 5%, so a gather-and-sum over the active rows
  (`embedding_bag`, or `index_select` plus a segment sum) would do roughly 20 times less work on the
  largest operation, and would skip building the dense input on the device (`batch::psq_on`'s
  `index_fill`), in the test pass too. The cost: the transform's backward has to produce a sparse
  gradient for the stacked `[arms, 256, 780]` weight, AdamW then updates rows no batch touched only
  through weight decay and the moments (decide whether to keep dense AdamW over the whole weight or
  switch to a lazy one, which changes the training), and it must stay equal to the dense transform
  within float tolerance. psq_threats gains the same, at its own width. **Designed (2026-10-12,
  sparse-transform.md):** `embedding_bag` over the existing active indices, dense AdamW kept.
  Implemented the same day, tests passing on the Lichess machine. **Done 2026-10-14:** generation 3 trains with
  it on the training machine, much faster by the developer's observation, but no clean figure: the per-round
  seconds in `history.tsv` were taken with several runs sharing the training machine and arms dropping out as they
  stop. Left: time a few rounds of each architecture alone on the training machine. **First figures (the
  developer, 2026-10-19), ten lambdas stacked, 4096 steps a round, round 1:** `lc0-psq` about 60 s
  of training (about 68 steps/s); `lc0-psq-threats` 92.9 s (44.2 steps/s) plus 5.9 s of test, about
  1.5 times psq's, for 6.25 times the rows.

- **Moot (2026-10-16): that training was deleted and retrained under rollback, its lambdas ending
  between rounds 36 and about 112 (generation-3.md); the cause was never checked.**
  **Amanda gen-0003: every lambda stopped at round 113 (noticed 2026-10-14).** All eleven
  `history.tsv` have 113 rounds and every candidate is at step 110,592, while betty's and
  caroline's lambdas stopped one by one (168 to 217, 175 to 247 rounds). Check whether the run was
  interrupted or a stopping rule ends every arm together; if cut short, her losses are pessimistic.

- **Compute the test features once per run (test-features-once.md): implemented and merged.**
  Done: run for real since caroline's generation 1 (2026-10-06).

- **halfkp and halfka_v2_hm: parked (the developer, 2026-10-12).** Not pursued, for two reasons:
  HalfK is what every other engine does, so it would contribute nothing of Rotor's own; and the
  corpus is too small for it (about 41k inputs per perspective, each king square seeing roughly
  1/64 of the data). Revisit only if the corpus grows by an order of magnitude. The code stays as
  it is. The item below applies only then.
  **The data reason is lifted by Lc0's labels (the developer, 2026-10-15):** no limit on the
  dataset's size any more, and training cheaper than first thought, so HalfK is doable on the training machine's
  one GPU; on the same labels it is also the reference Rotor's own shapes must beat. Before it
  trains: active indices (below) and the sparse transform. In the engine, a king move refreshes
  the whole accumulator; whether that needs Stockfish's per-king-square accumulator cache is
  decided by nodes/s. **"Plenty" gets a reality check (the developer):** HalfK's positions/s on
  the training machine (sparse transform, stacked arms, the Lc0 decoder feeding it) against psq's, then days
  per run. Dense AdamW over all 41k rows a step may set the pace, or the decoder may.
  **Active indices instead of dense rows, before either is trained again (agreed 2026-10-03).** Their host batches are dense `f32`, 96-99% zeros: about 17-20 KB a
  position, so the test cache (test-features-once.md) keeps only 3-4% of a 5M-position test half.
  As the retired big five did: per side, the row-relative `u16` indices of the active rows (a row is at most
  2,560 wide) with repeats, and a count; `upload` builds the dense rows on the device with
  `index_add` of ones (not psq's `index_fill`: two pieces can share a row, which then holds 2).
  About 0.5 KB a position, about 2.5 GB for 5M, under the training machine's budget of about 3.3 GB, but tightly.
  Training batches shrink alike, as psq's did. The budget grows with the training machine's memory (16 GB more
  is being considered); the 3.3 GB figure dates from WSL and is to re-measure under Windows.

- **Network sizing prices both training and inference.** Training says "go wider, nearly free";
  inference on the old naive said the value head's first layer was nearly all of an evaluation's
  cost, and feature extraction under a tenth of a percent, the reverse of the usual NNUE shape (a
  large feature transformer feeding a tiny first layer). Check what real engines do, fetched and
  confirmed, before resizing.

## Search

- **`arena margin`'s grid may stop too high (noticed 2026-10-19).** Its gauntlet tries 200 down to
  100 and nothing below, and 100, its floor, won for both caroline-0001 and
  `lc0-psq-threats-0001-0.25`: the best margin may lie lower. Extending the grid down (75, 50)
  costs a match or two more a tune. Weigh it against SPSA, which replaces `arena margin` anyway.

- **Tuning several search parameters per network, on a branch of its own, later (spsa.md, the
  developer, 2026-10-15).** It replaces `arena margin`'s search for `margin.txt`, run at the same
  point: on the selected candidate, just before it challenges the champion. Coordinate descent, each
  parameter with its own step `c`; per round a gauntlet of 7 players (`x0`, `x0 +/- c`, `x0 +/- c/2`,
  `x0 +/- c/4`), whose winner sets the next `x0` and `c`, no schedule. Revises the "no tuning" rule
  (search-simplification.md), the margin no longer the only exception. Its own branch, since it
  reshapes the search code (several margins instead of one, their values beside the network). Open
  questions in spsa.md: which parameters, how the gauntlet picks its winner, the budget. **Perhaps
  tuned only once, in the evaluation's own pawn (the developer, 2026-10-15):** each new network
  then only measures its pawn (`nnue_bench --eval-scale`), and the tuning reruns only when the search
  or the architecture changes; to be confirmed by checking the scaled values against a gauntlet on
  the next few networks.

- **The label path freeze for generation 2 lifted (2026-10-10):** amanda-0001's replay and
  self-play have ended.

- **B1, the null-move reduction growing with depth and margin: done (2026-10-07,
  null-move-reduction.md).** `3 + remaining/4 + min((eval - beta)/200 cp, 3)` plies, Stockfish's
  form of its versions 9 and 10, not master's harder one. +71.9 +/- 33.5 Elo at 10+0.1 against the
  search before it, resolved at 250 games by the duel rule.

- **B6, pawn correction history: shelved (2026-10-07, correction-history.md).** No gain shown at
  10+0.1 against B1, neither with a fresh table per `go` (+7.6 +/- 23.3 Elo, 500 games) nor with
  one kept for the game (-3.5 +/- 35.1 Elo, 200 games). Worth a retry with tuned constants (B9)
  or a network evaluation.

- **B4, capture history: shelved (2026-10-07, capture-history.md).** -6.9 +/- 49.4 Elo after
  100 games at 10+0.1 against B1, stopped as hopeless. The duel rule confirms only about +34 Elo
  at 1000 games, so small refinements cannot pass it; the developer kept the rule rather than
  SPRT, so only changes expected to be large are worth a duel. One duel per design, never rerun
  or extended past 1000 games after the fact (developer decision, 2026-10-07).

- **B2, singular extensions: shelved (2026-10-07, singular-extensions.md).** Stockfish's core
  plus the -1 extensions, at a minimum of 6 plies to go: +20.1 +/- 23.6 Elo after 450 games at
  10+0.1 against B1, stopped once 0.549 at 1000 was out of reach. At a minimum of 4, even after 239
  games.

- **B2, B4 and B6 to retry once stronger generations arrive (developer decision, 2026-10-07).**
  All three were judged with the hand-crafted evaluation, whose coarse score gaps likely made too
  many moves singular (B2) and whose errors are mostly tactical, not tied to the pawn structure
  (B6). A retry duels with the champion network on both sides, the evaluation that labels and
  plays: a new experiment, since the engine differs, and one duel each.

- **Pruning margins from the evaluator's own error: shelved (2026-10-08, eval-error-margins.md).**
  `nnue_bench --residuals` on 10,000 held-out master positions (Lumbra's Gigabase, `data/lumbra/`
  on Drive) puts amanda's error tails at 0.6 to 1.0 of hand-crafted's, around her pawn scale of
  0.7: the scaled margins are in the right range. Reopen only with nodes sampled from the engine's
  own search.

- **Mate scores in the TT are relative to the node: done (2026-10-07, tt-mate-distance.md).**
  Stockfish's `value_to_tt`/`value_from_tt` for both the mate and tablebase bands; duel skipped.
  New `search_bench` reference, depth 9, hand-crafted, the Lichess machine: 3,183,462 nodes, 1,083 ms.
  Since B1 (null-move-reduction.md): hand-crafted 3,140,380 nodes on the 49 positions, the same on
  the training machine and the Lichess machine; amanda-0001 1,579,395 before the scaled margins (psq-node-cost.md).
  **Current reference (2026-10-18): hand-crafted 3,139,294 nodes**, the same before and after
  `SearchParameters` (search-parameters.md).

- **Rotor against Stockfish, nodes/s on one thread (measured 2026-10-12, the Lichess machine plugged in and
  idle, replay stopped):** Stockfish 19 `bench 16 1 13` 2,032,475 nodes/s (its 109 MiB network);
  Rotor `nnue_bench --search-only`, depth 9, amanda-0002, **3,317,424 nodes/s** (950,209 nodes in
  0.29 s): **Rotor about 1.6x Stockfish**. Different positions, depths and node counting, so
  approximate. A first reading of 1.55M (0.76x) was a bench artifact, a fresh 64 MB TT per position
  paying page faults (node-cost.md, done the same day; bench fixed, node counts identical). On
  battery everything drops by about 1.6x: measure plugged in only. The gap to Stockfish is in
  evaluation quality and pruning, not nodes/s.

- **Search speed, paused: what is left.** The last profile (2026-10-03, on the since-removed big
  five's bench) was flat, no single hotspot. Done: the staged move picker (move-picker.md, about +115
  Elo) and quiescence's `noisy_moves` (quiescence-noisy-generator.md, 2-6% nodes/s). Left, a few
  percent each: move generation about 11% (pawns 4.3%), make/unmake and legality about 9%, TT about
  4%, `quiescence_from` and `alpha_beta_with_buffer` themselves about 13%; the early-exit `see_ge`
  is done (2026-10-07, above). Re-profile on a psq network before acting. Also:
  `nnue_bench`'s game-generation projection starts from a hard-coded 3 games/s, stale until
  re-measured.

- **Optimize pseudo-legal move checks: done 2026-10-07 (legality-before-make-move.md), timed
  with the other speed changes above. The original item:** Every move the search tries
  pays `make_move`, a full `is_in_check` on the new board and `unmake_move`, including illegal
  moves and moves futility or late-move pruning then skips (`alpha_beta_with_buffer`'s move loop;
  quiescence likewise). Stockfish instead tests legality before making the move, cheaply: pinned
  pieces once per node, then a real test only for king moves, pinned pieces and en passant; and it
  decides most pruning before the move is made. Aimed at nodes/s (Rotor about 250k per thread on
  Lichess, 2026-10-06, against Stockfish's 1 to 2M; superseded by the single-thread measurement
  under Search speed below), not depth. Pruning moved ahead of `make_move`
  changes the tree, so it is on the label path. Measure on idle the Lichess machine. Item A1 of
  search-optimizations.md, the full candidate list.

- **NNUE quantized inference: done (2026-10-07, nnue-inference-speed.md).** Quantized when the
  `.nnue` loads, no trainer or format change: `i16` accumulator, `u8` activations rounded to
  nearest, `i16` weights in the first head layer. 1.2 to 2.4 cp mean from f32. Betty int8 against
  betty f32 at 10+0.1: +32.1 +/- 17.1 Elo over 1000 games. The fused update is done
  (2026-10-07, node counts unchanged, timed with three search changes below). Betty's head: activations
  once and the tail in registers (betty-head-speed.md), 141 to 128 ns, search about 1% faster;
  its cost is the 512 `madd`s. **Generation 2 trains with weight clipping (decided 2026-10-07)**,
  so that the first hidden layer can go `i8` with `maddubs`, half the multiplies, judged by duel.
  **The clipping is implemented (2026-10-07, weight-clipping.md)**, tests passing; betty-0001
  already fit the bound (max |w| 1.943). Left: `nnue_bench --weights` on generation 2's first
  checkpoint. **The `i8` layer itself is shelved (2026-10-07, i8-hidden-layer.md):** the engine
  scales per row, so clipping buys no precision (`i8` 3.95 cp mean against `i16`'s 2.47); VNNI
  runs the layer 1.65x faster, about 7% of betty's search, too little for the duel rule. Retry when
  the first layer is a larger share of a node.

- **Four tree-identical speed changes, 2026-10-07, all committed, node counts unchanged**
  (depth 9, `nnue_bench --search-only`: caroline 1,697,834 and hand-crafted 3,183,476 after
  each, the latter moved from 3,183,462 by the DTZ commit; betty 1,643,715, checked after the
  fused update only): the fused accumulator update
  (nnue-inference-speed.md), legality before `make_move` (legality-before-make-move.md, A1), the
  TT prefetch (tt-prefetch.md, A3) and an early-exit `see_ge` (see-ge-early-exit.md, A5). **Timed
  together against `a5d2113`, idle the Lichess machine, best of two, same node counts:** hand-crafted 2.97M to
  3.28M nodes/s (+10%, 1.07 to 0.97 s), caroline 1.75M to 1.97M (+13%, 0.97 to 0.86 s), betty
  1.48M to 1.63M (+10%, 1.11 to 1.01 s). The fused update does not touch hand-crafted, so its
  10% is the other three; betty's head, untouched, is the largest share of her node. That
  caroline was psq. psq_threats was measured only with random weights on the training machine (psq-threats.md,
  step 4: +6% a node's evaluation, +10% a depth-9 search); its nodes/s with caroline gen-1's
  trained network on idle the Lichess machine is left, the cost the threats' test-loss gain must pay for.

- **Singular extensions: tried and removed (2026-10-03, singular-extensions.md, in `done/`).** A
  duel at 10s+0.1 drawn at 1000 games, singular about -11 Elo (-29 to +7). A later attempt would
  port more of Stockfish's version (`ttPv`, its margin, -3, double extensions), each by duel.
  Continuation history, IIR, null move,
  razoring, both futilities, LMR, late-move, SEE and history pruning, check extensions, PVS, mate
  distance pruning, aspiration windows and Lazy SMP all exist.

- **Simplify the search: done (2026-10-03, search-simplification.md).** Search parameters are
  picked by reasonable magnitudes, not tuned (no SPSA). The four pruning margins became one,
  `MARGIN_PER_PLY` (2 pawns/ply); the aspiration delta lost its 8-thread repeat; every kept
  parameter has its value and one reason in its doc comment. Left: scaling the margin to the
  evaluation's own pawn, a separate duel (see the pruning-margins item).

- **A `[from][to]` history table (4096 buckets), as an alternative or addition to `[piece][to]`
  (768).** `[piece][to]` fills faster at Rotor's node counts; `[from][to]` keeps more positional
  specificity (search.md, including how Stockfish combines both). A playing-strength question, so
  decided by a match (`arena sprt`), not node counts.

- **Per-technique benchmarking.** A unit test checks for bugs, not performance (the old killer and
  ordering node-count tests were deleted for that reason). To isolate one technique's benefit, add
  a UCI option per technique (e.g. `setoption name UseKillerMoves value false`) and run
  `search_bench`'s position suite with it on and off. Not designed: which techniques, and whether in
  `search_bench.rs` or a sibling bench.

- **Time management: what remains after time-management.md.** The target follows best-move
  instability, supports `movestogo`, and follows the score against the previous move
  (falling-score-time.md, kept 2026-10-08 by a harm-check duel). **The four parameters are settled as they are (decided 2026-10-12):** alpha 0.05, beta 0.5,
  r 2, s 0.5, each with its reason in `TimeParameters`' doc comments; not tuned, as no search
  parameter is, since a few Elo is below what a duel resolves and Lichess games use the clock
  sensibly.
  **Left, before entering CCRL: a repeating-control check.** Lichess sends only Fischer, so the
  `movestogo` path has never played a real game, while CCRL plays 40/15 repeating. A few dozen
  cutechess-cli games at `tc=40/60` on idle the Lichess machine, Rotor against itself: no time forfeits, a
  sensible clock left at move 40, and no move near the control spending close to the 0.5 cap.
  Rotor subtracts no move overhead, relying on the GUI's; if forfeits appear, add a `Move Overhead`
  UCI option, the usual fix.

- **A single legal move gets a capped search: done (2026-10-04, single-legal-move.md).** Every
  game move with one legal move is capped at 500 ms, Stockfish's value, under a clock or a fixed
  depth; never self-play or replay, whose depth-9 score is the label. All tests pass.

## UCI and release

- **Going public at the end of the hidden-layer grid, with a first binary release (the developer,
  2026-10-26), before any CCRL application.** The old history was already reset (the developer,
  2026-10-26), and will not be reset again: since then every commit is written as public under
  CLAUDE.md's "Never leak" section. Rotor holds no secrets; an occasional personal habit is
  acceptable. Before going public, one read-only sweep of the tree for what that section lists, its
  findings reported by file and line. **The release's network, the best of the 2x2 grid by
  its duels, tuned first (the developer, 2026-10-26):** its config's `tune:` at 10+0.1, so the binary embeds its `parameters.txt`; then the
  strength assessment below, with the tuned parameters, then the README, then the release. Earlier plan (2026-10-21), superseded: a new repository from
  one first commit at the CCRL application.

- **The README for the public release (the developer, 2026-10-26).** Its Strength section is
  amanda-0002's, from the earlier training (2026-10-13). Measure again with the release's network,
  the grid's best, against Stockfish 19's skill levels as before, and name the network assessed.
  Its "Inside" section describes the new training's first networks (780 inputs, straight to the
  output): bring it to the released architecture (threats, its head), and its opening paragraph's
  "starting again from scratch" up to date.

- **Fixed 2026-10-20, to build and test (fifty-move-pv.md):** a drawn node's PV row was left full.
  **The reported PV runs past a fifty-move draw (seen 2026-10-20, fastchess in the OpenBench smoke
  tune).** fastchess warned "PV continues after fifty-move rule": in a rook endgame Rotor-base
  reported `score cp 0` with `pv f6e5 d6d2 ...`, the game drawn by the fifty-move rule after
  `f6e5`. Cosmetic: the score was already the draw's, the move played unaffected, and fastchess and
  the arena apply the rule themselves. The PV is presumably extended past the draw node (from the
  TT); cut it at a fifty-move or repetition draw, as Stockfish does. A small fix, any time.

- **A release for testers (CCRL) embeds the network and its tuned parameters (decided 2026-10-08,
  tuned-margin-storage.md; `margin.txt` became `parameters.txt` on 2026-10-20).** No GitHub
  releases exist yet; `smithy install` is the only install, and it puts `parameters.txt` beside
  `rotor.nnue` in `bin/`. When releases come, the binary embeds the champion's network and its
  `parameters.txt` at build time, so testers run it with nothing beside it to lose (today an
  embedded network plays the defaults, and with the UCI tuning options gone nothing else sets
  them); `EvalFile` stays for development.

- **The arena follows the rules of chess (checked 2026-10-07, `arena/game.rs`).** No adjudication of
  any kind; mate and stalemate first, then the fifty-move rule and threefold repetition, both
  applied without a claim, as engine tournaments do. The hash's en passant square can only delay a
  repetition by one occurrence, never invent one. A time forfeit is drawn when the other side
  cannot mate (a bare king, or a dead position), FIDE's rule 6.9, as cutechess-cli and Lichess judge
  it; it was always a loss before.

- **A book on Lichess: yes, read by lichess-bot (decided 2026-10-02, opening-book.md).** Built from
  CCRL's testers' moves by `arena book`; Rotor itself reads no book (no engine-side `OwnBook`:
  `src/polyglot_book.rs` was last present at commit `08afda6`, rotor-curate-polyglot-book.md).

- **Judge book lines by search, not by game results (to discuss).** The Polyglot book from CCRL's
  testers' moves weighs them by frequency only. Results were dropped:
  a line's game outcomes are not an alpha-beta evaluation of it. How to evaluate book lines with
  the search instead is a future discussion with the developer.

- **Pondering, for Lichess only: done (pondering.md, ponderhit-continue.md, both in `done/`).** A
  ponder hit continues the ponder search under its clock, the ponder time counted as spent, as
  Stockfish does. `arena` never ponders: it sends neither `go ponder` nor `setoption name
  Ponder`, for any engine, Stockfish included.

- **Stockfish's `+25%` of the clock target when `Ponder` is on: rejected (2026-10-03).** SPRT with
  cutechess-cli on the Lichess machine, Rotor against itself with it on and off: stopped inconclusive after 620
  games at +5 Elo. The temporary `PonderBonus` option is removed; the target is the same with or
  without `Ponder`.

- **The Syzygy sets are complete on every machine (the developer, 2026-10-15)**, 6-piece DTZ
  included (the Lichess machine had 306 of 365 on 2026-10-07). A `bin/syzygy.sh` run over complete sets prints
  only one `Checking <set>` line per set; never run it while a Rotor process has the tables open.

- **DTZ ranks the root moves: done (2026-10-07, dtz-root-ranking.md).** Stockfish's
  `root_probe`, fetched: DTZ only at the root, WDL in the search only after a zeroing move, as
  before. Everywhere (game play, self-play, replay), in-search probes kept. The engine takes
  whatever `SyzygyPath` gives; the arena requires all four sets. Tests mate KQ vs K and KR vs K
  from far starts within the fifty-move rule. Left: lichess-bot's `SyzygyPath` gains
  `3-4-5-dtz` and `6-dtz` (by hand).

- **Tablebases stay Syzygy DTZ, not DTM, not 7 pieces.** DTM is the wrong tool: Syzygy has none,
  the available DTM sets (Gaviota, 5 pieces) ignore the 50-move rule, which Lichess enforces, and
  DTZ already plays a won ending correctly under it (the DTZ item above). 7-piece Syzygy is many
  terabytes, out of this project's budget.

- **Release packaging must not depend on Python, `uv` or a venv.** The engine itself has no libtorch
  (only the `smithy` crate uses `tch`), so a release is the `rotor`
  binary and its `.nnue`. Check that nothing in `bin/rotor`'s setup leaks into what a release needs.

- **Publish on GitHub, with a versioned Windows release: when the developer judges Rotor ready
  (2026-10-06).** The license is AGPLv3 only, done (2026-10-12, as in aio64: `LICENSE`, `Cargo.toml`,
  the README's License section, CLAUDE.md's Licensing). **Decided 2026-10-12 (the developer):** the
  repository itself goes public, whole (`claude/` included); releases are Windows
  binaries, each embedding one chosen network (one architecture's champion) in `rotor.exe`, one
  file for testers. Before going public: check the history for anything private (tokens,
  lichess-bot's `config.yml`, local paths, `data/machines.txt`). Open: the version scheme and UCI
  `id name` (e.g. `Rotor 0.1`); `target-cpu` builds (AVX2, perhaps one for older CPUs).

## After chess settles

- **Other games: planned again from 2026-11-05 (temper-games-restart.md), Go first, not started.**
  Parked until then (the developer, 2026-10-13). Go was dropped: on CGOS Rotor would mostly
  play clones of other engines, not engines their authors built. The repository is named rotor
  again (was cigizeta-games). The chess code stays written for chess alone.
- **Chess variants, only where an avenue like CCRL exists (the developer, 2026-10-13).** Chess960
  is dropped for now, both as a list to play on and as training data (its games, and DFRC's, need
  960 castling in `pgn-replay` and in the move generator). Other variants only once a rating list
  of independently built engines is found for them. Chess324 needs no new rule.
