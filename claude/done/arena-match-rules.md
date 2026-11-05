# rotor-arena: match takes its rule as a flag, SPRT error rates fixed at 5%

Status: plan, agreed with the developer 2026-09-27. Supersedes the command line of
rotor-evolve-sprt-match.md (the `sprt` subcommand); the runner that plan built stays.

## The commands

```
rotor-arena gate  <network> --threads <n>          SPRT at elo_d from net.yaml, depth 9, parallel, writes verdict.txt
rotor-arena match <one> <two> --best-of <n>        best of n at 3+2, one game at a time
rotor-arena match <one> <two> --sprt <elo_d>       SPRT at elo_d, 3+2, one game at a time
```

The command says who plays and under what conditions; for `match`, the flag says when to stop.

- **`gate`** is unchanged: SPRT only, no rule flag, since a promotion has one meaning and
  `verdict.txt` always rests on the test.
- **`match`** requires exactly one of `--best-of <n>` and `--sprt <elo_d>`, each with its value.
  Neither, both, or a flag without its value prints the usage text. A bare `match <one> <two>`,
  today's best of 12, is refused: the command always says which rule it plays.
- **The `sprt` subcommand goes**, absorbed into `match --sprt`.

**Parallelism is fixed by the command, not policed by the runner.** `gate` passes its
`--threads`; `match` always passes one thread and takes no `--threads`. Fixed depth makes the
gate's result independent of wall time, so parallel games are safe there; under a clock they would
compete for cores, cache and memory bandwidth and bias the result, unequally between two different
engines. The runner could derive this from `Control`, and deliberately does not yet: two commands,
two fixed choices, is simpler for now.

## SPRT error rates: alpha = beta = 5%, hard-coded

- `sprt.rs`: `ALPHA` and `BETA`, both `0.05`, replace `DEFAULT_ALPHA`/`DEFAULT_BETA` as the values
  every run uses. `Sprt::new(elo_d, alpha, beta)` keeps its three parameters, since its tests check
  the derivation at other error rates (asymmetric bounds among them); callers pass the constants.
  `DEFAULT_ELO_D` stays for the tests only.
- `config.rs`: `SprtSpec` is `{ elo_d }`. `validate` checks `elo_d > 0` (and not NaN).
- `net.yaml`: the `sprt:` section is `elo_d` alone, as `configs/amanda.yaml` already has it. The
  current code requires `alpha` and `beta` there, so the gate cannot load amanda's config today;
  this fixes that. An `alpha` or `beta` key is refused with a message saying they are fixed at 5%,
  rather than rejected as an unknown field or silently ignored, so no config can claim error rates
  the test does not use.
- `Sprt::describe` still prints alpha and beta, so every run's log records the whole test.

## match

`run_match` takes the rule:

```rust
enum MatchRule {
    BestOf(u32),
    Sprt(f64), // elo_d
}
```

- **`--best-of <n>`**: `n >= 1`. The win threshold stays `n / 2 + 0.5`, so an odd `n` cannot end
  level and an even one ending level is a drawn match, as today. Played by `pairing::play_pairing`
  as now, with `best_of: n`.
- **`--sprt <elo_d>`**: `elo_d > 0`. Played by `gate::play_sprt` exactly as `run_sprt` does now:
  one thread, 3+2, per-move output, half pairs recorded. The test itself is applied at the end of
  each pair only, as everywhere.
- The two share one `run_match` up to the rule: player resolution, the "two different players"
  check, the dated folder, `spec.txt`, the database and the Ctrl+C handler. Only the play call and
  the `result.txt` content differ.

`spec.txt` lines:

- `best-of 12 at 3+2`, `best-of 7 at 3+2`, ...
- `sprt elo_d 35 at 3+2`

A folder without `spec.txt` predates it and is `best-of 12 at 3+2`, as now. So `--best-of 10`
will not resume a `--best-of 12` folder, nor `--sprt 20` an `--sprt 35` one. The SPRT line loses
alpha and beta; no folder with the old three-parameter line exists outside test runs of the
just-built `sprt`, and such a folder is refused as a mismatch, which is correct.

## Text

- The usage text of `rotor-arena` and the module header of `arena/main.rs`.
- README.md's match section: `match ... --best-of 12`, `match ... --sprt 35`, alpha and beta
  fixed at 5%.
- `rotor-evolve-sprt-match.md`: a note at its top that its command line is superseded here.

## Tests

- `match` arguments: `--best-of 12`, `--best-of 7`, `--sprt 35`; refused: no flag, both flags,
  `--best-of` or `--sprt` without a value, `--best-of 0`, `--sprt 0`, `--sprt x`, `--threads`.
- spec lines: `best-of 12 at 3+2`, `sprt elo_d 35 at 3+2`, and `35` and `35.0` giving the same.
- `load_sprt_spec`: `elo_d` alone loads; `alpha` or `beta` present is refused with the 5% message;
  a missing `elo_d` is refused.
- the existing `sprt.rs` derivation tests, unchanged.
