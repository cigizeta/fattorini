# CLAUDE.md -- Fattorini

**Fattorini** is a UCI chess engine, written in Rust, whose neural network evaluation functions
are trained on a limited compute budget.

## Never leak secrets or sensitive information

Assume everything committed may become public, forever, history included. **Never write**, to any
file, commit message, issue, pull request, published page or external service:

- **Secrets:** passwords, tokens, API keys, Lichess or other credentials of any kind, even expired
  or fake-looking ones.
- **Identifying details beyond what the project already names:** IP addresses, MAC addresses,
  e-mail addresses, share names, serial numbers, paths that reveal more than the data root, and
  **machine names** and anything describing the developer's machines, how many there are, their
  hardware or age (the developer, 2026-10-24): a machine is named by its role alone, never by its
  hostname, in files and in conversation alike. Write as generally as the point allows.
- **Personal or household information:** schedules, habits, travel, when the home is empty or a
  machine is off, where the developer works or lives.

Real values live only in gitignored local files (as `data/engines.yaml`), with committed examples
holding placeholders. Before writing a file the developer may commit, check it for any of the above;
when in doubt, leave it out and ask. A leak already committed cannot be fixed by a later commit:
say so at once, so the developer can rewrite history and rotate the secret.

**The same holds in conversation (the developer, 2026-10-24):** neither Claude nor the developer
mentions any of the above in a chat. Claude never brings it up, not even to quote what it found;
it names the file and line instead. When the developer's message or pasted output carries some (a
prompt with the user name, a path, a schedule), Claude says so in one line, without repeating it,
so the developer can be more careful next time.

## No autonomous writes allowed

The development of this project assumes that access to NotebookEdit, Bash,
and Monitor is fully denied to Claude. As a consequence, Git is also
fully denied.

## claude/ organization

Each repository has its own `claude/` directory (prose, distinct from the
`.claude/` harness config), holding `TODO.md` plus `active/`, `done/`, and
`archived/`.

- `active/`: work in progress.
- `done/`: completed, verified work, moved from `active/` as-is, no rename.
- `archived/`: superseded/abandoned/stale docs, moved here as-is with
  a "superseded, see X" note if applicable.

Source code must never reference a `claude/` document (bare filename or
path) -- inline the "why" directly in the code instead. Fix existing
violations opportunistically, not as a sweep.

Within `claude/`, docs may reference each other, but only by bare filename,
because filenames survive moves between the three subdirectories; paths do not.

## Design first, implement later

Do not rush off implementing. First discuss the design thoroughly, then write
the implementation plan as a document in `claude/active/`, and only then apply
it.

## Fenced blocks

When you want the developer to run something themselves, such as shell
commands or a commit, put it inside a fenced block so that they can copy it.
Put each command in its own fenced block: three consecutive commands are
three fenced blocks, never one block holding all three. Commit messages
must be one line only.

## Development platform: Windows only

Fattorini is developed, tested, measured and distributed on native Windows only,
not WSL (decided 2026-10-05: CCRL testers run a Windows binary). **Linux is
no longer a target platform (decided 2026-10-08):** new code is written for
Windows alone, with no `#[cfg(not(windows))]` twin, no `$OSTYPE` branch and no
Linux path, and nothing is built or tested on Linux. The shell is **Git
Bash**, never PowerShell or cmd: every command in a fenced block uses Bash
syntax.

- Machines are named by role only, never by hostname or description: training needs a machine with
  a GPU, and Lichess plays on a machine of its own. Which machines exist lives only in gitignored
  local files. Speed and A/B measurements may run on any machine (decided 2026-10-18), as long as
  it is idle (no build, training, self-play or replay beside them), plugged in, and every
  side of one comparison runs on the same machine.
- Rust uses the default `x86_64-pc-windows-msvc` toolchain on Windows.
- The scripts in `bin/` stay Bash, one copy only, run from Git Bash on
  Windows: no PowerShell or batch twins. Windows paths are converted with
  `cygpath -u`.
- Line endings are governed by `.gitattributes` (`bin/**`, `*.sh`, `*.awk`
  always LF; `*.bat`, `*.cmd`, `*.ps1` always CRLF). Every other text file
  is stored LF and checked out with the OS's native endings, CRLF on Windows.
- Windows-specific Rust code needs no `#[cfg]` pair any more, and no new
  dependency when a few lines suffice. The data root is the repository's gitignored `data/`. The arena's
  snapshots of the engine go straight to Drive's `data/snapshots/`, never locally; lichess-bot
  runs one of them.
  **Nothing in this repository reads or writes `%LOCALAPPDATA%`** (the developer, 2026-11-06), nor
  anywhere outside the repository and Drive; a release is a plain `.exe` that writes nothing.
  Google Drive is `G:` (`/g` in Git Bash), and Fattorini's store on it is `G:/My Drive/fattorini`,
  the same tree as `data/`. Folders there under the project's older names (as `temper-chess`) hold
  stale runs: never look there.
- Git Bash rewrites arguments that look like Unix paths when calling native
  Windows programs; prefix the command with `MSYS_NO_PATHCONV=1` when that
  bites. It did not help with `winget` (2026-10-07, an `--override "/features ..."`
  still arrived as `C:/Program Files/Git/features`); a leading space inside
  the quoted argument (`" /features ..."`) does, since only an argument
  starting with `/` is rewritten.
- Code that launches processes cannot rely on Bash: Rust's `Command` needs
  the real executable name, including `.exe` or `.cmd`.

## Run the binaries through bin/

When giving the developer a command that runs a Fattorini binary, always use its wrapper in `bin/`
(`bin/fattorini`, `bin/smithy`, `bin/arena`), never `cargo run`, `cargo build` followed by
`target/release/...`, or a bare `target/...` path. Each wrapper builds
the release binary if needed, `cd`s to the repository root, and sets what the binary needs:
`bin/smithy` also requires this repository's `.fattorini` venv to be active and sets libtorch's library
paths. Only tests (`cargo test ...`) are given as cargo commands.

## Licensing

Fattorini is AGPLv3 only (`LICENSE`, `AGPL-3.0-only`): the developer's goal
is that nobody makes money by forking or using the code, and AGPLv3 also
obliges anyone running a modified Fattorini engine as a network service to share its
source. GPLv3 code may be combined with it (AGPLv3 section 13).

## No em dashes

Never write em dashes (—) or en dashes (–) to any file in this
repository, Markdown or otherwise. Write a double hyphen (--) instead.
Ordinary hyphens (-) are fine.
