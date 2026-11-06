//! How long one network evaluation actually costs, against the PeSTO evaluation it would
//! replace.
//!
//! In-process and single-threaded, unlike `search_bench.rs`, which drives a real UCI engine: the
//! question here is the cost of one `evaluate` call, not what a whole search does with it, and
//! putting a process boundary and a search around a microsecond-scale measurement would bury it.
//!
//! For a psq network, four timings, because which one dominates decides what is worth optimizing
//! next and the answer is not obvious in advance:
//!
//! 1. **The PeSTO evaluation**, as the baseline being given up.
//! 2. **The from-scratch evaluation**: both accumulators built from every active row, then the
//!    head.
//! 3. **One incremental update** of both accumulators across a move.
//! 4. **The head alone**, from already-built accumulators.
//!
//! (3) plus (4) is what a search node costs, since the search updates on every legal move and
//! evaluates from the updated accumulators.
//!
//! With `--depth N` it also times a real `choose_best_move` to that depth with each evaluation, and
//! that is the number a gate's wall clock actually depends on. The per-evaluation ratio badly
//! overstates it: evaluation is only part of what a node costs, and movegen, move ordering,
//! make/unmake and the transposition probe are identical either way. Node counts are reported
//! alongside the times because they are not expected to match -- a different evaluation prunes
//! differently at the same depth, which is part of what "a better evaluation" means.
//!
//! Usage: `cargo bench --bench nnue_bench`, optionally with a real network to time instead of a
//! randomly generated one, and optionally with a search depth:
//!
//! `cargo bench --bench nnue_bench -- data/chess/networks/pesto-psq/run-1/gen-1/lambda-0.95/candidate.nnue --depth 9`
//!
//! A relative path is read from the repository root. A forge run's player can also be named, from
//! this machine's runs (`players::resolve_network_file`):
//!
//! `cargo bench --bench nnue_bench -- --network pesto-psq-g1-r1 --depth 9`
//!
//! A `.nnue` is recognized by its own feature-set tag. Without one, the random network is a psq
//! one (`--psq`, the default), whose shape defaults to self-psq's, (780 -> 256) x 2 -> 1,
//! and can be changed with `--width N` (the accumulator, per perspective) and `--head a,b,...` (the
//! hidden layers; `--head ""` for none, the output reading the accumulators directly). Both are
//! ignored when a `.nnue` is given, whose own shape wins:
//!
//! `cargo bench --bench nnue_bench -- --psq --width 256 --head "" --depth 9`
//!
//! `--threats` makes the random network a psq_threats one (emily's, with `--head 16`), whose head
//! timing then includes recomputing the threat accumulators:
//!
//! `cargo bench --bench nnue_bench -- --threats --width 256 --head 16 --depth 9`
//!
//! `--pairwise` makes it a psq_pairwise one (greta's, with `--head ""`), each perspective's halves
//! multiplied before the head:
//!
//! `cargo bench --bench nnue_bench -- --pairwise --width 256 --head "" --depth 9`
//!
//! Both together make it a psq_threats_pairwise one (the experiments', with `--head ""`):
//!
//! `cargo bench --bench nnue_bench -- --threats --pairwise --width 256 --head "" --depth 9`
//!
//! `--beside` with `--threats --pairwise` and one hidden width makes it a psq_threats_pairwise_hidden
//! one: the hidden layer beside the output, which reads its values clipped and squared and every
//! product. Without `--beside`, the same `--head "16"` is the plain hidden layer, the output reading
//! it alone:
//!
//! `cargo bench --bench nnue_bench -- --threats --pairwise --beside --width 256 --head 16 --depth 9`
//!
//! `--screlu` with `--threats` makes it a psq_threats_screlu one (screlu.md's, its head always none),
//! each lane's clipped value squared before the output. Against `--threats --head ""` and
//! `--threats --pairwise --head ""`, what the squares cost:
//!
//! `cargo bench --bench nnue_bench -- --threats --screlu --width 256 --head "" --depth 9`
//!
//! `--halfk` makes it a halfk_threats_screlu one: psq_threats_screlu with
//! halfk's 22,528 piece rows in place of psq's, a king changing bucket refreshing its perspective
//! through the refresh cache. Against the line above, what halfk costs; `--depth` matters most here,
//! since only a search moves the kings:
//!
//! `cargo bench --bench nnue_bench -- --halfk --width 256 --depth 9`
//!
//! `--hidden` with `--halfk` makes it a halfk_threats_screlu_hidden one: two hidden layers in place of
//! the output, 16 and 32 unless `--head` says otherwise, the output reading the second alone; it also
//! prints the share of zero squares, what skipping them in the first layer could save. `--int8` runs
//! that first layer in 8 bits instead of 16 (`HiddenArithmetic`), for a random network or a loaded
//! one, and with `--quantization` measures its distance from f32 the same way:
//!
//! `cargo bench --bench nnue_bench -- --halfk --hidden --width 256 --depth 9`
//!
//! `cargo bench --bench nnue_bench -- --halfk --hidden --int8 --width 256 --depth 9`
//!
//! `--pairwise` with `--halfk --hidden` makes it a halfk_threats_pairwise_hidden one: each
//! perspective's halves multiplied in place of the squares, the first layer reading half as many:
//!
//! `cargo bench --bench nnue_bench -- --halfk --hidden --pairwise --width 256 --depth 9`
//!
//! `--stack` makes it a psq_stack one (hilda's, with `--head "16,16"`), the head's layers bucketed by
//! our king, their king and the material:
//!
//! `cargo bench --bench nnue_bench -- --stack --width 256 --head "16,16" --depth 9`
//!
//! With `--threats` too, a psq_threats_stack one (the experiments'):
//!
//! `cargo bench --bench nnue_bench -- --stack --threats --width 256 --head "16,16" --depth 9`
//!
//! With `--threats --screlu` instead, a psq_threats_screlu_kings one: each side's squares into a
//! layer picked by its own king, `--head` its one width (16 by default), then one output over those
//! values and every square. Against `--threats --screlu --head ""`, what the king layers cost:
//!
//! `cargo bench --bench nnue_bench -- --stack --threats --screlu --width 256 --depth 9`
//!
//! `--material` makes it a psq_threats_pairwise_material one (experiment 3b's, its head always
//! none): psq_threats_pairwise with its output one of 78 sets by the material. Against
//! `--threats --pairwise`, what the buckets cost:
//!
//! `cargo bench --bench nnue_bench -- --material --width 256 --depth 9`
//!
//! `--material --screlu` makes it a psq_threats_screlu_material one (psq-threats-screlu-material.md's):
//! psq_threats_screlu with its output one of 78 sets by the material. Against `--threats --screlu
//! --head ""`, what the buckets cost:
//!
//! `cargo bench --bench nnue_bench -- --material --screlu --width 256 --depth 9`
//!
//! `--phase` with `--material --screlu` makes it a psq_threats_screlu_phase one: the same, its
//! output one of 25 sets by the game phase instead:
//!
//! `cargo bench --bench nnue_bench -- --material --screlu --phase --width 256 --depth 9`
//!
//! `--count` with `--material --screlu` makes it a psq_threats_screlu_count one: the same, its
//! output one of 8 sets by the piece count (Stockfish's key) instead:
//!
//! `cargo bench --bench nnue_bench -- --material --screlu --count --width 256 --depth 9`
//!
//! `--taper` with `--material --screlu` makes it a psq_threats_screlu_taper one: two sets, a
//! middlegame and an endgame one, both read at every position and blended by the game phase:
//!
//! `cargo bench --bench nnue_bench -- --material --screlu --taper --width 256 --depth 9`
//!
//! `--one-bucket` times the head with the first position's buckets for every position, so a stack
//! reads one weight set throughout: the head without bucket cache misses.
//!
//! `--threat-parts [dir]` splits a psq_threats network's evaluation (threats-timing, the developer,
//! 2026-10-29): the whole evaluation from built piece-square accumulators; the same head without
//! threats (the network's own head over its psq rows alone); the threats' finding (`push_threat_rows`); the copy and add, what is left; the active threat rows per
//! perspective; and evaluation's share of a search to `--depth` (9 by default), its nodes times the
//! whole evaluation's cost over its time, an upper bound, as not every node is evaluated. On the
//! positions of the games under `dir`, or the seven above:
//!
//! `cargo bench --bench nnue_bench -- --network <player> --threat-parts data/chess/matches/<a test>`
//!
//! `--search-only` skips every timing above and only searches Stockfish's 49 bench positions with the
//! network, at `--depth` (9 by default): the run to profile.
//!
//! `cargo bench --bench nnue_bench -- --search-only`
//!
//! With `--pesto` as well, that search uses the PeSTO evaluation instead of a network,
//! the baseline for the same 49 positions:
//!
//! `cargo bench --bench nnue_bench -- --search-only --pesto`
//!
//! `--threads N` runs N of those searches at once, as the replay runs one game per logical core,
//! and reports the machine's total rate; `--syzygy` loads the four Syzygy sets from the data root,
//! as the replay does, instead of searching without tables:
//!
//! `cargo bench --bench nnue_bench -- --network pesto-psq-g1-r1 --search-only --threads 20 --syzygy`
//!
//! `--games <dir or .pgn> [--sample N]` searches N whole games (40 by default) of those under `dir`,
//! or in one PGN file such as a replayed chunk, instead
//! of the 49, as the replay searches them: every position in order with the game's history, one
//! 16 MB table per game. The same games for every evaluator:
//!
//! `cargo bench --bench nnue_bench -- --search-only --pesto --games data/chess/networks/pesto-psq/run-1/gen-1`
//!
//! Under the `node-stats` feature, a single-threaded `--search-only` also counts the accumulator
//! updates, how many an evaluation ever reads, and the evaluations of positions the TT holds:
//!
//! `cargo bench --bench nnue_bench --features node-stats -- --network pesto-psq-g1-r1 --search-only`
//!
//! `--eval-scale <dir>` compares the network's static evaluation with the PeSTO one over every
//! position (not in check) of the games under `dir`, such as a generation's round robin:
//! percentiles, how often each clears 1 to 6 pawns, and the slope of one on the other. Whether the
//! search's margins, set in the PeSTO pawn, fit the network:
//!
//! `cargo bench --bench nnue_bench -- --network pesto-psq-g1-r1 --eval-scale data/chess/networks/pesto-psq/run-1/gen-1`
//!
//! `--residuals [dir] [--max-depth D] [--sample N]` measures how far `d` plies of a plain search
//! (no pruning, no TT) move the evaluator's static evaluation, for `d` = 0 to `D` (4 by default),
//! over `N` positions (10,000 by default) of the reference games in `data/chess/lumbra/` (copied down
//! from Drive when missing), or of the games under `dir`: the quantiles of the tails each pruning
//! bets on, beside today's margin. With `--pesto`, the PeSTO evaluation instead of the
//! network:
//!
//! `cargo bench --bench nnue_bench -- --network pesto-psq-g1-r1 --residuals`
//!
//! Each depth is also printed split into quiet and noisy positions (`is_noisy`: a winning capture or
//! a promotion to play), the same split for every evaluator: whether a network trained on quiet
//! positions alone errs more where a capture is pending, which the search still evaluates.
//!
//! `--reference-games N` reads the reference of N games instead of the first one's 5,000, and
//! `--per-game K` keeps only K positions of each game, for a reference too large to hold whole. Each
//! tail is printed in centipawns and in thousandths of label space (`labels::LOGISTIC`).
//!
//! `--cut-reference <big.pgn> [--reference-games N]` writes a reference once: N games (5,000 by
//! default) of 40 plies or more, evenly spaced over the source, compressed into `data/chess/lumbra/`, then
//! copied up to Drive:
//!
//! `cargo bench --bench nnue_bench -- --cut-reference "/g/My Drive/kalo-games/data/chess/lumbra/LumbrasGigaBase_OTB_ELO2400.pgn.zst"`
//!
//! The source, a PGN or a `.pgn.zst`, is read this once; nothing reads it again.
//!
//! `--quantization <dir>` quantizes the network as the engine would at load (`QuantizedPsqNetwork`)
//! and compares its evaluation with the f32 one over the games under `dir`, such as a duel against
//! PeSTO: the accumulator's scale and headroom, and the differences in centipawns.
//!
//! `cargo bench --bench nnue_bench -- --network pesto-psq-g1-r1 --quantization data/chess/matches/pesto-vs-self-psq-0001`
//!
//! `--weights` prints how the first hidden layer's weights sit against `127 / 64`, the range an `i8`
//! layer at a fixed scale of 64 holds: what clipping in training would cut.
//!
//! `cargo bench --bench nnue_bench -- --network pesto-psq-g1-r1 --weights`
//!
//! `--i8 <dir>` asks whether the first hidden layer is worth running in `i8`: its precision with
//! `i8` weights against today's `i16` (both per-row scales, both against f32, over the positions of
//! the games under `dir`, as `--quantization`), and the first layer's time with `i16` `madd`, `i8`
//! `maddubs` (AVX2) and `i8` `dpbusd` (AVX-VNNI, when the CPU has it). The kernels are this bench's
//! own copies: the engine is not changed by it. A network with exactly one hidden layer only.
//!
//! `cargo bench --bench nnue_bench -- --network pesto-psq-g1-r1 --i8 data/chess/matches/pesto-vs-self-psq-0001`
//!
//! Weight *values* should not affect the per-evaluation timing, but "should not" is not "do not" --
//! a ReLU is a data-dependent branch and denormals are real -- so timing a genuine trained network
//! is worth the one extra argument when a question turns on the number. For the search comparison
//! the weights matter outright, since they decide what gets pruned.

use std::hint::black_box;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use kalo_chess::board::chess_move::{Move, MoveKind};
use kalo_chess::board::piece_type::PieceType;
use kalo_chess::board::Board;
use kalo_chess::eval::evaluate;
use kalo_chess::movegen::attacks::is_in_check;
use kalo_chess::movegen::noisy_moves;
use kalo_chess::search::MATE_SCORE;
use kalo_chess::features::{halfk, psq, psq_threats};
use kalo_chess::movegen::legal::legal_moves;
use kalo_chess::board::color::Color;
use kalo_chess::nnue::{HiddenArithmetic, Layer, Network, PsqNetwork, QuantizedPsqNetwork, StackLayers};
use kalo_chess::stack_buckets::{COUNT_BUCKETS, MATERIAL_BUCKETS, OUR_KING_BUCKETS, PHASE_BUCKETS, TAPER_SETS, THEIR_KING_BUCKETS};
use kalo_chess::search::choose_best_move;
use kalo_chess::tablebase::SyzygyTablebase;
use kalo_chess::tt::TranspositionTable;

/// The forge's psq shape (the archived `pesto-psq` config's), the default when
/// `--width`/`--head` are not given, so the measurement with no arguments describes the network
/// actually deployed rather than a convenient small one.
const DEFAULT_EMBEDDING_DIM: usize = 256;
const DEFAULT_HIDDEN_DIMS: [usize; 0] = [];

/// Enough repetitions that one position's measurement is not dominated by timer resolution or by a
/// single scheduling hiccup, while keeping the whole run to a few seconds.
const REPETITIONS: usize = 2000;

