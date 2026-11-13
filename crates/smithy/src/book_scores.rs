//! A book's scores: every move of an opening book judged by Fattorini's own search, so the book can
//! weigh its moves by what Fattorini thinks of them rather than by human results alone. The search
//! runs on the arenas (the `score` chores the smithy's score job deals); this module holds what does
//! not search:
//!
//! - **The scores file**, `data/books/<source>/scores-<network>-<tuned|untuned>-d<depth>.txt`, beside
//!   the source's graph: a header naming everything the scores depend on (the graph, the network and
//!   its parameters in full, the build, the depth, the search's table), then one `row` per book move
//!   as its record arrives: parent key, Polyglot move, child key, score from the side that played
//!   the move, depth, nodes. Rows are appended, so a stopped job resumes with the moves still
//!   without one, and a different header is refused rather than mixed in: a new network or depth is
//!   a new file.
//! - **The softmax weights** of a book written from it: every move weighs `e^(score / T)` against its
//!   position's best, scaled so the best gets 65,535. Weighing by the score's distance from the best,
//!   not by the score, keeps the numbers in range; the ratios are the same.
//! - **The first-gate report** beside that book: how many of the book's moves Fattorini finds bad,
//!   and how often the human book, weighted by points, would have played one where lichess-bot reads
//!   the book. It decides whether a search-judged book is worth having at all.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::Write;

use fattorini::pipeline::protocol::BookScore;
use fattorini::polyglot::Entry;

/// A mate as centipawns, for the weights and the report: far beyond any evaluation, a nearer mate
/// a little better, so mating moves take a position's whole weight and mated ones none.
const MATE_CP: i32 = 10_000;

/// How far below its position's best a move must score for the report to call it bad.
pub(crate) const BAD_MARGIN_CP: i32 = 50;

/// The last ply lichess-bot reads the book at: its `max_depth` of 12 moves, read while the ply is at
/// most `2 * 12 - 1`. A move is within reach when the plies before it are at most this.
pub(crate) const LICHESS_REACH_PLY: u32 = 23;

/// The positions the report lists, the worst by the human book's chance of a bad move.
const WORST_LISTED: usize = 20;

/// The largest Polyglot weight: the best move of each position gets it.
const TOP_WEIGHT: f64 = 65_535.0;

/// The name of a scores file, from the network's player name, whether it plays tuned, and the depth.
pub(crate) fn file_name(player: &str, tuned: bool, depth: u32) -> String {
    format!("scores-{player}-{}-d{depth}.txt", if tuned { "tuned" } else { "untuned" })
}

/// Whether `name` is a scores file's: `scores-...-d<depth>.txt`.
pub(crate) fn is_scores_file(name: &str) -> bool {
    name.strip_prefix("scores-")
        .and_then(|rest| rest.strip_suffix(".txt"))
        .and_then(|stem| stem.rsplit_once("-d"))
        .is_some_and(|(_, depth)| !depth.is_empty() && depth.bytes().all(|byte| byte.is_ascii_digit()))
}

/// A scores file's stem, its name without `scores-` and `.txt`, which names the books written from it.
pub(crate) fn stem(name: &str) -> &str {
    name.strip_prefix("scores-").and_then(|rest| rest.strip_suffix(".txt")).unwrap_or(name)
}

/// One book move's score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Row {
    pub parent: u64,
    pub book_move: u16,
    pub child: u64,
    pub score: BookScore,
    pub depth: u32,
    pub nodes: u64,
}

impl Row {
    pub(crate) fn text(&self) -> String {
        format!(
            "row\t{:016x}\t{}\t{:016x}\t{}\t{}\t{}\n",
            self.parent,
            self.book_move,
            self.child,
            self.score.text(),
            self.depth,
            self.nodes
        )
    }
}

/// A scores file read back: its header, in order, and its rows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Scores {
    pub header: Vec<(String, String)>,
    pub rows: Vec<Row>,
}

/// The header as the file starts with it, one `name<TAB>value` line each.
pub(crate) fn header_text(header: &[(String, String)]) -> String {
    header.iter().map(|(name, value)| format!("{name}\t{value}\n")).collect()
}

