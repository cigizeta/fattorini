# Training corrections

## Expected scores to centipawns

We adopt Lc0's approach. Therefore

```
cp = A * tan(B * q)
```

where `q` is the expected score from -1 (loss) to +1 (win), that is W - L, as in Lc0.

We adopt A and B as defined by Lc0 (score type `centipawn`, in `src/mcts/search.cc` as of v0.31.2):

```
A = 90
B = 1.5637541897
```

so `q = +-1` gives `cp = +-A * tan(B)`, about +-12,800.

## Centipawns to expected scores

The inverse of the above:

```
q = clamp(atan(cp / A) / B, -1, +1)
```

with the same A and B. Since B < pi/2, any `|cp| > A * tan(B)` (about 12,800) would give
`|q| > 1`, hence the clamp.

## Training loss

We experiment with changing the training loss. For now the game result is left out: no lambda, the
target is the teacher's evaluation alone.

### Target

The target is the expected score from 0 to 1. Every teacher that gives centipawns, PeSTO and our
own networks alike, is read with the same rule, `t = sigma(cp / S)`; Lc0 alone, which gives a
score, is read as it is.

**Lc0.** Let `q` be Lc0's evaluation, W - L, from -1 to +1. Then

```
t = (q + 1) / 2
```

No scale is needed: `q` is already a score, and the tan curve above plays no part in training.

**PeSTO.** Let `cp` be PeSTO's search score in centipawns, from the side to move's view. Then

```
t = sigma(cp / S_pesto)
S_pesto = S = 70.37
```

or equivalently `q = tanh(cp / (2 * S_pesto))`. Mates and tablebase results stay outside the
curve, at `t = 0` or `1`.

With `S_pesto = S` and no lambda, the student is trained so that `sigma(net / S) = sigma(cp / S)`,
so it learns PeSTO's evaluation itself: `net = cp`. PeSTO's centipawns are read as Rotor's.

**Why S_pesto is a convention, not fitted.** PeSTO's centipawns are hand-made, so no curve is
known to turn them into expected results. S_pesto could be fitted on PeSTO's own games: for each
score PeSTO gives, the average result of the games, and the S whose `sigma(cp / S)` passes closest
to those averages. Without lambda that buys little: S_pesto only rescales the student's output
linearly (`net = cp * S / S_pesto`), a change of unit that SPSA absorbs later. It matters once the
game result is blended in: if `sigma(cp / S_pesto)` does not match the results PeSTO's games
actually reach, part of what lambda corrects is that mismatch rather than the noise of the search.
Accepted for now.

**Our networks.** Let `cp` be the network's search score in self-play, in Rotor's centipawns, from
the side to move's view. Then, as for PeSTO,

```
t = sigma(cp / S)
```

Mates and tablebase results stay outside the curve, at `t = 0` or `1`.

Here S is not a convention but the network's own unit: the network was trained so that
`sigma(net / S)` is its expected score, and the search score is in the same unit. Reading it back
at the same S is the exact inverse of training, so with no lambda the student learns the teacher's
search score itself, `net = cp`, and no scale drifts from generation to generation. (Reading it
through Lc0's tan curve instead would not invert the training curve: past equality every label
would shrink toward 0, `q` 0.90 coming back as 0.74, and each generation would shrink it again.)

The caveat is PeSTO's: a network is calibrated to its training labels, not to the results of its
own self-play games, and the two can differ (measured for emily-g1 under the atan curve: a pawn up
ended at 0.71 where the curve said 0.54). Once lambda returns, part of what it corrects may be that
mismatch. Accepted on the same terms.

### Prediction

Let `net` be the output of the neural network, in centipawns. It is unbounded, so it is mapped to
(0, 1) by the logistic sigmoid:

```
p = sigma(net / S)
S = A * B / 2 = 70.37
```

Equivalently, the network learns `q = tanh(net / (2 * S))`.

### Loss

The loss is the binary cross entropy of `p` and `t`.

### Why the sigmoid, not Lc0's curve

Binary cross entropy backpropagated naively through Lc0's curve would be bad for training:

- Since B < pi/2, `p` reaches 0 or 1 at the finite `net = +-A * tan(B)`: there `log p` or
  `log (1 - p)` diverges and the gradient blows up, and beyond it the clamp makes the gradient
  zero, so a wrong prediction out there is never corrected.
- The gradient is no longer `p - t` but `p - t` times a varying factor, weighting positions away
  from equality erratically.

With the sigmoid, the gradient of the binary cross entropy with respect to `net / S` is just
`p - t`: bounded, never zero while the prediction is wrong, numerically stable, and every position
keeps a gradient. It is also what Bullet and most NNUE trainers use.

**The current trainer does not have these problems either.** It maps its output through an atan
curve of the same shape as Lc0's (one pawn at 0.50, reaching 1 at 128 pawns), but trains with a
matching loss: the gradient on the output is `score(net) - t`, with no backpropagation through the
curve or its clamp. That is the same form as the sigmoid's `p - t`, the residual times a constant.
So the sigmoid does not fix a gradient problem of today's trainer. What the proposal really changes
is the curve, atan to tanh, and with it the unit of `net`:

| Curve | Slope at `q = 0` | `q` at one pawn |
|---|---|---|
| Today's atan | 0.63 per pawn | 0.50 |
| Proposed tanh, S = 70.37 | 0.71 per pawn | 0.61 |
| Lc0's tan curve | 0.71 per pawn | 0.53 |

The proposal matches Lc0 exactly at equality and departs from it by one pawn; today's curve is a
little flatter at equality but stays close to Lc0's out to large evaluations. Past about 2 pawns
tanh saturates much sooner than either atan, so `net` must grow much larger to express the same
advantage, and it never reaches `q = +-1` at a finite value, where today's curve does at the
engine's ceiling of 128 pawns. What the sigmoid does bring: the loss value is a true binary cross
entropy, and the setup is Bullet's, so experience from other engines carries over.

### Why S = A * B / 2

S cannot be derived from training: with a linear output layer, multiplying S and the output weights
by the same factor leaves predictions and loss unchanged. S only fixes the unit of `net`, so it is
chosen by a convention, and the convention here is that `net` reads as Lc0's centipawns near
equality.

The two curves are made to agree in slope at `q = 0`:

- Lc0's curve, `q = atan(cp / A) / B`, has slope `1 / (A * B)` at 0.
- The sigmoid, read as `q = tanh(net / (2 * S))`, has slope `1 / (2 * S)` at 0.

Equating them gives `2 * S = A * B`, so `S = 90 * 1.5637541897 / 2`, about 70.37.

The agreement holds only near equality: at `net = 100` the sigmoid gives `q` about 0.61, Lc0's curve
0.53, and beyond that tanh flattens while tan keeps growing.

### What S does not change

S does not change the strength of the network, only the scale its output is read on. Strength could
still change through what reads `net`:

- Search margins in centipawns: SPSA tunes them later to whatever scale `net` has, so S is fixed
  before SPSA, and changing it afterwards means tuning again.
- Quantisation: SPSA does not tune it, so the chosen S is checked once to keep `net` within the
  integer range, neither clipped nor wasting precision.
