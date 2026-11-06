//! The on-disk format for a trained network, and the engine-side representation it loads into.
//!
//! Written by `smithy export` (which has `tch` and can read a `VarStore`) and read by the
//! engine (which must not, and does not, link `libtorch` at all). Both halves live here rather than
//! the writer living beside the training code: a round trip is then a plain library test with no
//! `tch` in it, and the layout has exactly one definition instead of a writer's and a reader's that
//! have to be kept in agreement by hand.
//!
//! This module is the format and the weights only. The forward pass that consumes them is a
//! separate concern and lands separately, so a layout bug and an arithmetic bug can never present
//! as one failing test.

use std::cell::RefCell;
use std::fs;
use std::path::Path;

use crate::board::bitboard::Bitboard;
use crate::board::color::Color;
use crate::board::Board;
use crate::features::{extract_canonical, halfk, psq, psq_threats};
use crate::labels::CEILING_PAWNS;
use crate::search::SearchParameters;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use crate::features::oriented_square;
use crate::stack_buckets::{
    buckets_of_board, output_bucket_of_board, taper, COUNT_BUCKETS, MATERIAL_BUCKETS, OUR_KING_BUCKETS, PHASE_BUCKETS, TAPER_SETS,
    THEIR_KING_BUCKETS,
};
use crate::simd::{
    activation_dot_i16_avx2, activations_i16_avx2, add_assign_avx2, add_assign_i16_avx2, add_rows_saturating_i16_avx2, clipped_copy_avx2,
    clipped_dot_product_pair_avx2, clipped_dot_products_8_avx2, clipped_relu_avx2, dot_product_avx2, dot_products_8_avx2, dots_i16_8_avx2, dots_u8_i8_8_avx2,
    hidden_values_avx2, pairwise_activations_i16_avx2, pairwise_dot_i16_avx2, screlu_activations_i16_avx2, screlu_dot2_i16_avx2,
    screlu_dot_i16_avx2, update_i16_avx2, whole_products_f32_avx2, Activation,
    DOT_PRODUCT_BLOCK,
};

/// Identifies the file at all, before anything is trusted about its contents. Its length is free:
/// every field after it is copied out with `from_le_bytes`, never read in place, so no alignment
/// depends on it.
pub const MAGIC: &[u8; 4] = b"KALO";

/// Bumped whenever the byte layout changes in a way an older reader would misread. A reader that
/// does not recognise a version refuses the file rather than guessing, which is the whole point of
/// carrying one: silently reading a newer layout as if it were this one produces a network that
/// loads cleanly and plays nonsense.
///
/// Version 2 has version 1's layout exactly; it says what the network was trained under: version 2,
/// the logistic at S with the binary cross entropy (`labels::LOGISTIC`), version 1 the atan curve
/// (`labels::NETWORK`). Both are read and played; only version 2 may teach or be trained further,
/// which the smithy checks (`file_version`), and each network's pawn is measured through its own curve
/// (`curve_of_version`).
pub const VERSION: u32 = 2;

/// The version of every network trained under the atan curve, before the logistic.
pub const ATAN_VERSION: u32 = 1;

/// The curve a network of format `version` was trained to mean, what its pawn is measured through.
pub fn curve_of_version(version: u32) -> crate::labels::Curve {
    if version == ATAN_VERSION { crate::labels::Curve::Atan(*crate::labels::NETWORK) } else { crate::labels::LOGISTIC }
}

/// The format version of the network file at `path`, read from its header alone.
pub fn file_version(path: &str) -> Result<u32, String> {
    let bytes = fs::read(path).map_err(|error| format!("{path}: {error}"))?;
    if bytes.len() < MAGIC.len() + 4 || &bytes[..MAGIC.len()] != MAGIC {
        return Err(format!("{path}: not a Kalo Chess network file (bad magic)"));
    }
    Ok(u32::from_le_bytes(bytes[MAGIC.len()..MAGIC.len() + 4].try_into().expect("four bytes")))
}

/// Which feature set the weights were trained under. Stored rather than assumed so a file built for
/// one architecture cannot be loaded as another -- the shapes alone would not always disagree, and
/// a transform sized for one feature set applied to another's rows is exactly the kind of error
/// that produces plausible-looking output.
///
/// The row count is deliberately *not* stored alongside it. It is fully determined by the feature
/// set (`psq::ROWS`), and storing a derivable quantity only creates a second source of truth that
/// can disagree with the first.
///
/// Tag 0 belonged to a retired single-perspective feature set and is deliberately left unused, so
/// an old file carrying it is refused as unknown rather than read as the current psq network and
/// failing only at its shapes. Psq keeps tag 3, the number it had before the rename, so existing
/// `.nnue` files carrying it still load. Tag 4 belonged to the big five, retired, and is left
/// unused for the same reason. Tags 1 and 2 belonged to the old pipeline's halfkp and halfka_v2_hm,
/// which no build ever ran, removed on 2026-11-02; they are left unused too, so that a file of
/// either can never load as a later feature set.
pub const FEATURE_SET_PSQ: u32 = 3;
/// Psq's rows plus each perspective's own threats (`features::psq_threats`), in psq's layout.
pub const FEATURE_SET_PSQ_THREATS: u32 = 5;
/// Psq's rows and transform, each perspective's clipped accumulator split in two halves multiplied
/// element-wise before the head (`PsqNetwork::pairwise`). Not a feature set strictly, the rows being
/// psq's, but a tag of its own, so that a build without pairwise products refuses the file rather
/// than reading it as psq; a header field instead would have meant a new `VERSION` and every
/// existing file refused.
pub const FEATURE_SET_PSQ_PAIRWISE: u32 = 6;
/// Psq pairwise's accumulator and products into a head whose every layer is one of several weight
/// sets, by our king, their king and the material (`PsqNetwork::stack`, `stack_buckets`). The header
/// is the same; the body holds each layer's sets in bucket order. A tag of its own for the reason
/// `FEATURE_SET_PSQ_PAIRWISE` gives.
pub const FEATURE_SET_PSQ_STACK: u32 = 7;
/// Psq_threats' rows and transform with psq pairwise's products: each perspective's clipped
/// accumulator, its threat rows added, split in two halves multiplied element-wise before the head.
/// A tag of its own for the reason `FEATURE_SET_PSQ_PAIRWISE` gives.
pub const FEATURE_SET_PSQ_THREATS_PAIRWISE: u32 = 8;
/// Psq_threats pairwise's accumulator and products into psq_stack's head. The body is psq_stack's
/// over psq_threats' rows. A tag of its own for the reason `FEATURE_SET_PSQ_PAIRWISE` gives.
pub const FEATURE_SET_PSQ_THREATS_STACK: u32 = 9;
/// Psq without castling rights and the en passant file, 768 transform rows: retired on 2026-10-20,
/// having drawn psq, so psq keeps its 780 rows. Kept only so that an old file is refused by name.
pub const FEATURE_SET_PSQ768: u32 = 10;
/// Psq_threats pairwise's accumulator and products into an output that is one of
/// `MATERIAL_BUCKETS` layers, by the material (`PsqNetwork::material`): the body is psq_threats
/// pairwise's, its one output replaced by every material set in bucket order. A tag of its own for
/// the reason `FEATURE_SET_PSQ_PAIRWISE` gives.
pub const FEATURE_SET_PSQ_THREATS_PAIRWISE_MATERIAL: u32 = 11;
/// Psq_threats' rows, transform and head, each accumulator lane's clipped value squared before the
/// head (`PsqNetwork::screlu`), no hidden layer. A tag of its own for the reason
/// `FEATURE_SET_PSQ_PAIRWISE` gives.
pub const FEATURE_SET_PSQ_THREATS_SCRELU: u32 = 12;
/// Psq_threats_screlu's accumulator and squares into an output that is one of `MATERIAL_BUCKETS`
/// layers, by the material (`PsqNetwork::material`): the body is psq_threats_screlu's, its one output
/// replaced by every material set in bucket order. A tag of its own for the reason
/// `FEATURE_SET_PSQ_PAIRWISE` gives.
pub const FEATURE_SET_PSQ_THREATS_SCRELU_MATERIAL: u32 = 13;
/// Psq_threats_screlu_material's layout with `PHASE_BUCKETS` output sets instead, by the game phase
/// (`stack_buckets::phase_bucket`). A tag of its own for the reason `FEATURE_SET_PSQ_PAIRWISE` gives.
pub const FEATURE_SET_PSQ_THREATS_SCRELU_PHASE: u32 = 14;
/// Psq_threats_screlu_material's layout with `COUNT_BUCKETS` output sets instead, by the piece count
/// (`stack_buckets::count_bucket`, Stockfish's key). A tag of its own for the reason
/// `FEATURE_SET_PSQ_PAIRWISE` gives.
pub const FEATURE_SET_PSQ_THREATS_SCRELU_COUNT: u32 = 15;
/// Psq_threats_screlu_material's layout with `TAPER_SETS` output sets instead, middlegame then
/// endgame, both read at every position and blended by the game phase (`stack_buckets::taper`). A
/// tag of its own for the reason `FEATURE_SET_PSQ_PAIRWISE` gives.
pub const FEATURE_SET_PSQ_THREATS_SCRELU_TAPER: u32 = 16;
/// Psq_threats_screlu with a king-sensitive hidden layer beside its output: each perspective's
/// squares through one of `OUR_KING_BUCKETS` (or `THEIR_KING_BUCKETS`) layers picked by that side's
/// own king, then one output over those layers' values and every square (`kings_head`). The body:
/// our side's sets, their side's, the output; one hidden width in the header, a side's. A tag of its
/// own for the reason `FEATURE_SET_PSQ_PAIRWISE` gives. Its first layout, two king layers in a row
/// over both perspectives, was dropped (2026-10-26) with no file of it kept, so the tag was reused.
pub const FEATURE_SET_PSQ_THREATS_SCRELU_KINGS: u32 = 17;
/// Psq_threats_screlu with one hidden layer beside its output: every square through the hidden
/// layer, then one output over its values and every square (`hidden_beside_head`), the king
/// layers of `FEATURE_SET_PSQ_THREATS_SCRELU_KINGS` without the king. The body: the hidden layer,
/// then the output. A tag of its own for the reason `FEATURE_SET_PSQ_PAIRWISE` gives.
pub const FEATURE_SET_PSQ_THREATS_SCRELU_HIDDEN: u32 = 18;
/// Psq_threats_screlu_hidden's head over psq_threats pairwise's products instead of the squares:
/// every product through the hidden layer, then one output over its values and every product. The
/// body as `FEATURE_SET_PSQ_THREATS_SCRELU_HIDDEN`'s. A tag of its own, not
/// `FEATURE_SET_PSQ_THREATS_PAIRWISE` with a hidden layer, whose output reads the hidden layer alone.
pub const FEATURE_SET_PSQ_THREATS_PAIRWISE_HIDDEN: u32 = 19;
/// Psq_threats_screlu with psq's rows replaced by halfk's (`features::halfk`): each perspective's
/// pieces in its own king's bucket, 32 buckets, mirrored left-right with the king, the threat rows
/// mirrored too. The layout psq_threats_screlu's over `halfk::ROWS` rows (`PsqNetwork::halfk`). A
/// king changing bucket or mirror refreshes its perspective's accumulator, through a cache of one
/// accumulator per king square (`RefreshCache`).
pub const FEATURE_SET_HALFK_THREATS_SCRELU: u32 = 20;
/// Halfk_threats_screlu with two hidden layers in place of its one output: every square through
/// the first, clipped, then the second, clipped, then the output, reading the second alone (no
/// skip). The body halfk_threats_screlu's transform, then the two layers and the output, the two
/// widths in the header. A tag of its own for the reason `FEATURE_SET_PSQ_PAIRWISE` gives.
pub const FEATURE_SET_HALFK_THREATS_SCRELU_HIDDEN: u32 = 21;
/// Halfk_threats_screlu_hidden with each perspective's lanes clipped and their two halves multiplied
/// in place of the squares, as psq_threats_pairwise's: the head reads `embedding_dim` products. The
/// body halfk_threats_screlu_hidden's, its first layer half as wide.
pub const FEATURE_SET_HALFK_THREATS_PAIRWISE_HIDDEN: u32 = 22;

/// The transform's row count and the head's input width (in multiples of `embedding_dim`) for each
/// feature set the engine can run, `None` for the others. The one place the kinds' layouts differ:
/// the byte format itself is shared.
fn shape_of(feature_set: u32) -> Option<(usize, usize)> {
    match feature_set {
        FEATURE_SET_PSQ => Some((psq::ROWS, 2)),
        FEATURE_SET_PSQ_THREATS
        | FEATURE_SET_PSQ_THREATS_SCRELU
        | FEATURE_SET_PSQ_THREATS_SCRELU_MATERIAL
        | FEATURE_SET_PSQ_THREATS_SCRELU_PHASE
        | FEATURE_SET_PSQ_THREATS_SCRELU_COUNT
        | FEATURE_SET_PSQ_THREATS_SCRELU_TAPER
        | FEATURE_SET_PSQ_THREATS_SCRELU_KINGS
        | FEATURE_SET_PSQ_THREATS_SCRELU_HIDDEN => Some((psq_threats::ROWS, 2)),
        FEATURE_SET_PSQ_PAIRWISE | FEATURE_SET_PSQ_STACK => Some((psq::ROWS, 1)),
        FEATURE_SET_PSQ_THREATS_PAIRWISE
        | FEATURE_SET_PSQ_THREATS_STACK
        | FEATURE_SET_PSQ_THREATS_PAIRWISE_MATERIAL
        | FEATURE_SET_PSQ_THREATS_PAIRWISE_HIDDEN => Some((psq_threats::ROWS, 1)),
        FEATURE_SET_HALFK_THREATS_SCRELU | FEATURE_SET_HALFK_THREATS_SCRELU_HIDDEN => Some((halfk::ROWS, 2)),
        FEATURE_SET_HALFK_THREATS_PAIRWISE_HIDDEN => Some((halfk::ROWS, 1)),
        _ => None,
    }
}

fn feature_set_name(feature_set: u32) -> &'static str {
    match feature_set {
        FEATURE_SET_PSQ => "psq",
        FEATURE_SET_PSQ768 => "psq768",
        FEATURE_SET_PSQ_THREATS => "psq_threats",
        FEATURE_SET_PSQ_PAIRWISE => "psq_pairwise",
        FEATURE_SET_PSQ_STACK => "psq_stack",
        FEATURE_SET_PSQ_THREATS_PAIRWISE => "psq_threats_pairwise",
        FEATURE_SET_PSQ_THREATS_STACK => "psq_threats_stack",
        FEATURE_SET_PSQ_THREATS_PAIRWISE_MATERIAL => "psq_threats_pairwise_material",
        FEATURE_SET_PSQ_THREATS_SCRELU => "psq_threats_screlu",
        FEATURE_SET_PSQ_THREATS_SCRELU_MATERIAL => "psq_threats_screlu_material",
        FEATURE_SET_PSQ_THREATS_SCRELU_PHASE => "psq_threats_screlu_phase",
        FEATURE_SET_PSQ_THREATS_SCRELU_COUNT => "psq_threats_screlu_count",
        FEATURE_SET_PSQ_THREATS_SCRELU_TAPER => "psq_threats_screlu_taper",
        FEATURE_SET_PSQ_THREATS_SCRELU_KINGS => "psq_threats_screlu_kings",
        FEATURE_SET_PSQ_THREATS_SCRELU_HIDDEN => "psq_threats_screlu_hidden",
        FEATURE_SET_PSQ_THREATS_PAIRWISE_HIDDEN => "psq_threats_pairwise_hidden",
        FEATURE_SET_HALFK_THREATS_SCRELU => "halfk_threats_screlu",
        FEATURE_SET_HALFK_THREATS_SCRELU_HIDDEN => "halfk_threats_screlu_hidden",
        FEATURE_SET_HALFK_THREATS_PAIRWISE_HIDDEN => "halfk_threats_pairwise_hidden",
        _ => "an unknown feature set",
    }
}

/// A `psq_stack` network's head: each layer one weight set per bucket (`stack_buckets`), the first
/// reading the pairwise products, clipped after each hidden layer. A kings network's (screlu) holds
/// its two sides' layers and its output instead (`kings_head`).
#[derive(Debug, Clone, PartialEq)]
pub struct StackLayers {
    /// By our king: `OUR_KING_BUCKETS` layers, `embedding_dim` products in; a kings network's our
    /// side's, `embedding_dim` of our squares in.
    pub l1: Vec<Layer>,
    /// By their king: `THEIR_KING_BUCKETS` layers; a kings network's their side's, `embedding_dim`
    /// of their squares in, as wide as ours.
    pub l2: Vec<Layer>,
    /// By the material: `MATERIAL_BUCKETS` layers, one output each; a kings network's one layer,
    /// over both sides' values clipped then squared, ours first, then every square, own first.
    pub output: Vec<Layer>,
}

impl StackLayers {
    /// Every layer in the order the file holds them: the first layer's sets, the second's, the
    /// output's.
    pub fn layers(&self) -> impl Iterator<Item = &Layer> {
        self.l1.iter().chain(&self.l2).chain(&self.output)
    }

    /// Every set present and every set of a layer the same shape, chained from `head_input` to one
    /// output, through `output_sets` output sets (`MATERIAL_BUCKETS`, or 1 for a kings network).
    fn validate(&self, head_input: usize, output_sets: usize) -> Result<(), String> {
        let stages = [
            ("the stack's first layer", &self.l1, OUR_KING_BUCKETS),
            ("the stack's second layer", &self.l2, THEIR_KING_BUCKETS),
            ("the stack's output", &self.output, output_sets),
        ];
        let mut input_dim = head_input;
        for (index, (what, layers, buckets)) in stages.into_iter().enumerate() {
            if layers.len() != buckets {
                return Err(format!("{what} holds {} weight sets, but has {buckets} buckets", layers.len()));
            }
            let out_dim = layers[0].out_dim;
            for (bucket, layer) in layers.iter().enumerate() {
                layer.validate(&format!("{what}, bucket {bucket}"))?;
                if layer.in_dim != input_dim || layer.out_dim != out_dim {
                    return Err(format!(
                        "{what}, bucket {bucket}: {} -> {}, but the layer is {input_dim} -> {out_dim}",
                        layer.in_dim, layer.out_dim
                    ));
                }
            }
            if index == 2 && out_dim != 1 {
                return Err(format!("{what} produces {out_dim} values, expected exactly 1"));
            }
            input_dim = out_dim;
        }
        Ok(())
    }

    /// A kings network's sets: each side's `embedding_dim` squares in, one width out for both, and
    /// one output over both sides' clipped and squared values and every square.
    fn validate_kings(&self, embedding_dim: usize) -> Result<(), String> {
        let width = self.l1.first().map_or(0, |layer| layer.out_dim);
        let sides = [("our side's layer", &self.l1, OUR_KING_BUCKETS), ("their side's layer", &self.l2, THEIR_KING_BUCKETS)];
        for (what, layers, buckets) in sides {
            if layers.len() != buckets {
                return Err(format!("{what} holds {} weight sets, but has {buckets} buckets", layers.len()));
            }
            for (bucket, layer) in layers.iter().enumerate() {
                layer.validate(&format!("{what}, bucket {bucket}"))?;
                if layer.in_dim != embedding_dim || layer.out_dim != width {
                    return Err(format!(
                        "{what}, bucket {bucket}: {} -> {}, but the layer is {embedding_dim} -> {width}",
                        layer.in_dim, layer.out_dim
                    ));
                }
            }
        }
        if self.output.len() != 1 {
            return Err(format!("the kings' output holds {} weight sets, but has 1", self.output.len()));
        }
        let output = &self.output[0];
        output.validate("the kings' output")?;
        let input = 4 * width + 2 * embedding_dim;
        if output.in_dim != input || output.out_dim != 1 {
            return Err(format!("the kings' output: {} -> {}, but the layer is {input} -> 1", output.in_dim, output.out_dim));
        }
        Ok(())
    }
}

/// Their king's bucket in their own frame: the buckets give both kings from the side to move
/// (`stack_buckets::buckets_of_board`), and their accumulator, which their king's layer reads, is in
/// theirs, the ranks flipped.
fn their_own_frame(their_king: usize) -> usize {
    their_king ^ 56
}

/// A kings network's head in f32: each lane clipped and squared; each side's squares through its
/// own king's layer, ours by `buckets[0]`, theirs by `buckets[1]` in their own frame
/// (`their_own_frame`), each value clipped, then squared beside it; one output over those, ours
/// first, and every square, own first, so the squares reach the output directly, as a screlu
/// network's do. Scalar: the reference.
fn kings_head(own: &[f32], opponent: &[f32], stack: &StackLayers, buckets: [usize; 3]) -> f32 {
    let squares = |lanes: &[f32]| -> Vec<f32> {
        lanes.iter().map(|lane| lane.clamp(0.0, 1.0) * lane.clamp(0.0, 1.0)).collect()
    };
    let (own_squares, opponent_squares) = (squares(own), squares(opponent));
    let output = &stack.output[0];
    let mut inputs = Vec::with_capacity(output.in_dim);
    for (layer, side) in [(&stack.l1[buckets[0]], &own_squares), (&stack.l2[their_own_frame(buckets[1])], &opponent_squares)] {
        let values: Vec<f32> = (0..layer.out_dim)
            .map(|out| {
                let row = &layer.weight[out * layer.in_dim..(out + 1) * layer.in_dim];
                (layer.bias[out] + row.iter().zip(side).map(|(weight, square)| weight * square).sum::<f32>()).clamp(0.0, 1.0)
            })
            .collect();
        inputs.extend(&values);
        inputs.extend(values.iter().map(|value| value * value));
    }
    inputs.extend(&own_squares);
    inputs.extend(&opponent_squares);
    output.bias[0] + output.weight.iter().zip(&inputs).map(|(weight, input)| weight * input).sum::<f32>()
}

/// `layer` over `inputs` in f32, each output clipped to `[0, 1]`. Scalar: the reference.
fn clipped_layer(layer: &Layer, inputs: &[f32]) -> Vec<f32> {
    (0..layer.out_dim)
        .map(|out| {
            let row = &layer.weight[out * layer.in_dim..(out + 1) * layer.in_dim];
            (layer.bias[out] + row.iter().zip(inputs).map(|(weight, input)| weight * input).sum::<f32>()).clamp(0.0, 1.0)
        })
        .collect()
}

/// A head with one hidden layer beside its output in f32, over a screlu network's squares or a
/// pairwise network's products, own first: every one through `hidden`, each value clipped, then
/// squared beside it; one output over those and every input, so the inputs reach the output
/// directly, as they do with no hidden layer. Scalar: the reference.
fn hidden_beside_head(inputs: &[f32], hidden: &Layer, output: &Layer) -> f32 {
    let values: Vec<f32> = (0..hidden.out_dim)
        .map(|out| {
            let row = &hidden.weight[out * hidden.in_dim..(out + 1) * hidden.in_dim];
            (hidden.bias[out] + row.iter().zip(inputs).map(|(weight, input)| weight * input).sum::<f32>()).clamp(0.0, 1.0)
        })
        .collect();
    let inputs = values.iter().copied().chain(values.iter().map(|value| value * value)).chain(inputs.iter().copied());
    output.bias[0] + output.weight.iter().zip(inputs).map(|(weight, input)| weight * input).sum::<f32>()
}

/// One fully-connected layer of the value head, in the orientation a matrix-vector product wants:
/// `weight` is `[out_dim][in_dim]` row-major, so computing one output reads one contiguous run.
/// That is also `tch`'s own `nn::Linear` layout, so exporting a value-head layer is a straight copy
/// with no transpose -- unlike the feature transform, see `PsqNetwork::transform`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layer {
    pub in_dim: usize,
    pub out_dim: usize,
    pub weight: Vec<f32>,
    pub bias: Vec<f32>,
}

impl Layer {
    /// `bias + weight * input`, with no activation -- applying one is the caller's business, since
    /// every hidden layer is followed by a ReLU and the output layer is not.
    ///
    /// One `dot_product_avx2` per output, which is where essentially all of an evaluation's time
    /// goes: the value head is about 172k multiply-adds against the feature transform's 35k adds,
    /// and its first layer alone is 131k of them.
    ///
    /// Accumulated in `f32`, matching what the network was trained in. Accumulating in `f64` here
    /// would be marginally more accurate in isolation and marginally *less* faithful to the thing
    /// this has to reproduce, which is the wrong trade for an implementation whose whole job is to
    /// agree with another one.
    ///
    /// Written into `output`, cleared first, rather than returned: the caller reuses one buffer
    /// across evaluations, since a fresh allocation per evaluation cost about 14% of a whole
    /// search on Windows' heap (samply, caroline, 2026-10-07).
    fn forward_into(&self, input: &[f32], output: &mut Vec<f32>) {
        output.clear();
        for out_index in 0..self.out_dim {
            let row = &self.weight[out_index * self.in_dim..(out_index + 1) * self.in_dim];
            output.push(self.bias[out_index] + dot_product_avx2(row, input));
        }
    }

    /// `forward_into` for a first hidden layer, reading `clamp(own ++ opponent, 0, 1)` in place and
    /// computing eight outputs at a time (`clipped_dot_products_8_avx2`). `out_dim` and each half
    /// must be multiples of 8. The summation order differs from `forward_into`'s, so the two agree
    /// within a tolerance, not bit for bit.
    fn clipped_pair_forward_into(&self, own: &[f32], opponent: &[f32], output: &mut Vec<f32>) {
        output.clear();
        for group in 0..self.out_dim / 8 {
            let rows = &self.weight[group * 8 * self.in_dim..(group + 1) * 8 * self.in_dim];
            let sums = clipped_dot_products_8_avx2(rows, own, opponent);
            for (index, sum) in sums.into_iter().enumerate() {
                output.push(self.bias[group * 8 + index] + sum);
            }
        }
    }

    fn validate(&self, what: &str) -> Result<(), String> {
        if self.weight.len() != self.out_dim * self.in_dim {
            return Err(format!(
                "{what}: weight holds {} values, but {} x {} needs {}",
                self.weight.len(), self.out_dim, self.in_dim, self.out_dim * self.in_dim
            ));
        }
        if self.bias.len() != self.out_dim {
            return Err(format!("{what}: bias holds {} values, but out_dim is {}", self.bias.len(), self.out_dim));
        }
        Ok(())
    }
}

thread_local! {
    /// `PsqNetwork::evaluate_accumulators`'s two activation buffers, the current layer's input
    /// and its output, per thread since the search's threads share one network.
    static HEAD_SCRATCH: RefCell<(Vec<f32>, Vec<f32>)> = const { RefCell::new((Vec::new(), Vec::new())) };
}

/// A network's evaluation in pawns as the engine's centipawns: clamped to `±CEILING_PAWNS`, the
/// bound of the label scale (beyond it `score` is saturated, so training never told those values
/// apart), then `100 *`, rounded.
fn centipawns(pawns: f32) -> i32 {
    let ceiling = CEILING_PAWNS as f32;
    (100.0 * pawns.clamp(-ceiling, ceiling)).round() as i32
}

/// A trained psq network, as the engine holds it: one shared transform applied to both
/// perspectives, concatenated side to move first, clipped ReLU throughout.
///
/// `step` and `learning_rate` are provenance: they say which checkpoint of which arm this came
/// from, so a net found on disk can be traced back to the run that produced it. Nothing reads them
/// as logic.
#[derive(Debug, Clone, PartialEq)]
pub struct PsqNetwork {
    pub embedding_dim: usize,
    /// The feature transform, **transposed** relative to `tch`: `[psq::ROWS][embedding_dim]`
    /// row-major, so one feature row's whole contribution is contiguous.
    ///
    /// This is the one place the format departs from how the weights are stored during training,
    /// and it is worth the departure. The input is sparse -- a few dozen of 780 rows are active --
    /// so the first layer is not a matrix-vector product at all but a sum of the active rows' own
    /// vectors. In `tch`'s `[embedding_dim][rows]` orientation each of those rows is a stride-`rows`
    /// column walk, touching one cache line per float; transposed, each is a single contiguous run.
    /// Paid once at export instead of at every evaluation.
    pub transform: Vec<f32>,
    pub transform_bias: Vec<f32>,
    /// The value head's hidden layers; the first reads `2 * embedding_dim` values (`embedding_dim`
    /// if `pairwise`). Empty for a stack, whose head is `stack`.
    pub hidden: Vec<Layer>,
    /// The output layer; for a stack or a material network, unused and empty (`Layer::default`),
    /// its head being `stack` or `material`.
    pub output: Layer,
    pub step: u64,
    pub learning_rate: f64,
    /// Whether this is a psq_threats network: its transform holds psq's rows and then the threat
    /// rows (`psq_threats::ROWS` in all), and a perspective's accumulator is the sum of two, the
    /// piece-square one, kept incrementally as psq's, and the threat one, recomputed from zero at
    /// every evaluation since a move changes the attacks on many pieces. Psq's row numbers are
    /// the same in both kinds, so everything incremental is shared.
    pub threats: bool,
    /// Whether each perspective's clipped accumulator is split in two halves, `a` (lanes
    /// `0..embedding_dim / 2`) and `b` (the rest), multiplied element-wise before the head, which
    /// then reads `embedding_dim` values, half from each perspective. The accumulator itself is
    /// psq's, so everything incremental is shared too.
    pub pairwise: bool,
    /// Whether each perspective's clipped accumulator lane is squared before the head, which reads
    /// `2 * embedding_dim` squares as psq_threats reads its activations: the squared clipped ReLU of
    /// single-layer networks. Psq_threats only, with no hidden layer, its output one layer, material
    /// sets, or a kings head (`validate`).
    pub screlu: bool,
    /// A `psq_stack` network's head, in place of `hidden` and `output`: pairwise products into
    /// layers bucketed by our king, their king and the material; for a screlu network (kings), each
    /// side's layers by its own king and the one output (`kings_head`). `None` for every other kind.
    pub stack: Option<StackLayers>,
    /// A `psq_threats_pairwise_material` or `psq_threats_screlu_material` network's output, in place
    /// of `output`: one layer per material bucket (`MATERIAL_BUCKETS`, `stack_buckets::material_bucket`),
    /// or, for screlu, one per phase bucket (`PHASE_BUCKETS`, `stack_buckets::phase_bucket`) or per
    /// piece-count bucket (`COUNT_BUCKETS`, `stack_buckets::count_bucket`), the number of sets
    /// telling them apart (`stack_buckets::output_bucket_of_board`), or the two of a taper
    /// (`TAPER_SETS`, middlegame then endgame), both read and blended by the phase,
    /// each reading the pairwise products or the squares, no hidden layer before it. Summed from the trainer's shared part and each
    /// bucket's delta at export. `None` for every other kind.
    pub material: Option<Vec<Layer>>,
    /// Whether the piece rows are halfk's (`features::halfk`) instead of psq's: `halfk::ROWS` rows,
    /// the threat rows after them mirrored with the king. Screlu over threats only, no hidden layer
    /// and a plain output (`validate`).
    pub halfk: bool,
}

/// The screlu head: each lane clipped and squared, own first, into `output`. Scalar: the reference,
/// as the pairwise one is.
fn screlu_head(own: &[f32], opponent: &[f32], output: &Layer) -> f32 {
    let squares = own.iter().chain(opponent).map(|lane| {
        let clipped = lane.clamp(0.0, 1.0);
        clipped * clipped
    });
    output.bias[0] + squares.zip(&output.weight).map(|(square, weight)| square * weight).sum::<f32>()
}

impl PsqNetwork {
    /// The transform's row count.
    fn rows(&self) -> usize {
        if self.halfk {
            halfk::ROWS
        } else if self.threats {
            psq_threats::ROWS
        } else {
            psq::ROWS
        }
    }

    /// `perspective`'s active piece-square rows of `board`.
    fn piece_square_active_rows(&self, board: &Board, perspective: Color) -> Vec<u16> {
        piece_square_active_rows(self.halfk, board, perspective)
    }

    /// What the first head layer reads: both perspectives' accumulators, or their products.
    fn head_input(&self) -> usize {
        if self.pairwise { self.embedding_dim } else { 2 * self.embedding_dim }
    }

    /// One perspective's whole accumulator computed from scratch: the bias plus every active row,
    /// the threats' included. The reference every faster path is tested against.
    pub fn refresh(&self, board: &Board, perspective: Color) -> Vec<f32> {
        let mut accumulator = self.piece_square_refresh(board, perspective);
        if self.threats {
            self.add_threats(&mut accumulator, board, perspective);
        }
        accumulator
    }

    /// One perspective's piece-square accumulator from scratch: the bias plus psq's active rows.
    /// What an incremental update must always agree with.
    fn piece_square_refresh(&self, board: &Board, perspective: Color) -> Vec<f32> {
        let mut accumulator = self.transform_bias.clone();
        for row in self.piece_square_active_rows(board, perspective) {
            self.add_row(&mut accumulator, row as usize);
        }
        accumulator
    }

    /// Adds `perspective`'s threat rows of `board` to `accumulator`: the threat accumulator, summed
    /// into the piece-square one before the activation.
    fn add_threats(&self, accumulator: &mut [f32], board: &Board, perspective: Color) {
        let mut rows = Vec::with_capacity(32);
        push_threat_rows(self.halfk, board, perspective, &mut rows);
        for row in rows {
            self.add_row(accumulator, row as usize);
        }
    }

