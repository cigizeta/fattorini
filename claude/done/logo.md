# Fattorini's logo

**Status:** 2026-11-20: `logo/` written (`logo-{light,dark}-{large,small}.svg`,
`social-preview.svg`); steps 1 (README) and 2 (smithy's page) done and
checked in a browser; steps 3 and 4 written (`bin/logo.sh`, `build.rs`), the build and the upload
to check.

## The drawing

Antonio Fattorini's firm in Bradford made early chess clocks of the tumbling kind: two clocks
standing on a beam that rocks on a pivot, tipping it stops one clock and starts the other (who
invented it, Fattorini or Thomas Wilson, is to be checked against a source before any public text
says so). The
logo is that clock, simplified from a photo of one:

- two round dials, cream, a dark rim, each inside an open **pentagon** frame, point up, its two
  lower corners standing on the beam, a round knob at the apex and at each shoulder (the photo's
  scrollwork, which would blur small);
- the **beam bent at the pivot** like a flat roof, each arm with its clock sloping 8 degrees down
  from the middle; the whole tipped 7 degrees, so the left arm lies almost flat and the right one
  drops about 15 degrees, as in the photo. The tilt is what says "chess clock";
- a triangular pivot and a rounded brass base, both level;
- each dial its own time, hour and minute hands only.

No fonts anywhere: every mark is a path or a stroke, so the logo renders the same everywhere. No
wordmark for now (a "FATTORINI" on the base, as on the original, would have to be drawn as paths).

Colours, from the photo: body `#2f4a3a` (verdigris green), dials `#f4efe2` (cream), base `#b8893b`
(brass).

## Two variants

- **Large** (`logo-{light,dark}-large.svg`): the dials carry XII, III, VI and IX, drawn as strokes (I, V and X are
  only lines). For the README and anything shown at 200 px or more.
- **Small** (`logo-{light,dark}-small.svg`): four thick ticks instead of the numerals, thicker hands. For
  anything under about 100 px: favicon, the `.exe`'s icon. Square canvas, the drawing centred, since
  icons are square.

Both live in a new `logo/` folder at the repository root.

## Where it goes, in order

1. **README.** The large logo under the title, by relative path (`logo/logo-light-large.svg`, `logo-dark-large.svg` for a dark theme); GitHub renders
   SVG in Markdown.
2. **Favicon of smithy's page** (`crates/smithy/src/page.html`, the only progress page: arena has
   none). The small logo inline as a `data:image/svg+xml` link in `<head>`: no new route, no file
   served, one line.
3. **GitHub's social preview.** A PNG, 1280x640, the large logo centred with margins
   (`logo/social-preview.png`, from `logo/social-preview.svg`, named after GitHub's setting).
   Uploaded by hand under the repository's Settings;
   GitHub takes no SVG there.
4. **The `.exe`'s icon.** `logo/fattorini.ico`, holding 16, 32, 48 and 256 px images of the small
   logo, compiled into `fattorini.exe` as a Windows resource by `crates/fattorini/build.rs`.
5. **TCEC**, only when Fattorini is invited: a PNG of the large logo at the size TCEC asks for.
   Nothing to do now.

Not applicable: Lichess has no profile pictures, CCRL's lists no logos.

## Step 4, decided (the developer, 2026-11-20)

- **Rendering:** `bin/logo.sh`, run by hand whenever a logo changes, never by a build. `resvg`
  renders `logo-light-small.svg` at 16, 32, 48 and 256 px into `target/logo/`, and Pillow, from the
  `.fattorini` venv, packs the four PNGs into `logo/fattorini.ico`, each size from its own
  rendering, not downscaled from one; `resvg` renders `social-preview.svg` at 1280 px to
  `logo/social-preview.png`. Both outputs committed, the PNGs in `target/` not.
- **Embedding:** `embed-resource` as a build-dependency of `fattorini`. `build.rs` writes a
  one-line `.rc` (`1 ICON "<path of fattorini.ico>"`) into `OUT_DIR` and compiles it; the path is
  absolute, from `CARGO_MANIFEST_DIR`, so `rc.exe`'s search for relative paths does not matter.
  Every build carries the icon, development, snapshot and release alike.
- **Checks:** a dev build shows the icon in Explorer; `arena release --dry-run` still passes (the
  C runtime static, the notices: a build-dependency never reaches the binary, so `cargo-about`'s
  list should not change).

## Licence, decided (the developer, 2026-11-20)

The logo stays AGPLv3 like the code, with one additional term under section 7(e), in the README's
License section: no right to use the name "Fattorini" or the logo for a modified version. Forks
are welcome under another name, without the logo, `id name` and the `.exe`'s icon included.

## Background and colouring, decided

- **Background: decided (the developer, 2026-11-20).** The logo owns its background: one look on
  light and dark pages alike. Rounded corners, transparent outside the curve (the developer,
  2026-11-20): radius 20 on the large logos' 240 width, 40 on the small one's 230.
- **Colouring: decided (the developer, 2026-11-20).** Where the viewer's theme can be detected,
  dark gets **green** (green background, brass frames and beam, cream dials) and light gets
  **cream** (cream background, green frames and beam, white dials); where it cannot, cream. So:
  the README switches through `<picture>` with `prefers-color-scheme`, smithy's page's logo beside its title
  through its CSS; the favicon is cream alone (a light browser showed the green one: `media` on
  an icon link is not reliably honoured); the `.exe`'s icon, the social preview and TCEC are cream.
- **Four files, no `@media` in any (the developer, 2026-11-20):** every renderer must draw a file
  the same, so each size and colouring is its own plain file, `logo-{theme}-{size}.svg` (the
  developer: `light` or `dark`, `large` or `small`), and the switching happens outside them.
