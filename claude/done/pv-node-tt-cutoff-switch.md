# The PV-node TT cutoff as a temporary switch

**Status: done and removed, 2026-11-16.** Test 10 played (pv-node-tt-cutoff.md: drawn, the change
kept); the switch then removed from the search, the chores, the smithy and the page, `+pvcut`
refused from then on as `+blend` is. Test 10's own record no longer parses, accepted (the developer,
2026-11-16: it ages out of the kept tests by itself).

Agreed and implemented 2026-11-16 (the developer: "No need for UCI"); tests and clippy
to run, then the duel. Where it differs from the steps below: the page's tests never pass through
`match_player`, so the arena's command line was left alone (it refuses `+pvcut` as an unknown
suffix); the switch reaches the arenas in the chore's player text instead, as a `pvcut=1` pair
(`protocol::Player`), which an arena a build behind refuses; the page has a two-option select,
not a checkbox (the form's handler reads `value`); and book scoring refuses a `+pvcut` side, its
scores file named by player and tuning alone. Rewrites the implementation
of pv-node-tt-cutoff.md (the developer, 2026-11-16: "an option to use either setting") so that its
no-harm-check duel runs from the page's tests screen, one commit on every arena, in-process.

## Why

The page's tests play one commit in-process, with no per-side choice beyond the tuning suffix: the
search switches it once had were removed on 2026-11-02 (`matches.rs`, "The search switches"). So
new against old needs either two binaries from the command line, or one binary that plays both.

## Decisions to agree

1. **One switch, temporary, named `pvcut`:** the old behaviour, a TT cutoff at PV nodes too. The
   new behaviour (null-window nodes only) is the default, so `@search-time-tuned` alone is the new
   side, and the old side is `@search-time-tuned+pvcut`.
2. **Written after the suffix with `+`,** as the switches of before 2026-11-02 were
   (`aa@untuned+blend`): `<network>@<suffix>+pvcut`. Read by the smithy (`matches::Side`), the
   arena (`match_player::split_suffix`) and so the tests list, the PGN and the database, which
   name a side by its text. Any other word after `+` is refused.
3. **Carried in `SearchParameters`,** as a `bool` field `pv_tt_cutoff`, `false` in `DEFAULT`: that
   struct already reaches every in-process search from the player's name, so no new plumbing. Not
   a tuned value: never written to or read from `search-parameters.txt`, never perturbed by SPSA
   (to check in `spsa.rs` and `SearchParameters`'s file code that every field is listed by name).
4. **The search:** the cutoff's guard becomes `(beta - alpha == 1 || params.pv_tt_cutoff)`.
5. **The page:** a `pvcut` checkbox beside each side's suffix on the tests form, appending `+pvcut`
   to that side's text. The tests list shows the side's text as it is.
6. **Not over UCI:** no UCI option, and an arena player over UCI (against an external engine, or a
   snapshot) refuses a `+` switch, since `snapshot::parameter_files` writes only the tuned values.
   The release never sees it.
7. **Removed after the verdict,** with the parse of `+` words kept as a refusal (as today's
   `aa@untuned+blend` test), whichever side wins: if the duel shows harm, the old behaviour comes
   back as the only one.

## Steps

1. `search.rs`: the field, `DEFAULT`, the guard; the field excluded from the parameter file's read
   and write, and from SPSA; tests: the switch on cuts off at a PV window, off does not (today's two
   tests, a third with the switch).
2. `players.rs` or `match_player.rs`: `+pvcut` parsed after the suffix, setting the field on the
   in-process entry's parameters; refused for a UCI player. Tests.
3. `matches.rs`: `Side` gains `pvcut: bool`, its `text` and `parse`; `mixed` unaffected. The
   2026-11-02 refusal test keeps `+blend`, and gains `+pvcut` accepted.
4. `page.html`: the checkbox per side.
5. Tests and clippy, then the duel from the page: challenger
   `lc0-psq-threats-screlu-g1-r1@search-time-tuned`, champion
   `lc0-psq-threats-screlu-g1-r1@search-time-tuned+pvcut`, 10+0.1, every arena on the commit.
