# The search switches removed: implementation plan

Status: written 2026-11-02, steps 1 to 4 applied the same day, done: tests, clippy and the bench's
nodes unchanged, checked by the developer. As built: `blended` an
associated function (it reads no state); the correction test lost only its last check. Closes search-vs-stockfish-implementation.md's step 0
cleanup, wider than planned (the developer, 2026-11-02: "delete everything"): not only `pv`,
`root` and `ttquiet` with their code, but the switch mechanism itself. `corr` and `blend` become
the search's plain code, `SearchSwitches` and the `@default+<switch>` syntax go from the search,
arena, smithy and the page. A later search experiment builds its own mechanism again.

**No behaviour change:** `DEFAULT` already plays `corr` and `blend` with the other three off, so the
search is the same tree: `bin/rotor bench`'s nodes stay 3,684,206.

## Steps

### 1. The search (`crates/rotor/src/search.rs`)

- `SearchSwitches` (struct, `NONE`, `NAMES`, `switch_mut`, `names`, `with`) and
  `SearchParameters::switches` removed, with `DEFAULT`'s comment on the five candidates moved to
  where `corr` and `blend` now live.
- `correction` and the correction history's update: no switch read, always on. `blended`: no
  switch read, the midpoint unless beta is a mate or tablebase score.
- `pv`: `static_cuts` gone, razoring, reverse futility and null move as without the switch.
  `root`: `root_futility` gone, the root's evaluation computed within futility's depth as today.
  `ttquiet`: `quiet_tt_move` gone.
- The measuring instruments built for the trials, test-only: `StaticCut`, `STATIC_CUTS`,
  `count_cut` and its calls, `CORRECTIONS`; the ignored measurements `static_cuts_at_pv_nodes`,
  `measure_switch`, `root_futility_at_the_root`, `reverse_futility_with_a_quiet_tt_move`,
  `correction_history_at_work`, `blended_returns_at_work`. Their numbers are in
  search-vs-stockfish.md.
- Tests: `the_pv_switch_...`, `the_ttquiet_switch_...`, `the_root_switch_...`,
  `switched_nodes` and `switched_nodes_after` removed; `ROOT_FUTILITY` replaced by `DEFAULT` (they
  are equal); the blend and correction tests keep their checks of the value, without the
  "without the switch" ones.

### 2. Arena (`crates/arena/src/match_player.rs`)

`Suffix::Switched` and its parsing removed; `@default+...` then refused as any unknown suffix. The
module's doc lines on switches; the test `search_switches_follow_default_by_name_in_any_order`
replaced by one line in the suffix test that `pesto@default+blend` is refused.

### 3. Smithy (`matches.rs`, `smithy.rs`, `page.html`)

- `Side::switches` removed; `Side::text` and `Side::parse` back to `@t1` and `@default`;
  `smithy.rs`' side parameters the plain `DEFAULT`. The test
  `a_side_s_search_switches_read_back_in_their_own_order` reduced to `aa@default+pv` being
  refused.
- The page's duel form: the switches box and its two fields gone.
- Past duels' records keep their text (`...@default+blend`) and stay in the log, but their live
  block (the pairs' table, `Record::spec`) is no longer rebuilt, their words no longer parsing;
  a stopped one cannot resume (found while applying).

### 4. Docs

search-vs-stockfish-implementation.md's status (cleanup done, the mechanism removed); TODO.md's
"Left: ... the switches' cleanup (step 0)" closed; SPSA's two `blend` weights stay a TODO, as new
parameters when SPSA comes.

## Checks

Tests and clippy; then `bin/rotor bench`, its nodes 3,684,206 unchanged.

```bash
cargo test --workspace --release
```

```bash
cargo clippy --workspace --all-targets --release -- -D warnings
```

```bash
bin/rotor bench
```
