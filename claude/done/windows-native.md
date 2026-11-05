# Rotor on native Windows, Linux kept

**Status:** done 2026-10-14: every machine runs native Windows, `smithy install` has run there many
times (item 7) and the training machine trains with CUDA on Windows (item 8). Linux was dropped as a target on
2026-10-08, after this document's title. Earlier status:

`bin/arena self hand-crafted` runs on Windows (the training machine), beside the Lichess machine under
WSL: items 1, 2, 3 and 6 verified together. Item 5 verified: Ctrl+C during a timed match on
Windows behaved as expected. Still to verify: CUDA at the first `train` on Windows (item 8),
`install` over a running engine (item 7). the Lichess machine moving from WSL to Windows since 2026-10-05:
cloned, scripts LF; its WSL `data/` and data root to be copied over rather than left behind, since
a generating machine's `data/self/` holds the staging file of its stopped batch and the kept
`.count`s. The rest of this line is the history.

Earlier status: design agreed; the order below is settled. Item 1 (data root) done and verified on
Windows. Item 2 (remote root) done. Item 3 (memory guard) done. Item 6 (executable names) done.
`bin/syzygy.sh` moved to curl (Git Bash has no wget); the WDL tables copied from WSL, DTZ
downloading. Item 6b done: a timed
match ran to the end on Windows. Item 4 seen working (one `rotor.exe` pinned to CPU 6 alone); the
others were not checked. Item 5 applied but not tested by hand: Ctrl+C during a timed match, both
stages, is still open. Item 7 applied, to be verified with item 8 (`smithy` does not build
on Windows before it). Item 8 applied (`build.rs` for the CUDA link argument, `bin/smithy`'s
Windows venv layout and PATH), to be verified: venv with torch 2.11.0 for CUDA, then `smithy`
reporting CUDA. A recent NVIDIA GPU and driver need a cu128 or newer wheel. `cargo test` for `smithy` on Windows goes through the Windows `runner` in
`.cargo/config.toml`, which puts torch's lib directory on PATH; fixed 2026-10-07 to convert `$PWD`
with `cygpath -u`, since Cargo can hand it over as `C:/dev/rotor`, whose colon split the entry. `smithy` builds and runs
on Windows; the CUDA check waits for the first `prepare` and `train` there. README updated (step 4). `bin/arena self hand-crafted`
waits for the tables. Same machine as the WSL checkout (the training machine), so the GPU, Drive for desktop on `G:` and
the data on Drive are all already there.

## Moving the work to the Windows checkout

### Before leaving WSL

Everything to carry over must be committed and pushed: `data/` is gitignored and stays behind.

```
git status
```

```
git push
```

### Tools on Windows

- Git for Windows, which brings Git Bash: every `bin/` script runs from it.
- Rust through rustup, the default `x86_64-pc-windows-msvc` toolchain, and the Visual Studio Build
  Tools' "Desktop development with C++" (the MSVC linker, and the C compiler `rusqlite`'s bundled
  SQLite needs). The toolchain must satisfy `rust-version` in `Cargo.toml`.
- Python, for the `.rotor` venv: only needed at item 8.

### Clone

`.gitattributes` decides line endings: CRLF for every text file on Windows, LF for `bin/`, `*.sh`
and `*.awk`. Its `eol` attributes take precedence over `core.autocrlf`, so Git for Windows' default
needs no change. In Git Bash, clone the branch directly:

```
git clone -b bartolomeo <the origin URL, from `git remote get-url origin` in WSL> rotor
```

Not a clone of `main` followed by `git switch bartolomeo`: `main`'s checkout came out CRLF for the
scripts, and the switch rewrote only the files the port changed, which left `bin/ccrl-games.awk`
CRLF on the Lichess machine. Once the branch is merged, a plain clone is enough.

Check that the scripts came out LF (`w/lf` on every line):

```
git ls-files --eol bin/
```

A `w/crlf` line is fixed by deleting the file and checking it out again (`rm` then
`git checkout -- <file>`); `git checkout` alone skips a file Git sees as unmodified.

