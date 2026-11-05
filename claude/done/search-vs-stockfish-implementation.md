# Rotor's search closer to Stockfish's: implementation plan

**Status:** proposed 2026-10-31; steps 0 to 6 done the same day, results and decisions in
search-vs-stockfish.md (`corr` and `blend` kept, `pv`, `root` and `ttquiet` reverted, the verdicts
played again standing). Step 7 dropped with the quiet filter (quiet-filter.md). Step 0's cleanup
done 2026-11-02, wider than planned (search-switches-cleanup.md): every switch and the
`@default+<switch>` syntax removed, `corr` and `blend` the search's plain code. The design: search-vs-stockfish.md
(the comparison, the five candidates, their defaults, how each is judged).

## Order

Step 0, then candidates 1 to 5 one at a time, each a step of code, tests and duels; then the
verdicts played again; then the experiment with the full quiet filter (TODO). A candidate kept
becomes the base the next is duelled against.

## Step 0: two searches in one process

**Written 2026-10-31, to build and test:** `SearchSwitches` in `search.rs` (a `switches` field of
`SearchParameters`, every switch off, read by nothing yet), arena's `split_suffix`, smithy's
`matches::Side` and `test_player`, and the page's duel form (a switches box beside a side playing
the defaults). Tests: `search_switches_follow_default_by_name_in_any_order` (arena),
`a_side_s_search_switches_read_back_in_their_own_order` (smithy).

A duel plays the old search against the new in one arena process, as `@default` against `@t1`
plays two parameter sets today. So each candidate is a switch, not a replacement, while it is
tested.

- `SearchParameters` gains one `bool` per candidate (`pv_guard`, `root_futility_off`,
  `rfp_tt_guard`, `correction`, `blended_returns`), each `false` in `DEFAULT` until its candidate
  is kept.
- **The syntax (proposed 2026-10-31):** `@default` followed by `+<switch>` for each switch turned
  on, by name, not number, so a duel's record still reads in a month: `g1-r1@default+pv`,
  `@default+pv+corr`. The names: `pv` (1), `root` (2), `ttquiet` (3), `corr` (4), `blend` (5).
  Order free, a repeat or an unknown name refused with the list of names. On `@default` only:
  `@t1+...` and `@<cp>+...` refused, every test being untuned. In-process only: over UCI a suffix
  cannot set the search (`match_player.rs`), so a switched player there is refused too. Parsed in
  `split_suffix` (`crates/arena/src/match_player.rs`) and the duel form's sides
  (`crates/smithy/src/matches.rs`).
- Kept: the switch's `DEFAULT` turns `true`, then the switch and its old branch are deleted once
  the re-run verdicts are in, so the search does not carry dead branches. Dropped: switch and
  code deleted at once.
- Tests: a suffix parses to the right parameters; an unknown candidate number is refused.

## Steps 1 to 5: each candidate

Each step: the code behind its switch, unit tests on the pure function where there is one (as
`is_null_move_pruning_applicable` is tested today), `cargo test` and clippy, a bench of nodes and
speed on the bench positions, then four duels (two networks, two controls), both sides `@default`
but for the switch:

- `lc0-psq-threats-screlu-g1-r1@default` against `...@default+<switch>` (with the kept ones'
  switches on both sides until their `DEFAULT` turns), 10+0.1, then depth 9;
- `-quiet`'s generation 1 winner the same.

10+0.1 decides (won kept, lost dropped, drawn by the developer); depth 9's result is reported with
both sides' node counts.

1. **No razoring, reverse futility or null move at PV nodes.** `let pv_node = beta - alpha > 1;`
   at the node's top; the three guards gain `!pv_node` when the switch is on.
   `is_null_move_pruning_applicable` takes it as an argument. Test: at a PV node none fires.
2. **No futility at the root.** The classical futility check gains `ply > 0`; the root's
   evaluation is then not computed (`node_eval`'s condition loses `remaining <= FUTILITY_MAX_DEPTH`
   at `ply == 0`). Test: the root's quiet moves are all searched at depth 1 to 3.
3. **Reverse futility only without a quiet TT move.** The guard gains `tt_move.is_none_or(|mv|
   !is_quiet(board, mv))`. Test: a node whose TT move is quiet is not pruned.
4. **A correction history.** Two parts:
   - **The keys, in `Board`:** `pawn_key: u64` and `non_pawn_keys: [u64; 2]`, kept by
     `place_piece` and `remove_piece` from `zobrist::PIECE_SQUARE`, kings in the non-pawn keys
     (decided 2026-10-31, as Stockfish's), and computed from scratch where `hash` is (`zobrist.rs`).
     Tests: after any sequence of `make_move`/`unmake_move`, and a null move, each key equals its
     from-scratch value (the existing hash test's shape).
   - **The tables, in `Search`:** three tables of 16,384 entries by side, fresh per search; the
     correction read once per node where `node_eval` is computed and in quiescence's stand pat,
     the TT keeping the uncorrected value; the update at the node's end, with the conditions,
     weight `min(depth, 8) / 16`, error clamp of two pawns and correction clamp of one pawn of
     search-vs-stockfish.md, in the evaluator's pawn (`pawn_percent`). Tests: an entry moves
     halfway at depth 8 and a sixteenth at depth 1; a fail high never lowers an entry, a fail low
     never raises it; the correction never passes one pawn; mate and tablebase scores never enter.
5. **Blended returns.** Reverse futility returns `(static_eval + beta) / 2`, stand pat on a cutoff
   `(stand_pat + beta) / 2`; mate and tablebase scores unblended. Test: both returns, and that a
   decisive score passes through.

## Step 6: the verdicts again

With every kept switch on, `@default`, the same controls as before:

- `-quiet`'s generation 1 winner against `lc0-psq-threats-screlu-g1-r1`, depth 9 and 10+0.1 (were
  about -160 and -108 Elo).
- `-quiet-r1`'s generation 2 winner against its generation 1 (the gate was 48.5%, about -10 Elo),
  as a duel, both sides `@default`.

Then the switches of the kept candidates deleted (step 0), and search-vs-stockfish.md's results
filled in.

## Step 7: the experiment with the full quiet filter

As TODO's entry: after the Convert ends and the forges read `data/lc0-counts`, with the new
search. Planned in a document of its own then.

## Docs

search-vs-stockfish.md gets each duel's result as it comes; TODO.md an entry pointing here;
`search.md` (the search's design notes, if it still describes these techniques) amended for each
kept candidate.

## Decided

- **Every duel on this machine (the developer, 2026-10-31),** idle while it runs.

## Open

- The suffix syntax (step 0), proposed.
