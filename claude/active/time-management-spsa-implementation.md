# The time management's SPSA: implementation plan

Status: **implemented and verified 2026-11-13:** tests and clippy clean; the renames done by hand;
the snapshot `...-search-tuned-20261113-1730.exe` benches 565,412 nodes, as before, and lichess-bot
plays it. Next: every arena on the commit, then `--time-tune lc0-psq-threats-screlu-g1-r1` from the
page. For
time-management-spsa.md's agreed design and tuning-suffixes.md.

**As built, where it differs from the steps below:**
- The time set travels inside `TimeLimit::Clock` (`time`), the one limit that reads it, so no
  search entry point changed; `time_limit` takes it.
- `Tuning` stays the search's alone; the time file is read apart (`nnue::read_time_tuning_beside`,
  `players::pesto_time_parameters`, `FattoriniPlayer::own_time_tuning`): only suffixes, tests and
  builds read it.
- The suffix is one shared `players::TuningSuffix`, read by the arena and the smithy's tests alike.
- A time tune is its own job word, `--time-tune <player>`, its page screen `tune of <player>, time`;
  the page offers it once the search tune is finished.
- A search tune's `spec.txt` and seed are what they were (no kind line; FNV of the name), so one
  under way resumes; a time tune's spec carries `kind` and the search it plays.
- The soft fraction is floored at 0.005 (`MIN_SOFT_FRACTION`), as the design's ranges allow
  `soft_rise` past `soft_max`.
- Book scores files are named by the suffix too (`scores-<network>-search-tuned-d20.txt`).

**By hand when it lands (the developer):** on Drive and locally, `parameters.txt` to
`search-parameters.txt` and `tune/` to `search-tune/` under the lc0 network's generation 1; pesto's
`data/spsa/pesto/` tune files into `search-tune/`, its `parameters.txt` renamed, if any; the tests
list's `@tuned` to `@search-tuned`; a book scores file's name and `side` header line likewise; a new
snapshot (`-search-tuned-`) and `config.yml`. **The network (the developer, 2026-11-13):**
`lc0-psq-threats-screlu-g1-r1`, search-tuned on 2026-11-09, is time-tuned now. Every later network:
trained, then its search tuned, then its time.

## Decisions this plan adds (to check)

1. **Agreed (the developer, 2026-11-13). The fixed constants leave `TimeParameters`.** `inc_fraction`, `cyclic_soft`, `cyclic_hard`
   and `eval_window_cp` become plain `const`s beside `EXTRA_MARGIN_MS`; `TimeParameters` holds the
   22 tuned ones alone, so its file, its specs and its names are the same 22, in one order.
2. **A player's text without `tm.*` plays the default time set,** as the design says, so every
   chore of before reads the same. The smithy writes a `@search-time-tuned` player's time set in
   full, as it writes a tuned player's search set, so an arena reads no file for either; a
   `@search-tuned` player's text carries none (tuning-suffixes.md).
3. **An arena one build behind refuses a `tm.*` chore**, it does not ignore it:
   `SearchParameters::parse_file_text` rejects an unknown name. The chore fails loudly, which is the
   safe side; the arenas are updated before the tune anyway (design, section 6).
