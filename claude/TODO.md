# TODO

Loose, running list of outstanding action items -- not full designs (those live in
`active/`/`done/`/`archived/`), just things not to forget.

**Pruned 2026-11-06 (the developer):** only open items are kept. Everything settled, superseded or
moot before then, the reasoning behind it included, is in git: the version of this file before the
commit that pruned it. The documents in `claude/` keep their own history as written.

**Names:** the project was Rotor until 2026-11-04, then Temper, Temper Games from 2026-11-05, and
Kalo Games from 2026-11-06, its engine Kalo Chess (kalo-chess-rename.md), and Kalo Chess alone,
with no `chess/` level in `data/` or `configs/`, since later that day (kalo-chess-repository.md),
and Fattorini from 2026-11-07 (fattorini-rename.md). Older documents use the older names and paths.

## Next (the developer, 2026-11-06)

In this order:

**Where things stand (2026-11-11).** **Fattorini 2026.11.0 is released** (item 4), published on
GitHub on 2026-11-11. Next: the CCRL testers (item 5), then the wiki page (item 6). Done on
2026-11-10: the CCRL Blitz estimate, about 3250 (ccrl-conditions.md); the root's tablebase score
(root-tablebase-score.md), its snapshot on Lichess since 2026-11-11, waiting to be seen in a
tablebase ending.

**Time management, 2026-11-13: Reckless's hand-picked set beat Classic at 180+2** (test 8, 53.4%
over 654 games, reckless-time-management.md), and ships untuned: Classic, Split and the
`TimeManagement` option removed (reckless-only.md), Lichess on the new snapshot. **lichess-bot's
config was stale:** an old engine, `move_overhead` 2000, and a `SyzygyPath` to a tree that no
longer exists, so no tablebases; all fixed the same day. To check in its log which binary played
the past week's games before trusting them (the 2612, the +1.59 in KQ vs KN). Every arena to be
updated to the commit before the next gate or tune.

**In progress (2026-11-11): blitz time management** (blitz-time-management.md, its plan
blitz-time-management-implementation.md). Two Lichess blitz games ended with a minute and half the
clock unused; the new `Split` time management (a soft time, a hard maximum, the future increments
counted) is implemented with `Classic` kept behind the temporary `TimeManagement` UCI option, all
tests and clippy passing. lichess-bot's `move_overhead` went from 2000 to 500 ms the same day.
The duel binary is copied to `data/duel-bin/fattorini-tm.exe` and registered twice in
`data/engines.yaml` (`fattorini-split`, `fattorini-classic`). Split's first version lost at 10+0.1
(S 0.4044 over 136 games, front-loading); redesigned the same day as v2
(blitz-time-management-v2.md), whose 10+0.1 duel was drawn (S 0.5086 over 752 games, gate passed). **Next:** the 180+2
duel on one idle machine, `bin/arena duel fattorini-split fattorini-classic --time 180+2`, up to
about 12 to 15 hours, resumable; its verdict rule is in v2's decisions; record the results there.
Also waiting: the CCRL forum account's activation.

