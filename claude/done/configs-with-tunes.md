# Configs with their tunes, and pesto

**Status: plan, 2026-10-22, from the developer's redesign of `configs/` after forge-configs.md;
confirmed, implemented the same day, tests passed. Steps 1 to 6 done; step 7, the Lichess machine check,
to do.**

## Decisions (the developer, 2026-10-22, one question at a time)

1. **`configs/` holds five files:**
   - three forge configs: `smoke.yaml` (was `smoke-psq.yaml`), `self-psq.yaml` and `lc0-psq.yaml`;
   - `pesto.yaml`, the hand-crafted evaluation's config;
   - `lc0.yaml`, `smithy lc0`'s conversion of Lc0's games, as now.
2. **Each forge config has a `tune:` section,** the tune of every generation of its runs; the tune
   configs (`tune-first.yaml`, `tune-smoke.yaml`) go.
3. **No `self.yaml`:** self-play has no conversion step to configure; its fixed settings (depth 9)
   stay constants until something asks them to vary.
4. **`pesto.yaml` holds `tune:` alone** for now, the hand-crafted evaluation's tune; its own
   settings would go there if they ever become configurable.
5. **`hand-crafted` is renamed `pesto`, everywhere, with no backward compatibility:** `hand-crafted`
   is refused as a player, and nothing reads the old name. Old PGNs and logs keep it, as history.
6. **A run's tune comes from its frozen `config.yaml`,** as its forge keys do: editing `configs/`
   changes only new runs. A run frozen without `tune:` cannot be tuned. `pesto.yaml` has no runs,
   so its `tune:` is read from `configs/`.

## The files

A forge config, `tune:` written as a tune config was:

```yaml
grid_center: 0.95
training_positions: ...
test_positions: ...
round_robin_pairs: ...
gate_elo_d: ...
max_failed_gates: ...
tune:
  length: first          # or {pairs: n}, or {fine: <player>}
  control: 10+0.1        # or {depth: d}
labels: self
architecture: {...}
training: {...}
```

`smoke.yaml`'s `tune:` is `tune-smoke.yaml`'s; `self-psq.yaml`'s, `lc0-psq.yaml`'s and
`pesto.yaml`'s are `tune-first.yaml`'s.

## Steps

1. **Configs:** `smoke-psq.yaml` renamed `smoke.yaml` (`git mv`, the developer); `tune:` added to the
   three; `pesto.yaml` written; the two tune configs deleted (`git rm`, the developer).
2. **The forge config reads `tune:`:** `GenConfig` takes it, required, checked as `TuneArgs` checks
   it. A file is a forge config as before (`architecture:` and `labels:`); `pesto.yaml` is not one.
3. **A tune's job is `--tune <player>`:** its config found from the player, the run's frozen
   `config.yaml` for a network, `configs/pesto.yaml` for `pesto`. `tune::config_args` reads a `tune:`
   section from one of those files; `CONFIG_PREFIX` and the tune configs' listing go. The page's Tune
   loses its tune config dropdown.
4. **The rename:** `HAND_CRAFTED` becomes `PESTO`, `"pesto"`, in rotor, arena and smithy;
   `HAND_CRAFTED_TUNING_DIR` becomes `data/spsa/pesto`; every message, test and comment naming the
   player says `pesto`; prose naming the evaluation says "the PeSTO evaluation" where it named the
   hand-crafted one. `examples/engines.yaml`, README.md and DEV.md follow. `claude/done/` and
   `claude/archived/` stay as written; `claude/active/` and TODO.md follow where they name the
   player.
5. **On disk, by hand (the developer):** `data/spsa/hand-crafted`, `data/margins/hand-crafted`
   and `data/self/hand-crafted` (self-play sources, `generation::LINEAGE_ROOT`) moved to `.../pesto`,
   on every machine that has them; `data/gen/smoke-psq` deleted or moved away (its runs froze no
   `tune:`).
6. **Docs:** DEV.md, forge-configs.md's status, TODO.md.
7. Build, test, then on the Lichess machine: a `smoke` new run to its end (it is expected to fail its gates
   against PeSTO, ending with no Tune); `pesto` tuned at smoke size, `pesto.yaml`'s `tune:` set to
   `smoke`'s for the check and restored after, `data/spsa/pesto` moved away; a passed generation's
   tune waits for the training machine's first real run (the developer, 2026-10-22).
