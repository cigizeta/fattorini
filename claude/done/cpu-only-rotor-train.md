# smithy builds on a Windows machine without a GPU

Decided with the developer 2026-10-06 (solution 1 of two; the other, moving `install` out of
`smithy`, was not taken). **Status:** done; `smithy install` builds and runs on the Lichess machine (checked
2026-10-14).

## The problem

On the Lichess machine (Windows, no GPU) `bin/smithy` fails to link:

```
LINK : error LNK2001: unresolved external symbol "int __cdecl at::cuda::warp_size(void)"
```

`build.rs` forces that symbol into `smithy` on every MSVC build, so that the linker keeps
`torch_cuda.dll` (otherwise `cuda_if_available()` silently falls back to the CPU). On a CPU-only
torch wheel there is no `torch_cuda.lib`, so nothing defines the symbol. `uv pip install
--torch-backend=auto` rightly installs the CPU wheel on a machine with no NVIDIA driver. The Lichess machine needs
`smithy` only for `install` (the engine, champion and book for lichess-bot), never to train.

Linux already handles the same case: `bin/smithy` preloads `libtorch_cuda.so` only when it
exists.

## What the code settles

- **torch-sys does not tell us the directory on Windows.** It prints `cargo:libtorch_lib=...`
  (`DEP_TCH_LIBTORCH_LIB` for direct dependents) only on Linux and macOS (torch-sys 0.24.0
  `build.rs`, line 378), and Rotor depends on `tch`, not on `torch-sys` directly, anyway.
- **The venv's path is fixed by convention.** `bin/smithy` refuses to run unless
  `VIRTUAL_ENV` is this repository's `.rotor`, and on Windows torch's libraries are in
  `.rotor/Lib/site-packages/torch/lib` (the same glob `bin/smithy` uses).
- **torch-sys decides by the same file.** It links `torch_cuda` when `torch_cuda.dll` exists in
  that directory (its `build.rs`, line 447). Checking the same file keeps the two decisions in step:
  the `/INCLUDE` is passed exactly when `torch_cuda` is linked.

## The change

`build.rs` only:

1. Under `training` and MSVC, compute `$CARGO_MANIFEST_DIR/.rotor/Lib/site-packages/torch/lib`.
2. `cargo:rerun-if-changed` on that directory, so that reinstalling torch (CPU wheel to CUDA wheel
   or back) reruns the script. A directory's mtime changes when files are added or removed in it.
3. Pass `/INCLUDE:?warp_size@cuda@at@@YAHXZ` only if `torch_cuda.dll` exists there.
4. Update the module comment: the symbol is forced in only with a CUDA torch; a CPU-only torch
   (a machine without a GPU) builds a CPU-only `smithy`, as `bin/smithy`'s Linux preload
   already does.

No change to `bin/smithy`, `Cargo.toml` or any dependency.

**The risk:** if the check is ever wrong on the training machine, the symbol is dropped silently and training runs
on the CPU. The guard is the device `smithy` reports when training starts; check it on
the training machine's next training run.

## Verify

On the Lichess machine (CPU-only torch):

```
cargo clean --release -p rotor
```

```
bin/smithy
```

It builds and prints its usage. Then `bin/smithy install amanda`.

**The Lichess machine: verified 2026-10-06.** `smithy` built against the CPU-only torch and installed
amanda's champion. Left: the training machine.

On the training machine, at its next build after this change: `smithy` still trains on CUDA. Not before
betty's training ends: nothing rebuilds on the training machine while it runs.
