# Continuous bars (design agreed 2026-11-26)

## Goal

Every progress bar on the smithy's page moves at every redraw (every two seconds), not only when a
result arrives (the developer, 2026-11-26: "every bar in the codebase should follow the continuous
logic"). The training budget's bar already does (`budgetProgress`); every other bar follows it.

## The rule

A bar shows what is done plus an estimate of the part under way. The estimate never reaches the
next real step, so the bar never overshoots and never moves backwards when a result arrives. The
percent and the ETA follow the bar; the count in the text stays whole, a fact rather than an estimate.

## Inventory

Already continuous, unchanged: the training budget, Lc0 Convert (bytes, positions), the book passes
(bytes), scoring (moves), the live bars (four times a second), and the terminal bars of the
`progress` crate (fine-grained counts).

Stepped, to change:

| Bar | Unit | The part under way |
|---|---|---|
| Trial (duel, match) | games | by time, capped at the pairs in play, 2 games each |
| Tune | pairs | by time, capped at the pairs in play |
| Round robin | games | by time, capped at the pairs in play, 2 games each |
| Self-play test set, training set | positions | by time, capped at the games in play times the average positions a game |
| Patience rounds | rounds | the round under way's live share, as the budget's |
| Stage progress | rounds | the round under way's live share, as the budget's |

Left out: the finished arm's bar (full) and the gate's gauge, which plots evidence, not progress: an
estimated LLR would be invented.

## The two training bars

As `budgetProgress`: the round under way (`round`, from the live pass) adds `round.done /
round.total` of its training half, 1 once it is testing, nothing when finished. Patience: the
estimate is added to `done` (the lighter, tentative stretch), not to `settled`. Stage: to `r.round`
in the bar, the percent and the ETA.

## The game bars

- **The estimate:** `elapsed since the count last changed / seconds a unit`, the seconds a unit
  the average the line already shows (`times`: games/s; self-play: positions/s).
- **The clock:** the page notes, per bar, the moment it saw the count change (client time, keyed by
  the bar's place). No server field. After a reload it starts from zero: the bar under-estimates
  for a moment, never overshoots.
- **The cap (choice (a), the developer, 2026-11-26):** the work actually in play, the sum of
  `state.arenas[].in_play` (tasks its workers are playing, from each arena's last poll), already in
  `state.json`. A task is a pair for trials, tunes and round robins, one game for self-play. Self-play
  converts games to positions at the set's average positions a game: if its view does not give games
  played, the server adds that count to the self-play view (the one backend change, if needed).
  The estimate is also capped below `total`.
- **The ETA:** `(total - position) * seconds a unit` on the page, rather than the server's
  `eta_seconds`, so it counts down at every redraw.

## Implementation

All in `crates/smithy/src/page.html`, unless self-play needs its games count:

1. A helper `underWay(key, count, perSecond, cap)`: records when `count` changed for `key`, returns
   the estimate, `min(elapsed * perSecond, cap)`, 0 without a pace.
2. A helper `inPlay()`: the sum of `state.arenas[].in_play`, nulls as 0.
3. Trial, tune, round robin, self-play: position = count + `underWay(...)`; bar, percent and ETA from
   the position.
4. `patienceProgress` and `stageProgress`: the round under way's share, as `budgetProgress`.
5. Comments name the rule once, at the helper, and each bar says which unit and cap it uses.

No test covers page.html: checked by eye on a running trial and a running training.

## Written 2026-11-26

As planned, with: `gamesUnderWay` (the game bars' position and ETA, nothing under way unless a job
runs and, for a trial, before its verdict: a past trial shown while another job plays stays still),
`roundShare` (the two training bars); `times` takes the continuous ETA in place of the record's.
Self-play needed its games: `Shared::self_play_games`, sent as `positions_per_game`.
