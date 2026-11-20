//! `smithy material --network <player>`: how much material is worth, by Lc0's teacher, by the game's
//! result, by the label a network trained on, and by the network itself, over Lc0's frozen test set.
//! A one-off check, run by hand, read-only but for label sets the freezer adds as training would, on
//! the CPU.
//!
//! The release's network reads a material pawn as 30 centipawns, and judged an extra exchange for a
//! pawn as nearly level. Where that comes from has three candidates, each with its own cure: the
//! teacher's Q (strong self-play, and selection bias: an imbalance there is mostly a sound
//! sacrifice), the blend with the game's result (many won positions end drawn), or a network that
//! does not follow its labels. Grouping the positions by material balance and averaging the four in
//! each group tells them apart.
//!
//! Only settled positions are grouped: PeSTO's quiescence equal to its static evaluation, so no capture
//! sequence is pending and the material on the board is the position's (the rule
//! `nnue::measure_label_scale` counts by). Material is counted 1, 3, 3, 5, 9, from the side to move.

use fattorini::board::color::Color;
use fattorini::board::piece::Piece;
use fattorini::board::piece_type::PieceType;
use fattorini::board::Board;
use fattorini::labels::Curve;
use fattorini::nnue::Network;
use fattorini::pipeline::preprocessing_config::{self, ArmLambda};

use super::buckets::Labelling;
use super::cache::CacheRecord;
use super::{freeze, lc0, lease, pgn_train_stream, polyline, remote};

/// The command, in no usage line: a check run by hand, as `king-buckets` is.
pub(crate) const COMMAND: &str = "material";

/// The balance's groups run from `-BALANCE_LIMIT` (or less) to `BALANCE_LIMIT` (or more).
const BALANCE_LIMIT: i32 = 9;

/// The pawns-only groups run from `-PAWNS_LIMIT` (or less) to `PAWNS_LIMIT` (or more).
const PAWNS_LIMIT: i32 = 3;

/// The label sets read, in this order: Lc0's Q alone, the game's result alone, and the polyline's,
/// what a network of Lc0's corpus trained on.
const ARMS: [ArmLambda; 3] = [ArmLambda::Constant(0.0), ArmLambda::Constant(1.0), ArmLambda::Polyline];

