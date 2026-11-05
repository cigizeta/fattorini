# Linux code removed

**Status (2026-10-08):** done; tests passing on Windows, committed and pushed.

The developer (2026-10-08): Linux is no longer a target platform (CLAUDE.md), and all Linux code
goes in one sweep.

## Rule

- Every `#[cfg(not(windows))]`, `#[cfg(unix)]` and `target_os = "linux"` item goes, with its
  tests. The `#[cfg(windows)]` gates its twins needed go too: with one platform they say nothing.
  `#[cfg(any(windows, test))]` becomes no gate at all.
- `windows-sys` and `mimalloc` move to plain `[dependencies]`; `libc`, used only by Linux pinning,
  is dropped.
- `bin/`: no `$OSTYPE` branch; Windows paths only (`cygpath -u`), `.exe` always, the venv's
  `Scripts/` and `Lib/` only, no `LD_LIBRARY_PATH` or `LD_PRELOAD`. `bin/mount-g.sh`, WSL only, is
  deleted.
- `.cargo/config.toml`: the Linux runner and its notes go. `build.rs`: no Linux aside.
- Comments citing Linux or WSL measurements as the reason for a Windows choice keep only the
  Windows reason. Measurements that were taken under WSL stay recorded in `claude/` docs, not here.
- README.md: the Linux column, the Linux install steps, the WSL Drive mount and the AppImage route
  for cutechess go; lichess-bot's `config.yml` example takes the Windows path.

## Sites

`src/arena/cpu_topology.rs` (sysfs topology, `sched_setaffinity`, the other-OS stubs, their
tests), `src/pipeline/data_root.rs`, `src/pipeline/system_memory.rs` (`/proc/meminfo` and its
tests), `src/pipeline/remote.rs`, `src/core_pinning.rs`, `src/tablebase.rs`,
`src/arena/uci_client.rs`, `src/bin/rotor_arena.rs`, `src/train/mod.rs` (`env_remove` of the
loader variables, the `cfg(windows)` on the move aside), `src/uci.rs` (the Linux test path, the
separator note), `src/arena/memory.rs`, `src/train/dataset_cache.rs`, `Cargo.toml`,
`.cargo/config.toml`, `build.rs`, `bin/rotor`, `bin/rotor-arena`, `bin/rotor-train`,
`bin/syzygy.sh`, `bin/lichess-book.sh`, the five `bin/ccrl-*.sh` and `bin/tcec-months.sh`,
`bin/mount-g.sh`, README.md.