/// A spread of real positions rather than one: the cost varies with piece count (an endgame gathers
/// far fewer transform rows than the opening does), and quoting only the starting position would
/// overstate the average.
const POSITIONS: &[&str] = &[
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
    "4rrk1/pp1n3p/3q2pQ/2p1pb2/2PP4/2P3N1/P2B2PP/4RRK1 b - - 7 19",
    "r1bq1rk1/ppp1nppp/4n3/3p3Q/3P4/1BP1B3/PP1N2PP/R4RK1 w - - 1 16",
    "6k1/6p1/6Pp/ppp5/3pn2P/1P3K2/1PP2P2/3N4 b - - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 11",
    "8/8/8/4k3/8/4K3/8/8 w - - 0 1",
];

/// A deterministic xorshift, so a rerun measures the same weights. Not `rand`: the values are
/// arbitrary and only their distribution matters, so a dependency would buy nothing.
struct Xorshift(u64);

impl Xorshift {
    fn next_weight(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        // Roughly [-0.5, 0.5], the scale a trained transform's own weights sit near.
        (self.0 >> 40) as f32 / 16_777_216.0 - 0.5
    }

    fn weights(&mut self, count: usize) -> Vec<f32> {
        (0..count).map(|_| self.next_weight()).collect()
    }
}

/// A random psq network: `psq::ROWS` transform rows, and a head reading both perspectives. With
/// `threats`, a psq_threats one: `psq_threats::ROWS` rows, the threats recomputed at evaluation.
/// With `pairwise`, each perspective's halves multiplied, the head reading `embedding_dim` products.
/// With `screlu`, each lane's clipped value squared, the head reading both perspectives' squares;
/// with one hidden width too, a psq_threats_screlu_hidden one, the output reading the hidden values
/// clipped and squared and every square. With `beside`, a pairwise network's one hidden layer is
/// beside its output the same way, over the products: a psq_threats_pairwise_hidden one.
fn random_psq_network(embedding_dim: usize, hidden_dims: &[usize], threats: bool, pairwise: bool, screlu: bool, beside: bool) -> PsqNetwork {
    let mut rng = Xorshift(0x9E3779B97F4A7C15);
    let rows = if threats { psq_threats::ROWS } else { psq::ROWS };
    let transform = rng.weights(rows * embedding_dim);
    let transform_bias = rng.weights(embedding_dim);

    let mut hidden = Vec::new();
    let head_input = if pairwise { embedding_dim } else { 2 * embedding_dim };
    let mut in_dim = head_input;
    for &out_dim in hidden_dims {
        hidden.push(Layer { in_dim, out_dim, weight: rng.weights(out_dim * in_dim), bias: rng.weights(out_dim) });
        in_dim = out_dim;
    }
    if (screlu || beside) && !hidden.is_empty() {
        in_dim = 2 * in_dim + head_input;
    }
    let output = Layer { in_dim, out_dim: 1, weight: rng.weights(in_dim), bias: rng.weights(1) };

    PsqNetwork {
        embedding_dim, transform, transform_bias, hidden, output, step: 0, learning_rate: 0.0, threats, pairwise,
        screlu, stack: None, material: None, halfk: false,
    }
}

/// A random psq_stack network (hilda's shape with `--head "16,16"`): a pairwise network whose head
/// is one weight set per bucket of each layer, `hidden_dims` its two hidden widths. With `threats`,
/// a psq_threats_stack one; with `screlu` too, a psq_threats_screlu_kings one, each side's squares
/// into a layer of `hidden_dims`' one width picked by its own king, and one output over those values
/// and every square.
fn random_stack_network(embedding_dim: usize, hidden_dims: &[usize], threats: bool, screlu: bool) -> PsqNetwork {
    assert!(!screlu || threats, "--stack --screlu is psq_threats': --stack --threats --screlu");
    match screlu {
        true => assert_eq!(hidden_dims.len(), 1, "--stack --screlu takes one hidden width, a side's, --head \"16\""),
        false => assert_eq!(hidden_dims.len(), 2, "--stack takes two hidden widths, --head \"16,16\""),
    }
    let mut network = random_psq_network(embedding_dim, &[], threats, !screlu, screlu, false);
    let mut rng = Xorshift(0xC2B2AE3D27D4EB4F);
    let mut sets = |buckets: usize, in_dim: usize, out_dim: usize| -> Vec<Layer> {
        (0..buckets).map(|_| Layer { in_dim, out_dim, weight: rng.weights(out_dim * in_dim), bias: rng.weights(out_dim) }).collect()
    };
    let (l1, l2, output) = if screlu {
        let width = hidden_dims[0];
        let l1 = sets(OUR_KING_BUCKETS, embedding_dim, width);
        let l2 = sets(THEIR_KING_BUCKETS, embedding_dim, width);
        (l1, l2, sets(1, 4 * width + 2 * embedding_dim, 1))
    } else {
        let l1 = sets(OUR_KING_BUCKETS, embedding_dim, hidden_dims[0]);
        let l2 = sets(THEIR_KING_BUCKETS, hidden_dims[0], hidden_dims[1]);
        (l1, l2, sets(MATERIAL_BUCKETS, hidden_dims[1], 1))
    };
    network.output = Layer::default();
    network.stack = Some(StackLayers { l1, l2, output });
    network
}

/// A random psq_threats_pairwise_material network (experiment 3b's shape with `--head ""`): a
/// psq_threats_pairwise network whose output is one weight set per material bucket. With `screlu`,
/// a psq_threats_screlu_material one: psq_threats_screlu's, its sets reading the squares; with
/// `phase` too, a psq_threats_screlu_phase one, one set per phase bucket; with `count`, a
/// psq_threats_screlu_count one, one set per piece-count bucket; with `taper`, a
/// psq_threats_screlu_taper one, a middlegame and an endgame set blended by the phase.
fn random_material_network(embedding_dim: usize, screlu: bool, phase: bool, count: bool, taper: bool) -> PsqNetwork {
    assert!(!phase || screlu, "--phase is screlu's only: --material --screlu --phase");
    assert!(!count || screlu, "--count is screlu's only: --material --screlu --count");
    assert!(!taper || screlu, "--taper is screlu's only: --material --screlu --taper");
    assert!([phase, count, taper].iter().filter(|&&key| key).count() <= 1, "--phase, --count and --taper are three keys: one at a time");
    let mut network = random_psq_network(embedding_dim, &[], true, !screlu, screlu, false);
    let in_dim = if screlu { 2 * embedding_dim } else { embedding_dim };
    let mut rng = Xorshift(0x165667B19E3779F9);
    let buckets = if phase {
        PHASE_BUCKETS
    } else if count {
        COUNT_BUCKETS
    } else if taper {
        TAPER_SETS
    } else {
        MATERIAL_BUCKETS
    };
    let sets = (0..buckets).map(|_| Layer { in_dim, out_dim: 1, weight: rng.weights(in_dim), bias: rng.weights(1) }).collect();
    network.output = Layer::default();
    network.material = Some(sets);
    network
}

/// `path` resolved from the repository root when relative: cargo runs a bench from its crate's
/// folder, not from where the command was typed.
fn from_repository(path: &str) -> String {
    if std::path::Path::new(path).is_absolute() {
        return path.to_string();
    }
    format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))
}

/// The network a forge run's player plays with, e.g. `pesto-psq-g2-r1`, from this machine's runs
/// (`players::resolve_network_file`), read from the repository root; from the run's copy on the
/// remote when this machine's has no such network, a run trained on another machine (its local
/// folder may exist without the network, holding only what a duel or a resume left there).
fn player_path(name: &str) -> String {
    use kalo_chess::pipeline::players::{resolve_network_file_in, run_dir};
    let local = |run: &str| -> Result<String, String> { Ok(from_repository(&run_dir(run)?)) };
    let remote = |run: &str| -> Result<String, String> { Ok(kalo_chess::pipeline::remote::remote_path(&run_dir(run)?)) };
    resolve_network_file_in(name, &local)
        .or_else(|on_local| resolve_network_file_in(name, &remote).map_err(|on_remote| format!("{on_local}; nor on the remote: {on_remote}")))
        .unwrap_or_else(|message| panic!("{message}"))
}

fn parameter_count(transform: &[f32], transform_bias: &[f32], hidden: &[Layer], output: &Layer) -> usize {
    let mut total = transform.len() + transform_bias.len();
    for layer in hidden {
        total += layer.weight.len() + layer.bias.len();
    }
    total + output.weight.len() + output.bias.len()
}

/// Nanoseconds per call of `call(index)` for `index` in `0..count`, averaged the same way as
/// `time_per_call`. For measurements whose inputs are prepared per position beforehand.
fn time_per_item(count: usize, mut call: impl FnMut(usize)) -> f64 {
    for index in 0..count {
        call(index);
    }
    let started = Instant::now();
    for _ in 0..REPETITIONS {
        for index in 0..count {
            call(index);
        }
    }
    started.elapsed().as_secs_f64() * 1e9 / (REPETITIONS * count) as f64
}

/// Nanoseconds per call, averaged over every position and repetition.
fn time_per_call(boards: &[Board], mut call: impl FnMut(&Board)) -> f64 {
    // One untimed pass, so the first measurement does not also pay for cold caches and the first
    // touch of every weight page.
    for board in boards {
        call(board);
    }

    let started = Instant::now();
    for _ in 0..REPETITIONS {
        for board in boards {
            call(board);
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    elapsed * 1e9 / (REPETITIONS * boards.len()) as f64
}

/// Cleared before every search, so neither evaluation inherits entries the other one stored, and one
/// position's result never carries into the next.
const TT_SIZE_MB: usize = 64;

/// The replay's and self-play's table, one per game, cleared between games (`self_play::TT_SIZE_MB`,
/// which a bench cannot import): `--search-only` searches with it, as they do.
const REPLAY_TT_SIZE_MB: usize = 16;

/// Times one `choose_best_move` to `depth` in `tt`, `ancestors` the hashes of the game's positions up
/// to and including `board`'s (empty for a lone position), returning seconds and the nodes it took.
fn time_search(
    board: &Board, depth: u32, evaluator: Option<&Network>, tablebase: &SyzygyTablebase, tt: &TranspositionTable, ancestors: &[u64],
) -> (f64, u64) {
    let stop = AtomicBool::new(false);

    let started = Instant::now();
    let (reports, _) = choose_best_move(board, depth, None, ancestors, tt, tablebase, evaluator, 0, &stop);
    let elapsed = started.elapsed().as_secs_f64();

    (elapsed, reports.last().map(|report| report.nodes).unwrap_or(0))
}


/// The search half: the same fixed depth with each evaluation, on a few positions.
///
/// Deliberately a handful rather than the whole list -- one deep search with the network takes
/// seconds, and the question is an order of magnitude, not a percentage.
///
/// Node counts are reported because they are *not* expected to match. A different evaluation makes
/// different pruning decisions at the same depth (null-move, futility and razoring all read a static
/// evaluation), so the two searches explore different trees. That is part of what a better
/// evaluation means, and it is also why the slowdown here is a property of the pair rather than a
/// clean per-node cost ratio.
fn compare_searches(positions: &[(&str, Board)], network: &Network, depth: u32, per_evaluation: f64) {
    println!("\nsearch to fixed depth {depth}:\n");
    println!("  {:>24}  {:>9}  {:>11}  {:>9}  {:>11}", "position", "hand (s)", "hand nodes", "net (s)", "net nodes");

    let mut hand_total = 0.0;
    let mut network_total = 0.0;
    let mut hand_nodes_total = 0u64;
    let mut network_nodes_total = 0u64;
    let tablebase = SyzygyTablebase::new();
    for (fen, board) in positions.iter().take(3) {
        let (hand_seconds, hand_nodes) = time_search(board, depth, None, &tablebase, &TranspositionTable::new(TT_SIZE_MB), &[]);
        let (network_seconds, network_nodes) =
            time_search(board, depth, Some(network), &tablebase, &TranspositionTable::new(TT_SIZE_MB), &[]);
        hand_total += hand_seconds;
        network_total += network_seconds;
        hand_nodes_total += hand_nodes;
        network_nodes_total += network_nodes;
        let short: String = fen.split_whitespace().next().unwrap_or(fen).chars().take(24).collect();
        println!("  {short:>24}  {hand_seconds:>9.3}  {hand_nodes:>11}  {network_seconds:>9.3}  {network_nodes:>11}");
    }

    let slowdown = network_total / hand_total;
    println!("\n  search is {slowdown:.1}x slower with the network, against {per_evaluation:.1}x per evaluation.");
    // Per node, because the two searches explore different trees: a random network's near-constant
    // output (clipped ReLU saturates on random weights) prunes far more than any real evaluation,
    // which makes the whole-search ratio above meaningless for random weights. Time per node is
    // what a trained network's search will cost, whatever its node count turns out to be.
    let hand_per_node = hand_total / hand_nodes_total as f64;
    let network_per_node = network_total / network_nodes_total as f64;
    println!(
        "  per node: {:.2} us PeSTO, {:.2} us with the network, {:.2}x.",
        hand_per_node * 1e6,
        network_per_node * 1e6,
        network_per_node / hand_per_node
    );
    println!("  The gap between those two is everything else a node does: movegen, ordering,");
    println!("  make/unmake and the transposition probe, none of which changed.");
}

/// `--search-only`: the network's search alone, at `depth` (9 unless `--depth` says otherwise), over
/// Stockfish's bench positions, with no evaluation timing loops and no PeSTO comparison, so
/// that a profile (`perf record`) samples the search and nothing else. Prints the total nodes and
/// nodes per second. `network` is `None` under `--pesto`, which searches with the PeSTO
/// evaluation instead, so both are measured over the same positions in the same way.
///
/// `threads` above 1 runs that many of these searches at once, each over all the positions with
/// its own transposition table, as the replay and self-play run one game per logical core: the
/// total nodes over the wall time is then the machine's rate, which a single search does not show
/// (shared caches and memory bandwidth). `tablebase` is empty unless `--syzygy` loaded the tables.
///
/// `games` are searched as `search_positions` says: each of the 49 bench positions alone, in a
/// fresh table of `TT_SIZE_MB` (so their node counts stay comparable with every earlier run); real
/// games as the replay searches them, every position in order with the game's history, in one table
/// of `REPLAY_TT_SIZE_MB` cleared between games.
fn search_only(network: Option<&Network>, depth: u32, threads: usize, tablebase: &SyzygyTablebase, games: &SearchGames) {
    // Returns the nodes and the seconds spent searching, clearing excluded. One table per thread,
    // cleared before each position (a fresh table's contents), never allocated per search: a fresh
    // allocation is handed over by Windows one zeroed page at a time, at the search's first touch of
    // each, so a small tree paid a page fault for nearly every probe (about 14% of amanda-0002's
    // profile, 2026-10-12, in `ntoskrnl`), a cost the engine never pays with its one table per game.
    let search_all = || -> (u64, f64) {
        let (mut nodes, mut seconds) = (0u64, 0f64);
        match games {
            SearchGames::Bench(boards) => {
                let mut tt = TranspositionTable::new(TT_SIZE_MB);
                for board in boards {
                    tt.clear();
                    let (elapsed, searched) = time_search(board, depth, network, tablebase, &tt, &[]);
                    nodes += searched;
                    seconds += elapsed;
                }
            }
            SearchGames::Games(games) => {
                let mut tt = TranspositionTable::new(REPLAY_TT_SIZE_MB);
                for game in games {
                    tt.clear();
                    let mut ancestors = Vec::with_capacity(game.len());
                    for board in game {
                        ancestors.push(board.hash());
                        let (elapsed, searched) = time_search(board, depth, network, tablebase, &tt, &ancestors);
                        nodes += searched;
                        seconds += elapsed;
                    }
                }
            }
        }
        (nodes, seconds)
    };
    let positions = match games {
        SearchGames::Bench(boards) => boards.len(),
        SearchGames::Games(games) => games.iter().map(Vec::len).sum(),
    };
    // One thread: the time spent searching. Several: the wall clock of the slowest, as a throughput.
    let started = Instant::now();
    let (total_nodes, total_seconds): (u64, f64) = if threads == 1 {
        search_all()
    } else {
        let nodes = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads).map(|_| scope.spawn(search_all)).collect();
            workers.into_iter().map(|worker| worker.join().expect("a search thread panicked").0).sum()
        });
        (nodes, started.elapsed().as_secs_f64())
    };
    println!(
        "search only, depth {depth}, {positions} positions x {threads} thread(s): {total_nodes} nodes in {total_seconds:.2} s, \
         {:.0} nodes/s, {:.0} positions/s",
        total_nodes as f64 / total_seconds,
        (positions * threads) as f64 / total_seconds
    );
    // The counters are per thread: with one search, all of them are this thread's.
    #[cfg(feature = "node-stats")]
    if threads == 1 {
        let stats = kalo_chess::node_stats::take();
        let share = |part: u64, whole: u64| -> f64 { return 100.0 * part as f64 / whole.max(1) as f64 };
        println!("\n  node-stats (timing above includes the counting):");
        println!(
            "    accumulator updates: {}, read by an evaluation {} ({:.1}%), never read {} ({:.1}%)",
            stats.updates,
            stats.needed_updates,
            share(stats.needed_updates, stats.updates),
            stats.updates - stats.needed_updates,
            share(stats.updates - stats.needed_updates, stats.updates)
        );
        println!(
            "    static evaluations: {} ({:.2} per node), of positions with a TT entry {} ({:.1}%)",
            stats.evaluations,
            stats.evaluations as f64 / total_nodes.max(1) as f64,
            stats.evaluations_with_tt_entry,
            share(stats.evaluations_with_tt_entry, stats.evaluations)
        );
        println!(
            "    quiescence nodes: {} ({:.1}% of nodes), interior nodes {}",
            stats.quiescence_nodes,
            share(stats.quiescence_nodes, total_nodes),
            total_nodes - stats.quiescence_nodes
        );
        // Where a move head tried right after the TT move would score the legal moves.
        println!(
            "    nodes ordered past the TT move: {} ({:.1}% of nodes, {:.1}% of interior nodes)",
            stats.ordered_nodes,
            share(stats.ordered_nodes, total_nodes),
            share(stats.ordered_nodes, total_nodes - stats.quiescence_nodes)
        );
        let cutoffs = stats.tt_cutoffs + stats.first_cutoffs + stats.later_cutoffs;
        println!(
            "    beta cutoffs: {cutoffs}; by the TT move {:.1}%, by the first move after it {:.1}%, by a later one {:.1}% \
             (on average the {:.1}th after the TT move)",
            share(stats.tt_cutoffs, cutoffs),
            share(stats.first_cutoffs, cutoffs),
            share(stats.later_cutoffs, cutoffs),
            stats.later_places as f64 / stats.later_cutoffs.max(1) as f64
        );
        // By the cutting move's kind: a hint tried after the good captures, as killers are, never
        // displaces a capture's cutoff, so the quiet cutoffs and the waste before them are its room.
        println!(
            "    of those after the TT move, by a quiet move: first {:.1}% of first-move cutoffs, later {:.1}% of later ones",
            share(stats.first_quiet_cutoffs, stats.first_cutoffs),
            share(stats.later_quiet_cutoffs, stats.later_cutoffs)
        );
        println!(
            "    nodes searched in vain before a later cutoff: {} ({:.1}% of nodes), what a perfect move hint would save there",
            stats.wasted_nodes,
            share(stats.wasted_nodes, total_nodes)
        );
        println!(
            "    of which before a quiet cutoff: {} ({:.1}% of nodes, {:.1}% of the waste)",
            stats.wasted_before_quiet,
            share(stats.wasted_before_quiet, total_nodes),
            share(stats.wasted_before_quiet, stats.wasted_nodes)
        );
        // A hint computed only at nodes with at least `d` plies left: the nodes that pay for it, and
        // the waste it could remove, both as shares of all nodes.
        println!("\n    a move hint only where at least d plies remain (shares of all nodes):");
        println!("    {:>4}  {:>14}  {:>16}", "d", "ordered nodes", "waste reachable");
        for d in 0..kalo_chess::node_stats::DEPTHS {
            let ordered: u64 = stats.ordered_by_depth[d..].iter().sum();
            let wasted: u64 = stats.wasted_by_depth[d..].iter().sum();
            let label = if d == kalo_chess::node_stats::DEPTHS - 1 { format!("{d}+") } else { d.to_string() };
            println!("    {label:>4}  {:>13.1}%  {:>15.1}%", share(ordered, total_nodes), share(wasted, total_nodes));
        }
    }
}

