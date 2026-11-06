//! `results.db` -- the shared results database every arena run writes alongside its
//! own PGN (`data/tournaments/<id>/results.db` next to `games.pgn`, for now -- see
//! the tournament design's "Results database and resumability" section). One table, per-game rows,
//! normalized `(round, engine_one, engine_two)` rather than a free-form pairing id, decided
//! directly with the developer. This table *is* the resumable state: a pairing's current score
//! is just a query over its own rows, with no separate mutable bracket state to keep in sync.

use std::path::Path;

use rusqlite::Connection;

use super::pgn::Termination;

/// Whether `termination` (as stored in the database, i.e. `Termination::as_str`'s own output)
/// is one of the two fault outcomes -- used by the resumability path (`pairing.rs`'s
/// `fault_winner`) to recognize an already-faulted, already-decided pairing without needing a
/// `Termination` value in hand, since a raw `SELECT` only ever gives back a `String`.
pub fn is_fault_termination(termination: &str) -> bool {
    termination == Termination::Crashed.as_str() || termination == Termination::IllegalMove.as_str()
}

pub struct GameRecord<'a> {
    pub round: u32,
    pub engine_one: &'a str,
    pub engine_two: &'a str,
    pub game_index: u32,
    pub white: &'a str,
    pub black: &'a str,
    /// White's own score: `1.0`/`0.5`/`0.0`. A fault always produces a decisive result for the
    /// non-faulting side -- there is no such thing as a drawn fault.
    pub result: f64,
    pub termination: Termination,
    pub ply_count: u32,
}

/// Opens (creating if needed) `path`'s own database, creating its parent directory first if
/// this is the first game written for this run: `Connection::open` fails outright against a
/// missing directory, unlike a missing file.
pub fn open(path: &str) -> Connection {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|error| panic!("failed to create directory {}: {error}", parent.display()));
    }
    let connection = Connection::open(path).unwrap_or_else(|error| panic!("failed to open database {path}: {error}"));
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS games (
                game_id     INTEGER PRIMARY KEY,
                round       INTEGER NOT NULL,
                engine_one  TEXT NOT NULL,
                engine_two  TEXT NOT NULL,
                game_index  INTEGER NOT NULL,
                white       TEXT NOT NULL,
                black       TEXT NOT NULL,
                result      REAL NOT NULL,
                termination TEXT NOT NULL,
                ply_count   INTEGER NOT NULL
            );",
        )
        .expect("failed to create the results database schema");
    connection
}

/// Inserts one game's own row, in its own transaction -- so a Ctrl+C-dropped in-flight game
/// (never reaching this call at all) simply never gets a row, rather than risking a
/// half-written one.
pub fn insert_game(connection: &mut Connection, record: &GameRecord) {
    let transaction = connection.transaction().expect("failed to start a database transaction");
    insert_row(&transaction, record);
    transaction.commit().expect("failed to commit a game's row");
}

fn insert_row(transaction: &rusqlite::Transaction, record: &GameRecord) {
    transaction
        .execute(
            "INSERT INTO games (round, engine_one, engine_two, game_index, white, black, result, termination, ply_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (
                record.round,
                record.engine_one,
                record.engine_two,
                record.game_index,
                record.white,
                record.black,
                record.result,
                record.termination.as_str(),
                record.ply_count,
            ),
        )
        .expect("failed to insert a games row");
}

/// Inserts both games of a completed opening pair in **one** transaction, which is what keeps the
/// database holding only whole pairs.
///
/// That invariant is what makes a resumed SPRT run correct without special cases: the test is
/// recomputed from the recorded games, and a colour-unbalanced fragment left by a crash between a
/// pair's two games would be a set of games no run could legitimately have produced. Writing the pair
/// atomically means the recomputation never has to recognize or discard one.
pub fn insert_pair(connection: &mut Connection, first: &GameRecord, second: &GameRecord) {
    let transaction = connection.transaction().expect("failed to start a database transaction");
    insert_row(&transaction, first);
    insert_row(&transaction, second);
    transaction.commit().expect("failed to commit a pair's rows");
}

/// One recorded game, with enough of its row to reconstruct a sequential test from it.
///
/// `white` rather than a derived colour: which engine had which colour is a recorded fact, and reading
/// it back is strictly better than re-deriving it from a convention about `game_index` parity that a
/// duplicate or missing row could violate without anything noticing.
pub struct GameRow {
    pub game_index: u32,
    pub white: String,
    /// White's own score.
    pub result: f64,
}

/// Every recorded game of one pairing, in `game_index` order, with its index and White's name.
/// `pairing_games` above is the same query narrowed to what the best-of rule needs.
pub fn pairing_game_rows(connection: &Connection, round: u32, engine_one: &str, engine_two: &str) -> Vec<GameRow> {
    let mut statement = connection
        .prepare(
            "SELECT game_index, white, result FROM games
             WHERE round = ?1 AND engine_one = ?2 AND engine_two = ?3
             ORDER BY game_index ASC",
        )
        .expect("failed to prepare the pairing_game_rows query");
    let rows = statement
        .query_map((round, engine_one, engine_two), |row| {
            Ok(GameRow {
                game_index: row.get(0)?,
                white: row.get(1)?,
                result: row.get(2)?,
            })
        })
        .expect("failed to run the pairing_game_rows query");
    rows.map(|row| row.expect("failed to read a games row")).collect()
}

/// Every already-played game's own `(white_result, termination)` for one pairing, in
/// `game_index` order -- the pairing's current state (running score, whether it's already
/// decided by a fault, how many games remain before the tiebreak rule kicks in) is derived from
/// this by the caller, not stored separately.
pub fn pairing_games(connection: &Connection, round: u32, engine_one: &str, engine_two: &str) -> Vec<(f64, String)> {
    let mut statement = connection
        .prepare(
            "SELECT result, termination FROM games
             WHERE round = ?1 AND engine_one = ?2 AND engine_two = ?3
             ORDER BY game_index ASC",
        )
        .expect("failed to prepare the pairing_games query");
    let rows = statement
        .query_map((round, engine_one, engine_two), |row| Ok((row.get::<_, f64>(0)?, row.get::<_, String>(1)?)))
        .expect("failed to run the pairing_games query");
    rows.map(|row| row.expect("failed to read a games row")).collect()
}