    /// The network's evaluation in pawns from the two perspectives' accumulators, `own` being the
    /// side to move's, unbounded (the centipawn conversions clamp it). **Already relative to the
    /// side to move**: every feature is oriented for its
    /// perspective's color, every identity is own/opponent, and the side to move's accumulator goes
    /// first, so the value is in negamax's convention with no sign handling anywhere on this path.
    ///
    /// The head's activations live in this thread's `HEAD_SCRATCH`, not in vectors allocated per
    /// call: the buffers reach their largest width on the first evaluation and are only cleared
    /// after that. Evaluation never re-enters itself, so the `RefCell` is never borrowed twice.
    pub fn evaluate_accumulators(&self, own: &[f32], opponent: &[f32]) -> f32 {
        assert!(self.stack.is_none(), "a stack network's head needs the position's buckets: evaluate_stack_accumulators");
        assert!(self.material.is_none(), "a material network's head needs the position's bucket: evaluate_material_accumulators");
        if self.pairwise {
            return self.evaluate_pairwise_accumulators(own, opponent);
        }
        if self.screlu {
            return self.evaluate_screlu_accumulators(own, opponent);
        }
        // No hidden layer and a width the fused kernel covers exactly: the output reads the
        // accumulators in place, clamping as it goes, with no copy at all.
        if self.hidden.is_empty() && own.len().is_multiple_of(DOT_PRODUCT_BLOCK) {
            return self.output.bias[0] + clipped_dot_product_pair_avx2(own, opponent, &self.output.weight);
        }
        HEAD_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            let (current, next) = &mut *scratch;
            let width = own.len();
            let remaining = match self.hidden.first() {
                // The first hidden layer reads the accumulators in place, eight outputs at a time.
                Some(first) if first.out_dim % 8 == 0 && width.is_multiple_of(8) => {
                    first.clipped_pair_forward_into(own, opponent, current);
                    clipped_relu_avx2(current);
                    &self.hidden[1..]
                }
                _ => {
                    // Resized only on the first evaluation of a thread (or a wider network): every
                    // element is overwritten below, so the zeros `resize` writes are never read.
                    current.resize(2 * width, 0.0);
                    clipped_copy_avx2(&mut current[..width], own);
                    clipped_copy_avx2(&mut current[width..], opponent);
                    &self.hidden[..]
                }
            };
            for layer in remaining {
                layer.forward_into(current, next);
                clipped_relu_avx2(next);
                std::mem::swap(current, next);
            }
            self.output.bias[0] + dot_product_avx2(&self.output.weight, current)
        })
    }

    /// `evaluate_accumulators` for a pairwise network: each perspective's halves clipped and
    /// multiplied, own first, then the head. Scalar products: this is the reference smithy's checks
    /// and the benches compare against, not what the search runs.
    fn evaluate_pairwise_accumulators(&self, own: &[f32], opponent: &[f32]) -> f32 {
        if self.pairwise_hidden_beside() {
            let mut products = Vec::with_capacity(self.embedding_dim);
            for lanes in [own, opponent] {
                let (a, b) = lanes.split_at(lanes.len() / 2);
                products.extend(a.iter().zip(b).map(|(a, b)| a.clamp(0.0, 1.0) * b.clamp(0.0, 1.0)));
            }
            return hidden_beside_head(&products, &self.hidden[0], &self.output);
        }
        self.pairwise_head(own, opponent, &self.output)
    }

    /// `evaluate_accumulators` for a screlu network: each lane clipped and squared, own first, into
    /// the output, or through its hidden layer and the output (`hidden_beside_head`), or through two
    /// hidden layers, each clipped, and the output reading the second alone (halfk's). Scalar: the
    /// reference, as the pairwise one is.
    fn evaluate_screlu_accumulators(&self, own: &[f32], opponent: &[f32]) -> f32 {
        let squares = || -> Vec<f32> { own.iter().chain(opponent).map(|lane| lane.clamp(0.0, 1.0) * lane.clamp(0.0, 1.0)).collect() };
        match self.hidden.as_slice() {
            [hidden] => hidden_beside_head(&squares(), hidden, &self.output),
            [first, second] => {
                let values = clipped_layer(first, &squares());
                let values = clipped_layer(second, &values);
                self.output.bias[0] + self.output.weight.iter().zip(&values).map(|(weight, value)| weight * value).sum::<f32>()
            }
            _ => screlu_head(own, opponent, &self.output),
        }
    }

    /// Whether this is a psq_threats_pairwise_hidden network: pairwise over psq_threats, one hidden
    /// layer, and an output reading its values clipped and squared and every product
    /// (`hidden_beside_head`), not the hidden layer alone as a pairwise network's with a hidden layer
    /// does. The output's width tells the two apart: `2 * width + embedding_dim` is never `width`.
    fn pairwise_hidden_beside(&self) -> bool {
        self.pairwise
            && self.threats
            && self.stack.is_none()
            && self.material.is_none()
            && matches!(self.hidden.as_slice(), [hidden] if self.output.in_dim == 2 * hidden.out_dim + self.head_input())
    }

    /// `evaluate_accumulators` for a material network: pairwise's or screlu's, through the output
    /// of the position's material bucket (`stack_buckets::material_bucket`). For a taper, `bucket` is
    /// the phase, blending both sets.
    pub fn evaluate_material_accumulators(&self, own: &[f32], opponent: &[f32], bucket: usize) -> f32 {
        let material = self.material.as_ref().expect("evaluate_material_accumulators on a network with no material sets");
        if self.screlu && material.len() == TAPER_SETS {
            return taper(screlu_head(own, opponent, &material[0]), screlu_head(own, opponent, &material[1]), bucket);
        }
        if self.screlu {
            return screlu_head(own, opponent, &material[bucket]);
        }
        self.pairwise_head(own, opponent, &material[bucket])
    }

    /// The pairwise head: each perspective's halves clipped and multiplied, own first, then the
    /// hidden layers and `output`.
    fn pairwise_head(&self, own: &[f32], opponent: &[f32], output: &Layer) -> f32 {
        HEAD_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            let (current, next) = &mut *scratch;
            current.clear();
            for lanes in [own, opponent] {
                let (a, b) = lanes.split_at(lanes.len() / 2);
                current.extend(a.iter().zip(b).map(|(a, b)| a.clamp(0.0, 1.0) * b.clamp(0.0, 1.0)));
            }
            for layer in &self.hidden {
                layer.forward_into(current, next);
                clipped_relu_avx2(next);
                std::mem::swap(current, next);
            }
            output.bias[0] + dot_product_avx2(&output.weight, current)
        })
    }

    /// `evaluate_accumulators` for a stack network: each perspective's halves clipped and multiplied,
    /// own first, then the three layers of `buckets` (our king, their king, material,
    /// `stack_buckets`), clipped after each hidden one. Scalar products, as the pairwise reference.
    /// A kings network's head is `kings_head`, reading the kings of `buckets` alone.
    pub fn evaluate_stack_accumulators(&self, own: &[f32], opponent: &[f32], buckets: [usize; 3]) -> f32 {
        let stack = self.stack.as_ref().expect("evaluate_stack_accumulators on a network with no stack");
        if self.screlu {
            return kings_head(own, opponent, stack, buckets);
        }
        HEAD_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            let (current, next) = &mut *scratch;
            current.clear();
            for lanes in [own, opponent] {
                let (a, b) = lanes.split_at(lanes.len() / 2);
                current.extend(a.iter().zip(b).map(|(a, b)| a.clamp(0.0, 1.0) * b.clamp(0.0, 1.0)));
            }
            for layer in [&stack.l1[buckets[0]], &stack.l2[buckets[1]]] {
                layer.forward_into(current, next);
                clipped_relu_avx2(next);
                std::mem::swap(current, next);
            }
            let output = &stack.output[buckets[2]];
            output.bias[0] + dot_product_avx2(&output.weight, current)
        })
    }

    /// Both accumulators from scratch, then the head: the reference every faster path is tested
    /// against.
    pub fn evaluate_pawns(&self, board: &Board) -> f32 {
        let side_to_move = board.side_to_move();
        let own = self.refresh(board, side_to_move);
        let opponent = self.refresh(board, side_to_move.opposite());
        if self.stack.is_some() {
            return self.evaluate_stack_accumulators(&own, &opponent, buckets_of_board(board));
        }
        if self.material.is_some() {
            let sets = self.material.as_ref().map_or(0, |material| material.len());
            return self.evaluate_material_accumulators(&own, &opponent, output_bucket_of_board(board, sets));
        }
        self.evaluate_accumulators(&own, &opponent)
    }

    /// `evaluate_pawns` in the engine's centipawns, rounded to `i32` to match `eval::evaluate`'s
    /// signature.
    pub fn evaluate(&self, board: &Board) -> i32 {
        centipawns(self.evaluate_pawns(board))
    }

    fn add_row(&self, accumulator: &mut [f32], row: usize) {
        let start = row * self.embedding_dim;
        add_assign_avx2(accumulator, &self.transform[start..start + self.embedding_dim]);
    }

    /// Every shape agreement the format itself cannot enforce. Run by `write` before a byte is
    /// produced and by `load` after the last one is consumed, so a malformed file and a malformed
    /// in-memory network fail the same way with the same message.
    fn validate(&self) -> Result<(), String> {
        if self.pairwise && !self.embedding_dim.is_multiple_of(2) {
            return Err(format!("a pairwise network splits its accumulator in halves, but embedding_dim {} is odd", self.embedding_dim));
        }
        if self.stack.is_some() && self.material.is_some() {
            return Err("a network's head is a stack or material sets, not both".to_string());
        }
        let halfk_activation = match self.hidden.len() {
            0 => self.screlu && !self.pairwise,
            2 => self.screlu != self.pairwise,
            _ => false,
        };
        if self.halfk && !(self.threats && halfk_activation && self.stack.is_none() && self.material.is_none()) {
            return Err(
                "a halfk network has threats and a plain head: screlu with no hidden layer or two, or pairwise with two".to_string(),
            );
        }
        if self.halfk
            && let [first, second] = self.hidden.as_slice()
        {
            // The quantized head keeps both layers' values in arrays of `MAX_HIDDEN_OUTPUTS`, and
            // reads the first layer's rows eight at a time.
            if !first.out_dim.is_multiple_of(8) || first.out_dim > MAX_HIDDEN_OUTPUTS || second.out_dim > MAX_HIDDEN_OUTPUTS {
                return Err(format!(
                    "a halfk network's hidden layers are {} and {} wide, but the first must be a multiple of 8 and both at most {MAX_HIDDEN_OUTPUTS}",
                    first.out_dim, second.out_dim
                ));
            }
        }
        if self.screlu {
            // The shapes the experiments need: everything else refused until it wins. A kings head
            // is checked below.
            let one_hidden_beside_the_output = self.hidden.len() == 1 && self.stack.is_none() && self.material.is_none();
            let two_hidden = self.halfk && self.hidden.len() == 2;
            if !self.threats || self.pairwise || !(self.hidden.is_empty() || one_hidden_beside_the_output || two_hidden) {
                return Err(
                    "a screlu network is psq_threats with no hidden layer but one beside its plain output, and not pairwise".to_string(),
                );
            }
            if one_hidden_beside_the_output {
                return self.validate_hidden_beside("screlu", "squares");
            }
        }
        if self.pairwise_hidden_beside() {
            return self.validate_hidden_beside("pairwise", "products");
        }
        if let Some(stack) = &self.stack {
            if !(self.pairwise || self.screlu) {
                return Err("a stack network reads pairwise products or squares, and is neither".to_string());
            }
            if !self.hidden.is_empty() || self.output != Layer::default() {
                return Err("a stack network's head is its stack: hidden and output must be empty".to_string());
            }
            validate_transform(self.rows(), self.embedding_dim, &self.transform, &self.transform_bias)?;
            if self.screlu {
                return stack.validate_kings(self.embedding_dim);
            }
            return stack.validate(self.head_input(), MATERIAL_BUCKETS);
        }
        if let Some(material) = &self.material {
            if !(self.pairwise || self.screlu) || !self.threats {
                return Err("a material network is psq_threats pairwise or screlu, its sets reading the products or the squares".to_string());
            }
            if !self.hidden.is_empty() || self.output != Layer::default() {
                return Err("a material network's head is its material sets: hidden and output must be empty".to_string());
            }
            let screlu_only = self.screlu && [PHASE_BUCKETS, COUNT_BUCKETS, TAPER_SETS].contains(&material.len());
            if material.len() != MATERIAL_BUCKETS && !screlu_only {
                return Err(format!(
                    "the material output holds {} weight sets, but has {MATERIAL_BUCKETS} buckets, or for screlu {PHASE_BUCKETS} by phase, {COUNT_BUCKETS} by piece count or {TAPER_SETS} tapered",
                    material.len()
                ));
            }
            validate_transform(self.rows(), self.embedding_dim, &self.transform, &self.transform_bias)?;
            for (bucket, layer) in material.iter().enumerate() {
                layer.validate(&format!("the material output, bucket {bucket}"))?;
                if layer.in_dim != self.head_input() || layer.out_dim != 1 {
                    return Err(format!(
                        "the material output, bucket {bucket}: {} -> {}, but the layer is {} -> 1",
                        layer.in_dim,
                        layer.out_dim,
                        self.head_input()
                    ));
                }
            }
            return Ok(());
        }
        validate_weights(
            self.rows(), self.embedding_dim, self.head_input(), &self.transform, &self.transform_bias, &self.hidden,
            &self.output,
        )
    }

    /// A network with one hidden layer beside its output, `kind` screlu or pairwise: every one of its
    /// `inputs` (squares or products) in, at most `MAX_HIDDEN_OUTPUTS` out (`QuantizedRows`), and the
    /// output over its values clipped and squared and every input.
    fn validate_hidden_beside(&self, kind: &str, inputs: &str) -> Result<(), String> {
        validate_transform(self.rows(), self.embedding_dim, &self.transform, &self.transform_bias)?;
        let hidden = &self.hidden[0];
        hidden.validate(&format!("the {kind} hidden layer"))?;
        let count = self.head_input();
        if hidden.in_dim != count {
            return Err(format!("the {kind} hidden layer reads {} values, but there are {count} {inputs}", hidden.in_dim));
        }
        self.output.validate("output layer")?;
        let input = 2 * hidden.out_dim + count;
        if self.output.in_dim != input || self.output.out_dim != 1 {
            return Err(format!(
                "the {kind} output: {} -> {}, but the layer is {input} -> 1 (the hidden values clipped and squared, then the {inputs})",
                self.output.in_dim, self.output.out_dim
            ));
        }
        Ok(())
    }

    /// Serializes to `path`, little-endian throughout. Validates first: writing a file that cannot
    /// be loaded back helps nobody, and the exporter is the place where a shape mistake is cheapest
    /// to diagnose.
    pub fn write(&self, path: &str) -> Result<(), String> {
        self.validate()?;
        if let Some(stack) = &self.stack {
            // A kings network's one width, a side's.
            let widths = if self.screlu { vec![stack.l1[0].out_dim] } else { vec![stack.l1[0].out_dim, stack.l2[0].out_dim] };
            let feature_set = if self.screlu {
                FEATURE_SET_PSQ_THREATS_SCRELU_KINGS
            } else if self.threats {
                FEATURE_SET_PSQ_THREATS_STACK
            } else {
                FEATURE_SET_PSQ_STACK
            };
            return write_weights(
                path, feature_set, self.embedding_dim, &self.transform, &self.transform_bias, &widths,
                stack.layers(), self.step, self.learning_rate,
            );
        }
        if let Some(material) = &self.material {
            let feature_set = if self.screlu && material.len() == PHASE_BUCKETS {
                FEATURE_SET_PSQ_THREATS_SCRELU_PHASE
            } else if self.screlu && material.len() == COUNT_BUCKETS {
                FEATURE_SET_PSQ_THREATS_SCRELU_COUNT
            } else if self.screlu && material.len() == TAPER_SETS {
                FEATURE_SET_PSQ_THREATS_SCRELU_TAPER
            } else if self.screlu {
                FEATURE_SET_PSQ_THREATS_SCRELU_MATERIAL
            } else {
                FEATURE_SET_PSQ_THREATS_PAIRWISE_MATERIAL
            };
            return write_weights(
                path, feature_set, self.embedding_dim, &self.transform, &self.transform_bias,
                &[], material.iter(), self.step, self.learning_rate,
            );
        }
        let feature_set = if self.halfk && self.pairwise {
            FEATURE_SET_HALFK_THREATS_PAIRWISE_HIDDEN
        } else if self.halfk && !self.hidden.is_empty() {
            FEATURE_SET_HALFK_THREATS_SCRELU_HIDDEN
        } else if self.halfk {
            FEATURE_SET_HALFK_THREATS_SCRELU
        } else if self.screlu && !self.hidden.is_empty() {
            FEATURE_SET_PSQ_THREATS_SCRELU_HIDDEN
        } else if self.screlu {
            FEATURE_SET_PSQ_THREATS_SCRELU
        } else if self.pairwise_hidden_beside() {
            FEATURE_SET_PSQ_THREATS_PAIRWISE_HIDDEN
        } else if self.threats && self.pairwise {
            FEATURE_SET_PSQ_THREATS_PAIRWISE
        } else if self.threats {
            FEATURE_SET_PSQ_THREATS
        } else if self.pairwise {
            FEATURE_SET_PSQ_PAIRWISE
        } else {
            FEATURE_SET_PSQ
        };
        let widths: Vec<usize> = self.hidden.iter().map(|layer| layer.out_dim).collect();
        write_weights(
            path, feature_set, self.embedding_dim, &self.transform, &self.transform_bias, &widths,
            self.hidden.iter().chain([&self.output]), self.step, self.learning_rate,
        )
    }
}

/// The most transform rows one perspective can have active at once: 32 pieces (kings included), 4
/// castling rights and 1 en passant file (`psq::active_rows`). What bounds an accumulator lane.
const MAX_ACTIVE_ROWS: usize = 37;

/// `perspective`'s active piece-square rows of `board`: psq's, or halfk's piece rows.
fn piece_square_active_rows(halfk: bool, board: &Board, perspective: Color) -> Vec<u16> {
    let canonical = extract_canonical(board);
    if halfk { halfk::piece_rows(&canonical, perspective) } else { psq::active_rows(&canonical, perspective) }
}

/// Pushes `perspective`'s threat rows of `board` onto `rows`: psq_threats', or halfk's, after its
/// piece rows and mirrored with the perspective's king.
fn push_threat_rows(halfk: bool, board: &Board, perspective: Color, rows: &mut Vec<u16>) {
    if halfk {
        let (_, mirrored) = halfk_king(board, perspective);
        psq_threats::push_threat_rows_from(board, perspective, halfk::THREAT_ROW, mirrored, rows);
    } else {
        psq_threats::push_threat_rows(board, perspective, rows);
    }
}

/// `perspective`'s halfk king bucket in `board`, and whether its view is mirrored
/// (`halfk::king_bucket`).
fn halfk_king(board: &Board, perspective: Color) -> (usize, bool) {
    halfk::king_bucket(oriented_square(king_square(board, perspective), perspective))
}

fn king_square(board: &Board, color: Color) -> Square {
    board.bitboard(Piece::new(color, PieceType::King)).squares().next().expect("a legal position has one king per side")
}

/// The clipped ReLU's 1.0 as an 8-bit activation: activations run from 0 to 127. With `i16`
/// weights, 512 products of at most `127 * 32767` sum to under `i32::MAX`.
const ACTIVATION_ONE: i32 = 127;

/// A pairwise network's half as an 8-bit activation, as Stockfish's: 0 to 255, one bit more per half
/// than `ACTIVATION_ONE`. Halves are only ever multiplied, never fed to the head, so the top bit is
/// theirs to use. 1.0 at 255 rather than Stockfish's 256, so that the f32 network's clamp to
/// `[0, 1]` is exactly the integer one, and training knows nothing of quantization.
const HALF_ONE: i32 = 255;

/// The head's 1.0 when it reads products (`product_of`): `255 * 255 / 512`, about 127.002. Not a
/// whole number, and no need to be: the head's rescale is an f32 division (`output_value`), which
/// takes it exactly.
const PRODUCT_ONE: f32 = (HALF_ONE * HALF_ONE) as f32 / 512.0;

/// Two halves' activations, each in `0..=255`, multiplied and brought back to `0..=127` with
/// Stockfish's divisor, 512, rounding to nearest (Stockfish truncates, its trainer modelling it;
/// Kalo Chess's does not, and a truncation bias across lanes is what `QuantizedPsqNetwork::rounding`
/// exists to avoid). At most `(65025 + 256) >> 9 = 127`.
fn product_of(a: i32, b: i32) -> i32 {
    (a * b + 256) >> 9
}

/// A whole product's 1.0, `a * b` of two halves at `HALF_ONE`: what a hidden layer beside the output
/// reads (`QuantizedHead::HiddenBeside`), its weights divided by it once at load.
const WHOLE_ONE: f32 = (HALF_ONE * HALF_ONE) as f32;

/// A quantization's identity, for the `RefreshCache`: each `quantize` takes the next number, so a
/// thread's cache built for one network is never read for another, even one loaded at the same
/// address. Equal whatever the numbers, so two quantizations of one network still compare equal:
/// it says which cache entries are valid, not what the network is.
#[derive(Debug, Clone)]
struct CacheId(u64);

