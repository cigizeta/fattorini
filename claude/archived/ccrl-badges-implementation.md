# CCRL badges: implementation plan

**Abandoned 2026-11-25:** superseded, see ccrl-badges.md.

Plan for ccrl-badges.md, its section "The Action" (designed 2026-11-25). The 40/15 badge only:
Blitz does not rate Fattorini yet.

## 1. `bin/ccrl-badge`

Bash, LF, executable, as the other scripts in `bin/`. Arguments:
`<list-dir> <label> <color> <out.json>`, as `4040 "CCRL 40/15" 56B4E9 badges/ccrl-4040.json`.

1. `set -euo pipefail`; four arguments or a usage message and exit 2.
2. curl `https://computerchess.org.uk/<list-dir>/cgi/compare_engines.cgi?family=Fattorini&print=Rating+list+%28text%29`
   with `--fail --silent --show-error --max-time 60` into a variable: curl failing ends the run.
3. Strip tags (`sed 's/<[^>]*>//g'`), then awk keeps lines matching
   `^ *Fattorini [0-9]+(\.[0-9]+)* 64-bit +[0-9]+ ` and prints version and rating.
4. Highest version: `sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1`.
5. No row: a message to stderr naming the URL, exit 1, nothing written.
6. Otherwise write the JSON through a temporary file and `mv` over `<out.json>`, so a failure
   never leaves it half written:
   `{"schemaVersion":1,"label":"<label>","message":"<rating> (<version>)","color":"<color>"}`.

## 2. `.github/workflows/ccrl-badges.yml`

- `on`: `schedule` daily at `17 5 * * *` (UTC, an off-hour minute GitHub's queue favours) and
  `workflow_dispatch`.
- `permissions: contents: write`; one job on `windows-latest`, `defaults.run.shell: bash` (Git Bash
  on GitHub's Windows images).
- Steps: `actions/checkout` at a pinned major version; `bin/ccrl-badge 4040 "CCRL 40/15" 56B4E9
  badges/ccrl-4040.json`; then, only if `git status --porcelain badges/` shows a change, commit as
  `github-actions[bot]` with the one-line message `CCRL 40/15 badge: <message>` and push.

## 3. `badges/ccrl-4040.json`

Committed once by the developer from a local run of the script, so the badge works before the
first scheduled run.

## 4. README

The badge block (`README.md`, under the title) split in two rows with `<br>`: release and license
first; CCRL 40/15 then Lichess blitz second. The new badge:

```html
<a href="https://computerchess.org.uk/4040/cgi/compare_engines.cgi?family=Fattorini&print=Rating+list"><img src="https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fcigizeta%2Ffattorini%2Fmain%2Fbadges%2Fccrl-4040.json&style=for-the-badge" alt="CCRL 40/15 rating"></a>
```

## 5. Checks (the developer)

1. `bin/ccrl-badge 4040 "CCRL 40/15" 56B4E9 badges/ccrl-4040.json` locally: the JSON reads
   `3227 (2026.11.0)`.
2. The same with `404`: exits 1, no file written (Blitz has no row yet), which tests the failure
   path for free.
3. Merged to `main`, the workflow run by hand from the Actions tab: green, no commit (unchanged).
4. The README on GitHub shows the badge.

## Then

TODO.md's README item updated; ccrl-badges.md and this plan to `done/` once the badge is seen.
