//! The labelling of scored games: a `.pgn.zst` segment read game by game (`SelfPlaySegmentReader`),
//! each game's training positions replayed (`self_play_game_positions`) and labelled by the direct
//! blend (`self_play_game_labels`), as `pgn_stream` streams them into training. Beside them,
//! `RoundProgress`, the in-progress round a training run backs up.
//!
//! Every game is one written by `arena`'s self-play or replay, or converted from Lc0's data by
//! the smithy's Convert job: a `{score}` comment after every move past the book, mover-relative pawns, optionally
//! followed by the position's best move in UCI.

use std::fs::File as StdFile;
use std::io::{BufReader, BufWriter, Read, Write};
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pgn_reader::{RawComment, RawTag, Reader, SanPlus, Visitor};

use temper_chess::board::color::Color;
use temper_chess::board::Board;
use temper_chess::movegen::legal::legal_moves;
use temper_chess::search::MATE_SCORE;
use temper_chess::tablebase::{SyzygyTablebase, TbResult};
use temper_chess::uci::parse_move;

use temper_chess::features::{extract_canonical, CanonicalPosition};
use temper_chess::labels::{compute_labels_direct, outcome_label, side_to_move_relative, Curve};
use pgn_replay::STARTING_POSITION_FEN;

/// How often the training loop backs up `model.vs`/`optimizer.opt` mid-round: "a crash should lose
/// at most a few minutes of work". Named for what it actually is -- a resumable backup of
/// still-in-progress state -- not "checkpoint", which this codebase reserves for the separate,
/// immutable, per-round archive in `checkpoints/`.
pub const BACKUP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5 * 60);

fn read_u64(reader: &mut impl Read) -> u64 {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes).expect("failed to read a u64 field");
    u64::from_le_bytes(bytes)
}

/// One labelled game: its starting position (`[FEN]`, or the standard one), its moves, the book plies
/// before `{ Book exit }` (0 without one), each later move's own `{score}` comment (mover-relative
/// pawns, as `arena replay` and self-play write them), and the real outcome.
/// `scores_pawns.len() == moves.len() - book`: every move after the
/// book carries a score, and the book moves carry none. Its training positions are every position
/// from ply `book` to the final one, inclusive: the final position has no score, and its label is
/// the outcome. `pub(crate)` so that `pgn_stream` can hold and move one; its fields stay private,
/// constructed and consumed only inside this module.
pub(crate) struct CollectedSelfPlayGame {
    starting_fen: String,
    moves: Vec<SanPlus>,
    book: usize,
    scores_pawns: Vec<f64>,
    /// Beside each score, the best move of the position it belongs to, in UCI, as generation 4's
    /// replay and self-play write it after the score (`{0.31 g8f6}`); `None` in older corpora.
    best_moves: Vec<Option<String>>,
    /// Beside each score, Lc0's visit count of every legal move, the moves sorted by their UCI text,
    /// when the comment carries them (`{0.31 g8f6 12,0,...}`, Lc0's converted corpus): checked, one
    /// per legal move, so a corrupt comment fails before training.
    counts: Vec<Option<Vec<u32>>>,
    outcome: f64,
}

impl CollectedSelfPlayGame {
    /// How many training records this game will produce once labelled and featurized: one per move
    /// after the book, and one for the final position. Readable without replaying a single move -- no
    /// `Board`, no feature extraction, none of the expensive half.
    pub(crate) fn position_count(&self) -> usize {
        self.moves.len() - self.book + 1
    }

    /// Each searched position's score, in pawns for its side to move, in ply order after the book:
    /// for the measurement of lambda by progress (`polyline`), its length a game's searched count n.
    pub(crate) fn scores_pawns(&self) -> &[f64] {
        &self.scores_pawns
    }
}

/// Collects one `CollectedSelfPlayGame` per game off a scored `.pgn.zst`'s own `Reader`.
/// `starting_fen`/`outcome` are reset in `begin_tags` (the true "a new game is starting" hook), not
/// left to whatever the previous game happened to leave behind, in case a game were ever missing a
/// tag its own PGN writer normally always includes.
struct SelfPlayGameCollector {
    starting_fen: String,
    moves: Vec<SanPlus>,
    book: Option<usize>,
    scores_pawns: Vec<f64>,
    best_moves: Vec<Option<String>>,
    counts: Vec<Option<Vec<u32>>>,
    outcome: f64,
}

impl SelfPlayGameCollector {
    fn new() -> SelfPlayGameCollector {
        SelfPlayGameCollector {
            starting_fen: STARTING_POSITION_FEN.to_string(),
            moves: Vec::new(),
            book: None,
            scores_pawns: Vec::new(),
            best_moves: Vec::new(),
            counts: Vec::new(),
            outcome: 0.0,
        }
    }
}

/// A comment's visit counts, `c1,c2,...`: whole counts, at least one.
fn parse_counts(text: &str) -> Option<Vec<u32>> {
    text.split(',').map(|count| count.parse().ok()).collect()
}

/// The comment `arena replay` places after a game's book, TCEC's own convention.
const BOOK_EXIT_MARKER: &str = "book exit";