impl CacheId {
    fn next() -> CacheId {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        CacheId(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

impl PartialEq for CacheId {
    fn eq(&self, _: &CacheId) -> bool {
        true
    }
}

/// A halfk network's refresh cache: per perspective and king square, one piece-square accumulator
/// and the pieces it sums (`QuantizedPsqNetwork::refresh_through_cache`). 128 entries, about 66 KB
/// at width 256. `network` is the `CacheId` the entries belong to, 0 before any.
struct RefreshCache {
    network: u64,
    entries: Vec<RefreshEntry>,
    /// The rows a refresh adds and removes, reused.
    added: Vec<usize>,
    removed: Vec<usize>,
}

struct RefreshEntry {
    accumulator: Vec<i16>,
    /// Every piece bitboard the accumulator sums, by `color * 6 + piece_type`.
    pieces: [Bitboard; 12],
}

thread_local! {
    /// Each search thread's own `RefreshCache`.
    static REFRESH_CACHE: RefCell<RefreshCache> =
        const { RefCell::new(RefreshCache { network: 0, entries: Vec::new(), added: Vec::new(), removed: Vec::new() }) };
}

thread_local! {
    /// A psq_threats evaluation's two accumulators with their threat rows added, per thread, so the
    /// search's own per-ply accumulators stay piece-square only; and its threat rows, reused, not
    /// allocated at every evaluation (a quarter of one, measured 2026-10-29 by `nnue_bench
    /// --threat-parts`).
    static THREAT_SCRATCH: RefCell<(Vec<i16>, Vec<i16>, Vec<u16>)> = const { RefCell::new((Vec::new(), Vec::new(), Vec::new())) };

    /// A quantized first hidden layer's input activations, computed once per evaluation and read by
    /// every group of eight rows. Per thread, sized on the first evaluation and reused after it,
    /// rather than a stack array of the widest input, which would be zeroed on every call: tried
    /// with 1024 `i16`s, the head took 131 ns instead of 121 on betty, 116 instead of 103 on a
    /// psq_stack (2026-10-15).
    static ACTIVATION_SCRATCH: RefCell<Vec<i16>> = const { RefCell::new(Vec::new()) };

    /// A hidden layer beside the output's input, the whole products or squares (`whole_products`),
    /// per thread as `ACTIVATION_SCRATCH`.
    static WHOLE_SCRATCH: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };

    /// An 8-bit first hidden layer's input, the squares narrowed to `u8` (`QuantizedHead::TwoHiddenI8`),
    /// per thread as `ACTIVATION_SCRATCH`.
    static U8_SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// The output of a head with no hidden layer from its integer sum, the same arithmetic on every
/// path so that they agree exactly. `input_one` is the head's input 1.0: `ACTIVATION_ONE`, or
/// `PRODUCT_ONE` for a pairwise network.
fn output_value(bias: f32, sum: i32, scale: f32, input_one: f32) -> f32 {
    bias + sum as f32 / (input_one * scale)
}

/// One hidden output from its integer sum, clipped, the same arithmetic on every path.
fn hidden_value(bias: f32, sum: i32, scale: f32, input_one: f32) -> f32 {
    (bias + sum as f32 / (input_one * scale)).clamp(0.0, 1.0)
}

/// A psq network quantized when it is loaded: what the engine evaluates with. The f32
/// `PsqNetwork` it comes from stays the from-scratch reference and what `smithy export`'s
/// checks compare against.
///
/// - The accumulator is `i16`, the f32 one scaled by `127 << accumulator_shift`, the largest shift
///   at which no lane can overflow (`quantize`).
/// - Activations are the clipped accumulator shifted down to `0..=127`, rounding to nearest.
/// - With no hidden layer, the output's weights are `i16`, scaled to use the whole range. With one,
///   its weights are `i16` too, each row on its own scale, and the small output layer after it
///   stays f32.
///
/// Measured against f32 over 97,081 positions (`nnue_bench --quantization`, 2026-10-07): caroline
/// 1.16 cp mean and 7.8 cp p99, betty 2.42 and 23.1.
///
/// Every integer sum is exact whatever its order, and each path turns sums into a value with the
/// same `output_value`/`hidden_value`, so the vectorized evaluation equals the scalar one exactly.
///
/// A pairwise network's halves are activations in `0..=255` (`HALF_ONE`), the accumulator scaled
/// by `255 << accumulator_shift`; their products (`product_of`) are the head's input, in `0..=127`,
/// whose 1.0 is `PRODUCT_ONE`. The head is quantized as psq's. A screlu network's lanes are
/// quantized the same way, each one's product with itself its square (`reads_products`).
#[derive(Debug, Clone, PartialEq)]
pub struct QuantizedPsqNetwork {
    embedding_dim: usize,
    pub accumulator_shift: u32,
    threats: bool,
    pairwise: bool,
    screlu: bool,
    /// A halfk network (`PsqNetwork::halfk`): its piece rows halfk's, refreshed through the
    /// thread's `RefreshCache` when a king changes bucket or mirror.
    halfk: bool,
    /// Which quantization this is, for the `RefreshCache`: its entries hold this network's sums.
    cache_id: CacheId,
    transform: Vec<i16>,
    transform_bias: Vec<i16>,
    head: QuantizedHead,
    /// This network's pawn against `piece_value`'s, in percent (`measure_label_percent`): what the
    /// search scales its evaluation margins by. 100 until `Network::from_psq` measures it.
    pawn_percent: i32,
    /// What this network's search was tuned to, from the files beside its own (`read_tuning_beside`).
    /// Its margins are in `piece_value`'s pawn, scaled by `pawn_percent` like the defaults.
    tuning: Tuning,
}

/// A player's tuned search, as the files beside its network (or pesto's tuning directory)
/// give it: `parameters.txt`, else nothing. The older single margin of `margin.txt` was retired
/// when the pawn scale moved to label space: tuned at the old scale, it no longer meant its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tuning {
    Untuned,
    /// Every parameter, from `parameters.txt`.
    Parameters(SearchParameters),
}

impl Tuning {
    /// The parameters this tuning searches with.
    pub fn parameters(&self) -> SearchParameters {
        match *self {
            Tuning::Untuned => SearchParameters::DEFAULT,
            Tuning::Parameters(parameters) => parameters,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum QuantizedHead {
    /// No hidden layer: `bias + sum(activation * weight) / (127 * scale)`.
    Output { weight: Vec<i16>, scale: f32, bias: f32 },
    /// A first hidden layer quantized, row `r` on `scales[r]`; then any further hidden layers and
    /// the output layer in f32, being small. `denominators[r]` is `127 * scales[r]`, computed once
    /// at load exactly as `hidden_value` computes it, for `hidden_values_avx2`.
    Hidden { weight: Vec<i16>, scales: Vec<f32>, denominators: Vec<f32>, biases: Vec<f32>, rest: Vec<Layer>, output: Layer },
    /// A stack (`StackLayers`): each of the first layer's weight sets quantized as `Hidden`'s first
    /// layer, one per our-king bucket, each contiguous; the second layer's and the output's sets in
    /// f32, being small.
    Stack { l1: Vec<QuantizedRows>, l2: Vec<Layer>, output: Vec<Layer> },
    /// A kings network (`kings_head`): each side's layers quantized as `Hidden`'s first layer, one per
    /// king bucket, over that side's squares; the output's weights on the squares, the skip,
    /// quantized as `Output`'s; its weights on the layers' values in f32, `head`, with its bias.
    Kings { ours: Vec<QuantizedRows>, theirs: Vec<QuantizedRows>, skip: Vec<i16>, skip_scale: f32, head: Layer },
    /// A screlu or pairwise network with one hidden layer beside its output (`hidden_beside_head`):
    /// the hidden layer in f32 over the whole products or squares (`whole_products`), its weights
    /// divided by `WHOLE_ONE`; the skip and `head` as `Kings`'. Not quantized as `Hidden`'s first
    /// layer: the head's weights on its values, up to 10 or more against the skip's 1, magnified the
    /// 7-bit products' rounding into a p99 of 50 cp on pairwise_hidden, 13 with the whole products
    /// (2026-10-27).
    HiddenBeside { hidden: Layer, skip: Vec<i16>, skip_scale: f32, head: Layer },
    /// A screlu network with two hidden layers (halfk's): the first quantized as `Hidden`'s over the
    /// squares, the second and the output in f32, being small; computed as a stack's one set
    /// (`stack_head_value`).
    TwoHidden { first: QuantizedRows, second: Layer, output: Layer },
    /// `TwoHidden` with its first layer in 8 bits, Reckless's: the squares as `u8`, the weights `i8`
    /// per row (`QuantizedRowsI8`). Built only on request (`HiddenArithmetic::Int8`), to measure its
    /// speed and its distance from f32 against `TwoHidden`'s before one of the two is kept.
    TwoHiddenI8 { first: QuantizedRowsI8, second: Layer, output: Layer },
    /// Material sets (`PsqNetwork::material`): each bucket's output quantized as `Output`'s, on its
    /// own scale, so a bucket's evaluation is a plain pairwise one with that bucket's weights.
    Material { weights: Vec<Vec<i16>>, scales: Vec<f32>, biases: Vec<f32> },
}

/// An output layer of one value quantized: `i16` weights scaled to use the whole range, the scale,
/// and the f32 bias. Shared by `QuantizedHead::Output` and every set of `QuantizedHead::Material`.
fn quantize_output(layer: &Layer) -> (Vec<i16>, f32, f32) {
    let largest = layer.weight.iter().fold(0.0f32, |most, weight| most.max(weight.abs()));
    let scale = if largest > 0.0 { i16::MAX as f32 / largest } else { 1.0 };
    let weight = layer.weight.iter().map(|weight| (weight * scale).round() as i16).collect();
    (weight, scale, layer.bias[0])
}

/// An output over `head_inputs` hidden values and then the squares, split: the skip, its weights on
/// the squares quantized as an output's (`quantize_output`), and its scale; and the f32 layer of its
/// weights on the hidden values, with the output's bias.
fn split_skip(output: &Layer, head_inputs: usize) -> (Vec<i16>, f32, Layer) {
    let skip = Layer { in_dim: output.in_dim - head_inputs, out_dim: 1, weight: output.weight[head_inputs..].to_vec(), bias: vec![0.0] };
    let (skip, skip_scale, _) = quantize_output(&skip);
    let head = Layer { in_dim: head_inputs, out_dim: 1, weight: output.weight[..head_inputs].to_vec(), bias: output.bias.clone() };
    (skip, skip_scale, head)
}

/// A first hidden layer quantized: `i16` weights, row `r` on `scales[r]`, `denominators[r]` its
/// input 1.0 times `scales[r]` (`hidden_value`), and the f32 biases.
#[derive(Debug, Clone, PartialEq)]
struct QuantizedRows {
    weight: Vec<i16>,
    scales: Vec<f32>,
    denominators: Vec<f32>,
    biases: Vec<f32>,
}

impl QuantizedRows {
    /// `layer` quantized row by row, for a head whose input 1.0 is `input_one`. i16, not i8: with i8,
    /// each row's 127 levels left betty at 3.8 cp mean and 34 cp p99 away from f32, against
    /// caroline's 1.2 and 7.8 (2026-10-07).
    fn quantize(layer: &Layer, input_one: f32) -> Result<QuantizedRows, String> {
        if layer.out_dim > MAX_HIDDEN_OUTPUTS {
            return Err(format!("a first hidden layer of {} outputs, beyond the {MAX_HIDDEN_OUTPUTS} the head holds", layer.out_dim));
        }
        let mut weight = Vec::with_capacity(layer.weight.len());
        let mut scales = Vec::with_capacity(layer.out_dim);
        for row in layer.weight.chunks(layer.in_dim) {
            let largest = row.iter().fold(0.0f32, |most, weight| most.max(weight.abs()));
            let scale = if largest > 0.0 { i16::MAX as f32 / largest } else { 1.0 };
            weight.extend(row.iter().map(|weight| (weight * scale).round() as i16));
            scales.push(scale);
        }
        let denominators = scales.iter().map(|&scale| input_one * scale).collect();
        Ok(QuantizedRows { weight, scales, denominators, biases: layer.bias.clone() })
    }
}

/// A first hidden layer quantized to 8 bits: `i8` weights, row `r` on `scales[r]` (its largest weight
/// at 127), `denominators[r]` as `QuantizedRows`', and the f32 biases.
#[derive(Debug, Clone, PartialEq)]
struct QuantizedRowsI8 {
    weight: Vec<i8>,
    scales: Vec<f32>,
    denominators: Vec<f32>,
    biases: Vec<f32>,
}

impl QuantizedRowsI8 {
    fn quantize(layer: &Layer, input_one: f32) -> Result<QuantizedRowsI8, String> {
        if layer.out_dim > MAX_HIDDEN_OUTPUTS {
            return Err(format!("a first hidden layer of {} outputs, beyond the {MAX_HIDDEN_OUTPUTS} the head holds", layer.out_dim));
        }
        let mut weight = Vec::with_capacity(layer.weight.len());
        let mut scales = Vec::with_capacity(layer.out_dim);
        for row in layer.weight.chunks(layer.in_dim) {
            let largest = row.iter().fold(0.0f32, |most, weight| most.max(weight.abs()));
            let scale = if largest > 0.0 { i8::MAX as f32 / largest } else { 1.0 };
            weight.extend(row.iter().map(|weight| (weight * scale).round() as i8));
            scales.push(scale);
        }
        let denominators = scales.iter().map(|&scale| input_one * scale).collect();
        Ok(QuantizedRowsI8 { weight, scales, denominators, biases: layer.bias.clone() })
    }
}

/// The arithmetic of a two-hidden head's first layer (`QuantizedHead::TwoHidden`): `Int16`, the
/// default, or `Int8`, Reckless's, built to be measured against it. Every other head ignores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenArithmetic {
    Int16,
    Int8,
}

/// An 8-bit two-hidden head's value from its `u8` squares: the first layer's integer sums, eight rows
/// at a time where 32 inputs a block and its outputs allow, their values, bit for bit the same
/// either way (`hidden_values_avx2`), then `second_layer_and_output`.
fn two_hidden_i8_value(squares: &[u8], vectorized: bool, first: &QuantizedRowsI8, second: &Layer, output: &Layer) -> f32 {
    let in_dim = squares.len();
    let out_dim = first.biases.len();
    let vectorized = vectorized && in_dim.is_multiple_of(32) && out_dim.is_multiple_of(8);
    let mut sums = [0i32; MAX_HIDDEN_OUTPUTS];
    let sums = &mut sums[..out_dim];
    for (group, group_sums) in sums.chunks_mut(8).enumerate() {
        if vectorized {
            group_sums.copy_from_slice(&dots_u8_i8_8_avx2(&first.weight[group * 8 * in_dim..(group + 1) * 8 * in_dim], squares));
        } else {
            for (index, sum) in group_sums.iter_mut().enumerate() {
                let row = &first.weight[(group * 8 + index) * in_dim..(group * 8 + index + 1) * in_dim];
                *sum = squares.iter().zip(row).map(|(&square, &weight)| square as i32 * weight as i32).sum();
            }
        }
    }
    let mut values = [0.0f32; MAX_HIDDEN_OUTPUTS];
    let values = &mut values[..out_dim];
    if vectorized {
        hidden_values_avx2(sums, &first.denominators, &first.biases, values);
    } else {
        for row in 0..out_dim {
            values[row] = hidden_value(first.biases[row], sums[row], first.scales[row], PRODUCT_ONE);
        }
    }
    second_layer_and_output(values, second, output)
}

/// A head's second hidden layer and its output in f32, from the first layer's clipped `values`.
/// Eight rows at a time where its shape allows, one reduction per eight outputs instead of a
/// horizontal sum per output: the values are already in [0, 1], so the kernel's clamp of its input
/// changes nothing. Chosen by shape alone, never by a path's `vectorized`, so that the scalar and
/// vectorized paths still agree exactly.
fn second_layer_and_output(values: &[f32], second: &Layer, output: &Layer) -> f32 {
    let out_dim = values.len();
    let mut second_values = [0.0f32; MAX_HIDDEN_OUTPUTS];
    let second_values = &mut second_values[..second.out_dim];
    if second.out_dim.is_multiple_of(8) && out_dim.is_multiple_of(16) {
        let (low, high) = values.split_at(out_dim / 2);
        for (group, group_values) in second_values.chunks_mut(8).enumerate() {
            let rows = &second.weight[group * 8 * out_dim..(group + 1) * 8 * out_dim];
            let sums = clipped_dot_products_8_avx2(rows, low, high);
            for (index, value) in group_values.iter_mut().enumerate() {
                *value = (second.bias[group * 8 + index] + sums[index]).clamp(0.0, 1.0);
            }
        }
    } else {
        for (index, value) in second_values.iter_mut().enumerate() {
            let row = &second.weight[index * out_dim..(index + 1) * out_dim];
            *value = (second.bias[index] + dot_product_avx2(row, values)).clamp(0.0, 1.0);
        }
    }
    output.bias[0] + dot_product_avx2(&output.weight, second_values)
}

/// The most outputs a quantized first hidden layer may have: its sums and values live in stack
/// arrays this long, zeroed on every evaluation, so it is kept small. `quantize` refuses more.
const MAX_HIDDEN_OUTPUTS: usize = 64;

impl QuantizedPsqNetwork {
    /// Quantizes `network`: the transform and the first head layer. Refuses an accumulator whose
    /// worst case does not fit `i16` even unshifted.
    ///
    /// The shift: a lane's worst case is its bias plus its `MAX_ACTIVE_ROWS` largest piece-square
    /// row weights, all in magnitude, plus half a unit of rounding per term; the shift is the
    /// largest at which the worst lane still fits `i16`. A loose bound (no position has every lane
    /// at its worst), so the accumulator never overflows but may use less of its range than it
    /// could.
    ///
    /// A psq_threats network's threat rows are not in the bound: they are never kept in the
    /// incremental `i16` accumulator, only added at evaluation, in `i32`, saturating
    /// (`add_rows_saturating_i16_avx2`). Counting them in, up to 128 rows at their largest, refused
    /// dorothy's generation 1 (2026-10-16) though no position comes near it. Each of their weights
    /// must still fit `i16` once scaled, which the narrower bound no longer implies.
    ///
    /// A halfk network's lanes are bounded bucket by bucket: a perspective's piece rows all lie in
    /// its own king's bucket, so a lane's worst case is the worst over the buckets of each one's
    /// largest rows, far tighter than the largest over all 22,528.
    pub fn quantize(network: &PsqNetwork) -> Result<QuantizedPsqNetwork, String> {
        QuantizedPsqNetwork::quantize_with(network, HiddenArithmetic::Int16)
    }

    /// `quantize`, a two-hidden head's first layer in `arithmetic`.
    pub fn quantize_with(network: &PsqNetwork, arithmetic: HiddenArithmetic) -> Result<QuantizedPsqNetwork, String> {
        let dim = network.embedding_dim;
        // Each group's rows start at `group * size`: halfk's buckets, or psq's rows as one group.
        let (groups, size) = if network.halfk { (halfk::BUCKETS, halfk::FACTOR_ROWS) } else { (1, psq::ROWS) };
        let mut worst = 0.0f32;
        for lane in 0..dim {
            for group in 0..groups {
                let rows = group * size..(group + 1) * size;
                let mut column: Vec<f32> = rows.map(|row| network.transform[row * dim + lane].abs()).collect();
                column.sort_by(|a, b| b.total_cmp(a));
                let bound = network.transform_bias[lane].abs() + column[..MAX_ACTIVE_ROWS.min(column.len())].iter().sum::<f32>();
                worst = worst.max(bound);
            }
        }
        let largest_weight = network.transform.iter().fold(0.0f32, |most, weight| most.max(weight.abs()));
        let rounding = 0.5 * (MAX_ACTIVE_ROWS + 1) as f32;
        let reads_products = network.pairwise || network.screlu;
        let one = if reads_products { HALF_ONE } else { ACTIVATION_ONE };
        // The scale itself is an `i16` too (`Activation::one`): 255 << 8 would not be.
        let lanes_fit = |shift: u32| -> bool {
            (one << shift) <= i16::MAX as i32 && (one << shift) as f32 * worst + rounding <= i16::MAX as f32
        };
        let weights_fit = |shift: u32| -> bool {
            (one << shift) as f32 * largest_weight + 0.5 <= i16::MAX as f32
        };
        if !lanes_fit(0) {
            return Err(format!("an accumulator lane can reach {worst:.1}, beyond i16 even at scale {one}"));
        }
        if !weights_fit(0) {
            return Err(format!("a transform weight of {largest_weight:.1} is beyond i16 even at scale {one}"));
        }
        let fits = |shift: u32| -> bool {
            lanes_fit(shift) && weights_fit(shift)
        };
        let mut accumulator_shift = 0;
        while accumulator_shift < 8 && fits(accumulator_shift + 1) {
            accumulator_shift += 1;
        }
        let scale = (one << accumulator_shift) as f32;
        let input_one = if reads_products { PRODUCT_ONE } else { ACTIVATION_ONE as f32 };
        let to_i16 = |values: &[f32]| -> Vec<i16> { values.iter().map(|value| (value * scale).round() as i16).collect() };

        let head = match network.hidden.as_slice() {
            [] if network.screlu && network.stack.is_some() => {
                let stack = network.stack.as_ref().expect("checked");
                let sets = |layers: &[Layer]| {
                    layers.iter().map(|layer| QuantizedRows::quantize(layer, input_one)).collect::<Result<Vec<_>, _>>()
                };
                let (skip, skip_scale, head) = split_skip(&stack.output[0], 4 * stack.l1[0].out_dim);
                QuantizedHead::Kings { ours: sets(&stack.l1)?, theirs: sets(&stack.l2)?, skip, skip_scale, head }
            }
            [] if network.stack.is_some() => {
                let stack = network.stack.as_ref().expect("checked");
                let l1 = stack.l1.iter().map(|layer| QuantizedRows::quantize(layer, input_one)).collect::<Result<Vec<_>, _>>()?;
                if stack.l2[0].out_dim > MAX_HIDDEN_OUTPUTS {
                    return Err(format!(
                        "a second stack layer of {} outputs, beyond the {MAX_HIDDEN_OUTPUTS} the head holds",
                        stack.l2[0].out_dim
                    ));
                }
                QuantizedHead::Stack { l1, l2: stack.l2.clone(), output: stack.output.clone() }
            }
            [] if network.material.is_some() => {
                let sets: Vec<(Vec<i16>, f32, f32)> = network.material.as_ref().expect("checked").iter().map(quantize_output).collect();
                let scales = sets.iter().map(|set| set.1).collect();
                let biases = sets.iter().map(|set| set.2).collect();
                QuantizedHead::Material { weights: sets.into_iter().map(|set| set.0).collect(), scales, biases }
            }
            [] => {
                let (weight, scale, bias) = quantize_output(&network.output);
                QuantizedHead::Output { weight, scale, bias }
            }
            // Halfk's, over the squares or the products alike; psq pairwise's two layers are `Hidden`.
            [first, second] if network.halfk && arithmetic == HiddenArithmetic::Int8 => QuantizedHead::TwoHiddenI8 {
                first: QuantizedRowsI8::quantize(first, input_one)?,
                second: second.clone(),
                output: network.output.clone(),
            },
            [first, second] if network.halfk => QuantizedHead::TwoHidden {
                first: QuantizedRows::quantize(first, input_one)?,
                second: second.clone(),
                output: network.output.clone(),
            },
            [hidden] if network.screlu || network.pairwise_hidden_beside() => {
                let (skip, skip_scale, head) = split_skip(&network.output, 2 * hidden.out_dim);
                let hidden = Layer {
                    in_dim: hidden.in_dim,
                    out_dim: hidden.out_dim,
                    weight: hidden.weight.iter().map(|weight| weight / WHOLE_ONE).collect(),
                    bias: hidden.bias.clone(),
                };
                QuantizedHead::HiddenBeside { hidden, skip, skip_scale, head }
            }
            [hidden, rest @ ..] => {
                let QuantizedRows { weight, scales, denominators, biases } = QuantizedRows::quantize(hidden, input_one)?;
                QuantizedHead::Hidden { weight, scales, denominators, biases, rest: rest.to_vec(), output: network.output.clone() }
            }
        };

        Ok(QuantizedPsqNetwork {
            embedding_dim: dim,
            accumulator_shift,
            threats: network.threats,
            pairwise: network.pairwise,
            screlu: network.screlu,
            halfk: network.halfk,
            cache_id: CacheId::next(),
            transform: to_i16(&network.transform),
            transform_bias: to_i16(&network.transform_bias),
            head,
            pawn_percent: 100,
            tuning: Tuning::Untuned,
        })
    }

    /// This network's pawn against `piece_value`'s, in percent, as measured when it loaded.
    pub fn pawn_percent(&self) -> i32 {
        self.pawn_percent
    }

    /// The accumulator's scale: the f32 accumulator's 1.0.
    pub fn accumulator_one(&self) -> i32 {
        let one = if self.reads_products() { HALF_ONE } else { ACTIVATION_ONE };
        one << self.accumulator_shift
    }

    /// Whether the head reads products of lanes in `0..=255` (`product_of`): a pairwise network's
    /// two halves, or a screlu network's lane with itself.
    fn reads_products(&self) -> bool {
        self.pairwise || self.screlu
    }

    /// The head's input 1.0, what its integer sums are divided by along with the weights' scale.
    fn input_one(&self) -> f32 {
        if self.reads_products() { PRODUCT_ONE } else { ACTIVATION_ONE as f32 }
    }

    /// Half a step of the shift down to an activation, added first so that the shift rounds to
    /// nearest. Truncating instead put every active lane half a step low on average, a bias that
    /// adds up across lanes rather than cancelling: 2.1 cp mean and 14 cp p99 on caroline, 3.9 and
    /// 37 on betty (2026-10-07). `0..=one` plus it, shifted, still stays within `0..=127`.
    fn rounding(&self) -> i32 {
        if self.accumulator_shift == 0 { 0 } else { 1 << (self.accumulator_shift - 1) }
    }

    /// The activation parameters the vectorized head needs.
    fn activation(&self) -> Activation {
        Activation {
            one: self.accumulator_one() as i16,
            rounding: self.rounding() as i16,
            shift: self.accumulator_shift,
        }
    }

    /// One accumulator lane as its activation, in `0..=127` (`0..=255` for a pairwise network's
    /// halves and a screlu network's lanes).
    fn activation_of(&self, lane: i16) -> i32 {
        ((lane as i32).clamp(0, self.accumulator_one()) + self.rounding()) >> self.accumulator_shift
    }

    /// One perspective's whole quantized accumulator from scratch, threats included, each lane
    /// widened to `i32` so that a caller can check it against the `i16` range: with threats, the
    /// sum may exceed it, and is saturated where read. The reference the incremental path is tested
    /// against.
    pub fn refresh(&self, board: &Board, perspective: Color) -> Vec<i32> {
        let dim = self.embedding_dim;
        let mut rows = piece_square_active_rows(self.halfk, board, perspective);
        if self.threats {
            push_threat_rows(self.halfk, board, perspective, &mut rows);
        }
        let mut accumulator: Vec<i32> = self.transform_bias.iter().map(|&value| value as i32).collect();
        for row in rows {
            let start = row as usize * dim;
            for (lane, &weight) in accumulator.iter_mut().zip(&self.transform[start..start + dim]) {
                *lane += weight as i32;
            }
        }
        accumulator
    }

    /// Both piece-square accumulators from scratch, indexed by color (`[White, Black]`), not
    /// own/opponent: an accumulator belongs to a fixed color, so a move never swaps them, and only
    /// the order they are read in depends on the side to move. The threats are not in them:
    /// `evaluate_by_color_cp` adds those.
    pub fn refresh_by_color(&self, board: &Board) -> [Vec<i16>; 2] {
        [Color::White, Color::Black].map(|perspective| {
            let mut accumulator = self.transform_bias.clone();
            for row in piece_square_active_rows(self.halfk, board, perspective) {
                self.add_row(&mut accumulator, row as usize);
            }
            accumulator
        })
    }

    /// `perspective`'s halfk piece-square accumulator of `board`, into `accumulator`, through this
    /// thread's `RefreshCache`: its entry for the king's square holds the sums of the pieces it last
    /// saw there, so only the pieces that differ since are added or removed, rather than all 32.
    /// Stockfish's "Finny tables". The entry is left at `board`'s pieces.
    fn refresh_through_cache(&self, accumulator: &mut [i16], board: &Board, perspective: Color) {
        let king = king_square(board, perspective);
        let (bucket, mirrored) = halfk::king_bucket(oriented_square(king, perspective));
        REFRESH_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if cache.network != self.cache_id.0 {
                cache.network = self.cache_id.0;
                cache.entries.clear();
                cache.entries.resize_with(2 * 64, || RefreshEntry {
                    accumulator: self.transform_bias.clone(),
                    pieces: [Bitboard::EMPTY; 12],
                });
            }
            let RefreshCache { entries, added, removed, .. } = &mut *cache;
            let entry = &mut entries[perspective as usize * 64 + king as usize];
            added.clear();
            removed.clear();
            for color in [Color::White, Color::Black] {
                for piece_type in PieceType::ALL {
                    let index = color as usize * 6 + piece_type as usize;
                    let now = board.bitboard(Piece::new(color, piece_type));
                    let changed = entry.pieces[index] ^ now;
                    if changed == Bitboard::EMPTY {
                        continue;
                    }
                    let is_own = color == perspective;
                    let row = |square: Square| halfk::piece_row(bucket, mirrored, piece_type, is_own, oriented_square(square, perspective));
                    removed.extend((entry.pieces[index] & changed).squares().map(&row));
                    added.extend((now & changed).squares().map(&row));
                    entry.pieces[index] = now;
                }
            }
            update_i16_avx2(accumulator, &entry.accumulator, &self.transform, added.as_slice(), removed.as_slice());
            entry.accumulator.copy_from_slice(accumulator);
        })
    }

    /// Sets `child_accumulators` to `parent_accumulators` updated from `parent` to `child`
    /// (`ChangedRows`), without recomputing them: the piece-square accumulators only, whichever
    /// the kind. Overwritten rather than returned so a search keeps one buffer per ply. Integer
    /// arithmetic, so it equals a from-scratch refresh exactly, however long the line.
    ///
    /// The boards are compared once for both perspectives, and each perspective's accumulator is
    /// written in a single pass (`update_i16_avx2`): no copy of the parent first, and no
    /// read-modify-write of the whole accumulator per changed row.
    ///
    /// For a halfk network, a perspective whose king changed bucket or mirror has every piece row
    /// changed, so it is refreshed instead (`refresh_through_cache`); the other updates as psq's.
    pub fn update(&self, parent_accumulators: &[Vec<i16>; 2], child_accumulators: &mut [Vec<i16>; 2], parent: &Board, child: &Board) {
        let mut refreshed = [false; 2];
        let changes = if self.halfk {
            let kings = [Color::White, Color::Black].map(|perspective| halfk_king(child, perspective));
            for perspective in [Color::White, Color::Black] {
                let index = perspective as usize;
                if halfk_king(parent, perspective) != kings[index] {
                    self.refresh_through_cache(&mut child_accumulators[index], child, perspective);
                    refreshed[index] = true;
                }
            }
            ChangedRows::between_halfk(parent, child, kings)
        } else {
            ChangedRows::between(parent, child)
        };
        for perspective in [Color::White, Color::Black] {
            let index = perspective as usize;
            if refreshed[index] {
                continue;
            }
            update_i16_avx2(
                &mut child_accumulators[index],
                &parent_accumulators[index],
                &self.transform,
                changes.added(perspective),
                changes.removed(perspective),
            );
        }
    }

    fn add_row(&self, accumulator: &mut [i16], row: usize) {
        let start = row * self.embedding_dim;
        add_assign_i16_avx2(accumulator, &self.transform[start..start + self.embedding_dim]);
    }

    /// The value of `board` in centipawns from its piece-square accumulators indexed by color, the
    /// side to move's first; for a psq_threats network, each with its threat rows added first.
    /// Vectorized where the shapes allow (`evaluate_lanes`).
    pub fn evaluate_by_color_cp(&self, accumulators: &[Vec<i16>; 2], board: &Board) -> i32 {
        let side_to_move = board.side_to_move();
        let own = &accumulators[side_to_move as usize];
        let opponent = &accumulators[side_to_move.opposite() as usize];
        if !self.threats {
            return centipawns(self.evaluate_lanes(own, opponent, true, self.buckets(board)));
        }
        THREAT_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            let (own_with_threats, opponent_with_threats, rows) = &mut *scratch;
            for (copy, lanes, perspective) in
                [(&mut *own_with_threats, own, side_to_move), (&mut *opponent_with_threats, opponent, side_to_move.opposite())]
            {
                copy.resize(lanes.len(), 0);
                rows.clear();
                push_threat_rows(self.halfk, board, perspective, rows);
                // Summed in i32 and saturated: the threat rows are not in `quantize`'s bound.
                add_rows_saturating_i16_avx2(copy, lanes, &self.transform, rows);
            }
            centipawns(self.evaluate_lanes(own_with_threats, opponent_with_threats, true, self.buckets(board)))
        })
    }

    /// `board`'s stack buckets (`stack_buckets::buckets_of_board`) for a stack network, its kings
    /// alone for a kings network, and its material bucket alone in the third place for a material
    /// network, read only by the head; zeros for every other kind, which spares them the count.
    fn buckets(&self, board: &Board) -> [usize; 3] {
        match self.head {
            QuantizedHead::Kings { .. } => {
                let [our_king, their_king, _] = buckets_of_board(board);
                [our_king, their_king, 0]
            }
            QuantizedHead::Stack { .. } => buckets_of_board(board),
            QuantizedHead::Material { ref weights, .. } => [0, 0, output_bucket_of_board(board, weights.len())],
            _ => [0; 3],
        }
    }

    /// The evaluation in pawns from the two perspectives' whole accumulators, `own` the side to
    /// move's. `vectorized` takes the AVX2 kernels where the shapes allow them (lanes a multiple
    /// of 16; a hidden layer's outputs a multiple of 8), and the scalar loops otherwise: both
    /// produce the same integer sums and turn them into a value the same way, so they agree
    /// exactly. `buckets` are read by a stack head only (`buckets`).
    fn evaluate_lanes(&self, own: &[i16], opponent: &[i16], vectorized: bool, buckets: [usize; 3]) -> f32 {
        if self.pairwise {
            return self.evaluate_pairwise_lanes(own, opponent, vectorized, buckets);
        }
        if self.screlu {
            return self.evaluate_screlu_lanes(own, opponent, vectorized, buckets);
        }
        let vectorized = vectorized && own.len().is_multiple_of(16);
        let scalar_dot = |weights: &[i16]| -> i32 {
            own.iter().chain(opponent).zip(weights).map(|(&lane, &weight)| self.activation_of(lane) * weight as i32).sum()
        };
        match &self.head {
            QuantizedHead::Output { weight, scale, bias } => {
                let sum = if vectorized { activation_dot_i16_avx2(own, opponent, weight, self.activation()) } else { scalar_dot(weight) };
                output_value(*bias, sum, *scale, self.input_one())
            }
            QuantizedHead::Hidden { weight, biases, .. } => {
                let in_dim = own.len() + opponent.len();
                let out_dim = biases.len();
                let vectorized = vectorized && out_dim % 8 == 0;

                // The first layer's integer sums: in groups of eight rows over activations computed
                // once, or row by row in scalar.
                let mut sums = [0i32; MAX_HIDDEN_OUTPUTS];
                let sums = &mut sums[..out_dim];
                if vectorized {
                    ACTIVATION_SCRATCH.with(|scratch| {
                        let mut activations = scratch.borrow_mut();
                        activations.resize(in_dim, 0);
                        let (own_part, opponent_part) = activations.split_at_mut(own.len());
                        activations_i16_avx2(own_part, own, self.activation());
                        activations_i16_avx2(opponent_part, opponent, self.activation());
                        for (group, group_sums) in sums.chunks_mut(8).enumerate() {
                            let rows = &weight[group * 8 * in_dim..(group + 1) * 8 * in_dim];
                            group_sums.copy_from_slice(&dots_i16_8_avx2(rows, &activations));
                        }
                    });
                } else {
                    for (row, sum) in sums.iter_mut().enumerate() {
                        *sum = scalar_dot(&weight[row * in_dim..(row + 1) * in_dim]);
                    }
                }
                self.hidden_head_value(sums, vectorized)
            }
            QuantizedHead::Stack { .. }
            | QuantizedHead::Material { .. }
            | QuantizedHead::Kings { .. }
            | QuantizedHead::HiddenBeside { .. }
            | QuantizedHead::TwoHidden { .. }
            | QuantizedHead::TwoHiddenI8 { .. } => {
                panic!("a stack, material, kings, hidden-beside or two-hidden head reads products or squares")
            }
        }
    }

    /// `evaluate_lanes` for a screlu network: each lane's activation, in `0..=255`, squared into
    /// `0..=127` (`product_of` with itself), own first, into the output, its only head. Vectorized
    /// when a perspective is a multiple of 16 lanes; integer either way, so the two agree exactly.
    fn evaluate_screlu_lanes(&self, own: &[i16], opponent: &[i16], vectorized: bool, buckets: [usize; 3]) -> f32 {
        // A kings network: the squares stored once, then read by every row of each side's layer, at
        // a pairwise product's scale, and by the skip.
        if let QuantizedHead::Kings { ours, theirs, skip, skip_scale, head } = &self.head {
            return ACTIVATION_SCRATCH.with(|scratch| {
                let mut squares = scratch.borrow_mut();
                self.screlu_squares(own, opponent, vectorized, &mut squares);
                let vectorized = vectorized && own.len().is_multiple_of(16);
                let (own_squares, opponent_squares) = squares.split_at(own.len());
                // Each side's values clipped, then squared beside them, ours first: the head's input.
                let width = head.in_dim / 4;
                let mut values = [0.0f32; 4 * MAX_HIDDEN_OUTPUTS];
                let sides = [(&ours[buckets[0]], own_squares), (&theirs[their_own_frame(buckets[1])], opponent_squares)];
                for ((rows, side_squares), side_values) in sides.into_iter().zip(values[..4 * width].chunks_mut(2 * width)) {
                    let (clipped, squared) = side_values.split_at_mut(width);
                    self.first_layer_values(side_squares, vectorized, rows, clipped);
                    for (square, value) in squared.iter_mut().zip(clipped.iter()) {
                        *square = value * value;
                    }
                }
                let skip_value = self.skip_value(own, opponent, &squares, vectorized, skip, *skip_scale);
                head.bias[0] + dot_product_avx2(&head.weight, &values[..4 * width]) + skip_value
            });
        }
        // A hidden layer beside the output: the squares stored once, read by every row of the hidden
        // layer and by the skip, as a kings network's with one set over every square.
        if let QuantizedHead::HiddenBeside { hidden, skip, skip_scale, head } = &self.head {
            return ACTIVATION_SCRATCH.with(|scratch| {
                let mut squares = scratch.borrow_mut();
                self.screlu_squares(own, opponent, vectorized, &mut squares);
                let vectorized = vectorized && own.len().is_multiple_of(16);
                let skip_value = self.skip_value(own, opponent, &squares, vectorized, skip, *skip_scale);
                self.hidden_beside_value(own, opponent, vectorized, hidden, head) + skip_value
            });
        }
        // Two hidden layers: the squares stored once, read by every row of the first layer, then the
        // second layer and the output, as a stack's one set.
        if let QuantizedHead::TwoHidden { first, second, output } = &self.head {
            return ACTIVATION_SCRATCH.with(|scratch| {
                let mut squares = scratch.borrow_mut();
                self.screlu_squares(own, opponent, vectorized, &mut squares);
                self.stack_head_value(&squares, vectorized && own.len().is_multiple_of(16), first, second, output)
            });
        }
        // The same in 8 bits: the squares, already in `0..=127`, narrowed to `u8` once, then the
        // first layer eight rows at a time where 32 inputs a block allow, the rest as `TwoHidden`'s.
        if let QuantizedHead::TwoHiddenI8 { first, second, output } = &self.head {
            return ACTIVATION_SCRATCH.with(|scratch| {
                let mut squares = scratch.borrow_mut();
                self.screlu_squares(own, opponent, vectorized, &mut squares);
                U8_SCRATCH.with(|scratch| {
                    let mut narrow = scratch.borrow_mut();
                    narrow.clear();
                    narrow.extend(squares.iter().map(|&square| square as u8));
                    two_hidden_i8_value(&narrow, vectorized, first, second, output)
                })
            });
        }
        // A taper reads both sets, `buckets`' third place being the phase that blends them.
        if let QuantizedHead::Material { weights, scales, biases } = &self.head
            && weights.len() == TAPER_SETS
        {
            // Vectorized, both sets in one pass, each square computed once (`screlu_dot2_i16_avx2`).
            let [middlegame, endgame] = if vectorized && own.len().is_multiple_of(16) {
                let sums = screlu_dot2_i16_avx2(own, opponent, &weights[0], &weights[1], self.activation());
                [0, 1].map(|set| output_value(biases[set], sums[set], scales[set], PRODUCT_ONE))
            } else {
                [0, 1].map(|set| self.screlu_output(own, opponent, false, &weights[set], scales[set], biases[set]))
            };
            return taper(middlegame, endgame, buckets[2]);
        }
        // Its output, or its material bucket's set (`buckets`' third place).
        let (weight, scale, bias) = match &self.head {
            QuantizedHead::Output { weight, scale, bias } => (weight, *scale, *bias),
            QuantizedHead::Material { weights, scales, biases } => (&weights[buckets[2]], scales[buckets[2]], biases[buckets[2]]),
            _ => panic!("a screlu network's head is its output or its material sets"),
        };
        self.screlu_output(own, opponent, vectorized, weight, scale, bias)
    }

    /// Each lane's activation squared (`product_of` with itself), own first, into `squares`:
    /// vectorized when a perspective is a multiple of 16 lanes, the same integers either way.
    fn screlu_squares(&self, own: &[i16], opponent: &[i16], vectorized: bool, squares: &mut Vec<i16>) {
        squares.resize(own.len() + opponent.len(), 0);
        let (own_part, opponent_part) = squares.split_at_mut(own.len());
        for (part, lanes) in [(own_part, own), (opponent_part, opponent)] {
            if vectorized && lanes.len() % 16 == 0 {
                screlu_activations_i16_avx2(part, lanes, self.activation());
            } else {
                for (square, &lane) in part.iter_mut().zip(lanes) {
                    let activation = self.activation_of(lane);
                    *square = product_of(activation, activation) as i16;
                }
            }
        }
    }

    /// The skip's value, the output's weights on the squares (`split_skip`): its integer sum, from the
    /// lanes when `vectorized`, else from the stored `squares`, the same either way.
    fn skip_value(&self, own: &[i16], opponent: &[i16], squares: &[i16], vectorized: bool, skip: &[i16], scale: f32) -> f32 {
        let sum = if vectorized {
            screlu_dot_i16_avx2(own, opponent, skip, self.activation())
        } else {
            squares.iter().zip(skip).map(|(&square, &weight)| square as i32 * weight as i32).sum()
        };
        sum as f32 / (PRODUCT_ONE * scale)
    }

    /// One screlu output set's value from the lanes, vectorized when a perspective is a multiple of
    /// 16 lanes; integer either way, so the two agree exactly.
    fn screlu_output(&self, own: &[i16], opponent: &[i16], vectorized: bool, weight: &[i16], scale: f32, bias: f32) -> f32 {
        let sum = if vectorized && own.len().is_multiple_of(16) {
            screlu_dot_i16_avx2(own, opponent, weight, self.activation())
        } else {
            own.iter()
                .chain(opponent)
                .zip(weight)
                .map(|(&lane, &weight)| {
                    let activation = self.activation_of(lane);
                    product_of(activation, activation) * weight as i32
                })
                .sum()
        };
        output_value(bias, sum, scale, PRODUCT_ONE)
    }