/// What `--search-only` searches (`search_only` says how).
enum SearchGames {
    /// Stockfish's 49 bench positions, each alone.
    Bench(Vec<Board>),
    /// Whole games, each its positions in order.
    Games(Vec<Vec<Board>>),
}

/// Stockfish's 49 bench positions, or with `--games <dir>`, `--sample` games (40 by default) spread
/// evenly over the games under `dir`, read as `--quantization` reads them: real games, searched as
/// the replay searches them, so that two evaluators are compared on the same work.
fn search_positions(arguments: &[String]) -> SearchGames {
    let Some(dir) = arguments.iter().position(|argument| argument == "--games").and_then(|at| arguments.get(at + 1)) else {
        return SearchGames::Bench(POSITIONS_FOR_SEARCH.iter().map(|line| board_from_line(line)).collect());
    };
    let sample = arguments
        .iter()
        .position(|argument| argument == "--sample")
        .and_then(|at| arguments.get(at + 1))
        .map(|value| value.parse::<usize>().expect("--sample takes a number"))
        .unwrap_or(40);
    let mut all = selection_games(dir);
    all.retain(|game| !game.is_empty());
    assert!(!all.is_empty(), "no games found under {dir}");
    let sample = sample.min(all.len());
    SearchGames::Games((0..sample).map(|index| all[index * all.len() / sample].clone()).collect())
}

/// `--syzygy`: the four sets the replay loads, from the data root, or an empty tablebase.
fn bench_tablebase(arguments: &[String]) -> SyzygyTablebase {
    if !arguments.iter().any(|argument| argument == "--syzygy") {
        return SyzygyTablebase::new();
    }
    let root = kalo_chess::pipeline::data_root::data_root();
    let directories: Vec<String> =
        ["3-4-5-wdl", "6-wdl", "3-4-5-dtz", "6-dtz"].iter().map(|set| format!("{root}/chess/syzygy/{set}")).collect();
    SyzygyTablebase::from_directories(&directories).unwrap_or_else(|message| panic!("{message}"))
}

#[path = "../../pgn-replay/src/lib.rs"]
mod pgn_replay;

/// One game of a selection PGN: its starting position and its moves.
#[derive(Default)]
struct ScaleGame {
    starting_fen: Option<String>,
    moves: Vec<pgn_reader::SanPlus>,
}

struct ScaleReader {
    game: ScaleGame,
}

impl pgn_reader::Visitor for ScaleReader {
    type Tags = ();
    type Movetext = ();
    type Output = ScaleGame;

    fn begin_tags(&mut self) -> std::ops::ControlFlow<Self::Output, Self::Tags> {
        self.game = ScaleGame::default();
        std::ops::ControlFlow::Continue(())
    }

    fn tag(
        &mut self, _tags: &mut Self::Tags, name: &[u8], value: pgn_reader::RawTag<'_>,
    ) -> std::ops::ControlFlow<Self::Output> {
        if name == b"FEN" {
            self.game.starting_fen = Some(String::from_utf8_lossy(value.as_bytes()).into_owned());
        }
        std::ops::ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, _tags: Self::Tags) -> std::ops::ControlFlow<Self::Output, Self::Movetext> {
        std::ops::ControlFlow::Continue(())
    }

    fn san(&mut self, _movetext: &mut Self::Movetext, san_plus: pgn_reader::SanPlus) -> std::ops::ControlFlow<Self::Output> {
        self.game.moves.push(san_plus);
        std::ops::ControlFlow::Continue(())
    }

    fn end_game(&mut self, _movetext: Self::Movetext) -> Self::Output {
        std::mem::take(&mut self.game)
    }
}

/// Every position of every game in every `games.pgn` one level below `dir`, side to move not in
/// check (the search reads no static evaluation in check).
fn selection_positions(dir: &str) -> Vec<Board> {
    selection_games(dir)
        .into_iter()
        .flatten()
        .filter(|board| !kalo_chess::movegen::attacks::is_in_check(board, board.side_to_move()))
        .collect()
}

/// Every game in every `games.pgn` one level below `dir`, each as its positions in order, every one
/// before a move (the final position is not included, as the replay does not search it). A game
/// whose move names no legal move stops there. `dir` may also be one PGN file, such as a chunk the
/// replay wrote: the games labelling itself searches.
fn selection_games(dir: &str) -> Vec<Vec<Board>> {
    if std::path::Path::new(dir).is_file() {
        let file = std::fs::File::open(dir).unwrap_or_else(|error| panic!("cannot open {dir}: {error}"));
        return pgn_games(file, dir);
    }
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read {dir}: {error}"))
        .filter_map(|entry| {
            let path = entry.ok()?.path().join("games.pgn");
            path.is_file().then_some(path)
        })
        .collect();
    paths.sort();
    let mut games = Vec::new();
    for path in paths {
        let file = std::fs::File::open(&path).unwrap_or_else(|error| panic!("cannot open {}: {error}", path.display()));
        games.extend(pgn_games(file, &path.display().to_string()));
    }
    games
}

/// Every game of the PGN `read` yields, as `selection_games` reads each file; `label` names it in a
/// panic.
fn pgn_games(read: impl std::io::Read, label: &str) -> Vec<Vec<Board>> {
    let mut games = Vec::new();
    for_each_game(read, label, |boards| games.push(boards));
    games
}

/// Calls `each` with every game of the PGN `read` yields, as its positions in order, one game at a
/// time, so that a caller keeping only a few positions per game never holds them all.
fn for_each_game(read: impl std::io::Read, label: &str, mut each: impl FnMut(Vec<Board>)) {
    let mut reader = pgn_reader::Reader::new(read);
    let mut visitor = ScaleReader { game: ScaleGame::default() };
    while let Some(game) = reader.read_game(&mut visitor).unwrap_or_else(|error| panic!("{label}: {error}")) {
        let fen = game.starting_fen.as_deref().unwrap_or(pgn_replay::STARTING_POSITION_FEN);
        let (mut board, _) = Board::from_fen(fen).unwrap_or_else(|problem| panic!("{fen}: {problem:?}"));
        let mut boards = Vec::with_capacity(game.moves.len());
        for san_plus in &game.moves {
            boards.push(board);
            let Some(mv) = pgn_replay::try_resolve_san(&board, &san_plus.san) else {
                break;
            };
            board.make_move(mv);
        }
        each(boards);
    }
}

/// The reference's zstd level, as the smithy compresses what it keeps.
const COMPRESSION_LEVEL: i32 = 19;

/// Lumbra's Gigabase under `data/`, mirrored on Google Drive like the rest of it
/// (`pipeline::remote`): the source, recompressed as `.pgn.zst`, and the reference cut from it.
/// Relative to the repository's root, not the working directory: `cargo bench` runs from the crate's
/// own directory, where a copy would land outside the data root (`from_repository`).
const REFERENCE_DIR: &str = "data/chess/lumbra";

/// `--residuals`' positions: human over-the-board games, held out from every training corpus (TCEC,
/// CCRL and self-play), so that no network is measured on positions like those it was trained on,
/// and less balanced than engine games, closer to the search's interior nodes. Each reference holds
/// `games` games, cut once from `LumbrasGigaBase_OTB_ELO2400.pgn` by `--cut-reference`, never
/// regenerated; several can sit side by side.
fn reference_file(games: usize) -> String {
    format!("LumbrasGigaBase_OTB_ELO2400-{games}.pgn.zst")
}

/// The games a reference holds unless `--games` says otherwise: the first one cut, read whole.
const REFERENCE_DEFAULT_GAMES: usize = 5000;

/// Shorter games are left out of the reference: mostly short draws, opening positions and nothing
/// else.
const REFERENCE_MIN_PLIES: u32 = 40;

/// The `PlyCount` of one game's raw text, if it has the tag.
fn ply_count(game: &[u8]) -> Option<u32> {
    let text = String::from_utf8_lossy(game);
    let line = text.lines().find(|line| line.starts_with("[PlyCount \""))?;
    line.trim_start_matches("[PlyCount \"").trim_end_matches("\"]").parse().ok()
}

/// Calls `each` with every game of the PGN at `path` (zstd-compressed when it ends in `.zst`) as its
/// raw bytes, tags and moves verbatim, split where an `[Event ` line starts the next one.
fn for_each_raw_game(path: &str, mut each: impl FnMut(&[u8])) {
    use std::io::BufRead;
    let file = std::fs::File::open(path).unwrap_or_else(|error| panic!("cannot open {path}: {error}"));
    let read: Box<dyn std::io::Read> = if path.ends_with(".zst") {
        Box::new(zstd::Decoder::new(file).unwrap_or_else(|error| panic!("{path}: {error}")))
    } else {
        Box::new(file)
    };
    let mut reader = std::io::BufReader::new(read);
    let mut game: Vec<u8> = Vec::new();
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).unwrap_or_else(|error| panic!("{path}: {error}"));
        if read == 0 || (line.starts_with(b"[Event ") && !game.is_empty()) {
            if !game.is_empty() {
                each(&game);
            }
            game.clear();
            if read == 0 {
                return;
            }
        }
        game.extend_from_slice(&line);
    }
}

