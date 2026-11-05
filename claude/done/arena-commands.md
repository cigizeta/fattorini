# rotor-arena: three decision rules, one control option, and a gate that picks a rule

## Status

Done. Implemented, tested (161 tests) and used for a first real run: `duel amanda-0001 hand-crafted
--depth 9`, +50 -21 =15 in 86 games, amanda-0001 stronger. `gate.rs` was superseded by `runner.rs`
and deleted. One addition found while implementing: an in-process player cannot keep a
clock, so under `time:` the gate plays its two networks over UCI, through
`match_player::rotor_entry`, as the other commands already did.

Decided afterwards: `--hash <MB>` on `match`, `sprt` and `duel`, 1 to 1024 (Rotor's advertised UCI
range), 16 by default always, for UCI and in-process players alike, replacing the former 64 MB over
UCI. The gate uses 16. A memory guard (`memory.rs`) refuses a run unless `hash * players` is under
half of `MemTotal` (under WSL, the RAM given to the VM), with players = 2 per pair in flight; strict
by design, no overhead allowance, and an unreadable `/proc/meminfo` refuses too.

Decided afterwards: both players are in-process or both over UCI, never one of each. In-process
only at a fixed depth with two Rotor players (`match_player::plays_in_process`); under a clock, or
with an external engine such as Stockfish, both over UCI.

## Goal

Reorganize `rotor-arena` around three decision rules, each its own subcommand, sharing one way of
saying how a game is limited. The gate stops being a rule of its own: it runs whichever rule its
network's config names.

`rotor-arena self` is out of scope: it will be redesigned completely, so it keeps its own
`--threads` and everything else as it is.

## The commands

```
rotor-arena match <p1> <p2> <n>       (--depth <d> | --time <base+inc>)    best of n
rotor-arena sprt  <p1> <p2> <elo_d>   (--depth <d> | --time <base+inc>)    is p1 at least elo_d stronger?
rotor-arena duel  <p1> <p2>           (--depth <d> | --time <base+inc>)    which is stronger? (duel.md)
rotor-arena gate  <network>                                                rule and control from the config
rotor-arena self  ...                                                      unchanged
rotor-arena <tournament-id>                                                unchanged
```

Players are named as today (`match_player::resolve_player`): `hand-crafted`, `stockfish-<level>`,
`stockfish-elo-<rating>`, `<network>-NNNN`.

This replaces `match <p1> <p2> --best-of <n>` and `match <p1> <p2> --sprt <elo_d> [--depth <d>
--threads <n>]`, and `gate <network> --threads <n>`. No backward compatibility (see Decisions).

## The control option

Exactly one of the two, with no default:

- **`--depth <d>`**, d from 1 to 9. A fixed depth per move for both players, no clock.
- **`--time <base+inc>`**, a Fischer control parsed by the existing `TimeControl::parse` (`3+2` is
  3 minutes plus 2 seconds a move).

## Threads: no option

`--threads` is removed from `match`, `sprt`, `duel` and `gate`. The number of pairs in flight
follows from the control:

- **Fischer: one game at a time.** Parallel games would compete for the CPU and distort each
  other's clocks. Each game is recorded as it ends (half pairs), so a Ctrl+C loses only the game in
  flight, as `match` does today.
- **Depth: as many pairs as the machine has logical cores**, `std::thread::available_parallelism()`.
  Timing does not matter at a fixed depth, so parallel pairs only speed the run up. Whole pairs are
  recorded, as the gate does today, so a Ctrl+C loses the pairs in flight.

This reverses a deliberate choice: `main.rs` gave the gate's `--threads` no default because the
right count is a property of the machine. Reading the logical core count is that property, so the
flag carried no information the machine does not already have.

The thread count stays out of `spec.txt`, as today: it changes how fast the games are played, not
which test they belong to, so a run can still resume on another machine.

## One runner for all three rules

`gate.rs` is today the SPRT runner behind `gate` and `match --sprt`: colour-reversed pairs, parallel
workers, half or whole pairs, resume from `results.db`, discarding pairs that finish after the
verdict, Ctrl+C. It touches the test in only a few places (`Sprt::new`, `add_game`, `verdict`,
`record`, `score`, `games`, and the `llr [lower, upper]` status text).

Plan: extract those into a trait and make the runner generic over it, rather than copying it.

```rust
pub trait SequentialTest {
    /// One game from the first player's side: 1.0, 0.5 or 0.0.
    fn add_game(&mut self, first_score: f64);
    /// `None` while undecided.
    fn outcome(&self) -> Option<Outcome>;
    fn record(&self) -> (u64, u64, u64);
    fn score(&self) -> f64;
    fn games(&self) -> u64;
    /// The test's own state after each pair: `llr +1.234 [-2.944, +2.944]`, `S 0.61 > 0.609`, `6.5-5.5`.
    fn status(&self) -> String;
    /// Printed when a run starts, so its log records which test produced the verdict.
    fn describe(&self) -> String;
}

pub enum Outcome { FirstWins, SecondWins, Drawn }
```