pub(crate) fn run(args: &[String]) {
    const USAGE: &str = "usage: smithy material --network <player or .nnue file>";
    let network = match args {
        [flag, network] if flag == "--network" => network.as_str(),
        _ => fail(USAGE),
    };
    match measure(network) {
        Ok(report) => println!("{report}"),
        Err(message) => fail(&message),
    }
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

/// The network named by `network`, a `.nnue` file or a player (any `@<tuning>` ignored: the
/// material it sees is its network's), and the curve it was trained to mean.
fn load_network(network: &str) -> Result<(Network, Curve, String), String> {
    let path = if network.ends_with(".nnue") {
        network.to_string()
    } else {
        let player = network.split_once('@').map_or(network, |(player, _)| player);
        fattorini::pipeline::players::resolve_network_file(player)?
    };
    let curve = fattorini::nnue::curve_of_version(fattorini::nnue::file_version(&path)?);
    Ok((Network::load(&path)?, curve, path))
}

/// The test set and polyline the network at `path` trained with, as training finds them: its
/// generation's own (a scaled attempt freezes its set and keeps its polyline in its generation's
/// directory), else Lc0's corpus's, shared by every unscaled attempt. The set's directory, its
/// positions, the polyline's file, and whether the set is the corpus's shared one.
fn test_set_of(path: &str) -> Result<(String, u64, String, bool), String> {
    let generation = std::path::Path::new(path).parent().and_then(std::path::Path::parent).map(|dir| dir.to_string_lossy().replace('\\', "/"));
    if let Some(generation) = generation {
        let frozen = format!("{generation}/frozen");
        if let Some(rows) = freeze::frozen_rows(&frozen)? {
            return Ok((frozen, rows, format!("{generation}/polyline.txt"), false));
        }
    }
    let root = preprocessing_config::LC0_ROOT;
    Ok((remote::remote_path(&format!("{root}/frozen")), lc0::test_positions()?, remote::remote_path(&format!("{root}/polyline.txt")), true))
}

/// The test set the network at `path` trained with, with its three label sets, any one missing added
/// by the freezer as training would add it, the corpus's shared set under its lease.
fn frozen_records(path: &str) -> Result<Vec<CacheRecord>, String> {
    let root = preprocessing_config::LC0_ROOT;
    let (frozen, rows, polyline_path, shared) = test_set_of(path)?;
    let polyline = polyline::read(&polyline_path).ok_or_else(|| format!("{polyline_path}: no polyline, so no label set the network trained on"))?;
    let labelling = Labelling::new(&ARMS, Some(polyline))?;
    if !freeze::is_complete(&frozen, rows, &labelling) {
        let _lease = match shared {
            true => Some(lease::take(&format!("{frozen}/{}", lease::FILE), &format!("the frozen test set {frozen}"))?),
            false => None,
        };
        let games = remote::remote_path(&preprocessing_config::source_test_dir(root));
        let curve = lc0::rewrite::corpus_curve()?;
        freeze::ensure(&frozen, &games, || pgn_train_stream::test_files(root), rows, &labelling, &curve)?;
    }
    println!("the test set {frozen}, {} positions; the polyline {polyline_path}", progress::grouped(rows));
    freeze::read(&frozen, rows, &labelling)
}

fn measure(network: &str) -> Result<String, String> {
    let (network, curve, path) = load_network(network)?;
    let records = frozen_records(&path)?;
    let threads = std::thread::available_parallelism().map_or(1, |count| count.get());
    let chunk = records.len().div_ceil(threads).max(1);
    let (network, curve) = (&network, &curve);
    let partials: Vec<Tally> = std::thread::scope(|scope| {
        let handles: Vec<_> = records.chunks(chunk).map(|part| scope.spawn(move || tally(part, network, curve))).collect();
        handles.into_iter().map(|handle| handle.join().expect("a tally thread")).collect()
    });
    let mut total = Tally::default();
    for partial in partials {
        total.merge(partial);
    }
    Ok(report(&total, &path, records.len()))
}

/// What one position contributes: its four expected scores, from the side to move, in `[0, 1]`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Sums {
    positions: u64,
    q: f64,
    result: f64,
    label: f64,
    network: f64,
}

impl Sums {
    fn add(&mut self, scores: [f64; 4]) {
        self.positions += 1;
        self.q += expected(scores[0]);
        self.result += expected(scores[1]);
        self.label += expected(scores[2]);
        self.network += expected(scores[3]);
    }

    fn merge(&mut self, other: &Sums) {
        self.positions += other.positions;
        self.q += other.q;
        self.result += other.result;
        self.label += other.label;
        self.network += other.network;
    }
}

/// A score in `[-1, 1]` as an expected score in `[0, 1]`.
fn expected(score: f64) -> f64 {
    (1.0 + score) / 2.0
}

/// The groups' sums, and the positions left out as unsettled.
#[derive(Debug, Default)]
struct Tally {
    by_balance: std::collections::BTreeMap<i32, Sums>,
    by_kind: std::collections::BTreeMap<Kind, Sums>,
    unsettled: u64,
}

impl Tally {
    fn merge(&mut self, other: Tally) {
        for (balance, sums) in &other.by_balance {
            self.by_balance.entry(*balance).or_default().merge(sums);
        }
        for (kind, sums) in &other.by_kind {
            self.by_kind.entry(*kind).or_default().merge(sums);
        }
        self.unsettled += other.unsettled;
    }
}