/// `--cut-reference <big.pgn> [--games N]`: a reference file, cut once. `games` games of at least
/// `REFERENCE_MIN_PLIES` plies (by their `PlyCount` tag; a game without one is left out), evenly
/// spaced over `source`, copied verbatim, compressed with the arena's encoder (level 19), written via
/// `.partial` and a rename, then copied up to Drive, where the source's `.pgn.zst` already sits
/// beside it, so that the reference can be rebuilt from Drive alone.
fn cut_reference(source: &str, games: usize) {
    let long_enough = |game: &[u8]| ply_count(game).is_some_and(|plies| plies >= REFERENCE_MIN_PLIES);
    let mut eligible = 0usize;
    for_each_raw_game(source, |game| eligible += usize::from(long_enough(game)));
    assert!(eligible >= games, "{source} has only {eligible} games of {REFERENCE_MIN_PLIES} plies or more");
    let stride = eligible / games;
    println!("{eligible} games of {REFERENCE_MIN_PLIES} plies or more in {source}; keeping every {stride}th");

    let local = from_repository(REFERENCE_DIR);
    std::fs::create_dir_all(&local).unwrap_or_else(|error| panic!("cannot create {local}: {error}"));
    let name = reference_file(games);
    let path = format!("{local}/{name}");
    let partial = format!("{path}.partial");
    let file = std::fs::File::create(&partial).unwrap_or_else(|error| panic!("cannot create {partial}: {error}"));
    let mut encoder =
        zstd::Encoder::new(std::io::BufWriter::new(file), COMPRESSION_LEVEL).unwrap_or_else(|error| panic!("{partial}: {error}"));
    let (mut seen, mut kept) = (0usize, 0usize);
    for_each_raw_game(source, |game| {
        if !long_enough(game) {
            return;
        }
        if seen % stride == 0 && kept < games {
            std::io::Write::write_all(&mut encoder, game).unwrap_or_else(|error| panic!("{partial}: {error}"));
            kept += 1;
        }
        seen += 1;
    });
    encoder
        .finish()
        .and_then(|mut inner| std::io::Write::flush(&mut inner))
        .unwrap_or_else(|error| panic!("{partial}: {error}"));
    std::fs::rename(&partial, &path).unwrap_or_else(|error| panic!("cannot rename {partial}: {error}"));
    println!("{kept} games written to {path}");

    kalo_chess::pipeline::remote::check_mounted().unwrap_or_else(|error| panic!("{error}"));
    // The reference only: the source's `.pgn.zst` is put on Drive by hand, and an uncompressed PGN never.
    let remote = kalo_chess::pipeline::remote::remote_path(REFERENCE_DIR);
    kalo_chess::pipeline::remote::copy_files(&local, &remote, &|file: &str| file == name).unwrap_or_else(|error| panic!("{error}"));
    println!("copied up to {}", kalo_chess::pipeline::remote::remote_path(REFERENCE_DIR));
}

/// The positions (not in check) of the reference of `games` games, copied down from Drive first when
/// this machine has no local copy. Every position with `per_game` 0; otherwise `per_game` of each
/// game's, evenly spaced from an offset drawn from the game's index, so that a large reference never
/// holds all its boards at once and every machine keeps the same ones.
fn reference_positions(games: usize, per_game: usize) -> (Vec<Board>, String) {
    let name = reference_file(games);
    let local = from_repository(REFERENCE_DIR);
    let path = format!("{local}/{name}");
    if !std::path::Path::new(&path).is_file() {
        kalo_chess::pipeline::remote::check_mounted().unwrap_or_else(|error| panic!("{error}"));
        std::fs::create_dir_all(&local).unwrap_or_else(|error| panic!("cannot create {local}: {error}"));
        kalo_chess::pipeline::remote::copy_down(REFERENCE_DIR, &local, &|file: &str| file == name)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(std::path::Path::new(&path).is_file(), "{path} is neither here nor on Drive: run --cut-reference first");
    }
    let file = std::fs::File::open(&path).unwrap_or_else(|error| panic!("cannot open {path}: {error}"));
    let decoder = zstd::Decoder::new(file).unwrap_or_else(|error| panic!("{path}: {error}"));
    let mut kept = Vec::new();
    let mut index: u64 = 0;
    for_each_game(decoder, &path, |boards| {
        let candidates: Vec<Board> =
            boards.into_iter().filter(|board| !kalo_chess::movegen::attacks::is_in_check(board, board.side_to_move())).collect();
        index += 1;
        if per_game == 0 {
            kept.extend(candidates);
            return;
        }
        if candidates.is_empty() {
            return;
        }
        // A multiplicative hash of the game's index: deterministic, and no dependency for one offset.
        let offset = (index.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 33) as usize % candidates.len();
        let take = per_game.min(candidates.len());
        for j in 0..take {
            kept.push(candidates[(offset + j * candidates.len() / take) % candidates.len()]);
        }
    });
    (kept, path)
}

/// The `fraction`-quantile of `sorted`, nearest rank.
fn quantile(sorted: &[i32], fraction: f64) -> i32 {
    let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
    sorted[index]
}

/// `--eval-scale [dir]`: does the network's evaluation share the PeSTO one's scale, which the
/// search's pruning margins assume, set in a fixed 100 per pawn? Both evaluations, in centipawns
/// from the side to move, over every position of the selection games under `dir`.
fn eval_scale(network: &Network, dir: &str) {
    let boards = selection_positions(dir);
    assert!(!boards.is_empty(), "no positions found under {dir}");
    let hand: Vec<i32> = boards.iter().map(evaluate).collect();
    let net: Vec<i32> = boards.iter().map(|board| network.evaluate(board)).collect();

    // Least squares through the origin: the factor that best maps the PeSTO evaluation onto
    // the network's. Below 1, the network is compressed toward 0 against the PeSTO scale.
    let cross: f64 = hand.iter().zip(&net).map(|(&h, &n)| h as f64 * n as f64).sum();
    let square: f64 = hand.iter().map(|&h| h as f64 * h as f64).sum();
    let slope = cross / square;
    let n = boards.len() as f64;
    let (mean_h, mean_n) = (hand.iter().map(|&h| h as f64).sum::<f64>() / n, net.iter().map(|&v| v as f64).sum::<f64>() / n);
    let covariance: f64 = hand.iter().zip(&net).map(|(&h, &v)| (h as f64 - mean_h) * (v as f64 - mean_n)).sum();
    let spread_h: f64 = hand.iter().map(|&h| (h as f64 - mean_h).powi(2)).sum();
    let spread_n: f64 = net.iter().map(|&v| (v as f64 - mean_n).powi(2)).sum();
    let correlation = covariance / (spread_h * spread_n).sqrt();

    println!("eval scale over {} positions not in check, from {dir}\n", boards.len());
    println!("  {:>12}  {:>6}  {:>6}  {:>6}  {:>6}", "|eval| (cp)", "p50", "p75", "p90", "p99");
    for (label, values) in [("PeSTO", &hand), ("network", &net)] {
        let mut sorted: Vec<i32> = values.iter().map(|value| value.abs()).collect();
        sorted.sort_unstable();
        println!(
            "  {label:>12}  {:>6}  {:>6}  {:>6}  {:>6}",
            quantile(&sorted, 0.50),
            quantile(&sorted, 0.75),
            quantile(&sorted, 0.90),
            quantile(&sorted, 0.99)
        );
    }
    println!("\n  {:>12}  {:>6}  {:>6}  {:>6}  {:>6}", "beyond", "1 pawn", "2", "4", "6");
    for (label, values) in [("PeSTO", &hand), ("network", &net)] {
        let share = |pawns: i32| values.iter().filter(|value| value.abs() > pawns * 100).count() as f64 / n * 100.0;
        println!("  {label:>12}  {:>5.1}%  {:>5.1}%  {:>5.1}%  {:>5.1}%", share(1), share(2), share(4), share(6));
    }
    println!("\n  slope of network on PeSTO, through the origin: {slope:.3}");
    println!("  correlation: {correlation:.3}");
}

/// Where `--write-pawn-scale-positions` writes the positions the engine measures a network's pawn on
/// at load (`include_str!`): a file of the library, checked into git, written once.
const PAWN_SCALE_POSITIONS_PATH: &str = "src/pawn_scale_positions.txt";

/// `--write-pawn-scale-positions`: the 10,000 positions of the 5,000-game reference read two per game,
/// the set on which the static slope `c` replaced agreed, to the rounded percent, with 100,000 positions of other
/// games for every network, as FENs, one per line. Written once; the engine reads them at load.
fn write_pawn_scale_positions() {
    let (boards, source) = reference_positions(REFERENCE_DEFAULT_GAMES, 2);
    let text: String = boards.iter().map(|board| format!("{}\n", board.to_fen(1))).collect();
    std::fs::write(PAWN_SCALE_POSITIONS_PATH, text).unwrap_or_else(|error| panic!("cannot write {PAWN_SCALE_POSITIONS_PATH}: {error}"));
    println!("{} positions from {source} written to {PAWN_SCALE_POSITIONS_PATH}", boards.len());
}

/// `--residuals`' search depths unless `--max-depth` says otherwise: 0 (quiescence alone) to 4.
const RESIDUALS_DEFAULT_MAX_DEPTH: u32 = 4;

/// `--residuals`' positions unless `--sample` says otherwise, spread evenly over the selection games.
const RESIDUALS_DEFAULT_SAMPLE: usize = 10_000;

/// A value this close to `MATE_SCORE` is a mate found by the search: no margin ever acts on it, so
/// its position is left out of the residuals.
const RESIDUALS_MATE_BAND: i32 = MATE_SCORE - 1000;

/// The `|static|` beyond which `--residuals` also prints the residuals apart: whether positions far
/// from equality, common at interior nodes, err more than game positions near it.
const RESIDUALS_FAR_CP: i32 = 300;

/// MVV-LVA's ruler for the plain search's move order only: the order changes how many nodes
/// alpha-beta visits, never the value it returns.
fn order_value(piece_type: PieceType) -> i32 {
    match piece_type {
        PieceType::Pawn => 1,
        PieceType::Knight | PieceType::Bishop => 3,
        PieceType::Rook => 5,
        PieceType::Queen => 9,
        PieceType::King => 0,
    }
}

/// Captures first, the most valuable victim by the least valuable attacker, then promotions.
fn order_key(board: &Board, mv: Move) -> i32 {
    let victim = if mv.kind() == MoveKind::EnPassantCapture {
        Some(PieceType::Pawn)
    } else {
        board.piece_at(mv.to()).map(|piece| piece.piece_type())
    };
    let attacker = board.piece_at(mv.from()).map_or(0, |piece| order_value(piece.piece_type()));
    let capture = victim.map_or(0, |victim| 100 * order_value(victim) - attacker);
    capture + mv.promotion().map_or(0, order_value)
}

/// What `--residuals` evaluates with: PeSTO, or a psq network evaluated as the engine does,
/// from accumulators updated move by move.
#[derive(Clone, Copy)]
enum Evaluator<'a> {
    Pesto,
    Psq(&'a QuantizedPsqNetwork),
}

impl Evaluator<'_> {
    /// From scratch: the static evaluation the residuals start from, and the reference the incremental
    /// path is checked against.
    fn evaluate(&self, board: &Board) -> i32 {
        match self {
            Evaluator::Pesto => evaluate(board),
            Evaluator::Psq(network) => network.evaluate(board),
        }
    }
}

/// Quiet moves past the killers are ordered by history, below every killer: a history score stays
/// far under this at the depths `--residuals` searches (at most `4 * 4` per cutoff).
const KILLER_KEY: i32 = 1 << 24;

/// The plain search `--residuals` measures with: none of the engine's pruning, extensions or TT, so
/// that what it measures is the evaluator alone, not the margins it is meant to set. Full-window
/// negamax with alpha-beta, and a quiescence of captures and promotions with stand-pat, all evasions
/// when in check. Scores from the side to move, mates as the engine scores them.
///
/// Two things make it fast without changing any value, since a full-window alpha-beta returns the same
/// value whatever the move order, and the incremental accumulator equals a refresh exactly (integer
/// arithmetic): a network's accumulators updated move by move, one pair per ply, as the engine keeps
/// them; and quiet moves ordered by two killers per ply and a `[piece][to]` history, carried from one
/// depth to the next of the same position (`values`). `ordering` off orders by MVV-LVA alone, the
/// reference the check compares against; `verify` checks every incremental evaluation against a
/// refresh.
struct PlainSearch<'a> {
    evaluator: Evaluator<'a>,
    ordering: bool,
    verify: bool,
    accumulators: Vec<[Vec<i16>; 2]>,
    killers: Vec<[Option<Move>; 2]>,
    history: [[i32; 64]; 12],
}

