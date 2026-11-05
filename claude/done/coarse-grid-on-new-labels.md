# A coarse lambda grid when the labels change

## The problem

A generation's grid is 9 lambdas 0.01 apart, centred on its parent's winner (`grid`, `GRID`,
`STEP_HUNDREDTHS`). Lambda is the game result's weight in a label: `lambda * outcome + (1 - lambda)
* score`.

`lc0-psq-threats-screlu-quiet-r1`'s generation 1 trained on Lc0's labels and was won by lambda 0.02.
Its generation 2 trains on Rotor's self-play, labelled by Rotor's depth 9 search, and would inherit
the grid 0.01 to 0.09. That grid was tuned for Lc0's deep-search scores, which deserve almost all
the weight. Our earlier self-play, on shallow scores, was won by 0.98 instead. Where Rotor's depth 9
scores stand has never been measured.

## The rule (the developer, 2026-10-30)

- **The first generation on new labels gets a coarse grid: 0.10 to 0.90, 0.10 apart.** The labels
  are new when the promoted parent trained on a source (Lc0's corpus) and the child plays its own
  games.
- **Every attempt of that generation keeps it.** A failed gate means more data, not a wrong lambda,
  and the attempts stay comparable. That is today's behaviour: a failed gate changes only the
  attempt and the failed gates count.
- **The generation after it gets the fine grid again,** 0.01 apart, centred on its parent's winner,
  as today.

The developer does not expect the coarse tournament to be flat: 0.10 is close to Rotor's own scores
and 0.90 close to the game results, very different targets.

## The change

1. `GenState` gains `step_hundredths`, written as `step` beside `center` in the run's `state`. A
   state without `step` reads as 1, so every existing run is unchanged.
2. `grid(center, step)`: 9 lambdas `step` apart, centred on `center`, sliding to stay in
   0.01..=0.99. `GRID` stays; `STEP_HUNDREDTHS` becomes the fine step, beside a new
   `COARSE_STEP_HUNDREDTHS` = 10 and `COARSE_CENTER_HUNDREDTHS` = 50, which give 0.10 to 0.90.
3. Every call of `grid` passes the state's step (the snapshot, the attempt view, the training).
4. At promotion: when the promoted generation trained on a source (`run.config.on_source`), the next
   state gets the coarse step and centre; otherwise the fine step and the winner's centre. A scaled
   run's promotion despite a failed gate follows the same rule.
5. Generation 1's own state: the fine step, centred on `grid_center`, as today.

## Tests

- `grid(50, 10)` is 10, 20, ..., 90; `grid(2, 1)` and the other existing cases unchanged.
- A state without `step` parses with step 1; a state with it reads back what it wrote.
- Promotion from a generation on a source gives step 10 and centre 50; from a self-play generation,
  step 1 and the winner's centre.

## This run

`-quiet-r1`'s generation 2 was promoted before the change, so its `state` lacks the step. Once the
change is built:

1. Stop the run before its self-play ends (the page's Stop), so training does not start on the old
   grid.
2. In the run's `state`, set `center` to 50 and add `step` 10.
3. Restart the smithy on the new build and Resume.

No lambda directory exists yet for generation 2, so nothing trained is thrown away.
