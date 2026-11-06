# `arena match`: 400 games by default, `--full` to play them all

**Status:** planned and applied 2026-11-06; tests and clippy to run. DEV.md's duel line also
corrected on the way (1000 games, stale since the 400-pair cap: 800).

## Why

A duel is capped at 400 pairs, 800 games; a match had no default, its n required. The developer
(2026-11-06): a match is half a duel, **400 games, 200 pairs, by default**. And a best of n stops
once the leader cannot be caught, so a lopsided match ends early, its score over the games played up
to the clinch. The release's strength table (binary-release.md, decision 10) wants every score over
the same games, so **`--full` plays all n** (the developer chose a flag over changing the rule).

The tournament's `best_of: 12` (`config.rs`, a knockout pairing with a tie-break) is another thing
and stays.

## The changes, all in `crates/arena`

1. **`config.rs`:** `Rule::BestOf(u32)` becomes `Rule::BestOf { n: u32, full: bool }`;
   `Rule::best_of(n, full)`; `spec_text` says `best-of 400`, or `best-of 400 full`, so a rerun with
   or without the flag is refused rather than resumed as the other.
2. **`best_of.rs`:** `BestOf::new(n, full)`. With `full`, `outcome` is `None` until n games, then
   decided as today (past n/2 wins, n/2 each drawn); `pairs_left_estimate` is the pairs left to n;
   `describe` says all n are played. Without it, nothing changes.
3. **`main.rs`:**
   - `MATCH_GAMES: u32 = 400`, beside the duel's `MAX_PAIRS` in spirit (half of it, in games).
   - The positional arguments are those before the first `--` flag: `match` takes two or three
     (n optional, `MATCH_GAMES` when absent), `sprt` three, `duel` two.
   - `--full` takes no value: taken out of the flags before `parse_flags` pairs them, accepted for
     `match` only, refused at most once and for `sprt` and `duel`.
   - The usage: `arena match <champion> <challenger> [<n>] ... [--full]`, n 400 by default, and
     `--full` explained; the module header likewise.
4. **`DEV.md`:** `match <a> <b> [<n>]`, 400 by default, and `--full`.

## Tests

- `match a b --depth 9` parses as `BestOf { n: 400, full: false }` (it was refused; taken out of
  `each_command_refuses_anything_else`).
- `match a b 12 --depth 9 --full` and `match a b --full --depth 9` parse with `full: true`.
- Refused: `--full` twice; `--full` with `sprt` or `duel`; `--full` given a value is just the
  value read as an unknown argument, refused.
- The spec line: `best-of 400 full at 10+0.1`.
- `best_of.rs`: a full best of 4 whose challenger wins the first three games is not decided until
  the fourth; a full match's pairs left are the pairs to n.

## Check (the developer)

```
cargo test -p arena
```

```
cargo clippy --workspace --all-targets
```

## Done when

Tests and clippy pass. Then this document goes to `done/`.