impl<'a> PlainSearch<'a> {
    fn new(evaluator: Evaluator<'a>, ordering: bool, verify: bool) -> PlainSearch<'a> {
        PlainSearch { evaluator, ordering, verify, accumulators: Vec::new(), killers: Vec::new(), history: [[0; 64]; 12] }
    }

    fn is_quiet(board: &Board, mv: Move) -> bool {
        mv.kind() != MoveKind::EnPassantCapture && board.piece_at(mv.to()).is_none() && mv.promotion().is_none()
    }

    fn ordered(&self, board: &Board, mut moves: Vec<Move>, ply: u32) -> Vec<Move> {
        let killers = self.killers.get(ply as usize).copied().unwrap_or([None, None]);
        moves.sort_by_key(|&mv| {
            if !self.ordering || !PlainSearch::is_quiet(board, mv) {
                return -(2 * KILLER_KEY + order_key(board, mv));
            }
            if killers[0] == Some(mv) {
                return -(KILLER_KEY + 1);
            }
            if killers[1] == Some(mv) {
                return -KILLER_KEY;
            }
            let piece = board.piece_at(mv.from()).expect("a move from an empty square");
            -self.history[piece as usize][mv.to() as usize]
        });
        moves
    }

    /// A quiet move that caused a cutoff: the ply's newest killer, and `remaining * remaining` of
    /// history.
    fn record_cutoff(&mut self, board: &Board, mv: Move, ply: u32, remaining: u32) {
        if !self.ordering || !PlainSearch::is_quiet(board, mv) {
            return;
        }
        let killers = &mut self.killers[ply as usize];
        if killers[0] != Some(mv) {
            killers[1] = killers[0];
            killers[0] = Some(mv);
        }
        let piece = board.piece_at(mv.from()).expect("a move from an empty square");
        self.history[piece as usize][mv.to() as usize] += (remaining * remaining) as i32;
    }

    /// Makes room for ply `ply + 1` and, for a network, computes its accumulators from `ply`'s.
    fn enter(&mut self, ply: u32, parent: &Board, child: &Board) {
        let next = ply as usize + 1;
        if self.killers.len() <= next {
            self.killers.resize(next + 1, [None, None]);
        }
        if let Evaluator::Psq(network) = self.evaluator {
            if self.accumulators.len() <= next {
                self.accumulators.resize(next + 1, self.accumulators[0].clone());
            }
            let (before, after) = self.accumulators.split_at_mut(next);
            network.update(&before[next - 1], &mut after[0], parent, child);
        }
    }

    fn static_eval(&self, board: &Board, ply: u32) -> i32 {
        let value = match self.evaluator {
            Evaluator::Pesto => evaluate(board),
            Evaluator::Psq(network) => network.evaluate_by_color_cp(&self.accumulators[ply as usize], board),
        };
        if self.verify {
            assert_eq!(value, self.evaluator.evaluate(board), "the incremental evaluation differs from a refresh at ply {ply}");
        }
        value
    }

    fn search(&mut self, board: &Board, depth: u32, ply: u32, mut alpha: i32, beta: i32) -> i32 {
        if depth == 0 {
            return self.quiescence(board, ply, alpha, beta);
        }
        let color = board.side_to_move();
        let moves = legal_moves(board, color);
        if moves.is_empty() {
            return if is_in_check(board, color) { -(MATE_SCORE - ply as i32) } else { 0 };
        }
        let mut best = -MATE_SCORE;
        for mv in self.ordered(board, moves, ply) {
            let mut child = *board;
            child.make_move(mv);
            self.enter(ply, board, &child);
            let score = -self.search(&child, depth - 1, ply + 1, -beta, -alpha);
            best = best.max(score);
            alpha = alpha.max(score);
            if alpha >= beta {
                self.record_cutoff(board, mv, ply, depth);
                break;
            }
        }
        best
    }

    fn quiescence(&mut self, board: &Board, ply: u32, mut alpha: i32, beta: i32) -> i32 {
        let color = board.side_to_move();
        if is_in_check(board, color) {
            let moves = legal_moves(board, color);
            if moves.is_empty() {
                return -(MATE_SCORE - ply as i32);
            }
            let mut best = -MATE_SCORE;
            for mv in self.ordered(board, moves, ply) {
                let mut child = *board;
                child.make_move(mv);
                self.enter(ply, board, &child);
                let score = -self.quiescence(&child, ply + 1, -beta, -alpha);
                best = best.max(score);
                alpha = alpha.max(score);
                if alpha >= beta {
                    break;
                }
            }
            return best;
        }
        let stand_pat = self.static_eval(board, ply);
        if stand_pat >= beta {
            return stand_pat;
        }
        let mut best = stand_pat;
        alpha = alpha.max(stand_pat);
        let mut moves = Vec::new();
        noisy_moves(board, color, &mut moves);
        moves.sort_by_key(|&mv| -order_key(board, mv));
        for mv in moves {
            let mut child = *board;
            child.make_move(mv);
            if is_in_check(&child, color) {
                continue;
            }
            self.enter(ply, board, &child);
            let score = -self.quiescence(&child, ply + 1, -beta, -alpha);
            best = best.max(score);
            alpha = alpha.max(score);
            if alpha >= beta {
                break;
            }
        }
        best
    }

    /// Readies a new root: fresh killers and history, so a position's values never depend on what
    /// was searched before it, and for a network the root's accumulators from scratch.
    fn start(&mut self, root: &Board) {
        self.killers.clear();
        self.killers.resize(2, [None, None]);
        self.history = [[0; 64]; 12];
        if let Evaluator::Psq(network) = self.evaluator {
            if self.accumulators.is_empty() {
                self.accumulators.push(network.refresh_by_color(root));
            } else {
                self.accumulators[0] = network.refresh_by_color(root);
            }
        }
    }

    fn window(&mut self, board: &Board, depth: u32, alpha: i32, beta: i32) -> i32 {
        self.start(board);
        self.search(board, depth, 0, alpha, beta)
    }

    /// The full-window value at every depth from 0 to `max_depth`, in turn, the killers and history
    /// of one depth ordering the next; each depth's time added to `seconds`.
    fn values(&mut self, board: &Board, max_depth: u32, seconds: &mut [f64]) -> Vec<i32> {
        self.start(board);
        let mut values = Vec::with_capacity(max_depth as usize + 1);
        for depth in 0..=max_depth {
            let started = Instant::now();
            values.push(self.search(board, depth, 0, -MATE_SCORE - 1, MATE_SCORE + 1));
            seconds[depth as usize] += started.elapsed().as_secs_f64();
        }
        values
    }
}

/// Checked before `--residuals` measures anything, on 8 positions at depths 0 to 2:
/// - a fail-soft alpha-beta whose full-window value is `v` fails high on `(v - 1, v)` and low on
///   `(v, v + 1)`; a broken cutoff or bound breaks one of the two;
/// - the killers and history change no value: the search ordered by MVV-LVA alone gives the same;
/// - for a network, every incremental evaluation equals a refresh.
///
/// Not against a search without alpha-beta or in the reverse order: without a TT, either explodes in
/// quiescence on tactical positions.
fn check_plain_search(evaluator: Evaluator, boards: &[Board]) {
    let mut ordered = PlainSearch::new(evaluator, true, true);
    let mut plain = PlainSearch::new(evaluator, false, false);
    let mut seconds = [0.0; 3];
    for board in boards.iter().step_by((boards.len() / 8).max(1)).take(8) {
        let values = ordered.values(board, 2, &mut seconds);
        for depth in 0..=2u32 {
            let value = values[depth as usize];
            assert_eq!(plain.window(board, depth, -MATE_SCORE - 1, MATE_SCORE + 1), value, "the ordering changed the value at depth {depth}");
            assert!(ordered.window(board, depth, value - 1, value) >= value, "no fail high below {value} at depth {depth}");
            assert!(ordered.window(board, depth, value, value + 1) <= value, "no fail low above {value} at depth {depth}");
        }
    }
}

/// `seconds` as `m:ss`, for a run that lasts minutes.
fn minutes_seconds(seconds: f64) -> String {
    let whole = seconds.round() as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// Positions between two progress lines of `--residuals`.
const RESIDUALS_PROGRESS: usize = 10_000;

/// `--residuals [dir]`: how far `d` plies of a plain search move the evaluator's own static
/// evaluation, `r_d = value_d - static`, in its own centipawns from the side to move, for `d` = 0
/// (quiescence alone) to `max_depth`, over `sample` positions of `all` (read from `source`), evenly
/// spaced. The tails each pruning bets on (`print_residual_tails`), and today's margin, `2 pawns * d`
/// in the evaluator's pawn, beside them. Each position is searched at every depth in turn by one
/// thread, positions handed out one at a time, so every depth is printed at the end.
fn residuals(evaluator: Evaluator, pawn_percent: i32, all: Vec<Board>, source: &str, max_depth: u32, sample: usize) {
    assert!(!all.is_empty(), "no positions found in {source}");
    let step = (all.len() / sample).max(1);
    let boards: Vec<Board> = all.into_iter().step_by(step).take(sample).collect();
    println!("checking the plain search on 8 positions, depths 0 to 2");
    check_plain_search(evaluator, &boards);
    println!("residuals over {} positions not in check, from {source}, pawn scale {pawn_percent}%\n", boards.len());

    let statics: Vec<i32> = boards.iter().map(|board| evaluator.evaluate(board)).collect();
    let threads = std::thread::available_parallelism().map_or(1, |count| count.get());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let started = Instant::now();
    let depths = max_depth as usize + 1;
    let (mut values, mut seconds) = (vec![Vec::new(); boards.len()], vec![0.0; depths]);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let (boards, next) = (&boards, &next);
                scope.spawn(move || {
                    let mut search = PlainSearch::new(evaluator, true, false);
                    let mut seconds = vec![0.0; depths];
                    let mut found: Vec<(usize, Vec<i32>)> = Vec::new();
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if index >= boards.len() {
                            return (found, seconds);
                        }
                        found.push((index, search.values(&boards[index], max_depth, &mut seconds)));
                        if (index + 1) % RESIDUALS_PROGRESS == 0 {
                            println!("  {} positions started, {}", index + 1, minutes_seconds(started.elapsed().as_secs_f64()));
                        }
                    }
                })
            })
            .collect();
        for handle in handles {
            let (found, thread_seconds) = handle.join().expect("a residuals thread panicked");
            for (index, position_values) in found {
                values[index] = position_values;
            }
            for (total, part) in seconds.iter_mut().zip(thread_seconds) {
                *total += part;
            }
        }
    });
    println!("searched in {} on {threads} threads\n", minutes_seconds(started.elapsed().as_secs_f64()));

    let noisy: Vec<bool> = boards.iter().map(is_noisy).collect();
    let noisy_count = noisy.iter().filter(|&&noisy| noisy).count();
    println!("noisy positions (a winning capture or a promotion to play): {noisy_count} of {}\n", boards.len());

    for depth in 0..depths {
        // (static, value, quiescence's value) of every position where neither search found a mate.
        let unmated = |i: &usize| values[*i][depth].abs() < RESIDUALS_MATE_BAND && values[*i][0].abs() < RESIDUALS_MATE_BAND;
        let triple = |i: usize| (statics[i], values[i][depth], values[i][0]);
        let kept: Vec<(i32, i32, i32)> = (0..boards.len()).filter(unmated).map(triple).collect();
        let margin = 2 * 100 * depth as i32 * pawn_percent / 100;
        println!(
            "d = {depth}: {} positions ({} mates left out), {:.1} s of CPU, today's margin {margin} cp",
            kept.len(),
            boards.len() - kept.len(),
            seconds[depth]
        );
        print_residual_tails("all", &kept);
        for (label, kind) in [("quiet", false), ("noisy", true)] {
            let part: Vec<(i32, i32, i32)> = (0..boards.len()).filter(|i| noisy[*i] == kind).filter(unmated).map(triple).collect();
            if !part.is_empty() {
                print_residual_tails(label, &part);
            }
        }
        let far: Vec<(i32, i32, i32)> = kept.iter().copied().filter(|(static_eval, _, _)| static_eval.abs() > RESIDUALS_FAR_CP).collect();
        if !far.is_empty() {
            print_residual_tails(&format!("|static| > {RESIDUALS_FAR_CP}"), &far);
        }
        println!();
    }
}

/// Whether `board` is noisy for `--residuals`' split: the side to move has a legal capture that wins
/// material by static exchange, or a promotion. The same for every evaluator, so two networks are
/// compared on the same positions; close to the training's quiet filter (in check, or a capture or
/// promotion best), without a search to name the best move. A network trained on quiet positions
/// alone has never seen these, yet the search evaluates them at every node for its pruning.
fn is_noisy(board: &Board) -> bool {
    legal_moves(board, board.side_to_move()).into_iter().any(|mv| {
        mv.promotion().is_some()
            || ((board.piece_at(mv.to()).is_some() || mv.kind() == MoveKind::EnPassantCapture) && kalo_chess::movegen::see::see(board, mv) > 0)
    })
}

/// One line per tail, each the bet of the pruning that reads it, with the 50, 90, 95 and 99%
/// quantiles:
/// - `value - static`: how far the static evaluation underestimates, captures included;
/// - `value - qsearch`: how far the search rises above quiescence's value, what razoring (which
///   returns quiescence's value) and classical futility (which skips quiet moves only, so the
///   captures quiescence sees are still searched) bet on;
/// - `static - value`: how far the static evaluation overestimates, what reverse futility (which
///   returns the static evaluation) bets on.
#[allow(clippy::type_complexity)]
fn print_residual_tails(label: &str, kept: &[(i32, i32, i32)]) {
    let mean = kept.iter().map(|&(static_eval, value, _)| (value - static_eval) as f64).sum::<f64>() / kept.len() as f64;
    println!("  {label} ({} positions), mean value - static {mean:+.1} cp", kept.len());
    let tails: [(&str, fn(&(i32, i32, i32)) -> i32); 3] = [
        ("value - static", |&(static_eval, value, _)| value - static_eval),
        ("value - qsearch", |&(_, value, quiescence)| value - quiescence),
        ("static - value", |&(static_eval, value, _)| static_eval - value),
    ];
    for (tail, residual) in tails {
        let mut sorted: Vec<i32> = kept.iter().map(residual).collect();
        sorted.sort_unstable();
        print_quantiles(&format!("{tail} (cp)"), &sorted);
    }
    // The same tails in label space (`labels::LOGISTIC`, the curve in [-1, 1] the networks are trained
    // against), in thousandths: whether a margin in expected score fits better than one in
    // centipawns. Only the forward conversion, from each centipawn value, never its inverse.
    let label = |cp: i32| -> i32 { (kalo_chess::labels::LOGISTIC.score(cp as f64 / 100.0) * 1000.0).round() as i32 };
    let label_tails: [(&str, &dyn Fn(&(i32, i32, i32)) -> i32); 3] = [
        ("value - static", &|&(static_eval, value, _)| label(value) - label(static_eval)),
        ("value - qsearch", &|&(_, value, quiescence)| label(value) - label(quiescence)),
        ("static - value", &|&(static_eval, value, _)| label(static_eval) - label(value)),
    ];
    for (tail, residual) in label_tails {
        let mut sorted: Vec<i32> = kept.iter().map(residual).collect();
        sorted.sort_unstable();
        print_quantiles(&format!("{tail} (1/1000)"), &sorted);
    }
}

/// One tail's line: its 50, 90, 95, 99 and 99.9% quantiles. The last rests on about 100 positions of
/// 100,000, but it is the one that matters: margins set at p99 lost a duel badly, so a margin that
/// works in play sits far past it.
fn print_quantiles(label: &str, sorted: &[i32]) {
    println!(
        "    {label:<24} p50 {:>5}  p90 {:>5}  p95 {:>5}  p99 {:>5}  p99.9 {:>5}",
        quantile(sorted, 0.50),
        quantile(sorted, 0.90),
        quantile(sorted, 0.95),
        quantile(sorted, 0.99),
        quantile(sorted, 0.999)
    );
}

/// `--quantization [dir]`: how far quantizing at load moves the network's evaluation, over every
/// position (not in check) of the games under `dir`. The quantized evaluation is the scalar reference
/// (`QuantizedPsqNetwork`), so this measures the scheme's precision, not a kernel's speed.
fn quantization(network: &Network, dir: &str) {
    let Network::Psq(network, quantized) = network;
    let boards = selection_positions(dir);
    assert!(!boards.is_empty(), "no positions found under {dir}");

    let mut peak = 0;
    let mut differences_pawns: Vec<f64> = Vec::with_capacity(boards.len());
    let mut differences_cp: Vec<i32> = Vec::with_capacity(boards.len());
    for board in &boards {
        for perspective in [Color::White, Color::Black] {
            peak = quantized.refresh(board, perspective).iter().fold(peak, |most, lane| most.max(lane.abs()));
        }
        differences_pawns.push((quantized.evaluate_pawns(board) as f64 - network.evaluate_pawns(board) as f64).abs());
        differences_cp.push((quantized.evaluate(board) - network.evaluate(board)).abs());
    }
    differences_pawns.sort_by(|a, b| a.total_cmp(b));
    differences_cp.sort_unstable();
    let n = boards.len();
    let share_cp = |at_least: i32| -> f64 { differences_cp.iter().filter(|&&d| d >= at_least).count() as f64 / n as f64 * 100.0 };

    println!("quantization over {n} positions not in check, from {dir}\n");
    println!(
        "  accumulator: i16, 1.0 = {} (shift {}); largest lane seen {peak}, {:.1}% of i16",
        quantized.accumulator_one(),
        quantized.accumulator_shift,
        peak as f64 / i16::MAX as f64 * 100.0
    );
    let pawn_at = |fraction: f64| -> f64 { differences_pawns[((n - 1) as f64 * fraction).round() as usize] };
    let mean = differences_pawns.iter().sum::<f64>() / n as f64;
    println!(
        "  |quantized - f32| in centipawns: mean {:.2}, p50 {:.2}, p90 {:.2}, p99 {:.2}, max {:.2}",
        100.0 * mean,
        100.0 * pawn_at(0.50),
        100.0 * pawn_at(0.90),
        100.0 * pawn_at(0.99),
        100.0 * differences_pawns[n - 1]
    );
    println!(
        "  rounded evaluations differing: by 1 cp or more {:.1}%, 5 or more {:.1}%, 20 or more {:.1}%",
        share_cp(1),
        share_cp(5),
        share_cp(20)
    );
}