/// A scores file's text read back. A last line cut short (no line end: the smithy stopped while
/// writing it) is dropped, as never written; any other malformed line is an error.
pub(crate) fn parse(text: &str) -> Result<Scores, String> {
    let whole = match text.rfind('\n') {
        Some(end) => &text[..=end],
        None => "",
    };
    let key = |field: &str| u64::from_str_radix(field, 16).map_err(|_| format!("not a key: {field:?}"));
    let mut scores = Scores { header: Vec::new(), rows: Vec::new() };
    for (index, line) in whole.lines().enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        let bad = || format!("scores line {}: {line:?}", index + 1);
        match fields.as_slice() {
            ["row", parent, book_move, child, score, depth, nodes] => scores.rows.push(Row {
                parent: key(parent)?,
                book_move: book_move.parse().map_err(|_| bad())?,
                child: key(child)?,
                score: BookScore::parse(score).map_err(|message| format!("{}: {message}", bad()))?,
                depth: depth.parse().map_err(|_| bad())?,
                nodes: nodes.parse().map_err(|_| bad())?,
            }),
            ["row", ..] => return Err(bad()),
            [name, value] if scores.rows.is_empty() => scores.header.push((name.to_string(), value.to_string())),
            _ => return Err(bad()),
        }
    }
    Ok(scores)
}

/// The scores file at `path`, appended to as rows arrive: opened with `header`, written when the
/// file is new, checked against the file's own when it is not.
pub(crate) struct ScoresFile {
    pub path: String,
    /// The moves scored, by parent and move.
    pub scored: HashSet<(u64, u16)>,
}

impl ScoresFile {
    /// Opens `path` for `header`: a new file starts with it; an existing one must have the same
    /// header, every line equal, else it holds another scoring and is refused. A last line cut short
    /// is cut off the file, so the next row starts on a line of its own.
    pub(crate) fn open(path: &str, header: &[(String, String)]) -> Result<ScoresFile, String> {
        let existing = match std::fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("{path}: {error}")),
        };
        let Some(text) = existing else {
            if let Some(parent) = std::path::Path::new(path).parent().filter(|parent| !parent.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
            }
            std::fs::write(path, header_text(header)).map_err(|error| format!("{path}: {error}"))?;
            return Ok(ScoresFile { path: path.to_string(), scored: HashSet::new() });
        };
        let scores = parse(&text).map_err(|message| format!("{path}: {message}"))?;
        if scores.header != header {
            let differing = header
                .iter()
                .zip(&scores.header)
                .find(|(wanted, held)| wanted != held)
                .map_or("the number of header lines".to_string(), |(wanted, held)| format!("{} {:?} against {:?}", wanted.0, held.1, wanted.1));
            return Err(format!("{path} holds another scoring ({differing}): move it aside, or score with what it names"));
        }
        if !text.ends_with('\n') {
            let whole = text.rfind('\n').map_or(0, |end| end + 1);
            std::fs::write(path, &text[..whole]).map_err(|error| format!("{path}: {error}"))?;
        }
        Ok(ScoresFile { path: path.to_string(), scored: scores.rows.iter().map(|row| (row.parent, row.book_move)).collect() })
    }

    /// Appends `row`, unless its move is scored already.
    pub(crate) fn append(&mut self, row: &Row) -> Result<(), String> {
        if !self.scored.insert((row.parent, row.book_move)) {
            return Ok(());
        }
        let mut file = std::fs::OpenOptions::new().append(true).open(&self.path).map_err(|error| format!("{}: {error}", self.path))?;
        file.write_all(row.text().as_bytes()).map_err(|error| format!("{}: {error}", self.path))
    }
}

/// A score as centipawns from the mover's side: a mate `MATE_CP` less its distance, so nearer is
/// better, a mated move as far below.
pub(crate) fn mover_cp(score: BookScore) -> i32 {
    match score {
        BookScore::Cp(cp) => cp,
        BookScore::Mate(moves) if moves > 0 => MATE_CP - moves,
        BookScore::Mate(moves) => -MATE_CP - moves,
    }
}

/// One book move as the weights and the report need it: its position, its move, its score, and the
/// points it scored for humans (2 a win, 1 a draw), the old book's weight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Judged {
    pub parent: u64,
    pub book_move: u16,
    pub cp: i32,
    pub points: u64,
}

/// The book's moves by position, each position's in move order.
fn by_position(moves: &[Judged]) -> BTreeMap<u64, Vec<Judged>> {
    let mut positions: BTreeMap<u64, Vec<Judged>> = BTreeMap::new();
    for judged in moves {
        positions.entry(judged.parent).or_default().push(*judged);
    }
    for list in positions.values_mut() {
        list.sort_by_key(|judged| judged.book_move);
    }
    positions
}

