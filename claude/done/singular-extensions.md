# Singular extensions

**Result (2026-10-03): removed.** Duel on the training machine at 10s+0.1, `amanda-0001+nose` (without) against
`amanda-0001` (with), records from `+nose`'s side: drawn at the 1000-game cap, `+301 -269 =430`,
S 0.516 [0.451, 0.549]. Singular extensions about -11 Elo (95% roughly -29 to +7): not shown
worse, but leaning against for the last 700 games, and this version is not Stockfish's, so
"Stockfish has it" cannot break the tie. Removed whole (the developer): the test, its constants,
the per-ply excluded move, the temporary `SingularExtensions` option and `+nose` suffix, and the
depth-9 fixed search recorded with it. What a later attempt could change, each one a duel:
`ttPv` (Stockfish widens the margin off the PV and raises the minimum depth on it), a margin under
1 cp a ply (Stockfish's), the -3 negative extension instead of -1, the double and triple
extensions (which need correction history), and Stockfish's `depth++` on a singular move.

Agreed with the developer 2026-10-03. The last missing technique of TODO.md's "Missing
techniques" item: extend the TT move when every alternative fails well below its score.

## Stockfish's rule (fetched 2026-10-03, `src/search.cpp` on master, no commit hash in the file)

The excluded move is per node (`ss->excludedMove`); the TT key is the plain `pos.key()`, so the
excluded search reads the same entry. At a node with an excluded move, Stockfish:

- skips the TT cutoff (`if (!PvNode && !excludedMove && ...)`), the tablebase probe (`if (!rootNode
  && !excludedMove && tbConfig.cardinality)`), null move (`&& !excludedMove && pos.non_pawn_material(us)`)
  and the TT store (`if (!excludedMove && !(rootNode && pvIdx))`);
- does **not** skip razoring or futility;
- reuses the static evaluation (`else if (excludedMove) unadjustedStaticEval = eval = ss->staticEval;`);
- skips the move in the loop and in ProbCut (`if (move == excludedMove) continue;`);
- with no move searched, returns `alpha`, not mate or stalemate
  (`bestValue = excludedMove ? alpha : ss->inCheck ? mated_in(ss->ply) : VALUE_DRAW;`).

The test itself (Step 16):

```cpp
if (!rootNode && move == ttData.move && !excludedMove && depth >= 6 + ss->ttPv
    && is_valid(ttData.value) && !is_decisive(ttData.value) && (ttData.bound & BOUND_LOWER)
    && ttData.depth >= depth - 3 && !is_shuffling(move, ss, pos) && !seekMate)
{
    Value singularBeta  = ttData.value - (59 + 66 * (ss->ttPv && !PvNode)) * depth / 63;
    Depth singularDepth = newDepth / 2;

    ss->excludedMove = move;
    value = search<NonPV>(pos, ss, singularBeta - 1, singularBeta, singularDepth, cutNode);
    ss->excludedMove = Move::none();

    if (value < singularBeta)
    {   // doubleMargin, tripleMargin from correction history and ttMoveHistory
        extension = 1 + (value < singularBeta - doubleMargin) + (value < singularBeta - tripleMargin);
        depth++;
    }
    else if (value >= beta && !is_decisive(value))
    {   // ttMoveHistory and correction-history updates
        return value;
    }
    else if (ttData.value >= beta || cutNode)
        extension = -3;
}
```

## Rotor's version

### The excluded move

`Search::excluded: [Option<Move>; MAX_PLY + 1]`, `None` everywhere outside a singular search,
beside the root-only `excluded_root_moves` (MultiPV), which stays as it is. In
`alpha_beta_with_buffer`, with `excluded[ply]` set:

- no TT cutoff, no TT store, no tablebase probe, no null-move pruning;
- razoring and reverse futility stay, as in Stockfish;
- **the per-ply rows are not cleared at entry**: `static_evals[ply]` belongs to the parent node at
  the same ply (the evaluation is reused from it, not recomputed, and must survive for the parent's
  own move loop and `is_improving`); `pv[ply]` is still empty at that point, since the TT move is
  the parent's first move and the test runs before it is searched;
- the excluded move is skipped in the move loop, before make/legality/pruning, as the root
  exclusion is;
- no legal move searched: return `alpha` (fail low), never mate or stalemate;
- killers and history update as at any node, as in Stockfish.

The excluded search does not push `ancestors` (same position, no move made).

### The test

Inside the move loop, before the TT move is made, when all hold:

- `ply > 0`, `mv == tt_move`, `excluded[ply]` is `None`;
- not in check (Rotor-specific: a check evasion node already has its own treatment, and keeps the
  test away from forced sequences the check extension already handles);
- `remaining >= SINGULAR_MIN_DEPTH = 6` (Stockfish's base, without `ttPv`, which Rotor lacks);
- the TT entry's bound is `Lower`, its score neither a mate nor a tablebase win/loss
  (`is_win`/`is_loss`/`is_tb_win`/`is_tb_loss`), its depth `>= remaining - 3`.

Not ported: `ttPv`, `is_shuffling`, `seekMate` (Rotor has none of the three).

`singular_beta = tt_score - SINGULAR_MARGIN_PER_PLY * remaining`, `SINGULAR_MARGIN_PER_PLY = 2`
(centipawns). Stockfish's margin is about 1 internal unit a ply (2 off the PV), under a centipawn a
ply; Rotor's is a round value a little wider, since its evaluations are noisier. Search with the
null window `(singular_beta - 1, singular_beta)` at `remaining / 2`.

### The outcomes

- `value < singular_beta`: the TT move is singular, `extension = 1`. No double or triple extension
  (their margins rest on correction history and TT-move history, which Rotor lacks), no `depth++`.
- `value >= beta`, not a mate or tablebase score: multi-cut, return `value`.
- otherwise, `tt_score >= beta`: `extension = -1` (Stockfish's -3 is tuned against its LMR; Rotor's
  is different, so the smallest step).

Combined with the check extension, a move gets at most +1 in total. `extension` becomes signed
(`i32`), and the child's depth is computed with a floor so that `target_depth + extension` never
drops below `ply + 1` (a reduced TT move still gets searched, as LMR's own floor already ensures).

## Tests

- An excluded move is never searched at that ply: a position with one winning move, excluded,
  scores worse (mirrors the root exclusion test).
- With an excluded move, no legal alternative returns `alpha`, not a mate score.
- A position with one clearly only move (all alternatives lose material) gets the extension: the
  singular search fails low (checked through a small helper, or a node count larger than without).
- The fixed-search test (`a_fixed_depth_search_reproduces_its_recorded_nodes_scores_and_moves`) is
  re-recorded at depth 6. Depth 6 at the root fires the test only at `remaining >= 6`, i.e. never
  below the root; if that leaves the recorded values unchanged, add one deeper position.

## Measurement

Time to depth does not judge it: extensions grow the tree on purpose. SPRT against the current
code at 10s+0.1 (`rotor-arena sprt`), as for the move picker. The bench numbers (nodes, seconds at
depths 10 to 12) are recorded anyway, to know the cost.

## The duel's switch (agreed 2026-10-03, temporary)

`rotor-arena` runs one binary and names players by network, so the old search needs a switch, as
`PonderBonus` had: `search::SINGULAR_EXTENSIONS`, a process-wide `AtomicBool` (default on) read by
the singular test, set by the UCI option `SingularExtensions` (check, default `true`); the arena
suffix `<player>+nose` sets it off. Process-wide, so meaningful only over UCI: the arena refuses
`+nose` in-process, which drops the depth-9 smoke test (the bench and the depth-9 fixed search
already show the test fires and the search runs). The duel: `amanda-0001+nose` (champion) against
`amanda-0001` at 10+0.1. Once it decides, the option, the flag and the suffix are removed.

Step 2 measured (2026-10-03, the training machine, `nnue_bench --search-only`, amanda's champion), against the
tree before it: depth 10, 3,105,910 nodes in 2.48 s (+8.1% nodes, +7.4% time); depth 12, 8,563,149
in 5.62 s (-21.4%, -16.6%). Few nodes have six plies left at depth 10, so the extensions cost more
than multi-cut saves; at depth 12 multi-cut and the -1 dominate.

## Order of work

1. The excluded move and its tests; with `excluded` always `None`, the fixed-search test must
   reproduce exactly (a pure refactor).
2. The singular test and outcomes, its tests, the re-recording.
3. The SPRT on the training machine or the Lichess machine; TODO.md updated; this document to `done/`.
