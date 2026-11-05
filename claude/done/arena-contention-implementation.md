# Arena contention: implementation plan

**Status:** proposed 2026-10-29. The design: arena-contention.md (decisions numbered as there).
Each step tested and committed before the next; the arena and the smithy change their claim
together (step 2), so both binaries are rebuilt on every machine at that step.

## Steps

1. **`machines.yaml`' one list (decision 6):** `protocol::machines` parses `machines:` into an
   ordered list and refuses `smithy:` or `arenas:` with the new form in its message;
   `arenas_to_claim` returns it, and the smithy's rank is its machine's index (`None` unlisted).
   `examples/machines.yaml`, DEV.md. The developer rewrites the copy on Drive at this step. Tests:
   the parse, the refusal, the rank.
   **Written 2026-10-29, to build and test:** `Machines { machines }`, `Machines::rank`; the smithy
   logs its rank at start, the rank carried in no claim until step 2. **Done 2026-10-29.**
   **Step 2 written 2026-10-29, to build and test:** `Claim::scaled`, `Claim::rank`, `Claim::outranks`;
   `ClaimState::take` gives way to an outranking claim, logged; the smithy's claim carries its rank,
   `scaled` always false until step 3. **Done 2026-10-29** (tests and clippy passing; the live
   check left to step 5).
   **Step 3 written 2026-10-29, to build and test:** `Claims::round` claims only while `Shared::work`
   deals, releasing every arena after `CLAIM_IDLE_GRACE` (60 s) of none; `Shared::scaled_job`, set by
   the forge and the tune, in each claim. **Install claims (the developer, 2026-10-29):** the page's
   Install offers every listed machine; `ask_install` claims a free arena as a full job's, waits up
   to 15 s for its hello, and keeps it until its install ends. Also: Install offers PeSTO and full
   runs' generation winners alone (`installable`); what a machine has installed shown only for an
   arena held, never remembered (the developer: "not pretend that we know"); a book source whose PGN
   is gone still lists its books. **Done 2026-10-29.**
   **Step 4 written 2026-10-29, to build and test:** `Claim::standing` (rank and kind, no name) in the
   arena's `CLAIMED` answer and its `arena status`; the Machines table's note "held by the smithy
   ranked 1, for a full job"; `Snapshot::no_arena` and the page's waiting line. **Done 2026-10-29.**
2. **The claim carries its standing (decisions 2 to 4):** `Claim` gains `scaled: bool` and
   `rank: Option<u32>`, in its text and parse (an old arena or smithy refused on the commit check
   already in the claim). The arena's `ClaimState::take` gives way, by decision 4's order, to a
   claim that outranks the one held: the held claim dropped as a lapse drops it (its sessions and
   outbox gone), and its smithy's next poll told it is released, so it deals that arena's chores
   again. `/alive`'s refusal names the standing that took it. Tests in `chores.rs`: full over
   scaled, rank over rank, unranked first come, the holder kept against an equal or lower claim.
3. **Claims only while playing (decision 1):** the smithy's claimer, today started at `start` for
   good, is driven by the job: `Shared` gains what the claimer wants (`None`, or a claim's
   standing), set when a job enters a phase that deals chores and cleared when it leaves one, ends
   or stops; cleared, the claimer releases every arena held (`release_all`) and stops claiming.
   The forge sets it around Play, the round robin and the gate; a tune, a test and PeSTO's play for
   their whole run. Tests: a phase without chores releases; the standing a scaled run's claim
   carries.
4. **The page and `arena status` (decision 5):** each arena held here, held by another smithy (its
   rank), or free, from the claimer's last answers; a job dealing chores with no arena held says it
   waits for one. `arena status` prints the holder's rank. Tests: the status text.
5. **Done 2026-10-29** (the developer). **Checks:** `cargo test`, `cargo clippy` with and without `--tests`; two smithies on two
   machines, a scaled run then a full run started: the arenas move to the full run, and back once it
   trains; two full runs: the higher-ranked smithy's job holds every arena while both play.
6. **Done 2026-10-29.** **Docs:** arena-contention.md's results, DEV.md, TODO.md, discovery-and-heartbeats.md's note.
