# Rotor Tablebase Support (WDL-only Syzygy)

**Done:** WDL-only Syzygy probing (3 to 6 pieces) is implemented in `tablebase.rs`. DTZ and probing after quiet moves are tracked in TODO.md. Mentions of `rotor-curate` below are stale.

Resume with: "resume from rotor-tablebase-implementation."

## Status

Implementation plan for adding Syzygy tablebase support to the play-time engine (the `rotor`
binary). Scope is deliberately narrow: **WDL only, both the 3-4-5-piece and 6-piece sets, no
DTZ** -- `syzygy.sh` downloads only `.rtbw` files (`download_tables "3-4-5-wdl" ...`/
`download_tables "6-wdl" ...`, filtering on `href="[^"]+\.rtbw"`), never `.rtbz` (DTZ), so there is
no distance-to-conversion information anywhere in this pipeline to build on, now or later without
downloading a different table set entirely. Nothing here touches `rotor-curate`/`rotor-train`:
tablebases are a search-time concern, and neither of those tools runs a search.

## Grounding: how this fits the existing codebase

- **CLAUDE.md rule 1 forces a hand-written prober, not a dependency.** `pgn_replay.rs`'s own module
  doc comment already establishes the exact line this project draws: `shakmaty`/`pgn-reader` "must
  never link into the shipped `rotor` engine binary (CLAUDE.md rule 1)," confined entirely to the
  `rotor-curate`/`rotor-train` tooling binaries via `#[path]` inclusion, never the `rotor` library
  crate the real engine binary links against. A Syzygy WDL prober is exactly the kind of thing rule
  1 is about -- there is no such thing as a non-chess-specific tablebase-format crate, so this has
  to be a hand-written module in `rotor` itself, informed by reverse-engineering existing tools
  (rule 1's own explicit allowance), never a `Cargo.toml` dependency.
- **Rule 2 covers the research, not the code.** The Syzygy WDL format itself was collaboratively
  reverse-engineered by the community over years and is not formally specified anywhere. Two real,
  open references to study (never copy from): Stockfish's own `tbprobe.cpp`/`tbprobe.h`, and
  `shakmaty-syzygy` (confirmed via docs.rs, not recalled -- niklasf's own pure-Rust WDL/DTZ prober,
  up to 7 pieces, the same author as the `shakmaty` crate already used in `pgn_replay.rs`). Neither
  can ever be a dependency of the shipped `rotor` binary (rule 1 -- `shakmaty-syzygy` itself depends
  on `shakmaty`, so it's a chess crate by the same definition), but `shakmaty-syzygy` being pure Rust
  already makes it a genuinely more direct reference for the actual format-parsing pieces (steps 1-4
  below) than Stockfish's C++, which rule 1's own "reverse engineering... allowed, and necessary"
  exists precisely to cover. Facts about the format are fair game; no code gets copied from either.
- **The score-band problem was already anticipated in this codebase.** `search.rs`'s `is_loss`/
  `is_win` doc comments say outright that Rotor "collapsed to one comparison since Rotor has no
  tablebase layer to distinguish 'search proved a mate' from 'a tablebase says this is lost' the
  way Stockfish's own three-constant version does." This plan is what closes that gap.
- **Real Stockfish constants** (fetched from `types.h`, not recalled):
  ```
  MAX_PLY                 = 246
  VALUE_MATE              = 32000
  VALUE_MATE_IN_MAX_PLY   = VALUE_MATE - MAX_PLY           = 31754
  VALUE_TB                = VALUE_MATE_IN_MAX_PLY - 1      = 31753
  VALUE_TB_WIN_IN_MAX_PLY = VALUE_TB - MAX_PLY             = 31507
  ```
  `VALUE_MATE = 32000` is exactly Rotor's own `MATE_SCORE` (`search.rs`), confirming that constant
  was already borrowed from Stockfish. Modern Stockfish has no separate `VALUE_TB_WIN` -- `VALUE_TB`
  is the flat ceiling a fresh TB-win report starts from, faded down by ply from there exactly like
  `VALUE_MATE` is for real mates.
- **Existing score infrastructure to extend, not replace** (`search.rs`): `MATE_SCORE = 32_000`,
  `MAX_PLY = 100` (deliberately different from Stockfish's own `246`, for unrelated reasons already
  documented on that constant), `is_win`/`is_loss` (checked against `MATE_SCORE - MAX_PLY`),
  `mate_in(ply)`/`mated_in(ply)` (`MATE_SCORE - ply` and its negation). The new TB band mirrors this
  exact shape, translated onto Rotor's own constants (see "Settled" below).
- **`Board` already exposes everything a probe key needs**: `bitboard(Piece) -> Bitboard`,
  `occupancy(Color) -> Bitboard`, `piece_at(Square) -> Option<Piece>`, `side_to_move() -> Color`,
  `en_passant_square() -> Option<Square>` (`src/board/mod.rs`). No new board representation needed --
  a probe key is built directly from these.
- **UCI option precedent** (`uci.rs`): one dedicated `UciCommand` variant per option (`SetOptionHash`,
  `SetOptionThreads`), each parsed in `parse_setoption` by matching an exact token shape
  (`["name", "Hash", "value", mb]`). A `SyzygyPath` option needs a different shape than either of
  those -- see open point C.

## Settled (from discussion with the developer)

- **WDL only, forever, for this pass.** No DTZ support is being built now, and none of the design
  below assumes DTZ could be added later without real additional work -- the score scheme
  specifically avoids needing a distance value anywhere.
- **Both the 3-4-5-piece and 6-piece sets, not 6-only.** Considered and rejected: probing only the
  6-piece set would lose *all* tablebase coverage the instant a single capture drops the position to
  5 pieces, since there would be no smaller table to fall back to -- exactly when TB guidance matters
  most (converting toward the actual result). 3-5-piece endgames are also both more common in
  practice (material only ever decreases across a game) and often harder for the heuristic eval to
  judge correctly (KBNvK's checkmate technique, close pawn races) than 6-piece configurations. The
  file-lookup mechanism has to key off material signature regardless of how many piece-counts are
  supported, so there is no real implementation-cost argument for excluding 3-4-5, and those tables
  are far smaller on disk than the 6-piece set besides.
- **Score band.** New constants in `search.rs`, mirroring Stockfish's real, fetched shape:
  ```rust
  const TB_WIN_SCORE: i32 = (MATE_SCORE - MAX_PLY as i32) - 1; // 31_899
  ```
  TB win band: `[TB_WIN_SCORE - MAX_PLY as i32, TB_WIN_SCORE]` = `[31_799, 31_899]` -- entirely below
  the real mate band (`[31_900, 32_000]`, from `is_win`/`is_loss`'s own existing threshold), with no
  overlap in either direction even after `MAX_PLY` worth of ply-adjustment as a score bubbles up the
  tree.
- **Fade by search ply, not by any tablebase-reported distance** (there is none, WDL-only). A TB win
  found at search ply `p` reports `TB_WIN_SCORE - p`, mirroring `mate_in(ply) = MATE_SCORE - ply`
  exactly -- `p` here is purely "how deep into this recursion," the same quantity `mate_in`/`mated_in`
  already use, never anything looked up from the table itself. This makes the search prefer forcing
  entry into a known-won tablebase position sooner rather than shuffling once it's found, without
  needing or pretending to have real distance-to-mate information.
- **TB draw reports `0`**, the same score as any other known draw -- no separate band needed.
- **TB loss is the exact negation**: `-(TB_WIN_SCORE - p)`, mirroring `mated_in`.

## Plan

1. **Syzygy WDL format research and a hand-written parser.** Study the on-disk WDL layout (per-
   material-signature file naming, e.g. `KQvK.rtbw`; the compressed/indexed block structure) from
   Stockfish's own `tbprobe.cpp`/`tbprobe.h` and whatever else clarifies unclear points, fetched
   directly rather than recalled (rule 2). New module (likely `src/tablebase/`, given file-format
   decoding, indexing, and probe-key construction are separable concerns worth their own files
   rather than one large one) -- never touched by `rotor-curate`/`rotor-train`.
2. **Probe-key construction from `Board`.** A function building whatever normalized position
   representation the WDL index scheme needs (piece placement per type/color, side to move, en
   passant file) directly from `Board::bitboard`/`occupancy`/`piece_at`/`side_to_move`/
   `en_passant_square` -- no new board representation.
3. **Table file loading and material-signature lookup.** Given a directory (from the new
   `SyzygyPath` UCI option, step 6) and a queried material signature, find and load/cache the
   matching `.rtbw` file, across both the 3-4-5 and 6-piece sets.
4. **The probe function itself.** `probe_wdl(&Board) -> Option<Wdl>` (`Wdl` a small `Win`/`Draw`/
   `Loss` enum, from the side-to-move's own perspective), `None` whenever the position's total piece
   count exceeds what's loaded or no matching table exists for its material signature.
5. **Score integration into search**, in two distinct stages rather than one:
   - **Root probing first.** Probe the actual root position once, before the iterative-deepening
     loop starts. On a hit, restrict/reorder the root move list so a known win is never turned into a
     draw or loss and a known draw is never turned into a loss. WDL-only means this can only
     *preserve* classification, not pick the fastest-converting move among several win-preserving
     ones (that needs DTZ) -- a real, accepted limitation of this scope, not an oversight.
   - **In-tree probing as a separate, later step.** Probe at interior nodes too (positions reached
     via search, not just the literal root), feeding the ply-faded `TB_WIN_SCORE`/`0`/
     `-TB_WIN_SCORE` into the same alpha-beta cutoff logic real mate scores already use
     (`is_tb_win`/`is_tb_loss`, mirroring `is_win`/`is_loss`). Deferred separately because it touches
     far more of the hot path and needs its own scrutiny against the transposition table, aspiration
     windows, and mate-distance pruning's existing bounds -- see open point A.
6. **`SyzygyPath` UCI option.** A new `UciCommand::SetOptionSyzygyPath(String)`, parsed in
   `parse_setoption` -- but not via the existing fixed-token-count match those two options share (see
   open point C). Real engines (Stockfish included) accept a *list* of directories in one
   `SyzygyPath` value, delimited by the OS path-list separator (`:` on Linux/macOS, `;` on Windows)
   -- this is how "use both the 3-4-5 and 6-piece sets" gets expressed over UCI at all, since
   `syzygy.sh` downloads them into two separate directories (`syzygy-3-4-5-wdl`/`syzygy-6-wdl`)
   rather than one. Step 3's file lookup tries each directory in the list in turn for a given
   material signature. Wired into wherever the engine's running state lives in `uci.rs`, alongside
   `Hash`'s own `TranspositionTable`.
7. **Testing** (CLAUDE.md rule 4: assertions, not eyeballing). Golden-value unit tests against known
   small endgames with textbook-certain results (e.g. specific KPvK positions), plus a statistical
   cross-check of probed results against an independent trusted source where practical -- the same
   "never assumed to agree, checked" discipline rule 7 already applies to the learned eval.

## Open design points

- **A. Root-only vs. in-tree probing for v1 -- not settled.** Root-only is smaller and safer to land
  first (touches `uci.rs`'s `go_search` entry point, not the hot recursive path at all); in-tree
  probing is where most of the real strength gain lives but is riskier to get right the first time.
  Leaning toward root-only as an explicit first milestone, in-tree as an explicitly separate follow-
  up, but this hasn't been discussed with the developer yet.
- **B. How aggressively to probe in-tree** (every node under some piece-count threshold vs. only
  near leaves) is an efficiency question with no measurement behind it yet -- CLAUDE.md rule 5
  applies once this is actually being built, not before.
- **C. `SyzygyPath` value parsing.** Two real changes to `parse_setoption`'s existing convention at
  once: the value is a *list* of directories (colon- or semicolon-delimited, OS-dependent -- how
  "both tables" gets expressed over UCI at all, per the developer's own question), and any individual
  path can contain spaces, which the existing fixed-token-shape matching (`["name", "Hash", "value",
  mb]`) can't handle as-is. This is the first option needing "join everything after `value`, then
  split on the path-list separator" rather than one exact token-count match.
- **D. Table file caching/memory strategy** (load-everything-up-front vs. lazy per-material-signature
  loading) -- 6-piece WDL sets are large; not measured yet.
- **E. Companion book chapter.** Per `companion/`'s own convention, a chapter covers a technique once
  it's actually done, not while still being designed -- nothing to write yet.

## Settled (round two): in-tree only, no root probing

Discussed directly with the developer once the root-filtering soundness problem above surfaced:
tablebases should not be queried at the root node at all. Root filtering would require probing
*hypothetical* children of the root to decide which candidate moves to keep, which (per the
finding above) only works for the subset of moves that are themselves zeroing -- guessing about
the rest is unsound. In-tree probing has no such problem: every node the search actually reaches
was reached by a real move, so `should_probe`'s halfmove-clock-zero check is a genuine fact about
that exact position, not a guess about an unvisited alternative. Open point A is settled by this:
in-tree only, no separate root-probing milestone.

Also settled: Stockfish's own real reporting convention (fetched from `search.cpp`, not recalled)
confirms the score-band design already in `search.rs` needs no special UCI formatting. A
tablebase hit's value flows through the same `score cp`/`score mate` formatting as any other
node's score, tagged specially only via a separate `tbhits` diagnostic counter in the `info`
line -- not a score annotation. `TB_WIN_SCORE` (31,899) already sits strictly below `format_
score`'s existing mate threshold (`MATE_SCORE - MAX_PLY` = 31,900), so a TB-band score
automatically prints as plain `cp <x>`, matching Stockfish's own behavior with no changes needed
in `uci.rs`. A `tbhits` counter in `format_info_line` is a real, matching nice-to-have, not yet
added -- worth doing once in-tree probing actually runs enough to count.

## Progress (round two): in-tree probing, step by step

Threading a real tablebase into the hot recursive search path is happening in small, separately
verified slices (CLAUDE.md rule 3), each confirmed by a real `cargo test` run before the next:

- **Slice 1 -- the dormant field and the probe hook itself, done and confirmed (511 lib/rotor/
  rotor-curate tests + 160 rotor-train tests passing).** `Search` gained `tablebase: Option<&'a
  SyzygyTablebase>`. `new_search` (and the handful of test call sites that build a `Search` via a
  bare struct literal instead, for fixture control the constructor doesn't give them) all default
  it to `None` -- zero behavior change for any of the dozens of existing tests, since nothing yet
  ever sets it to `Some`. The actual probe sits in `alpha_beta_with_buffer`, right after the
  existing `is_repetition`/`is_insufficient_material` checks (same "genuine, definitive fact about
  this exact position" class, same `ply > 0` guard against a root-with-no-move bug), returning
  `tb_win_at(ply)`/`0`/`tb_loss_at(ply)` on a hit. One new test (`a_tablebase_with_no_loaded_
  tables_leaves_alpha_beta_with_buffer_unaffected`) confirms `Some(&tablebase)` with nothing
  loaded behaves identically to `None` -- the only thing checkable without real `.rtbw` files in
  this environment (same limitation `tablebase.rs`'s own tests already have).
- **Slice 2 -- threading a real `&SyzygyTablebase` through, done, not yet confirmed by a build.**
  `search_root`/`choose_best_move`/`choose_best_move_with_progress` all gained a `tablebase:
  &SyzygyTablebase` parameter (not `Option` at this layer -- unlike `Search::tablebase` itself,
  `Engine` always holds a real, possibly-empty one, so there's no `None` case to thread through
  here). `search_root` sets `Search::tablebase` to `Some(tablebase)` directly on the `Search`
  `new_search` hands back. `Engine::tablebase` became `Arc<SyzygyTablebase>` (mirroring `tt`
  exactly, same reasoning -- `go_search`'s background thread needs its own handle), cloned into
  that thread alongside `tt`/`board`/`ancestors`, then re-borrowed the same way inside it before
  being passed to both the thread-0 and every-other-thread call. `rotor-curate`'s own
  `play_game` (a second, independent production call site of `choose_best_move` -- self-play data
  generation, not the play-time engine) got a local, always-empty `SyzygyTablebase::new()`: it has
  no UCI session and no `SyzygyPath` option to load one through, so every probe there is a no-op
  by construction, not a special case. Every test call site of all three functions (about a dozen)
  updated to pass `&SyzygyTablebase::new()`.
- **Slice 3 -- the same probe hook in `quiescence`, done, not yet confirmed by a build.** Placed
  right after `is_insufficient_material`, before the fifty-move-draw check, mirroring `alpha_beta_
  with_buffer`'s placement -- but with no `ply > 0` guard, since `quiescence` is never the literal
  search root (only ever entered from inside `alpha_beta_with_buffer`, already at `ply >= 1`), so
  there's no equivalent bestmove-0000 risk to guard against. Arguably the more valuable of the two
  hooks in practice, not just for consistency: quiescence chases forced capture sequences, exactly
  the kind of position most likely to satisfy `should_probe`'s zeroing-move precondition and drop
  to a low piece count. One new test (`a_tablebase_with_no_loaded_tables_leaves_quiescence_
  unaffected`), same dormancy-only shape as slice 1's.
- **Slice 4 -- widening every mate-scale pruning guard to also cover TB-scale, done, not yet
  confirmed by a build.** A real, concrete finding, not a hypothetical: `is_tb_win`/`is_tb_loss`
  (from the score-band work) were defined but never called anywhere. Several pruning techniques
  already guard themselves against a mate-scale `alpha`/`beta`/`best_score` (comparing an
  ordinary evaluation against a mate-huge bound is meaningless and would wrongly prune a move
  that's actually needed) -- but every one of those guards only checked `is_win`/`is_loss`, and
  `TB_WIN_SCORE` sits deliberately *below* that threshold (so it prints as `cp`, not `mate`), so
  a TB-scale bound sailed straight past every one of them unprotected. Widened all six real
  sites: `is_null_move_pruning_applicable`'s `!is_win(beta)`, late-move pruning's `!is_loss(
  best_score)`, razoring's `!is_win(alpha)`, the null-move score clamp's `is_win(null_score)`,
  quiescence's move-count cap's `!is_loss(best_score)`, and classical futility pruning's own
  (see below). New TB-scale regression tests mirror the four that could be driven directly (two
  pure-function tests, plus recursion-level analogs of the razoring/futility tests); the null-
  move clamp and quiescence move-count cap widenings don't have new recursion-level tests of
  their own -- reproducing a genuine TB-scale value there needs a real probed hit inside the
  tree, which needs real `.rtbw` files this environment doesn't have.
- **A separate, pre-existing bug found and fixed along the way, not caused by tablebases:**
  classical futility pruning (the quiet-move skip inside the main move loop, `static_eval +
  FUTILITY_BASE_MARGIN * remaining <= alpha`) had *no* `is_win(alpha)` guard at all -- unlike
  razoring's own identical margin check right above it. Worse than merely unprotected: razoring's
  own comparison direction (`e < alpha - margin`) makes a huge `alpha` *less* likely to fire, so
  it was already self-protected in practice even before its explicit guard was added; futility's
  direction (`... <= alpha`) makes a huge `alpha` fire *more* easily, so this was a real, live gap
  for any genuinely mate-scale `alpha`, independent of tablebases entirely. Fixed with a matching
  `!is_win(alpha)` guard (immediately widened to also cover `!is_tb_win(alpha)` in the same slice
  as everything else above), plus two new regression tests (mate-scale and TB-scale, mirroring
  razoring's own pair).
- **Slice 5 -- caching a TB hit in the TT, done, not yet confirmed by a build, and genuinely
  untestable past one small piece in this environment.** Both `alpha_beta_with_buffer`'s and
  `quiescence`'s own TB hooks now call `self.tt.store(...)` right before returning, with
  `Bound::Exact` (already true, not a choice -- Rotor's `TbResult` already collapsed Stockfish's
  own cursed-value ambiguity away in `wdl_to_result`, so there is no Lower/Upper bound left to
  represent the way Stockfish's five-way `Wdl` still needs), depth `MAX_PLY` (not this node's own
  `remaining`, nor quiescence's usual sentinel `0` -- a resolved WDL fact never becomes stale as
  remaining depth changes, so it's stored as trustworthy at *any* future remaining depth, the
  highest depth `store`'s own replacement policy and every probe's own cutoff check already know
  how to treat as authoritative), and a new `tablebase_tt_move` placeholder (`A1`-to-`A1`, an
  `Entry::best_move` requires a real `Move` but a WDL verdict is a fact about the position, not a
  move recommendation -- mirrors Stockfish's own real `Move::none()` for the identical reason;
  `from == to` can never be a real generated move, so it's guaranteed to be silently ignored by
  the TT-move priority stage rather than "tried," the same already-established fate a stale TT
  move from an unrelated context already has). One new test (`tablebase_tt_move_can_never_match_
  a_real_generated_move`) covers the one piece actually testable without real table files -- the
  `store` call itself is genuinely unreachable in any test here, since it only runs after a real
  `Some(TbResult)` from `probe`, which needs real `.rtbw` files this environment doesn't have.
  This is the honest limit of what can be verified before the developer smoke-tests a real probe.
- **Not yet done:** a `tbhits`-style UCI counter (see "Settled, round two" above).

## Revised: depends on `shakmaty-syzygy` instead of a hand-written parser

CLAUDE.md rule 1 gained an explicit, narrow exception (see the rule itself): hand-writing a Syzygy
parser is a binary-format/compression exercise, not a chess-engine-internals one, so the shipped
`rotor` binary now depends on `shakmaty-syzygy` (niklasf, pure Rust, confirmed via docs.rs) for
this one subsystem. This collapses steps 1-4 of the original plan (reverse-engineer and hand-write
a WDL parser) into: add the dependency, then build a thin adapter between `Board` and whatever
position type it expects.

**A real correctness point found doing this, not anticipated in the original discussion:**
`Wdl` isn't three values, it's five -- `Loss`/`BlessedLoss`/`Draw`/`CursedWin`/`Win` (confirmed via
docs.rs, not recalled). The two conditional values are properties of the position and material
alone, not of how much of the halfmove clock has already ticked by: `CursedWin` means "a genuine
win in unlimited-move chess, but the 50-move rule forces a draw before conversion even from a
freshly reset clock." Under real tournament rules, that can never actually be forced, so it
collapses to `Draw` (`tablebase.rs::wdl_to_result`), not `Win` -- reporting it as a real win would
claim a result the rules don't deliver. `BlessedLoss` mirrors this into `Draw` as well.

Also found: `shakmaty_syzygy::Tablebase::probe_wdl_after_zeroing` (the only method usable without
DTZ tables) is documented as correct only "for a position reached directly after a capture or pawn
move" -- i.e. `board.halfmove_clock() == 0`. Without DTZ tables there's no way to handle the
general, clock-aware case (that needs `probe_wdl`, which requires DTZ too), so `tablebase.rs`'s
`should_probe` simply declines to probe any position reached by a quiet move, rather than risk a
stale classification. A real, accepted scope limit of going WDL-only, not a bug.

## Progress

- **Score band** (`search.rs`): `TB_WIN_SCORE`, `tb_win_at`/`tb_loss_at` (fade by search ply),
  `is_tb_win`/`is_tb_loss` -- done, mirroring `MATE_SCORE`/`mate_in`/`mated_in`/`is_win`/`is_loss`'s
  existing shape exactly.
- **`tablebase.rs`** (new module, `pub mod tablebase;` in `lib.rs`): `TbResult` (`Win`/`Draw`/
  `Loss`), `wdl_to_result`, `should_probe`, `SyzygyTablebase` (`new`/`add_directory`/`probe`, the
  last built on a `Board::to_fen` -> `shakmaty::fen::Fen` -> `shakmaty::Chess` round trip) -- done,
  tested (the `Wdl` mapping, the piece-count and halfmove-clock gates, and a no-tables-loaded probe
  returning `None`). Not tested against real table files -- those are downloaded locally by the
  developer (`syzygy.sh`), never checked into the repository, so there's nothing for a probe
  against real data to run against in this environment; only pure logic is unit-tested here.
- **`Cargo.toml`**: `shakmaty-syzygy = "0.28.1"` added, unconditional (a play-time engine feature,
  not gated behind `training` the way `tch`/`zstd`/`serde` are).
- **`SyzygyPath` UCI option and `Engine` wiring -- done.** `UciCommand::SetOptionSyzygyPath(Vec<
  String>)`, parsed by `parse_syzygy_path_value` (joins everything after `value`, then
  `std::env::split_paths` -- the OS-correct list splitter, the same one `PATH` itself uses, rather
  than a hand-rolled one). `Engine` gained a plain `tablebase: SyzygyTablebase` field (no `Arc`
  needed: unlike `tt`, nothing inside `go_search`'s background thread touches it yet -- see below),
  rebuilt from scratch on every `SetOptionSyzygyPath` (mirrors `SetOptionHash`'s own "replace, don't
  accumulate" convention). `uci`'s advertised option list gained `option name SyzygyPath type
  string default <empty>`. A directory `add_directory` can't open is silently skipped (`Result`
  ignored) -- stdout is UCI-only (rule 8), so there's nowhere to report it either way.
- **Root-move filtering (original plan step 5, first bullet) -- blocked on a real soundness
  problem, not yet attempted.** Read `go_search` in full this pass (it spawns a background thread;
  nothing about that blocks this feature, since a root probe can run synchronously *before* that
  thread is spawned -- no `Arc` needed). The blocker is different and more fundamental: WDL-only,
  without DTZ, `shakmaty_syzygy::Tablebase::probe_wdl_after_zeroing` is only valid on a position
  reached directly by a capture or pawn move (`should_probe`'s own halfmove-clock-zero check).
  "Restrict/reorder the root move list" means probing the position *after* each candidate root
  move -- but that child position is only probeable this same way when the *move itself* is a
  capture or pawn push (the only thing that re-zeroes the clock one ply later). For an ordinary
  quiet root move, the resulting child can't be probed at all under this scheme, so there is no
  sound way to tell whether that move preserves the root's own classification or not -- excluding
  it from the root move list on no evidence would be wrong exactly as often as it's right. Real
  engines (Stockfish included) solve this at the root by combining WDL *and* DTZ (`Tablebases::
  probe_wdl` proper, not the after-zeroing variant), which is exactly the case rule "no DTZ tables"
  was scoped to avoid needing. This narrows what step 5's first bullet can actually mean under a
  WDL-only, no-DTZ design -- needs a decision from the developer on how (or whether) to scope a
  root-time feature down to something sound before writing more code here. Candidate framings, none
  chosen yet: (i) verify-only -- after the search picks its own move, silently confirm/leave alone
  rather than override, since even "trust the tablebase over the search when they disagree" needs a
  known-good replacement move to fall back to, which usually isn't available either; (ii) restrict
  only among root moves that are themselves captures or pawn pushes, when at least one such move
  exists and is provably at least as good as the root's own classification -- a real but narrow
  slice of positions; (iii) drop root-time filtering from v1 entirely and revisit only if DTZ tables
  are ever added. In-tree probing (step 5's second bullet) is unaffected by this specific finding
  either way and remains its own, separately deferred piece of work (open point A).