/// `--hidden-precision [dir]`: for a hidden layer beside the output (screlu_hidden, pairwise_hidden),
/// how far the engine's input rounding moves the evaluation, and how far it would with the hidden
/// layer reading finer inputs. Simulated in f32 from the f32 network: each lane rounded to 8 bits
/// (`0..=255`), as the engine's activations; each product or square either rounded to 7 bits as
/// `product_of` does, or kept whole (`a * b / 65025`). Weights stay f32, so only the inputs' rounding
/// is measured. Three variants against the exact network: the engine's (7 bits into both the hidden
/// layer and the skip), the hidden layer's input whole, and the skip's input whole.
fn hidden_precision(network: &Network, dir: &str) {
    let Network::Psq(network, _) = network;
    let [hidden] = network.hidden.as_slice() else {
        panic!("--hidden-precision takes a network with one hidden layer beside its output");
    };
    let output = &network.output;
    let width = hidden.out_dim;
    assert!(
        (network.pairwise || network.screlu) && output.in_dim == 2 * width + hidden.in_dim,
        "--hidden-precision takes a screlu_hidden or pairwise_hidden network"
    );
    let boards = selection_positions(dir);
    assert!(!boards.is_empty(), "no positions found under {dir}");

    // A lane as the engine's 8-bit activation, `0..=255`.
    let lane = |value: f32| -> i32 { (value.clamp(0.0, 1.0) * 255.0).round() as i32 };
    // Each product or square from both perspectives, own first: (whole, rounded to 7 bits), both in
    // `[0, 1]`.
    let inputs = |own: &[f32], opponent: &[f32]| -> (Vec<f32>, Vec<f32>) {
        let mut whole = Vec::with_capacity(hidden.in_dim);
        let mut rounded = Vec::with_capacity(hidden.in_dim);
        for lanes in [own, opponent] {
            let pairs: Vec<(i32, i32)> = if network.pairwise {
                let (a, b) = lanes.split_at(lanes.len() / 2);
                a.iter().zip(b).map(|(&a, &b)| (lane(a), lane(b))).collect()
            } else {
                lanes.iter().map(|&a| (lane(a), lane(a))).collect()
            };
            for (a, b) in pairs {
                whole.push((a * b) as f32 / 65025.0);
                rounded.push(((a * b + 256) >> 9) as f32 / (65025.0 / 512.0));
            }
        }
        (whole, rounded)
    };
    // The head, its hidden layer reading `hidden_in` and its skip reading `skip_in`.
    let head = |hidden_in: &[f32], skip_in: &[f32]| -> f32 {
        let mut value = output.bias[0];
        for row in 0..width {
            let weights = &hidden.weight[row * hidden.in_dim..(row + 1) * hidden.in_dim];
            let sum: f32 = hidden.bias[row] + weights.iter().zip(hidden_in).map(|(w, x)| w * x).sum::<f32>();
            let clipped = sum.clamp(0.0, 1.0);
            value += output.weight[row] * clipped + output.weight[width + row] * clipped * clipped;
        }
        value + output.weight[2 * width..].iter().zip(skip_in).map(|(w, x)| w * x).sum::<f32>()
    };

    let names = ["engine (7 bits into both)", "hidden layer's input whole", "skip's input whole"];
    let mut differences: [Vec<f64>; 3] = std::array::from_fn(|_| Vec::with_capacity(boards.len()));
    for board in &boards {
        let side_to_move = board.side_to_move();
        let own = network.refresh(board, side_to_move);
        let opponent = network.refresh(board, side_to_move.opposite());
        let exact = network.evaluate_accumulators(&own, &opponent) as f64;
        let (whole, rounded) = inputs(&own, &opponent);
        let variants = [head(&rounded, &rounded), head(&whole, &rounded), head(&rounded, &whole)];
        for (difference, variant) in differences.iter_mut().zip(variants) {
            difference.push((variant as f64 - exact).abs());
        }
    }
    let n = boards.len();
    println!("hidden precision over {n} positions not in check, from {dir}");
    println!("|variant - f32| in centipawns; lanes at 8 bits in every variant, weights f32\n");
    println!("  {:<28} {:>6} {:>6} {:>6} {:>7} {:>7}  {:>6}", "variant", "mean", "p50", "p90", "p99", "max", ">=20cp");
    for (name, difference) in names.iter().zip(differences.iter_mut()) {
        difference.sort_by(|a, b| a.total_cmp(b));
        let at = |fraction: f64| -> f64 { 100.0 * difference[((n - 1) as f64 * fraction).round() as usize] };
        let mean = 100.0 * difference.iter().sum::<f64>() / n as f64;
        let beyond = difference.iter().filter(|&&d| d >= 0.2).count() as f64 / n as f64 * 100.0;
        println!(
            "  {name:<28} {mean:>6.2} {:>6.2} {:>6.2} {:>7.2} {:>7.2}  {beyond:>5.1}%",
            at(0.50),
            at(0.90),
            at(0.99),
            100.0 * difference[n - 1]
        );
    }
}

/// The bound an `i8` first hidden layer at Stockfish's fixed scale of 64 can hold: `127 / 64`.
const I8_WEIGHT_BOUND: f32 = 127.0 / 64.0;

/// `--weights`: how the first hidden layer's weights sit against `I8_WEIGHT_BOUND`, overall and per
/// row: the largest |w|, the share beyond the bound, and quantiles of |w|.
fn weights(network: &Network) {
    let Network::Psq(network, _) = network;
    let Some(first) = network.hidden.first() else {
        println!("no hidden layer: nothing to clip");
        return;
    };
    let beyond = |values: &[f32]| -> usize { values.iter().filter(|w| w.abs() > I8_WEIGHT_BOUND).count() };
    let mut sorted: Vec<f32> = first.weight.iter().map(|w| w.abs()).collect();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let at = |fraction: f64| -> f32 { sorted[((sorted.len() - 1) as f64 * fraction).round() as usize] };
    println!("first hidden layer: {} x {}, bound +/-{I8_WEIGHT_BOUND:.4}\n", first.out_dim, first.in_dim);
    println!(
        "  |w|: p50 {:.4}, p90 {:.4}, p99 {:.4}, p99.9 {:.4}, max {:.4}",
        at(0.50), at(0.90), at(0.99), at(0.999), sorted[sorted.len() - 1]
    );
    let total = beyond(&first.weight);
    println!("  beyond the bound: {total} of {} ({:.3}%)\n", sorted.len(), total as f64 / sorted.len() as f64 * 100.0);
    println!("  {:>4}  {:>8}  {:>7}", "row", "max |w|", "beyond");
    for (row, weights) in first.weight.chunks(first.in_dim).enumerate() {
        let largest = weights.iter().fold(0.0f32, |most, w| most.max(w.abs()));
        println!("  {row:>4}  {largest:>8.4}  {:>7}", beyond(weights));
    }
    // A hidden layer beside the output (screlu_hidden, pairwise_hidden): the output reads the layer's
    // clipped values, then their squares, then the skip's inputs. The engine quantizes the skip only,
    // so the head's f32 weights say how much the hidden values' rounding error is magnified.
    let width = first.out_dim;
    let output = &network.output;
    if network.hidden.len() == 1 && output.in_dim > 2 * width {
        let skip = &output.weight[2 * width..];
        let mut skip_sorted: Vec<f32> = skip.iter().map(|w| w.abs()).collect();
        skip_sorted.sort_by(|a, b| a.total_cmp(b));
        let skip_at = |fraction: f64| -> f32 { skip_sorted[((skip_sorted.len() - 1) as f64 * fraction).round() as usize] };
        println!("\nhidden layer beside the output: head over {} values, skip over {}, bias {:.4}\n", 2 * width, skip.len(), output.bias[0]);
        println!(
            "  skip |w|: p50 {:.4}, p90 {:.4}, p99 {:.4}, max {:.4}, sum {:.4}\n",
            skip_at(0.50),
            skip_at(0.90),
            skip_at(0.99),
            skip_sorted[skip_sorted.len() - 1],
            skip_sorted.iter().sum::<f32>()
        );
        println!("  {:>4}  {:>10}  {:>10}", "row", "w clipped", "w squared");
        for row in 0..width {
            println!("  {row:>4}  {:>10.4}  {:>10.4}", output.weight[row], output.weight[width + row]);
        }
    }
}

/// A first hidden layer quantized per row, as the engine's `quantize` does: row `r` multiplied by
/// `largest / max |w|` of the row and rounded, `largest` being the type's own maximum. Returns the
/// weights and each row's scale.
fn quantize_rows<T>(layer: &Layer, largest: f32, convert: impl Fn(f32) -> T) -> (Vec<T>, Vec<f32>) {
    let mut weights = Vec::with_capacity(layer.weight.len());
    let mut scales = Vec::with_capacity(layer.out_dim);
    for row in layer.weight.chunks(layer.in_dim) {
        let most = row.iter().fold(0.0f32, |most, w| most.max(w.abs()));
        let scale = if most > 0.0 { largest / most } else { 1.0 };
        weights.extend(row.iter().map(|w| convert((w * scale).round())));
        scales.push(scale);
    }
    (weights, scales)
}

/// The evaluation in pawns from a first hidden layer's integer sums, by the engine's arithmetic
/// (`hidden_value`, then the f32 output layer), in scalar.
fn head_from_sums(network: &PsqNetwork, sums: &[i32], scales: &[f32]) -> f32 {
    let hidden = &network.hidden[0];
    let mut value = network.output.bias[0];
    for row in 0..hidden.out_dim {
        let activation = (hidden.bias[row] + sums[row] as f32 / (127.0 * scales[row])).clamp(0.0, 1.0);
        value += network.output.weight[row] * activation;
    }
    value
}

/// Both perspectives' activations, side to move first, from the quantized accumulators: the input
/// both the `i16` and the `i8` first layer read.
fn head_activations(quantized: &QuantizedPsqNetwork, board: &Board) -> Vec<i16> {
    let one = quantized.accumulator_one();
    let shift = quantized.accumulator_shift;
    let rounding = if shift == 0 { 0 } else { 1 << (shift - 1) };
    let side_to_move = board.side_to_move();
    let mut activations = Vec::new();
    for perspective in [side_to_move, side_to_move.opposite()] {
        activations.extend(quantized.refresh(board, perspective).iter().map(|&lane| ((lane.clamp(0, one) + rounding) >> shift) as i16));
    }
    activations
}

/// `--i8 [dir]`: the precision and the speed of an `i8` first hidden layer against today's `i16`.
fn i8_head(network: &Network, dir: &str) {
    let Network::Psq(network, quantized) = network;
    if network.pairwise {
        println!("--i8 reads the activations of plain psq lanes, not a pairwise network's products");
        return;
    }
    let [hidden] = network.hidden.as_slice() else {
        println!("--i8 needs exactly one hidden layer, this network has {}", network.hidden.len());
        return;
    };
    let (in_dim, out_dim) = (hidden.in_dim, hidden.out_dim);
    assert!(in_dim % 32 == 0 && out_dim % 8 == 0, "the kernels need inputs in 32s and rows in 8s, not {in_dim} x {out_dim}");
    let (weights_i16, scales_i16) = quantize_rows(hidden, i16::MAX as f32, |w| w as i16);
    let (weights_i8, scales_i8) = quantize_rows(hidden, i8::MAX as f32, |w| w as i8);

    let boards = selection_positions(dir);
    assert!(!boards.is_empty(), "no positions found under {dir}");
    let activations: Vec<Vec<i16>> = boards.iter().map(|board| head_activations(quantized, board)).collect();
    let scalar_sums = |activations: &[i16], weights: &dyn Fn(usize) -> i32| -> Vec<i32> {
        (0..out_dim).map(|row| (0..in_dim).map(|i| activations[i] as i32 * weights(row * in_dim + i)).sum()).collect()
    };

    // Precision, in scalar, against f32. The i16 head is checked against the engine's own scalar
    // reference, so that this reimplementation is known to be the engine's arithmetic.
    let mut differences = [Vec::with_capacity(boards.len()), Vec::with_capacity(boards.len())];
    for (board, activations) in boards.iter().zip(&activations) {
        let exact = network.evaluate_pawns(board);
        let value_i16 = head_from_sums(network, &scalar_sums(activations, &|i| weights_i16[i] as i32), &scales_i16);
        let engine = quantized.evaluate_pawns(board);
        assert!((value_i16 - engine).abs() <= 1e-4 * engine.abs().max(1.0), "the bench's i16 head gives {value_i16}, the engine's {engine}");
        let value_i8 = head_from_sums(network, &scalar_sums(activations, &|i| weights_i8[i] as i32), &scales_i8);
        differences[0].push(100.0 * (value_i16 - exact).abs() as f64);
        differences[1].push(100.0 * (value_i8 - exact).abs() as f64);
    }
    println!("first hidden layer {out_dim} x {in_dim}, over {} positions not in check, from {dir}\n", boards.len());
    println!("  |quantized - f32| in centipawns:");
    let [differences_i16, differences_i8] = &mut differences;
    for (label, values) in [("i16", differences_i16), ("i8", differences_i8)] {
        values.sort_by(|a, b| a.total_cmp(b));
        let at = |fraction: f64| -> f64 { values[((values.len() - 1) as f64 * fraction).round() as usize] };
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        println!("    {label:>4}: mean {mean:.2}, p50 {:.2}, p90 {:.2}, p99 {:.2}, max {:.2}", at(0.5), at(0.9), at(0.99), at(1.0));
    }

    // Speed: the first layer alone, all rows, from the activations, per evaluation. The i8 kernels
    // include packing the activations to u8, which only they need.
    let sample = &activations[..activations.len().min(1000)];
    let vnni = std::arch::is_x86_feature_detected!("avxvnni");
    for activations in sample {
        let expected_i16 = scalar_sums(activations, &|i| weights_i16[i] as i32);
        let expected_i8 = scalar_sums(activations, &|i| weights_i8[i] as i32);
        assert_eq!(first_layer_i16(&weights_i16, activations, out_dim), expected_i16, "the i16 kernel");
        let mut packed = vec![0u8; in_dim];
        assert_eq!(first_layer_i8(&weights_i8, activations, &mut packed, out_dim, false), expected_i8, "the AVX2 i8 kernel");
        if vnni {
            assert_eq!(first_layer_i8(&weights_i8, activations, &mut packed, out_dim, true), expected_i8, "the VNNI i8 kernel");
        }
    }
    let mut packed = vec![0u8; in_dim];
    let time_i16 = time_per_item(sample.len(), |index| {
        std::hint::black_box(first_layer_i16(&weights_i16, std::hint::black_box(&sample[index]), out_dim));
    });
    let time_avx2 = time_per_item(sample.len(), |index| {
        std::hint::black_box(first_layer_i8(&weights_i8, std::hint::black_box(&sample[index]), &mut packed, out_dim, false));
    });
    println!("\n  the first layer per evaluation, over {} positions (kernels checked against scalar sums):", sample.len());
    println!("    i16 madd:          {time_i16:>6.1} ns");
    println!("    i8 maddubs (AVX2): {time_avx2:>6.1} ns, {:.2}x", time_i16 / time_avx2);
    if vnni {
        let time_vnni = time_per_item(sample.len(), |index| {
            std::hint::black_box(first_layer_i8(&weights_i8, std::hint::black_box(&sample[index]), &mut packed, out_dim, true));
        });
        println!("    i8 dpbusd (VNNI):  {time_vnni:>6.1} ns, {:.2}x", time_i16 / time_vnni);
    } else {
        println!("    i8 dpbusd (VNNI):  not available on this CPU");
    }
}

