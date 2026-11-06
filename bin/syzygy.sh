#!/usr/bin/env bash
# Downloads the Syzygy tables into the data root's chess/syzygy/ directory, regardless of where this
# script is invoked from. The data root is the repository's gitignored data/. The same rule is in
# src/pipeline/data_root.rs, and the two must agree.
#
# Downloads both parts of Syzygy, 3 to 6 pieces: WDL (.rtbw), complete on its own, and DTZ (.rtbz),
# an extra that is useless without WDL beside it. Nothing is ever deleted, so rerunning is safe.
#
# curl, not wget: Git Bash ships only curl. A table downloads to <file>.part, resumed from where it
# stopped (curl -C -), and is renamed only once complete. A table already under its final name is
# kept when its size matches the server's; otherwise it is an interrupted download from before the
# .part names (wget wrote straight to the final name), and is moved back to .part and resumed. The
# engine loads only .rtbw and .rtbz files, never a .part.
#
# Never run while a Kalo Chess process has the tables open: Kalo Chess reads them through memory maps, and a
# table resumed under its final name would change under the map.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_ROOT="$REPO_ROOT/data"
SYZYGY="$DATA_ROOT/chess/syzygy"

BASE="https://tablebase.lichess.ovh/tables/standard"

# The server times out now and then: every request retries a timeout or a transient error 5 times,
# 30 s apart, so only a long outage stops the script.
RETRY=(--retry 5 --retry-delay 30)

# The size the server gives for $1, in bytes: the last Content-Length, after any redirect.
remote_size() {
    curl "${RETRY[@]}" -fsSIL "$1" | tr -d '\r' | awk 'tolower($1) == "content-length:" { size = $2 } END { print size }'
}

# Downloads $1 to $2 unless $2 is already complete.
download_table() {
    local url="$1"
    local path="$2"

    if [ -f "$path" ]; then
        local size
        size="$(remote_size "$url")"
        # No size, the server unreachable: stop rather than take the table for incomplete and move it
        # out of the engine's sight.
        if [ -z "$size" ]; then
            echo "no size from the server for $(basename "$path"): is $BASE reachable?" >&2
            exit 1
        fi
        if [ "$(stat -c %s "$path")" = "$size" ]; then
            return
        fi
        echo "Resuming the incomplete $(basename "$path")"
        mv "$path" "$path.part"
    else
        echo "Downloading $(basename "$path")"
    fi
    curl "${RETRY[@]}" -fL -C - -o "$path.part" "$url"
    mv "$path.part" "$path"
}

download_tables() {
    local set="$1"
    local extension="$2"
    local directory="$SYZYGY/$set"

    mkdir -p "$directory"
    # A complete table prints nothing, and checking one costs a request to the server: say which set
    # is being checked, or a run over complete sets looks hung.
    echo "Checking $set"

    curl "${RETRY[@]}" -fsSL "$BASE/$set/" |
        grep -oE "href=\"[^\"]+\\.$extension\"" |
        sed 's/^href="//; s/"$//' |
        while read -r file; do
            download_table "$BASE/$set/$file" "$directory/$file"
        done
}

download_tables "3-4-5-wdl" rtbw
download_tables "6-wdl" rtbw
download_tables "3-4-5-dtz" rtbz
download_tables "6-dtz" rtbz
