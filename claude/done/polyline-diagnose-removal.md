# Removing `smithy polyline-diagnose`

**Status:** planned and applied 2026-11-06; tests and clippy to run. The command existed only to answer
polyline-calibration.md, closed the same day without running it (the developer: the late lambdas of
0 are sound), so it goes.

## What goes

- `crates/smithy/src/polyline.rs`: everything from `DIAGNOSE_COMMAND` to the end of `run_diagnose`
  (`S_BINS`, `s_bin`, `GroupBy`, `pieces_group`, `Diagnosis`, `diagnose`, `scan`, `MOST_PLIES_LEFT`,
  `PliesLeft`, `Moments::gap`, `fitted`, `Steps`, `steps`, `read_games`, `FromGames`,
  `segment_paths`, `from_games`, `plies_left`, `run_diagnose`); `Moments::slope`, used only there;
  the imports left unused (`Curve`, `LOGISTIC`, `SEGMENT_SUFFIX`, `Progress`, `game_labels`, `lc0`).
- Its tests: `the_bins_of_s_cover_its_range`, `a_diagnosis_sets_the_drawn_pairs_apart`,
  `segments_are_found_at_any_depth_and_sorted`, `a_step_sums_two_neighbours_and_skips_a_certainty`,
  `the_line_through_the_gaps_finds_the_noise_and_the_step`,
  `plies_left_cap_at_the_last_line_and_set_the_drawn_pairs_apart`, `the_groups_of_pieces_cover_a_game`.
  `a_squeezed_score_reads_as_a_slope_above_1_and_a_negative_lambda` keeps its lambda check, its slope
  check dropped, renamed `a_squeezed_score_gives_a_negative_lambda`.
- `crates/smithy/src/main.rs`: the `DIAGNOSE_COMMAND` arm.
- `crates/smithy/src/buckets.rs`: `scattered_paths`, whose only caller was `steps`.

## What stays

`measure`, `ensure`, `read`, `Polyline`, `Moments` (but `slope` and `gap`), `bucket`,
`is_certainty`. `buckets::for_each_pair` keeps its `pieces` argument, which `measure` ignores: a
signature change is not worth it here.

## Check (the developer)

```
cargo test -p smithy
```

```
cargo clippy --workspace --all-targets
```

## Done when

Tests and clippy pass. Then this document goes to `done/`.
