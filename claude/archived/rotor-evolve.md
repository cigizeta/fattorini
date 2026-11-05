# rotor-evolve: self-play match runner and tournament infrastructure

**Archived: the original match-runner design, since built as `rotor-arena`** (UCI client,
in-process player, `self`, `replay`, `gate`, `select`, `match`, `sprt`) under rules that changed
since: the bracket gave way to the round robin and the gate (round-robin-selection.md), and
`rotor-curate` is gone. Kept for the history of its decisions (no cutechess-cli, the handshake
timeouts).

## Goal

One module serving two purposes at once, from the same underlying primitive (play games
between two UCI engines, produce an annotated PGN):

1. **Curation data generation.** Rotor-vs-rotor self-play games become new PGN sources for
   the existing `rotor-curate generate` pipeline. No new glue code needed here: `rotor-curate
   generate` already re-derives every label itself (its own depth search replaying whatever
   PGN it's given, per `preprocessing_config.rs` and the "no more teacher concept" design), so
   it doesn't care whether a PGN's moves came from real OTB games or from self-play -- any
   file dropped at `data/games/{stem}.pgn` would be immediately usable from any
   `configs/<net>.yaml`'s `preprocessing.pgn` list. As actually built, self-play output lives at
   `data/self/hand-crafted/` instead, not yet folded into `data/games/` (see "Output location"
   below) -- that link is deliberately not made yet. Self-play from one fixed startpos has the
   same degeneracy problem
   fitness selection does (arguably worse -- a fixed-depth search is even more deterministic than
   an engine on a wall-clock budget): every game collapses to the same one or two lines. The
   opening book being built for fitness selection (see "Opening book" below) is intended to solve
   both at once -- self-play should draw its own starting positions from it too, once this goal
   is actually implemented.
2. **Engine fitness selection.** Arbitrary UCI engines (different Rotor checkpoints, Stockfish
   at various strengths, any other UCI engine) play each other under a decisive match format,
   to decide which is actually stronger.

Both purposes reuse the same match runner; they differ only in what search limit is used and
whether the result of a pairing is acted on.

**Current implementation pass is fitness selection only.** Curation data generation (goal 1,
self-play) is deferred entirely -- not designed as a CLI surface or config format yet, just
kept as the documented second purpose this architecture is meant to also serve later. Nothing
below should be read as "must also handle self-play now"; where a section describes both, the
self-play half is intent, not scope.

## Decision: no cutechess-cli

`claude/TODO.md` previously recorded a decision to adopt `cutechess-cli` as the external match
referee, specifically to avoid a from-scratch match runner. Revisited directly with the
developer: cutechess-cli's built-in tournament formats are round-robin, gauntlet,
knockout (single-elimination only), and pyramid (confirmed against its own help text) -- no
double-elimination, which this project wants. A full reimplementation was initially pushed
back on (cutechess-cli absorbs years of UCI-compatibility edge cases), but the developer's
call is that a match runner is not actually hard to get right here, especially since Rotor
already has the pieces that matter: legal move generation, checkmate/stalemate/repetition/
fifty-move detection (`play_self_play_game` in `rotor_curate.rs` already implements exactly
this loop, just as a manual test driver with no persistence). What's genuinely new is a UCI
*client* (Rotor driving an arbitrary engine subprocess), which `uci.rs` does not provide today
(it's a UCI *server* -- Rotor answering as the engine).

**`claude/TODO.md`'s "Strength-measurement framework" entry needs a note added: this decision
is superseded, replaced by this document.**

## Architecture

### 1. UCI client

Spawns an engine subprocess (arbitrary binary + args, from the engine registry), speaks UCI
over its stdin/stdout: `uci`/`isready`/`ucinewgame`/`position`/`go`, parses `info score cp/mate
...` and `bestmove` lines. Rotor's own `uci.rs` already emits `score cp`/`score mate` in its
`info` lines (confirmed directly, `uci.rs` tests), so no engine-side changes are needed for
Rotor to be a valid participant on either side of this client.

**Handshake timeouts**: reusing cutechess-cli's own real, source-confirmed defaults
(`chessengine.h`) rather than picking arbitrary numbers -- **35s** for the initial `uci` ->
`uciok` protocol handshake (generous enough to cover a slow engine loading something at
startup, e.g. Stockfish's NNUE file, or Rotor's own network once that exists), and **15s** for
`isready` -> `readyok`. Either one timing out is a protocol-violation fault -- zero tolerance,
same as any other fault (see Match runner below): the offending engine is disqualified from
the whole pairing immediately.

### 2. Match runner

Given two UCI clients and a search limit (`depth=N`, or a Fischer clock spec like `3+2` --
see Clock below), plays one game to conclusion. The **arbiter owns game-end detection** --
checkmate, stalemate, threefold repetition, fifty-move rule, strictly the rules of chess, no
adjudication of any kind (developer decision, 2026-09-22, matching self-play's own -- see
"Self-play data generation" below -- except tournaments keep the real threefold repetition
rule untouched, rather than self-play's own deliberate twofold deviation). Engines only ever
answer "what's your move from this position," never self-declare a game over.

**No ply cap.** `play_self_play_game`'s own `SELF_PLAY_MAX_PLIES` safety net was considered and
explicitly removed per developer instruction, once the fifty-move-rule check (unconditional,
not waiting on a player's own claim -- the same way this arbiter already checks it) was
recognized as already guaranteeing termination on its own: a pawn move or capture is the only
thing that can ever reset that counter, and both are strictly finite over any legal game, so a
separate backstop adds nothing. Checked against real practice too: TCEC uses eval-based
adjudication (a draw once the evaluation stays near zero at low material for several plies), not
a flat ply count, and CCRL documents no such rule at all -- only cutechess-cli's own optional
`-maxmoves` is directly comparable, and this project isn't using it.

**Clock**: real Fischer time control only (base + increment, e.g. `3+2`), not a flat per-move
budget -- tracks each side's own remaining time (`wtime`/`btime`), sends it on every `go` along
with the increment (`winc`/`binc`), and decrements it by the actual measured wall-clock time
between sending `go` and receiving that side's `bestmove`. A side whose clock reaches zero loses
on time -- a new termination reason alongside checkmate/stalemate/etc.

**Fault handling has zero tolerance, decided directly with the developer** -- and is a
different thing from losing on time. A normal game-ending outcome (checkmate, stalemate,
repetition, fifty-move, time-forfeit) is a legitimate result that counts toward the
pairing's ongoing 6.5-point race like any other game. A **fault** (process crash/unexpected
exit, illegal or malformed move, protocol violation such as not answering the initial
`uci`/`isready` handshake) is not a chess outcome at all -- it means the engine is broken, not
that it lost a fair game -- so it immediately disqualifies the offending engine from the *whole
pairing*, no matter the current score: the non-faulting engine wins the pairing outright, no
further games played. This matters most once Rotor itself joins the registry, since it's under
active development and more likely to hit a bug than a mature opponent -- a single crash should
surface as a real signal, not get smoothed over by replaying more games. A hang specifically
still needs no separate detection mechanism: an engine that never responds simply runs its own
clock to zero and loses on time (a normal outcome, not a fault).

Naive-first scope: sequential games only (one at a time), no draw/resign adjudication
heuristics (games always end via real rules alone -- see "No ply cap" above for why that's
sufficient on its own). Concurrency and adjudication heuristics are explicitly deferred, not
designed now, added later only if throughput or wasted-game-time actually become a real
problem.

Single-threaded play is enforced by the match runner itself -- always issuing
`setoption name Threads value 1` to every engine it spawns, regardless of what the engine
registry says -- not left to per-engine configuration in `data/engines.yaml`. Keeps every
comparison apples-to-apples on a single, non-cluster machine and avoids CPU oversubscription
once concurrency is eventually added.

**Progress output** (`indicatif`, already a dependency via `rotor_curate.rs`): live display of
current round, current match index (which pairing within the round), current game index
(within the pairing), current ply (within the game), the two engines actually playing (names,
plus which is white this game), the pairing's own running score so far (e.g. `3.5-2.0`, a cheap
read off `results.db`), and total elapsed wall-clock time for the whole run (`FormattedDuration`,
same as `rotor_curate.rs` already uses) -- relevant given these runs can span days on a machine
that isn't on 24/7. Round/match/game each get a real bar with a known total (round count and
games-per-pairing are both fixed upfront); ply is a plain counter, not a bar, since game length
varies and there's no meaningful total to show progress against. Remaining clock time (`wtime`/
`btime`) per side is deliberately left out of this output for now, despite the match runner
tracking it internally for the Fischer clock -- not needed yet, can be added later if wanted.

