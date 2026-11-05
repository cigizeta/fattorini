# Smoke tests by a scale factor

**Archived 2026-11-04: superseded, see smithy-anywhere.md. Scaled runs from the page replaced
`smithy smoke` and the smoke tree.**

**Status: plan confirmed and implemented, 2026-10-22; DEV.md done; every test passed; `smithy smoke pesto-psq` ran end to end on the Lichess machine (2026-10-22: Play, train,
round robin, gate failed at 1/30 as expected at smoke size, the run stopped). To do: `smithy smoke
lc0-psq`; the closing message of a failed generation 1 says "the network is wrong", to soften in a
smoke test. Beyond the
plan: the smoke tree is set once per process (`layout.rs`), its Drive a local stand-in,
`data/smoke/s<s>/drive`; configs are scaled where they load, the frozen `config.yaml` staying real;
tunes are not scaled (a smoke test never tunes); `labels: self` is gone. Open: `MIN_CAP_BYTES`
(256 MiB) to check against a real smoke Convert; `check`, `calibrate`, `correlate` do not read smoke
runs.** No
code touched. Follows sources.md.

## Decisions (the developer, 2026-10-22)

1. **`smoke.yaml` goes.** A smoke test is any of the four configs (`lc0`, `pesto`, `lc0-psq`,
   `pesto-psq`) scaled down.
2. **Command line only, for debugging:** never on the page, and its runs never listed there.
3. **One scale factor, not hard-coded smoke values** (hard-coded constants are bad style): every size
   follows from the config and the factor, `--scale 1` the real run exactly.
4. **The factor scales each key by the kind of quantity it is, down to a floor** (Claude's proposal,
   accepted):
   - **Data volumes, by 1/s:** `training_positions`, `test_positions` (forges' and `pesto`'s), `lc0`'s
     `cap_bytes`; floored at one batch's worth of rounds, and at one tar for Lc0.
   - **Work a round, by 1/s:** `steps_per_round` (floor 1), `training_batch_size`,
     `test_batch_size` (floor a few hundred).
   - **Shape, fixed:** the 9 lambdas and the grid, `warmup_rounds`, `patience_rounds`, the
     learning-rate stages, `gate_elo_d`, `max_failed_gates`: every stage, early stop and gate rule
     still runs.
   - **Game counts, by sqrt(s):** `round_robin_pairs`, a tune's pairs (floor the tune's 100).
   - **Search effort, by log:** depth minus log2(s) / 2, floor 2 (a ply about doubles the cost); a
     tune's clock by 1/s, floored where the engine can still move.

5. **One command does everything:** `smithy smoke <config> --scale <s>` starts a smithy without the
   page's controls, makes the scaled source when missing, runs the scaled forge to its end, and
   exits; arenas started with `arena start` join it as usual. `smithy smoke pesto --scale <s>` (or
   `lc0`) makes the scaled source alone.
6. **A separate local tree, `data/smoke/`** (Claude's, not objected to): its own `gen/`, `pesto/`,
   `lc0/`, `buckets/`, never moved to `G:`, never on the page; deleting it resets every smoke test. A
   scaled source is made once per factor and reused.

7. **`--scale` defaults to 1024:** `smithy smoke pesto-psq` alone, about 98K training positions, 13
   round robin pairs, depth 4, minutes a phase on the Lichess machine's CPU; `--scale 1` the real config.

8. **No page for a smoke test:** development only, the terminal is enough. A smoke smithy serves the
   arenas alone: no status page, no job controls.

## Plan

1. **The scale:** a function from a config and `s` to its scaled copy, each key by its kind
   (decision 4), with its floors; for the forge configs (`GenConfig`, `NetConfig`), `pesto.yaml`,
   `lc0.yaml` (its cap), the tune's length and control, and the self-play depth (today the constant
   `generation::SEARCH_DEPTH`, then carried by the chore). Unit tests: `s = 1` returns the config
   unchanged; every floor holds at a huge `s`.
2. **The smoke tree:** every path of a smoke test under `data/smoke/` (runs, sources, buckets, frozen
   sets), local, never mapped to `G:`; a root passed down rather than a global switch, so a real
   smithy and a smoke test can never mix.
3. **`smithy smoke <config> [--scale s]`:** serves the arenas without the page or the controls; makes
   the scaled source when `data/smoke/` lacks it at that `s` (Lc0: a scaled Convert, its cap at
   least one tar; PeSTO: a scaled Play); then, for a forge, a new scaled run to its end; then exits.
   Stop by Ctrl+C, resumable by the same command.
4. **`smoke.yaml` goes** (`git rm`, the developer), and with it `smoke`'s special case: every forge
   needs a source now.
5. **Docs:** DEV.md (a smoke test), TODO.md.
6. Build, test; on the Lichess machine `smithy smoke pesto-psq` to its end, then `smithy smoke lc0-psq`.
