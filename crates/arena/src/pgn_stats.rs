//! `arena pgn-stats <pgn>`: how each player of a PGN spent its clock, from the `{<score>/<depth>
//! <time>s}` comment the arena writes after every move. Per player (its `White` or `Black` tag):
//! the moves counted, the mean and median time a move, the mean depth, the same by phase, and the
//! mean of each game's total time. Read-only.
//!
//! Why (2026-11-28): the dual networks drew their twins at fixed depths 9 and 12 and lost at 10+0.1,
//! so the loss was on the clock, the time manager's or the speed's; this tells the two apart.

use std::collections::BTreeMap;
use std::fs;
use std::process;

/// The full move a phase ends before: the opening and early middlegame, the middlegame, the rest.
const PHASE_ENDS: [u32; 2] = [20, 40];
const PHASE_NAMES: [&str; 3] = ["moves 1-19", "moves 20-39", "moves 40+"];

/// `arena pgn-stats <pgn>`: prints each player's block, or says why not and exits.
pub(crate) fn command(rest: &[String]) {
    let [path] = rest else {
        eprintln!("usage: arena pgn-stats <pgn>");
        process::exit(2);
    };
    let text = fs::read_to_string(path).unwrap_or_else(|error| {
        eprintln!("{path}: {error}");
        process::exit(1);
    });
    print!("{}", report(&stats(&text)));
}

/// One move's comment: its depth and its time in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Spent {
    depth: u32,
    seconds: f64,
}

/// `<score>/<depth> <time>s`, the score a number or `#<n>`: `None` for anything else.
fn parse_comment(comment: &str) -> Option<Spent> {
    let (score, rest) = comment.trim().split_once('/')?;
    let score = score.strip_prefix('#').unwrap_or(score);
    score.parse::<f64>().ok()?;
    let (depth, time) = rest.split_once(' ')?;
    let seconds = time.trim().strip_suffix('s')?.parse::<f64>().ok()?;
    Some(Spent { depth: depth.parse().ok()?, seconds })
}

/// One player's moves, each with its full-move number, and each game's total time.
#[derive(Debug, Default)]
struct Player {
    moves: Vec<(u32, Spent)>,
    game_totals: Vec<f64>,
    skipped: usize,
}

/// Every player's moves over the PGN's games, by its tag.
fn stats(text: &str) -> BTreeMap<String, Player> {
    let mut players: BTreeMap<String, Player> = BTreeMap::new();
    let mut tags: BTreeMap<String, String> = BTreeMap::new();
    let mut movetext = String::new();
    let mut flush = |tags: &mut BTreeMap<String, String>, movetext: &mut String| {
        if !movetext.trim().is_empty() {
            game(tags, movetext, &mut players);
        }
        tags.clear();
        movetext.clear();
    };
    for line in text.lines() {
        let line = line.trim();
        if let Some(tag) = line.strip_prefix('[').and_then(|line| line.strip_suffix(']')) {
            if !movetext.trim().is_empty() {
                flush(&mut tags, &mut movetext);
            }
            if let Some((name, value)) = tag.split_once(' ') {
                tags.insert(name.to_string(), value.trim_matches('"').to_string());
            }
        } else {
            movetext.push_str(line);
            movetext.push(' ');
        }
    }
    flush(&mut tags, &mut movetext);
    players
}

/// One game's comments, in order, credited to White and Black by alternation from the side to move
/// of its `FEN` (White without one), each with its full-move number.
fn game(tags: &BTreeMap<String, String>, movetext: &str, players: &mut BTreeMap<String, Player>) {
    let names = ["White", "Black"].map(|tag| tags.get(tag).cloned().unwrap_or_else(|| format!("({tag})")));
    let fen_fields: Vec<&str> = tags.get("FEN").map(|fen| fen.split_whitespace().collect()).unwrap_or_default();
    let mut side = usize::from(fen_fields.get(1) == Some(&"b"));
    let mut fullmove: u32 = fen_fields.get(5).and_then(|field| field.parse().ok()).unwrap_or(1);
    let mut totals = [0.0f64; 2];
    let mut rest = movetext;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        let comment = &rest[open + 1..open + close];
        let player = players.entry(names[side].clone()).or_default();
        match parse_comment(comment) {
            Some(spent) => {
                player.moves.push((fullmove, spent));
                totals[side] += spent.seconds;
            }
            None => player.skipped += 1,
        }
        if side == 1 {
            fullmove += 1;
        }
        side = 1 - side;
        rest = &rest[open + close + 1..];
    }
    for (name, total) in names.iter().zip(totals) {
        players.entry(name.clone()).or_default().game_totals.push(total);
    }
}