impl Visitor for SelfPlayGameCollector {
    type Tags = ();
    type Movetext = ();
    type Output = CollectedSelfPlayGame;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        self.starting_fen = STARTING_POSITION_FEN.to_string();
        self.outcome = 0.0;
        ControlFlow::Continue(())
    }

    /// Neither `FEN`'s value (a plain board/side/castling/en-passant/clock string) nor `Result`'s
    /// three values ever contain a quote or backslash, so reading `value.as_bytes()` directly is safe
    /// here; `RawTag::decode()` (which un-escapes `\"`/`\\`) only matters for tags like player names
    /// that can actually contain them.
    fn tag(&mut self, _tags: &mut Self::Tags, name: &[u8], value: RawTag<'_>) -> ControlFlow<Self::Output> {
        if name == b"FEN" {
            self.starting_fen = String::from_utf8(value.as_bytes().to_vec())
                .unwrap_or_else(|error| panic!("invalid UTF-8 in a self-play PGN's own [FEN] tag: {error}"));
        } else if name == b"Result" {
            self.outcome = match value.as_bytes() {
                b"1-0" => 1.0,
                b"0-1" => 0.0,
                b"1/2-1/2" => 0.5,
                other => panic!("unexpected Result tag value in a self-play PGN: {:?}", String::from_utf8_lossy(other)),
            };
        }
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, _tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        self.moves.clear();
        self.book = None;
        self.scores_pawns.clear();
        self.best_moves.clear();
        self.counts.clear();
        ControlFlow::Continue(())
    }

    fn san(&mut self, _movetext: &mut Self::Movetext, san_plus: SanPlus) -> ControlFlow<Self::Output> {
        self.moves.push(san_plus);
        ControlFlow::Continue(())
    }

    /// Self-play writes a bare `{0.25}`-shaped comment (score only, never a depth/time suffix) for an
    /// ordinary or tablebase-scale score, or `{#7}`-shaped `#N` mate notation for a real forced mate.
    /// Trimming any leading/trailing brace before parsing is defensive, not load-bearing either way
    /// (`pgn_reader`'s own `RawComment` may or may not include them, unconfirmed against its source).
    /// A `#N` comment reconstructs to a pawns value pinned at `MATE_SCORE` itself (signed by `N`'s own
    /// sign), not an attempt to invert self-play's own `mate_in` conversion (`moves_to_mate =
    /// (ply_at_mate + 1) / 2` loses information -- both `ply_at_mate = 2N - 1` and `2N - 2` map to the
    /// same `N`, so there is no single correct ply to recover) -- unnecessary anyway, since all the
    /// labelling downstream ever needs is a score decisively in the mate/TB band with the right sign,
    /// and `MATE_SCORE` (the top of that band) always qualifies regardless of `N`.
    ///
    /// From generation 4 the score may be followed by a space and the best move of the position it
    /// belongs to, in UCI (`{0.31 g8f6}`): kept as text here, resolved against the position when the
    /// game is replayed (`self_play_game_positions`). Lc0's converted corpus may add a third field,
    /// the visit counts of every legal move (`{0.31 g8f6 12,0,7}`): kept, and checked against the
    /// position's legal moves when the game is replayed. Anything more fails, as a bad score does.
    fn comment(&mut self, _movetext: &mut Self::Movetext, comment: RawComment<'_>) -> ControlFlow<Self::Output> {
        let text = String::from_utf8_lossy(comment.as_bytes());
        let comment_text = text.trim().trim_start_matches('{').trim_end_matches('}').trim();
        if comment_text.to_lowercase().contains(BOOK_EXIT_MARKER) {
            assert!(self.book.is_none(), "a game with two Book exit markers");
            assert!(self.scores_pawns.is_empty(), "a score comment inside a game's book");
            self.book = Some(self.moves.len());
            return ControlFlow::Continue(());
        }
        let mut fields = comment_text.split_whitespace();
        let trimmed = fields.next().unwrap_or("");
        let best_move = fields.next().map(str::to_string);
        let counts = fields.next().map(|text| {
            parse_counts(text).unwrap_or_else(|| panic!("a move comment {comment_text:?} holds visit counts {text:?} that are not whole counts"))
        });
        assert!(fields.next().is_none(), "a move comment {comment_text:?} holds more than a score, a best move and its counts");
        assert!(counts.is_none() || best_move.is_some(), "visit counts without a best move");
        self.best_moves.push(best_move);
        self.counts.push(counts);
        let pawns: f64 = if let Some(mate_in) = trimmed.strip_prefix('#') {
            let sign = if mate_in.starts_with('-') { -1.0 } else { 1.0 };
            sign * MATE_SCORE as f64 / 100.0
        } else {
            trimmed.parse().unwrap_or_else(|error| {
                panic!("failed to parse a self-play PGN's own move comment {trimmed:?} as a decimal pawns score: {error}")
            })
        };
        self.scores_pawns.push(pawns);
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, _movetext: Self::Movetext) -> Self::Output {
        CollectedSelfPlayGame {
            starting_fen: self.starting_fen.clone(),
            moves: std::mem::take(&mut self.moves),
            book: self.book.take().unwrap_or(0),
            scores_pawns: std::mem::take(&mut self.scores_pawns),
            best_moves: std::mem::take(&mut self.best_moves),
            counts: std::mem::take(&mut self.counts),
            outcome: self.outcome,
        }
    }
}

/// Opens a scored segment's own compressed PGN for replay: a `zstd::Decoder` sits between the raw
/// file and the `Reader`, since every `.pgn.zst` segment is a complete, independent zstd stream. No
/// `_at(offset)` counterpart: a zstd stream can't be cheaply seeked to an arbitrary byte offset and
/// resumed mid-decompression.
fn open_self_play_pgn_reader(pgn_path: &str) -> std::io::Result<SegmentPgnReader> {
    Ok(Reader::new(zstd::Decoder::new(StdFile::open(pgn_path)?)?))
}

type SegmentPgnReader = Reader<zstd::Decoder<'static, BufReader<StdFile>>>;

/// How a segment reader waits out a read that fails: segments live on Google Drive's `G:`, where a
/// read fails while Drive has not finished syncing the file or while the machine is offline. The
/// program cannot tell which (both are the same I/O error), and waiting is the answer to both: the
/// reader waits `pause`, reopens the segment and tries again, with no limit, until a read succeeds or
/// `stop` is set (decided 2026-11-04, after a scatter died on a segment Drive had not synced).
#[derive(Clone, Debug)]
pub(crate) struct ReadWait {
    pub stop: Option<Arc<AtomicBool>>,
    pub pause: Duration,
}

impl ReadWait {
    /// Retry every minute, until a read succeeds or the process ends.
    pub(crate) fn forever() -> ReadWait {
        ReadWait { stop: None, pause: Duration::from_secs(60) }
    }

    /// Retry every minute, until a read succeeds or `stop` is set.
    pub(crate) fn until(stop: Arc<AtomicBool>) -> ReadWait {
        ReadWait { stop: Some(stop), pause: Duration::from_secs(60) }
    }

    fn stopped(&self) -> bool {
        self.stop.as_ref().is_some_and(|stop| stop.load(Ordering::SeqCst))
    }
}

/// A read given up because the job was stopped while it waited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stopped;

