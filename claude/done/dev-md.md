# README.md for users, DEV.md for developers

**Status:** decided and applied 2026-10-13.

## Why

The developer, 2026-10-13: `README.md` is for people using the Windows release, a CCRL tester
above all; `DEV.md` is for whoever builds Rotor or trains its networks. This reverses "One file,
rewritten" in readme-for-humans.md, whose outline otherwise stands, split between the two files.

**Priorities (the developer, 2026-10-13):** `DEV.md` gets minimal upkeep, only what keeps it
correct, since nobody else develops Rotor; the care goes to `README.md`.

## The split

**README.md:** the introduction, Strength, Running Rotor (requirements, UCI options), How Rotor
learns, Inside, a pointer to `DEV.md`, License. The introduction loses the sentence on what the
repository holds; Inside's "see Development" becomes a link to `DEV.md`.

**DEV.md:** `# Developing Rotor`, one line linking back to `README.md`, then today's Development
section as is, every heading one level up: the workspace (with the sentence on what the repository
holds), one generation, setting up a machine, the archives, running a generation, matches, Lichess.

## Steps

1. `DEV.md` from README.md's Development section.
2. `README.md` without it, pointing to `DEV.md`.
3. `bin/smithy`: its three hints name README.md's "Learned-evaluation environment setup", a section
   gone since the rewrite; they name `DEV.md`'s "Setting up a machine".
4. readme-for-humans.md and `TODO.md`: the reversal noted, the release revision covers both files.