4. **Agreed (the developer, 2026-11-13): the tunes' folders named as the suffixes,** `search-tune/`
   (today's `tune/`, renamed once, no fallback) and `time-tune/`, side by side in the generation's
   folder (pesto's tuning directory alike), so both live and resume apart. The developer renames
   the existing `tune/` folders by hand when the code lands. The time tune's seed
   `seed_of("<player> time")`, so its Deltas are not the search tune's.

## Steps

1. **search.rs, the parameters as a value.**
   - `TimeParameters` made `pub`, its 22 fields `pub f64`, `Clone, Copy, Debug, PartialEq`; the four
     fixed ones moved out (decision 1). `TIME_PARAMETERS` renamed `TimeParameters::DEFAULT`, as
     `SearchParameters::DEFAULT`.
   - `NAMES: [&str; 22]`, `values() -> [f64; 22]`, `from_values`, `to_file_text`, `parse_file_text`,
     `read_file`, `write_file`, as `SearchParameters`' (real numbers, written with Rust's shortest
     exact form). Every name exactly once, nothing else.
   - `TimeManager` holds its `TimeParameters` (`TimeManager::new` takes it); `bounds`, `factors` and
     `record_iteration` read it instead of the const.
   - `iterate` and its public entries take the time set beside the search's: `parameters:
     Option<SearchParameters>` becomes `tuning: Option<Tuning>` where they take one (step 3), the
     time set from it, else the evaluator's, else the default.
   - Tests: the defaults unchanged (`time_bounds_by_hand` and the multiplier tests pass as they are);
     a file round trip; a missing, doubled, unknown or non-numeric name refused; a non-default set
     changes the bounds.

2. **The file renamed: `search-parameters.txt`** (design, section 3; 33 mentions in 11 files).
   `nnue::parameters_file_beside` becomes `search_parameters_file_beside`, and a
   `time_parameters_file_beside` added. In the same step, the suffixes of tuning-suffixes.md:
`@tuned` becomes `@search-tuned` (renamed once, no alias), `@search-time-tuned` added, each refused
when its files are missing, `@search-tuned` ignoring a time file; the smithy's stored rows naming
`@tuned` rewritten once; `players::PESTO_TUNING_DIR`'s file, `match_player`,
   `snapshot`'s `{stem}.parameters.txt`, `build.rs`' `embedded-parameters.txt`, uci.rs, the smithy
   and their doc comments. No fallback to the old name.

3. **nnue.rs, `Tuning` holds both.** `Tuning` becomes `struct Tuning { search:
   Option<SearchParameters>, time: Option<TimeParameters> }`, `Tuning::UNTUNED` both `None`, with
   `search()` and `time()` giving the defaults for a `None`. `read_tuning_beside` reads both files,
   each absent meaning `None`. Which the player plays is its suffix's (step 2).

4. **Embedding.** `build.rs` embeds `FATTORINI_TIME_PARAMETERS` into `embedded-time-parameters.txt`
   as it embeds the search's; `uci::embedded_time_parameters()` as `embedded_parameters()`;
   `EmbeddedBuild` gets `time_parameters: Option<&str>`; `snapshot`, `release` and
   `build_uci_player` write the network's time file beside the search's when it is not the default.
   The engine searches with it; no UCI option (design, section 2).

5. **protocol.rs, the player's text.** `Player { network, parameters: Option<SearchParameters>, time:
   Option<TimeParameters> }`. Written: the search list as today, then, when `time` is `Some`, all 22
   as `tm.<name>=<value>` on the same comma list. Read: the `tm.` pairs split off and parsed by
   `TimeParameters::parse_file_text`, all 22 or none; `None` plays the defaults (decision 2). Tests: round trip with and without; 21 `tm.` keys refused;
   a chore of before reads with `time: None`.

6. **The arena's players.** `InProcessPlayer` and `match_player` pass the time set to the search:
   the chore's when given, else the defaults; a named player (`match_player`) its suffix's. The UCI path embeds it (step 4).

7. **spsa.rs, generalized** (design, section 4).
   - `Spec { start, min, max, c_end }` all `f64`; `Theta`, `Delta` and `Schedule`'s arrays become
     `Vec`s of the set's length. `draw_delta`'s 64 bits cover 22.
   - `SEARCH_SPECS` (today's `SPECS`, written as reals, rounded when played as now) and `TIME_SPECS`
     (the design's table). `point` and `rounded` split: `search_point` rounds to `SearchParameters`,
     `time_point` keeps the reals for `TimeParameters`.
   - `enum TuneKind { Search, Time }`, its `names()`, `specs()`, `specs_from(&Tuning)`.
   - Tests: today's, on the search set, unchanged in what they check; `TIME_SPECS` start at
     `TimeParameters::DEFAULT`, each start inside its range, every `c_end` positive.

8. **tune.rs and the smithy.**
   - `TuneArgs` and `TuneConfig` get `kind`, `search` the default; `--kind time` on the command line
     and `kind: time` in a config's `tune:`.
   - `Tune` carries its kind; `spec.txt` gains a `kind` line (a search tune's spec without one reads
     as `search`, so a finished tune still reads); `state.tsv`, `pairs.tsv`, `rows()` and
     `run_in_spec` iterate the kind's names. The search tune's files read exactly as today.
   - A time tune's chore: both sides play the network's tuned search in full, and their own time
     point, `tm.*` in full. `finish` writes `time-parameters.txt` beside the network.
   - `tune_dir` becomes the kind's, `search-tune/` or `time-tune/` (decision 4); one tune of each
     kind a player.
   - The page: the tune's start form gets the kind; its table lists the kind's names, as now.

9. **`cargo test --workspace --release` and `cargo clippy --workspace --all-targets`,** clean;
   `bin/fattorini bench` and the snapshot's `bench` the same nodes as before (565,412), proof the
   defaults and the renamed file changed nothing.

10. **Before the run (the developer):** rename the three `parameters.txt` on Drive under the lc0
    network's generation 1 to `search-parameters.txt`, and every `tune/` folder to `search-tune/`; a snapshot, `config.yml` updated; every
    arena on the commit; the smithy's bench on an idle machine.

11. **The run:** `lc0-psq-threats-screlu-g1-r1@search-tuned`, kind `time`, 32k pairs at 10+0.1; then the
    verdict, the 180+2 duel of the tuned set against the untuned (design, "Already decided").

## Not changed

The search tune's behaviour, files and results; the time formula itself; the move overhead and
`EXTRA_MARGIN_MS`; the UCI options.