/// A segment read failing since `since`, shown on the page while held.
struct Waiting {
    since: Instant,
    notice: progress::Notice,
}

impl Waiting {
    /// Shows and logs the failure, then waits `wait.pause`, checking `wait.stop` every second.
    fn wait(waiting: &mut Option<Waiting>, wait: &ReadWait, pgn_path: &str, error: &std::io::Error) -> Result<(), Stopped> {
        let since = waiting.as_ref().map_or_else(Instant::now, |waiting| waiting.since);
        let minutes = since.elapsed().as_secs() / 60;
        let text = format!(
            "cannot read {pgn_path} from Google Drive for {minutes} minutes, retrying every {} s: {error}. Drive may still be syncing, or offline",
            wait.pause.as_secs()
        );
        log::warn!("{text}");
        match waiting {
            Some(waiting) => waiting.notice.set(text),
            None => *waiting = Some(Waiting { since, notice: progress::notice(text) }),
        }
        let deadline = Instant::now() + wait.pause;
        loop {
            if wait.stopped() {
                return Err(Stopped);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(());
            }
            std::thread::sleep(left.min(Duration::from_secs(1)));
        }
    }
}

/// One segment opened for sequential, game-by-game replay, so the `pgn_reader`/`zstd` machinery above
/// stays inside this module. A failed open or read is waited out (`ReadWait`): the segment is reopened
/// from its start, a zstd stream being unseekable, and the games already delivered are read again and
/// skipped, so the games delivered are exactly those of an undisturbed read. Parse errors, data bugs no
/// wait cures, still panic at once.
pub(crate) struct SelfPlaySegmentReader {
    reader: SegmentPgnReader,
    collector: SelfPlayGameCollector,
    next_game_index: u64,
    wait: ReadWait,
}

impl SelfPlaySegmentReader {
    /// Opens `pgn_path` from the very beginning, waiting forever if it cannot be read
    /// (`ReadWait::forever`): for callers with no stop to honour.
    pub(crate) fn open(pgn_path: &str) -> SelfPlaySegmentReader {
        Self::open_waiting(pgn_path, ReadWait::forever()).expect("a wait without a stop never stops")
    }

    /// Opens `pgn_path` from the very beginning, waiting out failures as `wait` says.
    pub(crate) fn open_waiting(pgn_path: &str, wait: ReadWait) -> Result<SelfPlaySegmentReader, Stopped> {
        let mut waiting = None;
        let reader = loop {
            match open_self_play_pgn_reader(pgn_path) {
                Ok(reader) => break reader,
                Err(error) => Waiting::wait(&mut waiting, &wait, pgn_path, &error)?,
            }
        };
        Ok(SelfPlaySegmentReader { reader, collector: SelfPlayGameCollector::new(), next_game_index: 0, wait })
    }

    /// This segment's next game, along with its own 0-based index within the file. `None` once the
    /// segment is exhausted. Waits forever on failures if opened by `open`.
    pub(crate) fn next_game(&mut self, pgn_path: &str) -> Option<(u64, CollectedSelfPlayGame)> {
        self.next_game_waiting(pgn_path).expect("a wait without a stop never stops")
    }

    /// `next_game`, or `Stopped` if the reader's stop was set while a failed read waited.
    pub(crate) fn next_game_waiting(&mut self, pgn_path: &str) -> Result<Option<(u64, CollectedSelfPlayGame)>, Stopped> {
        let mut waiting = None;
        loop {
            match self.reader.read_game(&mut self.collector) {
                Ok(Some(collected)) => {
                    let game_index = self.next_game_index;
                    self.next_game_index += 1;
                    return Ok(Some((game_index, collected)));
                }
                Ok(None) => return Ok(None),
                Err(error) => {
                    Waiting::wait(&mut waiting, &self.wait, pgn_path, &error)?;
                    self.reopen(pgn_path, &mut waiting)?;
                }
            }
        }
    }

    /// The segment opened again and read up to `next_game_index`, the games skipped: where the failed
    /// read stood. Each failure on the way waits as the first did.
    fn reopen(&mut self, pgn_path: &str, waiting: &mut Option<Waiting>) -> Result<(), Stopped> {
        'attempt: loop {
            let mut reader = match open_self_play_pgn_reader(pgn_path) {
                Ok(reader) => reader,
                Err(error) => {
                    Waiting::wait(waiting, &self.wait, pgn_path, &error)?;
                    continue;
                }
            };
            let mut collector = SelfPlayGameCollector::new();
            for _ in 0..self.next_game_index {
                match reader.read_game(&mut collector) {
                    Ok(Some(_)) => {}
                    Ok(None) => panic!("{pgn_path} holds fewer games than the {} already read from it", self.next_game_index),
                    Err(error) => {
                        Waiting::wait(waiting, &self.wait, pgn_path, &error)?;
                        continue 'attempt;
                    }
                }
            }
            self.reader = reader;
            self.collector = collector;
            return Ok(());
        }
    }
}

/// The side to move in each training position of `collected`, from ply `book` to the final position,
/// inclusive: the starting position's side at even plies, the other at odd ones. Side to move
/// strictly alternates every ply in standard chess (no double-moves), so this needs no replay of the
/// game -- which is what lets a label pass skip the board entirely (`self_play_game_labels`).
fn self_play_sides_to_move(collected: &CollectedSelfPlayGame, game_index: u64) -> Vec<Color> {
    let (board, _fullmove_number) = Board::from_fen(&collected.starting_fen)
        .unwrap_or_else(|error| panic!("game {game_index} has an invalid [FEN] tag: {error:?}"));
    let starting = board.side_to_move();
    (collected.book..=collected.moves.len()).map(|i| if i % 2 == 0 { starting } else { starting.opposite() }).collect()
}

