# Stale PV row corruption

## Symptom

Manual UCI testing (`go movetime 3000` from the starting position, run to sanity-check the
newly landed multi-threaded search before considering that work done) surfaced illegal moves in
the reported PV from depth 6 onward. Examples actually observed:

- `e2e4 d7d5 e2e3 g8f6 d2d4 e7e5` -- White's third move, `e2e3`, is played from `e2`, but
  White's pawn already left that square for `e4` on move one.
- `b1c3 b8c6 g1f3 b8c6 e2e4` -- `b8c6` appears twice; Black's knight already left `b8` the
  first time.
- `d2d4 g8f6 e2e3 e7e5 d1e2 c6b4` -- `c6b4` moves a piece off `c6`, even though nothing in this
  line ever put anything there.

Reproduced identically with `Threads = 1` (no concurrency at all, `stop_signal` never
contended), which ruled out a race from the concurrent multi-threaded search work and confirmed
this as a pre-existing, purely single-threaded bug that real usage had simply never surfaced
before -- none of the 341 unit tests passing at the time exercise a PV this deep on a real,
branchy position, and a malformed PV isn't something a node-count or score assertion can ever
catch, only reading one by eye can.

## Root cause

`Search::pv` is one row per `ply`, shared across the whole tree -- not one row per node. The
convention is: a node writes its own row, `pv[ply] = [mv] ++ pv[ply + 1]`, only at the exact
moment its own `best_score` actually improves. Several early-return paths in
`alpha_beta_with_buffer` -- a TT cutoff (an entry deep enough and with a bound that resolves the
node outright), a repetition, insufficient material, a fifty-move draw -- return before ever
reaching that improvement write. Only the "no legal moves" terminal branch had ever been fixed to
explicitly clear its own row first; every other early-return path left behind whatever an
entirely unrelated, earlier node (a different branch of the tree, visited earlier, that happened
to share the same `ply`) had last written there.

A parent that then read that row -- because *this* early-returning child's own score happened to
improve the parent's own best score, entirely plausible for a TT-cutoff result -- spliced that
stale, unrelated continuation onto its own line, indistinguishable from a real one. That's exactly
the corruption observed: a genuine move followed by a continuation that belonged to a completely
different position.

## Fix

`alpha_beta_with_buffer` now clears `self.pv[ply]` unconditionally on entry, before any early
return, not just inside the terminal-node branch. Every early return now hands back an
already-empty row -- correctly meaning "no further main-search continuation is known from here,"
which is the truth for all of them -- rather than whatever an unrelated node last left. The
terminal branch's own explicit clear became redundant once the entry-point clear covers it, and
was removed in favor of a short comment pointing back to it.

## Tests

- `a_terminal_node_clears_its_own_stale_pv_row_rather_than_leaking_it_to_a_parent` (already
  existed for the terminal-node case; still passes, now validated by the entry-level clear
  instead of a branch-local one).
- `a_tt_cutoff_clears_its_own_stale_pv_row_rather_than_leaking_it_to_a_parent` (new): primes a
  `TranspositionTable` entry deep enough to force an immediate `Bound::Exact` cutoff at a chosen
  ply, seeds that ply's row with unrelated garbage beforehand, and asserts the row comes back
  empty rather than leaking the garbage.

Verified against two real manual UCI sessions (`go movetime 3000` from the starting position,
`Threads = 1` and `Threads = 4`): every reported PV at every depth is now a legal, self-consistent
move sequence, confirmed by hand move-by-move.

## Accepted tradeoff: PVs can be shorter than before

A real side effect of the fix, not a new bug: several PVs are now visibly shorter than they were
before it (a fabricated-but-illegal 6-move line at depth 6 became a genuine 2-move line in the
same test session, once the earlier moves' own continuation resolved via a TT cutoff instead of
inheriting stale data). Whenever a node along the reported line resolves via one of the
early-return paths above, the PV now correctly truncates there instead of continuing with
unrelated data.

This is a known, accepted limitation of PV extraction without dedicated PV-verification
machinery -- real engines that want a longer guaranteed-accurate PV do extra work for it (walking
the TT again after the search finishes, or never pruning along the principal variation at all, so
it's always fully searched). Rotor has neither yet. This fix only stops the search from lying
about what it found; it doesn't attempt to make the reported line longer than what's genuinely
known. Worth revisiting only if shorter-than-expected PVs turn out to matter in practice (e.g. for
a future self-play/strength-testing harness that wants to inspect deep lines), not before.
