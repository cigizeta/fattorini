//! Static exchange evaluation. See `claude/active/search.md`'s own design section for the
//! full trail (why the real swap-list algorithm was required, not an approximation; why the
//! threshold form is built as a wrapper over an exact-value function rather than the other
//! way round). This file has the attacker-enumeration primitive, least-valuable-attacker
//! selection, and the gain-array walk (`see` itself) the algorithm is built on -- `see_ge`,
//! the threshold-test wrapper `quiescence` will actually call, lands in a later, separate
//! piece.

use crate::board::bitboard::Bitboard;
use crate::board::chess_move::{Move, MoveKind};
use crate::board::color::Color;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use crate::board::Board;
use crate::eval::piece_value;
use crate::movegen::bishop::bishop_attacks;
use crate::movegen::king::king_attacks;
use crate::movegen::knight::knight_attacks;
use crate::movegen::pawn::pawn_attacks;
use crate::movegen::rook::rook_attacks;

/// Every square, of either color, from which a piece currently attacks `target` -- generalizing
/// `attacks::is_square_attacked`'s own "place a virtual piece of each type on the target square
/// and see which real pieces answer" technique from a yes/no question into the actual attacking
/// bitboard, since static exchange evaluation needs to enumerate and rank *every* attacker, not
/// just confirm one exists.
///
/// Takes `occupancy` as its own parameter rather than always reading `board.all_occupancy()`:
/// SEE simulates a capture sequence by removing one attacker's origin square from occupancy at
/// each step (that piece has just "moved" onto `target`), which can reveal a sliding attacker
/// that was previously blocked (an X-ray attack). Passing occupancy in lets the same function
/// serve both the real, current position (call it with `board.all_occupancy()`) and every
/// intermediate, shrunk step of a simulated exchange, without duplicating this logic.
///
/// Every returned square is intersected with `occupancy`: a piece bitboard on `board` never
/// changes (the real board isn't mutated), so without this, a piece whose square was removed
/// from `occupancy` earlier in a simulated exchange would still show up here as if it were
/// still attacking -- occupancy is what actually tracks "is this square still populated" during
/// the simulation, not `board`'s own piece bitboards.
pub fn attackers_to(board: &Board, target: Square, occupancy: Bitboard) -> Bitboard {
    let mut attackers = Bitboard::EMPTY;

    let knights = board.bitboard(Piece::new(Color::White, PieceType::Knight))
        | board.bitboard(Piece::new(Color::Black, PieceType::Knight));
    attackers = attackers | (knight_attacks(target) & knights);

    let kings = board.bitboard(Piece::new(Color::White, PieceType::King))
        | board.bitboard(Piece::new(Color::Black, PieceType::King));
    attackers = attackers | (king_attacks(target) & kings);

    let diagonal_attackers = board.bitboard(Piece::new(Color::White, PieceType::Bishop))
        | board.bitboard(Piece::new(Color::Black, PieceType::Bishop))
        | board.bitboard(Piece::new(Color::White, PieceType::Queen))
        | board.bitboard(Piece::new(Color::Black, PieceType::Queen));
    attackers = attackers | (bishop_attacks(target, occupancy) & diagonal_attackers);

    let straight_attackers = board.bitboard(Piece::new(Color::White, PieceType::Rook))
        | board.bitboard(Piece::new(Color::Black, PieceType::Rook))
        | board.bitboard(Piece::new(Color::White, PieceType::Queen))
        | board.bitboard(Piece::new(Color::Black, PieceType::Queen));
    attackers = attackers | (rook_attacks(target, occupancy) & straight_attackers);

    // Same color-flip trick as `is_square_attacked`: the squares a white pawn would attack
    // *into* `target` are exactly the squares `pawn_attacks(target, Color::Black)` gives, since
    // diagonal attacks mirror under a color flip. Each color's real pawns are checked against
    // the flipped table for the *other* color, one call per side.
    let white_pawns = board.bitboard(Piece::new(Color::White, PieceType::Pawn));
    attackers = attackers | (pawn_attacks(target, Color::Black) & white_pawns);
    let black_pawns = board.bitboard(Piece::new(Color::Black, PieceType::Pawn));
    attackers = attackers | (pawn_attacks(target, Color::White) & black_pawns);

    attackers & occupancy
}

