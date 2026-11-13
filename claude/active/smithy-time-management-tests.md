# Smithy tests with a time management per side

Status: **design settled** with the developer 2026-11-13; steps 1 to 8 done the same day
(`cargo test -p fattorini -p arena -p smithy --release` and `cargo clippy --workspace
--all-targets` clean). Step 9 started 2026-11-13: test 5, "duel: champion
lc0-psq-threats-screlu-g1-r1@tuned@classic against challenger
lc0-psq-threats-screlu-g1-r1@tuned@reckless", clock 180+2, the form and its words seen working
on the page (the clock presets restyled as links that day: as buttons they read as fields).

## Why

The developer, 2026-11-13: the 180+2 duel of untuned Reckless against Classic
(reckless-time-management.md) should run as a smithy test, so every arena plays it. Today a
smithy test's players are networks played in-process (`InProcessPlayer`), whose time management
is `Split`, hard-coded, at the 20 ms default overhead: a test cannot say which management a side
plays. The same wiring is the tune's first piece of groundwork (an in-process Reckless).

## Design

- **A time management per player, `Split` when unsaid.** Every chore, test record and job written
  before this reads as today; nothing changes for self-play, the gate, round robins or tunes.
- **The protocol** (protocol.rs): a chore's `Player` gains `time_management`. Its text, today
  `<network>\t<parameters>`, takes an optional third field, `\t<Classic|Split|Reckless>`, written only
  when not `Split`. An arena of an older build refuses a chore with it rather than playing it
  wrong; every arena needs this build anyway, so both sides of a game run the same code.
- **The arena** (in_process.rs): `InProcessPlayer::new` takes the management; under a clock it
  plans with it, at that management's own default overhead (`default_move_overhead_ms`: Reckless
  100 ms, the others 20 ms), and with the game's full-move number, which `MoveRequest` gains from
  game.rs's `fullmove_number`. Every other caller (pairing.rs, the tests) passes `Split`, as today.
- **The smithy's test words** (matches.rs `Side`): `<network>@<tuned|untuned>`, optionally
  followed by `@<classic|split|reckless>`, absent meaning `split`: `pesto@untuned@reckless`. The
  side's text names it only when not `split`, so the log's older lines read back unchanged. A
  test of one network against itself, differing by management alone, is what this duel is; it is
  not "mixed" (that flag stays about tuned against untuned).
- **The smithy's dealing** (smithy.rs): a test's two `Player`s carry their sides' managements.
- **The page** (page.html): beside each side's tuned/untuned select, a "time" select, Split
  (the default), Classic, Reckless.
- **Measurement:** each game plays both sides on one arena, as every test. A test's clock is
  scaled to each arena's speed against the smithy's (`reference_nps`, as a tune's; corrected
  2026-11-13 from "not scaled", found in `Shared::deal`), so a slower arena plays a longer 180+2 at
  about the same depth, both sides alike.

## Steps

1. **protocol.rs:** `Player.time_management`, its text's optional third field, read and written;
   every `Player { .. }` literal given `Split`.
2. **in_process.rs:** `InProcessPlayer::new(.., time_management)`, its overhead and full-move
   number; `MoveRequest.fullmove` (player.rs), filled by game.rs.
3. **chores.rs:** the pair's two players built with their chores' managements; pairing.rs and the
   tests pass `Split`.
4. **matches.rs:** `Side.time_management`, parsed and written as above.
5. **smithy.rs:** a test's `Player`s carry it.
6. **page.html:** the "time" select per side, into the side's words.
7. **Unit tests:** `Player` text with and without the field, an unknown management refused;
   `Side` with and without it, older lines read back; an in-process Reckless planning with
   100 ms and the full-move number.
8. **`cargo test`** of fattorini, arena and smithy, and **`cargo clippy --workspace
   --all-targets`**, all clean; the form tried on the page.
9. **Every arena on this build,** then the test: a duel, Classic against Reckless, the same
   network and tune on both sides (open question 1), `clock 180+2`. "Untuned" in
   reckless-time-management.md is about the time constants, not the search's.

## Decided

1. **The network:** the one Lichess plays, `lc0-psq-threats-screlu-g1-r1`, with its tune, on both
   sides (the developer, 2026-11-13): the result is a Lichess improvement.
