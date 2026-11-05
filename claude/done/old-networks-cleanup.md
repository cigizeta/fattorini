# Removing the older networks and other legacy

**Status: plan, 2026-10-22, from the developer's request after configs-with-tunes.md; confirmed and
implemented the same day, tests passed. Beyond the plan: `label:` went whole (nothing read it once
the forge used its grid), refused as an unknown forge key with `preprocessing:`, `data:` and
`network:`; arena's `compression.rs` went with `replay`; `nnue_bench`'s `--network` takes a forge
player, and its `--eval-scale`, `--quantization` and `--i8` need a games directory. The bench
compiles (`cargo bench --no-run`), its reference encoder now its own zstd writer, arena's
`compression.rs` gone; the Lichess machine check (step 3) to do.** No backward compatibility: training restarts from scratch (forge-configs.md,
decision 6).

## Decisions (the developer, 2026-10-22, one question at a time)

1. **Every code path serving only `data/networks/`** goes: amanda to dorothy, `lc0-*-0001`, their
   champions, selections and Drive promotion. A network player is a forge run's, and only that.
2. **The other legacy goes too** (all four groups, the recommendation approved):
   - the forge's old-run leftovers: `generations.tsv`'s `-` as passed, the `corpus/` to `train/`
     rename, the `corpus_positions` alias, the `orders.txt` fallback, the refusal of a run at a
     config's top;
   - the refusals of retired config keys (`preprocessing.lambda`, `preprocessing.parent`,
     `preprocessing.pgn`, `depth`, `label.blend`, `label.cut`, `label.cut_drop`, `data:`,
     `labels: archives`): unknown keys from now on, refused as any unknown key is where the struct
     denies them; the tests reading `archived/configs/` for them go;
   - the `data/self` self-play source branch, unreachable (every caller passes Lc0's), with
     `LINEAGE_ROOT` (folded into `PESTO`) and `data/server.yaml`'s fallback;
   - DEV.md's old firewall rules, README.md's description of amanda-0002 as today's champion.
3. **`arena margin` goes:** its matches lived in `data/networks/`, and the SPSA tune is the search
   parameters' tuning now.
4. **`arena replay` goes:** labelling any PGN with a player's search would be an analysis feature,
   for another, separate project (TODO.md).

## What goes, by crate (from the survey, 2026-10-22)

- **rotor:**
  - `generation.rs`: `network_dir`, `generation_dir`, `PREFIX`, the champion and selection helpers,
    `LINEAGE_ROOT`, their tests; kept: `lambda_dir`, `candidate_network_path`, `record_field`,
    `SEARCH_DEPTH`.
  - `generation_sync.rs` whole; `candidate_record_path`, which the forge's `candidates.rs` uses,
    moves into `generation.rs` first.
  - `players.rs`: `local_network_path`, the older-network branch of `resolve_network_file_in` (left
    with a run directory alone) and of `RotorPlayer::resolve` (`selection/spsa`), `run_winner`'s `-`,
    their tests and docs.
  - `preprocessing_config.rs`: the retired keys' refusals, `Labels::Archives`, the `data/self`
    branch (`self_play_*_dir` become Lc0's), the archived-config tests, the docs naming amanda and
    `data/networks`; `RESERVED_TEST_NETWORK` if no test still needs it.
  - `net_config.rs`: the retired keys' refusals.
  - `protocol.rs`: `data/server.yaml`'s fallback.
  - `remote.rs`: `copy_files` private if nothing outside uses it; docs and test strings.
  - `nnue_bench.rs`: `champion_path`, `--network <network>-NNNN`, `EVAL_SCALE_DEFAULT_DIR`, doc
    examples; a forge player through `players::resolve_network_file` where the bench needs a network.
- **arena:**
  - `match_player.rs`: `Drive`, `network_path`, `pull_promotion`, the network-directory and Drive
    parameters, the "both a run and a network" refusal, their tests.
  - `replay.rs` and `margin_search.rs` whole, with their commands, usage and modules in `main.rs`.
- **smithy:**
  - `install.rs`: the champion kind; installs a forge player's candidate.
  - `smithy.rs`: `old_train`, `rename_old_training_sets`, the alias, the `orders.txt` fallback, the
    old-layout refusal, `run_winners`' `-`, the stale `network:` doc, their tests.
  - `main.rs`, `tune.rs`: usage and docs.
- **Docs:** DEV.md (Drive's champions, the older networks' paragraph, "Labelling any PGN", the
  player list and examples, `install` of an older network, `server.yaml`, the old firewall rules),
  README.md (today's champion and its history, rewritten as the restart from scratch), TODO.md.

## Steps

1. rotor, then arena, then smithy, as above; the workspace denies `dead_code` and `unused`, so every
   helper left without a caller goes too.
2. Docs.
3. Build and test; on the Lichess machine, `arena duel` between `pesto` and a forge player, `smithy install` of
   one.
