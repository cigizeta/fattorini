# Tuned constants in the network's unit: implementation

**Status:** plan, 2026-11-20, from tuned-units.md (agreed, its three open decisions as recommended:
one victim factor, a units line in new tune files, SPSA's ranges translated with the starts). Not
started.

## Units

- **Network units** (compared with an evaluation): razoring, reverse futility, futility, null move,
  delta margin, the aspiration window (already tuned in them today, untranslated), and the new
  correction cap and stability window.
- **Material units, unchanged** (compared with SEE, which counts PeSTO's material, never the
  network): `see_prune_margin`, `see_margin`.
- **Unitless, unchanged:** history pruning, the good-quiet threshold, every time factor but the
  trend's slope and the stability window.

## Steps

1. **`SearchParameters` (`search.rs`)** gains two fields, 12 in all (NAMES, DESCRIPTIONS, `values`,
   `from_values`): `victim_percent`, delta pruning's piece values in percent of `piece_value`'s,
   and `correction_cap`, correction history's bound on a correction (the update's bound twice it).
   `DEFAULT` stays the reasoned classical values (`victim_percent` 100, `correction_cap` one pawn).
   New `SearchParameters::translated(pawn_percent)`: the network-unit fields times `pawn_percent /
   100`, `victim_percent` set to `pawn_percent`; PeSTO's `translated(100)` is `DEFAULT`.
2. **The search uses parameters as they are.** `eval_margins` reads them unscaled; delta pruning adds
   `piece_value(victim) * victim_percent / 100`; correction history bounds by `correction_cap`.
   `Search::pawn_percent`, `set_pawn_percent` and `in_evaluator_pawns` go. Where a search takes its
   parameters (`Tuning::parameters`, the arena's and UCI's callers), an untuned network gets
   `DEFAULT.translated(network.pawn_percent())`.
3. **`TimeParameters`** gains `eval_window` (23 in all), replacing `EVAL_WINDOW_CP`; the trend's drop
   is no longer divided by the pawn scale. `TimeParameters::translated(pawn_percent)`: `eval_window`
   times `pawn_percent / 100`, `trend_b` times `100 / pawn_percent` (a slope per centipawn). The time
   manager drops its `pawn_percent`.
4. **Tune files carry their units.** `to_file_text` writes a first line `units<TAB>network`;
   `parse_file_text` requires it. A file without it is an old, classical tune: refused with a message
   naming it, so `@search-tuned` and `@search-time-tuned` of an old network fail at resolution
   (`players.rs`), "retune, or play it @untuned"; `@untuned` plays the translated defaults. Built
   binaries keep their embedded tune and are unaffected. A new embed (`FATTORINI_SEARCH_PARAMETERS`,
   `FATTORINI_TIME_PARAMETERS`) must be in the new format, as any file.
5. **SPSA (`spsa.rs`, smithy `tune.rs`).** `SEARCH_SPECS` and `TIME_SPECS` stay classical, gaining the
   new parameters (`victim_percent` and `correction_cap` as a margin's, starting at 100 and a pawn
   with ranges a quarter to twice; `eval_window` 12, 3 to 40). New `spsa::translated_specs(kind,
   pawn_percent)`: each network-unit parameter's start, min, max and `c_end` times `pawn_percent /
   100` (the trend's slope divided), the rest unchanged. A tune measures its network's pawn scale
   once, at its start, translates the specs, then goes on from its teacher's tune as now
   (`specs_from`), the teacher's values already in network units. A tune in flight from before
   restarts.
6. **The pawn scale's last role:** measured at load as now (about a second), read only by
   `translated` and `translated_specs`. Its doc comments in `nnue.rs` say so.
7. **Tests:** `translated(100)` is `DEFAULT` for both kinds; a translated field scales and a
   material one does not; the units line round-trips and its absence is refused naming the file; a
   search given translated parameters prunes as today's scaled one did (same `EvalMargins`, same
   delta-pruning bound) at a 30% network; `translated_specs` scales start, range and `c_end`
   together. Then `cargo test --release -p fattorini`, `-p smithy`, `-p arena`, and clippy.

## Decided while implementing (the developer, 2026-11-20)

- **Old tune files are translated by hand,** not dropped: a one-off command converts an old
  `search-parameters.txt` (each network-unit value times the network's pawn scale) and writes the
  units line. Until then an old file reads as no tune.
- **Untuned is concrete values, translated where they are built** (option 1): smithy's gate and
  round robin give `play_pairs` `None` per side, translated per network into `Work::Pairs.sides`,
  one pair per pairing (`untuned_parameters`, kept per name); a trial, the arena's `@untuned`
  (`match_player::untuned_parameters`) and the arena's timed self-play translate the network they
  load. `None` on the wire keeps meaning "the network's own tune" (book scoring).

## Progress

- **2026-11-20, written, not compiled:** steps 1, 2 and 4 (the search's 12 parameters, the units
  line, old files read as untuned, UCI and `bench`/`tm-probe` translating an untuned build); the
  untuned call sites above; step 5 for the search (`spsa::translated_specs`, `tune.rs` translating
  at a new tune's start; a tune in flight from before restarts).
- **2026-11-20, compiled and tested:** fattorini, arena and smithy tests pass; clippy clean.
- **2026-11-20, written, not compiled:** step 3 and step 5 for time. `eval_window` is the 23rd time
  parameter; `TimeParameters::translated` and `untuned_for`; time files carry the units line, old
  ones read as no tune; the time manager holds its untuned parameters for limits other than a clock,
  no pawn scale; UCI, the arena's chores and in-process pairings translate an untuned network's time
  defaults; `translated_specs` covers time. A snapshot now embeds whatever tune it is given, values
  equal to the classical defaults included, since in a network's unit those are not its untuned ones.
- **2026-11-20, compiled and tested:** step 3 and step 5 for time.
- **2026-11-20, written, not compiled:** `fattorini translate-tune <player>`. Each old file, the
  local one and the remote's, gets its missing fields at their classical defaults, then
  `translated(pawn)`, which is what the old engine did at load, so the tune plays as before. The old
  file is kept as `<file>.classical`.
- **2026-11-20, compiled and tested:** `translate-tune`; step 6's doc comments. Only the fields
  added that day may be filled in; a missing older field is refused.
- **2026-11-20, written, not compiled:** `smithy translate-tunes` (the developer's choice), which
  walks smithy's catalog, pesto first, and translates each player (`players::translate_tune`, moved
  into the library).
- **2026-11-20, run:** the release network's search and time tunes were translated at 30%, with
  the old files kept as `.classical`. No other player had tune files. Generations without a winner
  were reported as errors; they are skipped now, since only a winner is tuned.
- **Next:** move this plan and tuned-units.md to done.

## After

The release's network needs a search tune, then a time tune, before its next snapshot or release.