/// `color`'s cheapest attacker among `attackers_to`'s own return value (both colors combined --
/// this does the color split internally via `board.occupancy(color)`, so callers don't have to
/// remember to do it themselves at every call site) -- the square it stands on, and its piece
/// type, or `None` if `color` has no attacker at all under whatever occupancy `attackers` was
/// computed against.
///
/// Walks `PieceType::ALL` in its own already-declared ascending-value order (pawn through
/// king) -- the swap algorithm's own rule that each side captures with its cheapest available
/// piece first, so a trade is never conceded more material than it has to be. Returns the piece
/// *type*, not the full `Piece`, since the caller already knows `color`; returns the square (not
/// just "yes, this type attacks") since the caller needs it to remove that exact square from
/// occupancy for the exchange's next step.
pub fn least_valuable_attacker(board: &Board, attackers: Bitboard, color: Color) -> Option<(Square, PieceType)> {
    let attackers = attackers & board.occupancy(color);
    for piece_type in PieceType::ALL {
        if let Some(square) = (attackers & board.bitboard(Piece::new(color, piece_type))).squares().next() {
            return Some((square, piece_type));
        }
    }
    None
}

/// The material result of playing `mv`, then simulating the full capture/recapture sequence
/// on `mv.to()` with both sides playing optimally -- including each side's option to decline a
/// continuation that would leave them worse off than simply stopping (the standard swap-list
/// algorithm's whole point, as opposed to a naive "just add up every possible capture").
/// Positive means the side to move (before `mv`) comes out ahead; negative means behind; `mv`
/// need not even be a capture (a quiet move's own `gain[0]` is just `0`, the sequence then
/// unfolds however the recapture-only case works out -- passed as a value that can never
/// actually be won). Two known, accepted limitations, shared with real engines' own
/// implementations (see `search.md`'s design section): pins are ignored (a piece pinned to its
/// own king can still be "used" as an attacker here, even though the real recapture would be
/// illegal), and a promotion is only accounted for on `mv` itself, never on a later recapture
/// that happens to land on the back rank.
pub fn see(board: &Board, mv: Move) -> i32 {
    let target = mv.to();
    let (first_gain, mut occupancy, mut on_target) = exchange_start(board, mv);

    // On the stack, not a `Vec`: SEE runs for move ordering at every interior node, and a heap
    // allocation per call was a measurable share of a node. Each recapture below removes its
    // attacker from `occupancy`, so no piece captures twice and there are at most 31 of them
    // (32 pieces, less `mv`'s own): with `gain[0]`, 32 entries, and 33 leaves a margin.
    let mut gain = [0i32; 33];
    gain[0] = first_gain;
    let mut len = 1;

    let mut color = board.side_to_move().opposite();
    loop {
        let attackers = attackers_to(board, target, occupancy);
        let (square, piece_type) = match least_valuable_attacker(board, attackers, color) {
            Some(found) => found,
            None => break,
        };
        gain[len] = piece_value(on_target);
        len += 1;
        occupancy = occupancy ^ Bitboard::from_square(square);
        on_target = piece_type;
        color = color.opposite();
    }

    // Backward resolution, derived and hand-verified directly (not transcribed from any
    // fetched pseudocode -- chessprogramming.org's own page couldn't be quoted verbatim
    // through the tool available this session, character-limited to short paraphrased
    // fragments, and a first attempt at reconstructing its exact bookkeeping from those
    // fragments gave a demonstrably wrong answer on a hand-worked 2-ply check, caught before
    // landing here). `gain[i]` at this point is `d[i]`, the raw material captured at ply `i`
    // if that capture happens at all, in isolation -- what it's actually worth to whoever
    // captures at ply `i` also depends on what ply `i + 1`'s side can still get in return, and
    // that side will only continue if doing so is actually good for them (`> 0`), since they
    // can always simply decline and get nothing further instead. So: the last ply's raw value
    // is already final (nothing follows it to subtract), and each earlier ply's value is its
    // own raw capture minus whatever the very next ply is worth once resolved the same way,
    // floored at `0` for that "or just decline" option.
    //
    // Verified against two independent hand-worked positions before landing (see this
    // module's own tests below): a 2-ply queen-captures-a-defended-pawn trade (correctly
    // resolves to a clean material loss for the queen's side, not the wrong, larger loss an
    // earlier, buggy version of this exact loop produced), and a 3-ply knight/bishop/rook
    // sequence where declining a bad recapture is what makes the real, correct answer smaller
    // than "add up every hypothetical capture in the list" would suggest.
    for i in (0..len - 1).rev() {
        gain[i] -= gain[i + 1].max(0);
    }
    gain[0]
}