Each rule maps its own verdicts onto `Outcome`:

| Rule | FirstWins | SecondWins | Drawn |
|---|---|---|---|
| match, best of n | first past n/2 | second past n/2 | n/2 each after n games |
| sprt | H1: at least elo_d stronger | H0: not | never |
| duel | S past the threshold | 1 - S past it | 1000 games, no winner |

- `sprt.rs` keeps its derivation untouched; `Sprt` implements the trait on top of it.
- `duel.rs` is new: half-point score, win/loss/draw record, `C = 3.09`, `MAX_GAMES = 1000`.
- The best of n becomes a third implementation, `BestOf`, so `match` gets the parallel depth mode
  and the same resume logic. `pairing.rs` keeps `play_pairing` for tournaments, which need its
  tie-break pairs.
- `gate.rs` is renamed to `runner.rs` (it no longer belongs to the gate), `SprtRun` to `Run`,
  `play_sprt` to `play`, `SprtLabels` to `Labels` with one line per `Outcome`.

## The gate

A gate is one of the three rules, run in a fixed setting:

- **Players** derived from the network: the current generation's `challenger.nnue` against the
  previous champion, or the hand-crafted evaluation for `gen-0001`, as today.
- **Record** in the generation's directory, resumed on a rerun, as today.
- **Verdict** in `verdict.txt`, `pass` or `fail`, which `rotor-train promote` reads, as today.

The rule and the control come from the network's config, a `gate:` section replacing `sprt:`, so
every generation of a lineage is judged the same way and a rerun cannot pick another rule:

```yaml
gate:
  rule: duel        # duel, needs nothing else
  depth: 9          # or time: 3+2
```

```yaml
gate:
  rule: sprt
  elo_d: 35.0
  depth: 9
```

```yaml
gate:
  rule: match
  best_of: 12
  depth: 9
```

Validation, each refused with a message: an unknown rule; `elo_d` without `sprt` or `best_of`
without `match`, or either missing where required; both or neither of `depth` and `time`; a depth
outside 1 to 9; an odd `best_of`; a missing `gate:` section.

Pass and fail, the challenger always the first player:

| Rule | pass | fail |
|---|---|---|
| match | the challenger wins the match | it loses it, or the match is drawn |
| sprt | H1 | H0 |
| duel | the challenger wins | the champion wins, or the duel is drawn at 1000 games |

A draw never promotes.

Config changes: `configs/betty.yaml` already has `gate: {rule: duel, depth: 9}`.
`configs/amanda.yaml`'s `sprt: {elo_d: 35.0}` becomes `gate: {rule: sprt, elo_d: 35.0, depth: 9}`.

## Recording

`match`, `sprt` and `duel` all record in `data/matches/<p1>-vs-<p2>/<YYYYMMDD-HHMM>/`, with
`spec.txt`, `results.db`, `games.pgn` and `result.txt` as `match` does today. Command-line order is
kept, as today; the folder names p1 first.

`spec.txt` names the rule and the control, so a rerun with either changed is refused rather than
resumed:

```
best-of 12 at 3+2
sprt elo_d 35 at depth 9
duel at depth 9
```

## Tests

- Argument parsing: each command's positional arguments; exactly one of `--depth`/`--time`; depth
  1 to 9; `--threads` refused; an odd n refused.
- `spec_line` for every rule and control.
- `Duel`: no winner before N = 10; a 10-0 wins; the threshold at N = 100 and 1000; symmetry; drawn
  at 1000; the seeded simulation of equal engines at or below 5% false winners (`duel.md`).
- `BestOf`: first past n/2 wins, a level n/n drawn, a lead that cannot be caught ends early.
- `Sprt` through the trait: the existing tests unchanged.
- Gate config: every validation above, and `configs/betty.yaml`/`amanda.yaml` loading.
- The runner's existing replay, half-pair and discard tests, rewritten over the trait.

## Decisions

1. **Never an odd N.** Results arrive in pairs, so a best of n requires an even n, refused
   otherwise, in the command and in the gate config alike.
2. **The gate under a clock** is allowed: a config that wants one says `time:`, as it says
   `depth:` for a fixed depth. The fixed-depth argument of `rotor-network-gate.md` is a reason to
   choose `depth:`, not a restriction the code enforces.
3. **No backward compatibility.** Old command spellings, old `spec.txt` lines, the `sprt:` config
   section and unfinished old match folders are not handled: the old flags are simply unknown, and
   the old section simply unknown to the gate.
