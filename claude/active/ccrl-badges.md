# CCRL badges in the README

Status: design agreed, not implemented. Waits for Fattorini's first CCRL listing (the 40/15 list
expected on 2026-11-27, from a 12'+8" tournament of 44 games).

## The problem

CCRL rates an engine on several lists (40/15, Blitz), each in several configurations (1 CPU, 4 CPU,
8 CPU), and each version of the engine separately. One badge cannot show all of it, and a row of
badges for every combination would be unreadable.

## Decisions

- **Badges show single-CPU ratings only:** the configuration every engine is compared on, and
  Fattorini's default (`Threads` 1). Whether Fattorini will ever be tested at 4 or 8 CPU is unknown;
  nothing is designed for it.
- **Each badge is added only once its list rates Fattorini:** CCRL 40/15 first, CCRL Blitz later.
- **Exactly two badges, ever (the developer, 2026-11-20):** CCRL Blitz and CCRL 40/15, single CPU,
  never more. Multi-CPU ratings are never shown: claiming a strength at 8 threads is not a fair
  comparison.
- **Each badge shows the newest rated version,** with the version in the badge, as
  `CCRL 40/15 | 3210 (2026.11.0)` (number illustrative). The newest version is meant to be the
  strongest, and it is easy to compute: version names (`YYYY.M.N`) sort by date, so it is the
  highest version CCRL has rated on that list (comparing the numbers, not the strings). A new
  version takes over the badge on its first rating, even on few games, and even in the unlikely
  case that its rating is lower than the previous version's (the developer, 2026-11-20): the badge
  shows the engine people download. If CCRL's engine-family
  view lists every version on one page, the Action reads that page and its URL never changes; to
  be checked.
- **No badge for Fattorini's own estimate:** a badge reads as official. The estimate stays in the
  Strength section's text, marked as an estimate, until CCRL rates Fattorini on Blitz.
- **No CCRL table in the README (the developer, 2026-11-20):** a table would need hand edits at
  each list update. The badges carry CCRL's numbers; CCRL's own pages, which each badge links to,
  carry the detail (versions, margins, games).
- **Live badges, no hand edits (the developer, recorded in TODO.md):** a daily GitHub Action reads
  Fattorini's CCRL engine page for each list (`engine_details.cgi`, or its `Details (text)` view,
  where the rating reads "#rank (rating +e -e)") and writes a JSON file for a shields.io endpoint
  badge. If the Action reads per-version pages instead, their engine names change at each release. If no rating is found (CCRL's page changed),
  the Action keeps the last good value and fails, so GitHub's failed-workflow e-mail reports it and
  the badge never shows a wrong number. GitHub disables a scheduled workflow after 60 days without
  activity in a public repository, warning by e-mail first. Each badge links to Fattorini's entry on
  that list. To be designed in detail once the name CCRL uses is known.

## Strength section

Unchanged until CCRL rates Fattorini on Blitz: the estimate and how it was measured. The 40/15
badge is never presented as confirming or contradicting the Blitz estimate: the scales differ.

## Badge row

With up to two CCRL badges the row reaches five, crowded at `for-the-badge` size. Proposed: two
rows, release and license on the first, the ratings (CCRL 40/15, CCRL Blitz, Lichess blitz) on the
second. Colours continue the palette already in use (`0072B2`, `009E73`, `E69F00`): `56B4E9` for
40/15, `CC79A7` for Blitz.

## Steps, once the first listing appears

1. Read Fattorini's 40/15 entry: the engine name CCRL uses, rating, margin, games, and the URL.
2. Design the Action in detail (where the JSON lives, the parsing, the failure path) in this
   document, then implement it.
3. Add the CCRL 40/15 badge and split the badge row in two.

## Open

- Two badge rows, or one row of five: the developer to confirm.
- The two colours: the developer to confirm.
- The Strength section once CCRL rates Fattorini on Blitz: keep the estimate as history, or remove
  it.