/// One game's labels at `lambda`, the direct blend (`labels::compute_labels_direct`), one per training
/// position (`position_count`: every move after the book, and the final position labelled with the
/// outcome), each from its own side to move -- the frame every feature of the position beside it is
/// expressed in (`labels::side_to_move_relative`'s own doc comment has the measurement that followed
/// from getting this wrong). Its scores read through `curve`, the teacher's (Lc0's curve for Lc0's
/// corpus, `labels::LOGISTIC` for PeSTO and our networks). No board is replayed, so labelling costs
/// only the parse and the blend.
pub(crate) fn self_play_game_labels(collected: &CollectedSelfPlayGame, lambda: f64, game_index: u64, curve: &Curve) -> Vec<f32> {
    assert!(collected.book <= collected.moves.len(), "game {game_index}: its book is longer than the game");
    assert_eq!(
        collected.scores_pawns.len(), collected.moves.len() - collected.book,
        "game {game_index} has {} moves after its book but {} score comments -- every one must carry one",
        collected.moves.len() - collected.book, collected.scores_pawns.len()
    );
    let sides = self_play_sides_to_move(collected, game_index);
    let white_relative_scores: Vec<i32> = collected
        .scores_pawns
        .iter()
        .zip(&sides)
        .map(|(&pawns, &side)| {
            let cp = (pawns * 100.0).round() as i32;
            if side == Color::White { cp } else { -cp }
        })
        .collect();
    // The searched positions get the blend; the final position, never searched, the outcome itself.
    let mut white_relative_labels = if white_relative_scores.is_empty() {
        Vec::new()
    } else {
        compute_labels_direct(&white_relative_scores, collected.outcome, lambda, curve)
    };
    white_relative_labels.push(outcome_label(collected.outcome));
    white_relative_labels
        .iter()
        .zip(&sides)
        .map(|(&label, &side)| side_to_move_relative(label, side) as f32)
        .collect()
}

/// What a training set or a test set keeps of a game, as a frozen set's manifest and a bucket
/// directory's state record it: every searched position, all but the final one. No move is played
/// from the final position, so no search ever labelled it, and its label is the result alone; the
/// direct blend labels each position with the result itself, so the final one carries nothing (the
/// developer, 2026-10-28). A record naming anything else is another set's, as the quiet filter's
/// were, removed on 2026-11-01 after it hurt the networks it trained.
pub(crate) const KEPT: &str = "searched";

/// One game's training positions, replayed from its starting FEN: the position before every move
/// after the book, then the final position. Every one of them, as the labels are
/// (`self_play_game_labels`); `self_play_game_kept` says which a training set or a test set keeps.
/// For the tests and the diagnostics: training and the freezer read `self_play_game_kept`.
#[cfg(test)]
pub(crate) fn self_play_game_positions(collected: &CollectedSelfPlayGame, game_index: u64) -> Vec<CanonicalPosition> {
    self_play_game_kept(collected, game_index).0
}

/// One game's training positions, as `self_play_game_positions`, and beside each whether a training
/// set or a test set keeps it (`KEPT`: every searched one). Asserts each position's side to move is
/// the one `self_play_game_labels` assumes, so a label and the position it is written beside cannot
/// describe different perspectives, and that every best move a comment carries is legal in its
/// position, with one visit count per legal move: a corrupt comment fails here rather than training
/// silently.
pub(crate) fn self_play_game_kept(collected: &CollectedSelfPlayGame, game_index: u64) -> (Vec<CanonicalPosition>, Vec<bool>) {
    let mut positions = Vec::with_capacity(collected.position_count());
    let mut kept = Vec::with_capacity(collected.position_count());
    let (mut board, _fullmove_number) = Board::from_fen(&collected.starting_fen)
        .unwrap_or_else(|error| panic!("game {game_index} has an invalid [FEN] tag: {error:?}"));
    let sides = self_play_sides_to_move(collected, game_index);
    // Every position from ply `book` to the final one, inclusive: the book is played, not emitted.
    for ply in 0..=collected.moves.len() {
        // The book's plies carry no comment, and the final position no move.
        let comment_best = if ply >= collected.book { collected.best_moves.get(ply - collected.book) } else { None };
        if let Some(best) = comment_best.and_then(Option::as_ref) {
            let legal = legal_moves(&board, board.side_to_move());
            parse_move(best, &legal)
                .unwrap_or_else(|| panic!("game {game_index} ply {ply}: best move {best:?} is not legal at FEN {}", board.to_fen(1)));
            // The counts are checked here, where the position is known: one per legal move.
            if let Some(counts) = collected.counts.get(ply - collected.book).and_then(Option::as_ref) {
                assert_eq!(
                    counts.len(),
                    legal.len(),
                    "game {game_index} ply {ply}: {} visit counts for {} legal moves at FEN {}",
                    counts.len(),
                    legal.len(),
                    board.to_fen(1)
                );
            }
        }
        if ply >= collected.book {
            let canonical = extract_canonical(&board);
            assert_eq!(
                canonical.side_to_move,
                sides[ply - collected.book],
                "game {game_index} ply {ply}: the side to move does not alternate"
            );
            positions.push(canonical);
            kept.push(ply < collected.moves.len());
        }
        if let Some(played) = collected.moves.get(ply) {
            // The games were replayed by Temper itself, so every move is legal: a miss is a bug.
            let mv = pgn_replay::try_resolve_san(&board, &played.san).unwrap_or_else(|| {
                panic!(
                    "game {game_index} ply {ply}: SAN {:?} names no single legal move at FEN {}",
                    played.san,
                    board.to_fen(1)
                )
            });
            board.make_move(mv);
        }
    }
    (positions, kept)
}

/// The most pieces the Syzygy sets `bin/syzygy.sh` downloads cover: a position with more is never
/// probed, which keeps the replay below cheap.
const TABLEBASE_PIECES: u32 = 6;

