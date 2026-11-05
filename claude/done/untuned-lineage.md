# Untuned lineage: self-play, round robins and gates play the default parameters

Status: done (2026-11-03).

## Decision (the developer, 2026-11-03)

Self-play, the forge's round robins and its gates play every network with the untuned
`SearchParameters::DEFAULT`, never with a `parameters.txt` beside the network. Tunes stay for what
plays on a clock: the network that ships, Lichess, and measuring a tune's gain by a test.

Why:

- A tune maximises Elo at 10+0.1; self-play searches to a fixed depth for labels. Margins tuned to
  trade accuracy for speed only make a fixed-depth search's labels worse.
- Each generation changes one thing, the network. A tuned teacher against an untuned challenger, or
  the reverse, would mix a parameter change into the gate.
- Nothing waits on a tune, and no run depends on whether one was started by hand.

## Today

`parameters: None` means "the network's own": `search::default_parameters_for` reads its tuning,
the `parameters.txt` beside it, else the defaults. So a forge plays tuned exactly when someone tuned
the network first:

- `smithy.rs` `deal`: self-play sends `Player { network: teacher, parameters: None }`; a gate's pair
  sends `[None, None]` when there is no test.
- `smithy.rs` `tournament_entries`: a round robin's pairs, `None` for both.
- `arena` `chores.rs` `check`: refuses self-play with explicit parameters.
- `arena` `self_play.rs` `play_one_game`: searches with `choose_best_move` (no parameters, so the
  network's own) and `choose_best_move_multipv_with_progress` (same, no parameters argument at all).

## Changes

1. **rotor `search.rs`**: `choose_best_move_multipv_with_progress` takes
   `parameters: Option<SearchParameters>`, passed to every rank's root search as
   `choose_best_move_with_progress` already does. Existing callers (UCI) pass `None`, unchanged.
2. **arena `self_play.rs`**: `play_game_pgn` and `play_one_game` take
   `parameters: Option<SearchParameters>`; the best-move search calls
   `choose_best_move_with_progress` with it, the exploration plies' MultiPV search passes it too.
   Tests pass `None`.
3. **arena `chores.rs`**: `check` no longer refuses explicit parameters for self-play; the game is
   played with `task.players[0].parameters`. Its doc comment says so. The test
   `self_play_is_played_to_a_depth_with_its_own_parameters` becomes one that self-play plays the
   parameters it is given.
4. **smithy `smithy.rs`**: one place says what a forge plays, a constant
   `FORGE_PARAMETERS: Option<SearchParameters> = Some(SearchParameters::DEFAULT)` with the
   decision's why in its comment, used by self-play in `deal`, a gate's pair in `deal` (the `None`
   arm) and `tournament_entries`. The doc comment of `test` (around line 719) and the test at
   `dealt.entries[0].players` (around line 5288) follow.
5. **Comments**: `Work::Pairs` and the `test` field say the forge's pairs play the defaults; the
   TODO's release step keeps "SPSA tune of the best network".

Tunes (`Work::Tune`, `tune.rs`) and tests (`TestDealing`) are untouched: they already give their
parameters explicitly.

## Compatibility

There is no protocol version. An arena built before this change refuses the new self-play chores
(`check`: "self-play with explicit search parameters") rather than playing them tuned, which is the
safe failure: pull and rebuild every arena. Pairs with `Some(DEFAULT)` were already accepted.

A run's games played before the change stay as they are: no tune of a forge network has been run
yet, so every forge game so far was already untuned.

## Tests

- `cargo test -p rotor` and `-p arena`: self-play with given parameters plays them (a margin that
  changes the move at a fixed depth from a chosen position, as `in_process.rs` does under a clock).
- `cargo test -p smithy`: self-play, a gate's pair and a round robin's entries carry
  `Some(SearchParameters::DEFAULT)`; a test's pairs still carry the test's.