Not in the clone, and recreated by hand only once the item that needs them is done:

- `data/machine.yaml` (`name: the training machine`): needed by `arena self`. Only after items 1 and 2.
  While a WSL generator and a Windows one could both run, they must not share a name; on one
  machine, run only one of them.
- The data root's contents (book, Syzygy tables): re-downloaded by `bin/syzygy.sh` and
  `bin/lichess-book.sh` once item 1 points them at `%LOCALAPPDATA%\rotor`.
- `data/networks/...` local champions: downloaded from Drive by `arena self <network>`.

First check on Windows, before any change: the build and the tests that do not touch the
platform-specific parts.

```
cargo build --release --bin rotor --bin arena
```

```
cargo test --lib
```

## Decision

Development moves from WSL to native Windows. The Rust code is made to work on both: Linux stays
supported, since a generating machine or a lichess-bot host may still be Linux, and the cost is a
few `#[cfg]` pairs. The scripts in `bin/` stay Bash, one copy only, run from Git Bash on Windows.
No PowerShell or batch twins.

`.gitattributes` (committed) already keeps everything under `bin/`, `*.sh` and `*.awk` LF, so
Git Bash runs them on a Windows checkout.

## Inventory and decisions

### 1. Data root (`src/pipeline/data_root.rs`)

Today: `$HOME/.local/share/rotor`, exiting when `HOME` is unset. Native Windows usually has no
`HOME` outside Git Bash.

Decision: one fixed place per platform, still no variable to move it.

- Linux: `$HOME/.local/share/rotor`, unchanged.
- Windows: `%LOCALAPPDATA%\rotor`, from `LOCALAPPDATA`, exiting when it is unset.

A `#[cfg(windows)]` / `#[cfg(not(windows))]` pair rather than the `dirs` crate: two environment
variables do not justify a dependency, and the unit tests keep taking the variable's value as an
argument as they do now.

`bin/syzygy.sh` and `bin/lichess-book.sh` compute the same root: under Git Bash (`$OSTYPE` is
`msys` or `cygwin`, depending on its version: `cygwin` on the training machine) they use `$LOCALAPPDATA/rotor`,
else `$HOME/.local/share/rotor`.

### 2. Remote root (`src/pipeline/remote.rs`)

Today: `ROOT = "/mnt/g/My Drive/rotor"`, plus the error message in `check_mounted_under` that
explains the `/etc/wsl.conf` mount.

Decision: `G:/My Drive/rotor` on Windows (forward slashes work in every Windows file API, and
keep `format!("{root}/{local}")` unchanged), `/mnt/g/My Drive/rotor` elsewhere. On Windows the
drvfs mount disappears entirely: Drive for desktop's `G:` is read directly. The not-mounted
message gets a Windows variant ("Google Drive for desktop must be running with My Drive on G:").

Scripts with `/mnt/g` hard-coded (`ccrl-download.sh`, `ccrl-results.sh`, `ccrl-ratings.sh`,
`ccrl-total.sh`, `ccrl-first-no-book.sh`, `tcec-months.sh`): Git Bash names the drive `/g`, so
each computes `REMOTE_ROOT` once, `/g/My Drive/rotor` under `msys`, else `/mnt/g/My Drive/rotor`.
`bin/mount-g.sh` is WSL-only and says so, exiting early under `msys`.

### 3. Memory guard (`src/arena/memory.rs`, `src/train/test_cache.rs`)

Today: `/proc/meminfo`'s `MemTotal`. Absent on Windows, so `memory::check` refuses every run.

Decision: a shared `rotor::pipeline::system_memory::read()`, total and available physical memory
(the test cache also reads `MemAvailable`): `/proc/meminfo` on Linux, `GlobalMemoryStatusEx`'s
`ullTotalPhys` and `ullAvailPhys` on Windows, via `windows-sys` (Windows-only dependency). Both
callers use it; the `/proc/meminfo` parser moves there with its tests, which run on Windows too.

