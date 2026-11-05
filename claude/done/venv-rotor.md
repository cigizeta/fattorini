# The training venv is `.rotor`, not `.venv`

`.venv` is every Python project's default name, so an activated `.venv` in a shell prompt or a
`VIRTUAL_ENV` path says nothing about which project it belongs to. This repository's venv is
`.rotor` instead.

## Changes

- `bin/rotor-train`: `VIRTUAL_ENV` must be `$REPO_ROOT/.rotor`; torch's `lib/` is globbed under
  `.rotor`; comments and error messages name `.rotor`. `libtorch_cuda.so` is preloaded only when it
  exists, so a CPU-only machine (a laptop with the CPU wheel) runs it without a loader error.
- `.cargo/config.toml`: the runner's `LD_LIBRARY_PATH` points at `.rotor/lib/python3.14/...`.
- `README.md`: `uv venv .rotor`, `source .rotor/bin/activate` before `uv pip install` (uv finds a
  venv by itself only when it is named `.venv`, so it must be active first), the verification with
  plain `python` instead of `uv run python` (`uv run` looks for the project's `.venv`), the build
  line and the `LD_PRELOAD` alias.
- `CLAUDE.md`: `bin/rotor-train` requires this repository's `.rotor` to be active.

A `.venv` left from before is not used by anything and can be deleted.

## Verification

`source .rotor/bin/activate`, install torch, then `bin/rotor-train` with no arguments: it builds and
prints its usage, and without the venv active it refuses with the `.rotor` message.