fn tally(records: &[CacheRecord], network: &Network, curve: &Curve) -> Tally {
    let mut tally = Tally::default();
    for record in records {
        let board = fattorini::features::board_from_canonical(&record.canonical);
        if !is_settled(&board) {
            tally.unsettled += 1;
            continue;
        }
        let own = curve.score(f64::from(network.evaluate(&board)) / 100.0);
        let scores = [f64::from(record.labels[0]), f64::from(record.labels[1]), f64::from(record.labels[2]), own];
        let material = Material::of(&board);
        tally.by_balance.entry(material.balance().clamp(-BALANCE_LIMIT, BALANCE_LIMIT)).or_default().add(scores);
        tally.by_kind.entry(material.kind()).or_default().add(scores);
    }
    tally
}

/// No capture sequence pending: PeSTO's quiescence leaves its static evaluation as it is.
fn is_settled(board: &Board) -> bool {
    fattorini::search::pesto_quiescence(board) == fattorini::eval::evaluate(board)
}

/// Each side's pieces, the side to move's first: pawns, minor pieces, rooks, queens.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Material {
    own: [i32; 4],
    other: [i32; 4],
}

impl Material {
    fn of(board: &Board) -> Material {
        let count = |color: Color| -> [i32; 4] {
            let n = |piece_type: PieceType| board.bitboard(Piece::new(color, piece_type)).count() as i32;
            [n(PieceType::Pawn), n(PieceType::Knight) + n(PieceType::Bishop), n(PieceType::Rook), n(PieceType::Queen)]
        };
        let side = board.side_to_move();
        Material { own: count(side), other: count(side.opposite()) }
    }

    /// The side to move's material minus the other's, pawn 1, minor piece 3, rook 5, queen 9.
    fn balance(&self) -> i32 {
        const VALUES: [i32; 4] = [1, 3, 5, 9];
        (0..4).map(|kind| VALUES[kind] * (self.own[kind] - self.other[kind])).sum()
    }

    /// Which imbalance this is, from the side to move.
    fn kind(&self) -> Kind {
        let difference = |kind: usize| self.own[kind] - self.other[kind];
        let (pawns, minors, rooks, queens) = (difference(0), difference(1), difference(2), difference(3));
        match (minors, rooks, queens) {
            (0, 0, 0) => Kind::Pawns(pawns.clamp(-PAWNS_LIMIT, PAWNS_LIMIT)),
            (-1, 1, 0) => Kind::ExchangeUp,
            (1, -1, 0) => Kind::ExchangeDown,
            (1, 0, 0) => Kind::MinorUp,
            (-1, 0, 0) => Kind::MinorDown,
            _ => Kind::Other,
        }
    }
}

/// An imbalance from the side to move: the pieces equal and the pawns apart; the exchange (a rook
/// for a minor piece, the queens equal); a minor piece more or less, the rest of the pieces equal
/// (pawns any); anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Pawns(i32),
    ExchangeUp,
    ExchangeDown,
    MinorUp,
    MinorDown,
    Other,
}

impl Kind {
    fn name(self) -> String {
        match self {
            Kind::Pawns(pawns) if pawns.abs() == PAWNS_LIMIT => format!("pieces equal, pawns {pawns:+} or more"),
            Kind::Pawns(pawns) => format!("pieces equal, pawns {pawns:+}"),
            Kind::ExchangeUp => "the exchange up".to_string(),
            Kind::ExchangeDown => "the exchange down".to_string(),
            Kind::MinorUp => "a minor piece up".to_string(),
            Kind::MinorDown => "a minor piece down".to_string(),
            Kind::Other => "any other imbalance".to_string(),
        }
    }
}