    /// `evaluate_lanes` for a pairwise network: each perspective's halves as activations in
    /// `0..=255`, multiplied into `0..=127` (`product_of`), own first, then the head on the
    /// products. Vectorized when a half is a multiple of 16 lanes; integer either way, so the two
    /// agree exactly.
    fn evaluate_pairwise_lanes(&self, own: &[i16], opponent: &[i16], vectorized: bool, buckets: [usize; 3]) -> f32 {
        let half = own.len() / 2;
        let vectorized = vectorized && half.is_multiple_of(16);
        // No hidden layer: the products go straight into the output's dot product, never stored.
        if let (true, QuantizedHead::Output { weight, scale, bias }) = (vectorized, &self.head) {
            let sum = pairwise_dot_i16_avx2(own, opponent, weight, self.activation());
            return output_value(*bias, sum, *scale, PRODUCT_ONE);
        }
        // The same for a material network, through its bucket's set.
        if let (true, QuantizedHead::Material { weights, scales, biases }) = (vectorized, &self.head) {
            let bucket = buckets[2];
            let sum = pairwise_dot_i16_avx2(own, opponent, &weights[bucket], self.activation());
            return output_value(biases[bucket], sum, scales[bucket], PRODUCT_ONE);
        }
        ACTIVATION_SCRATCH.with(|scratch| {
            let mut products = scratch.borrow_mut();
            products.resize(2 * half, 0);
            let (own_part, opponent_part) = products.split_at_mut(half);
            for (part, lanes) in [(own_part, own), (opponent_part, opponent)] {
                if vectorized {
                    pairwise_activations_i16_avx2(part, lanes, self.activation());
                } else {
                    for (i, product) in part.iter_mut().enumerate() {
                        *product = product_of(self.activation_of(lanes[i]), self.activation_of(lanes[i + half])) as i16;
                    }
                }
            }
            let products: &[i16] = &products;
            let scalar_dot = |weights: &[i16]| -> i32 {
                products.iter().zip(weights).map(|(&product, &weight)| product as i32 * weight as i32).sum()
            };
            match &self.head {
                // Scalar only: the vectorized one returned above.
                QuantizedHead::Output { weight, scale, bias } => output_value(*bias, scalar_dot(weight), *scale, PRODUCT_ONE),
                QuantizedHead::Material { weights, scales, biases } => {
                    let bucket = buckets[2];
                    output_value(biases[bucket], scalar_dot(&weights[bucket]), scales[bucket], PRODUCT_ONE)
                }
                QuantizedHead::Hidden { weight, biases, .. } => {
                    let in_dim = products.len();
                    let out_dim = biases.len();
                    let vectorized = vectorized && out_dim % 8 == 0;
                    let mut sums = [0i32; MAX_HIDDEN_OUTPUTS];
                    let sums = &mut sums[..out_dim];
                    for (group, group_sums) in sums.chunks_mut(8).enumerate() {
                        if vectorized {
                            let rows = &weight[group * 8 * in_dim..(group + 1) * 8 * in_dim];
                            group_sums.copy_from_slice(&dots_i16_8_avx2(rows, products));
                        } else {
                            for (index, sum) in group_sums.iter_mut().enumerate() {
                                let row = group * 8 + index;
                                *sum = scalar_dot(&weight[row * in_dim..(row + 1) * in_dim]);
                            }
                        }
                    }
                    self.hidden_head_value(sums, vectorized)
                }
                QuantizedHead::Stack { l1, l2, output } => {
                    self.stack_head_value(products, vectorized, &l1[buckets[0]], &l2[buckets[1]], &output[buckets[2]])
                }
                // The products stored once, read by every row of the hidden layer and by the skip, as
                // a screlu network's squares are.
                QuantizedHead::HiddenBeside { hidden, skip, skip_scale, head } => {
                    let sum = if vectorized { pairwise_dot_i16_avx2(own, opponent, skip, self.activation()) } else { scalar_dot(skip) };
                    self.hidden_beside_value(own, opponent, vectorized, hidden, head) + sum as f32 / (PRODUCT_ONE * *skip_scale)
                }
                // Halfk's two hidden layers, over the products as over a screlu network's squares.
                QuantizedHead::TwoHidden { first, second, output } => self.stack_head_value(products, vectorized, first, second, output),
                QuantizedHead::TwoHiddenI8 { first, second, output } => U8_SCRATCH.with(|scratch| {
                    let mut narrow = scratch.borrow_mut();
                    narrow.clear();
                    narrow.extend(products.iter().map(|&product| product as u8));
                    two_hidden_i8_value(&narrow, vectorized, first, second, output)
                }),
                QuantizedHead::Kings { .. } => panic!("a kings head reads squares, not pairwise products"),
            }
        })
    }

    /// Each perspective's whole products, own first, into `whole`: a pairwise network's halves'
    /// activations multiplied, a screlu network's lane's with itself, `0..=65025` as f32, exact. Not
    /// rounded to 7 bits as `product_of`'s are: `QuantizedHead::HiddenBeside` reads them. Vectorized
    /// when a perspective's products are a multiple of 16 (`whole_products_f32_avx2`), the same
    /// integers either way: the scalar loop cost the head about three times its 7-bit form's time
    /// (2026-10-27).
    fn whole_products(&self, own: &[i16], opponent: &[i16], vectorized: bool, whole: &mut Vec<f32>) {
        let count = if self.pairwise { own.len() / 2 } else { own.len() };
        if vectorized && count.is_multiple_of(16) {
            whole.resize(2 * count, 0.0);
            let (own_part, opponent_part) = whole.split_at_mut(count);
            whole_products_f32_avx2(own_part, own, self.pairwise, self.activation());
            whole_products_f32_avx2(opponent_part, opponent, self.pairwise, self.activation());
            return;
        }
        whole.clear();
        for lanes in [own, opponent] {
            if self.pairwise {
                let (a, b) = lanes.split_at(lanes.len() / 2);
                whole.extend(a.iter().zip(b).map(|(&a, &b)| (self.activation_of(a) * self.activation_of(b)) as f32));
            } else {
                whole.extend(lanes.iter().map(|&lane| {
                    let activation = self.activation_of(lane);
                    (activation * activation) as f32
                }));
            }
        }
    }

    /// A hidden layer beside the output's share of the evaluation, the skip's left to the caller: the
    /// layer in f32 over the whole products, each value clipped, then squared beside it, through
    /// `head` with its bias. The rows summed the same way on every path, so that scalar and
    /// vectorized agree bit for bit.
    fn hidden_beside_value(&self, own: &[i16], opponent: &[i16], vectorized: bool, hidden: &Layer, head: &Layer) -> f32 {
        WHOLE_SCRATCH.with(|scratch| {
            let mut whole = scratch.borrow_mut();
            self.whole_products(own, opponent, vectorized, &mut whole);
            let (in_dim, width) = (hidden.in_dim, hidden.out_dim);
            let mut values = [0.0f32; 2 * MAX_HIDDEN_OUTPUTS];
            let (clipped, squared) = values[..2 * width].split_at_mut(width);
            // Eight rows at once where the shape allows (`dot_products_8_avx2`), one horizontal
            // reduction for the eight instead of four a row: chosen by the shape alone, never by
            // `vectorized`, so that both paths sum each row the same way.
            if width.is_multiple_of(8) && in_dim.is_multiple_of(8) {
                for (group, group_values) in clipped.chunks_mut(8).enumerate() {
                    let sums = dot_products_8_avx2(&hidden.weight[group * 8 * in_dim..(group + 1) * 8 * in_dim], &whole);
                    for (index, value) in group_values.iter_mut().enumerate() {
                        *value = (hidden.bias[group * 8 + index] + sums[index]).clamp(0.0, 1.0);
                    }
                }
            } else {
                for (row, value) in clipped.iter_mut().enumerate() {
                    *value = (hidden.bias[row] + dot_product_avx2(&hidden.weight[row * in_dim..(row + 1) * in_dim], &whole)).clamp(0.0, 1.0);
                }
            }
            for (square, value) in squared.iter_mut().zip(clipped.iter()) {
                *square = value * value;
            }
            head.bias[0] + dot_product_avx2(&head.weight, &values[..2 * width])
        })
    }

    /// A first hidden layer's clipped values from its input, products or squares, into `values`: its
    /// integer sums, in groups of eight rows when `vectorized` and its outputs allow, as `Hidden`'s,
    /// then their values, bit for bit the same either way.
    fn first_layer_values(&self, inputs: &[i16], vectorized: bool, first: &QuantizedRows, values: &mut [f32]) {
        let in_dim = inputs.len();
        let out_dim = first.biases.len();
        let vectorized = vectorized && out_dim.is_multiple_of(8);
        let mut sums = [0i32; MAX_HIDDEN_OUTPUTS];
        let sums = &mut sums[..out_dim];
        for (group, group_sums) in sums.chunks_mut(8).enumerate() {
            if vectorized {
                group_sums.copy_from_slice(&dots_i16_8_avx2(&first.weight[group * 8 * in_dim..(group + 1) * 8 * in_dim], inputs));
            } else {
                for (index, sum) in group_sums.iter_mut().enumerate() {
                    let row = &first.weight[(group * 8 + index) * in_dim..(group * 8 + index + 1) * in_dim];
                    *sum = inputs.iter().zip(row).map(|(&input, &weight)| input as i32 * weight as i32).sum();
                }
            }
        }
        if vectorized {
            hidden_values_avx2(sums, &first.denominators, &first.biases, values);
        } else {
            for row in 0..out_dim {
                values[row] = hidden_value(first.biases[row], sums[row], first.scales[row], PRODUCT_ONE);
            }
        }
    }

    /// A stack head's value from the products, through the weight sets of the position's buckets, or
    /// a two-hidden head's from the squares, through its one set:
    /// the first layer's integer sums (in groups of eight rows when `vectorized` and its outputs
    /// allow, as `Hidden`'s), their values, then the second layer and the output in f32.
    fn stack_head_value(&self, products: &[i16], vectorized: bool, first: &QuantizedRows, second: &Layer, output: &Layer) -> f32 {
        let out_dim = first.biases.len();
        let mut values = [0.0f32; MAX_HIDDEN_OUTPUTS];
        let values = &mut values[..out_dim];
        self.first_layer_values(products, vectorized, first, values);
        second_layer_and_output(values, second, output)
    }

    /// A head with a hidden layer, from its first layer's integer sums: their values, bit for bit
    /// the same either way (`hidden_values_avx2`), then any further layers and the output in f32.
    fn hidden_head_value(&self, sums: &[i32], vectorized: bool) -> f32 {
        let QuantizedHead::Hidden { scales, denominators, biases, rest, output, .. } = &self.head else {
            panic!("hidden_head_value called on a head with no hidden layer");
        };
        let out_dim = biases.len();
        let mut values = [0.0f32; MAX_HIDDEN_OUTPUTS];
        let values = &mut values[..out_dim];
        if vectorized {
            hidden_values_avx2(sums, denominators, biases, values);
        } else {
            for row in 0..out_dim {
                values[row] = hidden_value(biases[row], sums[row], scales[row], self.input_one());
            }
        }

        if rest.is_empty() {
            return output.bias[0] + dot_product_avx2(&output.weight, values);
        }
        // Further layers in f32, through `HEAD_SCRATCH`: the f32 head never runs inside this one.
        HEAD_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            let (current, next) = &mut *scratch;
            current.clear();
            current.extend_from_slice(values);
            for layer in rest {
                layer.forward_into(current, next);
                clipped_relu_avx2(next);
                std::mem::swap(current, next);
            }
            output.bias[0] + dot_product_avx2(&output.weight, current)
        })
    }

    /// The evaluation in pawns from scratch, relative to the side to move, in scalar arithmetic
    /// throughout: the reference every incremental and vectorized path must equal exactly.
    pub fn evaluate_pawns(&self, board: &Board) -> f32 {
        let side_to_move = board.side_to_move();
        // Saturating, as the engine's path adds the threat rows: exact, since a lane is read only as
        // an activation, clamped to `0..=accumulator_one()` within `i16`.
        let narrow = |lanes: Vec<i32>| -> Vec<i16> {
            lanes.into_iter().map(|lane| lane.clamp(i16::MIN as i32, i16::MAX as i32) as i16).collect()
        };
        let own = narrow(self.refresh(board, side_to_move));
        let opponent = narrow(self.refresh(board, side_to_move.opposite()));
        self.evaluate_lanes(&own, &opponent, false, self.buckets(board))
    }

    /// `evaluate_pawns` in the engine's centipawns.
    pub fn evaluate(&self, board: &Board) -> i32 {
        centipawns(self.evaluate_pawns(board))
    }
}

/// One piece's psq row seen from `perspective`, from its absolute square.
fn piece_row_for(piece_type: PieceType, is_own: bool, square: Square, perspective: Color) -> usize {
    psq::piece_row(piece_type, is_own, oriented_square(square, perspective))
}

/// The most rows one perspective can gain or lose in one update. A legal move loses at most 8
/// (castling: king and rook, four castling rights, an en passant file) and gains fewer; the
/// margin covers any two boards the search compares.
const MAX_CHANGED_ROWS: usize = 16;

/// Every psq row, per perspective, that differs between a `parent` and a `child` board: `added`
/// the rows active in `child` only, `removed` those active in `parent` only. On the stack, so an
/// update allocates nothing.
///
/// What changed is found by comparing the two boards, not by reading the move: for each of the 12
/// piece bitboards, `parent XOR child` is the squares that changed, of which those in `parent` lost
/// the piece and those in `child` gained it. Castling (king and rook), en passant (the captured pawn
/// is off the destination square), promotion (a pawn leaves, another piece arrives) and null moves
/// need no case of their own, and `make_move` is not touched. Castling rights and the en passant
/// file are compared directly. The boards are scanned once, each change mapped to both perspectives'
/// rows.
struct ChangedRows {
    added: [[usize; MAX_CHANGED_ROWS]; 2],
    removed: [[usize; MAX_CHANGED_ROWS]; 2],
    added_count: [usize; 2],
    removed_count: [usize; 2],
}

impl ChangedRows {
    fn empty() -> ChangedRows {
        ChangedRows {
            added: [[0; MAX_CHANGED_ROWS]; 2],
            removed: [[0; MAX_CHANGED_ROWS]; 2],
            added_count: [0; 2],
            removed_count: [0; 2],
        }
    }

    /// `between` for a halfk network: its piece rows alone, each perspective's in the bucket of its
    /// king in `child` (`kings`, `halfk_king`). No castling or en passant rows: halfk has none. A
    /// perspective whose king changed bucket is refreshed instead, and its rows here go unread.
    fn between_halfk(parent: &Board, child: &Board, kings: [(usize, bool); 2]) -> ChangedRows {
        let mut changes = ChangedRows::empty();
        for color in [Color::White, Color::Black] {
            for piece_type in PieceType::ALL {
                let piece = Piece::new(color, piece_type);
                let before = parent.bitboard(piece);
                let after = child.bitboard(piece);
                let changed = before ^ after;
                if changed == Bitboard::EMPTY {
                    continue;
                }
                for perspective in [Color::White, Color::Black] {
                    let (bucket, mirrored) = kings[perspective as usize];
                    let is_own = color == perspective;
                    let row = |square: Square| halfk::piece_row(bucket, mirrored, piece_type, is_own, oriented_square(square, perspective));
                    for square in (before & changed).squares() {
                        changes.push(perspective, row(square), false);
                    }
                    for square in (after & changed).squares() {
                        changes.push(perspective, row(square), true);
                    }
                }
            }
        }
        changes
    }

    fn between(parent: &Board, child: &Board) -> ChangedRows {
        let mut changes = ChangedRows::empty();
        let perspectives = [Color::White, Color::Black];

        for color in [Color::White, Color::Black] {
            for piece_type in PieceType::ALL {
                let piece = Piece::new(color, piece_type);
                let before = parent.bitboard(piece);
                let after = child.bitboard(piece);
                let changed = before ^ after;
                if changed == Bitboard::EMPTY {
                    continue;
                }
                for perspective in perspectives {
                    let is_own = color == perspective;
                    for square in (before & changed).squares() {
                        changes.push(perspective, piece_row_for(piece_type, is_own, square, perspective), false);
                    }
                    for square in (after & changed).squares() {
                        changes.push(perspective, piece_row_for(piece_type, is_own, square, perspective), true);
                    }
                }
            }
        }

        let (before, after) = (parent.castling_rights(), child.castling_rights());
        if before != after {
            for perspective in perspectives {
                let opponent = perspective.opposite();
                let castling = [
                    (before.kingside(perspective), after.kingside(perspective)),
                    (before.queenside(perspective), after.queenside(perspective)),
                    (before.kingside(opponent), after.kingside(opponent)),
                    (before.queenside(opponent), after.queenside(opponent)),
                ];
                for (index, &(had, has)) in castling.iter().enumerate() {
                    if had != has {
                        changes.push(perspective, psq::CASTLING_ROW + index, has);
                    }
                }
            }
        }

        let file_before = parent.en_passant_square().map(|square| square.file() as usize);
        let file_after = child.en_passant_square().map(|square| square.file() as usize);
        if file_before != file_after {
            for perspective in perspectives {
                if let Some(file) = file_before {
                    changes.push(perspective, psq::EN_PASSANT_ROW + file, false);
                }
                if let Some(file) = file_after {
                    changes.push(perspective, psq::EN_PASSANT_ROW + file, true);
                }
            }
        }
        changes
    }

    fn push(&mut self, perspective: Color, row: usize, added: bool) {
        let index = perspective as usize;
        if added {
            self.added[index][self.added_count[index]] = row;
            self.added_count[index] += 1;
        } else {
            self.removed[index][self.removed_count[index]] = row;
            self.removed_count[index] += 1;
        }
    }

    fn added(&self, perspective: Color) -> &[usize] {
        let index = perspective as usize;
        &self.added[index][..self.added_count[index]]
    }

    fn removed(&self, perspective: Color) -> &[usize] {
        let index = perspective as usize;
        &self.removed[index][..self.removed_count[index]]
    }
}

/// The step a measured pawn scale is rounded to, in percent, so that networks of nearly the same
/// scale search the same trees.
const PAWN_SCALE_STEP_PERCENT: i32 = 5;

/// The range a measured pawn scale is clamped to, in percent, against a degenerate network.
const PAWN_SCALE_MIN_PERCENT: i32 = 25;
const PAWN_SCALE_MAX_PERCENT: i32 = 200;

/// The positions `measure_label_percent` reads, as FENs, one per line: 10,000 positions of
/// over-the-board games between players rated 2400 or more (Lumbra's Gigabase), two per game from
/// 5,000 games, the side to move not in check. Games of humans, so held out from every training
/// corpus (TCEC, CCRL and self-play); written once by `nnue_bench --write-pawn-scale-positions` and
/// compiled in, so a release needs no data beside the binary.
///
/// They replaced Stockfish's bench positions and every position one legal move from each, a set of
/// about 1,700 that measured amanda-0001 at 70%, betty-0001 at 25% (the clamp's floor) and
/// caroline-0001 at 150%, where these measured 55%, 60% and 55% by the static slope that preceded
/// `measure_label_percent`, and 100,000 positions of other games agreed with them to the rounded
/// percent. One random legal move often leaves a piece hanging, which a network sees and the
/// PeSTO evaluation does not.
const PAWN_SCALE_POSITIONS: &str = include_str!("pawn_scale_positions.txt");

/// `PAWN_SCALE_POSITIONS` parsed, once per process: every network load measures on them.
fn pawn_scale_boards() -> &'static [Board] {
    static BOARDS: std::sync::OnceLock<Vec<Board>> = std::sync::OnceLock::new();
    BOARDS.get_or_init(|| {
        PAWN_SCALE_POSITIONS
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| Board::from_fen(line).unwrap_or_else(|problem| panic!("{line}: {problem:?}")).0)
            .collect()
    })
}

/// The bracket `fit_label_scale` searches `c` in: a network's pawn between a tenth and four times
/// PeSTO's. Measured on every position, betty-0002 and dorothy-0001 ended at an upper bound of 4
/// (2026-10-20), their pawn under a quarter of PeSTO's, so the bound sits well above that.
const LABEL_SCALE_MIN: f64 = 0.25;
const LABEL_SCALE_MAX: f64 = 10.0;

/// The label-space scale `c` that best maps PeSTO's centipawns onto a network's, in the space the
/// network is trained in, so that the search's margins and a PeSTO blend can both live in PeSTO's
/// centipawn whatever the network: the `c` minimising
/// `sum (score(h / 100 / c) - score(n / 100))^2` over `pairs` of `(h, n)`, PeSTO's and the
/// network's evaluations in centipawns from the side to move, every pair counted (no window, `h = 0`
/// kept). `c` sits inside `labels::score`'s `atan`, so there is no closed form: a golden-section
/// search over `[LABEL_SCALE_MIN, LABEL_SCALE_MAX]`, to well below any rounding `c` will get.
/// Returns `c` and the fit's residual RMS in label space. `score` is `curve`'s, the one the network
/// was trained under (`curve_of_version`).
pub fn fit_label_scale_through(pairs: &[(i32, i32)], curve: &crate::labels::Curve) -> (f64, f64) {
    let score = |pawns: f64| curve.score(pawns);
    let targets: Vec<(f64, f64)> = pairs.iter().map(|&(hand, net)| (hand as f64 / 100.0, score(net as f64 / 100.0))).collect();
    let loss = |c: f64| -> f64 { targets.iter().map(|&(pawns, target)| (score(pawns / c) - target).powi(2)).sum() };
    let ratio = (5f64.sqrt() - 1.0) / 2.0;
    let (mut low, mut high) = (LABEL_SCALE_MIN, LABEL_SCALE_MAX);
    let mut left = high - ratio * (high - low);
    let mut right = low + ratio * (high - low);
    let (mut left_loss, mut right_loss) = (loss(left), loss(right));
    for _ in 0..60 {
        if left_loss < right_loss {
            high = right;
            (right, right_loss) = (left, left_loss);
            left = high - ratio * (high - low);
            left_loss = loss(left);
        } else {
            low = left;
            (left, left_loss) = (right, right_loss);
            right = low + ratio * (high - low);
            right_loss = loss(right);
        }
    }
    let c = (low + high) / 2.0;
    let rms = if targets.is_empty() { 0.0 } else { (loss(c) / targets.len() as f64).sqrt() };
    (c, rms)
}

/// `fit_label_scale_through` a current network's curve, `labels::LOGISTIC`.
pub fn fit_label_scale(pairs: &[(i32, i32)]) -> (f64, f64) {
    fit_label_scale_through(pairs, &crate::labels::LOGISTIC)
}

/// `fit_label_scale` over the compiled-in positions (`PAWN_SCALE_POSITIONS`), all
/// of them, `evaluate` the network's evaluation in centipawns, PeSTO's side its quiescence
/// (`search::pesto_quiescence`), not its static evaluation: about 5% of these positions are
/// taken mid-exchange, where the static count is several pawns off and the network is right.
/// Returns `c`, the residual, the pairs (for a caller comparing other fits on the same data), and how
/// many positions quiescence moved off their static evaluation.
pub fn measure_label_scale(evaluate: impl Fn(&Board) -> i32) -> (f64, f64, Vec<(i32, i32)>, usize) {
    measure_label_scale_through(evaluate, &crate::labels::LOGISTIC)
}

/// `measure_label_scale` for a network trained under `curve`.
pub fn measure_label_scale_through(evaluate: impl Fn(&Board) -> i32, curve: &crate::labels::Curve) -> (f64, f64, Vec<(i32, i32)>, usize) {
    let boards = pawn_scale_boards();
    let resolved = pawn_scale_quiescence();
    let moved = boards.iter().zip(resolved).filter(|&(board, &value)| value != crate::eval::evaluate(board)).count();
    let pairs: Vec<(i32, i32)> = boards.iter().zip(resolved).map(|(board, &value)| (value, evaluate(board))).collect();
    let (c, rms) = fit_label_scale_through(&pairs, curve);
    (c, rms, pairs, moved)
}

/// PeSTO's quiescence on each of `pawn_scale_boards`, in order: the same for every network, so
/// computed once per process, as the boards are.
fn pawn_scale_quiescence() -> &'static [i32] {
    static VALUES: std::sync::OnceLock<Vec<i32>> = std::sync::OnceLock::new();
    VALUES.get_or_init(|| {
        pawn_scale_boards().iter().map(crate::search::pesto_quiescence).collect()
    })
}

/// An evaluator's pawn against `piece_value`'s, in percent, what the search scales its evaluation
/// margins by: `100 / c` of `measure_label_scale`, rounded to `PAWN_SCALE_STEP_PERCENT` and clamped.
/// Near 0 the label curve is linear, so one linear factor is `c`'s fit where the margins act. It
/// replaced a static slope (`sum(h * n) / sum(h * h)` within 300 cp of equality, against PeSTO's
/// static evaluation), which weighs the large evaluations most: the four
/// champions of 2026-10-20 measured 50 to 64% by that slope, scattered by network, and 33.6 to 35.4%
/// by this, all 35% rounded, `c` fitting better in label space on every one.
pub fn measure_label_percent(evaluate: impl Fn(&Board) -> i32) -> i32 {
    measure_label_percent_through(evaluate, &crate::labels::LOGISTIC)
}

/// `measure_label_percent` for a network trained under `curve`.
pub fn measure_label_percent_through(evaluate: impl Fn(&Board) -> i32, curve: &crate::labels::Curve) -> i32 {
    let (c, _, _, _) = measure_label_scale_through(evaluate, curve);
    let percent = 100.0 / c;
    let rounded = (percent / PAWN_SCALE_STEP_PERCENT as f64).round() as i32 * PAWN_SCALE_STEP_PERCENT;
    rounded.clamp(PAWN_SCALE_MIN_PERCENT, PAWN_SCALE_MAX_PERCENT)
}

/// Any network the engine can run, as `load` finds it on disk. What the search, UCI and the arena
/// hold: which kind a file is comes from its own feature-set tag, never from the caller.
///
/// A psq network is held twice: the f32 weights as loaded, which `smithy`'s checks and the
/// benches compare against, and their quantization, which is what the engine evaluates with.
#[derive(Debug, Clone, PartialEq)]
pub enum Network {
    Psq(PsqNetwork, QuantizedPsqNetwork),
}

impl Network {
    /// A psq network, quantized for the engine, its pawn scale measured.
    pub fn from_psq(network: PsqNetwork) -> Result<Network, String> {
        Network::from_psq_with(network, HiddenArithmetic::Int16)
    }

    /// `from_psq`, a two-hidden head's first layer in `arithmetic` (`QuantizedPsqNetwork::quantize_with`).
    pub fn from_psq_with(network: PsqNetwork, arithmetic: HiddenArithmetic) -> Result<Network, String> {
        Network::from_psq_through(network, arithmetic, &crate::labels::LOGISTIC)
    }

    /// `from_psq_with` for a network trained under `curve`, its pawn measured through it.
    fn from_psq_through(network: PsqNetwork, arithmetic: HiddenArithmetic, curve: &crate::labels::Curve) -> Result<Network, String> {
        let mut quantized = QuantizedPsqNetwork::quantize_with(&network, arithmetic)?;
        let pawn_percent = measure_label_percent_through(|board| quantized.evaluate(board), curve);
        quantized.pawn_percent = pawn_percent;
        Ok(Network::Psq(network, quantized))
    }

    /// This network's pawn against `piece_value`'s, in percent (`measure_label_percent`).
    pub fn pawn_percent(&self) -> i32 {
        match self {
            Network::Psq(_, quantized) => quantized.pawn_percent(),
        }
    }

    /// What this network's search was tuned to (`read_tuning_beside`).
    pub fn tuning(&self) -> Tuning {
        match self {
            Network::Psq(_, quantized) => quantized.tuning,
        }
    }

    /// Leaf evaluation in centipawns, relative to the side to move, whichever kind this is: the
    /// quantized network's, from scratch.
    pub fn evaluate(&self, board: &Board) -> i32 {
        match self {
            Network::Psq(_, quantized) => quantized.evaluate(board),
        }
    }

    /// Reads a network of any kind the engine runs, dispatching on the file's feature-set tag.
    /// Every failure is an `Err` naming the file and what was wrong with it, never a panic: this
    /// reads a file the caller named, so a bad path or a truncated download is an ordinary outcome
    /// rather than a bug.
    pub fn load(path: &str) -> Result<Network, String> {
        let bytes = fs::read(path).map_err(|error| format!("{path}: {error}"))?;
        let mut network = Network::from_bytes(&bytes, path)?;
        let tuning = read_tuning_beside(path)?;
        match &mut network {
            Network::Psq(_, quantized) => quantized.tuning = tuning,
        }
        Ok(network)
    }

    /// A network from a `.nnue` file's bytes, `name` standing for the file in every error, untuned:
    /// no file sits beside bytes, so `load` adds the tuning. Also the network a build embeds
    /// (`KALO_CHESS_EVALFILE`, `uci::embedded_network`).
    pub fn from_bytes(bytes: &[u8], name: &str) -> Result<Network, String> {
        let path = name;
        let mut reader = Reader { bytes, at: 0, path };

        let magic = reader.take(MAGIC.len())?;
        if magic != MAGIC {
            return Err(format!("{path}: not a Kalo Chess network file (bad magic)"));
        }
        let version = reader.u32()?;
        if version != VERSION && version != ATAN_VERSION {
            return Err(format!("{path}: format version {version}, but this build reads versions {ATAN_VERSION} and {VERSION}"));
        }
        let feature_set = reader.u32()?;
        let Some((rows, head_input_multiple)) = shape_of(feature_set) else {
            let name = feature_set_name(feature_set);
            return Err(format!("{path}: holds {name} weights, which this build cannot run yet"));
        };

        let embedding_dim = reader.u32()? as usize;
        let hidden_count = reader.u32()? as usize;
        let mut hidden_dims = Vec::with_capacity(hidden_count);
        for _ in 0..hidden_count {
            hidden_dims.push(reader.u32()? as usize);
        }
        let step = reader.u64()?;
        let learning_rate = reader.f64()?;

        // The header's dimensions decide how many floats to read, so a nonsense `embedding_dim`
        // must be caught here rather than turning into an enormous allocation below.
        if embedding_dim == 0 {
            return Err(format!("{path}: embedding_dim is 0"));
        }

        let transform = reader.f32s(rows * embedding_dim)?;
        let transform_bias = reader.f32s(embedding_dim)?;

        if hidden_dims.contains(&0) {
            return Err(format!("{path}: a hidden layer declares a width of 0"));
        }
        let is_kings = feature_set == FEATURE_SET_PSQ_THREATS_SCRELU_KINGS;
        let is_stack = matches!(feature_set, FEATURE_SET_PSQ_STACK | FEATURE_SET_PSQ_THREATS_STACK);
        if is_stack && hidden_count != 2 {
            return Err(format!("{path}: a {} network has two hidden layers, not {hidden_count}", feature_set_name(feature_set)));
        }
        if is_kings && hidden_count != 1 {
            return Err(format!("{path}: a psq_threats_screlu_kings network has one hidden width, a side's, not {hidden_count}"));
        }
        let is_screlu_hidden = feature_set == FEATURE_SET_PSQ_THREATS_SCRELU_HIDDEN;
        if is_screlu_hidden && hidden_count != 1 {
            return Err(format!("{path}: a psq_threats_screlu_hidden network has one hidden layer, not {hidden_count}"));
        }
        let halfk_hidden_count = match feature_set {
            FEATURE_SET_HALFK_THREATS_SCRELU => Some(0),
            FEATURE_SET_HALFK_THREATS_SCRELU_HIDDEN | FEATURE_SET_HALFK_THREATS_PAIRWISE_HIDDEN => Some(2),
            _ => None,
        };
        if let Some(expected) = halfk_hidden_count
            && hidden_count != expected
        {
            return Err(format!("{path}: a {} network has {expected} hidden layers, not {hidden_count}", feature_set_name(feature_set)));
        }
        let is_pairwise_hidden = feature_set == FEATURE_SET_PSQ_THREATS_PAIRWISE_HIDDEN;
        if is_pairwise_hidden && hidden_count != 1 {
            return Err(format!("{path}: a psq_threats_pairwise_hidden network has one hidden layer, not {hidden_count}"));
        }
        let is_material = matches!(
            feature_set,
            FEATURE_SET_PSQ_THREATS_PAIRWISE_MATERIAL
                | FEATURE_SET_PSQ_THREATS_SCRELU_MATERIAL
                | FEATURE_SET_PSQ_THREATS_SCRELU_PHASE
                | FEATURE_SET_PSQ_THREATS_SCRELU_COUNT
                | FEATURE_SET_PSQ_THREATS_SCRELU_TAPER
        );
        let output_sets = match feature_set {
            FEATURE_SET_PSQ_THREATS_SCRELU_PHASE => PHASE_BUCKETS,
            FEATURE_SET_PSQ_THREATS_SCRELU_COUNT => COUNT_BUCKETS,
            FEATURE_SET_PSQ_THREATS_SCRELU_TAPER => TAPER_SETS,
            _ => MATERIAL_BUCKETS,
        };
        if is_material && hidden_count != 0 {
            return Err(format!("{path}: a {} network has no hidden layer, not {hidden_count}", feature_set_name(feature_set)));
        }
        let mut layer = |in_dim: usize, out_dim: usize| -> Result<Layer, String> {
            Ok(Layer { in_dim, out_dim, weight: reader.f32s(out_dim * in_dim)?, bias: reader.f32s(out_dim)? })
        };
        let head_input = head_input_multiple * embedding_dim;
        let sets = |layer: &mut dyn FnMut(usize, usize) -> Result<Layer, String>, buckets: usize, in_dim: usize, out_dim: usize| {
            (0..buckets).map(|_| layer(in_dim, out_dim)).collect::<Result<Vec<Layer>, String>>()
        };
        let mut material = None;
        let (hidden, output, stack) = if is_kings {
            // Our side's sets, their side's, then the one output over both sides' values clipped and
            // squared and every square.
            let width = hidden_dims[0];
            let l1 = sets(&mut layer, OUR_KING_BUCKETS, embedding_dim, width)?;
            let l2 = sets(&mut layer, THEIR_KING_BUCKETS, embedding_dim, width)?;
            let output = sets(&mut layer, 1, 4 * width + head_input, 1)?;
            (Vec::new(), Layer::default(), Some(StackLayers { l1, l2, output }))
        } else if is_stack {
            let l1 = sets(&mut layer, OUR_KING_BUCKETS, head_input, hidden_dims[0])?;
            let l2 = sets(&mut layer, THEIR_KING_BUCKETS, hidden_dims[0], hidden_dims[1])?;
            let output = sets(&mut layer, MATERIAL_BUCKETS, hidden_dims[1], 1)?;
            (Vec::new(), Layer::default(), Some(StackLayers { l1, l2, output }))
        } else if is_material {
            material = Some(sets(&mut layer, output_sets, head_input, 1)?);
            (Vec::new(), Layer::default(), None)
        } else if is_screlu_hidden || is_pairwise_hidden {
            // The hidden layer over every square (or product), then the output over its values
            // clipped and squared and every square.
            let hidden = layer(head_input, hidden_dims[0])?;
            let output = layer(2 * hidden_dims[0] + head_input, 1)?;
            (vec![hidden], output, None)
        } else {
            let mut hidden = Vec::with_capacity(hidden_count);
            let mut input_dim = head_input;
            for &out_dim in &hidden_dims {
                hidden.push(layer(input_dim, out_dim)?);
                input_dim = out_dim;
            }
            (hidden, layer(input_dim, 1)?, None)
        };

        // Trailing bytes mean the file is not what the header says it is, even though everything
        // needed was read successfully -- most likely a layout mismatch that happens to land on a
        // shorter prefix. Refusing is the point: a partially-understood file is the one case where
        // a plausible network is worse than an error.
        if reader.at != bytes.len() {
            return Err(format!("{path}: {} unexpected bytes after the last weight", bytes.len() - reader.at));
        }

        let threats = matches!(
            feature_set,
            FEATURE_SET_PSQ_THREATS
                | FEATURE_SET_PSQ_THREATS_PAIRWISE
                | FEATURE_SET_PSQ_THREATS_STACK
                | FEATURE_SET_PSQ_THREATS_PAIRWISE_MATERIAL
                | FEATURE_SET_PSQ_THREATS_SCRELU
                | FEATURE_SET_PSQ_THREATS_SCRELU_MATERIAL
                | FEATURE_SET_PSQ_THREATS_SCRELU_PHASE
                | FEATURE_SET_PSQ_THREATS_SCRELU_COUNT
                | FEATURE_SET_PSQ_THREATS_SCRELU_TAPER
                | FEATURE_SET_PSQ_THREATS_SCRELU_KINGS
                | FEATURE_SET_PSQ_THREATS_SCRELU_HIDDEN
                | FEATURE_SET_PSQ_THREATS_PAIRWISE_HIDDEN
                | FEATURE_SET_HALFK_THREATS_SCRELU
                | FEATURE_SET_HALFK_THREATS_SCRELU_HIDDEN
                | FEATURE_SET_HALFK_THREATS_PAIRWISE_HIDDEN
        );
        let halfk = matches!(
            feature_set,
            FEATURE_SET_HALFK_THREATS_SCRELU | FEATURE_SET_HALFK_THREATS_SCRELU_HIDDEN | FEATURE_SET_HALFK_THREATS_PAIRWISE_HIDDEN
        );
        let screlu = matches!(
            feature_set,
            FEATURE_SET_HALFK_THREATS_SCRELU
                | FEATURE_SET_HALFK_THREATS_SCRELU_HIDDEN
                | FEATURE_SET_PSQ_THREATS_SCRELU
                | FEATURE_SET_PSQ_THREATS_SCRELU_MATERIAL
                | FEATURE_SET_PSQ_THREATS_SCRELU_PHASE
                | FEATURE_SET_PSQ_THREATS_SCRELU_COUNT
                | FEATURE_SET_PSQ_THREATS_SCRELU_TAPER
                | FEATURE_SET_PSQ_THREATS_SCRELU_KINGS
                | FEATURE_SET_PSQ_THREATS_SCRELU_HIDDEN
        );
        let pairwise = matches!(
            feature_set,
            FEATURE_SET_PSQ_PAIRWISE
                | FEATURE_SET_PSQ_THREATS_PAIRWISE
                | FEATURE_SET_PSQ_THREATS_PAIRWISE_HIDDEN
                | FEATURE_SET_HALFK_THREATS_PAIRWISE_HIDDEN
        ) || (is_stack && !screlu) || (is_material && !screlu);
        let network = PsqNetwork {
            embedding_dim, transform, transform_bias, hidden, output, step, learning_rate, threats, pairwise, screlu, stack, material,
            halfk,
        };
        network.validate().map_err(|problem| format!("{path}: {problem}"))?;
        Network::from_psq_through(network, HiddenArithmetic::Int16, &curve_of_version(version)).map_err(|problem| format!("{path}: {problem}"))
    }
}

