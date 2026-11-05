# Labelling speed: a faster search per node

**Status (2026-10-12): A1, A2, B1 done, 1,503 to 1,901 positions/s (+26%); A3 skipped (the
search's one shared move buffer never reallocates; little to gain for a wide change); B2 measured
and dropped; B1's depth-9 harm-check duel drawn, so B1 stays; B3 (pawns by shifts) +3.7%, its duel drawn,
kept. Done. Track B started during generation 3's replay, at the developer's request.** The goal is
the fastest search per node; the tree may change (the developer, 2026-10-12).

## Why

Labelling is search-bound: on the Lichess machine, plugged in, the replay ran at 1,382 positions/s while
`nnue_bench --search-only --games <a replayed chunk> --threads 16 --syzygy` searches the same kind
of games at 1,457 (1,503 with the temporary LTO build), so reading, writing and bookkeeping are
about 5%. Every percent of search speed is a percent of every generation's labelling time.

## The profile (2026-10-12, the Lichess machine, 16 threads, amanda-0002, a replayed CCRL chunk)

samply, inverted call tree, self time, with line tables (`Cargo.toml`'s temporary
`[profile.release]`), inlined functions named:

| share | where | what |
|---|---|---|
| about 10% | `simd::update_i16_avx2` (8.0% of it in the loops over the changed rows) | the accumulator update, waiting on weight-row loads |
| 5.7% + 3.7% | `move_picker::sort_descending`, `MovePicker::next` | ordering: a full insertion sort of each stage |
| 4.6% | `Search::alpha_beta_with_buffer` | the search's own body |
| about 11% | `Board::bitboard` 3.0, bitboard `eq` 2.2, bitboard iteration 2.1, `Square::try_offset` 2.1, `Square::new` 1.6 | move generation, pawns square by square (`movegen/pawn.rs`) |
| 4.1% | `RawVec` 2.3, `Vec::push_mut` 1.8 | move lists grown through `Vec` |
| about 7% | `see::attackers_to`, `see_ge` | SEE |
| 2.1% | `Board::piece_at` | |
| 1.6% | `tt::unpack_entry_data` | |

Sliders already use PEXT tables (`movegen/sliding.rs`); knights and kings, tables.

## Changes

### A. Tree-identical (node counts unchanged to the last digit)

Labels do not change, so each can land at any time, even during a replay.

- **A1. The accumulator update.** Several 16-lane chunks per pass (Stockfish keeps a tile of the
  accumulator in registers), so more weight-row loads are in flight at once; and specialised
  kernels for the common cases, one row added and one removed (a quiet move) and one added and two
  removed (a capture), with the general loop kept for the rest.
- **A2. One flat PEXT table.** `SlidingAttackTable::attacks` is a `Vec<Vec<Bitboard>>`, two
  dependent loads per lookup; one flat array with an offset per square, as Stockfish, is one.
- **A3. Move lists without `Vec` growth.** A fixed-capacity list (256 moves bound every position)
  written by index, no capacity checks per push.

### B. Tree-changing (one harm-check duel for the bundle)

Labels change, so B lands only between generations, never during a replay.

- **B1. Staged generation.** Today every pseudo-legal move is generated before the picker starts,
  and the quiets are partitioned from the captures. Stockfish generates the captures first and the
  quiets only when the picker reaches them: a node cut off by the TT move or a capture never
  generates, scores or sorts a quiet. The likely largest single gain at cut nodes.
- **B2. Partial insertion sort of quiets.** Stockfish's `partial_insertion_sort(cur, end, limit)`
  sorts only the quiets scoring above a depth-scaled limit (`-3560 * depth` on its history scale)
  and leaves the rest in generation order. Rotor's limit: scaled to Rotor's history range, chosen by
  reasonable magnitude, not tuned.
- **B3. Pawns by bitboard shifts.** All pushes, double pushes, captures and promotions at once by
  shifts of the pawn bitboard, instead of `try_offset` per pawn: changes the order of pawn moves.

Correctness: perft (every move still generated, none twice) for A2, A3, B1 and B3; the existing
picker tests for B1 and B2; the accumulator test against a refresh at every node for A1.

## Measurement

Each step alone, on idle the Lichess machine, plugged in, the replay stopped:

- **Speed:** `nnue_bench --search-only --games <the same replayed chunk> --threads 16 --syzygy`,
  positions/s against the step before (1,503 with LTO, 1,457 without), the faster of two runs.
- **A only:** single-thread node counts unchanged (950,209 on the 49 positions, 70,924,799 on the
  chunk with amanda-0002).
- **B:** positions/s at depth 9 is the labelling metric (nodes per position may change as well as
  their cost); then **one harm-check duel** for B1 to B3 together, at depth 9 (as run for B1, see Results) on idle the Lichess machine,
  amanda-0002 on both sides, two builds through cutechess-cli as before: the duel rule, **a draw
  keeps the bundle**, only a clear loss sends it back step by step. Speed-ups of a search's
  machinery are standard engine practice, as the falling-score term was judged.

## Results

| step | 49 positions, 1 thread | replayed chunk, 16 threads | node counts |
|---|---|---|---|
| before (LTO build) | 3.34M nodes/s | 1,503 positions/s | 950,209 / 1,120,250,384 |
| A1 | 3.50M (+4.8%) | **1,647 (+9.6%)** | identical |
| A2 | 3.51M (noise) | 1,686 (+2.4%) | identical |
| B1 (+ `pawn_attacks` as a table) | 3.95M, 865,212 nodes | **1,901 (+12.8%)** | 1,110,122,672 on the chunk (-0.9%) |

| B2, measured then reverted | 3.94M, 857,805 nodes | 1,879 (-1.2%, noise) | 1,116,751,056 (+0.6%) |

**B2 dropped (2026-10-12):** no gain once B1 generates the quiets only at nodes that reach them
(the sort's cost was mostly at cut nodes, gone with B1), and the unsorted tail made the tree
slightly larger. A tree change with no speed is not worth its duel.

| lazy pick of quiets, measured then reverted | 3.90M, 865,212 nodes | 1,869 (-1.7%, noise) | identical |

**The quiet sort stays (2026-10-12).** After B1 it was the top row of the profile (6.3%), but
picking the best one at a time (exactly the sort's order, tree identical) gained nothing: the
nodes that reach the quiets mostly try many of them, so the 6% is ordering's real cost, not waste.

| knight, king, pawn tables at compile time (no `LazyLock`) | 4.02M (+1.8%) | 1,907 (noise) | identical |

Kept: as simple as before and does strictly less. The bishop and rook tables stay `LazyLock`
(built from a `Vec`).

**Harm-check duel (2026-10-12): drawn, B1 kept.** At depth 9, not 10+0.1: labelling searches
at depth 9, and a clock would mix A1 and A2's speed into B1's tree. New (`cc9ed31`) against old
(`39f88d6`, A1 and A2), amanda-0002 on both sides, 1 thread, 16 MB hash, UHO_Lichess_4852_v1
`order=random`, colours reversed, 8 at once: 265 - 262 - 473 over 1,000 games, 0.501, +1.0 +/- 15.6
Elo. Neither bound was crossed, so the duel ran to the cap.

**Profile after B1 (2026-10-12, one of 16 threads, line tables):** pawn generation is about 5.7%
of the thread's time, `generate_captures` 2.8%, `pawn_moves` 1.1%, `add_pawn_move` 0.7%,
`pawn_attacks` 0.7%, `pawn_noisy_moves` 0.4% (`try_offset` inlined into them). B3 goes ahead:
expected 3 to 4%, and the depth-9 duel is cheap. Plan: `pawn_noisy_moves`, `pawn_quiet_moves` and
`pawn_moves` by shifts of the pawn bitboard (pushes, double pushes, both capture diagonals,
promotions, en passant through `pawn_attacks`); `is_pseudo_legal_pawn_move` unchanged. Checked by
perft, noisy plus quiet equals pseudo-legal, the exhaustive `is_pseudo_legal` test and the pawn
tests; then speed on the chunk and a depth-9 harm-check duel as B1's.

| B3 (pawns by shifts), no LTO on either side | | 1,918 (+3.7% on 1,849, the last commit's best of 1,849 and 1,839) | 1,110,940,912 (+0.07%) |

B3 kept for its duel: even its slower run (1,873) beats the baseline's faster one. It re-recorded the
fixed-depth regression test again: six best moves unchanged.

**B3's harm-check duel (2026-10-12): drawn, B3 kept.** Depth 9, set up as B1's, new (B3) against
old (the last commit): 260 - 283 - 457 over 1,000 games, 0.4885, -8.0 +/- 15.9 Elo, LOS 16%. The
lower bound at 1,000 games is 0.451, not crossed. Only the order of equally scored moves changed,
with no reason to favour either order; -8 is read as noise, as the rule reads it.

B1 re-recorded `search.rs`'s fixed-depth regression test: five of six best moves unchanged,
one changed (h5d1 for h5e2), scores within a few tens of centipawns.

## Cargo.toml

**Reverted (2026-10-12, the developer): no slower release builds.** LTO's gain was uncertain (0 to
3%, single runs, near the noise), and line tables were only for profiling; to profile again, re-add
`[profile.release] debug = "line-tables-only"` for the session. Every figure above was measured
with LTO, so the build without it is likely 0 to 3% slower than the table says.

The section as written before:

`[profile.release]` is temporary (debug line tables for samply, fat LTO, one codegen unit). LTO
measured +0.6% and +1.7% on one thread, +3.2% on 16 (one run each). Decided when this plan ends:
keep the line tables (no speed cost) or not, keep LTO if A and B leave its gain standing and the
slower `smithy` link is bearable.

## Order

A1, A2, A3 first (each measured, each committed), then B1, B2, B3 (each measured for speed), then
the duel; B lands in the binary that labels the next generation after generation 3.
