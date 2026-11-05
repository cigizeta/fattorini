# Round-robin selection: the lambdas play a round robin, the winner plays the champion

**Done:** candidates are chosen by a depth-9 round robin in Berger order (`round_robin.rs::berger_order`), then the winner is gated against the champion; replaces ladder.md and ladder-tie-break.md. Run on amanda and betty; results in TODO.md.

## Status

Designed with the developer, 2026-10-02. Implemented; `rotor-arena` and `rotor-train` tests pass.
Not yet run on real candidates. Replaces the ladder's chain of duels
(ladder.md) and its tie-break (ladder-tie-break.md); the gate is kept as it is.

## Problem

The ladder plays the lambdas in grid order, each newcomer against the holder, and picks one winner.
It ranks nothing, and it is biased by its order: a duel between engines less than about 34 Elo apart
usually reaches its 1000-game cap (duel.md), neighbouring lambdas plausibly sit that close, and the
holder is always the earlier lambda. The tie-break by wins (ladder-tie-break.md) removed the bias
for drawn duels but still ranks nothing.

What the selection should answer, across generations: where the best lambda is, whether it sits at
an edge of the grid (shift the grid), and whether strength depends on lambda at all (shrink it).
That needs every candidate's strength, not one winner.

## Decided

### Config

`configs/<name>.yaml`'s `ladder:` section becomes `selection:`, the same in amanda and betty:

```yaml
selection:
  depth: 9
  # Colour-reversed game pairs between every two candidates: 100 pairs, 200 games, resolve gaps of
  # about 25 Elo; closer lambdas are interchangeable.
  round_robin_pairs: 100
  gate:
    rule: duel
```

- `depth`: one control for both phases, so the challenger is chosen and gated under the same one.
  Fixed depth, so every core can play a game at once without weakening any player, which a clock
  would not allow. `time` is refused.
- `round_robin_pairs`: counted in pairs, not games, so an odd number of games cannot be asked for.
  At least 1.
- `gate.rule`: `duel`, `sprt` or `match` with the fields each needs (`elo_d`, `best_of`), exactly as
  the ladder's `rule` today.
- A config with a `ladder:` section is refused with a message naming `selection:`, as `gate:` was
  refused when it became `ladder:`. Unknown keys are refused.

### Choosing N

Per game, a standard deviation of about 0.4 (roughly 35% draws between close engines). With k
candidates each plays `2 (k - 1) N` games; the standard error of the difference between two
candidates' average scores is then about `sqrt(0.064 / (2N))` with N in pairs, and near 50% one
point of score percentage is about 7 Elo. At 95%:

| Gap resolved | Pairs per pairing | Games, 5 lambdas | Games, 6 lambdas |
|---|---|---|---|
| ~30 Elo | 70 | 1,400 | 2,100 |
| ~25 Elo | **100** | **2,000** | **3,000** |
| ~20 Elo | 150 | 3,000 | 4,500 |

The cost grows as k^2: keep the grid tight.

### Openings

As today: every game pair draws a random opening from the book when it starts, both its games play
it with colours reversed, and nothing tracks which openings were used. A repeat across pairs is
possible and accepted (a few thousand draws from 2.6M positions).

### Ranking

By points: each candidate's total score over all its round-robin games. In a balanced round robin
(every pairing the same number of games) this is the same order as a maximum-likelihood Elo fit
(Bradley-Terry): at the fit, each player's points equal its expected points, which increase with
its rating. So no fit is needed. The balance is what makes points valid, so no ranking is written
until every pairing has its full `round_robin_pairs`.

A tie at the top on points: the head-to-head score between the tied candidates decides, then the
earlier lambda in `label.lambda`.

The standings also report how far apart candidates are relative to noise: each candidate's score
percentage with its standard error, from the game results, so a reader sees which places are
resolved and which are not.

### Gate

Unchanged: the round robin's winner, the challenger, plays the champion (`hand-crafted` in
generation 1) under `gate.rule` at `depth`, and only its win passes; a draw never promotes.

## Running and resuming

1. Refuse unless every lambda of the grid has its `candidate.nnue`.
2. Play the pairings one after another, in Berger rounds (changed 2026-10-02 from grid order, below), each
   with every logical core in flight, as a fixed-depth duel uses today (`Placement`). Pairings in
   sequence rather than interleaved: the runner, its resume and its folder per pairing are reused as
   they are, and with all cores busy on one pairing nothing is lost.
3. Each pairing is a runner run under a new rule that only counts (`round_robin::Pairing`): it
   resolves when exactly `round_robin_pairs` pairs are recorded, never earlier. Without more, the
   runner would keep every worker busy until the rule resolves and then discard the pairs still in
   flight, which would also favour short games among the pairs kept. So the rule declares a
   `pair_limit`, and the runner starts a new pair only while the recorded pairs plus those in flight
   are below it: exactly that many pairs are played, none discarded. Counted, not compared with the
   pair index, since a resumed run has gaps in its indexes that still need filling.
4. A rerun rebuilds each pairing from its own `games.db`, as the ladder does: finished pairings are
   skipped, an unfinished one resumes. No state file of its own.
5. Once every pairing is complete: write the standings and the winner, then play the gate.

A one-value grid plays no round robin: its lambda is the winner, and only the gate is played.

## Live display (added 2026-10-02, implemented, tests not yet run)

