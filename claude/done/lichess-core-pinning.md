# `--lichess`: each Lichess game pinned to its own physical cores

Discussed with the developer 2026-10-06. **Status:** done; the bot plays with `--lichess`
(checked 2026-10-14).

## The goal

On the Lichess machine, lichess-bot plays two games at once (`concurrency: 2`, `Threads: 4`). Each game's engine
runs on its own physical cores, two each: one game on cores 0-1, the other on cores 2-3. The other
cores are left to the OS and everything else.

A hybrid CPU, with fast and efficient cores, both with SMT: two games of two cores should take
the fast ones.

## Decided (the developer)

- **Not a UCI option.** lichess-bot sends the same `uci_options` to every engine, so an option
  cannot tell the two games apart, and the engine would depend on lichess-bot sending it in time.
- **Not a sentinel file, and not "running from the data root's `bin/`".** The installed engine is
  also used outside Lichess (GUIs, testing, CCRL-style play), so the signal must come with each
  launch, not with the machine or the binary.
- **A command-line flag, `--lichess`.** lichess-bot passes `config.yml`'s `engine_options` as
  arguments (`lib/engine_wrapper.py`: `--{k}={v}`, or `--{k}` when the value is empty):

  ```yaml
  engine:
    engine_options:
      lichess:
  ```

- **No core count or core list is passed.** The engine finds them by itself.
- **`--lichess` alone, no value (2026-10-06).** Groups are laid out over every core; an engine
  reserves the first free group. Which cores stay free for the OS follows from lichess-bot's
  `concurrency` and `Threads`: two games of `Threads: 4` take the first two groups, the rest are
  never claimed. When every group is reserved, the engine plays unpinned. No retry at later `go`s.

## The design

1. **The flag.** `main.rs` reads `std::env::args()`; `--lichess` sets the pinning on. Any other
   argument is an error (exit with a message), so a typo is not silently ignored. Without arguments,
   nothing changes.
2. **The topology, from Windows.** `GetLogicalProcessorInformationEx(RelationProcessorCore)`: one
   record per physical core, with its logical processors (a mask) and its `EfficiencyClass` (higher
   is faster). Cores are ordered by `EfficiencyClass` descending, then by their first logical
   processor, so groups land on the fastest cores first.
3. **The groups, from `Threads`.** Walking the ordered cores, a group takes whole physical cores
   until it holds at least `Threads` logical processors: `Threads: 4` with SMT is 2 cores. The next
   group starts at the next core, and so on, as long as a whole group fits. Counting logical
   processors per core, not assuming one count for all, keeps this right where cores differ: an
   Intel hybrid's P-cores have SMT and its E-cores do not.
   The same binary runs on every machine; nothing in it names a machine.
4. **The claim.** The engine claims the first free group with a named mutex
   (`Local\rotor-lichess-group-{n}`, `CreateMutexW`, owned or not by `ERROR_ALREADY_EXISTS`), held
   until the process exits. Windows releases it on exit, crash included, so a group never stays
   claimed by a dead engine. lichess-bot starts one engine per game, so the claim lasts one game.
   - **No race between two engines.** Creating a named mutex is atomic in the kernel: of two
     simultaneous `CreateMutexW` calls on one name, exactly one creates it and the other gets
     `ERROR_ALREADY_EXISTS`. Both walk the groups in the same order, so the loser moves on.
   - **The loser closes its handle at once.** `ERROR_ALREADY_EXISTS` still opens a handle, and the
     mutex lives while any handle is open: kept, it would hold the group past its owner's exit.
     The window between that `CreateMutexW` and its `CloseHandle` can only make a third engine skip
     a group just freed, never put two engines on one group.
   - **Existence, not ownership.** The owner never waits on the mutex: ownership belongs to a
     thread, and a mutex whose owning thread ends is abandoned and claimable while its process
     still plays. Existence belongs to the process (its handle, never closed).
5. **When.** At the first `go`, when `Threads` is certainly set; never again in that process.
   `SetProcessAffinityMask` with the union of the group's logical processors: it covers every
   thread, including the search threads `uci.rs` spawns anew on every `go`.
6. **Reported.** One `info string` at that first `go`: the group and its logical processors, or
   why the engine runs unpinned (every group reserved, more than one processor group, or the
   topology call failed). Never a refusal to play.
7. **Linux.** `--lichess` is accepted and does nothing, with an `info string` saying so. A
   `#[cfg(windows)]` / `#[cfg(not(windows))]` pair; no new dependency: `windows-sys` 0.61 is
   already a direct Windows dependency with `Win32_System_SystemInformation` and
   `Win32_System_Threading`; only `Win32_Foundation` (`ERROR_ALREADY_EXISTS`, `CloseHandle`) may
   need adding to its features.

## Open points, to check while implementing

- **Does Windows give the fast and efficient cores different `EfficiencyClass` values?** If all
  report the same class, the order falls back to the core numbering, and the `info string` shows
  whether the first cores are the fast ones.

- **Other machines' CPUs** (not checked 2026-10-06): above 64 logical processors, Windows
  splits them into processor groups and `SetProcessAffinityMask` reaches only one. Handled without
  knowing them: when `GetActiveProcessorGroupCount` is more than 1, the engine runs unpinned with an
  `info string` saying why. Per-thread `SetThreadGroupAffinity` only if a machine ever needs it.

## Implemented (2026-10-06, not yet built or verified)

`src/core_pinning.rs` (new), `src/main.rs` (the flag), `src/uci.rs` (`run(lichess)`, the pin at
the first `go`), `Cargo.toml` (`windows-sys` gains `Win32_Foundation` and `Win32_Security`, needed
by `CreateMutexW`). The pinning itself is not unit-tested: a test would pin the test process and
hold a group's mutex; the pure parts (order, groups, the processor list) are.

## Measured on the Lichess machine (2026-10-06)

- **`EfficiencyClass` separates the fast cores from the efficient ones,** and Windows numbers the
  two kinds alternately, so taking the first cores by numbering would have mixed them; ordering by
  class puts both games on the fast cores.
- **Two engines at `Threads 4`:** two groups, no overlap.
- The `info string` lists each core of the group with its class, so the same check reads any other
  machine's ranking from its first game.

## Verify

On the Lichess machine:

1. `cargo test` for the parts that are pure functions: laying out groups over an ordered core
   list, and the group size from `Threads`.
2. `bin/smithy install amanda`, then add `engine_options: lichess:` to `config.yml`.
3. Two Lichess games at once: each engine's first `info string` names a different group, and Task
   Manager shows the load on those logical processors only.
4. The same installed engine started without `--lichess` (a GUI): no `info string` about pinning,
   no affinity change.