/// One game's tablebase labels, one per training position as `self_play_game_labels`: the tables'
/// result under the fifty-move rule (`label_result`, given every earlier position of the game), from
/// the side to move, 1 a win, 0 a draw, -1 a loss; `None` where a position has more pieces than the
/// tables cover, or they cannot answer (castling rights, a missing table). Within the tables the true
/// value is known, so it replaces the label at every lambda, the search's score and the game's result
/// alike: Lc0's own values there were never rescored, and self-play's search probes only after a
/// zeroing move (the developer, 2026-11-03).
pub(crate) fn self_play_game_tablebase(
    collected: &CollectedSelfPlayGame, tablebase: &SyzygyTablebase, game_index: u64,
) -> Vec<Option<f32>> {
    let mut labels = Vec::with_capacity(collected.position_count());
    let (mut board, _fullmove_number) = Board::from_fen(&collected.starting_fen)
        .unwrap_or_else(|error| panic!("game {game_index} has an invalid [FEN] tag: {error:?}"));
    let mut ancestors: Vec<u64> = Vec::with_capacity(collected.moves.len());
    for ply in 0..=collected.moves.len() {
        if ply >= collected.book {
            let label = (board.all_occupancy().count() <= TABLEBASE_PIECES)
                .then(|| tablebase.label_result(&board, &ancestors))
                .flatten()
                .map(|result| match result {
                    TbResult::Win => 1.0,
                    TbResult::Draw => 0.0,
                    TbResult::Loss => -1.0,
                });
            labels.push(label);
        }
        if let Some(played) = collected.moves.get(ply) {
            let mv = pgn_replay::try_resolve_san(&board, &played.san)
                .unwrap_or_else(|| panic!("game {game_index} ply {ply}: SAN {:?} names no single legal move", played.san));
            ancestors.push(board.hash());
            board.make_move(mv);
        }
    }
    labels
}

/// The in-progress round a training run backs up beside `model.vs`, so a resume continues the round's
/// loss and timing. A streamed run writes `train_cursors` empty and `rows_done` zero: its batches are
/// never short, so both are exact functions of `optimizer.opt`'s own persisted step count and are
/// derived on the way back in. What it does still need from here is `loss_sum` and
/// `train_duration_secs`, which nothing can derive.
pub struct RoundProgress {
    pub train_cursors: Vec<u64>,
    /// The true row-weighted denominator for `loss_sum / rows_done`. No `steps_done` field here (unlike
    /// an earlier version of this struct): how many steps into the current round a resumed run is at
    /// is always `optimizer.step_count() % steps_per_eval`, fully derivable from `optimizer.opt`'s own
    /// persisted step count, so storing it here separately would just be a second copy that a changed
    /// `steps_per_eval` between runs could silently drift out of sync with (found the hard way: a
    /// network's `net.yaml` had its `steps_per_eval` lowered between two runs of the same job, and the
    /// stale, larger persisted count was still past the new, smaller round size, completing a round
    /// with zero further training).
    pub rows_done: u64,
    pub loss_sum: f64,
    /// This round's own accumulated training wall-clock time so far, in seconds -- across however many
    /// separate `Ctrl+C`-interrupted-and-resumed process invocations it took to get this far, never
    /// counting the idle time between an interruption and a later resume. Named `train_` (not just
    /// `duration_secs`) to match `history.rs`'s own `EvalRecord` fields: test-pass time is never
    /// included here at all, since the test pass only ever runs once, after the round's training half
    /// has already fully completed (`round::run_round`'s own doc comment covers the split).
    pub train_duration_secs: f64,
}

const ROUND_PROGRESS_MAGIC: [u8; 4] = *b"CHTR";
const ROUND_PROGRESS_FORMAT_VERSION: u32 = 1;

impl RoundProgress {
    pub fn save(&self, path: &str) {
        let mut writer =
            BufWriter::new(StdFile::create(path).unwrap_or_else(|error| panic!("failed to create {path}: {error}")));
        writer.write_all(&ROUND_PROGRESS_MAGIC).expect("failed to write the round-progress magic bytes");
        writer
            .write_all(&ROUND_PROGRESS_FORMAT_VERSION.to_le_bytes())
            .expect("failed to write the format version");
        writer
            .write_all(&(self.train_cursors.len() as u64).to_le_bytes())
            .expect("failed to write the train cursor count");
        for cursor in &self.train_cursors {
            writer.write_all(&cursor.to_le_bytes()).expect("failed to write a train cursor");
        }
        writer.write_all(&self.rows_done.to_le_bytes()).expect("failed to write rows_done");
        writer.write_all(&self.loss_sum.to_le_bytes()).expect("failed to write loss_sum");
        writer.write_all(&self.train_duration_secs.to_le_bytes()).expect("failed to write train_duration_secs");
    }

    pub fn load(path: &str) -> RoundProgress {
        let mut reader = BufReader::new(StdFile::open(path).unwrap_or_else(|error| panic!("failed to open {path}: {error}")));

        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic).expect("failed to read the round-progress magic bytes");
        assert_eq!(magic, ROUND_PROGRESS_MAGIC, "not a smithy round-progress file (bad magic bytes)");

        let mut version_bytes = [0u8; 4];
        reader.read_exact(&mut version_bytes).expect("failed to read the format version");
        let version = u32::from_le_bytes(version_bytes);
        assert_eq!(
            version, ROUND_PROGRESS_FORMAT_VERSION,
            "round-progress format version mismatch: file has {version}, this build expects \
             {ROUND_PROGRESS_FORMAT_VERSION}"
        );

        let cursor_count = read_u64(&mut reader);
        let mut train_cursors = Vec::with_capacity(cursor_count as usize);
        for _ in 0..cursor_count {
            train_cursors.push(read_u64(&mut reader));
        }

        let mut rows_done_bytes = [0u8; 8];
        reader.read_exact(&mut rows_done_bytes).expect("failed to read rows_done");
        let rows_done = u64::from_le_bytes(rows_done_bytes);

        let mut loss_sum_bytes = [0u8; 8];
        reader.read_exact(&mut loss_sum_bytes).expect("failed to read loss_sum");
        let loss_sum = f64::from_le_bytes(loss_sum_bytes);

        let mut train_duration_secs_bytes = [0u8; 8];
        reader.read_exact(&mut train_duration_secs_bytes).expect("failed to read train_duration_secs");
        let train_duration_secs = f64::from_le_bytes(train_duration_secs_bytes);

        RoundProgress { train_cursors, rows_done, loss_sum, train_duration_secs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use temper_chess::labels::{score, Curve, NETWORK};

    #[test]
    fn round_progress_round_trips_every_field() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-game-labels-round-progress-test-{pid}.state"));
        let path = path.to_str().unwrap();

        // Distinct per index, so a mixed-up order would be caught.
        let train_cursors: Vec<u64> = (0..31).map(|i| i * 7).collect();
        let progress = RoundProgress { train_cursors: train_cursors.clone(), rows_done: 4200, loss_sum: 12.5, train_duration_secs: 3.75 };
        progress.save(path);

        let restored = RoundProgress::load(path);
        assert_eq!(restored.train_cursors, train_cursors);
        assert_eq!(restored.rows_done, 4200);
        assert_eq!(restored.loss_sum, 12.5);
        assert_eq!(restored.train_duration_secs, 3.75);

        std::fs::remove_file(path).ok();
    }

