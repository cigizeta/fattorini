# Replay: `rotor-arena replay <player>`

Status: design, not implemented. Follows ccrl-replay-sampling.md, which writes the chunks this
command replays.

## Context

Self-play is retired. Each generation, `rotor-train sample` writes about 200 chunks of whole
CCRL games (`data/games/sampled/ccrl/<player>/chunk-NNNN.pgn.zst`, plus the fixed TCEC chunks in
`data/games/sampled/tcec/`; the layout is in ccrl-replay-sampling.md), just over 100,000 trainable
positions each, every game from the standard position with `[Source]` and a single
`{ Book exit }` comment after its book, other comments stripped). This command
has the player (the hand-crafted evaluation, or a network's champion) search them at depth 9 on
every machine, and writes the labelled output that `rotor-train prepare` reads.

## Labels: the final position is a training position

A game of p plies with b book plies gives p - b + 1 training positions: every position from ply
b to the final position, inclusive, as the archive's `.count` files already count them.

- Positions b to p - 1 are searched at depth 9. Their score is recorded as today: the root score
  before the move, from the side to move, in pawns (`{0.25}`, `{#N}`), one comment per move.
- The final position is not searched, since its label does not depend on the search. Its label
  is the game result from its side to move: +1 won, 0 drawn, -1 lost (a checkmated side to move
  has -1). It is the base case of AB(lambda).
- The earlier positions, backwards: label = lambda * next label + (1 - lambda) * score, as
  `labels.rs` computes now (decisive scores still give +-1).
- Change from today: the final position was the anchor only and was thrown away; now it is also
  emitted as a training position, with the result as its label. `labels.rs` returns one more
  label (the anchor itself), and `prepare` emits one more position per game and checks moves + 1
  per game against the `.count`. Self-play segments, if any are ever read again, get the same
  treatment: their games also have a final position and a result.

## The output PGN

The whole game from the standard position, no `[FEN]`: the input game with the scores added.

    [Source "ccrl 2014-03 5821"]
    ...
    1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 { Book exit } 4... Nf6 {0.31} 5. O-O {0.27} ... 1-0

- The book moves carry no comment; `{ Book exit }` follows the last one (or precedes the first
  move when there is no book); every later move carries its score, as described above.
- The replay searches with the game's full history, book included, so repetitions are seen
  right.
- `prepare` learns one thing: plays the moves before the marker without emitting positions, then
  one score per move, then the final position with the result. Positions per game: p - b + 1.

## One player, one generation

The player is named as the ladder names it: `hand-crafted`, or `<network>-NNNN`, the promoted
champion of generation NNNN (`parse_network_name` and `network_path` in `match_player.rs`). The
form with a lambda (`amanda-0001-0.70`) is refused: only a promoted champion labels data. A
network player requires `data/networks/<network>/gen-NNNN/champion.txt`: if it or
`champion.nnue` is missing locally, both are copied down from Drive, the `.nnue` first and the
`.txt` last, so a `.txt` is never present without its network; if Drive lacks them too, the
generation was not promoted and the command refuses to start. The player folder is derived from
the name: `amanda-0001` gives `amanda/gen-0001`.

`rotor-arena replay <player>` replays the chunks of one player and exits when they are all done, or when none remain for this machine (the
endgame). No Drive pointer, no waiting for the next champion: the next generation is a new run,
after `sample` has written its chunks. It refuses to start if `sampled/ccrl/<player>/sample.txt`
is missing.

## Workers

As in `self`: one game per logical core within a chunk, each game with its own 16 MB table,
shared by both sides and cleared between games, depth 9 enforced, and the same memory guard.
A single writer collects the finished games and writes them in the chunk's order, so `[Source]`
matches game by game; a game finished early waits in a small buffer (a few games per core) until
its turn.

## Ctrl+C and resuming

A chunk is hours of work, so it resumes where it stopped instead of restarting.

- Invariant: the local staging file `chunk-NNNN.pgn` always holds exactly the first k games of
  the chunk, complete and in order. Games finish out of order; the writer appends game k only
  after games 0 to k - 1, holding early ones in its buffer.
- After each append the writer syncs the staging file, then replaces `chunk-NNNN.progress`
  (`.tmp`, then rename) with k, the positions, and the staging file's byte length L.
- Resuming: read the record, truncate the staging file to L (a crash between an append and the
  record leaves an unrecorded game after L, cut off and searched again), check that the last
  staged game's `[Source]` is the chunk's game k - 1 (a mismatch stops with an error), and
  restart the workers from game k.
- No game is lost (all before k are synced before the record says so) and none is duplicated in
  the output (anything after L is cut). At worst the games in flight and in the buffer are
  searched again, a few per core: accepted.
- Sealing: the chunk's games and positions must equal those `sample.txt` records for it.
- First Ctrl+C: no new game starts, the games in flight finish and are written, and the command
  exits. Second Ctrl+C: the games in flight are aborted and dropped, as are finished games still
  waiting for their turn, and the command exits.
- Either way the claim is kept: the chunk stays this machine's, and on its next start the machine
  resumes it after the last game written. Deleting a claim is done by hand, only to hand a chunk
  over to another machine.
- Finishing a chunk: compress the staging file to `.pgn.zst.tmp`, write the `.count`, rename,
  upload with `remote.rs`, then delete the claim and the staging file.

## Distributing the chunks across machines

Duplicate work is harmless (each output is named after its chunk and written `.tmp` then renamed),
so the aim is to make it rare, not impossible. Drive cannot give exclusivity: sync takes seconds
to minutes, there is no atomic create, and no sync status is visible from WSL.

Each machine, at every pick:

1. Lists the remaining chunks on Drive, in ascending order. Remaining: no output with its
   `.count`, and no claim.
2. Divides them into M equal contiguous parts, M being the number of machines, and takes the
   first chunk of its own part, part k for the machine of index k. The parts are recomputed at
   every pick, from the current listing: nothing is stored between picks.
3. Writes the claim `chunk-NNNN.claim.<machine>` and starts at once, without waiting.

The chunks of a player are its TCEC and CCRL chunks together, listed TCEC first; outputs and
claims go to `data/games/replayed/<tcec|ccrl>/<player>/`.
4. On finishing the chunk (output and `.count` on Drive), deletes its claim. Ctrl+C keeps it
   (see Ctrl+C and resuming).

Why it holds:

- The machines work about R/M chunks apart (R remaining). A view missing s recent claims moves the
  boundaries by at most s, so a collision needs s >= R/M: only at the very end, which the
  endgame below removes.
- An offline machine blocks nothing: its part is recomputed from what remains, and the others'
  boundaries move over its chunks as they finish theirs. Machines offline for days need no
  special handling.
- A claim never expires: no lease, nothing to tune. A crash leaves at most one claim per machine,
  blocking one chunk. On start, a machine first resumes the chunks it still claims, so the
  chunk comes back when its machine does; a machine that never comes back has its claim deleted
  by hand.

Active machines: with K machines and R remaining chunks, only the first
min(floor(R / 3), K) machines of the list pick, the fastest ones, and the remaining chunks are
divided among them alone, so every part holds at least 3 chunks and the machines stay that far
apart. The special case 1 <= R < 3 has the fastest machine alone. So the slowest drop out one by
one as the work runs out: with K = 3, all three while R >= 9, the first two for 6 <= R < 9, the
fastest alone below 6. A machine past the active count finishes the chunk it holds and stops.
Every pick writes a claim. (This replaces an earlier endgame, all machines until R < 3K, then the
fastest alone without claims.)

M and each machine's index come from `data/machines.txt` on the remote, shared by every machine
(`1: the training machine`, `2: the Lichess machine`, `3: another machine`, numbered from 1), ordered by speed: the first is the fastest,
the one that finishes the last chunks alone. A machine not listed exactly once refuses to start.
`data/machine.yaml` is unchanged: only `name`.

Rejected along the way: a single `.claimed` file (overwritten by two machines, stuck after a
crash, as it has no owner to resume it); claims with a winner rule (a sync wait per file); a
coordinator that reassigns (fragile with machines offline for days, races on reassignment); a
database on Drive (SQLite locks do not work over a synced folder); a git branch as a lock (works,
but misuses GitHub and fails once the repository is public); fixed lanes by chunk number modulo M
(an offline machine's lane stalls).

## Requirements from ccrl-replay-sampling.md

- Every output keeps each game's `[Source]`, in the chunk's order, so `prepare` can compare them.
- The output's `.count` is p - b + 1 per game, summed, equal to the chunk's recorded positions.

## Changes to `rotor-train prepare`

- It reads `replayed/tcec/<player>/` and `replayed/ccrl/<player>/`, in chunk order, TCEC first,
  instead of self-play segments ordered by timestamp and machine.
- Completeness is the `sample.txt` check (ccrl-replay-sampling.md), instead of the `.count` sum
  passing `corpus_positions`.
- It plays the moves before `{ Book exit }` without emitting positions, emits one position per
  scored move, then the final position with the result as its label: p - b + 1 per game,
  checked against the `.count`.
- It deletes the sampled CCRL chunks once the check has passed.
