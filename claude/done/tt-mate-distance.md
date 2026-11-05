# Mate distance in the transposition table

**Status:** done 2026-10-07. All tests pass; `search_bench` at depth 9, hand-crafted, on the Lichess machine:
3,183,462 nodes in 1,083 ms (2.94M nodes/s), against 3,183,476 before: only the rare positions
where a mate passes through the table changed. The duel was skipped (the developer): it needed a
temporary UCI switch, and 14 nodes in 3.18M cannot show in 1,000 games. On the label path: applied only after betty and caroline finish training (TODO.md), never
while a corpus is being labelled. Test 4 uses KQ vs K (`k7/8/2K5/8/8/8/8/7Q w`) and checks the
mate two plies along the PV against both the kept table and a fresh one, instead of a fixed mate
in 3.

## The problem

Every mate and tablebase score is relative to the root of the search that produced it:
`mated_in(ply)` returns `-(MATE_SCORE - ply)`, and `tb_win_at(ply)` / `tb_loss_at(ply)` return
`+-(TB_WIN_SCORE - ply)`. The transposition table stores that value unchanged and returns it
unchanged, but the same position can be reached at another ply, in the same search or in a later
search of the same game (the table lasts for the whole game). The distance it carries is then wrong:

- reached at a smaller ply than where it was stored, a mate looks farther than it is;
- reached at a larger ply, it looks closer than it is, so a slower line can look as good as a
  faster one or better.

The second case is how a won ending is not converted: lines that do not progress keep finding a
mate score that seems no farther away. A likely factor in the KQ vs K Rotor failed to convert on
Lichess on 2026-10-04 (TODO.md, the no-mop-up item).

## Where the scores go in and out

`src/search.rs`, four stores and two probes:

| | `alpha_beta_with_buffer` | `quiescence_from` |
|---|---|---|
| tablebase hit, stored at depth `MAX_PLY` | store | store |
| node result | store (`best_score`) | store (`best_score`, when a move produced it) |
| cutoff from an entry | probe (`entry.score()`, three arms) | probe (`entry.score()`, three arms) |

Nothing else reads a stored score: `uci.rs` takes only the entry's move (the ponder move), and the
`tt.rs` tests store plain `0` and `100`.

## Design

### Two conversions, in `search.rs`

Stockfish's `value_to_tt` and `value_from_tt`, translated to Rotor's two bands. Both go next to
`mate_in` / `mated_in`:

- `score_to_tt(score, ply)`: makes the score relative to the node. A score in a winning band
  (`is_win` or `is_tb_win`) gets `ply` added, a score in a losing band (`is_loss` or `is_tb_loss`)
  gets `ply` subtracted, and any other score is unchanged.
- `score_from_tt(score, ply)`: the reverse, back to relative to the root. It subtracts `ply` from a
  winning band and adds it to a losing one.

Each band is tested on its stored value, which is relative to the node: a mate there lies in
`(MATE_SCORE - MAX_PLY, MATE_SCORE]` and a tablebase win at `TB_WIN_SCORE` exactly, so the existing
`is_*` predicates still classify it. What remains to check for each band is that no value can cross
into the other: a mate at ply `p`, `MATE_SCORE - p`, plus `p` is `MATE_SCORE`, and a tablebase win
`tb_win_at(p)` plus `p` is `TB_WIN_SCORE`. Every value stays inside its own band, and the
`MAX_PLY`-wide gap between the two bands (`TB_WIN_SCORE`'s doc comment) is wider than any shift of
up to `MAX_PLY`.

The stored value fits the table's `i16`: the largest magnitude is `MATE_SCORE`, 32,000, the same as
today.

### The six call sites

- The four stores pass `score_to_tt(score, ply)` instead of `score`. `classify_bound` still takes
  the unconverted `best_score`, since it compares with this node's `alpha` and `beta`, which are
  relative to the root.
- The two probes compute `let score = score_from_tt(entry.score(), ply)` once, then compare and
  return `score` in all three arms.

The tablebase stores become exact facts about the position: `tb_win_at(ply)` is stored as
`TB_WIN_SCORE` and read back as `tb_win_at(ply')` at whatever ply it is reached from, which is what
storing them at depth `MAX_PLY` assumed but did not yet deliver.

### Not in this plan

- **Stockfish's fifty-move guard in `value_from_tt`.** It refuses to return a stored mate that the
  fifty-move rule would cut off, given the halfmove clock at the probe. Rotor's search already
  scores fifty-move draws as `0` when it reaches them, so the cost of leaving it out is a rare
  stored mate that turns out to be a draw. A separate change, if a game ever shows it.
- **The engine's reported `mate N`.** `uci.rs` decodes the root score, which this plan makes
  correct; its decoding itself does not change.

## Effect on search and labels

Search results change wherever a mate or tablebase score passes through the table: `search_bench`'s
node counts change too, so there is no "same tree" check for this one. Labels produced after it
differ from the current corpus in decisive positions only, so the change goes in between two
generations' labelling, never during one (TODO.md's label-path rule).

## Tests

In `search.rs`'s tests:

1. **Round trip.** For `ply` in `{0, 1, 7, MAX_PLY}`, `score_from_tt(score_to_tt(s, ply), ply) ==
   s` for a mate, a mated score, a tablebase win, a tablebase loss, `0`, and an ordinary
   evaluation; the last two are unchanged by each conversion alone too.
2. **Node-relative value.** `score_to_tt(mate_in(p), p) == MATE_SCORE` and
   `score_to_tt(tb_win_at(p), p) == TB_WIN_SCORE` for several `p`, and the same for their mirrors.
3. **Stored at one ply, probed at another.** `mate_in(5)` stored at ply 5 through a real
   `TranspositionTable`, probed, and converted at ply 1, gives `mate_in(1)`: four plies nearer the
   root. The same for `mated_in` and the tablebase bands. Going through the real table covers the
   `i16` packing.
4. **Across moves of one game.** A forced mate in 3 (a known position, to be chosen), searched with
   a fresh table: the root reports mate in 3. Both sides then play the PV's first two plies, and the
   same table, not cleared, searches the new root: it must report mate in 2. This is the case the
   bug gets wrong.

## Verification

- `cargo test` on Windows.
- `search_bench` at depth 9: node counts and time recorded as the new reference, not compared.
- The KQ vs K conversion. A position from the 2026-10-04 game (its PGN still to be fetched, TODO.md)
  becomes a regression position: the engine through `bin/rotor` from that position, `go movetime`
  a few seconds per move, must mate within the fifty-move rule. Without the PGN, a textbook KQ vs K
  start does the same job.
- A duel against the commit before, at 10s+0.1, under `hand-crafted`: not expected to change Elo
  much, since mates are rare outside endgames; it checks that nothing regressed.

## Order

1. The two conversions and tests 1 and 2.
2. The six call sites, then tests 3 and 4.
3. Verification as above, then this document moves to `done/` and TODO.md's item is closed.