/// Where every exchange on `mv.to()` starts, for `see` and `see_ge` alike: what `mv` itself
/// wins, the occupancy the opponent's first recapture is found against, and the piece then on
/// the target.
fn exchange_start(board: &Board, mv: Move) -> (i32, Bitboard, PieceType) {
    // What `mv` itself wins outright -- whatever sat on the target before it (`0` for a
    // non-capture), plus a promotion's own net material swing (the promoted piece's value over
    // a pawn's), if any.
    let capture_value = if mv.kind() == MoveKind::EnPassantCapture {
        piece_value(PieceType::Pawn)
    } else {
        match board.piece_at(mv.to()) {
            Some(victim) => piece_value(victim.piece_type()),
            None => 0,
        }
    };
    let promotion_value = match mv.promotion() {
        Some(promotion) => piece_value(promotion) - piece_value(PieceType::Pawn),
        None => 0,
    };

    // `mv`'s own piece has left `mv.from()`, which can reveal an x-ray attacker behind it, and
    // an en passant capture additionally removes the captured pawn's own square -- distinct
    // from the target for exactly this move kind, the one case where the captured piece never
    // actually stood on the destination square at all.
    let mut occupancy = board.all_occupancy() ^ Bitboard::from_square(mv.from());
    if mv.kind() == MoveKind::EnPassantCapture {
        let captured_pawn_square = Square::new(mv.to().file(), mv.from().rank());
        occupancy = occupancy ^ Bitboard::from_square(captured_pawn_square);
    }

    // The piece now sitting on the target, whose value the *next* capture in the sequence
    // would win -- `mv`'s own moving piece, or the promoted piece if `mv` promotes (see `see`'s
    // doc comment on why a later recapture never gets the same treatment).
    let on_target = match mv.promotion() {
        Some(promotion) => promotion,
        None => board.piece_at(mv.from()).expect("see: no piece on the from square").piece_type(),
    };
    (capture_value + promotion_value, occupancy, on_target)
}