### 4. CPU topology and pinning (`src/arena/cpu_topology.rs`)

Today: `/sys/devices/system/cpu/*/topology/thread_siblings_list`, and `sched_setaffinity` on the
worker thread, which spawned engine processes inherit.

Windows differs in kind, not only in API: a process created by `CreateProcess` inherits its
parent's *process* affinity mask, not the spawning thread's. Pinning the worker thread pins only
in-process players, never a UCI subprocess.

Decision:

- Topology: `GetLogicalProcessorInformationEx(RelationProcessorCore)`, lowest logical CPU of each
  core, keeping `physical_core_cpus`'s contract. Single processor group only (64 logical CPUs);
  more is an error, not silently ignored.
- Pinning: `SetThreadAffinityMask` for the worker thread, and `uci_client.rs` additionally sets
  the child's mask with `SetProcessAffinityMask` right after `spawn`, on Windows only. A short
  window where the child runs unpinned before the handshake is harmless: no clock is running yet.
  As applied: `pin_current_thread` remembers the CPU in a thread-local, and `uci_client.rs` calls
  `cpu_topology::pin_child(&child)`, a no-op on Linux, so `UciClient::start` keeps its signature.

### 5. Ctrl+C and engine subprocesses (`src/arena/uci_client.rs`)

Today: `process_group(0)` on Unix keeps the terminal's SIGINT away from the engines, so only the
arena decides when they stop.

Windows: a console Ctrl+C reaches every process attached to the console. Decision: spawn with
`CREATE_NEW_PROCESS_GROUP` (`std::os::windows::process::CommandExt::creation_flags`), the
equivalent guard. To be checked by hand: Ctrl+C during a timed match, both stages.

### 6. Executable names

