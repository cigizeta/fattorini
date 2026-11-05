# Lambdas that fell back to the teacher play no round robin

## The problem

A warm-started generation measures step 0, its teacher's own weights, and rolls back to the best
checkpoint at each stage switch. A lambda none of whose rounds beats step 0 hands in step 0 as its
candidate: the teacher itself. On 2026-10-30, `-quiet-r1`'s generation 2 spent its whole first
stage (3e-3, 7 rounds) without a single lambda beating step 0; if every stage did the same, the
round robin would match nine copies of the teacher, its winner chance, and the gate would match the
teacher against itself: hours of arena time for a result the training already showed.

## The rule (the developer, 2026-10-30)

After training, before the round robin, a lambda whose candidate's step is 0 has **fallen back**.

- **Some fell back:** they play no round robin; the others do.
- **One learned:** no round robin; it goes straight to the gate.
- **None learned:** no round robin and no gate. The generation **learned nothing**: it counts as a
  failed gate (`max_failed_gates`, the next attempt), with its own message and a `generations.tsv`
  row whose winner is `none`, gate games 0, so it is never read as a gate lost in games.
- A scaled run, which promotes a failed gate's winner all the same, has no winner to promote here:
  it fails like a full run.
- A lambda whose candidate record cannot be read is kept, as before, and its round robin says what
  is wrong.

## The change

- `learned_lambdas(dirs, generation, attempt, lambdas)`: the lambdas whose candidate step is above 0
  (`candidates::read_candidate_step`).
- `run_generations` plays the round robin over them alone, skips it for one, and skips both phases
  for none.
- The page's round robin, read from its record (`forge_view`), indexes the same lambdas: the
  record's players are the learned ones.

## Tests

- `a_lambda_whose_candidate_is_step_0_fell_back_to_its_teacher`: steps 0 and 8192 and a missing
  record; all at step 0 gives none.

## Applied to the running generation

Built during `-quiet-r1`'s generation 2 training, by the developer's choice: the smithy restarted
mid-training, losing at most the round under way.

## Status

Done 2026-10-30: `-quiet-r1`'s generation 2, every lambda having beaten its step 0 at 3e-4, played
its round robin with all nine lambdas, no fallen-back line logged. The fallback itself, a lambda at
step 0, has not happened in a real run yet.
