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

1. **Tune the lc0 network** (`lc0-psq-threats-screlu-g1-r1`), SPSA from the smithy's page:
   32,768 pairs, running 2026-11-09. When it ends, in this order (the developer, 2026-11-09):
   1. **Two duels of the new tune:** against the default parameters, and against the previous tune
      (`tune-16k-old-defaults`, 16,384 pairs, which stopped before reverse futility, razoring and
      aspiration had settled).
   2. **Move Overhead** (move-overhead.md): done 2026-11-09. The duel ended first: the 32k tune
      beat the hand-set defaults, 53.5% over 712 games (about +24 Elo), and ships.
   3. **A strength assessment against Stockfish** (scouting 2026-11-09: against Skill Level 10, a
      full match at 10+0.1, Fattorini won 300.5 to 99.5, +274 -73 =53, 75.1%, about +190 Elo,
      `data/matches/stockfish-10-vs-lc0-psq-threats-screlu-g1-r1@tuned/20261109-1747/`; scouting
      at level 13, Stockfish won 51.0 to 39.0 after 90 games, Fattorini 43.3%; next, the two full
      rows at 13 and 12, moving the bracket if either comes out the other way), for the README and the release (item 4's two
      Skill Level matches).
   4. **Lichess installation** with the winner (item 3), so the developer can start playing it.
2. **Train and tune a PeSTO network:** a forge run on PeSTO's played corpus, its winner tuned.
   PeSTO's play is filling the training set (about 47% on 2026-11-06), until `complete.txt`.
   Before the training machine trains on it: delete any local `data/pesto` and
   `data/buckets/pesto*` there, left from the old partial corpus.
3. **Lichess, the tuned lc0 network:** a `@tuned` snapshot (`bin/arena snapshot`), lichess-bot's
   `engine.name` pointed at it. The bot `fattorini-bot` plays the untuned snapshot until then.
   `temper-chess` to close.
4. **The first release** (binary-release.md, binary-release-implementation.md): the code done and
   its dry run passed 2026-11-06. Left: the two Skill Level matches, `RELEASE-NOTES.md` filled, a
   candidate tag, and the first real draft, which is also `gh`'s first test. The release ships the
   stronger network, expected the tuned lc0 one (decided 2026-11-06).
5. **CCRL testers:** tell them of the release.
6. **A Chess Programming Wiki page** (the developer, 2026-11-08), once the release is public and,
   ideally, CCRL lists it: facts only (author, first release, AGPLv3, Rust, UCI; the NNUE and how
   it is trained on a limited compute budget; links to the repository, releases and ratings).
   Claude drafts it from the README and `claude/` for the developer to edit; an account is needed.
   Beside it, a minimal author page: the developer's name and GitHub profile, filled in by the
   developer on the wiki and never in a file here, and their other public chess projects, as Aio64
   (a chess tool, not an engine) once its repository is public.

## Before the release or CCRL

- **Move Overhead's measurement** (move-overhead.md): the option is in, default 20 ms. On an idle
  machine with a network, the gap from the deadline to `bestmove` over a few hundred `go movetime`
  moves and near-empty clocks; the default changes only if the worst gap asks for more.
- **A repeating-control check.** Lichess sends only Fischer, so the `movestogo` path has never
  played a real game, while CCRL plays 40/15 repeating. A few dozen games at `tc=40/60`, Fattorini
  against itself, on an idle machine: no time forfeits, a sensible clock at move 40, no move near
  the control spending close to the 0.5 cap. Fattorini subtracts no move overhead; if forfeits
  appear, a margin built into the time management, safe at default settings: testers are assumed
  to set no option (the developer, 2026-11-07).
- **Clock safety at default settings, after that check:** no margin for lag is subtracted, on the
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
- **README at the release** (readme-for-humans.md): Strength from the release's two Skill Level
  matches, the network named; Inside brought to the released architecture; the opening paragraph
  up to date; "Download" linking to this repository's releases page.
- **Credit Lc0's data only if an Lc0 network ships:** `about.hbs` and `RELEASE-NOTES.md` credit it
  whatever the network.
- **CalVer, not SemVer** (the developer, 2026-11-08): the scheme to design first. Cargo needs a
  three-part version, so a CalVer such as `2026.11.0` fits it. Touches the workspace's version, the
  `id name`, `arena release`'s tag rule (`v{version}` and `-rc.{n}`) and its tests,
  binary-release.md decision 11, and `RELEASE-NOTES.md`. readme-for-humans.md already weighed a
  `YYYY-MM` name.

## Code to clean up

- **Atan support and lockstep training of several arms:** deprecated by the 2026-11-05 verdict
  (training-corrections.md), kept "for now". Old networks play from a build of the archive, not
  from the new binaries. To remove, after checking nothing still needs them.
- **The Tests form's network select, with the lambda grid** (the developer, 2026-11-08): once a
  generation trains the polyline's one arm alone, "winner (lpolyline)" chooses among one, so the
  select goes when the grid goes, not before.

## Written, not yet seen working for real

Each to confirm the first time it happens:

- A scatter resuming after a stop (resumable-scatter.md).
- A failed segment read waited out, the wait on the page (segment-read-wait.md).
- A non-training phase resumed on a machine without CUDA (cpu-phases.md).
- The polyline measurement's live bar and its Stop (polyline-from-buckets.md).
- The frozen test set's read in a box of its own, at a resume.

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
Stockfish's is acceptable; a lower speed is not. The tune's bench gave 841,150 nodes/s per thread
with `lc0-psq-threats-screlu-g1-r1`. Years of Stockfish's optimization are no argument: its
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

- **`blend` as two weights tuned by SPSA**, when SPSA tunes search code.
- **Singular extensions and capture history, retried with a network on both sides:** both were
  judged with the hand-crafted evaluation; one duel each.
- **A `[from][to]` history table** beside or instead of `[piece][to]`, decided by a match.
- **Per-technique benchmarking:** a switch per technique and the bench positions with it on and off.
- **To measure, on an idle machine:** today's search against the old one with a network, nodes at
  depth 9 on the bench positions, and positions per game in the two runs' segments.

## Data and tools

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
  by more than the dead band.

- **Lc0's Convert, if it runs again:** skip a slow tar only while another worker downloads at normal
  speed (a stall reads as slowness today), and stop retrying a tar skipped N times.
- **The scatter, faster:** only Drive's download bounds it; a wired line or a local copy helps a
  second scatter. Then the single scattering thread.
- **One smithy on any machine** (smithy-anywhere.md): step 7, reproducible buckets; step 8, checks
  across two machines.
- **The smithy as a window** (smithy-window.md): an idea, not urgent.
- **An analysis tool, for Mentes, a separate project:** label any PGN with a player's search.
- **Book lines judged by search, not game results:** to discuss.

## After chess settles

- **Other games** (temper-games-restart.md): Go first, not started.
- **Chess variants**, only where a rating list of independently built engines exists. Chess960 is
  dropped for now.
- **Four ways to use the engine,** label source (its own or Lc0's) by hardware (CPU or GPU): details
  later, among them a GPU network in a release without Python or libtorch.