/// What `softmax_entries` builds.
#[derive(Debug, PartialEq)]
pub(crate) struct SoftBook {
    pub entries: Vec<Entry>,
    pub positions: usize,
    /// Moves whose weight rounds to 0, left out: a weight is never written as 0, which readers take
    /// as "never play".
    pub zeros: usize,
}

/// The softmax book of `moves` at `temperature` centipawns: per position, each move weighs
/// `65,535 * e^((cp - best) / T)`, rounded, the best 65,535; a weight rounding to 0 is left out.
/// Sorted as Polyglot requires: by key, heaviest first, then by move.
pub(crate) fn softmax_entries(moves: &[Judged], temperature: f64) -> SoftBook {
    let mut entries = Vec::new();
    let mut zeros = 0;
    let positions = by_position(moves);
    for (&key, list) in &positions {
        let best = list.iter().map(|judged| judged.cp).max().expect("a position has a move");
        for judged in list {
            let weight = (TOP_WEIGHT * (f64::from(judged.cp - best) / temperature).exp()).round();
            if weight < 1.0 {
                zeros += 1;
                continue;
            }
            entries.push(Entry { key, mv: judged.book_move, weight: weight as u16 });
        }
    }
    entries.sort_by(|a, b| a.key.cmp(&b.key).then(b.weight.cmp(&a.weight)).then(a.mv.cmp(&b.mv)));
    SoftBook { entries, positions: positions.len(), zeros }
}

/// The first-gate report: the book's moves `BAD_MARGIN_CP` or more below their position's best, in
/// all and within lichess-bot's reach (`plies`: each position's fewest plies from the start), and
/// the chance the human book, weighted by points, plays one in a position within reach, averaged over
/// those positions, with the worst of them.
#[derive(Debug, PartialEq)]
pub(crate) struct Report {
    pub moves: usize,
    pub bad: usize,
    pub moves_in_reach: usize,
    pub bad_in_reach: usize,
    /// Positions within reach where the human book plays at all (some move with points).
    pub positions_in_reach: usize,
    pub old_chance_in_reach: f64,
    /// The worst positions within reach: the human book's chance, the ply, the parent's key, and the
    /// bad moves with their distance below the best.
    pub worst: Vec<Worst>,
}

/// One of the report's worst positions: the human book's chance of a bad move, the ply, the key, and
/// each bad move with how far below the best it scores.
pub(crate) type Worst = (f64, u32, u64, Vec<(u16, i32)>);

pub(crate) fn report(moves: &[Judged], plies: &HashMap<u64, u32>) -> Report {
    let mut report = Report {
        moves: moves.len(),
        bad: 0,
        moves_in_reach: 0,
        bad_in_reach: 0,
        positions_in_reach: 0,
        old_chance_in_reach: 0.0,
        worst: Vec::new(),
    };
    let mut chances = Vec::new();
    for (&key, list) in &by_position(moves) {
        let best = list.iter().map(|judged| judged.cp).max().expect("a position has a move");
        let bad: Vec<&Judged> = list.iter().filter(|judged| best - judged.cp >= BAD_MARGIN_CP).collect();
        report.bad += bad.len();
        let Some(&ply) = plies.get(&key).filter(|&&ply| ply <= LICHESS_REACH_PLY) else {
            continue;
        };
        report.moves_in_reach += list.len();
        report.bad_in_reach += bad.len();
        let points: u64 = list.iter().map(|judged| judged.points).sum();
        if points == 0 {
            continue;
        }
        let chance = bad.iter().map(|judged| judged.points).sum::<u64>() as f64 / points as f64;
        chances.push(chance);
        if chance > 0.0 {
            let listed = bad.iter().map(|judged| (judged.book_move, best - judged.cp)).collect();
            report.worst.push((chance, ply, key, listed));
        }
    }
    report.positions_in_reach = chances.len();
    report.old_chance_in_reach = if chances.is_empty() { 0.0 } else { chances.iter().sum::<f64>() / chances.len() as f64 };
    report.worst.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    report.worst.truncate(WORST_LISTED);
    report
}

/// Each position's fewest plies from `root`, over `edges` (parent, child).
pub(crate) fn plies_from(root: u64, edges: &[(u64, u64)]) -> HashMap<u64, u32> {
    let mut children: HashMap<u64, Vec<u64>> = HashMap::new();
    for &(parent, child) in edges {
        children.entry(parent).or_default().push(child);
    }
    let mut plies = HashMap::from([(root, 0)]);
    let mut queue = VecDeque::from([root]);
    while let Some(node) = queue.pop_front() {
        let ply = plies[&node];
        for &child in children.get(&node).map(Vec::as_slice).unwrap_or(&[]) {
            if let std::collections::hash_map::Entry::Vacant(entry) = plies.entry(child) {
                entry.insert(ply + 1);
                queue.push_back(child);
            }
        }
    }
    plies
}

