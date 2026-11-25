#!/usr/bin/env bash
# Renders the logo's raster files from its SVGs, run by hand whenever a logo changes, never by a
# build: docs/logo/fattorini.ico, the icon crates/fattorini/build.rs embeds in fattorini.exe, and
# docs/logo/social-preview.png, GitHub's social preview (uploaded by hand, Settings > General). Both are
# committed. Needs resvg (`cargo install resvg`) and Pillow in the active .fattorini venv.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
OUT="target/logo"
mkdir -p "$OUT"

command -v resvg > /dev/null || { echo "resvg not found: cargo install resvg" >&2; exit 1; }
python -c "import PIL" 2> /dev/null || { echo "Pillow not found: activate the .fattorini venv, uv pip install pillow" >&2; exit 1; }

# The icon's sizes, each rendered from the SVG at its own size rather than downscaled from one.
SIZES=(16 32 48 256)
for size in "${SIZES[@]}"; do
    resvg -w "$size" -h "$size" docs/logo/logo-light-small.svg "$OUT/icon-$size.png"
done
python - "$OUT" docs/logo/fattorini.ico "${SIZES[@]}" <<'EOF'
import sys
from PIL import Image

out, ico, sizes = sys.argv[1], sys.argv[2], [int(s) for s in sys.argv[3:]]
images = [Image.open(f"{out}/icon-{s}.png").convert("RGBA") for s in sizes]
# The largest is the base; the others are given as they are, so Pillow resamples none of them.
images[-1].save(ico, format="ICO", sizes=[(s, s) for s in sizes], append_images=images[:-1])
EOF

resvg -w 1280 docs/logo/social-preview.svg docs/logo/social-preview.png
echo "written: docs/logo/fattorini.ico, docs/logo/social-preview.png"
