# Quiescence: a noisy-only generator, after deterministic ties

Agreed 2026-10-03 (option 2 of the discussion). Supersedes nothing; it is the item "generate only
captures and promotions in quiescence outside of check" of TODO.md's search-speed list, after the
two discarded fixes recorded there.

## The problem

Outside of check, `quiescence_from` (`search.rs`) calls `pseudo_legal_moves`, which generates every
move, quiets included, and then `try_best_noisy_move` rescans the whole remaining buffer at every
pick. Quiets cost twice: generated, then rescanned. Together about three quarters of
`quiescence_from`'s self time, 12% of the search (depth 11 over the 49 Stockfish positions).

The obstacle met before: `try_best_noisy_move` is a swap-based selection, not stable, and the
TT-move swap (`try_killer`) is another swap. Which of two captures with equal `move_score` goes
first depends on where the quiets sit in the buffer. Dropping the quiets changed the tie order, and
the trees grew at every depth (d10 +4.9%, d11 +17%, d12 +11%), hiding the gain in nodes/s.

## The plan: two changes, measured separately

### Step 1: deterministic ties in quiescence's pick (its own change, its own measurement)

`try_best_noisy_move` picks the highest `move_score`, and among equal scores the move with the
smallest key `(from, to, promotion)`, compared as integers (`promotion` as `None` < Knight < Bishop
< Rook < Queen, or any fixed order; it only has to be total, since `from` and `to` alone tie only
between the promotions of one pawn). The pick then depends only on the set of moves left in
`moves[i..end]`, never on their positions, so no swap anywhere can change it.

- Only quiescence calls `try_best_noisy_move` (checked 2026-10-03), so the main search's staged
  picker is untouched.
- The TT move still goes first, as today; the key decides only what follows.
- This changes the trees. Measure it alone, on today's generator: time to depth at 10, 11 and 12
  over the 49 Stockfish positions (`nnue_bench --search-only`), against the current code. Not
  expected to gain or lose much; what matters is that its effect is known and is not mixed with
  step 2's.
- Tests: two captures with equal `move_score` in either buffer order give the same pick; a quiet
  placed between them changes nothing.

### Step 2: `noisy_moves`, a generator of the noisy moves only

`movegen::noisy_moves(board, color, moves)`, beside `pseudo_legal_moves` in `movegen/mod.rs`:
exactly the pseudo-legal moves that `is_quiet` rejects.

- **Pawns:** diagonal captures (each one expanded into the four promotions on the last rank), en
  passant, and the single push onto the last rank (four promotions). No other push.
- **Knight, bishop, rook, queen, king:** the same attack sets as today, masked by the enemy's
  occupancy.
- **Castling:** never noisy, skipped.
- Each piece module gets its own restricted function, sharing the attack computation with the full
  one, rather than generating everything and filtering (which is what is being removed).

`quiescence_from` calls `noisy_moves` outside of check and `pseudo_legal_moves` in check, where
every evasion is mandatory. The `!is_quiet` gate on the TT move stays: the TT is shared with the
main search, and a quiet TT move is still not in the buffer.

With step 1 in, step 2 searches **exactly the same tree**: same moves in the buffer apart from
quiets, which were never picked, and an order-independent pick. So step 2 is a pure speedup:

- **Exactness test (movegen):** over a set of positions (the perft suite's, plus positions with en
  passant, promotions by push and by capture, and both colours), `noisy_moves` equals the noisy
  subset of `pseudo_legal_moves` as a multiset.
- **Same-tree test (search):** a fixed-depth search from a few tactical positions gives the same
  node count, score and best move with `noisy_moves` as with full generation. Kept as a regression
  test only if it can be written without a temporary switch in the hot path; otherwise run once
  during the change and dropped.
- **Measurement:** nodes/s alone suffices, since the tree is the same. Same bench as step 1.

## Step 1 measured (2026-10-03, the training machine, amanda's champion, `nnue_bench --search-only`)

| tie order | nodes d10 | nodes d11 | nodes d12 | time d12 |
|---|---|---|---|---|
| old (buffer position) | 2,916,085 | 5,307,107 | 9,392,779 | 6.21 s |
| smallest key (kept) | -1.5% | +3.9% | +16.0% | +15.3% |
| largest key (trial only) | -4.0% | -0.9% | +11.8% | +12.1% |

Read as noise: ties nearly always share the attacking piece type, and within one type the old
order was close to ascending squares, i.e. to the smallest key, scrambled by the swaps; two mirror
keys have no reason to differ in expectation. At depth 12 over 49 positions the tie order alone
moves the tree by more than 15%, so this bench cannot rank tie orders. Step 1 is kept as neutral;
an SPRT against the old code is the check, should it ever be doubted.

## Step 2 measured (2026-10-03, same bench)

The same tree, as designed: nodes identical to step 1 at every depth (2,873,191, 5,514,404,
10,893,226), and the six positions of `a_fixed_depth_search_reproduces_its_recorded_nodes_scores_and_moves`
gave the same values with and without step 2 (their re-recording is step 1's). Single runs:

| depth | step 1 | step 2 | nodes/s |
|---|---|---|---|
| 10 | 2.38 s | 2.31 s | +3.1% |
| 11 | 4.08 s | 4.00 s | +1.9% |
| 12 | 7.16 s | 6.74 s | +6.2% |

Below the about 9% the profile suggested; with single runs, the gain is somewhere in 2-6%.

## Order of work

1. Step 1, its tests, its measurement; the developer commits it.
2. Step 2, its tests, the node-count check, the nodes/s measurement; the developer commits it.
3. TODO.md's item updated with both measurements; this document moves to `done/`.