/// The `parameters.txt` in the directory of the file at `path`: where a tune writes every search
/// parameter it settled on. One per directory, since each network has its own: a champion's
/// `gen-<g>/`, a candidate's `lambda-<l>/`.
pub fn parameters_file_beside(path: &str) -> String {
    let directory = Path::new(path).parent().unwrap_or(Path::new(""));
    directory.join("parameters.txt").to_string_lossy().into_owned()
}

/// The tuning beside the file at `path`: `parameters.txt` if present, else `Untuned`. A malformed
/// file is an `Err`.
pub fn read_tuning_beside(path: &str) -> Result<Tuning, String> {
    Ok(match SearchParameters::read_file(&parameters_file_beside(path))? {
        Some(parameters) => Tuning::Parameters(parameters),
        None => Tuning::Untuned,
    })
}

/// Every shape agreement the format itself cannot enforce. Run before a byte is written and after
/// the last one is read, so a malformed file and a malformed in-memory network fail the same way
/// with the same message. `head_input` is what the first head layer reads: twice `embedding_dim`
/// for psq's two perspectives.
fn validate_weights(
    rows: usize, embedding_dim: usize, head_input: usize, transform: &[f32], transform_bias: &[f32], hidden: &[Layer],
    output: &Layer,
) -> Result<(), String> {
    validate_transform(rows, embedding_dim, transform, transform_bias)?;
    // No hidden layer at all is a valid head: the output layer then reads the accumulator
    // directly, which is the loop below running zero times.
    //
    // The chain has to close: the first hidden layer reads the accumulator, each later one
    // reads its predecessor, and the output reads the last. A file whose dimensions are each
    // individually sane but do not connect would otherwise only fail deep inside the forward
    // pass, as a wrong number rather than an error.
    let mut input_dim = head_input;
    for (index, layer) in hidden.iter().enumerate() {
        layer.validate(&format!("hidden layer {index}"))?;
        if layer.in_dim != input_dim {
            return Err(format!(
                "hidden layer {index} reads {} values, but the layer before it produces {input_dim}",
                layer.in_dim
            ));
        }
        input_dim = layer.out_dim;
    }

    output.validate("output layer")?;
    if output.in_dim != input_dim {
        return Err(format!("output layer reads {} values, but the last hidden layer produces {input_dim}", output.in_dim));
    }
    if output.out_dim != 1 {
        return Err(format!("output layer produces {} values, expected exactly 1", output.out_dim));
    }
    Ok(())
}

/// The transform's shapes, the part every kind shares.
fn validate_transform(rows: usize, embedding_dim: usize, transform: &[f32], transform_bias: &[f32]) -> Result<(), String> {
    if embedding_dim == 0 {
        return Err("embedding_dim is 0".to_string());
    }
    if transform.len() != rows * embedding_dim {
        return Err(format!(
            "transform holds {} values, but {rows} rows x {embedding_dim} needs {}",
            transform.len(), rows * embedding_dim
        ));
    }
    if transform_bias.len() != embedding_dim {
        return Err(format!("transform bias holds {} values, but embedding_dim is {embedding_dim}", transform_bias.len()));
    }
    Ok(())
}

/// The byte layout of a transform-based network: header (magic, version, feature-set tag,
/// `embedding_dim`, the hidden widths, provenance), then transform, bias, and every head layer's
/// weight and bias in `layers`' order, all little-endian: each hidden layer and the output, or for a
/// stack each layer's weight sets in bucket order. The caller validates first.
#[allow(clippy::too_many_arguments)]
fn write_weights<'a>(
    path: &str, feature_set: u32, embedding_dim: usize, transform: &[f32], transform_bias: &[f32], hidden_widths: &[usize],
    layers: impl Iterator<Item = &'a Layer>, step: u64, learning_rate: f64,
) -> Result<(), String> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&feature_set.to_le_bytes());
    bytes.extend_from_slice(&(embedding_dim as u32).to_le_bytes());
    bytes.extend_from_slice(&(hidden_widths.len() as u32).to_le_bytes());
    for &width in hidden_widths {
        bytes.extend_from_slice(&(width as u32).to_le_bytes());
    }
    bytes.extend_from_slice(&step.to_le_bytes());
    bytes.extend_from_slice(&learning_rate.to_le_bytes());

    push_f32s(&mut bytes, transform);
    push_f32s(&mut bytes, transform_bias);
    for layer in layers {
        push_f32s(&mut bytes, &layer.weight);
        push_f32s(&mut bytes, &layer.bias);
    }

    fs::write(path, &bytes).map_err(|error| format!("{path}: {error}"))
}