/// `count, mean time, median time, mean depth` of `moves`, as one line's columns.
fn summary(moves: &[Spent]) -> String {
    if moves.is_empty() {
        return "no moves".to_string();
    }
    let n = moves.len() as f64;
    let mut times: Vec<f64> = moves.iter().map(|spent| spent.seconds).collect();
    times.sort_by(f64::total_cmp);
    let median = if !times.len().is_multiple_of(2) { times[times.len() / 2] } else { (times[times.len() / 2 - 1] + times[times.len() / 2]) / 2.0 };
    let mean_time = times.iter().sum::<f64>() / n;
    let mean_depth = moves.iter().map(|spent| f64::from(spent.depth)).sum::<f64>() / n;
    format!("{:>6} moves, time mean {mean_time:.3}s median {median:.3}s, depth mean {mean_depth:.2}", moves.len())
}

/// One block per player: overall, by phase, then the games' mean total and the skipped comments.
fn report(players: &BTreeMap<String, Player>) -> String {
    let mut out = String::new();
    for (name, player) in players {
        let all: Vec<Spent> = player.moves.iter().map(|&(_, spent)| spent).collect();
        out.push_str(&format!("{name}\n  all          {}\n", summary(&all)));
        for (phase, label) in PHASE_NAMES.iter().enumerate() {
            let from = if phase == 0 { 0 } else { PHASE_ENDS[phase - 1] };
            let to = PHASE_ENDS.get(phase).copied().unwrap_or(u32::MAX);
            let moves: Vec<Spent> =
                player.moves.iter().filter(|&&(fullmove, _)| (from..to).contains(&fullmove)).map(|&(_, spent)| spent).collect();
            out.push_str(&format!("  {label:<12} {}\n", summary(&moves)));
        }
        let games = player.game_totals.len();
        let mean_total = if games == 0 { 0.0 } else { player.game_totals.iter().sum::<f64>() / games as f64 };
        out.push_str(&format!("  {games} games, a game's time mean {mean_total:.2}s; {} comments skipped\n\n", player.skipped));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comment_reads_as_its_depth_and_time() {
        assert_eq!(parse_comment("0.19/15 0.5s"), Some(Spent { depth: 15, seconds: 0.5 }));
        assert_eq!(parse_comment("-318.97/27 0.0s"), Some(Spent { depth: 27, seconds: 0.0 }));
        assert_eq!(parse_comment("#-6/12 0.1s"), Some(Spent { depth: 12, seconds: 0.1 }));
        assert_eq!(parse_comment("book"), None);
        assert_eq!(parse_comment("0.19/15"), None);
        assert_eq!(parse_comment("x/15 0.5s"), None);
    }

    /// Two games, the first from a position with Black to move at move 7, the players swapping
    /// colours: each player's moves and totals are its own, whatever its colour.
    #[test]
    fn each_player_is_credited_its_own_moves_by_alternation() {
        let pgn = "\
[White \"a\"]
[Black \"b\"]
[FEN \"8/8/8/8/8/8/8/K6k b - - 3 7\"]

7... Kg1 {0.10/10 0.4s} 8. Kb1 {0.00/12 0.2s} Kf1 {#3/14 0.6s} 1/2-1/2

[White \"b\"]
[Black \"a\"]

1. e4 {0.20/9 1.0s} e5 {book} 2. Nf3 {0.30/11 0.0s} 1-0
";
        let players = stats(pgn);
        let a = &players["a"];
        assert_eq!(a.moves, vec![(8, Spent { depth: 12, seconds: 0.2 })]);
        assert_eq!(a.skipped, 1, "the book comment");
        assert_eq!(a.game_totals, vec![0.2, 0.0]);
        let b = &players["b"];
        let depths: Vec<u32> = b.moves.iter().map(|&(_, spent)| spent.depth).collect();
        assert_eq!(depths, vec![10, 14, 9, 11]);
        let fullmoves: Vec<u32> = b.moves.iter().map(|&(fullmove, _)| fullmove).collect();
        assert_eq!(fullmoves, vec![7, 8, 1, 2]);
        assert!((b.game_totals[0] - 1.0).abs() < 1e-9 && (b.game_totals[1] - 1.0).abs() < 1e-9);
    }
}