    #[test]
    #[should_panic(expected = "bad magic bytes")]
    fn round_progress_rejects_a_file_that_is_not_one() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-game-labels-round-progress-test-{pid}-bad-magic.state"));
        std::fs::write(&path, b"not a round progress file at all").unwrap();

        let _ = RoundProgress::load(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
    }

    /// Bare kings throughout (a king move never changes piece count), White to move first, from a
    /// `[FEN]` tag rather than the standard start.
    const BARE_KINGS_GAME: &str =
        "[Result \"1-0\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Kd2 {1.50} Kd7 {-1.20}\n1-0\n";

    #[test]
    fn a_score_comment_round_trips_back_to_its_own_mover() {
        // Exercises both halves of the frame conversion. At `lambda = 0` the result contributes
        // nothing, so the two conversions compose into the identity: a comment is written in the
        // mover's own frame, turned White-relative for the blend, and turned back for the mover it
        // describes, landing on exactly `score(comment)` for both plies -- the negative one included.
        let game = replayed_game(BARE_KINGS_GAME);
        assert_eq!(self_play_game_positions(&game, 0).len(), 3, "before Kd2, before Kd7, and the final one");

        let labels = self_play_game_labels(&game, 0.0, 0, &Curve::Atan(*NETWORK));
        assert_eq!(labels.len(), 3);
        assert_eq!(labels[2], 1.0, "the final position, White to move after 1-0, is labelled with the result");
        assert!((labels[0] - score(1.50) as f32).abs() < 1e-6, "White's own +1.50 stays +1.50-worth for White");
        assert!(
            (labels[1] - score(-1.20) as f32).abs() < 1e-6,
            "Black's own -1.20 must come back as -1.20-worth for Black, not as White-relative +1.20: the features \
             beside it describe the position from Black's side"
        );
    }

    #[test]
    fn one_outcome_reaches_alternating_plies_with_opposite_signs() {
        // The property whose absence cost three epochs of training: every feature the networks read is
        // side-to-move relative, so one game result has to reach a White-to-move ply and a Black-to-move
        // ply with *opposite* signs. `lambda = 1.0` isolates it -- every label is the outcome alone,
        // with no score blended in -- over a bare-kings game where nothing about the position changes
        // except whose move it is. Before `labels::side_to_move_relative`, both labels had the same sign.
        let labels = self_play_game_labels(&replayed_game(BARE_KINGS_GAME), 1.0, 0, &Curve::Atan(*NETWORK));
        assert_eq!(labels.len(), 3);
        assert!((labels[0] - 1.0).abs() < 1e-6, "White is to move and White won: {}", labels[0]);
        assert!((labels[1] + 1.0).abs() < 1e-6, "Black is to move and Black lost: {}", labels[1]);
        assert!((labels[2] - 1.0).abs() < 1e-6, "the final position, White to move: {}", labels[2]);
    }

    fn write_three_game_self_play_fixture(pgn_path: &std::path::Path) {
        let pgn_text = "[Result \"1-0\"]\n\n1. e4 {0.20} e5 {-0.15} 1-0\n\n\
             [Result \"0-1\"]\n\n1. d4 {0.10} d5 {-0.05} 0-1\n\n\
             [Result \"1/2-1/2\"]\n\n1. c4 {0.05} c5 {-0.02} 1/2-1/2\n";
        std::fs::write(pgn_path, zstd::encode_all(pgn_text.as_bytes(), 3).unwrap()).expect("failed to write the fixture PGN");
    }

    #[test]
    fn self_play_segment_reader_reads_games_in_order() {
        let pid = std::process::id();
        let pgn_path = std::env::temp_dir().join(format!("smithy-game-labels-segment-reader-test-{pid}.pgn.zst"));
        write_three_game_self_play_fixture(&pgn_path);
        let pgn_path_str = pgn_path.to_str().unwrap();

        let mut reader = SelfPlaySegmentReader::open(pgn_path_str);

        let (index_0, game_0) = reader.next_game(pgn_path_str).expect("game 0 should be present");
        assert_eq!(index_0, 0);
        assert_eq!(game_0.outcome, 1.0);

        let (index_1, game_1) = reader.next_game(pgn_path_str).expect("game 1 should be present");
        assert_eq!(index_1, 1);
        assert_eq!(game_1.outcome, 0.0);

        let (index_2, game_2) = reader.next_game(pgn_path_str).expect("game 2 should be present");
        assert_eq!(index_2, 2);
        assert_eq!(game_2.outcome, 0.5);

        assert!(reader.next_game(pgn_path_str).is_none(), "a 4th game should not exist in this fixture");

        std::fs::remove_file(&pgn_path).ok();
    }

    /// A wait of 50 ms, so the tests' files come back within a few retries.
    fn quick_wait(stop: Option<Arc<AtomicBool>>) -> ReadWait {
        ReadWait { stop, pause: Duration::from_millis(50) }
    }

    /// The fixture at `name`, and where it is moved away to make it unreadable.
    fn unreadable_fixture(name: &str) -> (String, String) {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-read-wait-{name}-{pid}.pgn.zst"));
        write_three_game_self_play_fixture(&path);
        let path = path.to_str().unwrap().to_string();
        (path.clone(), format!("{path}.away"))
    }

