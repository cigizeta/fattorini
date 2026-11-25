# CCRL badges in the README

**Abandoned 2026-11-25 (the developer, "give up"):** GitHub disables a scheduled workflow after 60
days without commits, which a mature project will reach. A keepalive commit, the API's re-enable,
an external converter and a link-only badge were all declined. The badge and its workflow removed;
see TODO.md.

Status: design agreed, the Action designed 2026-11-25; plan in ccrl-badges-implementation.md. CCRL's 40/15 list of 2026-11-20 already rates Fattorini
2026.11.0 (3227 +80 -80, 31 games of a 44-game 12'+8" tournament). The 40/15 badge goes into the
README tomorrow (the developer, 2026-11-20).

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
  `CCRL 40/15 | 3227 (2026.11.0)`. The newest version is meant to be the strongest. A new version
  takes over the badge on its first rating, even on few games, and even in the unlikely case that
  its rating is lower than the previous version's (the developer, 2026-11-20): the badge shows the
  engine people download.
- **No badge for Fattorini's own estimate:** a badge reads as official. The estimate stays in the
  Strength section's text, marked as an estimate, until CCRL rates Fattorini on Blitz.
- **No CCRL table in the README (the developer, 2026-11-20):** a table would need hand edits at
  each list update. The badges carry CCRL's numbers; CCRL's own pages, which each badge links to,
  carry the detail (versions, margins, games).
- **Live badges, no hand edits (the developer, recorded in TODO.md):** a daily GitHub Action writes
  a JSON file for a shields.io endpoint badge, one per list. Each badge links to the family page.

## The page the Action reads (checked 2026-11-20)

**The family page,** one URL per list that never changes, listing every rated version with its
rating and margin (Blitz's is the same under `/404/`):

```text
https://computerchess.org.uk/4040/cgi/compare_engines.cgi?family=Fattorini&print=Rating+list
```

Seen on Myrddin's (18 versions, rows as "Myrddin 0.96 64-bit, 3258, +15 -15"). It already works
for Fattorini's single version ("Comparing 1 engines!"), although the per-version page links to it
only from the second version on. The plain-text variant, `print=Rating+list+%28text%29`, gives one
line per version, the easiest to parse: `Fattorini 2026.11.0 64-bit  3227  +80  -80  40.3%  +60.9
[31]` (name, rating, margins, score, average opponent, games).

- **Only single-CPU rows count:** the family page mixes in `4CPU` entries (Myrddin 0.88 both plain
  and `4CPU`) and `32-bit` builds. The Action keeps rows named `Fattorini <version> 64-bit` exactly.
- **The newest rated version** is the highest `<version>` among them, compared as numbers
  (`2026.9.0` before `2026.11.0`). Its rating and version go to the JSON. A release CCRL has not
  tested is simply absent, so the badge stays on the previous version with no special case.
- **A page that yields no Fattorini row** (CCRL changed its layout): the last value is kept and the
  run fails, so GitHub's failed-workflow e-mail reports it and the badge never shows a wrong
  number.
- **Scheduled workflows run on the default branch only** (GitHub's rule): the Action and its JSON
  live on `main`, and the endpoint URL points there, whichever branch's README shows the badge.
- GitHub disables a scheduled workflow after 60 days without activity in a public repository,
  warning by e-mail first.

## Strength section

Unchanged until CCRL rates Fattorini on Blitz: the estimate and how it was measured. The 40/15
badge is never presented as confirming or contradicting the Blitz estimate: the scales differ.

## Badge row

With up to two CCRL badges the row reaches five, crowded at `for-the-badge` size. Proposed: two
rows, release and license on the first, the ratings (CCRL 40/15, CCRL Blitz, Lichess blitz) on the
second. Colours continue the palette already in use (`0072B2`, `009E73`, `E69F00`): `56B4E9` for
40/15, `CC79A7` for Blitz.

## Decided 2026-11-25 (the developer)

- **The badge links to the family page** (`compare_engines.cgi?family=Fattorini`, one fixed URL per
  list), not a version's `engine_details.cgi`, which would go stale at the next release.
- **Two badge rows:** release and license first; the ratings (CCRL 40/15, later CCRL Blitz, Lichess
  blitz) second.
- **CCRL 40/15 is `56B4E9`**, `CC79A7` kept for Blitz.

## The Action (designed 2026-11-25)

Checked that day: the 40/15 family page rates 2026.11.0 at 3227 +80 -80, 31 games; Blitz's says
"No engines selected!". The "text" variant is HTML, the row inside it reading
`Fattorini 2026.11.0 64-bit  3227  +80  -80  40.3%  +60.9  31`.

- **Workflow:** `.github/workflows/ccrl-badges.yml` on `main`, daily on a `schedule` and by hand
  (`workflow_dispatch`), on `windows-latest` under Git Bash, as every script here.
- **Script:** `bin/ccrl-badge <list-dir> <label> <color> <out.json>`, Bash, runnable locally too.
  It fetches the family text page with curl, strips tags, keeps lines matching
  `^ *Fattorini ([0-9.]+) 64-bit +([0-9]+) ` (dropping `4CPU` and `32-bit`), picks the highest
  version compared field by field as numbers, and writes
  `{"schemaVersion":1,"label":"CCRL 40/15","message":"3227 (2026.11.0)","color":"56B4E9"}`.
- **Output:** `badges/ccrl-4040.json` on `main`, committed by `github-actions[bot]` only when it
  changes. Blitz gets `badges/ccrl-404.json` and its workflow step once CCRL rates Fattorini there.
- **Failure:** no matching row, or curl failing, exits non-zero before writing, so the JSON keeps
  its last value and the failed run's e-mail reports it.
- **README:** a shields.io endpoint badge reading the JSON through raw.githubusercontent.com from
  `main`, linking to the family page.

## Steps

1. Design the Action in detail in this document: where the JSON lives on `main`, the parsing of the
   text page, the failure path, the schedule.
2. Implement it; run it once by hand (`workflow_dispatch`) and check the JSON.
3. Add the CCRL 40/15 badge to the README, linking to the family page, and split the badge row.

## Open

- The Strength section once CCRL rates Fattorini on Blitz: keep the estimate as history, or remove
  it.
