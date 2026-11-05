# Automatic tuning

**Archived:** search parameters are picked by reasonable magnitudes, not tuned (search-simplification.md), and tuning the hand-crafted weights is dropped. Kept as a record of the idea, which TODO.md cites.

## Automatic tuning of search parameters (Adam-inspired)

### Probe and duel

Let p[k][0], ..., p[k][n - 1] be the parameters at iteration k, stored as
floats. p[0] are the starting values, taken from the current search settings.

Let c[i] be the probe distance of parameter i, in the parameter's own units:
c[i] = max(1, round(0.1 * |p[0][i]|)), or a fixed small value (e.g. 5) when
p[0][i] = 0. At least 1, since the parameters are integers and a smaller probe
would round away.

At each iteration k + 1:

Let sign[k + 1][i] be a random sign in {-1, +1}, drawn uniformly and
independently for each i. The signs must stay random, independent and fair:
this is what lets a single duel tell the parameters apart.

Let q[i] = round(p[k][i] + c[i] * sign[k + 1][i])

Let r[i] = round(p[k][i] - c[i] * sign[k + 1][i])

Run rotor-arena duel with q against r, capped at MAX_GAMES games (1000 today,
possibly lowered to e.g. 400). Only the raw tally is used, not the duel's
verdict: nearly equal engines usually reach the cap drawn.

Let score[k + 1] = 2 * s - 1, where s = (wins + draws / 2) / games is q's
result. So score is in [-1, +1]: positive when q is better, negative when r
is better, 0 for an even duel.

### Update

beta1 = 0.9
beta2 = 0.95
G = 0.1

m[0][i] = 0
v[0] = 0

g[i] = score[k + 1] * sign[k + 1][i]
m[k + 1][i] = beta1 * m[k][i] + (1 - beta1) * g[i]
v[k + 1] = beta2 * v[k] + (1 - beta2) * g[0] * g[0]

mm[k + 1][i] = m[k + 1][i] / (1 - beta1 ** (k + 1))
vv[k + 1] = v[k + 1] / (1 - beta2 ** (k + 1))

epsilon = 1e-8

lr[i] = G * c[i]

step[k + 1][i] = lr[i] * mm[k + 1][i] / (sqrt(vv[k + 1]) + epsilon)

p[k + 1][i] = p[k][i] + step[k + 1][i]

### Champion check

Every K iterations (e.g. K = 10), run rotor-arena duel with round(p[k + 1])
against the champion, i.e. the best parameters so far (initially p[0]):

- p wins: round(p[k + 1]) becomes the champion.
- Drawn: keep going, keep the champion.
- Champion wins: reset p[k + 1] to the champion, and m and v to 0. The bias
  correction then restarts too: k in beta1 ** (k + 1) and beta2 ** (k + 1)
  counts iterations since the last reset, not since the start of the run.

The run stops when its budget is spent. At the end, the champion is the result.

## Automatic tuning of evaluation parameters

Given a corpus of positions, run Search(9) of the current untuned
evaluation function, and use AB(lambda) to build labels in [-1, +1].

At this point, we have a training set from positions to evaluations.

Now I am lost.

Can the hand-crafted evaluation function be written in torch, then
optimized as if it were a neural network?