`target/release/rotor` is `rotor.exe` on Windows. Affected: `ENGINE_BUILD_PATH` in
`src/train/mod.rs`, the installed engine `<data root>/bin/rotor` (and `eval_file_beside`, which
derives `rotor.nnue` from the binary's path: must strip `.exe`, not append to it), the `bin/`
wrappers' `exec` lines, and any engine path in `data/engines.yaml`. Use
`std::env::consts::EXE_SUFFIX` in Rust and a `$EXE` variable in the wrappers.

As applied: `eval_file_beside` already replaced the whole file name (`with_file_name`), so it needed
only a Windows test case. `install` also prints `rotor.exe` as lichess-bot's engine `name`.
`benches/search_bench.rs`'s default engine path gets the suffix too. `data/engines.yaml` is local,
not in the repository: its engine paths are edited by hand on each Windows machine.

### 6b. Rotor as a UCI engine in matches (`src/arena/match_player.rs`)

Missed by the first inventory. Every timed match, and every fixed-depth match against an external
engine, spawned `bin/rotor`, the Bash wrapper. Windows cannot spawn a script, and a wrapper
process in between would take item 4's pin and the arena's kill instead of the engine.

Decision: `bin/arena` builds `rotor` together with `arena`, and `match_player.rs`
spawns `target/release/rotor` plus `EXE_SUFFIX` directly, on every platform. The engine stays as
fresh as the arena, which is what spawning the wrapper was for.

### 7. Installing over a running engine (`install_file_or_exit` in `src/train/mod.rs`)

Today: copy to `.partial`, then rename over the target; Linux lets a running binary be replaced
that way ("text file busy" only on overwrite).

Windows refuses to rename over, or delete, an `.exe` that is running. Decision: on Windows,
rename the running `rotor.exe` aside to `rotor.exe.old` first (allowed while running), then
rename the new one into place, and remove `rotor.exe.old` best-effort at the next install. The
`.nnue` beside it is read once at startup and closed, so it replaces normally; to be verified.

As applied: every installed file (engine, network, book) goes through the same move aside on
Windows, harmless for a file nobody has open. The aside is `{name}.old-{unix seconds}`, not a
fixed `.old`: an aside whose engine still runs cannot be removed, and a fixed name would then block
the next install. Earlier asides are removed best-effort before each install.

### 8. smithy and libtorch (`bin/smithy`, `build_engine_or_exit`)

Today: checks `VIRTUAL_ENV == $REPO_ROOT/.rotor`, finds `.rotor/lib/python3.*/site-packages/torch/lib`,
sets `LD_LIBRARY_PATH` and `LD_PRELOAD=libtorch_cuda.so`.

Windows:

- The venv is laid out `.rotor/Scripts/activate` and `.rotor/Lib/site-packages/torch/lib`.
- `VIRTUAL_ENV` may be a Windows path (`C:\...`) while `$REPO_ROOT` is `/c/...`: compare after
  `cygpath -u`.
- DLLs are found through `PATH`, so `PATH="$TORCH_LIB_DIR:$PATH"` replaces `LD_LIBRARY_PATH`.
- There is no `LD_PRELOAD`. The GPU itself works on Windows; the issue is only that the linker
  drops `torch_cuda.dll` because nothing references it, so `cuda_if_available()` silently falls
  back to the CPU. The known `tch` fix is a link argument forcing it in, in a `build.rs` under the
  `training` feature: `cargo:rustc-link-arg=/INCLUDE:?warp_size@cuda@at@@YAHXZ`. Verified as part
  of this item by `smithy` itself reporting CUDA, not by a separate probe. If it cannot be
  made to work, training stays in WSL and only the arena and engine move.
- `build_engine_or_exit` removes `LD_PRELOAD`/`LD_LIBRARY_PATH` from the child `cargo`; on Windows
  there is nothing to remove, the `PATH` addition is harmless.

### 9. Already portable, no change

- `try_acquire_lock`: std's `File::try_lock` is `LockFileEx` on Windows, released on exit.
- `ctrlc` crate: handles Windows console events.
- `rusqlite` bundled, `zstd`: build with MSVC.
- Relative `data/...` paths: forward slashes work on Windows.

## Order

1. Items 1, 2, 3, 6: enough for `rotor` and `arena self` to run on Windows. None touches
   torch.
2. Items 4, 5: timed matches.
3. Item 7, then item 8, including the CUDA link argument.
4. README: the WSL setup sections (`/etc/wsl.conf`, `mount-g.sh`) become the Linux path, with a
   Windows path beside them (Git Bash, Rust MSVC toolchain, the venv).

## Performance, native Windows against WSL

Expected, not measured:

- The search itself: about the same. WSL2 is a Hyper-V VM, near-native for CPU-bound code. The
  MSVC target uses the Windows heap instead of glibc's `malloc`, which matters only if the search
  allocates in its hot path.
- Gains: Drive is read directly instead of through drvfs, which was slow for every copy, listing
  and size check; no VM capped at half the RAM, the reason for the move; builds and local files on
  NTFS with no VM boundary.
- Risks: Microsoft Defender's real-time scanning of `target/` and `data/` (exclude `C:\dev\rotor`
  under Windows Security > Virus & threat protection > Exclusions); engines unpinned until item 4,
  so SMT siblings share a core and matches are noisier; Windows timer granularity (sleeps and
  timeouts rounding up by about 1 to 15 ms), which hurts only very short time controls and is
  watched during item 4's timed matches; CUDA possibly slightly slower than under WSL because of
  the display driver model, seen at item 8.

Done on the training machine: Defender exclusions for the repository, the data root, `.cargo` and `.rustup`
(none existed before: VS Code, unlike IntelliJ, never offers them); README's Platforms section
names them, with the power mode.

To measure, while WSL is still installed: nodes per second at a fixed depth on the same positions,
in each environment (`benches/search_bench.rs` is a candidate).

## Verification

- `cargo test` on both platforms after each step.
- On Windows by hand: `bin/arena self hand-crafted` reaches its first batch; a short
  `bin/arena match ... --time 10+0.1` shows one engine per physical core pinned (Task
  Manager's affinity), and both Ctrl+C stages behave; `bin/smithy install` over a running
  lichess-bot engine.