fn push_f32s(bytes: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

/// A position in the byte buffer plus the path to name in any error. Every accessor checks the
/// remaining length first, so a truncated file produces a message saying what it ran out of rather
/// than a slice-index panic.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    path: &'a str,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        if self.at + count > self.bytes.len() {
            return Err(format!(
                "{}: truncated -- wanted {count} bytes at offset {}, but the file holds {}",
                self.path, self.at, self.bytes.len()
            ));
        }
        let slice = &self.bytes[self.at..self.at + count];
        self.at += count;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let slice = self.take(4)?;
        Ok(u32::from_le_bytes(slice.try_into().expect("take returned 4 bytes")))
    }

    fn u64(&mut self) -> Result<u64, String> {
        let slice = self.take(8)?;
        Ok(u64::from_le_bytes(slice.try_into().expect("take returned 8 bytes")))
    }

    fn f64(&mut self) -> Result<f64, String> {
        let slice = self.take(8)?;
        Ok(f64::from_le_bytes(slice.try_into().expect("take returned 8 bytes")))
    }

    fn f32s(&mut self, count: usize) -> Result<Vec<f32>, String> {
        let slice = self.take(count * 4)?;
        Ok(slice.as_chunks::<4>().0.iter().map(|&chunk| f32::from_le_bytes(chunk)).collect())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::stack_buckets::{count_bucket_of_board, material_bucket_of_board, phase_bucket_of_board};

    #[test]
    fn fit_label_scale_recovers_the_scale_a_network_s_pawns_were_made_with() {
        // A network whose pawn is exactly PeSTO's divided by 1.8, lopsided positions included: the
        // fit must find 1.8, to within the centipawn rounding, with almost no residual.
        let pairs: Vec<(i32, i32)> = (-2000..=2000).step_by(7).map(|hand| (hand, (hand as f64 / 1.8).round() as i32)).collect();
        let (c, rms) = fit_label_scale(&pairs);
        assert!((c - 1.8).abs() < 0.01, "c {c}");
        assert!(rms < 1e-3, "rms {rms}");
        let (c, _) = fit_label_scale(&pairs.iter().map(|&(hand, _)| (hand, hand)).collect::<Vec<_>>());
        assert!((c - 1.0).abs() < 0.01, "the same pawn gives 1: {c}");
    }

    fn temp_path(name: &str) -> String {
        let pid = std::process::id();
        std::env::temp_dir().join(format!("kalo-chess-nnue-{name}-{pid}.nnue")).to_str().unwrap().to_string()
    }

    /// Reads `path` back, insisting it is a psq network.
    fn load_psq(path: &str) -> PsqNetwork {
        let Network::Psq(network, _) = Network::load(path).expect("loading should succeed");
        network
    }

    /// Non-round values everywhere, so a layout error that swapped two blocks or read a bias where
    /// a weight belongs cannot pass by coincidence. Values are kept small (a few hundredths) so the
    /// clipped ReLU does not saturate everything to 1.0, which would make every position evaluate
    /// the same. `pub(crate)`: `uci.rs`'s and `search.rs`'s tests use one.
    pub(crate) fn sample_network(embedding_dim: usize, hidden_dims: &[usize]) -> PsqNetwork {
        sample_network_of(embedding_dim, hidden_dims, false)
    }

    /// `sample_network` of either kind: `threats` gives a psq_threats network.
    pub(crate) fn sample_network_of(embedding_dim: usize, hidden_dims: &[usize], threats: bool) -> PsqNetwork {
        sample_network_with(embedding_dim, hidden_dims, threats, false)
    }

    /// `sample_network`, pairwise: the head reads `embedding_dim` products.
    fn sample_pairwise_network(embedding_dim: usize, hidden_dims: &[usize]) -> PsqNetwork {
        sample_network_with(embedding_dim, hidden_dims, false, true)
    }

    fn sample_network_with(embedding_dim: usize, hidden_dims: &[usize], threats: bool, pairwise: bool) -> PsqNetwork {
        let rows = if threats { psq_threats::ROWS } else { psq::ROWS };
        let mut next = 0.0f32;
        let mut values = |count: usize| -> Vec<f32> {
            (0..count)
                .map(|_| {
                    next += 1.0;
                    ((next * 0.618).fract() - 0.5) * 0.05
                })
                .collect()
        };

        let transform = values(rows * embedding_dim);
        let transform_bias = values(embedding_dim);
        let mut hidden = Vec::new();
        let mut input_dim = if pairwise { embedding_dim } else { 2 * embedding_dim };
        for &out_dim in hidden_dims {
            hidden.push(Layer { in_dim: input_dim, out_dim, weight: values(out_dim * input_dim), bias: values(out_dim) });
            input_dim = out_dim;
        }
        let output = Layer { in_dim: input_dim, out_dim: 1, weight: values(input_dim), bias: values(1) };

        PsqNetwork {
            embedding_dim, transform, transform_bias, hidden, output, step: 512, learning_rate: 1e-3, threats, pairwise,
            screlu: false, stack: None, material: None, halfk: false,
        }
    }

    /// A stack sample network, its hidden widths `widths`, its transform bias raised as
    /// `lively_pairwise_network`'s, and every weight set distinct, so that a wrong bucket cannot pass
    /// by coincidence.
    fn sample_stack_network(embedding_dim: usize, widths: [usize; 2]) -> PsqNetwork {
        stack_over(lively_pairwise_network(embedding_dim, &[]), widths)
    }

    /// `sample_stack_network` over psq_threats' rows: a psq_threats_stack network.
    fn sample_threats_stack_network(embedding_dim: usize, widths: [usize; 2]) -> PsqNetwork {
        stack_over(lively_threats_pairwise_network(embedding_dim, &[]), widths)
    }

    /// `network`, a pairwise one with no hidden layer, its head replaced by a stack of `widths`.
    fn stack_over(mut network: PsqNetwork, widths: [usize; 2]) -> PsqNetwork {
        let embedding_dim = network.embedding_dim;
        let mut next = 0.0f32;
        let mut values = |count: usize| -> Vec<f32> {
            (0..count)
                .map(|_| {
                    next += 1.0;
                    ((next * 0.754877).fract() - 0.5) * 0.4
                })
                .collect()
        };
        let mut sets = |buckets: usize, in_dim: usize, out_dim: usize| -> Vec<Layer> {
            (0..buckets).map(|_| Layer { in_dim, out_dim, weight: values(out_dim * in_dim), bias: values(out_dim) }).collect()
        };
        let l1 = sets(OUR_KING_BUCKETS, embedding_dim, widths[0]);
        let l2 = sets(THEIR_KING_BUCKETS, widths[0], widths[1]);
        let output = sets(MATERIAL_BUCKETS, widths[1], 1);
        network.output = Layer::default();
        network.stack = Some(StackLayers { l1, l2, output });
        network
    }

    #[test]
    fn a_stack_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("stack-round-trip");
        let network = sample_stack_network(16, [8, 4]);
        network.write(&path).expect("writing a valid stack network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_STACK);
        let loaded = load_psq(&path);
        assert!(loaded.pairwise && loaded.stack.is_some());
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_stack_network_with_a_wrong_bucket_count_or_shape_is_refused() {
        let mut short = sample_stack_network(16, [8, 4]);
        short.stack.as_mut().unwrap().l2.pop();
        assert!(short.write(&temp_path("stack-short")).unwrap_err().contains("holds 63 weight sets"));

        let mut ragged = sample_stack_network(16, [8, 4]);
        let layer = &mut ragged.stack.as_mut().unwrap().output[5];
        *layer = Layer { in_dim: 3, out_dim: 1, weight: vec![0.0; 3], bias: vec![0.0] };
        assert!(ragged.write(&temp_path("stack-ragged")).unwrap_err().contains("bucket 5"));

        let mut unpaired = sample_stack_network(16, [8, 4]);
        unpaired.pairwise = false;
        assert!(unpaired.write(&temp_path("stack-unpaired")).unwrap_err().contains("reads pairwise products"));
    }

    #[test]
    fn a_threats_stack_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("threats-stack-round-trip");
        let network = sample_threats_stack_network(16, [8, 4]);
        network.write(&path).expect("writing a valid psq_threats_stack network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_THREATS_STACK);
        let loaded = load_psq(&path);
        assert!(loaded.pairwise && loaded.threats && loaded.stack.is_some());
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_quantized_threats_stack_network_stays_close_to_its_f32_original() {
        // As `a_quantized_stack_network_stays_close_to_its_f32_original_and_vectorized_equals_scalar`,
        // with threats of both sides on the first board, and the search's path against the reference.
        for (fen, widths) in [
            ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10", [16, 8]),
            ("8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1", [6, 4]),
        ] {
            let (board, _) = Board::from_fen(fen).unwrap();
            let network = sample_threats_stack_network(32, widths);
            let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
            let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
            assert!((exact - approximate).abs() <= 0.06, "{fen} {widths:?}: {approximate} against {exact}");
            let accumulators = quantized.refresh_by_color(&board);
            assert_eq!(quantized.evaluate_by_color_cp(&accumulators, &board), quantized.evaluate(&board), "{fen} {widths:?}");
        }
    }

    #[test]
    fn threats_stack_incremental_evaluation_reads_each_positions_buckets_with_saturated_lanes() {
        // The search's path: threat rows added at evaluation, saturating, then the head through the
        // position's buckets, whose every weight set is distinct, so reading bucket 0 everywhere
        // fails here. Threat rows of 100 take lanes past i16.
        use crate::movegen::legal::legal_moves;

        let mut heavy = sample_threats_stack_network(32, [16, 8]);
        for row in psq::ROWS..psq_threats::ROWS {
            let sign = if row % 2 == 0 { 1.0 } else { -1.0 };
            for lane in 0..32 {
                heavy.transform[row * 32 + lane] = sign * 100.0;
            }
        }
        let network = QuantizedPsqNetwork::quantize(&heavy).expect("100 at scale 255 fits i16");
        let mut saturated = false;
        let mut seed = 0xD1B54A32D192ED03u64;
        for _game in 0..6 {
            let (mut board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
            let mut accumulators = network.refresh_by_color(&board);
            let mut next = accumulators.clone();
            for ply in 0..100 {
                let moves = legal_moves(&board, board.side_to_move());
                if moves.is_empty() || board.halfmove_clock() >= 100 {
                    break;
                }
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let mut child = board;
                child.make_move(moves[(seed >> 32) as usize % moves.len()]);

                network.update(&accumulators, &mut next, &board, &child);
                assert_eq!(network.evaluate_by_color_cp(&next, &child), network.evaluate(&child), "ply {ply}");
                saturated |= network.refresh(&child, Color::White).iter().any(|lane| lane.abs() > i16::MAX as i32);
                std::mem::swap(&mut accumulators, &mut next);
                board = child;
            }
        }
        assert!(saturated, "no lane ever left i16, so saturation went untested");
    }

    #[test]
    fn the_stack_f32_head_takes_the_positions_buckets_weight_sets() {
        // Against a plain f64 computation through the sets the board's buckets name, Black to move so
        // the kings are flipped.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 0 10").unwrap();
        let network = sample_stack_network(16, [8, 4]);
        let stack = network.stack.as_ref().unwrap();
        let buckets = buckets_of_board(&board);
        let side_to_move = board.side_to_move();
        let own = network.refresh(&board, side_to_move);
        let opponent = network.refresh(&board, side_to_move.opposite());
        let mut current: Vec<f64> = Vec::new();
        for lanes in [&own, &opponent] {
            for i in 0..8 {
                current.push(lanes[i].clamp(0.0, 1.0) as f64 * lanes[i + 8].clamp(0.0, 1.0) as f64);
            }
        }
        for (layer, clip) in [(&stack.l1[buckets[0]], true), (&stack.l2[buckets[1]], true), (&stack.output[buckets[2]], false)] {
            current = (0..layer.out_dim)
                .map(|out| {
                    let row = &layer.weight[out * layer.in_dim..(out + 1) * layer.in_dim];
                    let value = layer.bias[out] as f64 + row.iter().zip(&current).map(|(w, x)| *w as f64 * x).sum::<f64>();
                    if clip { value.clamp(0.0, 1.0) } else { value }
                })
                .collect();
        }
        let fast = network.evaluate_pawns(&board) as f64;
        assert!((fast - current[0]).abs() <= 1e-5, "{fast} against {}", current[0]);
    }

    #[test]
    fn a_quantized_stack_network_stays_close_to_its_f32_original_and_vectorized_equals_scalar() {
        // Halves of 16 lanes and a first layer of 16 outputs take the vectorized kernels; a first
        // layer of 6 the scalar loops.
        for (fen, widths) in [
            ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10", [16, 8]),
            ("8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1", [16, 8]),
            ("8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1", [6, 4]),
        ] {
            let (board, _) = Board::from_fen(fen).unwrap();
            let network = sample_stack_network(32, widths);
            let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
            let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
            // A product is off by under 0.008 (`a_quantized_pairwise_network_stays_close_to_its_f32_original`);
            // 32 of them under weights of at most 0.2, then two layers of at most 1.6 and 0.8 in
            // total weight: 0.06 at the very worst, the errors rarely of one sign.
            assert!((exact - approximate).abs() <= 0.06, "{fen} {widths:?}: {approximate} against {exact}");
            let accumulators = quantized.refresh_by_color(&board);
            assert_eq!(quantized.evaluate_by_color_cp(&accumulators, &board), quantized.evaluate(&board), "{fen} {widths:?}");
        }
    }

    /// A psq_threats_screlu_kings sample network: `lively_threats_screlu_network`'s transform, its
    /// output replaced by each side's layers of `width` over its `embedding_dim` squares, every king's
    /// set distinct and our side's apart from theirs, so that a wrong bucket, a missing flip or the
    /// sides swapped cannot pass by coincidence, and the one output. Every weight within 0.05 of zero.
    fn sample_kings_network(embedding_dim: usize, width: usize) -> PsqNetwork {
        let mut network = lively_threats_screlu_network(embedding_dim);
        let mut next = 0.0f32;
        let mut values = |count: usize, span: f32| -> Vec<f32> {
            (0..count)
                .map(|_| {
                    next += 1.0;
                    ((next * 0.754877).fract() - 0.5) * span
                })
                .collect()
        };
        let mut sets = |buckets: usize, in_dim: usize, out_dim: usize, span: f32| -> Vec<Layer> {
            (0..buckets)
                .map(|_| Layer { in_dim, out_dim, weight: values(out_dim * in_dim, span), bias: values(out_dim, 0.4) })
                .collect()
        };
        let l1 = sets(OUR_KING_BUCKETS, embedding_dim, width, 0.1);
        let l2 = sets(THEIR_KING_BUCKETS, embedding_dim, width, 0.1);
        let output = sets(1, 4 * width + 2 * embedding_dim, 1, 0.1);
        network.output = Layer::default();
        network.stack = Some(StackLayers { l1, l2, output });
        network
    }

    /// `kings_head` in f64, written out from the design rather than shared with it: each side's
    /// squares through its king's set, theirs in their own frame, the values clipped and squared,
    /// ours first, then every square, own first, into the output.
    fn kings_reference(network: &PsqNetwork, board: &Board) -> f64 {
        let stack = network.stack.as_ref().unwrap();
        let [our_king, their_king, _] = buckets_of_board(board);
        let side_to_move = board.side_to_move();
        let squares = |perspective: Color| -> Vec<f64> {
            network.refresh(board, perspective).iter().map(|lane| (lane.clamp(0.0, 1.0) as f64).powi(2)).collect()
        };
        let (own, opponent) = (squares(side_to_move), squares(side_to_move.opposite()));
        let mut inputs = Vec::new();
        for (layer, side) in [(&stack.l1[our_king], &own), (&stack.l2[their_king ^ 56], &opponent)] {
            let values: Vec<f64> = (0..layer.out_dim)
                .map(|out| {
                    let row = &layer.weight[out * layer.in_dim..(out + 1) * layer.in_dim];
                    (layer.bias[out] as f64 + row.iter().zip(side).map(|(w, x)| *w as f64 * x).sum::<f64>()).clamp(0.0, 1.0)
                })
                .collect();
            inputs.extend(&values);
            inputs.extend(values.iter().map(|value| value * value));
        }
        inputs.extend(&own);
        inputs.extend(&opponent);
        let output = &stack.output[0];
        output.bias[0] as f64 + output.weight.iter().zip(&inputs).map(|(w, x)| *w as f64 * x).sum::<f64>()
    }

    #[test]
    fn a_kings_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("screlu-kings-round-trip");
        let network = sample_kings_network(16, 8);
        network.write(&path).expect("writing a valid psq_threats_screlu_kings network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_THREATS_SCRELU_KINGS);
        // One hidden width in the header, a side's.
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 1);
        let loaded = load_psq(&path);
        assert!(loaded.screlu && loaded.threats && !loaded.pairwise && loaded.material.is_none());
        assert_eq!(loaded.stack.as_ref().map(|stack| stack.output.len()), Some(1));
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_kings_network_of_any_other_shape_is_refused() {
        // Output buckets belong to the pairwise stack, one output to the kings.
        let mut bucketed = sample_kings_network(16, 8);
        let output = bucketed.stack.as_ref().unwrap().output[0].clone();
        bucketed.stack.as_mut().unwrap().output = vec![output; MATERIAL_BUCKETS];
        assert!(bucketed.write(&temp_path("kings-bucketed")).unwrap_err().contains("the kings' output holds 78 weight sets, but has 1"));

        let mut single = sample_threats_stack_network(16, [8, 4]);
        single.stack.as_mut().unwrap().output.truncate(1);
        assert!(single.write(&temp_path("stack-single")).unwrap_err().contains("holds 1 weight sets, but has 78 buckets"));

        // Both sides' squares into one side's layer.
        let mut wide = sample_kings_network(16, 8);
        wide.stack.as_mut().unwrap().l1[3] = Layer { in_dim: 32, out_dim: 8, weight: vec![0.0; 256], bias: vec![0.0; 8] };
        assert!(wide.write(&temp_path("kings-wide")).unwrap_err().contains("our side's layer, bucket 3"));

        // Their side narrower than ours.
        let mut uneven = sample_kings_network(16, 8);
        uneven.stack.as_mut().unwrap().l2[5] = Layer { in_dim: 16, out_dim: 4, weight: vec![0.0; 64], bias: vec![0.0; 4] };
        assert!(uneven.write(&temp_path("kings-uneven")).unwrap_err().contains("their side's layer, bucket 5"));

        // An output reading the layers' values alone, no squares: no skip.
        let mut no_skip = sample_kings_network(16, 8);
        no_skip.stack.as_mut().unwrap().output[0] = Layer { in_dim: 32, out_dim: 1, weight: vec![0.0; 32], bias: vec![0.0] };
        assert!(no_skip.write(&temp_path("kings-no-skip")).unwrap_err().contains("the kings' output: 32 -> 1, but the layer is 64 -> 1"));

        let mut hidden = sample_kings_network(16, 8);
        hidden.hidden = vec![Layer { in_dim: 32, out_dim: 4, weight: vec![0.0; 128], bias: vec![0.0; 4] }];
        assert!(hidden.write(&temp_path("kings-hidden")).unwrap_err().contains("a screlu network is psq_threats with no hidden layer"));

        let mut paired = sample_kings_network(16, 8);
        paired.pairwise = true;
        assert!(paired.write(&temp_path("kings-paired")).unwrap_err().contains("not pairwise"));
    }

    #[test]
    fn the_kings_f32_head_reads_each_side_through_its_own_kings_set_and_the_squares_directly() {
        // Against `kings_reference`, in f64: Black to move, so the side to move's frame is flipped,
        // and White to move; a rook and pawn ending, kings far from the middlegame's.
        for fen in [
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 0 10",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
            "8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1",
        ] {
            let (board, _) = Board::from_fen(fen).unwrap();
            let network = sample_kings_network(16, 8);
            let (fast, expected) = (network.evaluate_pawns(&board) as f64, kings_reference(&network, &board));
            assert!((fast - expected).abs() <= 1e-5, "{fen}: {fast} against {expected}");
        }
    }

    #[test]
    fn the_kings_head_reads_their_king_in_their_own_frame() {
        // Every set of their side zero but the one their king picks in their own frame, whose
        // value is lifted: unflipped, the head would read a zero set and lose the lift.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
        let [_, their_king, _] = buckets_of_board(&board);
        let mut network = sample_kings_network(16, 8);
        let stack = network.stack.as_mut().unwrap();
        for (bucket, layer) in stack.l2.iter_mut().enumerate() {
            layer.weight.iter_mut().for_each(|weight| *weight = 0.0);
            layer.bias.iter_mut().for_each(|bias| *bias = if bucket == their_king ^ 56 { 0.5 } else { 0.0 });
        }
        assert_ne!(their_king, their_king ^ 56);
        let baseline = network.evaluate_pawns(&board);
        let stack = network.stack.as_mut().unwrap();
        // Their side's clipped values, at 16..24 of the output's weights: ours clipped, ours squared,
        // then theirs.
        stack.output[0].weight[16..24].iter_mut().for_each(|weight| *weight += 1.0);
        assert!((network.evaluate_pawns(&board) - baseline - 4.0).abs() <= 1e-4, "eight values of 0.5 under a weight of 1 more");
    }

    #[test]
    fn a_quantized_kings_network_stays_close_to_its_f32_original_and_vectorized_equals_scalar() {
        // Under 0.008 a square (`a_quantized_screlu_network_stays_close_to_its_f32_original`), at
        // most 32 of them under weights of at most 0.05 into each value, then 64 inputs and 64 squares
        // under the output's 0.05: within 0.06. Width 32 and a side's 16 take the kernels, width 24
        // or a side's 6 the scalar loops.
        for (fen, embedding_dim, width) in [
            ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10", 32, 16),
            ("8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1", 32, 16),
            ("8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1", 24, 16),
            ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10", 32, 6),
        ] {
            let (board, _) = Board::from_fen(fen).unwrap();
            let network = sample_kings_network(embedding_dim, width);
            let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
            assert_eq!(quantized.accumulator_one() >> quantized.accumulator_shift, 255);
            let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
            assert!((exact - approximate).abs() <= 0.06, "{fen} {embedding_dim} {width}: {approximate} against {exact}");
            let accumulators = quantized.refresh_by_color(&board);
            assert_eq!(quantized.evaluate_by_color_cp(&accumulators, &board), quantized.evaluate(&board), "{fen} {embedding_dim} {width}");
        }
    }

    #[test]
    fn kings_incremental_evaluation_reads_each_positions_kings_with_saturated_lanes() {
        // The search's path: threat rows added at evaluation, saturating, then the head through the
        // position's king sets, every one distinct, so reading bucket 0 everywhere fails here. Threat
        // rows of 100 take lanes past i16; widths 32 and 24 take the kernel and the scalar loop.
        use crate::movegen::legal::legal_moves;

        for embedding_dim in [32, 24] {
            let mut heavy = sample_kings_network(embedding_dim, 16);
            for row in psq::ROWS..psq_threats::ROWS {
                let sign = if row % 2 == 0 { 1.0 } else { -1.0 };
                for lane in 0..embedding_dim {
                    heavy.transform[row * embedding_dim + lane] = sign * 100.0;
                }
            }
            let network = QuantizedPsqNetwork::quantize(&heavy).expect("100 at scale 255 fits i16");
            let mut saturated = false;
            let mut kings = std::collections::BTreeSet::new();
            let mut seed = 0xC2B2AE3D27D4EB4Fu64;
            for _game in 0..6 {
                let (mut board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
                let mut accumulators = network.refresh_by_color(&board);
                let mut next = accumulators.clone();
                for ply in 0..100 {
                    let moves = legal_moves(&board, board.side_to_move());
                    if moves.is_empty() || board.halfmove_clock() >= 100 {
                        break;
                    }
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let mut child = board;
                    child.make_move(moves[(seed >> 32) as usize % moves.len()]);

                    network.update(&accumulators, &mut next, &board, &child);
                    assert_eq!(network.evaluate_by_color_cp(&next, &child), network.evaluate(&child), "width {embedding_dim}, ply {ply}");
                    saturated |= network.refresh(&child, Color::White).iter().any(|lane| lane.abs() > i16::MAX as i32);
                    let [our_king, their_king, _] = buckets_of_board(&child);
                    kings.insert((our_king, their_king));
                    std::mem::swap(&mut accumulators, &mut next);
                    board = child;
                }
            }
            assert!(saturated, "width {embedding_dim}: no lane ever left i16, so saturation went untested");
            assert!(kings.len() > 3, "width {embedding_dim}: the kings should visit several buckets, saw {kings:?}");
        }
    }

    /// A psq_threats_screlu_hidden sample network: `lively_threats_screlu_network`'s transform, a
    /// hidden layer of `width` over the `2 * embedding_dim` squares and the output over its values
    /// clipped and squared and every square. Every weight within 0.05 of zero.
    fn sample_screlu_hidden_network(embedding_dim: usize, width: usize) -> PsqNetwork {
        let mut network = lively_threats_screlu_network(embedding_dim);
        let mut next = 0.0f32;
        let mut values = |count: usize, span: f32| -> Vec<f32> {
            (0..count)
                .map(|_| {
                    next += 1.0;
                    ((next * 0.754877).fract() - 0.5) * span
                })
                .collect()
        };
        let squares = 2 * embedding_dim;
        network.hidden = vec![Layer { in_dim: squares, out_dim: width, weight: values(width * squares, 0.1), bias: values(width, 0.4) }];
        let input = 2 * width + squares;
        network.output = Layer { in_dim: input, out_dim: 1, weight: values(input, 0.1), bias: values(1, 0.4) };
        network
    }

    /// `hidden_beside_head` in f64 over the squares, written out from the design: every square through the hidden
    /// layer, the values clipped and squared, then every square, into the output.
    fn screlu_hidden_reference(network: &PsqNetwork, board: &Board) -> f64 {
        let side_to_move = board.side_to_move();
        let squares: Vec<f64> = [side_to_move, side_to_move.opposite()]
            .iter()
            .flat_map(|&perspective| network.refresh(board, perspective))
            .map(|lane| (lane.clamp(0.0, 1.0) as f64).powi(2))
            .collect();
        let hidden = &network.hidden[0];
        let values: Vec<f64> = (0..hidden.out_dim)
            .map(|out| {
                let row = &hidden.weight[out * hidden.in_dim..(out + 1) * hidden.in_dim];
                (hidden.bias[out] as f64 + row.iter().zip(&squares).map(|(w, x)| *w as f64 * x).sum::<f64>()).clamp(0.0, 1.0)
            })
            .collect();
        let inputs: Vec<f64> = values.iter().copied().chain(values.iter().map(|value| value * value)).chain(squares).collect();
        network.output.bias[0] as f64 + network.output.weight.iter().zip(&inputs).map(|(w, x)| *w as f64 * x).sum::<f64>()
    }

    #[test]
    fn a_screlu_hidden_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("screlu-hidden-round-trip");
        let network = sample_screlu_hidden_network(16, 8);
        network.write(&path).expect("writing a valid psq_threats_screlu_hidden network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_THREATS_SCRELU_HIDDEN);
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 1);
        let loaded = load_psq(&path);
        assert!(loaded.screlu && loaded.threats && !loaded.pairwise && loaded.stack.is_none() && loaded.material.is_none());
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_screlu_hidden_network_of_any_other_shape_is_refused() {
        // An output reading the hidden values alone, no squares: no skip.
        let mut no_skip = sample_screlu_hidden_network(16, 8);
        no_skip.output = Layer { in_dim: 16, out_dim: 1, weight: vec![0.0; 16], bias: vec![0.0] };
        assert!(no_skip.write(&temp_path("screlu-hidden-no-skip")).unwrap_err().contains("the screlu output: 16 -> 1, but the layer is 48 -> 1"));

        // A hidden layer reading one side's squares only.
        let mut one_side = sample_screlu_hidden_network(16, 8);
        one_side.hidden[0] = Layer { in_dim: 16, out_dim: 8, weight: vec![0.0; 128], bias: vec![0.0; 8] };
        assert!(one_side.write(&temp_path("screlu-hidden-one-side")).unwrap_err().contains("reads 16 values, but there are 32 squares"));

        let mut paired = sample_screlu_hidden_network(16, 8);
        paired.pairwise = true;
        assert!(paired.write(&temp_path("screlu-hidden-paired")).unwrap_err().contains("not pairwise"));
    }

    #[test]
    fn the_screlu_hidden_f32_head_reads_the_squares_through_the_hidden_layer_and_directly() {
        for fen in [
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 0 10",
            "8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1",
        ] {
            let (board, _) = Board::from_fen(fen).unwrap();
            let network = sample_screlu_hidden_network(16, 8);
            let (fast, expected) = (network.evaluate_pawns(&board) as f64, screlu_hidden_reference(&network, &board));
            assert!((fast - expected).abs() <= 1e-5, "{fen}: {fast} against {expected}");
        }
    }

    #[test]
    fn a_quantized_screlu_hidden_network_stays_close_to_its_f32_original_and_vectorized_equals_scalar() {
        // Under 0.008 a square, 64 squares under weights of at most 0.05 into each value, then 32
        // values and 64 squares under the output's 0.05: within 0.06. Width 32 and a layer of 16 take
        // the kernels, width 24 or a layer of 6 the scalar loops.
        for (fen, embedding_dim, width) in [
            ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10", 32, 16),
            ("8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1", 32, 16),
            ("8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1", 24, 16),
            ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10", 32, 6),
        ] {
            let (board, _) = Board::from_fen(fen).unwrap();
            let network = sample_screlu_hidden_network(embedding_dim, width);
            let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
            let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
            assert!((exact - approximate).abs() <= 0.06, "{fen} {embedding_dim} {width}: {approximate} against {exact}");
            let accumulators = quantized.refresh_by_color(&board);
            assert_eq!(quantized.evaluate_by_color_cp(&accumulators, &board), quantized.evaluate(&board), "{fen} {embedding_dim} {width}");
        }
    }

    #[test]
    fn screlu_hidden_incremental_evaluation_matches_from_scratch_with_saturated_lanes() {
        // The search's path: threat rows added at evaluation, saturating, then the hidden head.
        // Threat rows of 100 take lanes past i16; widths 32 and 24 take the kernel and the scalar loop.
        use crate::movegen::legal::legal_moves;

        for embedding_dim in [32, 24] {
            let mut heavy = sample_screlu_hidden_network(embedding_dim, 16);
            for row in psq::ROWS..psq_threats::ROWS {
                let sign = if row % 2 == 0 { 1.0 } else { -1.0 };
                for lane in 0..embedding_dim {
                    heavy.transform[row * embedding_dim + lane] = sign * 100.0;
                }
            }
            let network = QuantizedPsqNetwork::quantize(&heavy).expect("100 at scale 255 fits i16");
            let mut saturated = false;
            let mut seed = 0x9E3779B97F4A7C15u64;
            for _game in 0..4 {
                let (mut board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
                let mut accumulators = network.refresh_by_color(&board);
                let mut next = accumulators.clone();
                for ply in 0..80 {
                    let moves = legal_moves(&board, board.side_to_move());
                    if moves.is_empty() || board.halfmove_clock() >= 100 {
                        break;
                    }
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let mut child = board;
                    child.make_move(moves[(seed >> 32) as usize % moves.len()]);
                    network.update(&accumulators, &mut next, &board, &child);
                    assert_eq!(network.evaluate_by_color_cp(&next, &child), network.evaluate(&child), "width {embedding_dim}, ply {ply}");
                    saturated |= network.refresh(&child, Color::White).iter().any(|lane| lane.abs() > i16::MAX as i32);
                    std::mem::swap(&mut accumulators, &mut next);
                    board = child;
                }
            }
            assert!(saturated, "width {embedding_dim}: no lane ever left i16, so saturation went untested");
        }
    }

    /// A psq_threats_pairwise_hidden sample network: `lively_threats_pairwise_network`'s transform, a
    /// hidden layer of `width` over the `embedding_dim` products and the output over its values
    /// clipped and squared and every product. Every weight within 0.05 of zero.
    fn sample_pairwise_hidden_network(embedding_dim: usize, width: usize) -> PsqNetwork {
        let mut network = lively_threats_pairwise_network(embedding_dim, &[]);
        let mut next = 0.0f32;
        let mut values = |count: usize, span: f32| -> Vec<f32> {
            (0..count)
                .map(|_| {
                    next += 1.0;
                    ((next * 0.754877).fract() - 0.5) * span
                })
                .collect()
        };
        let products = embedding_dim;
        network.hidden = vec![Layer { in_dim: products, out_dim: width, weight: values(width * products, 0.1), bias: values(width, 0.4) }];
        let input = 2 * width + products;
        network.output = Layer { in_dim: input, out_dim: 1, weight: values(input, 0.1), bias: values(1, 0.4) };
        network
    }

    /// `hidden_beside_head` in f64 over the products, written out from the design: each
    /// perspective's halves clipped and multiplied, own first, through the hidden layer, the values
    /// clipped and squared, then every product, into the output.
    fn pairwise_hidden_reference(network: &PsqNetwork, board: &Board) -> f64 {
        let side_to_move = board.side_to_move();
        let products: Vec<f64> = [side_to_move, side_to_move.opposite()]
            .iter()
            .flat_map(|&perspective| {
                let lanes = network.refresh(board, perspective);
                let half = lanes.len() / 2;
                (0..half).map(move |i| lanes[i].clamp(0.0, 1.0) as f64 * lanes[i + half].clamp(0.0, 1.0) as f64).collect::<Vec<_>>()
            })
            .collect();
        let hidden = &network.hidden[0];
        let values: Vec<f64> = (0..hidden.out_dim)
            .map(|out| {
                let row = &hidden.weight[out * hidden.in_dim..(out + 1) * hidden.in_dim];
                (hidden.bias[out] as f64 + row.iter().zip(&products).map(|(w, x)| *w as f64 * x).sum::<f64>()).clamp(0.0, 1.0)
            })
            .collect();
        let inputs: Vec<f64> = values.iter().copied().chain(values.iter().map(|value| value * value)).chain(products).collect();
        network.output.bias[0] as f64 + network.output.weight.iter().zip(&inputs).map(|(w, x)| *w as f64 * x).sum::<f64>()
    }

    #[test]
    fn a_pairwise_hidden_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("pairwise-hidden-round-trip");
        let network = sample_pairwise_hidden_network(16, 8);
        network.write(&path).expect("writing a valid psq_threats_pairwise_hidden network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_THREATS_PAIRWISE_HIDDEN);
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 1);
        let loaded = load_psq(&path);
        assert!(loaded.pairwise && loaded.threats && !loaded.screlu && loaded.stack.is_none() && loaded.material.is_none());
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_pairwise_network_whose_output_reads_the_hidden_layer_alone_stays_psq_threats_pairwise() {
        // No skip: the plain hidden layer psq_threats_pairwise always allowed, under its own tag.
        let path = temp_path("pairwise-hidden-no-skip");
        let mut network = sample_pairwise_hidden_network(16, 8);
        network.output = Layer { in_dim: 8, out_dim: 1, weight: vec![0.01; 8], bias: vec![0.0] };
        network.write(&path).expect("a plain pairwise hidden layer is valid");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_THREATS_PAIRWISE);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_pairwise_hidden_network_whose_hidden_layer_reads_the_wrong_count_is_refused() {
        // A hidden layer reading 8 values, the output still sized for 16 products.
        let mut half = sample_pairwise_hidden_network(16, 8);
        half.hidden[0] = Layer { in_dim: 8, out_dim: 8, weight: vec![0.0; 64], bias: vec![0.0; 8] };
        let error = half.write(&temp_path("pairwise-hidden-half")).unwrap_err();
        assert!(error.contains("the pairwise hidden layer reads 8 values, but there are 16 products"), "{error}");
    }

    #[test]
    fn the_pairwise_hidden_f32_head_reads_the_products_through_the_hidden_layer_and_directly() {
        for fen in [
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 0 10",
            "8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1",
        ] {
            let (board, _) = Board::from_fen(fen).unwrap();
            let network = sample_pairwise_hidden_network(16, 8);
            let (fast, expected) = (network.evaluate_pawns(&board) as f64, pairwise_hidden_reference(&network, &board));
            assert!((fast - expected).abs() <= 1e-5, "{fen}: {fast} against {expected}");
        }
    }

    #[test]
    fn a_quantized_pairwise_hidden_network_stays_close_to_its_f32_original_and_vectorized_equals_scalar() {
        // Width 64 (halves of 32) and a layer of 16 take the kernels, width 48 (halves of 24) or a
        // layer of 6 the scalar loops.
        for (fen, embedding_dim, width) in [
            ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10", 64, 16),
            ("8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1", 64, 16),
            ("8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1", 48, 16),
            ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10", 64, 6),
        ] {
            let (board, _) = Board::from_fen(fen).unwrap();
            let network = sample_pairwise_hidden_network(embedding_dim, width);
            let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
            let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
            assert!((exact - approximate).abs() <= 0.06, "{fen} {embedding_dim} {width}: {approximate} against {exact}");
            let accumulators = quantized.refresh_by_color(&board);
            assert_eq!(quantized.evaluate_by_color_cp(&accumulators, &board), quantized.evaluate(&board), "{fen} {embedding_dim} {width}");
        }
    }

    #[test]
    fn a_hidden_layer_beside_the_output_stays_f32_over_the_whole_products() {
        // Not quantized: the file's weights divided by the whole product's 1.0, its biases as they
        // are, for both kinds.
        for network in [sample_screlu_hidden_network(32, 16), sample_pairwise_hidden_network(64, 16)] {
            let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
            let QuantizedHead::HiddenBeside { hidden, .. } = &quantized.head else {
                panic!("a hidden layer beside the output quantizes as HiddenBeside");
            };
            let original = &network.hidden[0];
            assert_eq!((hidden.in_dim, hidden.out_dim), (original.in_dim, original.out_dim));
            assert_eq!(hidden.bias, original.bias);
            let expected: Vec<f32> = original.weight.iter().map(|weight| weight / WHOLE_ONE).collect();
            assert_eq!(hidden.weight, expected);
        }
    }

    #[test]
    fn pairwise_hidden_incremental_evaluation_matches_from_scratch_with_saturated_lanes() {
        // As screlu_hidden's: threat rows of 100 take lanes past i16; widths 64 and 48 take the
        // kernel and the scalar loop.
        use crate::movegen::legal::legal_moves;

        for embedding_dim in [64, 48] {
            let mut heavy = sample_pairwise_hidden_network(embedding_dim, 16);
            for row in psq::ROWS..psq_threats::ROWS {
                let sign = if row % 2 == 0 { 1.0 } else { -1.0 };
                for lane in 0..embedding_dim {
                    heavy.transform[row * embedding_dim + lane] = sign * 100.0;
                }
            }
            let network = QuantizedPsqNetwork::quantize(&heavy).expect("100 at scale 255 fits i16");
            let mut saturated = false;
            let mut seed = 0x9E3779B97F4A7C15u64;
            for _game in 0..4 {
                let (mut board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
                let mut accumulators = network.refresh_by_color(&board);
                let mut next = accumulators.clone();
                for ply in 0..80 {
                    let moves = legal_moves(&board, board.side_to_move());
                    if moves.is_empty() || board.halfmove_clock() >= 100 {
                        break;
                    }
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let mut child = board;
                    child.make_move(moves[(seed >> 32) as usize % moves.len()]);
                    network.update(&accumulators, &mut next, &board, &child);
                    assert_eq!(network.evaluate_by_color_cp(&next, &child), network.evaluate(&child), "width {embedding_dim}, ply {ply}");
                    saturated |= network.refresh(&child, Color::White).iter().any(|lane| lane.abs() > i16::MAX as i32);
                    std::mem::swap(&mut accumulators, &mut next);
                    board = child;
                }
            }
            assert!(saturated, "width {embedding_dim}: no lane ever left i16, so saturation went untested");
        }
    }

    /// A pairwise sample network with its transform bias raised, so that most halves sit well
    /// inside `(0, 1)` and the products are not all near zero, which would let a wrong product
    /// pass unseen.
    fn lively_pairwise_network(embedding_dim: usize, hidden_dims: &[usize]) -> PsqNetwork {
        let mut network = sample_pairwise_network(embedding_dim, hidden_dims);
        for (lane, bias) in network.transform_bias.iter_mut().enumerate() {
            *bias += 0.3 + 0.4 * (lane % 3) as f32 / 2.0;
        }
        network
    }

    /// `lively_pairwise_network` over psq_threats' rows: a psq_threats_pairwise network.
    fn lively_threats_pairwise_network(embedding_dim: usize, hidden_dims: &[usize]) -> PsqNetwork {
        let mut network = sample_network_with(embedding_dim, hidden_dims, true, true);
        for (lane, bias) in network.transform_bias.iter_mut().enumerate() {
            *bias += 0.3 + 0.4 * (lane % 3) as f32 / 2.0;
        }
        network
    }

    /// A psq_threats_pairwise_material sample network: `lively_threats_pairwise_network`'s transform,
    /// its output replaced by `MATERIAL_BUCKETS` sets, every one distinct, so that a wrong bucket
    /// cannot pass by coincidence.
    fn sample_material_network(embedding_dim: usize) -> PsqNetwork {
        let mut network = lively_threats_pairwise_network(embedding_dim, &[]);
        network.output = Layer::default();
        network.material = Some(material_sets(embedding_dim, 0.4));
        network
    }

    /// `MATERIAL_BUCKETS` output sets of `in_dim` inputs, every value distinct, within `width / 2` of
    /// zero.
    fn material_sets(in_dim: usize, width: f32) -> Vec<Layer> {
        let mut next = 0.0f32;
        let mut values = |count: usize| -> Vec<f32> {
            (0..count)
                .map(|_| {
                    next += 1.0;
                    ((next * 0.754877).fract() - 0.5) * width
                })
                .collect()
        };
        (0..MATERIAL_BUCKETS).map(|_| Layer { in_dim, out_dim: 1, weight: values(in_dim), bias: values(1) }).collect()
    }

    #[test]
    fn a_material_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("material-round-trip");
        let network = sample_material_network(16);
        network.write(&path).expect("writing a valid material network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_THREATS_PAIRWISE_MATERIAL);
        let loaded = load_psq(&path);
        assert!(loaded.pairwise && loaded.threats && loaded.material.is_some() && loaded.stack.is_none());
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_material_network_with_a_wrong_set_count_or_shape_or_kind_is_refused() {
        let mut short = sample_material_network(16);
        short.material.as_mut().unwrap().pop();
        assert!(short.write(&temp_path("material-short")).unwrap_err().contains("holds 77 weight sets"));

        let mut ragged = sample_material_network(16);
        ragged.material.as_mut().unwrap()[5] = Layer { in_dim: 3, out_dim: 1, weight: vec![0.0; 3], bias: vec![0.0] };
        assert!(ragged.write(&temp_path("material-ragged")).unwrap_err().contains("bucket 5"));

        let mut unpaired = sample_material_network(16);
        unpaired.pairwise = false;
        assert!(unpaired.write(&temp_path("material-unpaired")).unwrap_err().contains("psq_threats pairwise"));

        let mut with_output = sample_material_network(16);
        with_output.output = Layer { in_dim: 16, out_dim: 1, weight: vec![0.0; 16], bias: vec![0.0] };
        assert!(with_output.write(&temp_path("material-output")).unwrap_err().contains("must be empty"));
    }

    #[test]
    fn the_material_f32_head_takes_the_positions_material_set() {
        // Against a plain f64 computation through the set of the board's material bucket: the
        // start (bucket 77) and a rook and pawn ending (material 6, bucket 5).
        for fen in ["rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", "8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1"] {
            let (board, _) = Board::from_fen(fen).unwrap();
            let network = sample_material_network(16);
            let bucket = material_bucket_of_board(&board);
            let layer = &network.material.as_ref().unwrap()[bucket];
            let side_to_move = board.side_to_move();
            let own = network.refresh(&board, side_to_move);
            let opponent = network.refresh(&board, side_to_move.opposite());
            let mut products: Vec<f64> = Vec::new();
            for lanes in [&own, &opponent] {
                for i in 0..8 {
                    products.push(lanes[i].clamp(0.0, 1.0) as f64 * lanes[i + 8].clamp(0.0, 1.0) as f64);
                }
            }
            let expected = layer.bias[0] as f64 + layer.weight.iter().zip(&products).map(|(w, x)| *w as f64 * x).sum::<f64>();
            let fast = network.evaluate_pawns(&board) as f64;
            assert!((fast - expected).abs() <= 1e-5, "{fen}: {fast} against {expected}");
        }
    }

    #[test]
    fn a_quantized_material_network_stays_close_to_its_f32_original_and_vectorized_equals_scalar() {
        // Halves of 16 lanes take the vectorized kernel, of 12 the scalar loops; threats of both
        // sides on the first board.
        for fen in ["r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10", "8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1"] {
            for embedding_dim in [32, 24] {
                let (board, _) = Board::from_fen(fen).unwrap();
                let network = sample_material_network(embedding_dim);
                let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
                let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
                assert!((exact - approximate).abs() <= 0.06, "{fen} {embedding_dim}: {approximate} against {exact}");
                let accumulators = quantized.refresh_by_color(&board);
                assert_eq!(quantized.evaluate_by_color_cp(&accumulators, &board), quantized.evaluate(&board), "{fen} {embedding_dim}");
            }
        }
    }

    #[test]
    fn material_incremental_evaluation_follows_the_material_with_saturated_lanes() {
        // The search's path through random games: piece-square accumulators updated move by move,
        // the threat rows added at evaluation, saturating, then the set of the position's material
        // bucket, which captures change. Threat rows of 100 take lanes past i16.
        use crate::movegen::legal::legal_moves;

        let mut heavy = sample_material_network(32);
        for row in psq::ROWS..psq_threats::ROWS {
            let sign = if row % 2 == 0 { 1.0 } else { -1.0 };
            for lane in 0..32 {
                heavy.transform[row * 32 + lane] = sign * 100.0;
            }
        }
        let network = QuantizedPsqNetwork::quantize(&heavy).expect("100 at scale 255 fits i16");
        let mut saturated = false;
        let mut buckets = std::collections::BTreeSet::new();
        let mut seed = 0xA0761D6478BD642Fu64;
        for _game in 0..6 {
            let (mut board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
            let mut accumulators = network.refresh_by_color(&board);
            let mut next = accumulators.clone();
            for ply in 0..100 {
                let moves = legal_moves(&board, board.side_to_move());
                if moves.is_empty() || board.halfmove_clock() >= 100 {
                    break;
                }
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let mut child = board;
                child.make_move(moves[(seed >> 32) as usize % moves.len()]);

                network.update(&accumulators, &mut next, &board, &child);
                assert_eq!(network.evaluate_by_color_cp(&next, &child), network.evaluate(&child), "ply {ply}");
                saturated |= network.refresh(&child, Color::White).iter().any(|lane| lane.abs() > i16::MAX as i32);
                buckets.insert(material_bucket_of_board(&child));
                std::mem::swap(&mut accumulators, &mut next);
                board = child;
            }
        }
        assert!(saturated, "no lane ever left i16, so saturation went untested");
        assert!(buckets.len() > 3, "captures should visit several material buckets, saw {buckets:?}");
    }

    #[test]
    fn a_threats_pairwise_network_survives_a_write_and_a_load_as_its_own_kind() {
        for hidden_dims in [&[][..], &[6, 4][..]] {
            let path = temp_path("threats-pairwise-round-trip");
            let network = lively_threats_pairwise_network(8, hidden_dims);

            network.write(&path).expect("writing a valid psq_threats_pairwise network should succeed");
            let bytes = fs::read(&path).unwrap();
            assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_THREATS_PAIRWISE);
            let loaded = load_psq(&path);
            assert!(loaded.pairwise && loaded.threats);
            assert_eq!(loaded, network);
            std::fs::remove_file(&path).ok();
        }
    }

    #[test]
    fn a_quantized_threats_pairwise_network_stays_close_to_its_f32_original() {
        // As `a_quantized_pairwise_network_stays_close_to_its_f32_original`, with threats of both
        // sides on the board: White's e5 knight and d5 pawn, Black's b4 and h3 pawns.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
        for hidden_dims in [&[][..], &[16][..]] {
            let network = lively_threats_pairwise_network(64, hidden_dims);
            let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
            assert_eq!(quantized.accumulator_one() >> quantized.accumulator_shift, 255);
            let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
            assert!((exact - approximate).abs() <= 0.02, "hidden {hidden_dims:?}: {approximate} against {exact}");
        }
    }

    #[test]
    fn threats_pairwise_incremental_evaluation_matches_a_from_scratch_one_with_saturated_lanes() {
        // The search's path: piece-square accumulators updated move by move, the threat rows added
        // at evaluation, saturating, then the halves multiplied. Threat rows of 100 at the pairwise
        // scale take lanes past i16; widths 32 and 48 take the kernel with and without a scalar tail.
        use crate::movegen::legal::legal_moves;

        for embedding_dim in [32, 48] {
            let mut heavy = lively_threats_pairwise_network(embedding_dim, &[]);
            for row in psq::ROWS..psq_threats::ROWS {
                let sign = if row % 2 == 0 { 1.0 } else { -1.0 };
                for lane in 0..embedding_dim {
                    heavy.transform[row * embedding_dim + lane] = sign * 100.0;
                }
            }
            let network = QuantizedPsqNetwork::quantize(&heavy).expect("100 at scale 255 fits i16");
            let mut saturated = false;
            let mut seed = 0x9E3779B97F4A7C15u64;
            for _game in 0..6 {
                let (mut board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
                let mut accumulators = network.refresh_by_color(&board);
                let mut next = accumulators.clone();
                for ply in 0..100 {
                    let moves = legal_moves(&board, board.side_to_move());
                    if moves.is_empty() || board.halfmove_clock() >= 100 {
                        break;
                    }
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let mut child = board;
                    child.make_move(moves[(seed >> 32) as usize % moves.len()]);

                    network.update(&accumulators, &mut next, &board, &child);
                    assert_eq!(network.evaluate_by_color_cp(&next, &child), network.evaluate(&child), "width {embedding_dim}, ply {ply}");
                    saturated |= network.refresh(&child, Color::White).iter().any(|lane| lane.abs() > i16::MAX as i32);
                    std::mem::swap(&mut accumulators, &mut next);
                    board = child;
                }
            }
            assert!(saturated, "width {embedding_dim}: no lane ever left i16, so saturation went untested");
        }
    }

    /// A psq_threats_screlu_phase sample network: `sample_screlu_material_network` with its first
    /// `PHASE_BUCKETS` sets only, every one distinct.
    fn sample_screlu_phase_network(embedding_dim: usize) -> PsqNetwork {
        let mut network = sample_screlu_material_network(embedding_dim);
        network.material.as_mut().unwrap().truncate(PHASE_BUCKETS);
        network
    }

    #[test]
    fn a_screlu_phase_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("screlu-phase-round-trip");
        let network = sample_screlu_phase_network(16);
        network.write(&path).expect("writing a valid screlu phase network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_THREATS_SCRELU_PHASE);
        let loaded = load_psq(&path);
        assert!(loaded.screlu && loaded.threats && !loaded.pairwise && loaded.stack.is_none());
        assert_eq!(loaded.material.as_ref().map(|material| material.len()), Some(PHASE_BUCKETS));
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn phase_sets_are_screlu_only() {
        let mut pairwise = sample_material_network(16);
        pairwise.material.as_mut().unwrap().truncate(PHASE_BUCKETS);
        assert!(pairwise.write(&temp_path("pairwise-phase")).unwrap_err().contains("holds 25 weight sets"));
    }

    #[test]
    fn the_screlu_phase_f32_head_squares_into_the_positions_phase_set() {
        // The start (phase 24) and a rook and pawn ending (phase 2, where material would take
        // bucket 5), so a material bucket in place of the phase cannot pass.
        for fen in ["rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", "8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1"] {
            let (board, _) = Board::from_fen(fen).unwrap();
            let network = sample_screlu_phase_network(16);
            let layer = &network.material.as_ref().unwrap()[phase_bucket_of_board(&board)];
            let side_to_move = board.side_to_move();
            let own = network.refresh(&board, side_to_move);
            let opponent = network.refresh(&board, side_to_move.opposite());
            let squares: Vec<f64> = own.iter().chain(&opponent).map(|lane| (lane.clamp(0.0, 1.0) as f64).powi(2)).collect();
            let expected = layer.bias[0] as f64 + layer.weight.iter().zip(&squares).map(|(w, x)| *w as f64 * x).sum::<f64>();
            let fast = network.evaluate_pawns(&board) as f64;
            assert!((fast - expected).abs() <= 1e-5, "{fen}: {fast} against {expected}");
        }
    }

    #[test]
    fn a_quantized_screlu_phase_network_stays_close_to_its_f32_original_and_vectorized_equals_scalar() {
        // As the material one; the quantized head must take the phase's set too, or the two drift.
        for fen in ["r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10", "8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1"] {
            for embedding_dim in [32, 24] {
                let (board, _) = Board::from_fen(fen).unwrap();
                let network = sample_screlu_phase_network(embedding_dim);
                let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
                let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
                assert!((exact - approximate).abs() <= 0.03, "{fen} {embedding_dim}: {approximate} against {exact}");
                let accumulators = quantized.refresh_by_color(&board);
                assert_eq!(quantized.evaluate_by_color_cp(&accumulators, &board), quantized.evaluate(&board), "{fen} {embedding_dim}");
            }
        }
    }

    /// A psq_threats_screlu_count sample network: `sample_screlu_material_network` with its first
    /// `COUNT_BUCKETS` sets only, every one distinct.
    fn sample_screlu_count_network(embedding_dim: usize) -> PsqNetwork {
        let mut network = sample_screlu_material_network(embedding_dim);
        network.material.as_mut().unwrap().truncate(COUNT_BUCKETS);
        network
    }

    #[test]
    fn a_screlu_count_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("screlu-count-round-trip");
        let network = sample_screlu_count_network(16);
        network.write(&path).expect("writing a valid screlu count network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_THREATS_SCRELU_COUNT);
        let loaded = load_psq(&path);
        assert!(loaded.screlu && loaded.threats && !loaded.pairwise && loaded.stack.is_none());
        assert_eq!(loaded.material.as_ref().map(|material| material.len()), Some(COUNT_BUCKETS));
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn count_sets_are_screlu_only() {
        let mut pairwise = sample_material_network(16);
        pairwise.material.as_mut().unwrap().truncate(COUNT_BUCKETS);
        assert!(pairwise.write(&temp_path("pairwise-count")).unwrap_err().contains("holds 8 weight sets"));
    }

    #[test]
    fn the_screlu_count_f32_head_squares_into_the_positions_count_set() {
        // The start (32 pieces, bucket 7) and a rook and pawn ending (4 pieces, bucket 0, where the
        // phase would take bucket 2 and material bucket 5), so another key cannot pass.
        for fen in ["rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", "8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1"] {
            let (board, _) = Board::from_fen(fen).unwrap();
            let network = sample_screlu_count_network(16);
            let layer = &network.material.as_ref().unwrap()[count_bucket_of_board(&board)];
            let side_to_move = board.side_to_move();
            let own = network.refresh(&board, side_to_move);
            let opponent = network.refresh(&board, side_to_move.opposite());
            let squares: Vec<f64> = own.iter().chain(&opponent).map(|lane| (lane.clamp(0.0, 1.0) as f64).powi(2)).collect();
            let expected = layer.bias[0] as f64 + layer.weight.iter().zip(&squares).map(|(w, x)| *w as f64 * x).sum::<f64>();
            let fast = network.evaluate_pawns(&board) as f64;
            assert!((fast - expected).abs() <= 1e-5, "{fen}: {fast} against {expected}");
        }
    }

    #[test]
    fn a_quantized_screlu_count_network_stays_close_to_its_f32_original_and_vectorized_equals_scalar() {
        // As the phase one; the quantized head must take the count's set too, or the two drift.
        for fen in ["r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10", "8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1"] {
            for embedding_dim in [32, 24] {
                let (board, _) = Board::from_fen(fen).unwrap();
                let network = sample_screlu_count_network(embedding_dim);
                let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
                let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
                assert!((exact - approximate).abs() <= 0.03, "{fen} {embedding_dim}: {approximate} against {exact}");
                let accumulators = quantized.refresh_by_color(&board);
                assert_eq!(quantized.evaluate_by_color_cp(&accumulators, &board), quantized.evaluate(&board), "{fen} {embedding_dim}");
            }
        }
    }

    /// A psq_threats_screlu_taper sample network: `sample_screlu_material_network` with its first
    /// `TAPER_SETS` sets only, distinct.
    fn sample_screlu_taper_network(embedding_dim: usize) -> PsqNetwork {
        let mut network = sample_screlu_material_network(embedding_dim);
        network.material.as_mut().unwrap().truncate(TAPER_SETS);
        network
    }

    #[test]
    fn a_screlu_taper_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("screlu-taper-round-trip");
        let network = sample_screlu_taper_network(16);
        network.write(&path).expect("writing a valid screlu taper network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_THREATS_SCRELU_TAPER);
        let loaded = load_psq(&path);
        assert!(loaded.screlu && loaded.threats && !loaded.pairwise && loaded.stack.is_none());
        assert_eq!(loaded.material.as_ref().map(|material| material.len()), Some(TAPER_SETS));
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn taper_sets_are_screlu_only() {
        let mut pairwise = sample_material_network(16);
        pairwise.material.as_mut().unwrap().truncate(TAPER_SETS);
        assert!(pairwise.write(&temp_path("pairwise-taper")).unwrap_err().contains("holds 2 weight sets"));
    }

    #[test]
    fn the_screlu_taper_f32_head_is_the_middlegame_set_at_24_the_endgame_set_at_0_blended_between() {
        // The start (phase 24), kings and pawns (phase 0), a queen each (phase 8).
        let cases = [
            ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 24),
            ("8/pppk4/8/8/8/8/4KPPP/8 w - - 0 1", 0),
            ("3qk3/8/8/8/8/8/8/3QK3 b - - 0 1", 8),
        ];
        for (fen, phase) in cases {
            let (board, _) = Board::from_fen(fen).unwrap();
            assert_eq!(phase_bucket_of_board(&board), phase, "{fen}");
            let network = sample_screlu_taper_network(16);
            let side_to_move = board.side_to_move();
            let own = network.refresh(&board, side_to_move);
            let opponent = network.refresh(&board, side_to_move.opposite());
            let squares: Vec<f64> = own.iter().chain(&opponent).map(|lane| (lane.clamp(0.0, 1.0) as f64).powi(2)).collect();
            let [middlegame, endgame] = [0, 1].map(|set| {
                let layer = &network.material.as_ref().unwrap()[set];
                layer.bias[0] as f64 + layer.weight.iter().zip(&squares).map(|(w, x)| *w as f64 * x).sum::<f64>()
            });
            let expected = (phase as f64 * middlegame + (24 - phase) as f64 * endgame) / 24.0;
            let fast = network.evaluate_pawns(&board) as f64;
            assert!((fast - expected).abs() <= 1e-5, "{fen}: {fast} against {expected}");
        }
    }

    #[test]
    fn a_quantized_screlu_taper_network_stays_close_to_its_f32_original_and_vectorized_equals_scalar() {
        // As the count one; the quantized head must blend both sets too, or the two drift.
        for fen in ["r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10", "8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1"] {
            for embedding_dim in [32, 24] {
                let (board, _) = Board::from_fen(fen).unwrap();
                let network = sample_screlu_taper_network(embedding_dim);
                let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
                let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
                assert!((exact - approximate).abs() <= 0.03, "{fen} {embedding_dim}: {approximate} against {exact}");
                let accumulators = quantized.refresh_by_color(&board);
                assert_eq!(quantized.evaluate_by_color_cp(&accumulators, &board), quantized.evaluate(&board), "{fen} {embedding_dim}");
            }
        }
    }

    /// A psq_threats_screlu_material sample network: `lively_threats_screlu_network`'s transform, its
    /// output replaced by `MATERIAL_BUCKETS` distinct sets over the `2 * embedding_dim` squares. The
    /// weights within 0.05 of zero, so that the quantized test's bound stays tight at 64 squares.
    fn sample_screlu_material_network(embedding_dim: usize) -> PsqNetwork {
        let mut network = lively_threats_screlu_network(embedding_dim);
        network.output = Layer::default();
        network.material = Some(material_sets(2 * embedding_dim, 0.1));
        network
    }

    #[test]
    fn a_screlu_material_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("screlu-material-round-trip");
        let network = sample_screlu_material_network(16);
        network.write(&path).expect("writing a valid screlu material network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_THREATS_SCRELU_MATERIAL);
        let loaded = load_psq(&path);
        assert!(loaded.screlu && loaded.threats && !loaded.pairwise && loaded.material.is_some() && loaded.stack.is_none());
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_screlu_material_network_with_a_wrong_set_count_or_shape_is_refused() {
        let mut short = sample_screlu_material_network(16);
        short.material.as_mut().unwrap().pop();
        assert!(short.write(&temp_path("screlu-material-short")).unwrap_err().contains("holds 77 weight sets"));

        // A set as wide as pairwise's products, half the squares.
        let mut narrow = sample_screlu_material_network(16);
        narrow.material.as_mut().unwrap()[5] = Layer { in_dim: 16, out_dim: 1, weight: vec![0.0; 16], bias: vec![0.0] };
        assert!(narrow.write(&temp_path("screlu-material-narrow")).unwrap_err().contains("bucket 5"));

        let mut hidden = sample_screlu_material_network(16);
        hidden.hidden = vec![Layer { in_dim: 32, out_dim: 4, weight: vec![0.0; 128], bias: vec![0.0; 4] }];
        assert!(hidden.write(&temp_path("screlu-material-hidden")).unwrap_err().contains("a screlu network is psq_threats with no hidden layer"));
    }

    #[test]
    fn the_screlu_material_f32_head_squares_into_the_positions_material_set() {
        // Against a plain f64 computation, clip, square, own first, through the set of the board's
        // material bucket: the start (bucket 77) and a rook and pawn ending (material 6, bucket 5).
        for fen in ["rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", "8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1"] {
            let (board, _) = Board::from_fen(fen).unwrap();
            let network = sample_screlu_material_network(16);
            let layer = &network.material.as_ref().unwrap()[material_bucket_of_board(&board)];
            let side_to_move = board.side_to_move();
            let own = network.refresh(&board, side_to_move);
            let opponent = network.refresh(&board, side_to_move.opposite());
            let squares: Vec<f64> = own.iter().chain(&opponent).map(|lane| (lane.clamp(0.0, 1.0) as f64).powi(2)).collect();
            let expected = layer.bias[0] as f64 + layer.weight.iter().zip(&squares).map(|(w, x)| *w as f64 * x).sum::<f64>();
            let fast = network.evaluate_pawns(&board) as f64;
            assert!((fast - expected).abs() <= 1e-5, "{fen}: {fast} against {expected}");
        }
    }

    #[test]
    fn a_quantized_screlu_material_network_stays_close_to_its_f32_original_and_vectorized_equals_scalar() {
        // Under 0.008 a square (`a_quantized_screlu_network_stays_close_to_its_f32_original`), 64
        // squares under weights of at most 0.05: under 0.026, plus the sets' own rounding. Width 32
        // takes the kernel, 24 the scalar loop.
        for fen in ["r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10", "8/5k2/8/3P4/8/8/2K5/6R1 b - - 0 1"] {
            for embedding_dim in [32, 24] {
                let (board, _) = Board::from_fen(fen).unwrap();
                let network = sample_screlu_material_network(embedding_dim);
                let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
                assert_eq!(quantized.accumulator_one() >> quantized.accumulator_shift, 255);
                let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
                assert!((exact - approximate).abs() <= 0.03, "{fen} {embedding_dim}: {approximate} against {exact}");
                let accumulators = quantized.refresh_by_color(&board);
                assert_eq!(quantized.evaluate_by_color_cp(&accumulators, &board), quantized.evaluate(&board), "{fen} {embedding_dim}");
            }
        }
    }

    #[test]
    fn screlu_material_incremental_evaluation_follows_the_material_with_saturated_lanes() {
        // As the pairwise material one: threat rows of 100 take lanes past i16, captures move the
        // bucket; widths 32 and 48 take the kernel and the scalar loop.
        use crate::movegen::legal::legal_moves;

        for embedding_dim in [32, 48] {
            let mut heavy = sample_screlu_material_network(embedding_dim);
            for row in psq::ROWS..psq_threats::ROWS {
                let sign = if row % 2 == 0 { 1.0 } else { -1.0 };
                for lane in 0..embedding_dim {
                    heavy.transform[row * embedding_dim + lane] = sign * 100.0;
                }
            }
            let network = QuantizedPsqNetwork::quantize(&heavy).expect("100 at scale 255 fits i16");
            let mut saturated = false;
            let mut buckets = std::collections::BTreeSet::new();
            let mut seed = 0xA0761D6478BD642Fu64;
            for _game in 0..6 {
                let (mut board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
                let mut accumulators = network.refresh_by_color(&board);
                let mut next = accumulators.clone();
                for ply in 0..100 {
                    let moves = legal_moves(&board, board.side_to_move());
                    if moves.is_empty() || board.halfmove_clock() >= 100 {
                        break;
                    }
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let mut child = board;
                    child.make_move(moves[(seed >> 32) as usize % moves.len()]);

                    network.update(&accumulators, &mut next, &board, &child);
                    assert_eq!(network.evaluate_by_color_cp(&next, &child), network.evaluate(&child), "width {embedding_dim}, ply {ply}");
                    saturated |= network.refresh(&child, Color::White).iter().any(|lane| lane.abs() > i16::MAX as i32);
                    buckets.insert(material_bucket_of_board(&child));
                    std::mem::swap(&mut accumulators, &mut next);
                    board = child;
                }
            }
            assert!(saturated, "width {embedding_dim}: no lane ever left i16, so saturation went untested");
            assert!(buckets.len() > 3, "width {embedding_dim}: captures should visit several material buckets, saw {buckets:?}");
        }
    }

    /// A psq_threats_screlu sample network, its transform bias raised as `lively_pairwise_network`'s,
    /// so that most lanes sit well inside `(0, 1)` and their squares are not all near zero.
    fn lively_threats_screlu_network(embedding_dim: usize) -> PsqNetwork {
        let mut network = sample_network_with(embedding_dim, &[], true, false);
        network.screlu = true;
        for (lane, bias) in network.transform_bias.iter_mut().enumerate() {
            *bias += 0.3 + 0.4 * (lane % 3) as f32 / 2.0;
        }
        network
    }

    #[test]
    fn a_screlu_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("threats-screlu-round-trip");
        let network = lively_threats_screlu_network(8);
        network.write(&path).expect("writing a valid psq_threats_screlu network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_THREATS_SCRELU);
        let loaded = load_psq(&path);
        assert!(loaded.screlu && loaded.threats && !loaded.pairwise);
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_screlu_network_of_any_other_shape_is_refused() {
        let mut unthreatened = sample_network_with(8, &[], false, false);
        unthreatened.screlu = true;
        let mut paired = sample_network_with(8, &[], true, true);
        paired.screlu = true;
        // One hidden layer beside the output is psq_threats_screlu_hidden's; two are refused.
        let mut hidden = sample_network_with(8, &[4, 4], true, false);
        hidden.screlu = true;
        for (what, network) in [("psq rows", unthreatened), ("pairwise", paired), ("two hidden layers", hidden)] {
            let error = network.write(&temp_path("screlu-refused")).unwrap_err();
            assert!(error.contains("a screlu network is psq_threats with no hidden layer"), "{what}: {error}");
        }
    }

    #[test]
    fn the_screlu_f32_head_squares_each_clipped_lane() {
        // Against a plain f64 computation: clip, square, own first, then the output.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
        let network = lively_threats_screlu_network(16);
        let side_to_move = board.side_to_move();
        let own = network.refresh(&board, side_to_move);
        let opponent = network.refresh(&board, side_to_move.opposite());
        let squares: Vec<f64> = own.iter().chain(&opponent).map(|lane| (lane.clamp(0.0, 1.0) as f64).powi(2)).collect();
        assert!(squares.iter().any(|&square| square > 0.0 && square < 1.0), "some lane inside the clamp, so the square shows");
        let sum: f64 = network.output.weight.iter().zip(&squares).map(|(w, x)| *w as f64 * x).sum();
        let reference = network.output.bias[0] as f64 + sum;
        let fast = network.evaluate_accumulators(&own, &opponent) as f64;
        assert!((fast - reference).abs() <= 1e-5, "{fast} against {reference}");
    }

    #[test]
    fn a_quantized_screlu_network_stays_close_to_its_f32_original() {
        // A lane rounds by at most 0.5 / 255, its square by twice that and the rescale by 0.5 / 127.002:
        // under 0.008 a square, as a pairwise product. 128 squares under weights of at most 0.025
        // move the output by under 0.026 at worst, the errors all of one sign; a wrong scale (127
        // for 255) moves it by far more.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
        let network = lively_threats_screlu_network(64);
        let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
        assert_eq!(quantized.accumulator_one() >> quantized.accumulator_shift, 255);
        let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
        assert!((exact - approximate).abs() <= 0.026, "{approximate} against {exact}");
    }

    #[test]
    fn screlu_evaluation_by_color_equals_the_scalar_from_scratch_one() {
        // 32 and 64 lanes take the vectorized kernel, 24 the scalar loop.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 0 10").unwrap();
        for embedding_dim in [32, 64, 24] {
            let network = QuantizedPsqNetwork::quantize(&lively_threats_screlu_network(embedding_dim)).unwrap();
            let accumulators = network.refresh_by_color(&board);
            assert_eq!(network.evaluate_by_color_cp(&accumulators, &board), network.evaluate(&board), "width {embedding_dim}");
        }
    }

    #[test]
    fn screlu_incremental_evaluation_matches_a_from_scratch_one_with_saturated_lanes() {
        // As the threats pairwise one: threat rows of 100 at the scale 255 take lanes past i16; widths
        // 32 and 48 take the kernel and the scalar loop.
        use crate::movegen::legal::legal_moves;

        for embedding_dim in [32, 48] {
            let mut heavy = lively_threats_screlu_network(embedding_dim);
            for row in psq::ROWS..psq_threats::ROWS {
                let sign = if row % 2 == 0 { 1.0 } else { -1.0 };
                for lane in 0..embedding_dim {
                    heavy.transform[row * embedding_dim + lane] = sign * 100.0;
                }
            }
            let network = QuantizedPsqNetwork::quantize(&heavy).expect("100 at scale 255 fits i16");
            let mut saturated = false;
            let mut seed = 0x9E3779B97F4A7C15u64;
            for _game in 0..6 {
                let (mut board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
                let mut accumulators = network.refresh_by_color(&board);
                let mut next = accumulators.clone();
                for ply in 0..100 {
                    let moves = legal_moves(&board, board.side_to_move());
                    if moves.is_empty() || board.halfmove_clock() >= 100 {
                        break;
                    }
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let mut child = board;
                    child.make_move(moves[(seed >> 32) as usize % moves.len()]);

                    network.update(&accumulators, &mut next, &board, &child);
                    assert_eq!(network.evaluate_by_color_cp(&next, &child), network.evaluate(&child), "width {embedding_dim}, ply {ply}");
                    saturated |= network.refresh(&child, Color::White).iter().any(|lane| lane.abs() > i16::MAX as i32);
                    std::mem::swap(&mut accumulators, &mut next);
                    board = child;
                }
            }
            assert!(saturated, "width {embedding_dim}: no lane ever left i16, so saturation went untested");
        }
    }

    /// A halfk_threats_screlu sample: every row distinct, so a row of the wrong bucket or mirror
    /// cannot pass by coincidence, and its bias raised as `lively_threats_screlu_network`'s.
    fn lively_halfk_network(embedding_dim: usize) -> PsqNetwork {
        let mut network = sample_network_with(embedding_dim, &[], true, false);
        let mut next = 0.0f32;
        network.transform = (0..halfk::ROWS * embedding_dim)
            .map(|_| {
                next += 1.0;
                ((next * 0.618).fract() - 0.5) * 0.05
            })
            .collect();
        network.screlu = true;
        network.halfk = true;
        for (lane, bias) in network.transform_bias.iter_mut().enumerate() {
            *bias += 0.3 + 0.4 * (lane % 3) as f32 / 2.0;
        }
        network
    }

    /// A halfk_threats_screlu_hidden sample: `lively_halfk_network` with two hidden layers of
    /// `widths`, every weight distinct and the biases raised, so that most hidden values lie inside
    /// the clip and a wrong row or a swapped layer moves the value.
    fn lively_halfk_hidden_network(embedding_dim: usize, widths: [usize; 2]) -> PsqNetwork {
        lively_halfk_two_hidden_network(embedding_dim, widths, false)
    }

    /// `lively_halfk_hidden_network`, or with `pairwise` a halfk_threats_pairwise_hidden sample: each
    /// perspective's halves multiplied, the first layer reading `embedding_dim` products.
    fn lively_halfk_two_hidden_network(embedding_dim: usize, widths: [usize; 2], pairwise: bool) -> PsqNetwork {
        let mut network = lively_halfk_network(embedding_dim);
        network.screlu = !pairwise;
        network.pairwise = pairwise;
        let mut next = 0.0f32;
        let mut weights = |count: usize, size: f32| -> Vec<f32> {
            (0..count)
                .map(|_| {
                    next += 1.0;
                    ((next * 0.618).fract() - 0.5) * size
                })
                .collect()
        };
        let input = if pairwise { embedding_dim } else { 2 * embedding_dim };
        let first = Layer { in_dim: input, out_dim: widths[0], weight: weights(input * widths[0], 0.2), bias: vec![0.4; widths[0]] };
        let second = Layer { in_dim: widths[0], out_dim: widths[1], weight: weights(widths[0] * widths[1], 0.3), bias: vec![0.4; widths[1]] };
        network.output = Layer { in_dim: widths[1], out_dim: 1, weight: weights(widths[1], 0.5), bias: vec![0.1] };
        network.hidden = vec![first, second];
        network
    }

    #[test]
    fn a_halfk_hidden_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("halfk-hidden-round-trip");
        let network = lively_halfk_hidden_network(8, [8, 4]);
        network.write(&path).expect("writing a valid halfk_threats_screlu_hidden network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_HALFK_THREATS_SCRELU_HIDDEN);
        let loaded = load_psq(&path);
        assert!(loaded.halfk && loaded.screlu && loaded.threats && !loaded.pairwise && loaded.hidden.len() == 2);
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_halfk_hidden_f32_head_reads_the_squares_through_both_clipped_layers() {
        // Against a plain f64 computation: clip and square every lane, own first, then each layer
        // clipped, then the output over the second layer alone.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
        let network = lively_halfk_hidden_network(16, [8, 8]);
        let side_to_move = board.side_to_move();
        let own = network.refresh(&board, side_to_move);
        let opponent = network.refresh(&board, side_to_move.opposite());
        let mut current: Vec<f64> = own.iter().chain(&opponent).map(|lane| (lane.clamp(0.0, 1.0) as f64).powi(2)).collect();
        let mut inside = 0;
        for layer in &network.hidden {
            current = (0..layer.out_dim)
                .map(|out| {
                    let row = &layer.weight[out * layer.in_dim..(out + 1) * layer.in_dim];
                    let sum = layer.bias[out] as f64 + row.iter().zip(&current).map(|(w, x)| *w as f64 * x).sum::<f64>();
                    inside += (sum > 0.0 && sum < 1.0) as usize;
                    sum.clamp(0.0, 1.0)
                })
                .collect();
        }
        assert!(inside >= 8, "only {inside} of 16 hidden values inside the clip: the sample tests too little");
        let reference = network.output.bias[0] as f64 + network.output.weight.iter().zip(&current).map(|(w, x)| *w as f64 * x).sum::<f64>();
        let value = network.evaluate_accumulators(&own, &opponent) as f64;
        assert!((value - reference).abs() <= 1e-5, "{value} against {reference}");
        assert_eq!(network.evaluate_pawns(&board), network.evaluate_accumulators(&own, &opponent));
    }

    #[test]
    fn a_quantized_halfk_hidden_network_stays_close_to_its_f32_original_and_its_paths_agree() {
        // The first layer in int16 per row, the rest in f32: within 0.02 pawns of f32 (a wrong scale
        // or row moves it by far more), and the vectorized evaluation equal to the scalar one exactly,
        // with 32 lanes taking the kernels and 24 the scalar loops.
        for (embedding_dim, widths) in [(32, [16, 32]), (24, [8, 8])] {
            let network = lively_halfk_hidden_network(embedding_dim, widths);
            let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
            for fen in [
                "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
                "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 11",
            ] {
                let (board, _) = Board::from_fen(fen).unwrap();
                let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
                assert!((exact - approximate).abs() <= 0.02, "width {embedding_dim}, {fen}: {approximate} against {exact}");
                assert_eq!(quantized.evaluate(&board), centipawns(approximate), "width {embedding_dim}, {fen}");
            }
        }
    }

    #[test]
    fn an_int8_halfk_hidden_head_stays_close_to_f32_and_its_paths_agree() {
        // The first layer in 8 bits, Reckless's: built only on request, the rest as int16's. Looser
        // than int16's bound, each weight rounding to 1/254 of its row's largest instead of
        // 1/65534; the vectorized evaluation (32 lanes, 64 squares, the kernel) equal to the scalar
        // one exactly, and 24 lanes (48 squares, not a multiple of 32) the scalar loop both ways.
        for (embedding_dim, widths) in [(32, [16, 32]), (24, [8, 8])] {
            let network = lively_halfk_hidden_network(embedding_dim, widths);
            let int8 = QuantizedPsqNetwork::quantize_with(&network, HiddenArithmetic::Int8).expect("a small sample network quantizes");
            assert!(matches!(int8.head, QuantizedHead::TwoHiddenI8 { .. }));
            assert!(matches!(QuantizedPsqNetwork::quantize(&network).unwrap().head, QuantizedHead::TwoHidden { .. }), "int16 the default");
            for fen in [
                "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
                "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
                "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 11",
            ] {
                let (board, _) = Board::from_fen(fen).unwrap();
                let (exact, approximate) = (network.evaluate_pawns(&board), int8.evaluate_pawns(&board));
                assert!((exact - approximate).abs() <= 0.03, "width {embedding_dim}, {fen}: {approximate} against {exact}");
                assert_eq!(int8.evaluate(&board), centipawns(approximate), "width {embedding_dim}, {fen}");
            }
        }
    }

    #[test]
    fn a_halfk_pairwise_hidden_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("halfk-pairwise-round-trip");
        let network = lively_halfk_two_hidden_network(16, [8, 4], true);
        network.write(&path).expect("writing a valid halfk_threats_pairwise_hidden network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_HALFK_THREATS_PAIRWISE_HIDDEN);
        let loaded = load_psq(&path);
        assert!(loaded.halfk && loaded.pairwise && !loaded.screlu && loaded.threats && loaded.hidden.len() == 2);
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_halfk_pairwise_f32_head_reads_the_products_through_both_clipped_layers() {
        // Against a plain f64 computation: each perspective's halves clipped and multiplied, own
        // first, then each layer clipped, then the output over the second layer alone.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
        let network = lively_halfk_two_hidden_network(16, [8, 8], true);
        let side_to_move = board.side_to_move();
        let own = network.refresh(&board, side_to_move);
        let opponent = network.refresh(&board, side_to_move.opposite());
        let mut current: Vec<f64> = [&own, &opponent]
            .into_iter()
            .flat_map(|lanes| {
                let (a, b) = lanes.split_at(lanes.len() / 2);
                a.iter().zip(b).map(|(a, b)| a.clamp(0.0, 1.0) as f64 * b.clamp(0.0, 1.0) as f64).collect::<Vec<f64>>()
            })
            .collect();
        assert_eq!(current.len(), 16);
        let mut inside = 0;
        for layer in &network.hidden {
            current = (0..layer.out_dim)
                .map(|out| {
                    let row = &layer.weight[out * layer.in_dim..(out + 1) * layer.in_dim];
                    let sum = layer.bias[out] as f64 + row.iter().zip(&current).map(|(w, x)| *w as f64 * x).sum::<f64>();
                    inside += (sum > 0.0 && sum < 1.0) as usize;
                    sum.clamp(0.0, 1.0)
                })
                .collect();
        }
        assert!(inside >= 8, "only {inside} of 16 hidden values inside the clip: the sample tests too little");
        let reference = network.output.bias[0] as f64 + network.output.weight.iter().zip(&current).map(|(w, x)| *w as f64 * x).sum::<f64>();
        let value = network.evaluate_accumulators(&own, &opponent) as f64;
        assert!((value - reference).abs() <= 1e-5, "{value} against {reference}");
    }

    #[test]
    fn a_quantized_halfk_pairwise_network_stays_close_to_f32_in_both_arithmetics() {
        // Step 2's bounds, int16 then int8, over the products: 32 lanes (16 products a perspective)
        // take the kernels, 24 the scalar loops, and each `evaluate` agrees with its own pawns.
        for (embedding_dim, widths) in [(32, [16, 32]), (24, [8, 8])] {
            let network = lively_halfk_two_hidden_network(embedding_dim, widths, true);
            for (arithmetic, bound) in [(HiddenArithmetic::Int16, 0.02), (HiddenArithmetic::Int8, 0.03)] {
                let quantized = QuantizedPsqNetwork::quantize_with(&network, arithmetic).expect("a small sample network quantizes");
                for fen in [
                    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
                    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
                    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 11",
                ] {
                    let (board, _) = Board::from_fen(fen).unwrap();
                    let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
                    let what = format!("{arithmetic:?}, width {embedding_dim}, {fen}");
                    assert!((exact - approximate).abs() <= bound, "{what}: {approximate} against {exact}");
                    assert_eq!(quantized.evaluate(&board), centipawns(approximate), "{what}");
                }
            }
        }
    }

    #[test]
    fn a_halfk_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("halfk-round-trip");
        let network = lively_halfk_network(8);
        network.write(&path).expect("writing a valid halfk_threats_screlu network should succeed");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_HALFK_THREATS_SCRELU);
        let loaded = load_psq(&path);
        assert!(loaded.halfk && loaded.screlu && loaded.threats && !loaded.pairwise);
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_halfk_network_of_any_other_shape_is_refused() {
        let mut plain = lively_halfk_network(8);
        plain.screlu = false;
        let mut hidden = lively_halfk_network(8);
        hidden.hidden = vec![Layer { in_dim: 16, out_dim: 2, weight: vec![0.01; 32], bias: vec![0.0; 2] }];
        hidden.output = Layer { in_dim: 2 * 2 + 16, out_dim: 1, weight: vec![0.01; 20], bias: vec![0.0] };
        for (what, network) in [("not screlu", plain), ("one hidden layer", hidden)] {
            let error = network.write(&temp_path("halfk-refused")).unwrap_err();
            assert!(error.contains("a halfk network has threats and a plain head"), "{what}: {error}");
        }
        let narrow = lively_halfk_hidden_network(8, [4, 8]);
        assert!(narrow.write(&temp_path("halfk-narrow")).unwrap_err().contains("a multiple of 8"));
        let mut short = lively_halfk_network(8);
        short.transform.truncate(psq_threats::ROWS * 8);
        assert!(short.write(&temp_path("halfk-short")).unwrap_err().contains("transform holds"));
    }

    #[test]
    fn the_halfk_f32_accumulator_sums_its_kings_bucket_and_mirrored_threats() {
        // White's king on h1 mirrors White's view: its rows are halfk's, threats included.
        let (board, _) = Board::from_fen("4k3/8/8/8/8/8/2p5/N6K w - - 0 1").unwrap();
        let network = lively_halfk_network(8);
        let mut expected = network.transform_bias.clone();
        for row in halfk::active_rows(&extract_canonical(&board), &board, Color::White) {
            for (lane, value) in expected.iter_mut().zip(&network.transform[row as usize * 8..row as usize * 8 + 8]) {
                *lane += value;
            }
        }
        let accumulator = network.refresh(&board, Color::White);
        for (got, expected) in accumulator.iter().zip(&expected) {
            assert!((got - expected).abs() < 1e-5, "{got} against {expected}");
        }
    }

    #[test]
    fn a_quantized_halfk_network_stays_close_to_its_f32_original() {
        // The bound of `a_quantized_screlu_network_stays_close_to_its_f32_original`, at the same width.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
        let network = lively_halfk_network(64);
        let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
        let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
        assert!((exact - approximate).abs() <= 0.026, "{approximate} against {exact}");
    }

    #[test]
    fn a_halfk_lane_is_bounded_bucket_by_bucket() {
        // 20 rows of 4 in every bucket: 80 a bucket fits i16 at the scale 255 (20,400 and the bias),
        // while 37 of them, as a bound over all rows would count, would not (37,740).
        let mut network = lively_halfk_network(8);
        for bucket in 0..halfk::BUCKETS {
            for row in bucket * halfk::FACTOR_ROWS..bucket * halfk::FACTOR_ROWS + 20 {
                network.transform[row * 8..row * 8 + 8].fill(4.0);
            }
        }
        QuantizedPsqNetwork::quantize(&network).expect("each bucket alone fits");
        // 40 of them in one bucket do not.
        for row in 5 * halfk::FACTOR_ROWS..5 * halfk::FACTOR_ROWS + 40 {
            network.transform[row * 8..row * 8 + 8].fill(4.0);
        }
        assert!(QuantizedPsqNetwork::quantize(&network).unwrap_err().contains("an accumulator lane can reach"));
    }

    #[test]
    fn halfk_incremental_evaluation_matches_a_from_scratch_one_as_kings_cross_buckets() {
        // Random games from a middlegame and an endgame whose kings walk, with castling, so that
        // kings change bucket and cross the d/e line: each such move refreshes its perspective
        // through the cache, every other updates. 32 lanes take the kernel, 24 the scalar loop; each
        // width with no hidden layer and with two.
        use crate::movegen::legal::legal_moves;

        for (embedding_dim, head) in
            [(32, "output"), (24, "output"), (32, "screlu"), (24, "screlu"), (32, "pairwise"), (24, "pairwise")]
        {
            let sample = match head {
                "output" => lively_halfk_network(embedding_dim),
                _ => lively_halfk_two_hidden_network(embedding_dim, [8, 8], head == "pairwise"),
            };
            let network = QuantizedPsqNetwork::quantize(&sample).unwrap();
            let (mut refreshes, mut mirror_flips) = (0, 0);
            let mut seed = 0x9E3779B97F4A7C15u64;
            for start in [
                "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
                "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 11",
            ] {
                for _game in 0..4 {
                    let (mut board, _) = Board::from_fen(start).unwrap();
                    let mut accumulators = network.refresh_by_color(&board);
                    let mut next = accumulators.clone();
                    for ply in 0..120 {
                        let moves = legal_moves(&board, board.side_to_move());
                        if moves.is_empty() || board.halfmove_clock() >= 100 {
                            break;
                        }
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        let mut child = board;
                        child.make_move(moves[(seed >> 32) as usize % moves.len()]);

                        for perspective in [Color::White, Color::Black] {
                            let (before, after) = (halfk_king(&board, perspective), halfk_king(&child, perspective));
                            refreshes += (before != after) as usize;
                            mirror_flips += (before.1 != after.1) as usize;
                        }
                        network.update(&accumulators, &mut next, &board, &child);
                        assert_eq!(next, network.refresh_by_color(&child), "width {embedding_dim}, ply {ply}: {}", child.to_fen(1));
                        assert_eq!(network.evaluate_by_color_cp(&next, &child), network.evaluate(&child), "width {embedding_dim}, ply {ply}");
                        std::mem::swap(&mut accumulators, &mut next);
                        board = child;
                    }
                }
            }
            assert!(refreshes > 20 && mirror_flips > 0, "width {embedding_dim}: {refreshes} refreshes, {mirror_flips} mirror flips");
        }
    }

    #[test]
    fn the_refresh_cache_equals_a_full_rebuild_whatever_it_last_saw() {
        // One thread's cache across many unrelated positions, then back to the first: every entry
        // reused from a different position must still give the from-scratch sums. A second
        // network of the same shape must not read the first one's entries.
        let boards: Vec<Board> = [
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 11",
            "4k3/8/8/8/8/8/2p5/N6K w - - 0 1",
            "r1bq1rk1/pp2bppp/2n1pn2/3p4/2PP4/2N1PN2/PP2BPPP/R2QKB1R w KQ - 0 8",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        ]
        .iter()
        .map(|fen| Board::from_fen(fen).unwrap().0)
        .collect();
        let first = QuantizedPsqNetwork::quantize(&lively_halfk_network(32)).unwrap();
        let mut other = lively_halfk_network(32);
        other.transform_bias.iter_mut().for_each(|bias| *bias -= 0.2);
        let second = QuantizedPsqNetwork::quantize(&other).unwrap();
        for network in [&first, &second, &first] {
            for board in &boards {
                let expected = network.refresh_by_color(board);
                for perspective in [Color::White, Color::Black] {
                    let mut accumulator = vec![0i16; 32];
                    network.refresh_through_cache(&mut accumulator, board, perspective);
                    assert_eq!(accumulator, expected[perspective as usize], "{}", board.to_fen(1));
                }
            }
        }
    }

    #[test]
    fn a_pairwise_network_survives_a_write_and_a_load_as_its_own_kind() {
        for hidden_dims in [&[][..], &[6, 4][..]] {
            let path = temp_path("pairwise-round-trip");
            let network = sample_pairwise_network(8, hidden_dims);

            network.write(&path).expect("writing a valid pairwise network should succeed");
            let bytes = fs::read(&path).unwrap();
            assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), FEATURE_SET_PSQ_PAIRWISE);
            let loaded = load_psq(&path);
            assert!(loaded.pairwise && !loaded.threats);
            assert_eq!(loaded, network);
            std::fs::remove_file(&path).ok();
        }
    }

    #[test]
    fn a_pairwise_network_with_an_odd_width_is_refused() {
        let mut odd = sample_pairwise_network(8, &[]);
        odd.embedding_dim = 7;
        odd.transform.truncate(psq::ROWS * 7);
        odd.transform_bias.truncate(7);
        odd.output = Layer { in_dim: 7, out_dim: 1, weight: vec![0.01; 7], bias: vec![0.0] };
        assert!(odd.write(&temp_path("pairwise-odd")).unwrap_err().contains("is odd"));
    }

    #[test]
    fn the_pairwise_f32_head_multiplies_each_perspectives_clipped_halves() {
        // Against a plain f64 computation: clip, multiply the halves, own first, then the head.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
        for hidden_dims in [&[][..], &[8][..]] {
            let network = lively_pairwise_network(16, hidden_dims);
            let side_to_move = board.side_to_move();
            let own = network.refresh(&board, side_to_move);
            let opponent = network.refresh(&board, side_to_move.opposite());
            let mut current: Vec<f64> = Vec::new();
            for lanes in [&own, &opponent] {
                for i in 0..8 {
                    current.push(lanes[i].clamp(0.0, 1.0) as f64 * lanes[i + 8].clamp(0.0, 1.0) as f64);
                }
            }
            for layer in &network.hidden {
                current = (0..layer.out_dim)
                    .map(|out| {
                        let row = &layer.weight[out * layer.in_dim..(out + 1) * layer.in_dim];
                        let sum: f64 = row.iter().zip(&current).map(|(w, x)| *w as f64 * x).sum();
                        (layer.bias[out] as f64 + sum).clamp(0.0, 1.0)
                    })
                    .collect();
            }
            let sum: f64 = network.output.weight.iter().zip(&current).map(|(w, x)| *w as f64 * x).sum();
            let reference = network.output.bias[0] as f64 + sum;
            let fast = network.evaluate_accumulators(&own, &opponent) as f64;
            assert!((fast - reference).abs() <= 1e-5, "hidden {hidden_dims:?}: {fast} against {reference}");
        }
    }

    #[test]
    fn a_quantized_pairwise_network_stays_close_to_its_f32_original() {
        // Each half rounds by at most 0.5 / 255 and the product by 0.5 / 127.002, so a product is off
        // by under 0.008; 64 products under weights of at most 0.025 move a head with no hidden layer
        // by under 0.013 pawns at worst, the errors all of one sign. 0.02 covers the hidden layer
        // too, and a wrong scale (127 for 255) or a swapped half moves the value by far more.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
        for hidden_dims in [&[][..], &[16][..], &[8, 4][..]] {
            let network = lively_pairwise_network(64, hidden_dims);
            let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
            assert!(quantized.accumulator_one() <= i16::MAX as i32);
            assert_eq!(quantized.accumulator_one() >> quantized.accumulator_shift, 255);
            let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
            assert!((exact - approximate).abs() <= 0.02, "hidden {hidden_dims:?}: {approximate} against {exact}");
        }
    }

    #[test]
    fn pairwise_evaluation_by_color_equals_the_scalar_from_scratch_one_for_every_head_shape() {
        // Halves of 16 or 32 lanes take the vectorized kernels, 8 and 12 the scalar loops; a hidden
        // layer of 16 goes eight rows at a time, 6 does not.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 0 10").unwrap();
        for (embedding_dim, hidden_dims) in
            [(32, &[][..]), (64, &[][..]), (64, &[16][..]), (32, &[8, 4][..]), (64, &[6][..]), (16, &[][..]), (24, &[8][..])]
        {
            let network = QuantizedPsqNetwork::quantize(&lively_pairwise_network(embedding_dim, hidden_dims)).unwrap();
            let accumulators = network.refresh_by_color(&board);
            assert_eq!(
                network.evaluate_by_color_cp(&accumulators, &board),
                network.evaluate(&board),
                "width {embedding_dim}, hidden {hidden_dims:?}"
            );
            let side_to_move = board.side_to_move();
            let (own, opponent) = (&accumulators[side_to_move as usize], &accumulators[side_to_move.opposite() as usize]);
            assert_eq!(
                network.evaluate_lanes(own, opponent, true, [0; 3]),
                network.evaluate_pawns(&board),
                "width {embedding_dim}, hidden {hidden_dims:?}"
            );
        }
    }

    #[test]
    fn pairwise_incremental_evaluation_matches_a_from_scratch_one_along_random_games() {
        use crate::movegen::legal::legal_moves;

        let network = QuantizedPsqNetwork::quantize(&lively_pairwise_network(64, &[])).unwrap();
        let mut seed = 0xD1B54A32D192ED03u64;
        for fen in [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
        ] {
            for _game in 0..6 {
                let (mut board, _) = Board::from_fen(fen).unwrap();
                let mut accumulators = network.refresh_by_color(&board);
                let mut next = accumulators.clone();
                for ply in 0..150 {
                    if board.halfmove_clock() >= 100 {
                        break;
                    }
                    let moves = legal_moves(&board, board.side_to_move());
                    if moves.is_empty() {
                        break;
                    }
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let mut child = board;
                    child.make_move(moves[(seed >> 32) as usize % moves.len()]);

                    network.update(&accumulators, &mut next, &board, &child);
                    assert_eq!(next, network.refresh_by_color(&child), "{fen}, ply {ply}");
                    assert_eq!(network.evaluate_by_color_cp(&next, &child), network.evaluate(&child), "{fen}, ply {ply}");
                    std::mem::swap(&mut accumulators, &mut next);
                    board = child;
                }
            }
        }
    }

    #[test]
    fn a_plain_psq_network_keeps_its_quantization() {
        // The pairwise scale must not leak into psq: its activations stay at 127.
        let quantized = QuantizedPsqNetwork::quantize(&sample_network(32, &[])).unwrap();
        assert_eq!(quantized.accumulator_one() >> quantized.accumulator_shift, 127);
        assert_eq!(quantized.input_one(), 127.0);
    }

    #[test]
    fn the_product_of_two_full_halves_is_the_heads_full_scale() {
        assert_eq!(product_of(255, 255), 127);
        assert_eq!(product_of(0, 255), 0);
        assert_eq!(product_of(1, 1), 0);
        assert_eq!(product_of(16, 16), 1, "256 + 256 = 512, exactly one step after rounding");
        assert!((PRODUCT_ONE - 127.002).abs() < 1e-3);
    }

    #[test]
    fn a_psq_network_survives_a_write_and_a_load_as_its_own_kind() {
        for hidden_dims in [&[][..], &[6, 4][..]] {
            let path = temp_path("psq-round-trip");
            let network = sample_network(8, hidden_dims);

            network.write(&path).expect("writing a valid psq network should succeed");
            assert_eq!(Network::load(&path).expect("loading should succeed"), Network::from_psq(network).unwrap());
            std::fs::remove_file(&path).ok();
        }
    }

    #[test]
    fn a_psq_position_and_its_color_swapped_mirror_evaluate_identically() {
        // Both perspectives are oriented for their own color and the side to move's goes first,
        // so mirroring the board and swapping colors and the side to move leaves both
        // accumulators, and so the value, unchanged. Exact: the same rows are summed in the same
        // order.
        let (white_to_move, _) = Board::from_fen("4k3/8/8/8/8/2N5/4P3/R3K3 w Q - 0 1").unwrap();
        let (black_to_move, _) = Board::from_fen("r3k3/4p3/2n5/8/8/8/8/4K3 b q - 0 1").unwrap();
        let network = sample_network(8, &[4]);

        assert_eq!(network.evaluate_pawns(&white_to_move), network.evaluate_pawns(&black_to_move));
    }

    #[test]
    fn the_side_to_move_changes_a_psq_evaluation() {
        // The same pieces with the other side to move swap which accumulator goes first. If the
        // value did not change, the head would be blind to whose move it is.
        let (white_to_move, _) = Board::from_fen("4k3/8/8/8/8/2N5/4P3/R3K3 w Q - 0 1").unwrap();
        let (black_to_move, _) = Board::from_fen("4k3/8/8/8/8/2N5/4P3/R3K3 b Q - 0 1").unwrap();
        let network = sample_network(8, &[]);

        assert_ne!(network.evaluate_pawns(&white_to_move), network.evaluate_pawns(&black_to_move));
    }

    #[test]
    fn incremental_updates_track_a_from_scratch_refresh_along_random_games() {
        // The test the incremental path rests on. Random legal games from positions chosen so that
        // castling (both sides), losing castling rights to a rook capture, en passant and
        // promotion all come up, with an occasional null move. At every ply the updated
        // accumulators must equal a from-scratch refresh of the same board exactly: the quantized
        // accumulator is integer, so no drift is allowed at all. And the evaluation from them,
        // vectorized, must equal the scalar from-scratch one exactly.
        use crate::movegen::attacks::is_in_check;
        use crate::movegen::legal::legal_moves;

        let starts = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
            "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
            "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
            "8/1P4k1/8/8/8/8/1p4K1/8 w - - 0 1",
        ];
        let networks = [sample_network(16, &[])];
        let mut seed = 0x2545F4914F6CDD1Du64;

        for network in networks {
            let kind = "psq";
            let network = QuantizedPsqNetwork::quantize(&network).unwrap();
            for fen in starts {
                for _game in 0..8 {
                    let (mut board, _) = Board::from_fen(fen).unwrap();
                    let mut accumulators = network.refresh_by_color(&board);
                    let mut next = accumulators.clone();

                    for ply in 0..300 {
                        // A real game is drawn here, and `halfmove_clock` (a `u8`) is not meant to
                        // count past it: 255 quiet random plies would overflow it.
                        if board.halfmove_clock() >= 100 {
                            break;
                        }
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;

                        let mut child = board;
                        if seed.is_multiple_of(29) && !is_in_check(&board, board.side_to_move()) {
                            child.make_null_move();
                        } else {
                            let moves = legal_moves(&board, board.side_to_move());
                            if moves.is_empty() {
                                break;
                            }
                            child.make_move(moves[(seed >> 32) as usize % moves.len()]);
                        }

                        network.update(&accumulators, &mut next, &board, &child);
                        assert_eq!(next, network.refresh_by_color(&child), "{kind}: {fen}, ply {ply}");
                        assert_eq!(
                            network.evaluate_by_color_cp(&next, &child),
                            network.evaluate(&child),
                            "{kind}: {fen}, ply {ply}"
                        );
                        std::mem::swap(&mut accumulators, &mut next);
                        board = child;
                    }
                }
            }
        }
    }

    #[test]
    fn evaluating_by_color_equals_the_scalar_from_scratch_evaluation_for_every_head_shape() {
        // Widths of 16 or 32 take the vectorized kernels, 8 and 12 the scalar loops; a first
        // hidden layer of 16 or 8 goes eight rows at a time, 4 and 6 do not; and a second hidden
        // layer runs in f32 after the quantized first.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 0 10").unwrap();
        for (embedding_dim, hidden_dims) in
            [(16, &[][..]), (32, &[16][..]), (32, &[8, 4][..]), (16, &[6][..]), (8, &[4][..]), (12, &[8][..])]
        {
            let network = QuantizedPsqNetwork::quantize(&sample_network(embedding_dim, hidden_dims)).unwrap();
            let accumulators = network.refresh_by_color(&board);
            assert_eq!(
                network.evaluate_by_color_cp(&accumulators, &board),
                network.evaluate(&board),
                "width {embedding_dim}, hidden {hidden_dims:?}"
            );
            let side_to_move = board.side_to_move();
            let (own, opponent) = (&accumulators[side_to_move as usize], &accumulators[side_to_move.opposite() as usize]);
            assert_eq!(network.evaluate_lanes(own, opponent, true, [0; 3]), network.evaluate_pawns(&board), "width {embedding_dim}");
        }
    }

    /// The head computed the plainest way, in `f64`: clamp the concatenated accumulators, then one
    /// scalar dot product per output, layer by layer. What every vectorized path must agree with.
    fn scalar_head(network: &PsqNetwork, own: &[f32], opponent: &[f32]) -> f64 {
        let mut current: Vec<f64> = own.iter().chain(opponent).map(|x| x.clamp(0.0, 1.0) as f64).collect();
        for layer in &network.hidden {
            current = (0..layer.out_dim)
                .map(|out| {
                    let row = &layer.weight[out * layer.in_dim..(out + 1) * layer.in_dim];
                    let sum: f64 = row.iter().zip(&current).map(|(w, x)| *w as f64 * x).sum();
                    (layer.bias[out] as f64 + sum).clamp(0.0, 1.0)
                })
                .collect();
        }
        let sum: f64 = network.output.weight.iter().zip(&current).map(|(w, x)| *w as f64 * x).sum();
        network.output.bias[0] as f64 + sum
    }

    #[test]
    fn every_head_path_agrees_with_a_scalar_reference() {
        // Shapes chosen to take each path: no hidden layer at a width the fused pair kernel covers
        // (32) and one it does not (8); a first hidden layer of 16 or 8, eight outputs at a time,
        // with and without a second layer behind it; and widths that fall back to the copy (4, 6).
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
        for (embedding_dim, hidden_dims) in
            [(32, &[][..]), (8, &[][..]), (16, &[16][..]), (32, &[8, 4][..]), (8, &[4][..]), (8, &[6, 4][..]), (12, &[8][..])]
        {
            let network = sample_network(embedding_dim, hidden_dims);
            let side_to_move = board.side_to_move();
            let own = network.refresh(&board, side_to_move);
            let opponent = network.refresh(&board, side_to_move.opposite());
            let fast = network.evaluate_accumulators(&own, &opponent) as f64;
            let reference = scalar_head(&network, &own, &opponent);
            assert!(
                (fast - reference).abs() <= 1e-4,
                "width {embedding_dim}, hidden {hidden_dims:?}: {fast} against {reference}"
            );
        }
    }

    #[test]
    fn a_quantized_network_stays_close_to_its_f32_original() {
        // Truncating an activation to 0..=127 moves it by less than 1/127, and the sample weights
        // are at most 0.025 in magnitude, so 64 lanes move a head with no hidden layer by less than
        // 64 * 0.025 / 127, about 0.013 pawns; the hidden layer's rows add as much again at most,
        // shrunk by the output weights. 0.03 pawns covers both with room, and a misplaced row or a
        // wrong scale moves the value by far more.
        let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
        for hidden_dims in [&[][..], &[16][..], &[8, 4][..]] {
            let network = sample_network(32, hidden_dims);
            let quantized = QuantizedPsqNetwork::quantize(&network).expect("a small sample network quantizes");
            let (exact, approximate) = (network.evaluate_pawns(&board), quantized.evaluate_pawns(&board));
            assert!((exact - approximate).abs() <= 0.03, "hidden {hidden_dims:?}: {approximate} against {exact}");
            assert!(quantized.refresh(&board, Color::White).iter().all(|lane| lane.abs() <= i16::MAX as i32));
        }
    }

    #[test]
    fn a_psq_threats_network_survives_a_write_and_a_load_as_its_own_kind() {
        let path = temp_path("psq-threats-round-trip");
        let network = sample_network_of(8, &[4], true);

        network.write(&path).expect("writing a valid psq_threats network should succeed");
        let loaded = load_psq(&path);
        assert!(loaded.threats, "a psq_threats file must load as psq_threats");
        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn psq_threats_incremental_evaluation_matches_a_from_scratch_one_along_random_games() {
        // The search's path for psq_threats: piece-square accumulators updated move by move, the
        // threats recomputed at evaluation. At every ply it must give exactly `evaluate_pawns`'s
        // value, which refreshes everything, threats included, from scratch.
        use crate::movegen::legal::legal_moves;

        let network = QuantizedPsqNetwork::quantize(&sample_network_of(16, &[8], true)).unwrap();
        let mut seed = 0x9E3779B97F4A7C15u64;
        for fen in [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
        ] {
            for _game in 0..6 {
                let (mut board, _) = Board::from_fen(fen).unwrap();
                let mut accumulators = network.refresh_by_color(&board);
                let mut next = accumulators.clone();
                for ply in 0..150 {
                    if board.halfmove_clock() >= 100 {
                        break;
                    }
                    let moves = legal_moves(&board, board.side_to_move());
                    if moves.is_empty() {
                        break;
                    }
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let mut child = board;
                    child.make_move(moves[(seed >> 32) as usize % moves.len()]);

                    network.update(&accumulators, &mut next, &board, &child);
                    assert_eq!(network.evaluate_by_color_cp(&next, &child), network.evaluate(&child), "{fen}, ply {ply}");
                    std::mem::swap(&mut accumulators, &mut next);
                    board = child;
                }
            }
        }
    }

    /// A psq_threats sample network whose threat rows all weigh `magnitude` on every lane, the sign
    /// alternating row by row: three threats of 100 already take a lane past `i16` at scale 127.
    fn heavy_threats_network(embedding_dim: usize, magnitude: f32) -> PsqNetwork {
        let mut network = sample_network_of(embedding_dim, &[8], true);
        for row in psq::ROWS..psq_threats::ROWS {
            let sign = if row % 2 == 0 { 1.0 } else { -1.0 };
            for lane in 0..embedding_dim {
                network.transform[row * embedding_dim + lane] = sign * magnitude;
            }
        }
        network
    }

    #[test]
    fn threat_rows_beyond_the_i16_bound_quantize_and_saturate_exactly() {
        // The bound that refused dorothy's generation 1 counted 128 threat rows at their largest:
        // these networks' would be 128 * 100. Now only the piece-square rows are bounded, and the
        // threat rows saturate as they are added. Along random games, the vectorized path must
        // equal the scalar from-scratch one exactly, saturated lanes included; widths 16 and 24
        // take the kernel with and without a scalar tail.
        use crate::movegen::legal::legal_moves;

        for embedding_dim in [16, 24] {
            let network = QuantizedPsqNetwork::quantize(&heavy_threats_network(embedding_dim, 100.0))
                .expect("threat rows are not in the bound, and 100 at scale 127 fits i16");
            let mut saturated = false;
            let mut seed = 0x9E3779B97F4A7C15u64;
            for _game in 0..6 {
                let (mut board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
                let mut accumulators = network.refresh_by_color(&board);
                let mut next = accumulators.clone();
                for ply in 0..100 {
                    let moves = legal_moves(&board, board.side_to_move());
                    if moves.is_empty() || board.halfmove_clock() >= 100 {
                        break;
                    }
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let mut child = board;
                    child.make_move(moves[(seed >> 32) as usize % moves.len()]);

                    network.update(&accumulators, &mut next, &board, &child);
                    assert_eq!(network.evaluate_by_color_cp(&next, &child), network.evaluate(&child), "width {embedding_dim}, ply {ply}");
                    saturated |= network.refresh(&child, Color::White).iter().any(|lane| lane.abs() > i16::MAX as i32);
                    std::mem::swap(&mut accumulators, &mut next);
                    board = child;
                }
            }
            assert!(saturated, "width {embedding_dim}: no lane ever left i16, so saturation went untested");
        }
    }

    #[test]
    fn a_transform_weight_beyond_i16_on_its_own_is_refused() {
        // 300 at scale 127 is 38,100: no shift makes one such weight fit, whatever the bound says.
        let error = QuantizedPsqNetwork::quantize(&heavy_threats_network(16, 300.0)).unwrap_err();
        assert!(error.contains("a transform weight of 300.0"), "{error}");
    }

    #[test]
    fn a_threat_row_reaches_the_accumulator() {
        // White's pawn on e4 attacks Black's knight on d5: with White to move, the counter network
        // pointed at that threat row reads 1; with the knight elsewhere, 0.
        let row = psq_threats::threat_row(Square::E4, Square::D5);
        let mut counter = counter_network(0);
        counter.threats = true;
        counter.transform = vec![0.0; psq_threats::ROWS];
        counter.transform[row] = 1.0;
        let (attacked, _) = Board::from_fen("4k3/8/8/3n4/4P3/8/8/4K3 w - - 0 1").unwrap();
        let (safe, _) = Board::from_fen("4k3/8/8/n7/4P3/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(counter.evaluate_pawns(&attacked), 1.0);
        assert_eq!(counter.evaluate_pawns(&safe), 0.0);
        // The engine's path adds the threat rows at evaluation, onto piece-square accumulators.
        let quantized = QuantizedPsqNetwork::quantize(&counter).unwrap();
        let accumulators = quantized.refresh_by_color(&attacked);
        assert_eq!(quantized.evaluate_by_color_cp(&accumulators, &attacked), 100);
    }

    #[test]
    fn a_network_survives_a_write_and_a_load_unchanged() {
        let path = temp_path("round-trip");
        let network = sample_network(8, &[6, 4]);

        network.write(&path).expect("writing a valid network should succeed");
        let loaded = load_psq(&path);

        assert_eq!(loaded, network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_single_hidden_layer_round_trips_too() {
        // The hidden-layer count is read from the header rather than fixed, so the degenerate case
        // of one layer has to work without a special path.
        let path = temp_path("one-hidden");
        let network = sample_network(4, &[3]);

        network.write(&path).expect("writing should succeed");
        assert_eq!(load_psq(&path), network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_head_with_no_hidden_layer_round_trips_and_evaluates() {
        // The output layer reads the accumulator directly: a zero hidden-layer count in the header,
        // and a forward pass whose hidden loop runs zero times.
        let path = temp_path("no-hidden");
        let network = sample_network(4, &[]);

        network.write(&path).expect("writing should succeed");
        let loaded = load_psq(&path);
        assert_eq!(loaded, network);

        let (board, _) = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        assert!(loaded.evaluate_pawns(&board).is_finite());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn amandas_own_shape_round_trips() {
        // The real configured shape (`embedding_dim: 256`, `value_head.hidden_dim: []`), not just a
        // small fixture: this is the only test that exercises the actual ~200k-parameter file the
        // engine will load, including its size.
        let path = temp_path("amanda-shape");
        let network = sample_network(256, &[]);

        network.write(&path).expect("writing should succeed");
        assert_eq!(load_psq(&path), network);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_retired_psq768_file_is_refused_by_name() {
        // Tag 10, psq768's, retired: a file under it is refused, naming the kind.
        let path = temp_path("psq768");
        sample_network(8, &[]).write(&path).expect("writing should succeed");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[8..12].copy_from_slice(&FEATURE_SET_PSQ768.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        let error = Network::load(&path).expect_err("a psq768 file must be refused");
        assert!(error.contains("psq768"), "unexpected error: {error}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_file_that_is_not_a_network_is_refused_by_its_magic() {
        let path = temp_path("bad-magic");
        std::fs::write(&path, b"not a network at all, just some bytes").unwrap();

        let error = Network::load(&path).expect_err("a file with the wrong magic must be refused");
        assert!(error.contains("bad magic"), "unexpected error: {error}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_future_format_version_is_refused_rather_than_guessed_at() {
        // The case the version field exists for: the layout changes, an older engine meets a newer
        // file, and reading it as if it were this version would load cleanly and play nonsense.
        let path = temp_path("future-version");
        let network = sample_network(4, &[3]);
        network.write(&path).unwrap();

        let mut bytes = std::fs::read(&path).unwrap();
        bytes[4..8].copy_from_slice(&(VERSION + 1).to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();

        let error = Network::load(&path).expect_err("an unknown version must be refused");
        assert!(error.contains("version"), "unexpected error: {error}");
        std::fs::remove_file(&path).ok();
    }

    /// A network trained under the atan curve, version 1, still loads and plays, its pawn measured
    /// through that curve; a new file is version 2, measured through the logistic.
    #[test]
    fn an_atan_network_still_plays_its_pawn_measured_through_its_own_curve() {
        let path = temp_path("atan-version");
        let network = sample_network(16, &[8]);
        network.write(&path).unwrap();
        assert_eq!(file_version(&path).unwrap(), VERSION);
        let current = Network::load(&path).unwrap();

        let mut bytes = std::fs::read(&path).unwrap();
        bytes[4..8].copy_from_slice(&ATAN_VERSION.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(file_version(&path).unwrap(), ATAN_VERSION);
        let old = Network::load(&path).expect("a version 1 network still loads");

        let Network::Psq(_, quantized) = &old;
        let through_atan = measure_label_percent_through(|board| quantized.evaluate(board), &crate::labels::Curve::Atan(*crate::labels::NETWORK));
        assert_eq!(old.pawn_percent(), through_atan);
        let Network::Psq(_, quantized) = &current;
        assert_eq!(current.pawn_percent(), measure_label_percent(|board| quantized.evaluate(board)));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_retired_feature_set_tag_0_is_refused_as_unknown() {
        // Tag 0 held the retired single-perspective network. Reading such a file as psq would
        // fail only at its shapes, if at all, so the tag itself must be refused.
        let path = temp_path("tag-0-weights");
        let network = sample_network(4, &[3]);
        network.write(&path).unwrap();

        let mut bytes = std::fs::read(&path).unwrap();
        bytes[8..12].copy_from_slice(&0u32.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();

        let error = Network::load(&path).expect_err("a tag-0 file must be refused");
        assert!(error.contains("an unknown feature set"), "unexpected error: {error}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_truncated_file_reports_what_it_ran_out_of_rather_than_panicking() {
        let path = temp_path("truncated");
        let network = sample_network(8, &[6, 4]);
        network.write(&path).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() - 17]).unwrap();

        let error = Network::load(&path).expect_err("a truncated file must be refused");
        assert!(error.contains("truncated"), "unexpected error: {error}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn trailing_bytes_are_refused_even_though_every_weight_was_read() {
        // Everything the header asked for is present, so this only fails if the reader checks that
        // it consumed the file exactly -- which is what catches a layout mismatch landing on a
        // shorter prefix.
        let path = temp_path("trailing");
        let network = sample_network(8, &[6, 4]);
        network.write(&path).unwrap();

        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(&[0u8; 4]);
        std::fs::write(&path, &bytes).unwrap();

        let error = Network::load(&path).expect_err("trailing bytes must be refused");
        assert!(error.contains("unexpected bytes"), "unexpected error: {error}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_file_is_an_error_naming_the_path() {
        let path = temp_path("does-not-exist-at-all");
        std::fs::remove_file(&path).ok();

        let error = Network::load(&path).expect_err("a missing file must be an error");
        assert!(error.contains(&path), "the error should name the path it tried: {error}");
    }

    /// Against PeSTO's own quiescence the label percent is exactly 100, and it follows a scaled
    /// evaluation; rounded to its step and clamped, the floor's `c` at the bracket's top (10%).
    #[test]
    fn the_label_percent_of_pesto_s_quiescence_is_exactly_100_and_follows_a_scaled_evaluation() {
        let quiescence = crate::search::pesto_quiescence;
        assert_eq!(measure_label_percent(quiescence), 100);
        assert_eq!(measure_label_percent(|board| 2 * quiescence(board)), 200);
        assert_eq!(measure_label_percent(|board| (quiescence(board) as f64 * 0.6).round() as i32), 60);
        assert_eq!(measure_label_percent(|board| quiescence(board) / 100), PAWN_SCALE_MIN_PERCENT);
        assert_eq!(measure_label_percent(|board| 5 * quiescence(board)), PAWN_SCALE_MAX_PERCENT);
    }

    /// Every line of the compiled-in positions is a legal FEN, the side to move not in check (the
    /// search reads no static evaluation in check), and there are the 10,000 written.
    #[test]
    fn the_pawn_scale_positions_are_10000_legal_positions_not_in_check() {
        let boards = pawn_scale_boards();
        assert_eq!(boards.len(), 10_000);
        for board in boards {
            assert!(!crate::movegen::attacks::is_in_check(board, board.side_to_move()), "{}", board.to_fen(1));
        }
    }

    #[test]
    fn a_loaded_network_carries_its_measured_pawn_scale() {
        let network = Network::from_psq(sample_network(16, &[8])).unwrap();
        let Network::Psq(_, quantized) = &network;
        assert_eq!(network.pawn_percent(), measure_label_percent(|board| quantized.evaluate(board)));
        assert_eq!(network.pawn_percent() % PAWN_SCALE_STEP_PERCENT, 0);    }

    /// A network without `parameters.txt` is untuned, a retired `margin.txt` beside it ignored;
    /// `parameters.txt` round trips every field, and a malformed one is refused rather than falling
    /// back.
    #[test]
    fn a_loaded_network_carries_the_parameters_beside_it() {
        let dir = std::env::temp_dir().join(format!("kalo-chess-nnue-parameters-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("champion.nnue").to_str().unwrap().to_string();
        let parameters_path = parameters_file_beside(&path);
        assert_eq!(Path::new(&parameters_path), dir.join("parameters.txt"));
        sample_network(16, &[8]).write(&path).unwrap();
        std::fs::remove_file(&parameters_path).ok();
        std::fs::write(dir.join("margin.txt"), "margin_per_ply\t150\n").unwrap();
        assert_eq!(Network::load(&path).unwrap().tuning(), Tuning::Untuned, "margin.txt is ignored");

        let tuned = SearchParameters::from_values([1, 2, 3, 4, 5, 6, 7, 8, -9, -10]);
        tuned.write_file(&parameters_path).unwrap();
        assert_eq!(Network::load(&path).unwrap().tuning(), Tuning::Parameters(tuned));
        assert_eq!(Network::load(&path).unwrap().tuning().parameters(), tuned);

        let good = tuned.to_file_text();
        let malformed_files = [
            good.replacen("razoring_margin\t1\n", "", 1),
            good.replacen("razoring_margin\t1", "razoring_margin\tx", 1),
            good.replacen("razoring_margin\t1", "razoring_margn\t1", 1),
            good.replacen("razoring_margin\t1", "razoring_margin 1", 1),
            format!("{good}razoring_margin\t1\n"),
            String::new(),
        ];
        for malformed in malformed_files {
            std::fs::write(&parameters_path, &malformed).unwrap();
            assert!(Network::load(&path).is_err(), "{malformed:?} should be refused");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_refuses_a_value_head_whose_layers_do_not_connect() {
        // The check that matters most in `validate`: each layer's own weight length can be
        // self-consistent while the chain still does not join up, and that would otherwise surface
        // only as a wrong number deep inside the forward pass.
        let path = temp_path("disconnected");
        let mut network = sample_network(8, &[6, 4]);
        network.output.in_dim = 5;
        network.output.weight = vec![0.0; 5];

        let error = network.write(&path).expect_err("a disconnected value head must be refused");
        assert!(error.contains("output layer reads 5"), "unexpected error: {error}");
        assert!(!std::path::Path::new(&path).exists(), "nothing should have been written");
    }

    /// A network that reports whether one chosen feature row is active in the side to move's
    /// perspective: the transform is zero everywhere but that row, every bias is zero, there is no
    /// hidden layer, and the output reads the side to move's accumulator alone. So `evaluate_pawns`
    /// returns the clipped ReLU of that row's activation, which makes the accumulator directly
    /// observable without hand-computing a real forward pass.
    fn counter_network(row: usize) -> PsqNetwork {
        let mut transform = vec![0.0f32; psq::ROWS];
        transform[row] = 1.0;
        PsqNetwork {
            embedding_dim: 1,
            transform,
            transform_bias: vec![0.0],
            hidden: Vec::new(),
            output: Layer { in_dim: 2, out_dim: 1, weight: vec![1.0, 0.0], bias: vec![0.0] },
            step: 0,
            learning_rate: 0.0,
            threats: false,
            pairwise: false,
            screlu: false,
            stack: None,
            material: None,
            halfk: false,
        }
    }

    #[test]
    fn castling_rights_and_en_passant_actually_reach_the_accumulator() {
        // Psq's two additions over plain piece-square, and the two easiest to drop silently:
        // they are flat flags with no piece to hang off, so nothing else would notice their
        // absence. Each is checked by pointing the counter network at its own row.
        let (with_rights, _) = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        let (without_rights, _) = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1").unwrap();
        let own_kingside = counter_network(psq::CASTLING_ROW);
        assert_eq!(own_kingside.evaluate_pawns(&with_rights), 1.0);
        assert_eq!(own_kingside.evaluate_pawns(&without_rights), 0.0);

        // After 1. e4 the en passant target is on the e-file, file index 4.
        let (after_double_push, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1").unwrap();
        let e_file = counter_network(psq::EN_PASSANT_ROW + 4);
        assert_eq!(e_file.evaluate_pawns(&after_double_push), 1.0);
        assert_eq!(e_file.evaluate_pawns(&with_rights), 0.0);
    }

    #[test]
    fn centipawns_are_a_hundred_times_the_pawns_clamped_to_the_ceiling() {
        // The output is in pawns and structurally unbounded, so the conversion is `100 *` inside the
        // ceiling and the ceiling's 12,800 outside it, far below the tablebase and mate bands.
        let (board, _) = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        // Row 0 is an own pawn on a1, which never exists, so the output is its bias alone.
        let mut network = counter_network(0);
        for (pawns, expected) in
            [(-1e6f32, -12_800), (-140.0, -12_800), (-40.0, -4000), (-1.25, -125), (0.0, 0), (2.5, 250), (127.0, 12_700), (1e6, 12_800)]
        {
            // On the output bias: the clipped ReLU bounds everything before it.
            network.output.bias = vec![pawns];
            assert_eq!(network.evaluate_pawns(&board), pawns);
            assert_eq!(network.evaluate(&board), expected, "{pawns} pawns");
        }
    }

    #[test]
    fn write_refuses_a_transform_that_does_not_match_the_row_count() {
        let path = temp_path("bad-transform");
        let mut network = sample_network(8, &[6, 4]);
        network.transform.pop();

        let error = network.write(&path).expect_err("a short transform must be refused");
        assert!(error.contains("transform holds"), "unexpected error: {error}");
        assert!(!std::path::Path::new(&path).exists(), "nothing should have been written");
    }
}
