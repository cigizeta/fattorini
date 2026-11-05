# Installing the hand-crafted evaluation

Implementation plan. Branch: `alessio`. `rotor-train install <network>` deploys a network's
champion from Drive; there is no way to deploy the engine with no network, which is how it plays the
hand-crafted evaluation (no `rotor.nnue` beside the binary).

## Design

Settled in discussion.

- `rotor-train install hand-crafted`: the arena's name for this player. `run_install` takes its own
  path for that name; every other name installs a champion as now.
- In order: build the release engine; remove `rotor.nnue` if present (absent is fine, any other
  error stops the install); install the binary through `.partial` and a rename; write `rotor.txt`
  last, with `network hand-crafted`, `commit`, `installed`.
- Network first: a lichess-bot engine started midway then gets the old binary with no network, the
  hand-crafted evaluation, never the new binary beside a network it might not load (the engine exits
  when its `EvalFile` fails to load).
- No Drive: nothing is read from the remote.

## Tests

- The record holds exactly `network`, `commit`, `installed`, in that order.
- `USAGE` names `install hand-crafted`.
