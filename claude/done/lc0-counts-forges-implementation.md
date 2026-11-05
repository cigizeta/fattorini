# The forges on data/lc0-counts: implementation plan

Status: written 2026-11-02; steps 1 and 2 applied the same day, to test. As built: `lc0::LC0_ROOT`
is `preprocessing_config::LC0_ROOT`; `converted_complete` folded into `is_complete`; `data/lc0/frozen`
is no longer on the remote (found 2026-11-02; only the refused `frozen-with-final` is), so the
ignored test draws the set with `sample` from both corpora's test games, nothing written, and
compares the segments, the counts, and each slot's position (as `cache` writes it) and labels at
lambdas 0.01, 0.05 and 1.0. The freeze is deterministic, so equal draws stand for equal sets.
Step 4 done the same day: 8 segments, 66,542 games, all 4,194,304 positions and their labels equal
(64 s). The new corpus freezes the old test set: every old network's test loss compares.
Step 7 done the same day, and `archived/configs/emily.yaml` written ahead of step 3. Implements lc0-counts-forges.md. Each step with its tests; the
developer runs them and commits between steps. Steps 1 and 2 are code, applied any time; steps 3 to
6 are the developer's, by hand, after today's experiment (`lc0-psq-threats-screlu`'s generation 2
and its gate).

## Findings that shape the plan (2026-11-02, from the code)

- **A network player is found by its path alone.** `players::resolve_network_file_under` parses
  `<config>-g<g>-r<n>` and reads `data/networks/<config>/run-<n>/` (here, then on the remote); no
  config file is read. So moving `data/networks/lc0-psq-threats-screlu/` to `data/networks/emily/`
  makes the old winners play as `emily-g1-r1`, `emily-g2-r1`, with no code change.
- **The move frees the name.** With the old directory gone, `lc0-psq-threats-screlu`'s next run is
  `run-1` again (`runs_of` finds none), and nothing the new run writes can collide with the old.
- **`emily.yaml` cannot sit in `configs/`.** The config list (`smithy.rs`, `read_dir("configs")`)
  takes every `.yaml` there as a forge, and `check_forge_name` refuses a name without its source's
  prefix. It goes to `archived/configs/emily.yaml`, after amanda, betty, caroline and dorothy, their
  header's form: what she is, what she trained on.
- **The switch's sites:** `preprocessing_config::LC0_ROOT` (`data/lc0`); `lc0::TRAINED_COMPLETE_PATH`
  and `lc0::is_complete`; `engine_loss.rs`' own `LC0_ROOT` and `FROZEN_DIR`.

## Steps

### 1. The switch (code)

- `preprocessing_config::LC0_ROOT = "data/lc0-counts"`; its test's expected path follows.
- `lc0::is_complete` reads `COMPLETE_PATH`; `TRAINED_COMPLETE_PATH` removed; `lc0::LC0_ROOT` and
  `preprocessing_config::LC0_ROOT` now agree, the Convert's doc comment saying so (no longer "the
  forges still read the old one").
- `engine_loss.rs`: its `LC0_ROOT` and `FROZEN_DIR` replaced by `preprocessing_config::LC0_ROOT` and
  the frozen directory derived from it as every source's is.
- No other behaviour change: the buckets rescatter on the first run by themselves.

### 2. The frozen sets compared (an ignored test)

- In `freeze.rs`' tests, `the_counts_corpus_freezes_the_old_test_set` (`#[ignore]`): freezes the
  new corpus's test set into a temporary directory (`TestSet::of`'s path, at a few lambdas), then
  compares `test.positions` and each lambda's `test.labels` byte for byte with
  `data/lc0/frozen`'s on the remote. Run by hand once, before the old corpus is deleted.

### 3. The old run becomes emily (the developer)

After generation 2's gate, before any new run:

- `data/networks/lc0-psq-threats-screlu/` moved to `data/networks/emily/`, on this machine and on
  the remote.
- `configs/lc0-psq-threats-screlu.yaml` copied to `archived/configs/emily.yaml`, its header as
  dorothy's: psq_threats_screlu on the old Lc0 corpus (275 tars, about 1.0 billion positions), kept
  as trained, a historical record. The original stays as the new run's config, unchanged.
- A check: `emily-g1-r1` and `emily-g2-r1` play (a short duel or `smithy install`).

### 4. The frozen sets checked (the developer)

The ignored test of step 2 run. Equal: TODO.md says the old test losses stay comparable. Not
equal: the difference found before step 5.

### 5. Step 0 (the developer)

- `lc0-psq-threats-screlu`, New run on the page (its `run-1` on the new corpus); the scatter's time
  noted in lc0-counts-forges.md.
- Its winner duelled against `emily`'s last winner, both `@default`, depth 9 and 10+0.1; the
  result in TODO.md. Then `lc0-halfk-threats-screlu` against step 0's winner.

### 6. Stale data deleted (the developer)

After step 4: `data/lc0` here and on Drive, and the old buckets if the rescatter left any. `emily`'s
run directory stays.

### 7. Docs

TODO.md: the corpus statement corrected (not a superset: about 48 old tars missing, 135 later added),
the halfk line's step 0 pointing here, `emily` recorded. DEV.md if a command changes.

## Open

- Whether game records or ratings files outside `data/networks/` name the old run's players
  (`lc0-psq-threats-screlu-g1-r1`); if so, they keep the old name, as history, and only new games
  say `emily`. To check at step 3.

## Commands for the developer

After steps 1 and 2:

```bash
cargo test --workspace --release
```

```bash
cargo clippy --workspace --all-targets --release -- -D warnings
```

Step 4:

```bash
cargo test --release -p smithy the_counts_corpus_freezes_the_old_test_set -- --ignored
```
