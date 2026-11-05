# Self-play's targets in kept positions

Self-play plays until its test set and its training set hold `test_positions` and
`attempt x training_positions` positions, counting every position a game read, the final one
included. Training and the freezer keep fewer: `Keep::Searched` drops the final position, `Keep::Quiet`
every position in check or whose best move is a capture or a promotion. So no generation 2 can freeze
its test set (quiet-filter-implementation.md, Open), and a quiet run would train on less than its
config says. Both targets now count the positions the run's `Keep` keeps.

## Why now

`lc0-psq-threats-screlu-quiet` lost both duels against `lc0-psq-threats-screlu-g1-r1` (TODO.md): on
Lc0's labels, the filter cost the network its static view of positions with a capture pending. The
engines that filter (bullet's users, Renegade) label with their own search and filter by their own best
move. The experiment (the developer, 2026-10-30): `-quiet`'s generation 2, on Rotor's self-play
filtered by Rotor's best move, gated against its generation 1, then, if it passes, a duel against
`lc0-psq-threats-screlu-g1-r1`. Attempts beyond the first by hand, by Resume (each gate ends the job,
`max_failed_gates: 3`). A control, SCReLU's own generation 2 on every position, only if `-quiet`'s does
well. Both then train on the same number of kept positions, so the filter alone differs.

## Decisions

1. **Both targets count kept positions** (the developer, 2026-10-30), by the run's `config.keep()`.
   A quiet run plays more games for the same `training_positions`; every attempt still adds
   `training_positions` kept, attempt `a` training on `a x training_positions` (the training set is the
   generation's, each attempt starting again from the teacher's weights).
2. **A game's kept positions:** `game_labels::kept_count(text, keep) -> u64`, the PGN parsed as the
   readers parse it (`SelfPlayGameCollector`) and the `true`s of `self_play_game_kept` counted. Self-play
   carries no visit counts, so `Quiet` reads Rotor's best move alone.
3. **On resume, counted again from the games:** `play` reads what `test_dir` and `train_dir` hold
   (closed segments through `SelfPlaySegmentReader`, the open file through `SegmentWriter::resume`) and
   counts their kept positions, in place of the `.meta` sums. No `.meta` format change; segments
   written before still count right. The training set's reread takes minutes at each resume, accepted.
4. **`Work::SelfPlay` carries the `Keep`,** `Option<Keep>`: `None` counts every position read, as
   today, for PeSTO's play, which stays out of scope (the developer, 2026-10-30; its corpus is shared by
   every config, all `Searched`, its target short only by the final positions).
5. **The page and the log say "kept positions"** where self-play's targets are shown.

## Steps

1. `game_labels::kept_count`, with tests on a fixture game: `Searched` all but the final position,
   `Quiet` fewer (a capture as best move, a position in check).
2. `Work::SelfPlay { keep, .. }`; `record` adds the game's kept count to `test_got` or `train_got`
   (the segment's `.meta` still counts every position read); PeSTO's play passes `None`.
3. `play`: the kept positions already held, counted from the games (decision 3); a helper
   `kept_in(dir, keep)`.
4. The texts (decision 5).
5. A test: self-play's `record` stops at kept positions, not positions read.
6. `cargo test -p smithy`, clippy; a scaled `-quiet` run from the page on this machine, through
   generation 2's self-play, its freeze and the first rounds of its training.
7. The real run: `-quiet`'s Resume into generation 2 on the training machine, the Convert's machine's
   arena allowed for self-play but not for the gate or the duels (an A/B measurement on an idle
   machine).

## Open

- Whether the gate against generation 1, a network 100 to 160 Elo below SCReLU, is worth reading
  beyond "self-play helped": the duel against `lc0-psq-threats-screlu-g1-r1` decides.