/// A Polyglot move as from and to squares, `e2e4`, a promotion's piece after (`e7e8q`); castling is
/// Polyglot's king-takes-rook, `e1h1`.
pub(crate) fn move_text(book_move: u16) -> String {
    let square = |index: u16| format!("{}{}", (b'a' + (index % 8) as u8) as char, index / 8 + 1);
    let promotion = match (book_move >> 12) & 7 {
        1 => "n",
        2 => "b",
        3 => "r",
        4 => "q",
        _ => "",
    };
    format!("{}{}{promotion}", square((book_move >> 6) & 63), square(book_move & 63))
}

/// The report as the softmax book's record lines, `fens` naming each worst position.
pub(crate) fn report_lines(report: &Report, fens: &HashMap<u64, String>) -> Vec<(String, String)> {
    let share = |part: usize, whole: usize| if whole == 0 { 0.0 } else { 100.0 * part as f64 / whole as f64 };
    let mut lines = vec![
        ("report_bad_margin_cp".to_string(), BAD_MARGIN_CP.to_string()),
        ("report_reach_ply".to_string(), LICHESS_REACH_PLY.to_string()),
        ("report_bad_moves".to_string(), format!("{} of {} ({:.2}%)", report.bad, report.moves, share(report.bad, report.moves))),
        (
            "report_bad_moves_in_reach".to_string(),
            format!("{} of {} ({:.2}%)", report.bad_in_reach, report.moves_in_reach, share(report.bad_in_reach, report.moves_in_reach)),
        ),
        (
            "report_old_book_chance_in_reach".to_string(),
            format!("{:.2}% over {} positions", 100.0 * report.old_chance_in_reach, report.positions_in_reach),
        ),
    ];
    for (chance, ply, key, bad) in &report.worst {
        let moves: Vec<String> = bad.iter().map(|&(book_move, below)| format!("{} -{below}", move_text(book_move))).collect();
        let fen = fens.get(key).map_or("?", String::as_str);
        lines.push(("report_worst".to_string(), format!("{:.1}%\tply {ply}\t{fen}\t{}", 100.0 * chance, moves.join(", "))));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> String {
        std::env::temp_dir().join(format!("fattorini-book-scores-{name}-{}.txt", std::process::id())).display().to_string()
    }

    fn header(depth: &str) -> Vec<(String, String)> {
        vec![("source".to_string(), "elite".to_string()), ("depth".to_string(), depth.to_string())]
    }

    fn row(parent: u64, book_move: u16, cp: i32) -> Row {
        Row { parent, book_move, child: parent + 100, score: BookScore::Cp(cp), depth: 20, nodes: 1234 }
    }

    #[test]
    fn a_scores_file_is_named_after_its_scoring() {
        let name = file_name("lc0-psq-threats-screlu-g1-r1", true, 20);
        assert_eq!(name, "scores-lc0-psq-threats-screlu-g1-r1-tuned-d20.txt");
        assert!(is_scores_file(&name));
        assert_eq!(stem(&name), "lc0-psq-threats-screlu-g1-r1-tuned-d20");
        for other in ["graph.txt", "scores-x-d.txt", "scores-x-dz.txt", "book-n9-points.txt"] {
            assert!(!is_scores_file(other), "{other}");
        }
    }

    #[test]
    fn a_scores_file_reads_back_resumes_and_refuses_another_scoring() {
        let path = path("resume");
        std::fs::remove_file(&path).ok();
        let mut file = ScoresFile::open(&path, &header("20")).unwrap();
        file.append(&row(1, 10, 23)).unwrap();
        file.append(&row(1, 10, 99)).unwrap();
        let mut mate = row(2, 20, 0);
        mate.score = BookScore::Mate(-3);
        file.append(&mate).unwrap();
        // A row cut short by a stop: dropped on reading, cut off on opening.
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"row\t0000").unwrap();
        let scores = parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(scores.header, header("20"));
        assert_eq!(scores.rows, vec![row(1, 10, 23), mate], "a move once only, the first score kept");
        let mut resumed = ScoresFile::open(&path, &header("20")).unwrap();
        assert_eq!(resumed.scored, HashSet::from([(1, 10), (2, 20)]));
        resumed.append(&row(3, 30, -5)).unwrap();
        assert_eq!(parse(&std::fs::read_to_string(&path).unwrap()).unwrap().rows.len(), 3);
        let refused = ScoresFile::open(&path, &header("22")).err().unwrap();
        assert!(refused.contains("another scoring") && refused.contains("depth"), "{refused}");
        std::fs::remove_file(&path).ok();
        assert!(parse("row\t1\t2\n").is_err());
        assert!(parse("row\tzz\t1\t2\tcp 3\t20\t5\n").is_err());
    }

    #[test]
    fn mates_rank_beyond_every_evaluation_nearer_ones_first() {
        assert_eq!(mover_cp(BookScore::Cp(-40)), -40);
        assert!(mover_cp(BookScore::Mate(1)) > mover_cp(BookScore::Mate(3)));
        assert!(mover_cp(BookScore::Mate(3)) > 5_000);
        assert!(mover_cp(BookScore::Mate(-1)) < mover_cp(BookScore::Mate(-3)), "mated sooner is worse");
        assert!(mover_cp(BookScore::Mate(-3)) < -5_000);
    }

    fn judged(parent: u64, book_move: u16, cp: i32, points: u64) -> Judged {
        Judged { parent, book_move, cp, points }
    }

    #[test]
    fn softmax_weighs_each_move_by_its_distance_from_the_best() {
        let moves = [judged(1, 10, 30, 5), judged(1, 11, -16, 50), judged(1, 12, -300, 9), judged(2, 20, 0, 1)];
        let book = softmax_entries(&moves, 20.0);
        // 46 cp below the best at T = 20: e^-2.3, about a tenth.
        let tenth = (65_535.0 * (-46.0f64 / 20.0).exp()).round() as u16;
        assert_eq!(
            book.entries,
            vec![Entry { key: 1, mv: 10, weight: 65_535 }, Entry { key: 1, mv: 11, weight: tenth }, Entry { key: 2, mv: 20, weight: 65_535 }]
        );
        assert!((6_000..7_000).contains(&tenth), "{tenth}");
        assert_eq!((book.positions, book.zeros), (2, 1), "330 cp below rounds to 0: left out");
        let mated = [judged(3, 30, mover_cp(BookScore::Mate(2)), 1), judged(3, 31, 900, 1)];
        assert_eq!(softmax_entries(&mated, 20.0).entries, vec![Entry { key: 3, mv: 30, weight: 65_535 }], "a mate takes the position");
    }

    #[test]
    fn the_report_counts_bad_moves_and_the_human_book_s_chance_of_one() {
        let moves = [
            // In reach: the bad move holds 30 of 40 points, a 75% chance.
            judged(1, 10, 0, 10),
            judged(1, 11, -60, 30),
            // In reach: nothing bad.
            judged(2, 20, 5, 4),
            judged(2, 21, -10, 4),
            // Beyond reach: bad, counted in all only.
            judged(3, 30, 0, 1),
            judged(3, 31, -200, 1),
        ];
        let plies = HashMap::from([(1, 0), (2, 5), (3, LICHESS_REACH_PLY + 1)]);
        let report = report(&moves, &plies);
        assert_eq!((report.moves, report.bad, report.moves_in_reach, report.bad_in_reach), (6, 2, 4, 1));
        assert_eq!(report.positions_in_reach, 2);
        assert!((report.old_chance_in_reach - 0.375).abs() < 1e-12, "(0.75 + 0) / 2");
        assert_eq!(report.worst, vec![(0.75, 0, 1, vec![(11, 60)])]);
        let lines = report_lines(&report, &HashMap::from([(1, "start".to_string())]));
        assert!(lines.iter().any(|(name, value)| name == "report_worst" && value.contains("start") && value.contains("-60")), "{lines:?}");
    }

    #[test]
    fn plies_are_the_fewest_from_the_root() {
        let plies = plies_from(1, &[(1, 2), (2, 3), (1, 3), (3, 4)]);
        assert_eq!(plies, HashMap::from([(1, 0), (2, 1), (3, 1), (4, 2)]));
    }

    #[test]
    fn a_polyglot_move_reads_as_its_squares() {
        assert_eq!(move_text((12 << 6) | 28), "e2e4");
        assert_eq!(move_text((4 << 12) | (52 << 6) | 60), "e7e8q");
        assert_eq!(move_text((4 << 6) | 7), "e1h1");
    }
}