The round robin redraws a standings table in place, with a progress bar under it, instead of
scrolling a line per pair. The gate keeps the runner's usual output.

```
round robin, provisional (scores, not points, until every pairing is complete)
rank  lambda  games  score  95%      points
 1    0.75     600  53.1%  +-3.2%   318.5
 2    0.70     600  51.4%  +-3.3%   308.5
 ...
[=========>          ] 2000/4000 games  pairing 6/10 0.70-0.85  37m, ETA 37m
```

- `indicatif`'s `MultiProgress`: one line per row, text replaced as results arrive, and the bar.
- The runner reports each recorded pair through a callback instead of printing its line, for the
  round robin only; `match`, `sprt`, `duel` and the gate print as today.
- The uncertainty column is the 95% interval of the score, 1.96 standard errors, shown as `+-3.2%`:
  read directly as "the true score is within this much", with no statistics to recall. The final
  table and `standings.txt` show the same.
- After each pair the table is recomputed from the complete pairings plus the current one's running
  record. Until every pairing is complete the candidates have played different numbers of games,
  so the live table ranks by score percentage and says it is provisional; the final ranking is by
  points, as above.
- The bar: games so far against the round robin's total, the current pairing, elapsed time, ETA.
- Not a terminal (stderr redirected to a log): no table; the runner prints its usual line per pair,
  and the final table is printed at the end.
- A resume fills the table from the complete pairings' recorded games at once.

## Layout

```
gen-0001/selection/amanda-0001-0.65-vs-amanda-0001-0.70/   games.db, games.pgn: one pairing
...
gen-0001/selection/hand-crafted-vs-amanda-0001-0.75/        the gate
gen-0001/standings.txt                                      the round robin's table
gen-0001/selection.txt                                      `winner` and the gate's line, as promote reads it
gen-0001/verdict.txt                                        pass or fail, from the gate only
```

`standings.txt`: one line per candidate, best first: lambda, points, games, score percentage, and
the score's 95% interval (`+-3.2%`).

`selection.txt` has `ladder.txt`'s fields under a new name, and `verdict.txt` is unchanged.

## Code

- `config.rs`: `selection:` replaces `ladder:` (`load_selection_spec`, `SelectionSpec`); the
  committed-config test follows.
- `runner.rs` or a sibling: the counting rule (`SequentialTest` that resolves at its pair count).
- `main.rs`: `run_selection` replaces `run_ladder`, the command is `select`; `newcomer_takes_ladder`
  and its test go.
- `generation.rs`: the pairing folder, `standings.txt`, `selection.txt` replacing `ladder.txt` (its
  writer, `read_ladder_winner`, `promotable_challenger` and their tests).
- `rotor-train`: its messages that name `rotor-arena ladder` or `ladder.txt`.
- `configs/amanda.yaml`, `configs/betty.yaml`: the `selection:` section, in the same commit as the
  code, since `rotor-arena` refuses the old section and the committed-config test reads both.
- README: the ladder paragraph.
- ladder.md, ladder-tie-break.md: a "superseded, see round-robin-selection.md" note when this is
  done.

## Decided afterwards

- **The command is `rotor-arena select <name>`**, replacing `rotor-arena ladder`.
- **`ladder.txt` becomes `selection.txt`**, same fields (`winner`, then the gate's line);
  `generation::read_ladder_winner` and `promotable_challenger` read the new name. Generation 1 has no
  `ladder.txt` yet, so nothing is migrated.
- **Depth only**: `selection:` refuses `time`. The round robin's parallelism needs a fixed depth, and
  one control keeps the challenger chosen and gated under the same one.
- **No lock.** Amanda's and betty's selections run one after the other, by hand: the developer is the
  only user.

## After amanda's first selection (2026-10-02)

The first real run: 0.85 528.5, 0.80 527.5, 0.75 515.0, 0.65 482.5, 0.60 475.0, 0.70 471.5 points
over 1000 games each, margins about +-2.5%; the gate passed 0.85 at 10-0 after 10 games. Not
promoted: the selection is fixed and rerun first.

- **Berger rounds instead of grid order.** In grid order the first lambda played all its pairings
  before the last played any, so the live table was lopsided until the end. `berger_order` plays
  the pairings in rounds by the circle method: with 6 candidates, 5 rounds of 3 disjoint pairings,
  so after every round each candidate has met one more opponent. Only the order changes: a pairing
  is still one runner run in its own folder, named by its players. Interleaving pairs across
  pairings was rejected: one database for the whole round robin and engines switched per pair.
- **`margin` replaces `95%` (live table) and `interval_95` (`standings.txt`)**: the score's margin
  of error, still 1.96 standard errors, still ASCII `+-2.5%`.
- **The live table is one progress bar** whose prefix is the whole table, redrawn at most 4 times a
  second: a bar per line flickered.
- **The gate's rule reads pairs in index order, not completion order** (`runner.rs`, for every
  rule: duel, sprt, match). The first gate resolved 10-0 on the first 5 pairs to finish out of 20
  in flight: the shortest games. Pair indexes are handed out before play, so index order does not
  depend on how games go. A pair finished early is recorded at once and waits for the pairs below
  it; `replay` reads the database in the same order and stops at the same resolution, so recorded
  pairs past it never move a resumed verdict. `pair_limit` counts the waiting pairs too.
- The first run's games are deleted; only its table above is kept.

## Verification

```
cargo test --bin rotor-arena
```

Then a real run on amanda's generation 1.