/// Whether `see(board, mv) >= threshold`, exactly, but walking the exchange forward and
/// stopping as soon as the answer is decided, often before the next attacker is even looked up.
///
/// `see` resolves `gain[i] = d[i] - max(0, gain[i + 1])` backward, `d[i]` the value captured at
/// ply `i` and a missing ply's `max(0, ...)` being 0. Forward, the question alternates:
/// `gain[0] >= threshold` holds when `d[0] - threshold >= 0` and the opponent's best
/// continuation `max(0, gain[1])` is at most `b = d[0] - threshold`. That in turn holds when
/// the opponent has no recapture, or `d[1] - b <= 0`, or else `max(0, gain[2]) >= d[1] - b`,
/// which fails with no recapture or `d[2] - b' < 0`; and so on, `b` becoming `d[i] - b` at each
/// ply. Each `d[i]` is known before its capturer is searched, so a decided bound exits early.
/// Not Stockfish's `see_ge`, whose king rule differs from `see`'s (the king is worth 0 here):
/// this one must agree with `see` exactly, so that pruning and ordering stay unchanged.
pub fn see_ge(board: &Board, mv: Move, threshold: i32) -> bool {
    let target = mv.to();
    let (first_gain, mut occupancy, mut on_target) = exchange_start(board, mv);
    let mut bound = first_gain - threshold;
    if bound < 0 {
        return false;
    }

    // `at_most`: the side about to capture must gain at most `bound` from here on; otherwise at
    // least `bound`, which is then positive.
    let mut at_most = true;
    let mut color = board.side_to_move().opposite();
    loop {
        let next = piece_value(on_target) - bound;
        if at_most && next <= 0 {
            return true;
        }
        if !at_most && next < 0 {
            return false;
        }
        let attackers = attackers_to(board, target, occupancy);
        let Some((square, piece_type)) = least_valuable_attacker(board, attackers, color) else {
            return at_most;
        };
        bound = next;
        at_most = !at_most;
        occupancy = occupancy ^ Bitboard::from_square(square);
        on_target = piece_type;
        color = color.opposite();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_board_has_no_attackers() {
        let (board, _) = Board::from_fen("8/8/8/8/8/8/8/8 w - - 0 1").unwrap();
        let attackers = attackers_to(&board, Square::D4, board.all_occupancy());
        assert_eq!(attackers, Bitboard::EMPTY);
    }

    #[test]
    fn finds_attackers_of_both_colors_at_once() {
        // White knight on b5 and black knight on b3 both attack d4; a white pawn on e2 does
        // not (too far away). `attackers_to` doesn't take a color -- both real attackers
        // should come back together.
        let (board, _) = Board::from_fen("8/8/8/1N6/8/1n6/4P3/8 w - - 0 1").unwrap();
        let attackers = attackers_to(&board, Square::D4, board.all_occupancy());
        assert_eq!(attackers, Bitboard::from_square(Square::B5) | Bitboard::from_square(Square::B3));
    }

    #[test]
    fn finds_pawn_attackers_respecting_direction() {
        // A white pawn on c3 attacks d4 (moves toward higher ranks); a black pawn on c5
        // attacks d4 too (moves toward lower ranks); a white pawn on c5 does not (it would be
        // moving the wrong way to ever have reached that attack).
        let (board, _) = Board::from_fen("8/8/8/2p5/3P4/2P5/8/8 w - - 0 1").unwrap();
        let attackers = attackers_to(&board, Square::D4, board.all_occupancy());
        assert_eq!(attackers, Bitboard::from_square(Square::C3) | Bitboard::from_square(Square::C5));
    }

    #[test]
    fn sliding_attacker_blocked_by_a_piece_between_it_and_the_target() {
        let (board, _) = Board::from_fen("8/8/8/8/8/3R4/8/3R4 w - - 0 1").unwrap();
        // The d3 rook attacks d4; the d1 rook is blocked by the d3 rook and does not.
        let attackers = attackers_to(&board, Square::D4, board.all_occupancy());
        assert_eq!(attackers, Bitboard::from_square(Square::D3));
    }

    #[test]
    fn x_ray_attacker_appears_once_the_blocker_is_removed_from_occupancy() {
        // Same position as above, but with the front rook's own square removed from the
        // occupancy passed in -- simulating the moment it has "moved" onto d4 during an
        // exchange. The back rook on d1 should now show up too.
        let (board, _) = Board::from_fen("8/8/8/8/8/3R4/8/3R4 w - - 0 1").unwrap();
        let occupancy = board.all_occupancy() ^ Bitboard::from_square(Square::D3);
        let attackers = attackers_to(&board, Square::D4, occupancy);
        assert_eq!(attackers, Bitboard::from_square(Square::D1));
    }

    #[test]
    fn a_removed_attacker_does_not_count_even_though_its_board_bitboard_is_unchanged() {
        // The board itself still has a knight on b5, but if `occupancy` no longer includes
        // b5 (simulating that piece already having been used earlier in an exchange), it
        // must not be reported as an attacker.
        let (board, _) = Board::from_fen("8/8/8/1N6/8/8/8/8 w - - 0 1").unwrap();
        let occupancy = board.all_occupancy() ^ Bitboard::from_square(Square::B5);
        let attackers = attackers_to(&board, Square::D4, occupancy);
        assert_eq!(attackers, Bitboard::EMPTY);
    }

    #[test]
    fn least_valuable_attacker_returns_none_when_that_color_has_no_attacker() {
        let (board, _) = Board::from_fen("8/8/8/1n6/8/8/8/8 w - - 0 1").unwrap();
        let attackers = attackers_to(&board, Square::D4, board.all_occupancy());
        assert_eq!(least_valuable_attacker(&board, attackers, Color::White), None);
    }

    #[test]
    fn least_valuable_attacker_picks_the_pawn_over_the_rook() {
        // Both a white pawn (c3) and a white rook (d1) attack d4; the pawn is cheaper and
        // must be the one returned, even though the rook also attacks and is generated later
        // in `PieceType::ALL`'s own order.
        let (board, _) = Board::from_fen("8/8/8/8/8/2P5/8/3R4 w - - 0 1").unwrap();
        let attackers = attackers_to(&board, Square::D4, board.all_occupancy());
        assert_eq!(least_valuable_attacker(&board, attackers, Color::White), Some((Square::C3, PieceType::Pawn)));
    }

    #[test]
    fn least_valuable_attacker_only_considers_the_requested_color() {
        // A white knight on b5 and a black knight on b3 both attack d4 -- asking for Black
        // must return the black knight, not the white one, even though both are the same
        // piece type and both are present in `attackers_to`'s combined result.
        let (board, _) = Board::from_fen("8/8/8/1N6/8/1n6/8/8 w - - 0 1").unwrap();
        let attackers = attackers_to(&board, Square::D4, board.all_occupancy());
        assert_eq!(least_valuable_attacker(&board, attackers, Color::Black), Some((Square::B3, PieceType::Knight)));
        assert_eq!(least_valuable_attacker(&board, attackers, Color::White), Some((Square::B5, PieceType::Knight)));
    }

    #[test]
    fn see_of_a_quiet_move_is_zero() {
        let (board, _) = Board::from_fen("8/8/8/8/8/8/4P3/8 w - - 0 1").unwrap();
        assert_eq!(see(&board, Move::new(Square::E2, Square::E3, None)), 0);
    }

    #[test]
    fn see_of_an_undefended_capture_wins_the_full_victim() {
        let (board, _) = Board::from_fen("8/8/8/8/8/3n4/4P3/8 w - - 0 1").unwrap();
        assert_eq!(see(&board, Move::new(Square::E2, Square::D3, None)), piece_value(PieceType::Knight));
    }

    #[test]
    fn see_of_a_defended_capture_loses_the_attacker_minus_the_victim() {
        // White queen captures a black pawn defended by a second black pawn -- the queen is
        // recaptured next with nothing further, so this must come out strictly negative, not
        // just "less good than it looked": a naive move-orderer would rank this capture highly
        // (it wins a pawn) without ever noticing the queen is lost right back.
        let (board, _) = Board::from_fen("8/8/2p5/3p4/8/8/8/3Q4 w - - 0 1").unwrap();
        let see_value = see(&board, Move::new(Square::D1, Square::D5, None));
        assert_eq!(see_value, piece_value(PieceType::Pawn) - piece_value(PieceType::Queen));
        assert!(see_value < 0);
    }

    #[test]
    fn see_correctly_resolves_a_bad_recapture_as_declined() {
        // White knight (b4) takes a black knight (d5); a black bishop (b3) could recapture,
        // but a white rook (d1) would then take the bishop -- so recapturing is actually bad
        // for Black once White's own follow-up is accounted for, and Black should decline.
        // A naive "sum every capture in the list" would compute 320 - 320 + 330 = 330 (win a
        // knight and a bishop for a knight); the real, correct result is smaller: a plain
        // knight-for-knight trade, +320, with the exchange stopping there because Black never
        // actually plays the losing recapture. This is exactly the case that caught this
        // function's own resolution bug during development -- see `search.md`'s design
        // section for the full hand trace.
        let (board, _) = Board::from_fen("8/8/8/3n4/1N6/1b6/8/3R4 w - - 0 1").unwrap();
        assert_eq!(see(&board, Move::new(Square::B4, Square::D5, None)), piece_value(PieceType::Knight));
    }

    #[test]
    fn see_of_an_en_passant_capture_uses_the_captured_pawns_own_square() {
        // The captured pawn sits on d5, not on the move's own destination (d6) -- `see` must
        // still find and value it, and must remove d5 (not d6) from occupancy afterward.
        let (board, _) = Board::from_fen("8/8/8/3pP3/8/8/8/8 w - - 0 1").unwrap();
        assert_eq!(see(&board, Move::en_passant_capture(Square::E5, Square::D6)), piece_value(PieceType::Pawn));
    }

    #[test]
    fn see_of_a_capturing_promotion_adds_the_promoted_pieces_net_gain() {
        // A pawn capturing a rook while promoting to a queen wins the rook outright, plus the
        // net material swing of the pawn itself becoming a queen (900 - 100), not just the
        // promoted piece's raw value on its own.
        let (board, _) = Board::from_fen("4r3/3P4/8/8/8/8/8/8 w - - 0 1").unwrap();
        let mv = Move::new(Square::D7, Square::E8, Some(PieceType::Queen));
        let expected = piece_value(PieceType::Rook) + (piece_value(PieceType::Queen) - piece_value(PieceType::Pawn));
        assert_eq!(see(&board, mv), expected);
    }

    #[test]
    fn see_ge_reports_a_winning_capture_as_at_least_zero() {
        let (board, _) = Board::from_fen("8/8/8/8/8/3n4/4P3/8 w - - 0 1").unwrap();
        assert!(see_ge(&board, Move::new(Square::E2, Square::D3, None), 0));
    }

    #[test]
    fn see_ge_reports_a_losing_capture_as_not_at_least_zero() {
        let (board, _) = Board::from_fen("8/8/2p5/3p4/8/8/8/3Q4 w - - 0 1").unwrap();
        assert!(!see_ge(&board, Move::new(Square::D1, Square::D5, None), 0));
    }

    #[test]
    fn see_ge_respects_a_nonzero_threshold() {
        // The knight-for-knight trade nets exactly +320 (see the dedicated `see` test above)
        // -- at or below that threshold it's a pass, one point above it is not.
        let (board, _) = Board::from_fen("8/8/8/3n4/1N6/1b6/8/3R4 w - - 0 1").unwrap();
        let mv = Move::new(Square::B4, Square::D5, None);
        assert!(see_ge(&board, mv, piece_value(PieceType::Knight)));
        assert!(!see_ge(&board, mv, piece_value(PieceType::Knight) + 1));
    }

    #[test]
    fn see_ge_equals_comparing_see_for_every_move_and_threshold() {
        // Every pseudo-legal move along random games, against thresholds around its exact value
        // (where ties decide) and across the range the search asks (the move picker's
        // `-score / 18`, quiescence's margin, `MARGIN_PER_PLY` times the depth).
        use crate::movegen::legal::legal_moves;
        use crate::movegen::pseudo_legal_moves;

        let starts = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        ];
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        for fen in starts {
            for game in 0..30 {
                let (mut board, _) = Board::from_fen(fen).unwrap();
                for ply in 0..100 {
                    let mut moves = Vec::new();
                    pseudo_legal_moves(&board, board.side_to_move(), &mut moves);
                    for &mv in &moves {
                        let value = see(&board, mv);
                        let thresholds = [value - 1, value, value + 1, -2000, -900, -320, -100, -1, 0, 1, 100, 320, 900];
                        for threshold in thresholds {
                            assert_eq!(
                                see_ge(&board, mv, threshold),
                                value >= threshold,
                                "{fen}, game {game}, ply {ply}, move {} to {}, see {value}, threshold {threshold}",
                                mv.from() as usize,
                                mv.to() as usize
                            );
                        }
                    }
                    let legal = legal_moves(&board, board.side_to_move());
                    if legal.is_empty() {
                        break;
                    }
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    board.make_move(legal[(seed >> 33) as usize % legal.len()]);
                }
            }
        }
    }
}
