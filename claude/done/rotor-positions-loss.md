# The loss on Rotor's own positions

**Status:** approved, implemented and run 2026-10-28 (`engine_loss::tests::duel_loss`). Duel A
played the same day: SCReLU 518.0 to 452.0 of 970, pairwise_hidden 46.6%, about -24 Elo, drawn
(curtailed). Duel B the same day. **Both agree with their duels, but a network scored on its own
duel's games is biased toward the winner** (duel B's results): the shift likely, not proved; the
clean test scores both on games neither played (Open).

## Results (2026-10-28, duel A)

988 games in `games.pgn`, every one with a result: the duel stops counting at 970, the pairs in
flight then still end and are recorded. 111,722 positions out of check. A pairwise_hidden (lambda
0.01), B SCReLU (lambda 0.02), both quantized; loss against the game's result:

| pieces | to move | positions | pairwise_hidden | SCReLU | SCReLU against it | +/- 2 s.e. |
|---|---|---|---|---|---|---|
| 3-6 | either | 13,651 | 0.032518 | 0.046984 | +44.5% | 35.9% |
| 7-16 | either | 47,761 | 0.222372 | 0.214959 | -3.3% | 4.8% |
| 17-32 | either | 50,310 | 0.565329 | 0.553858 | -2.0% | 1.6% |
| 7-32 | either | 98,071 | 0.398307 | 0.388813 | -2.4% | 2.1% |
| 7-32 | pairwise_hidden | 49,129 | 0.403682 | 0.388147 | -3.9% | 2.2% |
| 7-32 | SCReLU | 48,942 | 0.392912 | 0.389481 | -0.9% | 2.0% |
| all | either | 111,722 | 0.353612 | 0.347046 | -1.9% | 2.1% |

- **The lead reverses.** On Lc0's test positions pairwise_hidden is 7.8% below SCReLU (both on
  lambda 0.02's labels); on Rotor's from 7 pieces up, SCReLU is 2.4% below it. A lead carried over
  would read about +8% in that column, far outside its error.
- **Worst where pairwise_hidden is to move** (-3.9% +/- 2.2%), level where SCReLU is: the positions
  its own search chose, as `duel_games`'s drops of a pawn said.
- **3 to 6 pieces** keep its gain, as on Lc0's positions, where the tablebases decide the duels.
- The label is the result alone, noisy: the error is wide, but not wide enough to save the lead.

## Results (2026-10-28, duel B, the control)

SCReLU 501.0 to 407.0 of 908 against plain pairwise (lambda 0.03), S 0.5518, **SCReLU shown
stronger**, about +36 Elo. 914 games in `games.pgn`, 105,986 positions:

| pieces | to move | pairwise | SCReLU | SCReLU against it | +/- 2 s.e. |
|---|---|---|---|---|---|
| 3-6 | either | 0.026665 | 0.037032 | +38.9% | 24.8% |
| 7-16 | either | 0.186861 | 0.188447 | +0.9% | 3.6% |
| 17-32 | either | 0.560670 | 0.546255 | -2.6% | 1.7% |
| 7-32 | either | 0.377974 | 0.371379 | -1.7% | 1.7% |
| 7-32 | pairwise | 0.383342 | 0.373294 | -2.6% | 1.8% |
| 7-32 | SCReLU | 0.372668 | 0.369486 | -0.9% | 1.7% |

Plain pairwise, level with SCReLU on Lc0's positions (+0.13%), also falls behind here, and also
most where it is to move. **The control moved: the measure sides with each duel's winner.** Part of
that is built in: the loser chose its moves because its evaluation liked them, and the games then
went against it, so its positions are selected for its evaluation's errors against the result.
Duel A's reading weakens to: pairwise_hidden swung about 10 points (from 7.8% ahead to 2.4%
behind), plain pairwise about 2 for a similar duel loss, so a shift beyond the selection is likely
for pairwise_hidden, not proved. The "Next" of loss-and-strength.md, on a machine
other than the training machine (CPU only).

## Why

pairwise_hidden fits Lc0's positions 7.8% better than SCReLU (quantized, both on lambda 0.02's
labels) and loses to it by about 55 Elo at depth 9 (loss-and-strength.md). The reading left there:
Rotor's games are decided on the positions Rotor's games reach, which Lc0's test set does not
represent, and there the hidden layer generalizes worse. The direct test: both networks' loss on the
positions of Rotor's own duels. If pairwise_hidden's lead reverses there, the shift is confirmed and
a loss on Rotor's positions becomes the screening measure the pipeline lacks (512's included); if it
still leads, an evaluation noisier everywhere is the likelier reading.

## Decisions (proposed)

1. **New duels, played on this machine (the developer, 2026-10-28),** not the earlier ones' PGNs,
   which stay on the machine that played them. Both `@default`, depth 9, by the deprecated `arena
   duel` as the earlier diagnostics were (its `games.pgn` in `data/matches/`, local):
   - **A:** `lc0-psq-threats-screlu-g1-r1` against `lc0-psq-threats-pairwise-hidden-g1-r1` (the
     duel network, lambda 0.01). Expected about 42% to 45% for pairwise_hidden, as tests 17 and
     the f32 replay; a second, independent sample of that duel too.
   - **B, the control:** `lc0-psq-threats-screlu-g1-r1` against `lc0-psq-threats-pairwise-g1-r1`.
     Plain pairwise is already explained (its quiet loss equal to SCReLU's on Lc0's positions, its
     ranking worse), so on Rotor's positions its loss should stay about level with SCReLU's. If it
     too moves sharply, the measure moves with something other than the shift.
   B runs only after A, and only if A's result is worth checking against a control.