/// Every row's sum for the `i16` weights, eight rows at a time, as the engine's `dots_i16_8_avx2`.
fn first_layer_i16(weights: &[i16], activations: &[i16], out_dim: usize) -> Vec<i32> {
    use std::arch::x86_64::{__m256i, _mm256_add_epi32, _mm256_loadu_si256, _mm256_madd_epi16, _mm256_setzero_si256};

    let in_dim = activations.len();
    let mut result = Vec::with_capacity(out_dim);
    for group in 0..out_dim / 8 {
        let rows = &weights[group * 8 * in_dim..(group + 1) * 8 * in_dim];
        let mut sums = [unsafe { _mm256_setzero_si256() }; 8];
        for chunk in 0..in_dim / 16 {
            let offset = chunk * 16;
            unsafe {
                let input = _mm256_loadu_si256(activations.as_ptr().add(offset) as *const __m256i);
                for (row, sum) in sums.iter_mut().enumerate() {
                    let weight = _mm256_loadu_si256(rows.as_ptr().add(row * in_dim + offset) as *const __m256i);
                    *sum = _mm256_add_epi32(*sum, _mm256_madd_epi16(input, weight));
                }
            }
        }
        result.extend(reduce_8(sums));
    }
    result
}

/// Every row's sum for the `i8` weights: the activations packed to `u8` into `packed`, then eight
/// rows at a time, with `maddubs` then `madd` by ones (AVX2), or `dpbusd` (AVX-VNNI) when `vnni`.
/// Activations are at most 127, so a `maddubs` pair is at most `2 * 127 * 127`, under `i16::MAX`:
/// it never saturates, and both kernels give the exact sums.
fn first_layer_i8(weights: &[i8], activations: &[i16], packed: &mut [u8], out_dim: usize, vnni: bool) -> Vec<i32> {
    use std::arch::x86_64::{__m256i, _mm256_loadu_si256, _mm256_packus_epi16, _mm256_permute4x64_epi64, _mm256_storeu_si256};

    let in_dim = activations.len();
    for chunk in 0..in_dim / 32 {
        let offset = chunk * 32;
        unsafe {
            let low = _mm256_loadu_si256(activations.as_ptr().add(offset) as *const __m256i);
            let high = _mm256_loadu_si256(activations.as_ptr().add(offset + 16) as *const __m256i);
            // `packus` interleaves the two inputs by 128-bit lane; the permute puts them back in order.
            let bytes = _mm256_permute4x64_epi64(_mm256_packus_epi16(low, high), 0b11_01_10_00);
            _mm256_storeu_si256(packed.as_mut_ptr().add(offset) as *mut __m256i, bytes);
        }
    }
    let mut result = Vec::with_capacity(out_dim);
    for group in 0..out_dim / 8 {
        let rows = &weights[group * 8 * in_dim..(group + 1) * 8 * in_dim];
        // Safety: `vnni` is set only when the CPU reports AVX-VNNI.
        let sums = if vnni { unsafe { rows_i8_vnni(rows, packed) } } else { rows_i8_avx2(rows, packed) };
        result.extend(reduce_8(sums));
    }
    result
}

/// Eight rows' sums in `i32` lanes, by `maddubs` then `madd` by ones.
fn rows_i8_avx2(rows: &[i8], packed: &[u8]) -> [std::arch::x86_64::__m256i; 8] {
    use std::arch::x86_64::{
        __m256i, _mm256_add_epi32, _mm256_loadu_si256, _mm256_madd_epi16, _mm256_maddubs_epi16, _mm256_set1_epi16,
        _mm256_setzero_si256,
    };

    let in_dim = packed.len();
    let mut sums = [unsafe { _mm256_setzero_si256() }; 8];
    for chunk in 0..in_dim / 32 {
        let offset = chunk * 32;
        unsafe {
            let input = _mm256_loadu_si256(packed.as_ptr().add(offset) as *const __m256i);
            for (row, sum) in sums.iter_mut().enumerate() {
                let weight = _mm256_loadu_si256(rows.as_ptr().add(row * in_dim + offset) as *const __m256i);
                let pairs = _mm256_maddubs_epi16(input, weight);
                *sum = _mm256_add_epi32(*sum, _mm256_madd_epi16(pairs, _mm256_set1_epi16(1)));
            }
        }
    }
    sums
}

/// `rows_i8_avx2` with `dpbusd`: the multiply, the pairing and the accumulation in one instruction.
///
/// # Safety
/// The CPU must have AVX-VNNI.
#[target_feature(enable = "avxvnni")]
unsafe fn rows_i8_vnni(rows: &[i8], packed: &[u8]) -> [std::arch::x86_64::__m256i; 8] {
    use std::arch::x86_64::{__m256i, _mm256_dpbusd_avx_epi32, _mm256_loadu_si256};

    let in_dim = packed.len();
    let mut sums = [unsafe { std::mem::zeroed::<__m256i>() }; 8];
    for chunk in 0..in_dim / 32 {
        let offset = chunk * 32;
        unsafe {
            let input = _mm256_loadu_si256(packed.as_ptr().add(offset) as *const __m256i);
            for (row, sum) in sums.iter_mut().enumerate() {
                let weight = _mm256_loadu_si256(rows.as_ptr().add(row * in_dim + offset) as *const __m256i);
                *sum = _mm256_dpbusd_avx_epi32(*sum, input, weight);
            }
        }
    }
    sums
}

/// The eight `i32` lanes of each of eight sums, each summed: the engine's `dots_i16_8_avx2` reduction.
fn reduce_8(sums: [std::arch::x86_64::__m256i; 8]) -> [i32; 8] {
    use std::arch::x86_64::{__m256i, _mm256_add_epi32, _mm256_hadd_epi32, _mm256_permute2x128_si256, _mm256_storeu_si256};

    let mut result = [0i32; 8];
    unsafe {
        let pair01 = _mm256_hadd_epi32(sums[0], sums[1]);
        let pair23 = _mm256_hadd_epi32(sums[2], sums[3]);
        let pair45 = _mm256_hadd_epi32(sums[4], sums[5]);
        let pair67 = _mm256_hadd_epi32(sums[6], sums[7]);
        let low4 = _mm256_hadd_epi32(pair01, pair23);
        let high4 = _mm256_hadd_epi32(pair45, pair67);
        let lanes_0_to_3 = _mm256_permute2x128_si256(low4, high4, 0x20);
        let lanes_4_to_7 = _mm256_permute2x128_si256(low4, high4, 0x31);
        _mm256_storeu_si256(result.as_mut_ptr() as *mut __m256i, _mm256_add_epi32(lanes_0_to_3, lanes_4_to_7));
    }
    result
}

use kalo_chess::bench_positions::{board as board_from_line, POSITIONS as POSITIONS_FOR_SEARCH};

/// `--reference-games`' value, the reference file to cut or read: `REFERENCE_DEFAULT_GAMES` when
/// absent. Not `--games`, which `--search-only` already reads as a directory.
fn reference_games(arguments: &[String]) -> usize {
    arguments
        .iter()
        .position(|argument| argument == "--reference-games")
        .and_then(|at| arguments.get(at + 1))
        .map_or(REFERENCE_DEFAULT_GAMES, |value| value.parse::<usize>().expect("--reference-games takes a number"))
}

/// `--head`'s value: comma-separated hidden-layer widths, or empty for no hidden layer.
fn parse_head(value: &str) -> Vec<usize> {
    value
        .split(',')
        .map(str::trim)
        .filter(|width| !width.is_empty())
        .map(|width| width.parse::<usize>().expect("--head takes comma-separated numbers, or \"\" for none"))
        .collect()
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.iter().any(|argument| argument == "--write-pawn-scale-positions") {
        write_pawn_scale_positions();
        return;
    }
    if let Some(at) = arguments.iter().position(|argument| argument == "--cut-reference") {
        cut_reference(arguments.get(at + 1).expect("--cut-reference takes the source PGN's path"), reference_games(&arguments));
        return;
    }
    let depth = arguments
        .iter()
        .position(|argument| argument == "--depth")
        .and_then(|at| arguments.get(at + 1))
        .map(|value| value.parse::<u32>().expect("--depth takes a number"));
    let path = arguments
        .iter()
        .position(|argument| argument == "--network")
        .map(|at| player_path(arguments.get(at + 1).expect("--network takes a forge run's player, <run>-NNNN")))
        .or_else(|| arguments.iter().find(|argument| argument.ends_with(".nnue")).map(|path| from_repository(path)));
    let embedding_dim = arguments
        .iter()
        .position(|argument| argument == "--width")
        .and_then(|at| arguments.get(at + 1))
        .map(|value| value.parse::<usize>().expect("--width takes a number"))
        .unwrap_or(DEFAULT_EMBEDDING_DIM);
    // A stack needs two hidden widths, hilda's without `--head`; a kings one (`--screlu`) one, a
    // side's.
    let stack = arguments.iter().any(|argument| argument == "--stack");
    let default_hidden_dims = match (stack, arguments.iter().any(|argument| argument == "--screlu")) {
        (true, true) => vec![16],
        (true, false) => vec![16, 16],
        (false, _) => DEFAULT_HIDDEN_DIMS.to_vec(),
    };
    let hidden_dims = arguments
        .iter()
        .position(|argument| argument == "--head")
        .map(|at| parse_head(arguments.get(at + 1).map(String::as_str).unwrap_or("")))
        .unwrap_or(default_hidden_dims);

    // `--psq` names the default explicitly.
    let network = match &path {
        Some(path) => Network::load(path).unwrap_or_else(|problem| panic!("{problem}")),
        None if arguments.iter().any(|argument| argument == "--stack") => {
            let threats = arguments.iter().any(|argument| argument == "--threats");
            let screlu = arguments.iter().any(|argument| argument == "--screlu");
            Network::from_psq(random_stack_network(embedding_dim, &hidden_dims, threats, screlu)).unwrap_or_else(|problem| panic!("{problem}"))
        }
        None if arguments.iter().any(|argument| argument == "--material") => {
            let screlu = arguments.iter().any(|argument| argument == "--screlu");
            let phase = arguments.iter().any(|argument| argument == "--phase");
            let count = arguments.iter().any(|argument| argument == "--count");
            let taper = arguments.iter().any(|argument| argument == "--taper");
            Network::from_psq(random_material_network(embedding_dim, screlu, phase, count, taper)).unwrap_or_else(|problem| panic!("{problem}"))
        }
        None if arguments.iter().any(|argument| argument == "--halfk") => {
            // psq_threats_screlu's random network with halfk's rows in place of psq's, drawn apart. No
            // hidden layer, whatever `--head` says; with `--hidden`, two, `--head`'s or 16 and 32, the
            // output reading the second alone; with `--pairwise` too, over the products instead of the
            // squares (halfk_threats_pairwise_hidden).
            let mut network = random_psq_network(embedding_dim, &[], true, false, true, false);
            network.transform = Xorshift(0xD1B54A32D192ED03).weights(halfk::ROWS * embedding_dim);
            network.halfk = true;
            if arguments.iter().any(|argument| argument == "--hidden") {
                let widths = if arguments.iter().any(|argument| argument == "--head") { hidden_dims.clone() } else { vec![16, 32] };
                assert_eq!(widths.len(), 2, "--halfk --hidden takes two hidden widths, --head \"16,32\"");
                let pairwise = arguments.iter().any(|argument| argument == "--pairwise");
                network.pairwise = pairwise;
                network.screlu = !pairwise;
                let mut rng = Xorshift(0x94D049BB133111EB);
                let mut in_dim = if pairwise { embedding_dim } else { 2 * embedding_dim };
                network.hidden = widths
                    .iter()
                    .map(|&out_dim| {
                        let layer = Layer { in_dim, out_dim, weight: rng.weights(out_dim * in_dim), bias: rng.weights(out_dim) };
                        in_dim = out_dim;
                        layer
                    })
                    .collect();
                network.output = Layer { in_dim, out_dim: 1, weight: rng.weights(in_dim), bias: rng.weights(1) };
            }
            Network::from_psq(network).unwrap_or_else(|problem| panic!("{problem}"))
        }
        None => {
            let threats = arguments.iter().any(|argument| argument == "--threats");
            let pairwise = arguments.iter().any(|argument| argument == "--pairwise");
            let screlu = arguments.iter().any(|argument| argument == "--screlu");
            let beside = arguments.iter().any(|argument| argument == "--beside");
            // The one shape a screlu network has (`PsqNetwork::validate`), said here rather than as a
            // wrong timing.
            assert!(
                !screlu || (threats && !pairwise && hidden_dims.len() <= 1),
                "--screlu is psq_threats with no hidden layer, --head \"\", or one beside its output, --head \"16\""
            );
            assert!(
                !beside || (threats && pairwise && hidden_dims.len() == 1),
                "--beside is psq_threats_pairwise's with one hidden layer: --threats --pairwise --beside --head \"16\""
            );
            Network::from_psq(random_psq_network(embedding_dim, &hidden_dims, threats, pairwise, screlu, beside))
                .unwrap_or_else(|problem| panic!("{problem}"))
        }
    };
    // `--int8`: a two-hidden head's first layer quantized to 8 bits instead (`HiddenArithmetic`), the
    // network otherwise the same, loaded or random. Every other head ignores it.
    let network = if arguments.iter().any(|argument| argument == "--int8") {
        let Network::Psq(psq_network, _) = network;
        Network::from_psq_with(psq_network, HiddenArithmetic::Int8).unwrap_or_else(|problem| panic!("{problem}"))
    } else {
        network
    };
    let source = format!(
        "{} (pawn scale {}%)",
        path.as_deref().unwrap_or("a randomly generated network"),
        network.pawn_percent()
    );

    let positions: Vec<(&str, Board)> = POSITIONS
        .iter()
        .map(|fen| {
            let (board, _) = Board::from_fen(fen).expect("benchmark FENs are valid");
            (*fen, board)
        })
        .collect();
    let boards: Vec<Board> = positions.iter().map(|(_, board)| *board).collect();

    if let Some(at) = arguments.iter().position(|argument| argument == "--residuals") {
        let dir = arguments.get(at + 1).filter(|value| !value.starts_with("--") && !value.ends_with(".nnue"));
        let per_game = arguments
            .iter()
            .position(|argument| argument == "--per-game")
            .and_then(|at| arguments.get(at + 1))
            .map_or(0, |value| value.parse::<usize>().expect("--per-game takes a number"));
        let (all, positions_source) = match dir {
            Some(dir) => (selection_positions(dir), dir.clone()),
            None => reference_positions(reference_games(&arguments), per_game),
        };
        let max_depth = arguments
            .iter()
            .position(|argument| argument == "--max-depth")
            .and_then(|at| arguments.get(at + 1))
            .map_or(RESIDUALS_DEFAULT_MAX_DEPTH, |value| value.parse::<u32>().expect("--max-depth takes a number"));
        let sample = arguments
            .iter()
            .position(|argument| argument == "--sample")
            .and_then(|at| arguments.get(at + 1))
            .map_or(RESIDUALS_DEFAULT_SAMPLE, |value| value.parse::<usize>().expect("--sample takes a number"));
        if arguments.iter().any(|argument| argument == "--pesto") {
            println!("evaluation: PeSTO");
            residuals(Evaluator::Pesto, 100, all, &positions_source, max_depth, sample);
        } else {
            println!("network: {source}");
            let Network::Psq(_, quantized) = &network;
            residuals(Evaluator::Psq(quantized), network.pawn_percent(), all, &positions_source, max_depth, sample);
        }
        return;
    }
    if arguments.iter().any(|argument| argument == "--search-only") {
        let threads = arguments
            .iter()
            .position(|argument| argument == "--threads")
            .and_then(|at| arguments.get(at + 1))
            .map(|value| value.parse::<usize>().expect("--threads takes a number"))
            .unwrap_or(1);
        let tablebase = bench_tablebase(&arguments);
        let games = search_positions(&arguments);
        if arguments.iter().any(|argument| argument == "--pesto") {
            println!("evaluation: PeSTO");
            search_only(None, depth.unwrap_or(9), threads, &tablebase, &games);
        } else {
            println!("network: {source}");
            search_only(Some(&network), depth.unwrap_or(9), threads, &tablebase, &games);
        }
        return;
    }
    println!("network: {source}");
    // The games each of these reads its positions from: the directory that follows the option.
    let games_dir = |at: usize, option: &str| -> String {
        let dir = arguments.get(at + 1).filter(|value| !value.starts_with("--") && !value.ends_with(".nnue"));
        // Read from the repository root, as every relative path here: cargo runs a bench from its crate.
        dir.map(|dir| from_repository(dir)).unwrap_or_else(|| panic!("{option} takes the directory of the games to read its positions from"))
    };
    if let Some(at) = arguments.iter().position(|argument| argument == "--eval-scale") {
        eval_scale(&network, &games_dir(at, "--eval-scale"));
        return;
    }
    if arguments.iter().any(|argument| argument == "--weights") {
        weights(&network);
        return;
    }
    if let Some(at) = arguments.iter().position(|argument| argument == "--i8") {
        i8_head(&network, &games_dir(at, "--i8"));
        return;
    }
    if let Some(at) = arguments.iter().position(|argument| argument == "--hidden-precision") {
        hidden_precision(&network, &games_dir(at, "--hidden-precision"));
        return;
    }
    if let Some(at) = arguments.iter().position(|argument| argument == "--quantization") {
        quantization(&network, &games_dir(at, "--quantization"));
        return;
    }
    let Network::Psq(psq_network, quantized) = &network;
    if let Some(at) = arguments.iter().position(|argument| argument == "--threat-parts") {
        let dir = arguments.get(at + 1).filter(|value| !value.starts_with("--") && !value.ends_with(".nnue"));
        let parts_boards = dir.map_or_else(|| boards.clone(), |dir| selection_positions(dir));
        threat_parts(psq_network, quantized, &parts_boards, &network, depth.unwrap_or(9));
        return;
    }
    let one_bucket = arguments.iter().any(|argument| argument == "--one-bucket");
    let per_evaluation = time_psq(psq_network, quantized, &boards, one_bucket);
    if psq_network.halfk && psq_network.hidden.len() == 2 {
        let arithmetic = if arguments.iter().any(|argument| argument == "--int8") { "int8" } else { "int16" };
        println!("  first hidden layer in {arithmetic}");
        zero_squares(quantized, &boards, psq_network.pairwise);
    }

    if let Some(depth) = depth {
        compare_searches(&positions, &network, depth, per_evaluation);
    } else {
        println!("\n  pass --depth N to also time a real search with each evaluation -- that ratio,");
        println!("  not the per-evaluation one above, is what a match's wall clock depends on.");
    }
}

