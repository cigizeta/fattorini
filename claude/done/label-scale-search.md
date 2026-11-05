# The label-space scale `c`, step 2: the search's pawn from `c`

**Status: plan, agreed 2026-10-20; done 2026-10-20, every step tested (the A/B stopped early, `c` accepted).** Follows label-scale.md, whose Result 4
accepted `c` (the developer, 2026-10-20) although the `100 / c` near `p` sanity check fails.

## Decisions

- **One linear factor.** The search keeps its percent arithmetic, `pawn_percent = 100 / c`, rounded
  to 5% and clamped to 25 to 200% as today. Near 0 the label curve is linear, so this is `c`'s fit
  where the margins act. The four champions measured 33.6 to 35.4%: all round to 35%.
- **Everything `p` scaled, `c` scales:** the four evaluation margins (razoring, reverse futility,
  futility, null move), delta pruning (piece value plus `DeltaMargin`; a material test, first
  suspect if the A/B loses), and time management's falling-score band. SEE margins and the
  aspiration window stay unscaled. Hand-crafted stays at 100%.
- **Measured at load**, in place of `measure_pawn_percent`. PeSTO's quiescence on the 10,000
  positions does not depend on the network: computed once per process, beside the parsed boards.
- **`margin.txt` retired.** Only `parameters.txt` remains as a network's tuning file. `arena margin`
  stays as the cheap tune before the gate (the developer, 2026-10-20), its winner written as a whole
  `parameters.txt` (`SearchParameters::with_margin`). Promotion, upload, the arena's pulls and
  `smithy install` carry `parameters.txt` where they carried `margin.txt`; install removes a stale
  `margin.txt` from the data root's `bin/`.
- **Existing tuning files deleted** by the developer, every `parameters.txt` and `margin.txt`: tuned
  under `p`, they mean about 35% less under `c`. No rescaling.

## A/B, with the arena

The arena's players live in one build, so the build keeps `p` measurable during the transition, and
a player can search at `p`'s scale (a suffix on the player's name, to be settled in step 4). The
match: caroline-0001's champion at `c` against itself at `p`, STC, on the Lichess machine. `arena sprt` asks
whether the challenger is at least `elo_d` stronger; a non-regression test needs the other question,
to be checked in `sprt.rs` before step 4 and added if missing. If `c` loses, delta pruning is
tried on `p` before anything else. After the A/B, the `p` path is removed.

## Steps

1. `nnue.rs`: `measure_label_percent(evaluate) -> i32`, `100 / c` rounded and clamped; PeSTO's
   quiescence values cached once per process. `Network::from_psq` uses it. Tests: the existing
   `measure_pawn_percent` tests rewritten for it (hand-crafted measures 100%, twice it 200%, the clamp).
2. `uci.rs`: the `info string` says "pawn scale N% (label space)". Its test updated.
3. Retire `margin.txt`: `Tuning::Margin`, `margin_file_beside`, `read_margin_file` go;
   `arena margin` writes `parameters.txt`; smithy's promote, upload and install, `match_player.rs`'s
   and `replay.rs`'s pulls carry `parameters.txt`. Tests updated.
4. The A/B. `sprt.rs` is not touched: its test is one-sided at `p0 = 0.5`, and its error bounds
   rest on that. The no-worse question is asked by swapping the roles: `c` the champion, `p` the
   challenger, elo_d 10 (the developer, 2026-10-20). Fail accepts `c`; Promote means `p` is at least
   10 Elo stronger, a regression. The `p` player is the suffix `#p` before any `@`
   (`caroline-0001#p@default`), a network only, in-process only:
   `Network::with_static_pawn_percent`, `EngineEntry::static_pawn_scale`. Both sides `@default`, so
   only the scale differs.
5. After the A/B: remove `measure_pawn_percent` and the `#p` player; `nnue_bench --pawn-scale` goes,
   `--label-scale` stays.

## Result (the Lichess machine, 2026-10-20): stopped early, `c` accepted

At 10+0.1, after 204 games, `c` (the champion) led 109 to 95 (+77 -63 =64 from its side, 53.4%,
about +24 Elo, noise about ±40), LLR -0.49 of the -2.944 that accepts `c`. Finishing the test at
elo_d 10 needed an estimated 1,000 to 7,000 more games, 5 to 15 hours. **Decided (the developer):**
stopped, `c` accepted on this, on its better fit in label space, and because SPSA retunes the
margins anyway. A small regression is not ruled out. Step 5 follows.

## Steps for the developer

- Before using the build: delete every `parameters.txt` and `margin.txt` beside a network, locally,
  on the training machine and on Drive.
- Run the tests, then the A/B (commands given at step 4).
