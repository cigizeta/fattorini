# Everything on Google Drive

**Status: plan confirmed and implemented, 2026-10-22, DEV.md done; every test passed; the Lichess machine and the training machine checks to do. Beyond or
against the plan, to judge: a chore names a network by its exact path in the store
(`data/gen/.../attempt-2/lambda-0.95/candidate.nnue`), not its player name, which a delayed sync
could resolve to an earlier attempt's file (PGN tags now carry those paths); the store's root
travels in the hello; whole copies go through `remote::copy_replacing` (a `.partial` written whole,
then renamed on the remote); no local cache yet for the sources' frozen test sets.** No code touched. Supersedes an earlier archive design (since removed).

## Decisions (the developer, 2026-10-22)

1. **No archive to another machine:** that machine's disk, its share, its account and rclone go;
   the archive's idea is dropped.
2. **Google Drive is the store,** on a paid plan with enough storage.
3. **Drive for desktop in streaming mode,** the local disks being smaller than the store: files are
   fetched on demand, and only those made available offline stay on the local disk.
4. **Precious folders** (networks, tunes, generation records, ...) may be made available offline, or
   not: undecided.

5. **A general principle: a file is made on the local disk, then moved to `G:` once complete.**
   Nothing is written on `G:` piece by piece; Drive only ever receives finished files.

6. **No partial file is ever written to `G:`, and nothing is appended there:** appended files are a
   bad use of Google Drive. Files that live along a run (`state.txt`, `history.tsv`,
   `generations.tsv`, the round robin and gate records, `chores.txt`, a tune's `state.tsv` and
   `pairs.tsv`) are the local copy's; `G:` receives a fresh, complete copy of each at every phase
   boundary, replacing the last, so a dead local disk loses at most one phase.

7. **A finished file stays local while it is in use, and is moved to `G:` only when done with:** a
   self-play segment, for one, stays on the local disk while its generation may still read it (the
   scatter, a retrain after a failed gate), then is moved, not copied. Accepted: a dead local disk
   mid-generation loses that generation's games, which can be played again.
8. **"Done with" is the generation's closing,** one rule for a run's generation-bound files
   (segments, checkpoints, candidates, pair records, the generation's frozen test set): promoted
   (the next generation's teacher copied out first) or the lineage stopped. The archive's test of a
   closed generation drives the move. The generation in training keeps its teacher files local.

9. **The sources' corpora (Lc0's, PeSTO's) are stored on Drive and read from Drive:** each segment
   moves to `G:` as soon as it is complete, and a forge reads it from there (an NNUE's scatter
   streams it once a machine into the local buckets). The frozen test sets, small and read every
   round, keep their master on `G:` and a local cache (Claude's, not objected to).

10. **A tune works locally:** a local mirror of its folder (`run-<n>/gen-NNNN/tune/`), the network's
    file copied down from `G:` once; its living files copied to `G:` whole at its boundaries; at its
    end, the folder and `parameters.txt` moved to `G:` beside the network. A local copy without its
    finished twin on `G:` is a tune in progress.

11. **Every network goes to `G:` as soon as it is complete:** every lambda's candidate of every
    attempt and every teacher, not only each generation's winner, copied (the local copy kept while
    the generation uses it, decision 7).
12. **The smithy sends the arenas a network's name, and the arenas read it from `G:`;** the download
    by hash goes. Every arena machine then needs `G:`.
13. **An arena dealt a network not yet visible on its `G:` waits and retries:** it looks every few
    seconds, says once that it is waiting for that network on `G:`, plays as soon as it appears, and
    after a long timeout (30 minutes, say) gives the chore back to be dealt elsewhere.

## Later

- **A strategy to make syncing faster (the developer, 2026-10-22):** to redesign at some point;
  marking some folders available offline has helped before.
- **Prefetch, for what cannot be offline (the developer, 2026-10-22):** MCTS and a policy will make
  some folders far too large to keep offline. Prefetching is reading a file from `G:` ahead of need,
  nothing more: the read makes Drive fetch it into its cache, and Drive evicts as it needs room. To
  watch (Claude): read a bounded window ahead of the consumer, not the whole corpus, or a prefetched
  file may be evicted before it is used; read each file to its end, in case Drive fetches only the
  parts read; and bandwidth decides in the end, so time a full read of a corpus from `G:` on the
  training machine against a training epoch's consumption before the policy arrives.

## Points raised by Claude (2026-10-22)

- Drive is sync, not backup: a deletion on `G:` reaches Drive at once (30 days of trash), and an
  account lost loses everything. What cannot be redone cheaply is small: the winners' networks, their
  tunes, the generation records. A copy of those on a disk the developer owns would cover the
  account's risk.
- Drive caps uploads at about 750 GB a day an account.
- Training's hot data (buckets, frozen sets read every round) stays on the local disk: streaming it
  from `G:` would be far too slow.

## Settled by Claude, confirmed by the developer (2026-10-22)

- **One tree, mirrored:** every path is the same relative path under the local data root and under
  `G:/My Drive/rotor`; a file moves between the two at the same place.
- **Local first, then `G:`:** every reader of a run's files (`arena duel`, `smithy install`, a tune,
  the page's View, `check`, `calibrate`, `correlate`) looks locally, then on `G:`.
- **The archive's code becomes the mover:** its test of a closed generation kept, its destination
  `G:` instead of another machine; the rclone remote, the SMB share, the treasure tier and
  `archive.yaml`'s keys go.
- **Copy boundaries:** a run copies its living files to `G:` at every phase boundary; a tune every
  15 minutes and at its end.
- **Without `G:`** a machine cannot resolve a closed generation's players, and says so. Since
  decision 12, an arena needs `G:` for its jobs too.
- **Offline folders** stay the developer's choice, outside the code (decision 4).
- **Order:** sources.md first, then this plan; sources.md's decision 7 (corpora on Drive) fits.

## Plan

1. **Paths:** one helper for "a data path, local or on `G:`" (`remote::remote_path` and its
   siblings already map local paths to `G:`); readers of run files resolve local first, then `G:`.
2. **The mover:** `archive.rs` reduced to moving a closed generation's directory (decision 8) from
   local to `G:` as whole files, then deleting the local copy; the remote archive, rclone and the
   treasure tier removed; `configs`' and `data/archive.yaml`'s keys with them.
3. **Living files:** at each phase boundary, the run's living files (decision 6) copied whole to
   `G:`, replacing the last copy, through a local temporary file and one move, never written in place.
4. **Sources:** Lc0's conversion and PeSTO's Play write each segment locally and move it to `G:` once
   closed (decision 9); readers read from `G:`; each frozen test set's master on `G:`, cached locally.
5. **Tunes:** decision 10: a local mirror of the tune folder, the network's file fetched from `G:`
   when not local, copies every 15 minutes and at the end, then the folder moved.
6. **Networks by name (decisions 11 to 13):** every candidate and teacher copied to `G:` once
   complete; a chore names its networks (the run's player names) instead of hashes; the arena
   resolves each on `G:`, waiting and retrying, then giving the chore back after its timeout;
   `NETWORK_PATH`, `Shared::networks` and the arena's download go.
7. **Docs:** DEV.md (Drive's role, every arena machine on `G:`, the archive's setup gone),
   TODO.md.
8. Build, test; on the Lichess machine a smoke run closing a generation (moved to `G:`, its players still
   resolved), a small tune; an arena on a second machine playing a candidate read from `G:`; on
   the training machine the real runs.