1. **Tune the lc0 network** (`lc0-psq-threats-screlu-g1-r1`): done 2026-11-09, 32,768 pairs at
   10+0.1. Its result is the network's `parameters.txt` on Drive (razoring 111, reverse futility
   64, futility 214, null move 185, delta 206, SEE prune 90, SEE 46, aspiration 8, history
   pruning -35, good quiet -128); the previous 16k tune's is kept in `tune-16k-old-defaults/`. Then,
   in this order (the developer, 2026-11-09):
   1. **Duel of the new tune: done.** Against the defaults (`SearchParameters::DEFAULT`, which are
      the previous 16k tune hand-rounded), test 4: won by the new tune, 53.5% over 712 games at
      10+0.1, about +24 Elo. It ships. The duel against the previous tune itself is dropped: the
      defaults already were it, near enough.
   2. **Move Overhead: done** (move-overhead.md), default 20 ms; its measurement is under "Before
      the release or CCRL".
   3. **Dropped (the developer, 2026-11-11):** Stockfish's Skill Level is a handicap, not an
      anchor; the release claims the CCRL Blitz estimate from the three anchors (ccrl-conditions.md).
      The matches played, for the record. **The strength assessment against Stockfish**, the release's two rows (binary-release.md,
      decision 10: 400 games all played, 10+0.1, one thread, 64 MB, UHO book, Syzygy to 6 pieces).
      So far: Skill Level 10, a full match, Fattorini 300.5 to 99.5 (+274 -73 =53, 75.1%),
      `data/matches/stockfish-10-vs-lc0-psq-threats-screlu-g1-r1@tuned/20261109-1747/`; Skill
      Level 12, a full match, Fattorini 226.5 to 173.5 (+183 -130 =87, 56.6%),
      `data/matches/stockfish-12-vs-lc0-psq-threats-screlu-g1-r1@tuned/20261110-0859/`; Skill
      Level 13, a full match, Fattorini 205.5 to 194.5 (+154 -143 =103, 51.4%),
      `data/matches/stockfish-13-vs-lc0-psq-threats-screlu-g1-r1@tuned/20261110-0948/` (its
      90-game scout had Fattorini at 43.3%, within the scout's noise). 13 beaten, so the bracket
      moves up: **next, Skill Level 14** (`bin/arena match stockfish-14
      lc0-psq-threats-screlu-g1-r1@tuned 400 --time 10+0.1 --hash 64 --full`), on an idle machine,
      the Stockfish version noted for the table. The developer expects 14 to win clearly at 10+0.1
      (2026-11-10). For the developer only, never the README: Stockfish 19's own skill formula
      puts 12 and 13 at about 2910 and 2965, so about 2940 on the scale where amanda-0002 had
      2795 (about level 10).
   4. **Lichess installation** with the tuned network (item 3): done 2026-11-10.
2. **Train and tune a PeSTO network:** a forge run on PeSTO's played corpus, its winner tuned.
   **Already measured (2026-11-08, `data/tests/tests.tsv`, tests 1 and 2):** `pesto-psq-threats-screlu`
   run 2's generation 1 (its gate passed, 91.25% against PeSTO), untuned, lost both duels to
   `lc0-psq-threats-screlu-g1-r1`, untuned: 11.5 of 36 at depth 9 (32%), 5.5 of 24 at 10+0.1
   (23%), about 130 to 200 Elo. Same architecture and pipeline, so the gap is the teacher's.
   **To run again (the developer, 2026-11-10):** PeSTO's hand-crafted evaluation tuned first, then
   the pipeline resumed from there, its corpus played by tuned PeSTO (tuned-self-play.md).
   PeSTO's play is filling the training set (about 47% on 2026-11-06), until `complete.txt`.
   Before the training machine trains on it: delete any local `data/pesto` and
   `data/buckets/pesto*` there, left from the old partial corpus.
3. **Lichess, the tuned lc0 network: done 2026-11-10.** `fattorini-bot` plays the `@tuned`
   snapshot of 97071e5 since 2026-11-11 (the root's tablebase score in); the untuned one on Drive can go. Left: `temper-chess` to close. **The
   mark to beat (the developer, 2026-11-10):** `rotor-bot`, retired, ended at 2395 in blitz (RD 51,
   161 rated games, +101 -39 =21, last seen 2026-11-07), from Lichess's public profile data;
   six weeks of mixed engines, the rating still rising when it retired. **Probably beaten, not yet
   settled:** `fattorini-bot` at 2612.8 in blitz on 2026-11-11 (RD 109, just past provisional; +18
   over the last 12 games), better than 99.9% of Lichess's blitz players (Lichess's own figure),
   its day including a draw against a 2819. Compare again once its RD is near Rotor's 51.
4. **The first release: done 2026-11-11.** Fattorini 2026.11.0, `lc0-psq-threats-screlu-g1-r1@tuned`,
   published on GitHub with `fattorini-v2026.11.0.exe`. Candidate `v2026.11.0-rc.1` checked
   (`about`) and its draft deleted, then `v2026.11.0` tagged on the same squashed commit of `main`;
   `gh`'s first real use worked. PeSTO's network not waited for (the developer, 2026-11-11: PeSTO
   has no chance against Lc0). For the next release, DEV.md's steps: the version of that month,
   the CCRL anchors' matches again if the network or the search changed.
5. **CCRL testers:** tell them of the release, in a topic of its own in the CCRL Discussion Board's
   CCRL Public forum (linked from CCRL's pages). Registered 2026-11-11, under the developer's real
   name as the board prefers; waiting for the administrators' activation, then the first two posts
   are approved before they appear. The post drafted, ready to paste: ccrl-announcement.md.
6. **A Chess Programming Wiki page** (the developer, 2026-11-08): drafted 2026-11-11 in
   cpw-page.md, not waiting for CCRL (the developer, 2026-11-11), claiming no rating ever; a wiki
   account next. As planned: facts only (author, first release, AGPLv3, Rust, UCI; the NNUE and how
   it is trained on a limited compute budget; links to the repository, releases and ratings).
   Claude drafts it from the README and `claude/` for the developer to edit; an account is needed.
   Beside it, a minimal author page: the developer's name and GitHub profile, filled in by the
   developer on the wiki and never in a file here, and their other public chess projects, as Aio64
   (a chess tool, not an engine) once its repository is public.

## Before the release or CCRL

- **The tablebase score at the root** (the developer, 2026-11-10, to fix): in KQ vs KN with tables,
  `fattorini-bot` reported +1.59. Search probes only after a zeroing move (`tablebase.rs`
  `should_probe`), so with the capture beyond the horizon every leaf is the network's; the root's
  DTZ ranking keeps play correct (`excluded_root_moves`) but never sets the reported score.
  Designed, plan written: root-tablebase-score.md.
- **The 8-thread, repeating-control and Move Overhead checks: passed 2026-11-11**
  (release-checks.md), cutechess-cli smoke tests on the dry run's `.exe`, no failure of any kind.
- **Clock safety at default settings, not blocking the release** (the checks saw no forfeit): no margin for lag is subtracted, on the
  assumption that the GUI hands over a reduced clock, which is lichess-bot's setting, not a tester's.
  - `MAX_CLOCK_FRACTION` is relative: at 20 ms left it allows 10 ms, while lag does not shrink.
  - `(alpha + 1/M) * T` always exceeds an even share of the time left. Rough estimate: moves using
    their full target reach the last move of a 900 s control with about 3 s, about 0.4 s at 120 s.
    Log the clock left at moves 38 to 40.
  - `CHECK_INTERVAL`'s 10 ms rests on an old 99,000 nodes/second figure: re-measure with a network.
    Neither `go`'s arrival nor writing `bestmove` is counted.
  - Whether depth 1 always finishes on a near-empty clock (`movetime 0` still returns a real move),
    and what it costs.
  - Candidate fix, not decided: a fixed reserve subtracted from `T`, a smaller `alpha` under
    `movestogo`. Run the check through a GUI with no margin of its own.
- **README at the release** (readme-for-humans.md): Strength, the network named: the CCRL Blitz
  estimate alone, no Stockfish (the developer, 2026-11-11), with its full methodology (the developer,
  2026-11-10): the Stockfish 10 bench and S, the scaled control (1'+0.5"), the 10 games at once
  and that the bench ran alone, the anchors with their CCRL ratings and builds, the book, hash,
  tablebases, the games played and the error margin (ccrl-conditions.md); Inside brought to the released architecture; the opening paragraph
  up to date; "Download" linking to this repository's releases page.
- **Say what ships (the developer, 2026-11-10):** the README and `RELEASE-NOTES.md` name the
  network, `lc0-psq-threats-screlu` generation 1: piece-square inputs with simplified threat
  features, SCReLU, trained on Lc0's data. The likely main source of its strength, so said plainly.
- **Credit Lc0's data only if an Lc0 network ships:** `about.hbs` and `RELEASE-NOTES.md` credit it
  whatever the network.
- **CalVer, not SemVer:** implemented 2026-11-11 (calver.md), `2026.11.0`, the month enforced by
  `arena release`. Left: its tests run, then calver.md to `done/`.

## Code to clean up

- **Atan support and lockstep training of several arms:** deprecated by the 2026-11-05 verdict
  (training-corrections.md), kept "for now". Old networks play from a build of the archive, not
  from the new binaries. To remove, after checking nothing still needs them.
- **The Tests form's network select, with the lambda grid** (the developer, 2026-11-08): once a
  generation trains the polyline's one arm alone, "winner (lpolyline)" chooses among one, so the
  select goes when the grid goes, not before.
- **`Control::Fischer` and `MoveLimit::Fischer` renamed `Clock`** (the developer, 2026-11-13,
  cosmetic): since arena-repeating-controls.md they cover repeating clocks too. 13 uses in 6 files
  of the arena.

## Written, not yet seen working for real

Each to confirm the first time it happens:

- A scatter resuming after a stop (resumable-scatter.md).
- A failed segment read waited out, the wait on the page (segment-read-wait.md).
- A non-training phase resumed on a machine without CUDA (cpu-phases.md).
- The polyline measurement's live bar and its Stop (polyline-from-buckets.md).
- The frozen test set's read in a box of its own, at a resume.
- A test killed without ending (a power cut) still listed and resumable (test-row-at-start.md).

## To design

- **A scaled run's share of a partial source.** `scaled_share` takes 1/s of the positions that
  exist, so on an incomplete corpus a scaled generation 1 trains on almost nothing. Proposed: 1/s of
  the full corpus's size, capped at what exists. Not decided (the developer, 2026-11-04).
- **Continuous training** (continuous-training.md): designed, not planned.

## When self-play comes back

Self-play is paused since 2026-11-02: training starts from Lc0's data or PeSTO's.

- **A warm start fine-tunes, so its stages begin at 3e-4** (3e-4, 1e-4, 3e-5), generation 1 from
  random weights keeping all five. Seen once (`-quiet-r1`'s generation 2: the first two stages only
  led back to the teacher's weights); not built.
- **A 1B-position generation 2**, matching generation 1's corpus size, so a verdict on self-play
  after Lc0 does not hang on data size; a failed gate ends it.
- **A self-play generation's buckets outlive it:** a run's `data/buckets/<run>/gen-<g>/` go only when
  the next generation trains, the last never. Delete them once the generation is decided and at the
  run's end.
- **Self-play segments by time, not positions:** a batch stops starting games after some minutes and
  is sealed once its games end, so every machine stops near the target and publishes often. To
  design: elapsed play across a restart, Drive's many more files.

## The halfk line (designed and written 2026-11-02, waiting for training time)

Lc0's labels, one stage, each step one change from the previous winner, duelled at depth 9 then
10+0.1:

1. `halfk_threats_screlu` against `lc0-psq-threats-screlu-g1-r1` (halfk-threats-screlu.md).
2. `halfk_threats_screlu_hidden` against step 1's winner (halfk-threats-screlu-hidden.md).
3. `halfk_threats_pairwise_hidden` against step 2, only if step 2 wins or draws
   (halfk-threats-pairwise-hidden.md).

## Speed (the developer, 2026-11-09)

**The goal: nodes/s at least Stockfish's, one thread, on the same machine.** A shallower depth than
Stockfish's is acceptable; a lower speed is not. With `lc0-psq-threats-screlu-g1-r1`, the arena
benched about 1.15M nodes/s per thread on a quiet machine (the smithy's 841,150 was taken under
load, see "Data and tools"). node-cost.md had amanda-0002, without threats, at 3.32M against
Stockfish 19's 2.03M on the same machine: the threats, recomputed at every evaluation, cost about
two thirds of the speed. Years of Stockfish's optimization are no argument: its
techniques are public, and porting them is quick; what costs time is checking each one (exact
against a from-scratch evaluation) and measuring it (a bench on an idle machine, a duel for search
changes).

1. **Measure:** `bin/fattorini bench` and Stockfish's `bench` on the same idle machine, then the
   bench timing the threat step alone (threats-timing.md), so the share of each cost is known.
2. **Make the threats incremental** (threats-timing.md, designed 2026-10-29): today the threat
   features are recomputed from zero at every evaluation, while psq's are kept incrementally.
3. **Then down the profile:** accumulator handling, move generation, make/unmake and legality, TT,
   `quiescence_from`; each kept only if a bench shows it pays.

## Evaluation

- **X-rays and pins in the threats:** a features experiment of its own, its inference cost timed.
- **Width 512** (psq-threats-screlu-512.md): its state is there.
- **Won endgames:** does a trained network still shuffle once well ahead? Play today's network into
  won endings once a CPU is free.
- **The transformer track, later** (rotor-transformer.md): not before the NNUEs are its baseline; MCTS,
  PUCT with certainty propagation; a policy head (lc0-policy.md, mcts-nnue.md). Its code stays as
  it is until then.
- **The move head** (best-eval-together-with-best-move.md): a move-ordering hint for alpha-beta,
  when started.

## Search

- **Optimize Lazy SMP** (the developer, 2026-11-11): in the release check, 8 threads did not crush 1
  (20 games at 10+0.1, too few to measure, release-checks.md). First measure, on an idle machine
  with enough physical cores: nodes/s at `Threads=1` against `8` (near 8 times is ideal; much less
  points to shared tables contended or hyperthreads), and time to a fixed depth on a few positions
  (about 2 to 3 times faster at 8 is typical; less means the helpers repeat the main thread's
  work). Then the fixes it points to: helper threads diversified (depth offsets, skipped
  iterations), shared counters and histories made per-thread, and a duel at a longer control,
  8 threads against 1, as the measure.
- **An SPSA of time management, after the search's** (the developer, 2026-11-11): the time rules
  depend on the search's curves (a depth's cost, how often the best move changes, the score's
  swings), so they are tuned against the search that ships. Only after Split's verdict
  (blitz-time-management-v2.md) and the smithy's bench on an idle machine (below), ideally after the
  speed work too. **Designed 2026-11-13: time-management-spsa.md.** **Tuned (the developer,
  2026-11-13): the 18 SPSA-born constants of reckless-time-management.md,** from Reckless's values; never its hand-picked ones, the overhead
  and margin among them, which a lag-free tune would drive to zero. At 10+0.1, scaled by the bench as every tune: its increment is 1/100 of the base against
  3+2's 1/90, so the clock goes through the same phases, and a few Elo needs 32k pairs or more. On
  every available arena, each idle: each game is its own comparison, both sides on one machine.
  Then a 180+2 duel of the tuned values against the untuned, for the depth 10+0.1 does not reach.
  **Budgets** (the developer, 2026-11-11), fixed in games, not hours (as spsa.md, 2026-10-16):
  - **The search's SPSA: 32k pairs**, as the last tune, which found +24 Elo with about 10
    parameters; 64k only if the parameters' traces are still climbing at the end.
  - **Time management's SPSA: also 32k pairs,** kept for its 18 parameters (the developer,
    2026-11-13), noisier values accepted. Sized when it had only 4: their optimum is flat (20%
    off costs perhaps 1 or 2 Elo), a weak signal needing many games for its direction. Values ending
    near the start are an answer too: the hand-chosen ones were fine, and the 180+2 duel can go.
  - About 12 hours each over every arena, as the last tune's 32k pairs took (the developer,
    2026-11-11).
- **`blend` as two weights tuned by SPSA**, when SPSA tunes search code.
- **Singular extensions and capture history, retried with a network on both sides:** both were
  judged with the hand-crafted evaluation; one duel each.
- **A `[from][to]` history table** beside or instead of `[piece][to]`, decided by a match.
- **Per-technique benchmarking:** a switch per technique and the bench positions with it on and off.
- **To measure, on an idle machine:** today's search against the old one with a network, nodes at
  depth 9 on the bench positions, and positions per game in the two runs' segments.

## Data and tools

- **A test's Stop once did nothing** (2026-11-13, test 6): clicked on the test's screen, opened
  before the smithy restarted, it sent no request the smithy logged (no "stop asked from the
  page"); a reload, then Stop, worked. Not the token, which every poll refreshes (`poll`). Suspect:
  the card redrawn by a poll losing its handler (`bindStop`). To reproduce before fixing; test 5's
  Stop had worked.

- **A failed claim's reason, and a shorter wait** (2026-11-13, after a power cut): an arena restarted
  and answering (checked with curl on loopback and the LAN) read "no arena answering" for minutes,
  then was claimed with nothing changed, most likely at the next retry of a backoff grown to 5 min.
  `claim_one` reports every `post` error as "no arena answering", an HTTP error code or a timeout
  included, so the page could not say which. To do, after test 8 (it needs a smithy restart): the
  error kept after "no arena answering"; `CLAIM_BACKOFF_CAP` down to about 1 min, or a retry when
  the page is asked.

- **The smithy's bench on an idle machine, as the arenas' is** (the developer, 2026-11-09): to
  design, after the current tune (it changes how a tune's games are timed). The tune of
  `lc0-psq-threats-screlu-g1-r1` showed it: the smithy benched its machine at 841,150 nodes/s per
  thread, while the arena on that same machine benched 1.15M to 1.34M by the same method
  (`loaded_bench`, half the threads at once). The arena holds a lock so its own workers wait; the
  smithy benches when the tune job starts, whatever else the machine is doing. Every arena's clock
  is scaled to the smithy's figure, so the tune played at about 6+0.06 on a quiet machine, not
  10+0.1. To decide: how the smithy makes sure its machine is idle (no chore dealt and every arena
  on it between chores, or the bench refused while anything runs), whether to take the best of a
  few runs, and whether the page warns when the arena on the smithy's machine and the smithy differ
  by more than the dead band. **Seen again 2026-11-13:** test 5 (180+2, Classic against Reckless),
  started with the arena on the smithy's machine playing, dealt clocks of 87.129+0.968 and
  120.122+1.335: about 90 to 120 s blitz at an idle machine's depth, not 180+2. The smithy's
  machine was idle then: the gap is between the processes, most likely the allocator (the arena's
  `mimalloc`, the smithy's system heap). **Fixed 2026-11-13 (smithy-bench.md):** the smithy on
  `mimalloc` and one `loaded_bench`, the fastest of five runs; the two benches now agree within 2%.
  This item goes once that doc is in `done/`.

- **Lc0's Convert, if it runs again:** skip a slow tar only while another worker downloads at normal
  speed (a stall reads as slowness today), and stop retrying a tar skipped N times.
- **The scatter, faster:** only Drive's download bounds it; a wired line or a local copy helps a
  second scatter. Then the single scattering thread.
- **One smithy on any machine** (smithy-anywhere.md): step 7, reproducible buckets; step 8, checks
  across two machines.
- **The smithy as a window** (smithy-window.md): an idea, not urgent.
- **An analysis tool, for Mentes, a separate project:** label any PGN with a player's search.
- **Book lines judged by search, not game results:** to discuss. Fattorini's own book, designed
  here rather than a ready-made one such as Cerebellum (the developer, 2026-11-11), for Lichess
  alone: CCRL testers use their own books, and the arena's matches keep the UHO book. Seen on Lichess:
  the human OTB book (LumbrasGigaBase 2400+) sometimes leaves the bot worse out of the opening;
  meanwhile lichess-bot's `min_weight` raised to 10 and `max_depth` lowered to 12, from the next
  session, to see whether that alone helps.

## After chess settles

- **Other games** (temper-games-restart.md): Go first, not started.
- **Chess variants**, only where a rating list of independently built engines exists. Chess960 is
  dropped for now.
- **Four ways to use the engine,** label source (its own or Lc0's) by hardware (CPU or GPU): details
  later, among them a GPU network in a release without Python or libtorch.