/// The two tables: by balance, then by kind, each row's expected scores in percent.
fn report(tally: &Tally, path: &str, positions: usize) -> String {
    let settled: u64 = tally.by_balance.values().map(|sums| sums.positions).sum();
    let mut text = format!(
        "{path}\nLc0's frozen test set: {} positions, {} settled, {} left out with a capture pending\n\
         Expected scores from the side to move, in percent: Q (lambda 0, Lc0's), result (lambda 1, the game's), \
         label (the polyline's, trained on), network (its own prediction)\n",
        progress::grouped(positions as u64),
        progress::grouped(settled),
        progress::grouped(tally.unsettled),
    );
    let header = |title: &str| format!("\n{title:<34} {:>12} {:>7} {:>7} {:>7} {:>7} {:>7}\n", "positions", "share", "Q", "result", "label", "network");
    let row = |name: String, sums: &Sums| {
        let n = sums.positions.max(1) as f64;
        format!(
            "{name:<34} {:>12} {:>6.2}% {:>6.1}% {:>6.1}% {:>6.1}% {:>6.1}%\n",
            progress::grouped(sums.positions),
            100.0 * sums.positions as f64 / settled.max(1) as f64,
            100.0 * sums.q / n,
            100.0 * sums.result / n,
            100.0 * sums.label / n,
            100.0 * sums.network / n,
        )
    };
    text.push_str(&header("material balance (1, 3, 3, 5, 9)"));
    for (balance, sums) in &tally.by_balance {
        let name = match *balance {
            b if b == -BALANCE_LIMIT => format!("{b:+} or less"),
            b if b == BALANCE_LIMIT => format!("{b:+} or more"),
            b => format!("{b:+}"),
        };
        text.push_str(&row(name, sums));
    }
    text.push_str(&header("imbalance"));
    for (kind, sums) in &tally.by_kind {
        text.push_str(&row(kind.name(), sums));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material(fen: &str) -> Material {
        Material::of(&Board::from_fen(fen).unwrap().0)
    }

    #[test]
    fn the_balance_and_the_kind_are_the_side_to_moves() {
        // The starting position: level.
        let start = material("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        assert_eq!((start.balance(), start.kind()), (0, Kind::Pawns(0)));
        // White a pawn up, seen by White to move, then by Black to move.
        let white = material("4k3/pp6/8/8/8/8/PPP5/4K3 w - - 0 1");
        assert_eq!((white.balance(), white.kind()), (1, Kind::Pawns(1)));
        let black = material("4k3/pp6/8/8/8/8/PPP5/4K3 b - - 0 1");
        assert_eq!((black.balance(), black.kind()), (-1, Kind::Pawns(-1)));
        // Four pawns up groups with three.
        assert_eq!(material("4k3/8/8/8/8/8/PPPP4/4K3 w - - 0 1").kind(), Kind::Pawns(PAWNS_LIMIT));
    }

    #[test]
    fn the_exchange_and_a_minor_piece_are_told_apart() {
        // White a rook against Black's knight, a pawn down: the exchange for a pawn, +1 in all.
        let exchange = material("4k3/pn6/8/8/8/8/R7/4K3 w - - 0 1");
        assert_eq!((exchange.balance(), exchange.kind()), (1, Kind::ExchangeUp));
        assert_eq!(material("4k3/pn6/8/8/8/8/R7/4K3 b - - 0 1").kind(), Kind::ExchangeDown);
        // A bishop for two pawns.
        let minor = material("4k3/pp6/8/8/8/8/B7/4K3 w - - 0 1");
        assert_eq!((minor.balance(), minor.kind()), (1, Kind::MinorUp));
        // A queen against two rooks is another imbalance.
        assert_eq!(material("r3k2r/8/8/8/8/8/8/3QK3 w - - 0 1").kind(), Kind::Other);
    }

    #[test]
    fn a_position_with_a_capture_pending_is_not_settled() {
        let quiet = Board::from_fen("4k3/pp6/8/8/8/8/PPP5/4K3 w - - 0 1").unwrap().0;
        assert!(is_settled(&quiet));
        // White's queen takes Black's undefended rook.
        let hanging = Board::from_fen("4k3/8/8/3r4/8/8/3Q4/4K3 w - - 0 1").unwrap().0;
        assert!(!is_settled(&hanging));
    }

    #[test]
    fn an_expected_score_runs_from_a_loss_to_a_win() {
        assert_eq!((expected(-1.0), expected(0.0), expected(1.0)), (0.0, 0.5, 1.0));
    }
}