    /// Moves `away` back to `path` after 300 ms, as Drive making a file readable again.
    fn back_later(away: &str, path: &str) -> std::thread::JoinHandle<()> {
        let (away, path) = (away.to_string(), path.to_string());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            std::fs::rename(&away, &path).unwrap();
        })
    }

    #[test]
    fn a_segment_that_cannot_be_opened_is_waited_for() {
        let (path, away) = unreadable_fixture("open");
        std::fs::rename(&path, &away).unwrap();
        let back = back_later(&away, &path);
        let mut reader = SelfPlaySegmentReader::open_waiting(&path, quick_wait(None)).unwrap();
        back.join().unwrap();
        let outcomes: Vec<f64> =
            std::iter::from_fn(|| reader.next_game_waiting(&path).unwrap()).map(|(_, game)| game.outcome).collect();
        assert_eq!(outcomes, [1.0, 0.0, 0.5]);
        std::fs::remove_file(&path).ok();
    }

    /// A read failing midway: the segment reopened, the games already delivered skipped, the next
    /// one delivered with its own index, as an undisturbed read would.
    #[test]
    fn a_reopened_segment_resumes_after_the_games_already_read() {
        let (path, away) = unreadable_fixture("reopen");
        let mut reader = SelfPlaySegmentReader::open_waiting(&path, quick_wait(None)).unwrap();
        assert_eq!(reader.next_game_waiting(&path).unwrap().map(|(index, _)| index), Some(0));
        std::fs::rename(&path, &away).unwrap();
        let back = back_later(&away, &path);
        reader.reopen(&path, &mut None).unwrap();
        back.join().unwrap();
        let rest: Vec<(u64, f64)> =
            std::iter::from_fn(|| reader.next_game_waiting(&path).unwrap()).map(|(index, game)| (index, game.outcome)).collect();
        assert_eq!(rest, [(1, 0.0), (2, 0.5)]);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_stop_ends_the_wait() {
        let (path, away) = unreadable_fixture("stop");
        std::fs::rename(&path, &away).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopper = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                stop.store(true, Ordering::SeqCst);
            })
        };
        assert!(matches!(SelfPlaySegmentReader::open_waiting(&path, quick_wait(Some(stop))), Err(Stopped)));
        stopper.join().unwrap();
        std::fs::remove_file(&away).ok();
    }

    #[test]
    fn mate_notation_comments_are_labelled_as_decisive() {
        // Ply 0 (White's own Kd2) carries a real `#3` mate comment, which has to parse rather than
        // panic. Ply 1 (Black's own Kd7) carries an ordinary decimal comment, confirming both comment
        // shapes coexist fine within the same game.
        let game = replayed_game(
            "[Result \"1-0\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Kd2 {#3} Kd7 {-1.20}\n1-0\n",
        );
        let labels = self_play_game_labels(&game, 0.0, 0, &Curve::Atan(*NETWORK));
        assert_eq!(labels.len(), 3, "two searched positions and the final one");
        // White's own `#3` reconstructs to `MATE_SCORE` pawns, which is decisive regardless of
        // `lambda` -- a hard `1.0` by sign.
        assert!((labels[0] - 1.0).abs() < 1e-6, "White's own #3 should read as a win for White, exactly 1.0");
        // lambda = 0.0, so this non-decisive label is exactly `score(pawns)`, and Black is to move, so
        // the two conversions compose to the identity.
        assert!((labels[1] - score(-1.20) as f32).abs() < 1e-6, "Black's own -1.20 should come back to Black as -1.20");
    }

    /// A scored game parsed from `pgn`, as `arena replay` and self-play write them: the book
    /// before `{ Book exit }` unscored and not emitted, a score after every later move.
    fn replayed_game(pgn: &str) -> CollectedSelfPlayGame {
        let mut reader = pgn_reader::Reader::new(std::io::Cursor::new(pgn.as_bytes().to_vec()));
        reader.read_game(&mut SelfPlayGameCollector::new()).unwrap().expect("the fixture holds a game")
    }

    /// KQ v K: every position labelled by the tables from its own side to move, the winner 1 and the
    /// bare king -1, whatever the game's scores say; the position before the book's end not at all.
    /// Skipped where `bin/syzygy.sh` has not run.
    #[test]
    fn positions_the_tables_know_are_labelled_by_them() {
        let directories = temper_chess::pipeline::data_root::syzygy_directories();
        let Ok(tablebase) = SyzygyTablebase::from_directories(&directories) else {
            println!("skipped: no Syzygy WDL and DTZ tables");
            return;
        };
        let game = replayed_game(concat!(
            "[Result \"1/2-1/2\"]\n[FEN \"k7/7Q/1K6/8/8/8/8/8 w - - 0 1\"]\n[SetUp \"1\"]\n\n",
            "1. Kc6 { Book exit } 1... Kb8 {0.10} 2. Kd6 {0.00} 1/2-1/2\n\n"
        ));
        assert_eq!(self_play_game_tablebase(&game, &tablebase, 0), vec![Some(-1.0), Some(1.0), Some(-1.0)]);
        // Middlegame positions are never probed.
        let game = replayed_game("[Result \"1/2-1/2\"]\n\n1. e4 {0.20} 1... e5 {-0.10} 1/2-1/2\n\n");
        assert_eq!(self_play_game_tablebase(&game, &tablebase, 0), vec![None, None, None]);
    }

    #[test]
    fn a_replayed_game_trains_on_every_position_after_its_book_and_its_final_one() {
        let game = replayed_game("[Result \"1/2-1/2\"]\n\n1. e4 e5 { Book exit } 2. Nf3 {0.20} 2... Nc6 {-0.10} 1/2-1/2\n\n");
        assert_eq!(game.book, 2);
        assert_eq!(game.position_count(), 3, "before Nf3, before Nc6, and the final position");

        let positions = self_play_game_positions(&game, 0);
        assert_eq!(positions.len(), 3);
        assert_eq!(positions[0].side_to_move, Color::White, "the first emitted position is after 1. e4 e5");
        assert_eq!(positions[2].side_to_move, Color::White, "the final position, after 2... Nc6");

        let labels = self_play_game_labels(&game, 0.0, 0, &Curve::Atan(*NETWORK));
        assert!((labels[0] - score(0.20) as f32).abs() < 1e-6);
        assert!((labels[1] - score(-0.10) as f32).abs() < 1e-6, "Black's own -0.10, for Black");
        assert_eq!(labels[2], 0.0, "a draw labels the final position 0");
        // Read through a teacher's scale, the same comment says what its games made of it.
        let teacher = temper_chess::labels::Scale::with_one_pawn(0.62, 128.0);
        let calibrated = self_play_game_labels(&game, 0.0, 0, &Curve::Atan(teacher));
        assert!((calibrated[0] - teacher.score(0.20) as f32).abs() < 1e-6);
        assert!(calibrated[0] > labels[0], "a teacher whose pawn is worth more reads its +0.20 higher");
    }

    #[test]
    fn a_score_may_carry_the_best_move_of_its_position() {
        let game = replayed_game(
            "[Result \"1/2-1/2\"]\n\n1. e4 e5 { Book exit } 2. Nf3 {0.20 g1f3} 2... Nc6 {#-2 d8h4} 3. Bb5 {-0.10} 1/2-1/2\n\n",
        );
        assert_eq!(game.best_moves, vec![Some("g1f3".to_string()), Some("d8h4".to_string()), None], "a score alone has none");
        assert_eq!(self_play_game_positions(&game, 0).len(), 4, "every best move is legal in its position");
        let labels = self_play_game_labels(&game, 0.0, 0, &Curve::Atan(*NETWORK));
        assert!((labels[0] - score(0.20) as f32).abs() < 1e-6, "the score reads as before");
        assert_eq!(labels[1], -1.0, "a mate with a move is still decisive");
    }

    #[test]
    #[should_panic(expected = "is not legal")]
    fn an_illegal_best_move_fails() {
        let game = replayed_game("[Result \"1-0\"]\n\n1. e4 {0.20 e2e5} 1-0\n\n");
        self_play_game_positions(&game, 0);
    }

    #[test]
    #[should_panic(expected = "not whole counts")]
    fn a_third_field_that_is_not_counts_fails() {
        replayed_game("[Result \"1-0\"]\n\n1. e4 {0.20 e2e4 extra} 1-0\n\n");
    }

    #[test]
    #[should_panic(expected = "more than a score, a best move and its counts")]
    fn a_comment_with_more_than_counts_fails() {
        replayed_game("[Result \"1-0\"]\n\n1. e4 {0.20 e2e4 1,2 extra} 1-0\n\n");
    }

    /// Two bare kings, White's on e1 with its five moves: d1, d2, e2, f1, f2.
    const KINGS: &str = "[Result \"1/2-1/2\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n";

    #[test]
    fn visit_counts_are_kept_and_checked_against_the_legal_moves() {
        let game = replayed_game(&format!("{KINGS}1. Kd2 {{0.00 e1d2 1,9,3,0,2}} Kd7 {{0.00 e8d7}} 1/2-1/2\n\n"));
        assert_eq!(game.counts, vec![Some(vec![1, 9, 3, 0, 2]), None], "an older comment, without counts, still reads");
        assert_eq!(self_play_game_positions(&game, 0).len(), 3, "five counts for five legal moves");
    }

    /// Whether a segment of Lc0's corpus with visit counts holds the same games as the old one without:
    /// by hand, before the forges move to the new corpus, `LC0_OLD_SEGMENT` and `LC0_NEW_SEGMENT`
    /// naming the same tar's first segment in each. Same start, moves, book, scores, best moves and
    /// outcome, so the same positions and labels at every lambda; and every count checked against its
    /// position's legal moves.
    #[test]
    #[ignore = "reads two corpora's segments, named by LC0_OLD_SEGMENT and LC0_NEW_SEGMENT"]
    fn a_segment_with_counts_holds_the_old_segment_s_games() {
        let path = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} names a segment"));
        let (old_path, new_path) = (path("LC0_OLD_SEGMENT"), path("LC0_NEW_SEGMENT"));
        let mut old = SelfPlaySegmentReader::open(&old_path);
        let mut new = SelfPlaySegmentReader::open(&new_path);
        let (mut games, mut counted) = (0u64, 0usize);
        loop {
            match (old.next_game(&old_path), new.next_game(&new_path)) {
                (None, None) => break,
                (Some((index, before)), Some((_, after))) => {
                    let moves = |game: &CollectedSelfPlayGame| game.moves.iter().map(|san| san.to_string()).collect::<Vec<_>>();
                    assert_eq!(before.starting_fen, after.starting_fen, "game {index}: start");
                    assert_eq!(moves(&before), moves(&after), "game {index}: moves");
                    assert_eq!((before.book, before.outcome), (after.book, after.outcome), "game {index}: book and outcome");
                    assert_eq!(before.scores_pawns, after.scores_pawns, "game {index}: scores");
                    assert_eq!(before.best_moves, after.best_moves, "game {index}: best moves");
                    assert!(after.counts.iter().all(Option::is_some), "game {index}: a comment without counts");
                    self_play_game_kept(&after, index);
                    counted += after.counts.len();
                    games += 1;
                }
                _ => panic!("the segments hold different numbers of games, {games} alike before"),
            }
        }
        println!("{games} games alike, {counted} positions' visit counts checked against their legal moves");
    }

    #[test]
    #[should_panic(expected = "4 visit counts for 5 legal moves")]
    fn visit_counts_not_one_per_legal_move_fail() {
        let game = replayed_game(&format!("{KINGS}1. Kd2 {{0.00 e1d2 1,9,3,0}} 1/2-1/2\n\n"));
        self_play_game_positions(&game, 0);
    }

    #[test]
    fn the_searched_positions_are_all_but_the_final_one() {
        let game = replayed_game("[Result \"1/2-1/2\"]\n\n1. e4 e5 { Book exit } 2. Nf3 {0.20} 2... Nc6 {-0.10} 1/2-1/2\n\n");
        let (positions, kept) = self_play_game_kept(&game, 0);
        assert_eq!(positions.len(), kept.len(), "a flag beside every position");
        assert_eq!(kept, vec![true, true, false]);
    }

    #[test]
    fn a_game_that_ends_at_its_book_exit_trains_on_its_final_position_alone() {
        let game = replayed_game("[Result \"0-1\"]\n\n1. f3 e5 2. g4 Qh4# { Book exit } 0-1\n\n");
        assert_eq!(game.position_count(), 1);
        assert_eq!(self_play_game_positions(&game, 0).len(), 1);
        assert_eq!(self_play_game_labels(&game, 0.7, 0, &Curve::Atan(*NETWORK)), vec![-1.0], "White is mated: -1 for the side to move");
    }
}