/// The share of a screlu network's squares (a pairwise network's products) that are zero over
/// `boards`, both perspectives, as the engine computes them (each lane's activation in `0..=255`,
/// multiplied into `0..=127` rounding to nearest): what skipping zero inputs in the first hidden
/// layer, as Reckless does, could save.
fn zero_squares(quantized: &QuantizedPsqNetwork, boards: &[Board], pairwise: bool) {
    let one = quantized.accumulator_one();
    let shift = quantized.accumulator_shift;
    let rounding = if shift == 0 { 0 } else { 1 << (shift - 1) };
    let (mut zeros, mut all, mut zero_blocks, mut blocks) = (0usize, 0usize, 0usize, 0usize);
    for board in boards {
        for perspective in [Color::White, Color::Black] {
            let activations: Vec<i32> =
                quantized.refresh(board, perspective).iter().map(|&lane| (lane.clamp(0, one) + rounding) >> shift).collect();
            let (a, b) = if pairwise { activations.split_at(activations.len() / 2) } else { (&activations[..], &activations[..]) };
            let squares: Vec<i32> = a.iter().zip(b).map(|(a, b)| (a * b + 256) >> 9).collect();
            zeros += squares.iter().filter(|&&square| square == 0).count();
            all += squares.len();
            // Reckless skips 4-byte chunks, four squares at a time, that are all zero.
            for chunk in squares.chunks(4) {
                zero_blocks += chunk.iter().all(|&square| square == 0) as usize;
                blocks += 1;
            }
        }
    }
    println!(
        "  zero squares: {:.1}% of {all}; all-zero blocks of 4 (what a skip saves): {:.1}%",
        100.0 * zeros as f64 / all as f64,
        100.0 * zero_blocks as f64 / blocks as f64
    );
}

/// `--threat-parts`: a psq_threats network's evaluation split into its parts, on `boards`, and
/// evaluation's share of a search of `network` to `depth` over the seven positions.
fn threat_parts(network: &PsqNetwork, quantized: &QuantizedPsqNetwork, boards: &[Board], whole_network: &Network, depth: u32) {
    assert!(network.threats, "--threat-parts takes a psq_threats network");
    // The same head over the psq rows alone: the evaluation without the threats' work.
    let mut plain = network.clone();
    plain.threats = false;
    plain.transform.truncate(psq::ROWS * network.embedding_dim);
    let plain = QuantizedPsqNetwork::quantize(&plain).unwrap_or_else(|problem| panic!("{problem}"));

    let built: Vec<[Vec<i16>; 2]> = boards.iter().map(|board| quantized.refresh_by_color(board)).collect();
    let plain_built: Vec<[Vec<i16>; 2]> = boards.iter().map(|board| plain.refresh_by_color(board)).collect();
    let whole = time_per_item(boards.len(), |index| {
        black_box(quantized.evaluate_by_color_cp(black_box(&built[index]), &boards[index]));
    });
    let head = time_per_item(boards.len(), |index| {
        black_box(plain.evaluate_by_color_cp(black_box(&plain_built[index]), &boards[index]));
    });
    let mut rows = Vec::with_capacity(64);
    let finding = time_per_call(boards, |board| {
        for perspective in [Color::White, Color::Black] {
            rows.clear();
            psq_threats::push_threat_rows(black_box(board), perspective, &mut rows);
            black_box(&rows);
        }
    });
    let counts: Vec<usize> = boards
        .iter()
        .flat_map(|board| {
            [Color::White, Color::Black].map(|perspective| {
                let mut rows = Vec::new();
                psq_threats::push_threat_rows(board, perspective, &mut rows);
                rows.len()
            })
        })
        .collect();
    let copy_and_add = whole - head - finding;
    let share = |part: f64| 100.0 * part / whole;
    println!("  {} positions; per evaluation, both perspectives:", boards.len());
    println!("    whole           {whole:7.1} ns");
    println!("    head            {head:7.1} ns  {:5.1}%  (the same head, no threats)", share(head));
    println!("    finding         {finding:7.1} ns  {:5.1}%", share(finding));
    println!("    copy and add    {copy_and_add:7.1} ns  {:5.1}%  (what is left)", share(copy_and_add));
    let mean = counts.iter().sum::<usize>() as f64 / counts.len().max(1) as f64;
    println!("  threat rows per perspective: mean {mean:.1}, largest {}", counts.iter().max().copied().unwrap_or(0));

    let tablebase = SyzygyTablebase::new();
    let (mut nodes, started) = (0u64, Instant::now());
    for fen in POSITIONS {
        let (board, _) = Board::from_fen(fen).expect("benchmark FENs are valid");
        let tt = TranspositionTable::new(TT_SIZE_MB);
        let (reports, _) = choose_best_move(&board, depth, None, &[], &tt, &tablebase, Some(whole_network), 0, &AtomicBool::new(false));
        nodes += reports.last().map_or(0, |report| report.nodes);
    }
    let seconds = started.elapsed().as_secs_f64();
    let evaluation_share = 100.0 * nodes as f64 * whole * 1e-9 / seconds;
    println!(
        "  search to depth {depth}: {nodes} nodes in {seconds:.2} s; evaluation at most {evaluation_share:.1}% of it, \
         its threats at most {:.1}%",
        evaluation_share * (whole - head) / whole
    );
}

/// psq's timings, on the quantized network the engine evaluates with (the f32 `network` gives
/// only the shape printed): the scalar from-scratch evaluation (both accumulators and the head), one
/// incremental update, and the head alone. Update plus head is what a search node costs, since the
/// search updates on every move it enters and evaluates from the updated accumulators. Returns that
/// per-node cost as a multiple of the PeSTO evaluation.
///
/// `one_bucket` times the head with the first board's buckets for every position (each position's
/// own accumulators still): a stack network then reads one weight set throughout, so the head's
/// time without bucket cache misses. The head reads a board for nothing else but its side to move.
fn time_psq(network: &PsqNetwork, quantized: &QuantizedPsqNetwork, boards: &[Board], one_bucket: bool) -> f64 {
    println!(
        "  {}, width {} per perspective, hidden {:?}, {} parameters",
        if network.halfk && network.pairwise {
            "halfk_threats_pairwise_hidden"
        } else if network.halfk && !network.hidden.is_empty() {
            "halfk_threats_screlu_hidden"
        } else if network.halfk {
            "halfk_threats_screlu"
        } else if network.material.as_ref().is_some_and(|material| material.len() == PHASE_BUCKETS) {
            "psq_threats_screlu_phase"
        } else if network.material.as_ref().is_some_and(|material| material.len() == COUNT_BUCKETS) {
            "psq_threats_screlu_count"
        } else if network.material.as_ref().is_some_and(|material| material.len() == TAPER_SETS) {
            "psq_threats_screlu_taper"
        } else if network.material.is_some() && network.screlu {
            "psq_threats_screlu_material"
        } else if network.material.is_some() {
            "psq_threats_pairwise_material"
        } else if network.stack.is_some() && network.screlu {
            "psq_threats_screlu_kings"
        } else if network.stack.is_some() && network.threats {
            "psq_threats_stack"
        } else if network.screlu && !network.hidden.is_empty() {
            "psq_threats_screlu_hidden"
        } else if network.screlu {
            "psq_threats_screlu"
        } else if network.threats && network.pairwise && network.hidden.last().is_some_and(|last| network.output.in_dim != last.out_dim) {
            "psq_threats_pairwise_hidden"
        } else if network.threats && network.pairwise {
            "psq_threats_pairwise"
        } else if network.threats {
            "psq_threats"
        } else if network.stack.is_some() {
            "psq_stack"
        } else if network.pairwise {
            "psq_pairwise"
        } else {
            "psq"
        },
        network.embedding_dim,
        match &network.stack {
            // A kings network's one width, a side's.
            Some(stack) if network.screlu => vec![stack.l1[0].out_dim],
            Some(stack) => vec![stack.l1[0].out_dim, stack.l2[0].out_dim],
            None => network.hidden.iter().map(|layer| layer.out_dim).collect::<Vec<usize>>(),
        },
        match &network.stack {
            Some(stack) => {
                let layers = stack.layers().collect::<Vec<&Layer>>();
                parameter_count(&network.transform, &network.transform_bias, &[], &Layer::default())
                    + layers.iter().map(|layer| layer.weight.len() + layer.bias.len()).sum::<usize>()
            }
            None => match &network.material {
                Some(sets) => {
                    parameter_count(&network.transform, &network.transform_bias, &[], &Layer::default())
                        + sets.iter().map(|layer| layer.weight.len() + layer.bias.len()).sum::<usize>()
                }
                None => parameter_count(&network.transform, &network.transform_bias, &network.hidden, &network.output),
            },
        }
    );
    println!("{} positions x {REPETITIONS} repetitions\n", boards.len());

    // One move per position, the first legal one, for the update timing: its parent and child
    // boards, and the parent's accumulators to update from.
    let pairs: Vec<(Board, Board)> = boards
        .iter()
        .filter_map(|board| {
            let mv = *legal_moves(board, board.side_to_move()).first()?;
            let mut child = *board;
            child.make_move(mv);
            Some((*board, child))
        })
        .collect();
    let parent_accumulators: Vec<[Vec<i16>; 2]> =
        pairs.iter().map(|(parent, _)| quantized.refresh_by_color(parent)).collect();
    let mut buffer = quantized.refresh_by_color(&boards[0]);
    let accumulators: Vec<[Vec<i16>; 2]> = boards.iter().map(|board| quantized.refresh_by_color(board)).collect();

    let pesto = time_per_call(boards, |board| {
        black_box(evaluate(black_box(board)));
    });
    let full = time_per_call(boards, |board| {
        black_box(quantized.evaluate_pawns(black_box(board)));
    });
    let update = time_per_item(pairs.len(), |index| {
        let (parent, child) = &pairs[index];
        quantized.update(&parent_accumulators[index], &mut buffer, black_box(parent), black_box(child));
        black_box(&buffer);
    });
    let head = time_per_item(boards.len(), |index| {
        let board = if one_bucket { &boards[0] } else { &boards[index] };
        black_box(quantized.evaluate_by_color_cp(black_box(&accumulators[index]), board));
    });
    let per_node = update + head;

    println!("  PeSTO evaluate             {pesto:>10.1} ns");
    println!("  psq from scratch           {full:>10.1} ns  ({:.0}x PeSTO)", full / pesto);
    println!("  psq incremental update     {update:>10.1} ns");
    println!(
        "  psq head alone             {head:>10.1} ns  (threats recomputed here, if any{})",
        if one_bucket { "; one bucket for every position" } else { "" }
    );
    println!("  psq per search node        {per_node:>10.1} ns  ({:.1}x PeSTO)", per_node / pesto);
    println!("  {:.0} incremental evaluations/second single-threaded", 1e9 / per_node);
    per_node / pesto
}