### 3. PGN writer

One game -> one PGN entry. Each move gets an annotation comment, **matching cutechess-cli's own
format and field order exactly** (decided directly with the developer, after briefly considering
and rejecting both "no annotation" and "score only" -- landed back on the fuller format,
cutechess-shaped): `{<score>/<depth> <time>s}`, e.g. `{0.35/16 5.1s}` -- score first, in decimal
pawns (not centipawns), then `/` and the depth, then a space and the elapsed time for that move
with a trailing `s`. Confirmed against cutechess's own `evalString` (`chessgame.cpp`): depth is
omitted from the comment if not available (`eval.depth() > 0` guard) and time is always present,
`0s` as its own floor rather than omitted. Written alongside a sibling results database -- see
"Results database and resumability" below.

**Output location depends on which of the two goals a run serves** (`data/games/` is
curation-usable data only, never fitness-selection output):

- Curation data generation (rotor-vs-rotor, depth 9): `data/self/hand-crafted/{stem}...` (see
  "Output" under "Curation data generation" below for the real, as-built shape -- numbered
  `.pgn.zst` segments, not a single `.pgn`), deliberately *not* `data/games/` -- self-play output
  isn't yet wired into `preprocessing_config::pgn_file_path`'s own `data/games/{stem}.pgn`
  expectation, kept apart under its own eval-identifying name until the developer decides how it
  should fold into that pipeline (decided directly with the developer: "we will generalize
  later").
- Fitness selection (a tournament, or even a single pairing outside a bracket): its own
  directory, `data/tournaments/<id>/`, holding `config.yaml` (the spec), `games.pgn`, and
  `results.db` -- fixed names, no stem needed, since the directory itself is the namespace. Kept
  entirely out of `data/games/` since these games (played at 3+2/2+1 against strength-limited
  opponents) aren't the distribution curation wants to train on.

### 4. Engine registry

`data/engines.yaml`, one shared file, one entry per UCI participant: binary path, fixed options
(Hash, Threads, SyzygyPath, ...), and for strength-limited opponents, generic option key/value
pairs (e.g. `UCI_LimitStrength`/`UCI_Elo` for Stockfish) rather than engine-specific logic --
keeps this genuinely generic across arbitrary engines, not Stockfish-specific. Lives under
`data/`, not `configs/`: a binary path is local/machine-specific (varies per checkout), unlike
the hand-written, checked-in configs `configs/` otherwise holds. Currently populated with 21
Stockfish entries (`stockfish-skill-0` through `stockfish-skill-20`), one per `Skill Level`
value -- chosen directly by the developer over the earlier `UCI_LimitStrength`/`UCI_Elo`
approach (worth keeping in mind: `Skill Level` isn't linearly calibrated to Elo the way
`UCI_Elo` is meant to be, so the 21 entries are an ordered ladder of opponents, not evenly
spaced strength points). Rotor itself is deliberately left out until it has a runtime way to
select a trained network (see Open items). Each entry also sets `Hash: 64` (Rotor's own
`TT_SIZE_MB`, reused here for consistency) and `SyzygyPath: syzygy-3-4-5-wdl:syzygy-6-wdl` --
the same two directories `rotor_curate.rs`'s `SYZYGY_DIRECTORIES` already expects, `:`-joined
(the OS list separator, confirmed as Stockfish's own convention on Unix too, matching
`uci.rs`'s `SetOptionSyzygyPath` parsing). Relative, not absolute -- assumes `rotor-evolve` is
invoked from the repo root, the same convention `rotor-curate` already relies on for these same
directories. `Threads` is deliberately absent from every entry: the match runner enforces
single-threaded play itself (see Match runner above), not the registry.

## Two match profiles

### Curation data generation (rotor-vs-rotor)

- Fixed search limit: `depth 9` -- matches `DEFAULT_SEARCH_DEPTH` in `rotor_curate.rs`, the
  same depth `rotor-curate generate` already uses to relabel any PGN.
- Search settings (decided directly with the developer): Syzygy tablebase probing on, same as a
  normal game; `64` MB transposition table per color, matching the `Hash: 64` convention every
  entry in `data/engines.yaml` already uses; single-threaded; no pondering. **Two separate
  tables, one per color, not one shared table for the whole game** -- decided directly with the
  developer: a real match has two independent engine processes, neither ever seeing the other's
  own TT entries, so one process searching both sides of a self-play game must keep the same
  separation itself, or each side's search would see cached results the other side's own search
  produced, something no real opponent match-up could ever give it. `128` MB total per worker
  thread (`64` MB x 2), not `64` MB split between them.
- No winner concept, and no target game count either -- an earlier `--games` ceiling flag was
  removed entirely (decided directly with the developer). Generation as a whole is meant to be an
  unbounded, infinite stream, but not by looping forever inside one process: each run stops itself
  once its own current file reaches `CHECKPOINT_GAMES` (see "Output" below), so an external
  process manager (not this code) is what actually keeps the stream going, simply by restarting
  the same command.
- The exploration window's own randomness (`M`, and the fallback-to-best when nothing else is
  eligible) draws from system entropy, not a seeded RNG -- decided directly with the developer:
  an interrupted game is simply not recoverable (same Ctrl+C-drops-the-current-game norm the
  fitness-selection match runner already uses), so nothing depends on reproducing one exactly.
  Resumability at the batch level means picking up generation from however many complete games
  the count file already reflects -- not replaying any single game's own randomness, and, as
  actually built with `RandomBook`, not "the next book index" either: each game draws an
  independent random opening, so there is no book-index sequence to resume from in the first
  place.
- **Parallelism across games, not within a game's own search.** Each game's search stays
  single-threaded (the earlier search-settings call), but at a fixed depth there's no reason to
  generate 5M+ games one at a time -- a `--threads` CLI option (decided directly with the
  developer) runs that many games concurrently, each an independent worker drawing a random book
  opening and playing a full game start to finish.
- **Output: timestamped segments, `data/self/{parent}/{stem}-{YYYYMMDD-HHMM}.pgn.zst`, one
  per completed batch, `hand-crafted` the only `parent` today.** `data/self/{parent}/`, not
  `data/games/` -- see "Output location" above for why. Named by compression time (when the
  batch finished; originally start time, changed per self-play-multi-machine.md), not an opaque
  incrementing number -- decided directly with the developer, revisiting an earlier
  numbered-segment design (`{stem}-1.pgn.zst`, `{stem}-2.pgn.zst`, ...): a timestamp is a
  meaningful, sortable identity a bare counter isn't, and stays useful even though `net.yaml`'s
  own `preprocessing.parent` (`preprocessing_config.rs`, superseding the earlier per-segment
  `preprocessing.self_play` design this note originally described) no longer needs to name a
  specific segment explicitly -- it names the parent alone, and every segment under it is
  discovered by `prepare` automatically. UTC, `YYYYMMDD-HHMM`
  (no seconds) -- granular enough that two segments for the same stem can never collide, since a
  single `CHECKPOINT_GAMES` batch takes well over a minute either way (the developer's own call:
  "I will take more than one day to generate one file"). Compression-batch size and
  crash-recovery granularity are deliberately decoupled, settled directly with the developer
  after a first pass conflated them: every completed game is appended immediately to a
  plain-text "current" staging file, `data/self/hand-crafted/{stem}.pgn` (a fixed name; it was
  `{stem}-{start-timestamp}.pending.pgn` before self-play-multi-machine.md), flushed to disk every
  game -- ordinary file I/O, no zstd involved, so nothing is ever at risk of being lost to a crash
  regardless of where a run sits in the compression cycle. Once the current file reaches
  `CHECKPOINT_GAMES` games, it's compressed wholesale into `{stem}-{timestamp}.pgn.zst.tmp`
  (timestamp taken at that moment), synced, renamed to `{stem}-{timestamp}.pgn.zst` (never over an
  existing segment), the staging file and then `{stem}.count` are removed (the count only ever
  covers the current staging file), and the whole
  process stops itself right there -- trips `stop_requested` exactly as a real Ctrl+C would (so
  every worker still mid-game notices and stops promptly), then returns -- rather than starting a
  fresh pending file and continuing (the developer's own words: "compress and stop the process, do
  not start a new current file"). A crash mid-batch loses nothing: every played game already sits
  safely in the plain staging file; resuming (a fresh invocation of the same command) means
  appending to that same still-partial `{stem}.pgn`, never regenerating anything. Multiple
  machines: each runs with its own `--stem`, and only finished `*.pgn.zst` segments are copied to
  the training machine (rsync is safe: it writes to a hidden temp name and renames when done);
  `rotor-train` keeps its list of used segments and picks any unused one available, re-globbing
  after each (self-play-multi-machine.md). Within one machine, a `rotor-self.lock` advisory lock
  allows one `rotor-self` at a time, and at most one stem may have an unfinished batch.
  `CHECKPOINT_GAMES` was
  originally `1,000,000`, sized for a 5M+-game target and coarser-compresses-better reasoning;
  lowered to `128 * 1024` (`131,072`) once a rough positions-per-game estimate (self-play's own
  adjudication rules -- mate/tablebase/+-1000cp/fishtest draw -- all cut games short of a natural
  conclusion, so shorter than a typical ~80-ply human game, order-of-magnitude 40-70 plies)
  showed a single file at the old size would already hold tens of millions of positions, far more
  than needed for a first pass -- decided directly with the developer, who confirmed one file
  would already be enough.
- **Progress output** (`indicatif`, the same `[{elapsed_precise}] {bar} {pos}/{len} ({percent}%)
  ETA: {eta_unsmoothed}` template and hand-rolled unsmoothed-ETA key `rotor_curate.rs`'s own
  progress bar already established, in place of `indicatif`'s own doubly exponentially smoothed
  `Estimator`): total is always `CHECKPOINT_GAMES`, the current segment's own known length, and
  the bar's own starting position is `completed % CHECKPOINT_GAMES` -- so resuming a still-partial
  pending file (a fresh process picking up where an earlier, interrupted one left off) starts the
  bar already part of the way there, rather than misreporting a fresh zero. `finish()` once the
  segment actually reaches `CHECKPOINT_GAMES`, `abandon()` if the run ends earlier via Ctrl+C --
  the same completed-vs-interrupted distinction `rotor_curate.rs`'s own bar draws, on the same
  `ProgressFinish::AndLeave` vs `Abandon` grounds.
- Kept as a fully separate flow/binary from tournament pairings below (`rotor-self`, not
  `rotor-evolve`) -- different search limit, different stopping rule, different purpose.
- **Evaluation: hand-crafted, matching `rotor-evolve`'s own Rotor participants -- material plus
  piece-square tables, King PST tapered by `game_phase` -- not the trained network** (there isn't
  a usable checkpoint yet, and self-play's whole purpose is generating training data, so it can't
  depend on training already being done). Reversed, 2026-09-14 -> 2026-09-22: self-play originally
  used a genuinely material-only variant (`eval::scaled_pure_material_value`, `material` value
  with no `pst_value` term at all, gated behind a `material-only-eval` Cargo feature that swapped
  `evaluate`'s body at compile time), on the reasoning that `material_value` already folds PST in
  alongside material, so a "material-only" mode needed its own dedicated function. Found, directly
  from real generated games, to have a real cost: a fully deterministic search past the first 6
  exploration plies, combined with zero positional signal once a position locks up (no captures
  available), meant the search had nothing to differentiate one quiet move from another and easily
  drifted back into a position it had already visited -- an early twofold-repetition draw, visible
  directly once `[Termination]` tags existed to diagnose it. Switched to `scaled_material_and_
  positional_value` instead (the same function a plain `rotor` build, and so `rotor-evolve`'s own
  Rotor participants, already use) -- decided directly with the developer: both self-play and
  tournament play should learn from the same hand-crafted evaluation's own genuine preferences,
  not a cheaper substitute unique to self-play. `scaled_pure_material_value`/`material-only-eval`
  are kept, not deleted (nothing currently requires them, but they still work for a future targeted
  comparison). Self-play is still its own binary, `rotor-self`, not a subcommand of `rotor-evolve`
  (`bin/rotor-self` wraps `target/release/rotor-self`) -- not for any evaluation-feature reason any
  more, but because it plays in-process (no UCI subprocess, no `EngineRegistry`/`UciClient`), a
  genuinely different architecture from `rotor-evolve`'s own external-process match running. Both
  binaries build **release only** now (performance matters at self-play's 5M+-game target, and
  `bin/rotor-evolve` builds release too, for real tournament play).
- **In-process, not UCI.** Rotor playing itself has no reason to pay subprocess/pipe/UCI-text
  overhead talking to itself -- self-play calls Rotor's own search function directly (`rotor` is
  already a library crate other bins in this project link against), rather than going through
  `EngineRegistry`/`UciClient`, which exist specifically for driving *arbitrary external* engines
  (Stockfish, etc.) and don't apply here.
- **Diversity beyond the opening book's own 2.5M positions.** One opening per game (index =
  games already written -- no color-pairing concept, since self-play has no second engine
  identity to keep fair) avoids everyone starting from one fixed position, but reusing a book
  position with a fully deterministic search still just reproduces the same game. So for the
  first `N = 6` plies only, Rotor doesn't always play its own best move. The exact mechanism
  (settled directly with the developer, chosen as a search-cost optimization over always running
  `MultiPV 3`): draw `M` uniformly from `{1, 2, 3}`, search with `MultiPV = M` (only that many
  lines get computed this ply -- `M=1` is a plain single-PV search, the cheapest case), then play
  the *lowest-ranked* of those `M` candidates that's still within 50cp of the best move's score
  (`M=1` -> always the best move outright; `M=2` -> the 2nd-best if eligible, else best; `M=3` ->
  3rd-best if eligible, else 2nd, else best). This isn't perfectly uniform over the theoretical
  top-3 pool (biased toward the best move, since a third of the time no alternative is even
  computed), but it's cheaper on average than always computing 3 PV lines, which is what actually
  matters at self-play's target volume. After ply 6, play normally (always the top move). This is
  what actually uncaps game count past 2.5M -- book position x random branching over a handful of
  plies combinatorially exceeds any single fixed target (5M+) even with heavy book reuse.
- **An exploration ply writes the *best* move's score, not the played move's** (corrected
  2026-09-25; it wrote the played move's before). The `{ }` annotation is the position's own
  depth-9 minimax value, which is a property of the position and not of whichever move exploration
  picked next, and past ply 6 the two coincide because the played move is the best move -- so
  writing the best move's score is what makes `score_cp` mean the same thing at every ply. Writing
  the played move's biased the first six labels of every game downward by up to 50cp, always in
  that one direction, since the mechanism deliberately picks the *lowest*-ranked eligible
  candidate. It matters out of proportion to 50cp: AB(lambda) draws most of an opening ply's label
  from just those few scores (`L_1` takes `0.3 * sum(0.7^k)`, about 88%, from plies 1 to 6), and
  evolutionary-training.md's grounding argument puts the opening where the game result has
  effectively no corrective weight, so the bias would be learned and would persist across
  generations.
- **Eval-to-centipawns for the 50cp filter -- turned out to be a non-issue.** Checked directly
  against `eval.rs` before implementing (correcting an earlier, premature note here): `eval::
  evaluate` has no neural network in it at all today -- it's 100% hand-crafted
  (`scaled_material_and_positional_value`: material plus piece-square tables, already in true
  centipawns via `piece_value`). The `[-1, 1]`-scale raw network output `rotor_train/labels.rs`'s
  `clamped_pawns_from_score` converts is a *future* trained checkpoint's own output, not anything
  `search.rs`/`eval.rs` touch yet (matches the still-open "Rotor needs a runtime way to select
  which trained network it plays with" item elsewhere in this doc). So the 50cp filter compares
  directly against `MultiPvLine`/`IterationReport.score`, no conversion needed -- self-play uses
  this exact same hand-crafted eval now too (the "Evaluation" bullet above covers the 2026-09-22
  reversal away from a separate material-only variant), so revisit only once self-play switches
  to a trained network instead.

### Fitness selection (arbitrary engines, tournament pairings)

Decision rule per pairing, chosen directly with the developer over an SPRT-based stopping rule
(rejected: needs elo0/elo1/alpha/beta tuning that doesn't obviously transfer from fishtest's
Stockfish-scale defaults) -- a World-Championship-style match format instead:

- Base control: **FIDE blitz, 3+2** (3 minutes + 2s increment) by default, for up to **12**
  games per pairing by default, alternating colors. Win = 1, draw = 0.5, loss = 0. `engine_one`
  (the stronger seed) plays white in `game_index = 0`, then colors alternate every subsequent
  game -- deterministic, no coin flip.
- **`best_of` is config-overridable, not a fixed constant** (`match.best_of` in `config.yaml`,
  default 12 if omitted -- decided directly with the developer, matching the earlier call that
  time controls are configurable too, not just the two specific values chosen for
  `data/tournaments/2026-09-22/config.yaml`, which sets `best_of: 2` explicitly for a faster
  first shakedown run). The win threshold is *derived*, not separately configured, so it always
  stays a clean majority: `best_of / 2 + 0.5` (`1.5` at `best_of: 2`, `6.5` at the default of
  12). **Must be even**, for clean color balance -- checked at config-load time, panicking on an
  odd value rather than silently misbehaving later (the same discipline
  `preprocessing_config.rs`'s `assert_lambda_precision` already established for `net.yaml`'s own
  `lambda` field, CLAUDE.md rule 4: assertions, not eyeballing).
- Stop immediately and declare a winner the moment either side's cumulative score reaches the
  win threshold (can happen before all `best_of` games are played).
- If still tied after `best_of` games: play additional **pairs** of games at the tiebreak
  control, repeating pairs until one side is strictly ahead. Tiebreak batch size stays fixed at
  2 games (matching the real World Championship format), not itself separately configurable --
  only `best_of` and the two time controls are. No escalating/armageddon-style tiebreak
  (rejected: armageddon's draw-odds mechanism relies on human psychology under pressure, which
  doesn't apply to engines -- draw odds don't change what move an engine thinks is best).
- Tiebreak control: **2+1** (bullet-convention, not FIDE-defined -- FIDE has no official bullet
  category at all, only classical/rapid/blitz by the base-time-plus-60x-increment formula).
  Chosen over 1+0 (equally common in practice) specifically because the 1s increment is
  earmarked as the per-move safety margin against the match runner's own subprocess/pipe
  communication overhead -- confirmed via research that fast time controls need an explicit
  overhead reservation (commonly ~250ms in other engine-testing tooling) since Rotor's `uci.rs`
  does not currently expose a `Move Overhead`-style option of its own.
- Search limit stays a configurable field on the match spec generally (not hardcoded to a
  clock) -- fixed-depth stays valid for rotor-vs-rotor comparisons specifically, since both
  sides share identical search code and pruning behavior; it is not a fair comparison against
  an engine with different search architecture (Stockfish reaching depth 44 vs. Rotor's depth
  10 at equal *time*, already measured directly and recorded in `claude/TODO.md`, is exactly
  why depth is not the right axis to compare across different engines).

## Tournament: single-elimination knockout

Simplified directly with the developer: single-elimination only, not double-elimination.
(cutechess-cli's own `knockout` format is single-elimination too -- the earlier motivation for
avoiding it, wanting double-elimination, no longer applies, but the decision to own the match
runner ourselves stands on its own merits by this point, see "Decision: no cutechess-cli" above.)

A bracket scheduler sits on top of the match runner: decides pairings round by round from prior
results, invoking one pairing (the World-Championship-format match above) per bracket matchup.

**Seeding is the participants list's own order** in `config.yaml` -- position 1 is seed 1 (the
top seed), no separate seed field. Byes and the full bracket tree (which pairings can only meet
in later rounds) are both derived from one recursive construction, computed once upfront, not
per round:

```
order(1)  = [1]
order(2k) = for each seed s in order(k): emit s, then emit (2k + 1 - s)
```

Doubled up to `P` = the next power of two >= the participant count `N` gives a bracket-position
-> seed-number mapping of length `P`. Round 1 pairs are consecutive entries in `order(P)`; a
pair is a bye whenever its higher seed number exceeds `N` (no such participant exists). Every
later round pairs up whichever entrant currently occupies each bracket slot (a real round
winner, or a bye-advanced participant) -- no further seeding math needed per round, since this
one sequence already fixes the entire tree. This is what guarantees seed 1 and seed 2 can only
meet in the final (and more generally, seed `k` and seed `k+1`-scale collisions are pushed as
late as the tree allows), not just that byes are assigned to top seeds.

Worked example, `data/tournaments/2026-09-22/config.yaml` (N=21, all 21 Stockfish skill
levels, seed 1 = `stockfish-skill-20` down to seed 21 = `stockfish-skill-0`): `P` = 32,
`order(32)` = `[1,32,16,17,8,25,9,24,4,29,13,20,5,28,12,21,2,31,15,18,7,26,10,23,3,30,14,19,6,
27,11,22]`. Round 1: seeds 1-11 (`skill-20` through `skill-10`) get byes; the 5 real matches are
`skill-5`v`skill-4`, `skill-8`v`skill-1`, `skill-9`v`skill-0`, `skill-6`v`skill-3`,
`skill-7`v`skill-2` -- note this round-1 output is identical to the simpler "pair `i` with
`P+1-i`, byes where the partner exceeds `N`" formula; the recursive construction only changes
*later*-round structure (e.g. seed 1 is paired against the winner of `skill-5`v`skill-4` in
round 2, seed 2 against the winner of `skill-6`v`skill-3` -- different branches, confirming
seeds 1 and 2 can't meet before the final).

## Results database and resumability

Superseding the earlier "count games in the PGN" idea: a plain game count is enough to resume a
flat N-game batch, but not a tournament, where resuming means knowing which bracket pairings are
already decided, which one is mid-pairing, and that pairing's current running score. So every
`rotor-evolve` run -- data-gen batch, single pairing, or tournament alike, not tournaments only
-- writes a **results database** alongside its PGN, `results.db` next to `games.pgn` (see "3.
PGN writer" above for exactly where that pair lands: `data/tournaments/<id>/*` for
fitness-selection runs). One shared mechanism rather than two -- except self-play, as actually
built, doesn't use it: `self_play.rs` has no results database at all, just the plain `.count` file
described under "Output" above (`data/self/hand-crafted/{stem}.count`) -- a tournament needs to
know which bracket pairings are decided and each one's running score, but a self-play batch has
no bracket, no running score, nothing beyond "how many games so far," so the fuller mechanism this
section originally motivated was never actually needed there.

Schema, one table, per-game rows, normalized round/engine columns rather than a free-form
pairing id (chosen over a per-pairing summary specifically so this file doubles as tier-1
analysis's own input -- result tally, termination-reason breakdown -- without re-parsing the
full annotated PGN):

```sql
CREATE TABLE games (
    game_id     INTEGER PRIMARY KEY,
    round       INTEGER NOT NULL,
    engine_one  TEXT NOT NULL,    -- the pairing's two engine registry names, normalized: by
    engine_two  TEXT NOT NULL,    -- convention engine_one is always the stronger seed (lower
                                   -- seed number). A pairing is identified by (round,
                                   -- engine_one, engine_two) together, not a single free-form
                                   -- id -- directly queryable, no string formatting/parsing.
    game_index  INTEGER NOT NULL, -- 0-based index within the pairing
    white       TEXT NOT NULL,    -- engine registry name (color alternates per game_index,
    black       TEXT NOT NULL,    -- independent of the engine_one/engine_two strength order)
    result      REAL NOT NULL,    -- 1.0 / 0.5 / 0.0, white's score
    termination TEXT NOT NULL,    -- normal outcomes (count toward the pairing's 6.5-point
                                   -- race): checkmate / stalemate / repetition / fifty-move /
                                   -- time-forfeit. Fault outcomes (crash / illegal-move /
                                   -- protocol-violation): decide the whole pairing immediately,
                                   -- regardless of score so far -- see Match runner's "zero
                                   -- tolerance" fault handling above.
    ply_count   INTEGER NOT NULL
);
```

A pairing's current state is a query keyed on `(round, engine_one, engine_two)`: fault present
-> decided (see below); else sum each side's score across its rows against 6.5 and the
12-game/tiebreak-pair rule from "Fitness selection" above. A fault-terminated game is, on its
own, sufficient for the scheduler to consider that pairing decided (non-faulting side wins) --
no need to check the running score or game count at all once one exists for a pairing.

Follows the same discipline `rotor_curate.rs`'s own database already established (CLAUDE.md
rule 8-adjacent: measure/build correctness in, don't bolt it on) -- each game's row commits in
its own transaction, so a Ctrl+C-dropped in-flight game (see below) simply never gets a row,
rather than risking a half-written one. Unlike `rotor-curate generate`'s database, no separate
`resume` table is needed: there's no pre-existing external PGN being sequentially replayed here,
so the `games` table itself *is* the resumable state -- on startup, a pairing's current score
(and whether it's already decided, mid-tiebreak, etc.) is just a query over its own rows, and
full bracket state (which pairings still need playing) is reconstructed fresh from the
participant list plus whatever rows already exist, not stored separately as mutable bracket
state that could drift out of sync with the games actually played.

Ctrl+C drops the current in-flight game (never written, to either the PGN or this database) and
stops cleanly rather than waiting for it to finish -- the same shape `rotor_curate.rs`'s own
`ctrlc::set_handler` + `Arc<AtomicBool>` stop flag already has (`run_generate`), except tripping
it here means stopping the engine currently to move (UCI `stop`, or killing its subprocess)
rather than aborting an in-process search. Applies uniformly across all three run kinds, same as
the results database above.

## Implementation phasing (naive-first)

1. **UCI client + match runner + PGN writer + results database.** Given two registered engines
   and a search limit, plays one World-Championship-format pairing to a decided result: N games
   sequential, arbiter-owned game-end detection, faults adjudicate as a loss, rules-only
   termination, Ctrl+C drops the in-flight game cleanly, `{score}`-annotated PGN + sibling
   results database written out. This is the core primitive everything else calls -- testable
   directly, before the bracket scheduler (phase 3) exists to drive it from a real config file.
   Scoped to fitness selection only for this pass (self-play/curation data-gen is deferred, see
   Goal above).
2. **`rotor-evolve analyze <stem>`** -- tier-1 PGN analysis: result tally, Elo estimate,
   termination-reason breakdown (catches real bugs like an illegal-move loss, not just a
   number) -- the piece `claude/TODO.md` already scoped as the next concrete step, now built
   generically rather than Stockfish-specific.
3. **Single-elimination bracket scheduler** on top of (1), for tournaments with more than two
   entrants, applying the World-Championship-format decision rule per pairing.
4. **Self-play data generation** (Goal 1, taken up once (1)-(3) proved out the match runner and
   the opening book existed). Concrete file layout:
   - `eval.rs`: `pure_material_value`/`scaled_pure_material_value` (mirrors `material_value`/
     `scaled_material_and_positional_value` but omits the `pst_value` term), `evaluate` gated
     between the two via `#[cfg(feature = "material-only-eval")]`.
   - `Cargo.toml`: `material-only-eval = []` feature; `zstd` moved off the `training`-only gate
     to an unconditional dependency (still pure Rust, no extra build requirement -- the same
     reasoning `serde`/`serde_yaml` already got -- now that self-play needs it too, not just
     `rotor-train`).
   - `src/bin/rotor_evolve/self_play.rs` (new): the game loop, modeled directly on
     `rotor_curate.rs`'s own `play_self_play_game` (same unconditional checkmate/stalemate/
     fifty-move/twofold-repetition checks, same no-ply-cap reasoning already settled for the
     fitness match runner) but starting from a book FEN, calling
     `choose_best_move_multipv_with_progress` for the first 6 plies (`M` from `{1,2,3}` via
     system entropy, 50cp-filtered lowest-eligible pick) and plain `choose_best_move` after, one
     `TT_SIZE_MB = 64` table per color (`tt_white`/`tt_black`, selected by `side_to_move` -- see
     "Curation data generation" above for why one shared table won't do)/`DEFAULT_SEARCH_DEPTH =
     9`/Syzygy probing on/single-threaded exactly as
     settled above. **No adjudication of any kind, final decision (2026-09-22).** Self-play games
     are played strictly by the rules of chess, one deliberate deviation: twofold repetition ends
     the game a draw, not the real threefold. That leaves exactly four ways a game ends --
     checkmate, stalemate, the fifty-move rule, and twofold repetition -- all unconditional, all
     already settled above and in `play_one_game`'s own doc comment. Every move, including the one
     that reaches a forced-mate or tablebase-scale score, is played out for real and recorded with
     its honest evaluation: a real mate gets an exact `mate_in` count (checkmate found at a
     definite search ply, the same conversion `uci.rs`'s own `format_score` uses for a UCI `score
     mate N` reply), everything else -- including a tablebase-scale score, since Rotor only has WDL
     tables, no DTZ (`tablebase.rs`'s own doc comment), so there is no real distance to report for
     one -- gets its raw score reported as `score_cp` as-is, even when that number is enormous and
     self-evidently non-material. This mirrors what real Stockfish's own `UCI::value` does too
     (only takes its `mate` branch once a score clears `VALUE_MATE_IN_MAX_PLY`; a tablebase-adjusted
     score is deliberately kept just below that, mirroring `TB_WIN_SCORE` sitting below `MATE_SCORE
     - MAX_PLY` here, so it falls through to the ordinary `cp` branch). `self_play.rs`'s own doc
     comment covers this in full.

     **History: three adjudication rules were tried and all removed, in favor of the above.**
     First landing (see "Curation data generation" above for the shared reasoning this drew from):
     a forced mate or tablebase-proven win/loss reported by search ended the game immediately,
     without playing it out, on the reasoning that both were provably exact. A real correctness bug
     surfaced and got fixed along the way (2026-09-22): the triggering move was being dropped
     entirely rather than played and recorded, losing a real move -- and its evaluation -- from the
     game record; fixed to play and record it honestly instead (the `mate_in`/`score_cp` split
     described above is what came out of that fix, and survived the later full removal since it's
     just honest reporting, not an adjudication). Once actually reviewed against how search
     pruning works, the whole rule turned out not to be sound in the first place: the terminal
     detection itself (no legal moves while in check) is exact everywhere it's checked, including
     inside quiescence's own check-evasion path (a full, unpruned enumeration there, not just
     captures), but the *path* leading to it depends on heuristic pruning that isn't all gated
     against mate/TB scores -- late-move reduction in particular can reduce away an opponent
     defense without a full-depth re-search ever finding it, so a reported `#N` only ever meant "no
     defense found," not "provably none exists," most plausible to actually bite at self-play's
     shallow, fixed `DEFAULT_SEARCH_DEPTH = 9`. A Syzygy probe at the arbiter level (independent of
     the search's own internal probing, checking the *actual current* position directly) adjudicated
     the exact WDL result the moment a position became tablebase-reachable -- sound on its own
     terms, but still an adjudication, not play. And a +-1000cp score, agreed on by two consecutive
     plies (one played by each color), adjudicated a decisive result the same way cutechess-cli
     does. Considering what TCEC itself actually does settled it: TCEC never early-stops on a
     single search's own mate claim (it plays a real mate out to genuine checkmate on the board)
     and only adjudicates tablebase results from a direct probe of the *current* position, exactly
     matching the arbiter-level check above -- but even that, on reflection, is still an
     adjudication rather than playing the actual game to its real conclusion, and the developer's
     final call was to drop all three rather than keep any one of them, however individually
     sound. A fourth rule -- **fishtest's own draw rule** -- was also tried and removed earlier,
     independently of the above: a draw once the score has stayed within 20cp of zero for 8
     consecutive full moves (16 plies), never before move 34, fishtest's own rule (Stockfish's own
     testing framework), confirmed directly against the source discussion, not recalled
     (`github.com/official-stockfish/Stockfish` issue #1904: *"if for 8 moves the score is within
     -20,20 centipawns range... the game is a draw"*), used exactly as published: no
     reset-on-capture-or-pawn-move condition (that's TCEC's own rule, not fishtest's -- confirmed
     the same way), no material gate either -- removed once the developer flagged it as broken for
     `scaled_pure_material_value`: the rule assumes an eval that keeps fluctuating from positional
     factors even without a capture, which a material-only eval never does -- it only moves when
     material itself changes, so the quiet streak accumulates trivially through any capture-free
     stretch (extremely common in the opening/early middlegame) and the rule would fire on almost
     every materially-even game the instant it reached move 34, regardless of how positionally
     decisive the real position was.
     Calls `pgn::format_game` with `include_depth_and_time =
     false` (bare `{<score>}` comments -- depth is fixed and there's no real clock in self-play,
     unlike the fitness-selection match runner's own call, which keeps `true`), and `book.rs`'s
     new `RandomBook` (added alongside the existing `opening_fen`, which stays as the
     fitness-selection match runner's own low-volume lookup) -- self-play draws one opening per
     *game*, millions of times over, so re-scanning the book from the start on every call (what
     `opening_fen` does) would cost O(index) *per game*. Games are independent of each other, so
     unlike a real match's move order there's no reason to draw openings in file order either;
     `RandomBook` instead scans the file once at startup to record every position's byte offset
     (a `Vec<u64>`, about 21MB for the book's 2.6M positions -- not the full ~180MB of text), then
     each pick draws a uniformly random index and `seek`s straight to it. Each worker thread opens
     its own `RandomBookReader` (one file handle) once at startup and reuses it for every game it
     plays, so concurrent workers never contend on a shared handle or seek position -- no lock, no
     shared cursor at all, since picks don't depend on each other.
   - `src/bin/rotor_self/main.rs` + `src/bin/rotor_self/self_play.rs`: self-play's own binary,
     `rotor-self` (`--stem <name> --threads <count>`, both mandatory -- `--threads` has no default,
     the same reasoning `curate.rs`'s own identical flag already has: genuinely a property of the
     machine running it, no universal right answer, so a silent default risks a days-long run
     quietly using a fraction of the machine it was meant to), not a subcommand of `rotor-evolve` --
     `book.rs` and `pgn.rs` stay canonically under `src/bin/rotor_evolve/`, `#[path]`-included into
     `rotor_self/main.rs` too (`pgn_replay.rs`'s own mechanism: one canonical source compiled
     twice), since the fitness-selection match runner still needs both. A small shared
     `src/bin/ctrlc_handler.rs`, `#[path]`-included into both binaries' crate roots, replaces what
     was originally one `install_ctrlc_handler` function private to `rotor-evolve`'s own
     `main.rs`.
   - `bin/rotor-self` (new, alongside the existing `bin/rotor-evolve` -- no `.sh`, matching that
     script's own naming convention, unlike the standalone utility scripts
     `syzygy.sh`/`lichess-book.sh`). As originally landed: the same check-and-error convention
     every wrapper in this repo used at the time, erroring with the exact build command if
     `target/release/rotor-self` was missing rather than auto-building on every invocation. Since
     superseded twice: every wrapper (including this one) now auto-builds before exec'ing instead
     of erroring, and the build command itself dropped `--features material-only-eval` entirely
     (`Cargo.toml`'s own comment on the `rotor-self` `[[bin]]` entry covers the 2026-09-22 switch
     to the real hand-crafted eval) -- current form is just `cargo build --release --bin
     rotor-self`.

## Open items (not yet decided)

- **Rotor itself needs a runtime way to select which trained network it plays with**, before it
  can be added to the engine registry. Today's `uci.rs` only exposes `Hash`/`Threads`/
  `SyzygyPath`/`MultiPV` -- no `EvalFile`-equivalent option -- and the network is baked into the
  binary at build time. Without this, comparing checkpoints (e.g. candidate vs. baseline) would
  need a separate compiled binary per checkpoint, which doesn't fit the registry's "one binary,
  generic option key/value pairs" design. Deliberately deferred: not needed until curation/
  training for a real network (amanda) is done, so there's nothing to select between yet.
  `data/engines.yaml` (started, Stockfish-only for now) intentionally leaves Rotor out until
  this exists.
- Exact engine registry schema (single file vs. one file per engine) -- resolved differently
  than first proposed: lives at `data/engines.yaml` (one shared file), not `configs/engines/`.
  Local/machine-specific (binary paths vary per checkout), so it belongs in gitignored `data/`
  rather than checked-in `configs/`, consistent with this project's existing configs-vs-data
  split.
- ~~Bracket bookkeeping details for non-power-of-two entrant counts.~~ Resolved -- see
  "Tournament: single-elimination knockout" above (the recursive seeding construction).
- ~~`rotor-evolve` CLI surface.~~ Resolved: one entry point, `rotor-evolve <tournament-id>` --
  a bare id (e.g. `2026-09-22`), not a path, resolved against `data/tournaments/<id>/config.yaml`
  via `config::config_path`/`config::tournament_dir`, the same "bare name in, real path out"
  convention `preprocessing_config::net_config_path` already established -- dispatching on the
  config's own `format` field (only `single-elimination` exists as a value for now, self-play
  deferred), plus a separate `rotor-evolve analyze <id>` planned for phase 2.
- ~~New `[[bin]]` entry in `Cargo.toml`.~~ Done: `rotor-evolve`, `src/bin/rotor_evolve/main.rs`
  (a subdirectory, matching `rotor_train`'s own layout, since the binary is split across several
  files -- `bracket.rs`/`clock.rs`/`config.rs`/`engines.rs`/`pairing.rs`/`pgn.rs`/`results_db.rs`/
  `uci_client.rs`).
- **Opening book.** A fixed startpos every game means two deterministic engines (no search
  randomization, e.g. a fixed-depth Rotor search) just replay the same two games (one per color)
  `best_of` times over -- no statistical signal beyond that. Resolved which book to use, not yet
  how to use it: `UHO_Lichess_4852_v1.epd.zip` (2.6M EPD positions, real Lichess games filtered
  to a 48-52% draw rate -- i.e. empirically balanced, not hand-picked; the same book the
  Stockfish project itself currently uses for its own engine-strength testing), from
  `official-stockfish/books` (CC0-1.0). The book data itself isn't committed --
  `bin/lichess-book.sh` downloads and unzips it into `books/` on demand, the same
  fetched-not-tracked convention `bin/syzygy.sh` already established for the Syzygy tables
  (`.gitignore` excludes everything under `books/` except one tracked exception,
  `books/README.md`, which carries the citation so it survives a fresh checkout even though the
  book itself doesn't). ~~Still open: how many/which positions...~~ Resolved: `book.rs`'s
  `opening_fen(path, index)` reads line `index` (0-based) straight out of the book file, no
  loading it wholesale; `play_pairing` picks `index = games_played / 2`, so both games of a pair
  (one per color) share the same opening, and different pairs within a match progress
  sequentially through the book. `play_one_game`/`UciClient::set_position` now take that FEN
  directly (`position fen ... moves ...`, never the `startpos` shorthand), and `pgn.rs`'s
  `format_game` writes `[FEN]`/`[SetUp]` tags and starts SAN generation from that FEN instead of
  `Chess::default()`, correctly emitting the `5... ` ellipsis convention when a book position
  starts with Black to move.
- `config.yaml`'s exact field set beyond what's now settled (`name`, `format`, `participants`,
  `match.base_time_control`, `match.tiebreak_time_control`, `match.best_of`) -- e.g. whether
  `tiebreak_time_control` is required or falls back to `base_time_control` if omitted. Low-risk,
  deferred to implementation; `data/tournaments/2026-09-22/config.yaml` is a real, working
  instance of the schema either way.
- PGN header tag conventions (`Event`/`Site`/`Date`/`Round`/`White`/`Black`/`Result`) -- not
  explicitly specified, but standard/derivable, low-risk.

## Progress log

- 2026-09-22: Design discussed at length with the developer (curation is currently mid-run,
  ~5 days of the developer's own time / ~2 days compute, so this was designed in parallel
  rather than blocking on it). All decisions above are final calls from that discussion, not
  yet implemented.
