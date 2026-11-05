# A network teacher's search cost: scaled margins and exact speed-ups, one duel

**Status:** done 2026-10-08. Step 1 (margins in the evaluator's pawn) passed its duel; the pawn
scale is now measured on held-out Lumbra positions (pawn-scale-positions.md, amanda 55%), and the
margin itself is tuned per network by play (margin-search-command.md: amanda-0001 at 125 cp per ply,
about +20 Elo over 200 at 10+0.1). Generation 2's replay restarted under both on 2026-10-08, with
tablebase labels (replay-tablebase-labels.md), at about hand-crafted's labelling rate.

## Why

The teacher's search cost sets each generation's labelling time: amanda's replay of the windows
takes about 48 h on the training machine alone. On the **same** games, searched as the replay searches them
(`nnue_bench --search-only --games`, 40 games of betty-0001's selection, 5,113 positions, depth 9,
one thread, the training machine), amanda takes **1.92x** hand-crafted's time: **1.35x the nodes** (73.0M against
54.1M) at **1.43x the cost per node** (3.19M nodes/s against 4.55M).

An earlier note (TODO.md, the same day) put the whole gap on the cost per node, from self-play
rates. That comparison searched different games for each player and was wrong; this one replaces it.

## What the measurements found (2026-10-07, the training machine)

**The node excess is mostly the pruning margins.** They are set in `piece_value`'s pawn, and
amanda's pawn near equality is about 0.6 of it (`nnue_bench --eval-scale`, 2026-10-03), so her
evaluations clear them less often. Scaling, on the same 40 games:

| amanda | nodes | against today |
|---|---|---|
| today | 72,971,675 | |
| margins x 0.6 | 61,763,056 | -15.4% |
| delta pruning x 0.6 | 71,661,731 | -1.8% |
| aspiration window x 0.6 | 73,486,312 | +0.7% |
| all three x 0.6 | 62,642,032 | -14.2% |
| all three x 0.5 | 59,495,844 | -18.5% |

The margins are the lever and delta pruning adds a little; a narrower aspiration window costs
re-searches and is left alone. About 10% excess remains at the best, probably the evaluation's own
noise, which the search cannot remove.

**The cost per node is mostly the evaluation, and little of it is waste.** `nnue_bench` on amanda:
an update 60.6 ns, the head 20.9 ns. The `node-stats` counters (49 positions): 0.88 updates and 0.75
evaluations per node; only 8.9% of updates are never read by an evaluation, and 10.7% of
evaluations are of a position the TT already holds. Lazy updates and the evaluation kept in the TT
together save a few percent. The rest of the 1.43x is inherent to the network, plus differences in
which nodes the two trees are made of. The training machine's efficiency cores also run 256-bit SIMD as two
halves, so a network scales worse across all cores (5.0x of one thread against hand-crafted's 6.0x
at 20 threads): hardware, not code.

## Design (decided 2026-10-07)

### 1. Margins in the evaluator's own pawn

- **The evaluator's pawn scale** `s`: 1 for hand-crafted, exactly, so its search does not change at
  all (node counts 3,140,380 on the 49 positions and 54,130,010 on the 40 games must not move).
  For a network, **measured when it loads** (decided: measured, not configured): no format
  change, and every network gets its own.
- **The measurement:** a fixed, deterministic set of positions, Stockfish's 49 bench positions and
  every position one legal move from each (about 1,700), not in check, with hand-crafted's
  evaluation `h` within +/-300 cp and not 0. `s` is the least-squares slope through the origin of
  the network's evaluation on `h`, over those positions: `sum(h * n) / sum(h * h)`. Near equality on
  purpose, since that is where the margins decide.
- **Rounded to 0.05** (decided), so that two networks of nearly the same scale search the same
  trees, and clamped to [0.25, 2.0] against a degenerate network.
