//! The games of a task an arena sends (`smithy.rs`, `POST /task`), read and checked before anything
//! of them is recorded: every tag, every move legal from the game's `FEN`, a result, and for
//! self-play a score comment after every move, as `game_labels` reads them back in training. An
//! arena is trusted to play well, never to write well: one malformed game in a corpus would stop a
//! training run hours later.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::str::FromStr;

use pgn_reader::{RawComment, RawTag, Reader, SanPlus, Visitor};
use shakmaty::fen::Fen;
use shakmaty::{CastlingMode, Chess, Position};

use pgn_replay::STARTING_POSITION_FEN;

/// One game of a task.
#[derive(Debug)]
pub(crate) struct TaskGame {
    pub(crate) tags: HashMap<String, String>,
    /// Its moves, all legal.
    pub(crate) plies: usize,
    /// White's score: 1, 0.5 or 0.
    pub(crate) result: f64,
    /// Whether every move carries a score comment `game_labels` reads: a pawns value or `#N`, and at
    /// most a best move after it.
    pub(crate) scored: bool,
}

impl TaskGame {
    pub(crate) fn tag(&self, name: &str) -> Option<&str> {
        self.tags.get(name).map(String::as_str)
    }

    /// Its training positions, as `game_labels` counts a game with no book: every move's, and the
    /// final one.
    pub(crate) fn positions(&self) -> u64 {
        self.plies as u64 + 1
    }
}

struct Collector {
    tags: HashMap<String, String>,
    moves: Vec<SanPlus>,
    comments: Vec<String>,
}

impl Visitor for Collector {
    type Tags = ();
    type Movetext = ();
    type Output = Collector;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        self.tags.clear();
        ControlFlow::Continue(())
    }

    fn tag(&mut self, _tags: &mut Self::Tags, name: &[u8], value: RawTag<'_>) -> ControlFlow<Self::Output> {
        self.tags.insert(String::from_utf8_lossy(name).to_string(), String::from_utf8_lossy(&value.decode()).to_string());
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, _tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        self.moves.clear();
        self.comments.clear();
        ControlFlow::Continue(())
    }

    fn san(&mut self, _movetext: &mut Self::Movetext, san_plus: SanPlus) -> ControlFlow<Self::Output> {
        self.moves.push(san_plus);
        ControlFlow::Continue(())
    }

    fn comment(&mut self, _movetext: &mut Self::Movetext, comment: RawComment<'_>) -> ControlFlow<Self::Output> {
        self.comments.push(String::from_utf8_lossy(comment.as_bytes()).trim().trim_start_matches('{').trim_end_matches('}').trim().to_string());
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, _movetext: Self::Movetext) -> Self::Output {
        Collector { tags: std::mem::take(&mut self.tags), moves: std::mem::take(&mut self.moves), comments: std::mem::take(&mut self.comments) }
    }
}

/// Whether `comment` is a move's score as self-play writes it: `{0.31}`, `{#-4}`, `{0.31 g8f6}`.
fn is_score(comment: &str) -> bool {
    let mut fields = comment.split_whitespace();
    let Some(score) = fields.next() else {
        return false;
    };
    let number = score.strip_prefix('#').unwrap_or(score);
    let best_ok = fields.next().is_none_or(|best| (4..=5).contains(&best.len()) && best.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    number.parse::<f64>().is_ok_and(f64::is_finite) && best_ok && fields.next().is_none()
}

/// The games of `text`, each checked; an error naming the first that is not a whole, legal game.
pub(crate) fn read(text: &str) -> Result<Vec<TaskGame>, String> {
    let mut reader = Reader::new(std::io::Cursor::new(text.as_bytes()));
    let mut collector = Collector { tags: HashMap::new(), moves: Vec::new(), comments: Vec::new() };
    let mut games = Vec::new();
    loop {
        let collected = match reader.read_game(&mut collector) {
            Ok(Some(collected)) => collected,
            Ok(None) => break,
            Err(error) => return Err(format!("game {}: {error}", games.len() + 1)),
        };
        games.push(check(collected).map_err(|message| format!("game {}: {message}", games.len() + 1))?);
    }
    if games.is_empty() {
        return Err("no game".to_string());
    }
    Ok(games)
}

fn check(collected: Collector) -> Result<TaskGame, String> {
    let fen_text = collected.tags.get("FEN").map_or(STARTING_POSITION_FEN, String::as_str);
    let fen = Fen::from_str(fen_text).map_err(|error| format!("FEN {fen_text:?}: {error}"))?;
    let mut position: Chess = fen.into_position(CastlingMode::Standard).map_err(|error| format!("FEN {fen_text:?} is illegal: {error}"))?;
    for (ply, san_plus) in collected.moves.iter().enumerate() {
        let mv = san_plus.san.to_move(&position).map_err(|error| format!("ply {ply}: {san_plus} is not legal: {error}"))?;
        position = position.play(mv).map_err(|error| format!("ply {ply}: {error}"))?;
    }
    let result = match collected.tags.get("Result").map(String::as_str) {
        Some("1-0") => 1.0,
        Some("0-1") => 0.0,
        Some("1/2-1/2") => 0.5,
        other => return Err(format!("its result {other:?} is no game's end")),
    };
    let scored = collected.comments.len() == collected.moves.len() && collected.comments.iter().all(|comment| is_score(comment));
    Ok(TaskGame { tags: collected.tags, plies: collected.moves.len(), result, scored })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAME: &str = "[Event \"fattorini self-play\"]\n[White \"pesto\"]\n[Black \"pesto\"]\n[Result \"1/2-1/2\"]\n\
        [FEN \"8/8/8/4k3/8/8/8/4K2R w K - 0 1\"]\n[SetUp \"1\"]\n[Chore \"c000001\"]\n\n\
        1. O-O {0.31 g1g2} Kd5 {#-4} 1/2-1/2\n\n";

    #[test]
    fn a_whole_scored_game_reads_with_its_tags_and_positions() {
        let games = read(&format!("{GAME}{GAME}")).unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].tag("Chore"), Some("c000001"));
        assert_eq!((games[0].plies, games[0].positions(), games[0].result, games[0].scored), (2, 3, 0.5, true));
    }

    #[test]
    fn an_illegal_move_a_missing_result_or_nothing_is_refused() {
        assert!(read(&GAME.replace("Kd5", "Kd3")).unwrap_err().contains("not legal"));
        assert!(read(&GAME.replace("[Result \"1/2-1/2\"]\n", "")).unwrap_err().contains("result"));
        assert!(read("").is_err());
    }

    #[test]
    fn a_move_without_a_score_leaves_the_game_unscored() {
        assert!(!read(&GAME.replace(" {#-4}", "")).unwrap()[0].scored);
        assert!(!read(&GAME.replace("{#-4}", "{good}")).unwrap()[0].scored);
        assert!(!read(&GAME.replace("{#-4}", "{-1.2/9 0.4s}")).unwrap()[0].scored, "a match's comment is no label");
    }
}