2. **Every position scored by both networks,** whoever played it: the comparison is over the same
   positions, so the duel's winner does not pick them. Reported also split by which network was to
   move, since a network's own positions are the ones its search chose.
3. **The label: the game's result from the side to move,** `rotor::labels::outcome_label`, lambda 1.
   Noisy per position, the same noise for both networks; no deeper search as a label, which would
   favour the network that searches. **Positions in check left out**, as the search never evaluates
   them; the game's last position too (no move played from it). Positions after the opening book's
   moves only, the book's own being no player's choice (the PGN's `[FEN]` is the book's exit).
4. **Both networks quantized,** as the engine plays them (`QuantizedPsqNetwork::evaluate_pawns`),
   as `engine_loss` does.
5. **The two networks named by the PGN's players,** resolved by `players::resolve_network_file`
   (local run, else Drive), the `@` suffix stripped; no second environment variable, so a result
   cannot be scored against the wrong file.
6. **Bands as `engine_loss`'s** (3 to 6, 7 to 16, 17 to 32 pieces), plus **7 pieces up** on its
   own line: the duels play with 3 to 6 piece tablebases, so that is what they can see
   (loss-and-strength.md, "Open").
7. **Code:** `duel_games` keeps its table and gains this one, in `engine_loss.rs`, test-only, one
   thread; the replay from `[FEN]` with `pgn_replay::try_resolve_san`, as
   `game_labels::self_play_game_best_moves` does. A unit test on a short hand-written PGN checks the
   replay, the positions kept and the labels' side.

## Steps

1. **Duel A,** on an idle machine (nothing else running; it takes every core). Its command, for the
   developer:

   `./bin/arena duel lc0-psq-threats-screlu-g1-r1@default lc0-psq-threats-pairwise-hidden-g1-r1@default --depth 9`
2. **Code** (decision 7) after the duel has ended, never while it plays: a build would load the
   machine the duel's speed depends on (the developer, 2026-10-28).
3. **Measure** on duel A's `games.pgn`; results here and in loss-and-strength.md.
4. **Duel B and its measure,** if wanted (decision 1).
5. **Docs:** loss-and-strength.md's "Where it stands"; TODO.md; engine-loss.md's "How to run".

## Results (2026-10-28, the clean test)

The games: `lc0-psq-threats-screlu-phase-g1-r1` against `lc0-psq-threats-screlu-count-g1-r1`, both
`@default`, depth 9: 452.0 to 460.0 of 912, drawn (curtailed); 914 games in `games.pgn`, about
107,000 positions. Scored by two networks neither of which played:

| from 7 pieces up | Lc0's positions | these positions, SCReLU against it | +/- 2 s.e. |
|---|---|---|---|
| pairwise_hidden | it 7.8% ahead | +0.36% | 0.87% |
| plain pairwise | level (+0.13%) | -0.35% | 0.77% |

By band, pairwise_hidden is 3.5% +/- 2.4% ahead at 7 to 16 pieces and 0.7% +/- 0.8% behind from 17
up; plain pairwise level in both.

- **pairwise_hidden's lead on Lc0's positions does not carry over** to positions Rotor's games
  reach: about 8% to nothing, well outside the error. That part of the shift is real.
- **Its deficit on its own duel's games was the selection:** on games neither played it is level.
- **Fit does not explain either lost duel:** both pairwise networks fit these positions as SCReLU
  does and lose to it, by -24 to -55 and about -36 Elo. What loses them games is in the search,
  as hypothesis 6 of loss-and-strength.md has it (ranking between siblings, not fit).
- **Caveat, the measure's sensitivity:** a result label is noisy, and noise shrinks relative gaps.
  Calibration: threats against psq (about +51 Elo, about 5% on Lc0's loss) scored on the same
  games. A clear gap there and "level" here means level; none, and this measure cannot see a gap
  of this size.

**Calibration, the same day:** `lc0-psq-threats-g1-r1` (lambda 0.02) against `lc0-psq-g1-r1`
(lambda 0.03) on the same games, from 7 pieces up: threats 3.19% +/- 0.81% below psq (7 to 16 pieces
-6.6% +/- 1.6%, from 17 up -1.9% +/- 0.9%), about eight standard errors. **The measure is
sensitive:** it sees a gap of about +51 Elo clearly, so the pairwise networks' "level" is level
within about 0.8%. The players' threats inputs may lean the games toward threats a little, not by
that much.

## Open

- A sibling-ranking test inside the search (loss-and-strength.md, hypothesis 6): what fit cannot
  explain.

- **The clean test (approved 2026-10-28):** both networks scored on games neither played, so no
  selection by either's evaluation. `ROTOR_DUEL_LOSS_NETWORKS="<a>,<b>"` names them (the PGN's
  players when unset; implemented the same day). The games: `lc0-psq-threats-screlu-phase-g1-r1`
  against `lc0-psq-threats-screlu-count-g1-r1`, both `@default`, depth 9, played here; scored
  pairwise_hidden against SCReLU, then plain pairwise against SCReLU. SCReLU's buckets are kin to
  both players, a mild bias toward it, the same in both comparisons.

- Whether the result label is too noisy at about 100,000 positions to separate 2% to 3%; if so, a
  bootstrap over games (not positions, which are correlated within a game) for its error.
