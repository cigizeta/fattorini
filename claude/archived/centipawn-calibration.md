# Search(9) labeling dataset generation

**Archived: `rotor-curate` and its Lumbra SQLite pipeline are gone**, replaced by `rotor-arena
replay` over TCEC/CCRL chunks plus self-play (corpus-windows.md). What carried over: a depth-9 score
for every ply, a transposition table per worker kept warm through a game.

## Status

Built and running: `rotor-curate generate` populates a SQLite database from the full Lumbra
corpus, resumable across sessions. `rotor-train prepare` (`training.md`) is the sole downstream
consumer.

## Scope

This document covers only `rotor-curate`'s dataset-generation pipeline: a bounded search run over
every ply of a real historical game corpus, and the SQLite database that run produces. The
centipawn conversion applied downstream to this data lives in `training.md`'s "Centipawn scaling"
section, not here.

## Source

Lumbra's Gigabase OTB Elite file (`data/lumbra/LumbrasGigaBase_OTB_ELO2400.pgn`, 729 MB,
over-the-board games, both players rated 2400+, CC BY-NC-SA 4.0). 863,790 games, 70,963,607 plies
total. No game filter: `Result` alone (`1-0`/`0-1`/`1/2-1/2`) decides every position's outcome
label.

PGN parsing via `pgn-reader`; SAN-to-move resolution goes entirely through Rotor's own board and
move generator (`resolve_san`, matching each parsed SAN against `legal_moves` by destination,
piece kind, promotion, and disambiguation).

## Search

For every ply of every replayed game: a real, bounded search, fixed depth 9, no time budget, no
stability test, producing the backed-up score `Search(9)` `training.md`'s own AB(lambda) needs.

## Schema

Two tables, no board or FEN stored anywhere (positions are reconstructed by replaying the PGN):

```sql
CREATE TABLE games (
    game_id    INTEGER PRIMARY KEY,
    outcome    REAL NOT NULL,   -- White-relative: 0, 0.5, or 1
    ply_count  INTEGER NOT NULL
);

CREATE TABLE plies (
    game_id    INTEGER NOT NULL,
    ply_index  INTEGER NOT NULL,  -- 0-based
    score      INTEGER NOT NULL,  -- raw Search(9), White-relative, every ply, mate-scale included
    PRIMARY KEY (game_id, ply_index)
);
```

`score` is stored raw (mate-scale sentinel values included) for every ply of every game -- the
conversion to a training label runs downstream, at read time, never here.

## Parallelization

One main thread reads the PGN sequentially through `pgn-reader` and dispatches each parsed game,
round robin, to one bounded channel per worker (capacity 2). Each worker replays its own game
against Rotor's own board and move generator, runs `Search(9)` at every ply, and sends completed
rows plus stats back over a second channel, tagged with sequence number and end-of-game byte
offset. A mandatory `--threads N` flag, no default. One persistent `TranspositionTable` per
worker (`TT_SIZE_MB = 64`), allocated once and reused across every game that worker processes;
`clear()`ed at the *start* of the next game, not the end of the previous one.

## Reassembly, restart, and progress

**Reassembly.** Workers finish out of order; the main thread buffers each arriving result
(`HashMap<sequence, result>` plus a `next_to_write` counter) and drains every consecutive
already-arrived sequence starting from `next_to_write`, inserting rows and persisting that game's
end offset as the new resume point.

**Restart.** The resume point is a one-row `resume` table inside the same SQLite database: each
game's `games`/`plies` rows and its `resume` row update commit inside one transaction. The offset
comes from `Reader`'s own `stream_position()`.

**Ctrl+C aborts in-flight games, not just future ones.** A shared `Arc<AtomicBool>` is passed as
every worker's `stop_signal` into `choose_best_move`. A worker's current game is discarded
outright, never sent, and retried on the next restart.

**Progress reporting: games completed, not bytes read.** A cheap pre-scan (`count_games`) gives a
real total and, on a resumed run, how many games lie before the resume offset. The bar's position
advances from `stats.count` inside the same reassembly step that inserts rows; a custom
average-rate template key (`pos / elapsed`, seeded correctly on resume) matches plain
`tqdm`-style `smoothing=0` semantics.

## Downstream consumption

`rotor-train`'s feature-cache builder is the only reader. Per game, it reads that game's `score`s
from `plies` and its `outcome` from `games`, runs AB(lambda)'s backward `L_t` recursion directly
against them (`training.md`), and replays the PGN (the only other place it's ever replayed) to
attach the resulting label to each actual position as it extracts features into its own
disk-backed sparse cache. No corpus-wide aggregate needs computing first. Train/test split
happens per *game*, not per position.
