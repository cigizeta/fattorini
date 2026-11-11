#!/usr/bin/env bash
# Downloads arena's opening book into the data root's books/ directory, regardless of where this
# script is invoked from. The data root is the repository's gitignored data/. The same rule is in
# src/pipeline/data_root.rs, and the two must agree.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_ROOT="$REPO_ROOT/data"
BOOKS="$DATA_ROOT/books"

# UHO_Lichess_4852_v1: Unbalanced Human Openings, positions from real Lichess games in which one side
# already has a clear edge, so that close engines play decisive games rather than draws; with colours
# reversed per opening, neither engine profits from the edge. The book the Stockfish project itself
# uses for its engine-strength testing. Source: https://github.com/official-stockfish/books
# (CC0-1.0).
URL="https://github.com/official-stockfish/books/raw/refs/heads/master/UHO_Lichess_4852_v1.epd.zip"
ARCHIVE="$BOOKS/UHO_Lichess_4852_v1.epd.zip"

mkdir -p "$BOOKS"
curl -fL -o "$ARCHIVE" "$URL"
unzip -o "$ARCHIVE" -d "$BOOKS"
rm "$ARCHIVE"