- **Where it applies:** `MARGIN_PER_PLY` wherever it meets a static evaluation (razoring, reverse
  and classical futility, null move's eval term), and quiescence's delta pruning (the victim's
  value and the margin). Not SEE pruning (material against material) and not the aspiration
  window (measured worse).
- **Where it lives:** `Network` computes `s` in `load` (the bench positions move from `benches/` to
  a module of the library, shared); `Search` reads it once in `set_evaluator` and keeps the
  scaled margin as a field, so the hot path reads an integer, as today.
- Printed by `nnue_bench` beside the network's path. Not by the engine at load: it loads before
  the GUI's `uci`, when an `info string` line could confuse a GUI or lichess-bot.
- As built: the scale is an integer percent (`pawn_percent`, 100 for hand-crafted), so the margins
  stay integer arithmetic and are exactly today's at 100. The bench positions moved to the library
  (`src/bench_positions.rs`); `benches/stockfish_positions.rs` is to `git rm`.

### 2 and 3: dropped (2026-10-07, the developer)

The counters bound them too tightly to pay for their complexity. Lazy updates can save at most the
8.9% of updates never read, about 5 ns a node, and deferring an update means keeping at every ply
what to update from (a board copy, with today's `ChangedRows`), which costs about as much. The
evaluation in the TT would skip 10.7% of heads at about 21 ns, about 2 ns a node, and grows the
entry. Together 1 to 2%, exact but not worth it. Lazy updates become worth a new look only with the
rows taken from the move (4 below), which removes the board copy.

The original designs, for reference:

### 2. Lazy accumulator updates

An accumulator slot is marked stale when its ply is entered; `evaluate` walks back to the nearest
computed slot and updates forward from there. Exact: evaluations do not change.

### 3. The static evaluation in the TT

`tt::Entry` gains the static evaluation (an `i16`; `None` when the node was in check or never
evaluated). A node whose entry holds one reads it instead of evaluating, and with (2) then needs no
accumulator. Exact. Open: whether `Entry` has room without growing the table (to read in `tt.rs`).

### 4. Not now

`ChangedRows` from the move instead of comparing boards: small, left for a later exact change.

## Steps

1. The pawn scale (design 1), replacing the temporary experiment code. **Done 2026-10-07**, tests
   passing. Hand-crafted unchanged: 3,140,380 nodes on the 49 positions, 54,130,010 on the 40
   games. Amanda-0001 measures **70%** (kept, as defined; the 0.6 of `--eval-scale` is a median
   ratio, a different statistic): 66,124,531 nodes on the 40 games, -9.4% (73.0M before), 20.48 s.
2. Lazy updates and 3. the evaluation in the TT: dropped (above).
4. **One duel**: amanda with step 1 against amanda as built before it, at 10+0.1 on idle the Lichess machine,
   cutechess-cli with two builds, the usual rule. **Started 2026-10-07** on the Lichess machine: `scaled`
   (`4f557bc`) against `before` (its parent, the i8 commit, whose search is the old one), both
   amanda-0001, UHO_Lichess_4852_v1, 8 at once, `duel-scaled-margins.pgn`. The commit first failed
   to push (GitHub answered 500 for those two commit objects only; rebuilt by `git rebase
   --force-rebase --reset-author-date`, it went through). **Decided before the result (the
   developer, 2026-10-07): a draw at 1000 games passes**, since the change is for labelling speed
   and only has to show it is not weaker; only `scaled` shown weaker keeps today's margins.
   **Interrupted the same day**, at about 141 games: +39 -32 =70
   for `scaled`, 52.5%. To finish it, the same command with `-rounds` set to the pairs still
   missing (1000 games in all) and `-pgnout` to a second file; both files together are this one
   duel, judged by the same rule over the total. Completing an interrupted duel is not a rerun.
   The first file holds 173 games, not 141 (the score above was read before the switch-off).
   **Resumed 2026-10-08** on the Lichess machine: `scaled` copied to `~/rotor-scaled.exe` (the build of
   `e4d683f`, whose source is `4f557bc`'s), `before` is `~/rotor-before-scale.exe`, 414 pairs
   (828 games, 1001 in all) with `order=random`, into `duel-scaled-margins-2.pgn`. Part of the
   second file ran with the Lichess machine on battery (about half speed), then plugged in again: both sides
   slowed alike, so the comparison holds, at a time control partly nearer 5+0.05.
   **Result (2026-10-08): drawn, so passed.** First file 173 games, +46 -38 =89; second 828,
   +238 -203 =387 (cutechess-cli: +14.7 +/- 17.3 Elo, LOS 95.2%). Together 1001 games, +284 -241
   =476 for `scaled`, 52.1% (about +15 Elo), inside the duel rule's bounds at 1001 (45.1% to
   54.9%). As decided before the result, a draw passes: the scaled margins stay.
   **The load-time measurement is unstable (found 2026-10-08, `nnue_bench --pawn-scale`).** The
   same statistic over 91,336 positions of the 50,000-game Lumbra reference (held-out master games)
   against the bench positions and their children at load: amanda-0001 55% (slope 53.1) against
   70%; betty-0001 60% (59.3) against 25%, the clamp's floor; caroline-0001 55% (53.2) against
   150%. On Lumbra the three agree, and with `--eval-scale`'s median ratio of about 0.6; on the
   small set they scatter. Likely cause, unverified: most of the small set is one random legal move
   from a bench position, often leaving a piece hanging, which the networks see and hand-crafted
   does not. So the duel above, and every margins experiment of 2026-10-08, ran amanda at 140 cp
   per ply where her pawn suggests about 110; betty searches at 50 and caroline at 300.
5. If it passes: the replay restarts afresh under the new search, since its labels would differ
   from those written before (the developer accepted that), and the Lichess machine and another machine join on the same
   commit. The restart waited for step 0 of eval-error-margins.md (decided 2026-10-08), so that
   generation 2 labels once; step 0 shelved the measured margins the same day, so the scaled
   margins label it. Before it, the stale replay outputs of
   2026-10-07 (amanda-0001, the old search) are deleted from Google Drive and from the training machine's local
   `data/`: the exact directories listed and checked first, so that nothing of generation 1's
   corpus or the CCRL 2026-09 chunks goes with them.

## Tests

- The scale: hand-crafted's is exactly 1; a network whose weights are all doubled measures twice
  the scale of the original (before rounding); the result is a multiple of 0.05 within the clamp.
- With `s = 1` every margin equals today's constant.
- Lazy updates: the existing check that incremental accumulators equal a refresh at every node
  (`a_psq_search_with_incremental_accumulators_matches_one_that_refreshes_every_node`).
- The TT evaluation: an entry's stored evaluation equals a fresh one for the same position.
- `node-stats` stays, to recount after steps 2 and 3.
