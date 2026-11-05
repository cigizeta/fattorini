# SPSA: implementation plan

Implementation plan, 2026-10-18, drafted for the developer to review. Implements spsa.md's decisions
of 2026-10-16 (the method, the budget, the ten parameters, their ranges and steps) on the search's
`SearchParameters` (search-parameters.md, done), with early-stop.md's adjudication. Nothing here
changes how a network searches until its tune has written `parameters.txt`.

## What spsa.md settled, restated in one place

- **Ten parameters,** `SearchParameters`' fields, their starts today's defaults, each with its range
  and `c_end` (spsa.md's table). theta is kept as real numbers; a player rounds each value to the
  nearest integer when it plays; every value is clipped to its range after each update.
- **Fishtest's schedule:** `c_k = c / (k + 1)^0.101`, `a_k = a / (A + k + 1)^0.602`, `A = N / 10`,
  `c = c_end * N^0.101`, `a_end = r_end * c_end^2`, `a = a_end * (A + N)^0.602`. One `r_end` for all,
  0.005. Each parameter has its own `c` and `a`, from its own `c_end`.
- **N = 2,500 pairs** per network, at STC (10+0.1), on the training machine.
- **One Delta per pair,** a random sign per parameter; the pair is theta + c_k Delta against
  theta - c_k Delta, colours reversed on one opening. Its result `r = wins - losses` from the plus
  side (-2 to 2) moves each parameter by `a_k / c_k * r * Delta_i` (fishtest's
  `R_k * c_k * r * flip` with `R_k = a_k / c_k^2`).
- **M pairs in flight,** M = the STC placement (half the logical cores, pinned): theta updated as
  each pair finishes, the next pair started at once from the current theta.
- **Games end early** (early-stop.md), turned on by SPSA's command and by nothing else.
- **After the tune, a duel at STC, tuned against untuned,** games played out: the tuned values stay
  unless the untuned win.

## Design

### 1. `parameters.txt` beside the network

- **Format:** ten lines, `<field name>\t<integer>`, in `SearchParameters`' order, every field
  required, nothing else. A missing, unknown, repeated or non-integer line is an error naming the
  file, as `margin.txt`'s is today. Written atomically (`.tmp`, then a rename).
- **Read at load,** beside the network file, as `margin.txt` is: the network holds
  `Option<SearchParameters>` in place of `margin_per_ply: Option<i32>`.
- **`margin.txt` still read** when `parameters.txt` is absent: its margin becomes
  `SearchParameters::with_margin(m)`, exactly today's search. With both present, `parameters.txt`
  wins. No existing network changes.
- `search::default_parameters_for(evaluator)` returns the network's parameters, else `DEFAULT`.
- `uci.rs`'s load line names where the parameters came from: `parameters.txt`, `margin.txt`, or the
  defaults.
- **Release:** the parameters are what a release embeds beside the network, in place of the margin
  (tuned-margin-storage.md's plan, unchanged otherwise).

### 2. In-process players by parameters

- `InProcessPlayer` holds `Option<SearchParameters>` in place of `margin_per_ply`, and calls
  `choose_best_move_with_progress` with it; `None` keeps the network's own.
- `set_parameters(&mut self, parameters: SearchParameters)`: SPSA's workers change both players'
  values between pairs, keeping their tables (cleared by `new_game` as now) and the shared network.
- **Player names:** `@<cp>` stays (`with_margin(cp)`); a new suffix **`@default`** plays
  `SearchParameters::DEFAULT`, ignoring the network's file. It serves the check duel (tuned against
  `<player>@default`) and any untuned comparison.

### 3. Early stop

early-stop.md as planned there, unchanged: the runner's `early_stop` setting, `game.rs`'s
`Adjudicator`, the three `Termination`s, `spec_line`'s suffix. SPSA is its one user.

### 4. `spsa.rs`: the method, pure

No games, no files: unit tested on its own.

- `Spec { name, start, min, max, c_end }` for the ten, from spsa.md's table, in `SearchParameters`'
  order; `SearchParameters::from_values(&[i32; 10])` and `values()` to cross between the two.
- `Schedule::new(n, r_end, &specs)`: `A`, and each parameter's `c` and `a`.
- `c_k(i, k)` and `a_k(i, k)`.
- `Delta::draw(seed, k)`: ten signs from a generator seeded by the run's seed and `k`, so a pair's
  direction is reproducible from its index alone.
- `point(theta, k, delta, sign)`: `theta + sign * c_k * delta`, clipped, rounded, as
  `SearchParameters`.
- `update(theta, k, delta, r)`: `theta_i += a_k / c_k * r * delta_i`, then clipped.

### 5. `arena spsa <player>`

- **The run's folder:** `gen-NNNN/selection/spsa/` for a generation's network, beside
  `selection/margin/`; `data/spsa/hand-crafted/` for hand-crafted, as `arena margin` does.
- **`spec.txt`:** the player, N, `r_end`, the seed, the control and the ten specs, written at the
  first start; a later start that disagrees is refused, as a changed control is today.
- **`state.tsv`:** `k_done` (pairs whose result is in theta) and the ten theta values, rewritten
  atomically after every pair. **`pairs.tsv`:** one appended row per finished pair, its `k`, Delta,
  result and theta after: the run's history, for plotting each parameter's path.
- **Workers:** M, pinned as the STC placement. Each starts two `InProcessPlayer`s sharing one
  `Arc<Network>`, then loops: take the next `k` (one past the highest handed out), read theta, draw
  Delta, set the plus and minus players' parameters, play the pair with `runner::play_games` and
  early stop on, from a random book opening, then report `(k, delta, r)`. The main thread applies the
  update under a lock and rewrites `state.tsv`.
- **Order of updates:** as pairs finish, not by `k`; `c_k` and `a_k` use the pair's own `k`, drawn
  when it started. Fishtest behaves the same way across its workers.
- **The end:** when N results are in, `parameters.txt` (theta rounded) is written beside the
  network, and the run prints the ten values against their starts.
- **Resume:** theta and `k_done` from `state.tsv`; pairs in flight at an interrupt are lost (their
  games are not recorded), and new pairs start from `k_done`, so their `k`s are drawn again. Two-stage
  Ctrl+C as the other commands.
- **Games:** appended to `games.pgn` in the folder, the players named `<player>+` and `<player>-`,
  for inspection only; nothing reads them back.

### 6. `arena select`

The margin search before the gate becomes the tune: round robin, then `arena spsa` on its winner,
then the check duel `<candidate>` against `<candidate>@default` at STC, games played out. If
`@default` wins, `parameters.txt` is rewritten with `DEFAULT`, so the gate plays untuned values;
a tuned win or a drawn duel keeps the tune. Then the gate, as now.

`arena margin` stays as a command for now; nothing calls it. To delete once SPSA has tuned a few
networks.

## Tests

- `parameters.txt`: a round trip; each malformed form refused; `margin.txt` alone gives
  `with_margin`; both present, `parameters.txt` wins; neither, `DEFAULT`.
- `@default` and `@<cp>` resolve to the expected parameters; `set_parameters` takes effect on the
  next move.
- `spsa.rs`: the schedule's `c_k` at `k = N - 1` equals `c_end` and `a_k` equals `a_end`, for each
  parameter; `Delta::draw` is deterministic per `(seed, k)` and balanced over many `k`s; `point`
  clips and rounds; `update` moves along Delta with `r`, is zero for `r = 0`, and clips at the range.
- A short end-to-end run: `arena spsa hand-crafted` with N = 4 at a fixed depth of 2, early stop
  on, in a temporary folder: four rows in `pairs.tsv`, `state.tsv` at `k_done = 4`, a
  `parameters.txt` that loads; then an interrupt after two pairs resumes to four.
- Early stop's own tests, as early-stop.md lists.

## Steps

1. `parameters.txt` and the network's `Option<SearchParameters>`; `margin.txt` as the fallback.
   Done 2026-10-18 (`Network::tuning`, `nnue::read_tuning_beside`), tests passing.
2. `InProcessPlayer` by parameters, `@default`, hand-crafted's tuning read from
   `data/spsa/hand-crafted/`. Done 2026-10-18, tests passing. `set_parameters` moved to step 5: the
   arena denies dead code, and SPSA's workers are its first caller.
3. Early stop (early-stop.md): the three rules in `game.rs` (`Adjudicator`, `tablebase_result`,
   `play_one_game`'s `early_stop: Option<&SyzygyTablebase>`), the three `Termination`s. Both callers
   pass `None`; the runner's setting and `spec_line`'s suffix wait for step 5, their one user.
4. `spsa.rs` (`crates/arena/src/`): `SPECS`, `Schedule`, `draw_delta` (splitmix64 over seed and
   `k`), `seed_of` (FNV-1a of the player's name), `point`, `update`, `rounded`. Under
   `#[allow(dead_code)]` in `main.rs` until step 5 calls it.
5. `arena spsa` (`spsa_command.rs`): `set_parameters` back on `InProcessPlayer`; `runner::play_games`
   takes `early_stop`; Ctrl+C is the stop flag the games watch, with no mirror; no runner setting or
   `spec_line` suffix, since the run has its own `spec.txt`. The resume test is a restart of a
   finished run (nothing replayed), not an interrupt mid-run.
6. `arena select` with the tune and the check duel in place of the margin search. **Gated by the
   comparison below** (the developer, 2026-10-18): SPSA replaces the margin search only once shown
   better.

## The comparison before step 6

**On `lc0-psq-0001`** (the developer, 2026-10-18), once forged and selected: `arena margin`, then
`arena spsa`, then the duel `lc0-psq-0001@<cp>` against `lc0-psq-0001`. Its 768 duel against
`lc0-psq768-0001` names both `@default`, so neither file touches it.

One network already margin-tuned (a `margin.txt` beside it), tuned again by `arena spsa`, then a
duel at STC, games played out: `<network>` (its new `parameters.txt`, which wins over `margin.txt`)
against `<network>@<cp>`, the margin search's result. SPSA goes into `arena select` if it wins; a
drawn duel keeps the margin search, which costs about 80 minutes against SPSA's 4 hours. While the
tune's `parameters.txt` sits beside the network, every player of that network without a suffix
plays it, so the comparison runs on a network no gate or tournament is using.

Steps 1 and 2 are small and useful at once: the 768 against 780 duel can name `@default` players to
say outright that neither is tuned. Steps 3 to 5 are the bulk; 6 is a few lines once 5 runs.

## Decided (the developer, 2026-10-18)

- **The seed is per network:** derived from the player's name (a fixed hash of it), written to
  `spec.txt`. The same network always draws the same Deltas, so a rerun from scratch repeats the run.
- **Hand-crafted is tuned too,** like any network: `arena spsa hand-crafted` runs in
  `data/spsa/hand-crafted/` and writes its `parameters.txt` there. The builtin in-process player reads
  that file when present (else `DEFAULT`), so gates against hand-crafted play its tuned values, and
  `hand-crafted@default` plays the untuned ones. The UCI engine without a network keeps the
  compiled-in `DEFAULT`; copying the tuned values into the source is a separate, later choice.
