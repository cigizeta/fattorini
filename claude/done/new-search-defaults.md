# New search defaults: implementation plan

**Status:** done 2026-11-08, tests and clippy passed, the bench 0.716 of the old nodes; step 5 of search-defaults.md, which holds the why: each value argued
in chess terms (question 1) and validated by four duels, new against old (PeSTO +29 and lc0 +54 Elo
at 10+0.1, both even at depth 9).

## The change

| Parameter | Old | New | Range, old to new | `c_end`, old to new |
|---|---|---|---|---|
| `razoring_margin` | 200 | 150 | [50, 400] kept | 25 to 19 |
| `reverse_futility_margin` | 200 | 100 | [50, 400] to [25, 300] | 25 to 12.5 |
| `futility_margin` | 200 | 200 | kept | kept |
| `null_move_margin` | 200 | 200 | kept | kept |
| `delta_margin` | 200 | 200 | kept | kept |
| `see_prune_margin` | 200 | 150 | [50, 400] kept | 25 to 19 |
| `see_margin` | 50 | 50 | kept | kept |
| `aspiration_delta` | 20 | 10 | [5, 60] to [3, 40] | 4 to 2 |
| `history_pruning_multiplier` | -35 | -35 | kept | kept |
| `good_quiet_threshold` | -128 | -140 | [-512, 0] kept | kept |

- **Ranges:** re-centred where the new value sat near an edge. `reverse_futility_margin` at 100
  had its floor at 50, half its value; `aspiration_delta` at 10 had its floor at 5, and the last
  tune ended at 7.
- **`c_end`, the perturbation:** kept at the old ratio to its value, an eighth for the margins
  (200 to 25), a fifth for `aspiration_delta` (20 to 4), so a smaller value is not perturbed by a
  larger share of itself. The end step scales with `c_end^2` (`R_END`), so it shrinks with it.

## Steps

1. **`search.rs`:**
   - `SearchParameters::DEFAULT` with the new values, each field's doc comment giving its argument
     in chess terms (from search-defaults.md's question 1, inlined, the code naming no document).
   - `MARGIN_PER_PLY`: no longer every margin's default. Kept as the old shared value only if the
     tests below still need it, its doc comment rewritten; otherwise removed.
   - The tests that assume a margin equals `MARGIN_PER_PLY` (futility, razoring, SEE pruning,
     quiescence, around `search.rs:4179-4315`, `5329-5345`, `6370-6480`) read the field they test
     from `SearchParameters::DEFAULT` instead, so they follow any later change.
2. **`spsa.rs`:** `SPECS` as the table above. The existing test (`rounded(start_theta(&SPECS)) ==
   SearchParameters::DEFAULT`, every start within its range) keeps the two in step; the two tests
   that clip at a range's edge (`spsa.rs:233`, `259`) checked against the new ranges.
3. **`smithy.rs`, the forge pinned (question 2 not yet decided):** `FORGE_PARAMETERS` no longer
   `SearchParameters::DEFAULT` but the old values, spelt out, its comment saying why: the forge
   moves to the new defaults only once the bench and question 2 say so. A test that it equals the
   old values, so no later change of `DEFAULT` moves the forge unnoticed.
4. **What changes for the players:** `@untuned` plays the new values everywhere (tests, duels, a
   new snapshot); a snapshot already built keeps what it embedded, lichess-bot's included; a
   finished tune keeps its own specs (`spec.txt`); PeSTO untuned plays the new values.

## Check (the developer)

```
cargo test --workspace
```

```
cargo clippy --workspace --all-targets
```

```
bin/fattorini bench
```

The bench's node count against the old defaults' 3,684,206 (search-defaults.md, question 2).
