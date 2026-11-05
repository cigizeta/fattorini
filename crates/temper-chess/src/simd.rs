//! The AVX2/FMA3 primitives `nnue.rs`'s forward pass is built on.
//!
//! # Safety obligation, shared by everything here
//! Requires the CPU to support AVX2 and FMA3. This project compiles for x86-64-v3 (see
//! `.cargo/config.toml`), which includes both, and `main` refuses a CPU without them before anything
//! here runs; nothing in this module checks again.

use std::arch::x86_64::__m256;

/// How many independent accumulator registers the dot product keeps in flight.
///
/// The reason this is not `1` is the whole point of the rewrite. An FMA has a latency of about four
/// cycles against a throughput of two per cycle, so a single accumulator -- each FMA waiting on the
/// previous one -- reaches at best a quarter of the achievable rate however wide the lanes are.
/// Four independent chains cover the latency, after which the loop is limited by issue width and by
/// how fast the weights arrive rather than by the dependency.
///
/// Measured on a since-retired network's value head (`benches/nnue_bench.rs`), the scalar single-accumulator version
/// this replaces ran at about 6 GFLOP/s, single digits against a core capable of tens.
const ACCUMULATORS: usize = 4;

/// The floats one pass of `dot_product_avx2`'s chains covers: `clipped_dot_product_pair_avx2` agrees
/// with it bit for bit only on halves that are a multiple of this.
pub(crate) const DOT_PRODUCT_BLOCK: usize = 8 * ACCUMULATORS;

/// `sum(a[i] * b[i])`, vectorized eight lanes at a time with `ACCUMULATORS` independent chains.
///
/// `a` and `b` must have equal length -- checked by `debug_assert_eq!`, not a `Result`, since a
/// length mismatch here is a caller bug (mismatched weight/feature shapes), not a real runtime
/// condition to recover from.
///
/// The summation order is fixed but is *not* left-to-right, so this does not reproduce a scalar
/// loop's last bits. Every caller compares against a tolerance for that reason, and the order is
/// deterministic, so a search built on it stays reproducible.
pub(crate) fn dot_product_avx2(a: &[f32], b: &[f32]) -> f32 {
    use std::arch::x86_64::{_mm256_fmadd_ps, _mm256_loadu_ps, _mm256_setzero_ps};

    debug_assert_eq!(a.len(), b.len());

    let mut accumulators = [unsafe { _mm256_setzero_ps() }; ACCUMULATORS];
    let lanes = 8 * ACCUMULATORS;
    let blocks = a.len() / lanes;

    for block in 0..blocks {
        let base = block * lanes;
        for (index, accumulator) in accumulators.iter_mut().enumerate() {
            let offset = base + index * 8;
            *accumulator = unsafe {
                let a_vec = _mm256_loadu_ps(a.as_ptr().add(offset));
                let b_vec = _mm256_loadu_ps(b.as_ptr().add(offset));
                _mm256_fmadd_ps(a_vec, b_vec, *accumulator)
            };
        }
    }

    // Whatever is left over: whole eight-lane chunks first, then any scalar tail. The value heads'
    // widths are usually multiples of 32 so neither need run in production, but a `hidden_dim` is
    // configurable and the primitive is shared, so both have to be right rather than merely unused.
    let mut offset = blocks * lanes;
    while offset + 8 <= a.len() {
        accumulators[0] = unsafe {
            let a_vec = _mm256_loadu_ps(a.as_ptr().add(offset));
            let b_vec = _mm256_loadu_ps(b.as_ptr().add(offset));
            _mm256_fmadd_ps(a_vec, b_vec, accumulators[0])
        };
        offset += 8;
    }

    let mut sum = 0.0;
    for accumulator in accumulators {
        sum += horizontal_sum_avx2(accumulator);
    }
    for i in offset..a.len() {
        sum += a[i] * b[i];
    }
    sum
}

/// `dot_product_avx2(clamp(own ++ opponent, 0, 1), weights)` without building the concatenation:
/// the clipped ReLU applied as each lane is loaded, `own` then `opponent` read through the same
/// `ACCUMULATORS` chains. Copying both halves into one buffer first cost about 15% of a whole search
/// (samply, caroline, 2026-10-07).
///
/// Bit for bit `dot_product_avx2` over the clamped concatenation, as long as each half is a whole
/// number of `8 * ACCUMULATORS` blocks: the blocks then fall exactly where they fall over the
/// concatenation, each lane in the same chain, and there is no tail. The caller checks that and
/// copies otherwise.
pub(crate) fn clipped_dot_product_pair_avx2(own: &[f32], opponent: &[f32], weights: &[f32]) -> f32 {
    use std::arch::x86_64::{_mm256_fmadd_ps, _mm256_loadu_ps, _mm256_max_ps, _mm256_min_ps, _mm256_set1_ps, _mm256_setzero_ps};

    let lanes = 8 * ACCUMULATORS;
    debug_assert_eq!(own.len(), opponent.len());
    debug_assert_eq!(own.len() % lanes, 0);
    debug_assert_eq!(weights.len(), own.len() + opponent.len());

    let zero = unsafe { _mm256_setzero_ps() };
    let one = unsafe { _mm256_set1_ps(1.0) };
    let mut accumulators = [zero; ACCUMULATORS];
    for (half, values) in [own, opponent].into_iter().enumerate() {
        let weights = &weights[half * own.len()..(half + 1) * own.len()];
        for block in 0..values.len() / lanes {
            let base = block * lanes;
            for (index, accumulator) in accumulators.iter_mut().enumerate() {
                let offset = base + index * 8;
                *accumulator = unsafe {
                    let loaded = _mm256_loadu_ps(values.as_ptr().add(offset));
                    let clipped = _mm256_min_ps(_mm256_max_ps(loaded, zero), one);
                    let weight = _mm256_loadu_ps(weights.as_ptr().add(offset));
                    _mm256_fmadd_ps(weight, clipped, *accumulator)
                };
            }
        }
    }

    let mut sum = 0.0;
    for accumulator in accumulators {
        sum += horizontal_sum_avx2(accumulator);
    }
    sum
}

/// Eight dot products at once, `rows` being eight consecutive rows of a layer's row-major weights
/// and the input `clamp(own ++ opponent, 0, 1)`, read in place: what a first hidden layer needs.
///
/// One FMA chain per output, so eight independent chains, enough to cover an FMA's latency, where
/// eight separate `dot_product_avx2` calls each paid a horizontal sum at the end; here a single
/// reduction turns the eight registers into the eight sums together. Each input block is loaded and
/// clamped once for all eight outputs. Halves must be a multiple of 8 long. Not bit for bit
/// `dot_product_avx2`: the summation order differs, so callers compare within a tolerance.
pub(crate) fn clipped_dot_products_8_avx2(rows: &[f32], own: &[f32], opponent: &[f32]) -> [f32; 8] {
    use std::arch::x86_64::{_mm256_fmadd_ps, _mm256_loadu_ps, _mm256_max_ps, _mm256_min_ps, _mm256_set1_ps, _mm256_setzero_ps};

    let in_dim = own.len() + opponent.len();
    debug_assert_eq!(rows.len(), 8 * in_dim);
    debug_assert_eq!(own.len() % 8, 0);
    debug_assert_eq!(opponent.len() % 8, 0);

    let zero = unsafe { _mm256_setzero_ps() };
    let one = unsafe { _mm256_set1_ps(1.0) };
    let mut sums = [zero; 8];
    for (start, values) in [(0, own), (own.len(), opponent)] {
        for chunk in 0..values.len() / 8 {
            let offset = chunk * 8;
            unsafe {
                let loaded = _mm256_loadu_ps(values.as_ptr().add(offset));
                let input = _mm256_min_ps(_mm256_max_ps(loaded, zero), one);
                for (output, sum) in sums.iter_mut().enumerate() {
                    let weight = _mm256_loadu_ps(rows.as_ptr().add(output * in_dim + start + offset));
                    *sum = _mm256_fmadd_ps(weight, input, *sum);
                }
            }
        }
    }

    sums_of_8(sums)
}

/// Eight dot products at once, `rows` being eight consecutive rows of a layer's row-major weights and
/// `input` read as it is, unclamped: `clipped_dot_products_8_avx2` without the clip, for a hidden
/// layer beside the output, whose whole products run to 65025. `input` a multiple of 8 long. Not bit
/// for bit `dot_product_avx2`; deterministic, so every caller computing a row through it agrees.
pub(crate) fn dot_products_8_avx2(rows: &[f32], input: &[f32]) -> [f32; 8] {
    use std::arch::x86_64::{_mm256_fmadd_ps, _mm256_loadu_ps, _mm256_setzero_ps};

    let in_dim = input.len();
    debug_assert_eq!(rows.len(), 8 * in_dim);
    debug_assert_eq!(in_dim % 8, 0);

    let mut sums = [unsafe { _mm256_setzero_ps() }; 8];
    for chunk in 0..in_dim / 8 {
        let offset = chunk * 8;
        unsafe {
            let values = _mm256_loadu_ps(input.as_ptr().add(offset));
            for (output, sum) in sums.iter_mut().enumerate() {
                let weight = _mm256_loadu_ps(rows.as_ptr().add(output * in_dim + offset));
                *sum = _mm256_fmadd_ps(weight, values, *sum);
            }
        }
    }
    sums_of_8(sums)
}

/// Eight registers to eight sums, register `i`'s in place `i`.
fn sums_of_8(sums: [__m256; 8]) -> [f32; 8] {
    use std::arch::x86_64::{_mm256_add_ps, _mm256_hadd_ps, _mm256_permute2f128_ps, _mm256_storeu_ps};

    // `hadd(a, b)` sums adjacent pairs within each 128-bit half,
    // giving [a0+a1, a2+a3, b0+b1, b2+b3] per half; twice over, each half of `low4`/`high4` holds
    // one partial sum per register (`low4` for registers 0 to 3, `high4` for 4 to 7): its lower
    // half from lanes 0 to 3, its upper half from lanes 4 to 7. Pairing the lower halves of both
    // and the upper halves of both, then adding, leaves register `i`'s whole sum in lane `i`.
    let mut result = [0.0f32; 8];
    unsafe {
        let pair01 = _mm256_hadd_ps(sums[0], sums[1]);
        let pair23 = _mm256_hadd_ps(sums[2], sums[3]);
        let pair45 = _mm256_hadd_ps(sums[4], sums[5]);
        let pair67 = _mm256_hadd_ps(sums[6], sums[7]);
        let low4 = _mm256_hadd_ps(pair01, pair23);
        let high4 = _mm256_hadd_ps(pair45, pair67);
        let lanes_0_to_3 = _mm256_permute2f128_ps(low4, high4, 0x20);
        let lanes_4_to_7 = _mm256_permute2f128_ps(low4, high4, 0x31);
        _mm256_storeu_ps(result.as_mut_ptr(), _mm256_add_ps(lanes_0_to_3, lanes_4_to_7));
    }
    result
}

/// `destination = clamp(source, 0, 1)`, vectorized: the clipped ReLU written into another buffer in
/// the same pass that reads it, instead of a copy and then `clipped_relu_avx2` in place. Lengths must
/// match.
pub(crate) fn clipped_copy_avx2(destination: &mut [f32], source: &[f32]) {
    use std::arch::x86_64::{_mm256_loadu_ps, _mm256_max_ps, _mm256_min_ps, _mm256_set1_ps, _mm256_setzero_ps, _mm256_storeu_ps};

    debug_assert_eq!(destination.len(), source.len());

    let zero = unsafe { _mm256_setzero_ps() };
    let one = unsafe { _mm256_set1_ps(1.0) };
    let chunks = source.len() / 8;
    for chunk in 0..chunks {
        let offset = chunk * 8;
        unsafe {
            let loaded = _mm256_loadu_ps(source.as_ptr().add(offset));
            _mm256_storeu_ps(destination.as_mut_ptr().add(offset), _mm256_min_ps(_mm256_max_ps(loaded, zero), one));
        }
    }
    for i in (chunks * 8)..source.len() {
        destination[i] = source[i].clamp(0.0, 1.0);
    }
}

/// `destination[i] += source[i]` on a quantized accumulator's `i16` lanes, sixteen at a time.
/// Wrapping, which never happens: the quantization's shift is chosen so no lane can overflow.
pub(crate) fn add_assign_i16_avx2(destination: &mut [i16], source: &[i16]) {
    use std::arch::x86_64::{__m256i, _mm256_add_epi16, _mm256_loadu_si256, _mm256_storeu_si256};

    debug_assert_eq!(destination.len(), source.len());
    let chunks = destination.len() / 16;
    for chunk in 0..chunks {
        let offset = chunk * 16;
        unsafe {
            let left = _mm256_loadu_si256(destination.as_ptr().add(offset) as *const __m256i);
            let right = _mm256_loadu_si256(source.as_ptr().add(offset) as *const __m256i);
            _mm256_storeu_si256(destination.as_mut_ptr().add(offset) as *mut __m256i, _mm256_add_epi16(left, right));
        }
    }
    for i in (chunks * 16)..destination.len() {
        destination[i] = destination[i].wrapping_add(source[i]);
    }
}

/// `destination = source + added rows` on a quantized accumulator's `i16` lanes, summed in `i32` and
/// saturated to `i16` only when stored, so that no number of rows can wrap a lane. Row `r` is
/// `rows[r * destination.len()..(r + 1) * destination.len()]`. What a psq_threats evaluation adds
/// its threat rows with: `source` the incremental piece-square accumulator, which fits `i16` by
/// itself. Saturating is exact for an accumulator lane, not an approximation: a lane is only ever
/// read as an activation, clamped to `0..=one` first, and `one` is within `i16`.
pub(crate) fn add_rows_saturating_i16_avx2(destination: &mut [i16], source: &[i16], rows: &[i16], added: &[u16]) {
    use std::arch::x86_64::{
        __m128i, __m256i, _mm256_add_epi32, _mm256_cvtepi16_epi32, _mm256_packs_epi32, _mm256_permute4x64_epi64,
        _mm256_storeu_si256, _mm_loadu_si128,
    };

    let length = destination.len();
    debug_assert_eq!(length, source.len());
    debug_assert!(added.iter().all(|&row| (row as usize + 1) * length <= rows.len()));
    // Sixteen `i16` lanes from `lanes` widened to two registers of eight `i32`: lanes 0 to 7, 8 to 15.
    let widen = |lanes: *const i16| -> [__m256i; 2] {
        unsafe {
            [
                _mm256_cvtepi16_epi32(_mm_loadu_si128(lanes as *const __m128i)),
                _mm256_cvtepi16_epi32(_mm_loadu_si128(lanes.add(8) as *const __m128i)),
            ]
        }
    };
    let chunks = length / 16;
    for chunk in 0..chunks {
        let offset = chunk * 16;
        unsafe {
            let [mut low, mut high] = widen(source.as_ptr().add(offset));
            for &row in added {
                let [row_low, row_high] = widen(rows.as_ptr().add(row as usize * length + offset));
                low = _mm256_add_epi32(low, row_low);
                high = _mm256_add_epi32(high, row_high);
            }
            // `packs` saturates, but within each 128-bit half: its 64-bit quarters come out as lanes
            // 0 to 3, 8 to 11, 4 to 7, 12 to 15. 0xD8 takes the quarters in order 0, 2, 1, 3.
            let packed = _mm256_permute4x64_epi64(_mm256_packs_epi32(low, high), 0xD8);
            _mm256_storeu_si256(destination.as_mut_ptr().add(offset) as *mut __m256i, packed);
        }
    }
    for i in (chunks * 16)..length {
        let sum = source[i] as i32 + added.iter().map(|&row| rows[row as usize * length + i] as i32).sum::<i32>();
        destination[i] = sum.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    }
}

/// `child = parent - removed rows + added rows` on a quantized accumulator's `i16` lanes, in one
/// pass: each block of lanes is loaded from `parent` once, every changed row's matching lanes applied
/// in registers, and the result stored to `child` once. Row `r` is
/// `rows[r * child.len()..(r + 1) * child.len()]`. Wrapping, as `add_assign_i16_avx2`.
///
/// Under the replay's sixteen threads this was about 10% of a search, nearly all of it waiting on
/// the weight rows' loads (samply, 2026-10-12): so the lanes go four chunks (64 lanes) at a time,
/// four independent loads per row in flight instead of one, and the two commonest moves, a quiet
/// move (one row added, one removed) and a capture (one added, two removed), get the row counts as
/// constants, so their row loops unroll.
pub(crate) fn update_i16_avx2(child: &mut [i16], parent: &[i16], rows: &[i16], added: &[usize], removed: &[usize]) {
    match (added, removed) {
        (&[add], &[remove]) => update_i16_blocks(child, parent, rows, &[add], &[remove]),
        (&[add], &[first, second]) => update_i16_blocks(child, parent, rows, &[add], &[first, second]),
        _ => update_i16_blocks(child, parent, rows, added, removed),
    }
}

/// `update_i16_avx2`'s body, inlined into each of its cases so that a constant row count unrolls.
/// Its loops index the lanes by `k`, which also offsets each pointer: an iterator would hide that.
#[inline(always)]
#[allow(clippy::needless_range_loop)]
fn update_i16_blocks(child: &mut [i16], parent: &[i16], rows: &[i16], added: &[usize], removed: &[usize]) {
    use std::arch::x86_64::{__m256i, _mm256_add_epi16, _mm256_loadu_si256, _mm256_storeu_si256, _mm256_sub_epi16};

    const BLOCK: usize = 4;
    let length = child.len();
    debug_assert_eq!(length, parent.len());
    debug_assert!(added.iter().chain(removed).all(|&row| (row + 1) * length <= rows.len()));
    let blocks = length / (16 * BLOCK);
    for block in 0..blocks {
        let offset = block * 16 * BLOCK;
        unsafe {
            let mut lanes: [__m256i; BLOCK] =
                std::array::from_fn(|k| _mm256_loadu_si256(parent.as_ptr().add(offset + 16 * k) as *const __m256i));
            for &row in removed {
                let weights = rows.as_ptr().add(row * length + offset);
                for k in 0..BLOCK {
                    lanes[k] = _mm256_sub_epi16(lanes[k], _mm256_loadu_si256(weights.add(16 * k) as *const __m256i));
                }
            }
            for &row in added {
                let weights = rows.as_ptr().add(row * length + offset);
                for k in 0..BLOCK {
                    lanes[k] = _mm256_add_epi16(lanes[k], _mm256_loadu_si256(weights.add(16 * k) as *const __m256i));
                }
            }
            for k in 0..BLOCK {
                _mm256_storeu_si256(child.as_mut_ptr().add(offset + 16 * k) as *mut __m256i, lanes[k]);
            }
        }
    }
    let chunks = length / 16;
    for chunk in (blocks * BLOCK)..chunks {
        let offset = chunk * 16;
        unsafe {
            let mut lanes = _mm256_loadu_si256(parent.as_ptr().add(offset) as *const __m256i);
            for &row in removed {
                let weights = _mm256_loadu_si256(rows.as_ptr().add(row * length + offset) as *const __m256i);
                lanes = _mm256_sub_epi16(lanes, weights);
            }
            for &row in added {
                let weights = _mm256_loadu_si256(rows.as_ptr().add(row * length + offset) as *const __m256i);
                lanes = _mm256_add_epi16(lanes, weights);
            }
            _mm256_storeu_si256(child.as_mut_ptr().add(offset) as *mut __m256i, lanes);
        }
    }
    for i in (chunks * 16)..length {
        let mut lane = parent[i];
        for &row in removed {
            lane = lane.wrapping_sub(rows[row * length + i]);
        }
        for &row in added {
            lane = lane.wrapping_add(rows[row * length + i]);
        }
        child[i] = lane;
    }
}

/// How a quantized accumulator lane becomes an activation: clamped to `0..=one`, `rounding` added,
/// shifted right by `shift`. `one` is `127 << shift` (`255 << shift` for a pairwise network's
/// halves) and `rounding` half a step, so an activation runs from 0 to 127 (or 255) and the shift
/// rounds to nearest (`QuantizedPsqNetwork`).
#[derive(Clone, Copy)]
pub(crate) struct Activation {
    pub one: i16,
    pub rounding: i16,
    pub shift: u32,
}

/// Sixteen accumulator lanes at `lanes`, as activations.
///
/// # Safety
/// `lanes` must point at 16 readable `i16`s.
unsafe fn activations_16(lanes: *const i16, activation: Activation) -> std::arch::x86_64::__m256i {
    use std::arch::x86_64::{
        __m256i, _mm256_add_epi16, _mm256_loadu_si256, _mm256_max_epi16, _mm256_min_epi16, _mm256_set1_epi16,
        _mm256_setzero_si256, _mm256_sra_epi16, _mm_cvtsi32_si128,
    };

    unsafe {
        let loaded = _mm256_loadu_si256(lanes as *const __m256i);
        let clamped = _mm256_min_epi16(_mm256_max_epi16(loaded, _mm256_setzero_si256()), _mm256_set1_epi16(activation.one));
        let rounded = _mm256_add_epi16(clamped, _mm256_set1_epi16(activation.rounding));
        _mm256_sra_epi16(rounded, _mm_cvtsi32_si128(activation.shift as i32))
    }
}

/// The eight `i32` lanes of `v`, summed.
fn horizontal_sum_epi32(v: std::arch::x86_64::__m256i) -> i32 {
    let mut lanes = [0i32; 8];
    unsafe {
        std::arch::x86_64::_mm256_storeu_si256(lanes.as_mut_ptr() as *mut std::arch::x86_64::__m256i, v);
    }
    lanes.iter().sum()
}

/// `sum(activation(own ++ opponent)[i] * weights[i])` in exact integer arithmetic: each accumulator
/// lane turned into its activation as it is loaded, and multiplied by its `i16` weight with
/// `_mm256_madd_epi16`. Each half must be a multiple of 16 long. Integer sums do not depend on their
/// order, so this equals the scalar sum exactly.
pub(crate) fn activation_dot_i16_avx2(own: &[i16], opponent: &[i16], weights: &[i16], activation: Activation) -> i32 {
    use std::arch::x86_64::{__m256i, _mm256_add_epi32, _mm256_loadu_si256, _mm256_madd_epi16, _mm256_setzero_si256};

    debug_assert_eq!(own.len() % 16, 0);
    debug_assert_eq!(own.len(), opponent.len());
    debug_assert_eq!(weights.len(), own.len() + opponent.len());

    let mut sum = unsafe { _mm256_setzero_si256() };
    for (start, lanes) in [(0, own), (own.len(), opponent)] {
        for chunk in 0..lanes.len() / 16 {
            let offset = chunk * 16;
            unsafe {
                let input = activations_16(lanes.as_ptr().add(offset), activation);
                let weight = _mm256_loadu_si256(weights.as_ptr().add(start + offset) as *const __m256i);
                sum = _mm256_add_epi32(sum, _mm256_madd_epi16(input, weight));
            }
        }
    }
    horizontal_sum_epi32(sum)
}

/// Each lane of `lanes` as its activation (`Activation`), into `destination`, sixteen at a time: the
/// `madd` input of `dots_i16_8_avx2`, computed once per evaluation however many rows read it. A
/// multiple of 16 long.
pub(crate) fn activations_i16_avx2(destination: &mut [i16], lanes: &[i16], activation: Activation) {
    use std::arch::x86_64::{__m256i, _mm256_storeu_si256};

    debug_assert_eq!(destination.len(), lanes.len());
    debug_assert_eq!(lanes.len() % 16, 0);
    for chunk in 0..lanes.len() / 16 {
        let offset = chunk * 16;
        unsafe {
            let values = activations_16(lanes.as_ptr().add(offset), activation);
            _mm256_storeu_si256(destination.as_mut_ptr().add(offset) as *mut __m256i, values);
        }
    }
}

/// A pairwise network's perspective, sixteen at a time: for each `i` below `half`, the activations
/// of `lanes[i]` and `lanes[i + half]`, each in `0..=255`, multiplied and brought back to `0..=127`
/// by `(a * b + 256) >> 9`, rounding to nearest (`nnue::product_of`). The product is at most
/// `255 * 255 + 256 = 65281`, under `u16::MAX`, so `mullo`'s low 16 bits hold it whole and the
/// logical shift reads them unsigned. `destination` is `half` long, `lanes` twice that, and `half`
/// a multiple of 16.
pub(crate) fn pairwise_activations_i16_avx2(destination: &mut [i16], lanes: &[i16], activation: Activation) {
    use std::arch::x86_64::{__m256i, _mm256_storeu_si256};

    let half = destination.len();
    debug_assert_eq!(lanes.len(), 2 * half);
    debug_assert_eq!(half % 16, 0);
    for chunk in 0..half / 16 {
        let offset = chunk * 16;
        unsafe {
            let products = products_16(lanes.as_ptr().add(offset), half, activation);
            _mm256_storeu_si256(destination.as_mut_ptr().add(offset) as *mut __m256i, products);
        }
    }
}

/// A screlu network's perspective, sixteen at a time: each lane's activation, in `0..=255`, squared and
/// brought back to `0..=127` by `(a * a + 256) >> 9` (`nnue::product_of` of a lane with itself), into
/// `destination`: the input of a hidden layer whose rows all read the same squares, so they are
/// computed once and stored rather than squared again for every row (`screlu_dot_i16_avx2`, for a
/// single output, stores nothing). A multiple of 16 long.
pub(crate) fn screlu_activations_i16_avx2(destination: &mut [i16], lanes: &[i16], activation: Activation) {
    use std::arch::x86_64::{__m256i, _mm256_add_epi16, _mm256_mullo_epi16, _mm256_set1_epi16, _mm256_srli_epi16, _mm256_storeu_si256};

    debug_assert_eq!(destination.len(), lanes.len());
    debug_assert_eq!(lanes.len() % 16, 0);
    for chunk in 0..lanes.len() / 16 {
        let offset = chunk * 16;
        unsafe {
            let a = activations_16(lanes.as_ptr().add(offset), activation);
            let squares = _mm256_srli_epi16(_mm256_add_epi16(_mm256_mullo_epi16(a, a), _mm256_set1_epi16(256)), 9);
            _mm256_storeu_si256(destination.as_mut_ptr().add(offset) as *mut __m256i, squares);
        }
    }
}

/// A perspective's whole products as f32, sixteen at a time, into `destination`: for a pairwise
/// network (`pairwise`), the activations of `lanes[i]` and `lanes[i + half]` multiplied, `half` being
/// `destination`'s length and `lanes` twice it; for a screlu one, each lane's activation squared,
/// `lanes` as long as `destination`. Each in `0..=65025`, not brought back to 7 bits as `products_16`
/// does: the input of a hidden layer beside the output (`nnue::QuantizedHead::HiddenBeside`). `mullo`'s
/// low 16 bits hold the product whole, read unsigned, widened in order and converted exactly. A
/// multiple of 16 long.
pub(crate) fn whole_products_f32_avx2(destination: &mut [f32], lanes: &[i16], pairwise: bool, activation: Activation) {
    use std::arch::x86_64::{
        _mm256_castsi256_si128, _mm256_cvtepi32_ps, _mm256_cvtepu16_epi32, _mm256_extracti128_si256, _mm256_mullo_epi16,
        _mm256_storeu_ps,
    };

    let count = destination.len();
    debug_assert_eq!(lanes.len(), if pairwise { 2 * count } else { count });
    debug_assert_eq!(count % 16, 0);
    for chunk in 0..count / 16 {
        let offset = chunk * 16;
        unsafe {
            let a = activations_16(lanes.as_ptr().add(offset), activation);
            let b = if pairwise { activations_16(lanes.as_ptr().add(count + offset), activation) } else { a };
            let products = _mm256_mullo_epi16(a, b);
            let low = _mm256_cvtepi32_ps(_mm256_cvtepu16_epi32(_mm256_castsi256_si128(products)));
            let high = _mm256_cvtepi32_ps(_mm256_cvtepu16_epi32(_mm256_extracti128_si256(products, 1)));
            _mm256_storeu_ps(destination.as_mut_ptr().add(offset), low);
            _mm256_storeu_ps(destination.as_mut_ptr().add(offset + 8), high);
        }
    }
}

/// Sixteen products of a pairwise perspective, `(a * b + 256) >> 9` for the halves' activations at
/// `lanes` and `lanes + half` (`pairwise_activations_i16_avx2`).
///
/// # Safety
/// `lanes` and `lanes + half` must each point at 16 readable `i16`s.
unsafe fn products_16(lanes: *const i16, half: usize, activation: Activation) -> std::arch::x86_64::__m256i {
    use std::arch::x86_64::{_mm256_add_epi16, _mm256_mullo_epi16, _mm256_set1_epi16, _mm256_srli_epi16};

    unsafe {
        let a = activations_16(lanes, activation);
        let b = activations_16(lanes.add(half), activation);
        _mm256_srli_epi16(_mm256_add_epi16(_mm256_mullo_epi16(a, b), _mm256_set1_epi16(256)), 9)
    }
}

/// `sum(products(own ++ opponent)[i] * weights[i])` in exact integer arithmetic: the head of a
/// pairwise network with no hidden layer, each perspective's products computed as its lanes are
/// loaded and multiplied by their `i16` weights at once, as `activation_dot_i16_avx2` does for psq.
/// Nothing is stored between the two: writing the products out and reading them back cost the head
/// about 15 ns of its 54 (2026-10-15). Each perspective is `2 * half` lanes, `half` a multiple
/// of 16, and `weights` is `2 * half` long, own products first.
pub(crate) fn pairwise_dot_i16_avx2(own: &[i16], opponent: &[i16], weights: &[i16], activation: Activation) -> i32 {
    use std::arch::x86_64::{__m256i, _mm256_add_epi32, _mm256_loadu_si256, _mm256_madd_epi16, _mm256_setzero_si256};

    let half = own.len() / 2;
    debug_assert_eq!(half % 16, 0);
    debug_assert_eq!(own.len(), opponent.len());
    debug_assert_eq!(weights.len(), own.len());

    let mut sum = unsafe { _mm256_setzero_si256() };
    for (start, lanes) in [(0, own), (half, opponent)] {
        for chunk in 0..half / 16 {
            let offset = chunk * 16;
            unsafe {
                let products = products_16(lanes.as_ptr().add(offset), half, activation);
                let weight = _mm256_loadu_si256(weights.as_ptr().add(start + offset) as *const __m256i);
                sum = _mm256_add_epi32(sum, _mm256_madd_epi16(products, weight));
            }
        }
    }
    horizontal_sum_epi32(sum)
}

/// `sum(squares(own ++ opponent)[i] * weights[i])` in exact integer arithmetic: the head of a screlu
/// network, each lane's activation, in `0..=255`, squared and brought back to `0..=127` by
/// `(a * a + 256) >> 9` as it is loaded (`nnue::product_of` of a lane with itself, the bound
/// `products_16`'s), and multiplied by its `i16` weight at once, nothing stored, as
/// `pairwise_dot_i16_avx2`. Each perspective a multiple of 16 lanes, `weights` both perspectives
/// long, own first.
pub(crate) fn screlu_dot_i16_avx2(own: &[i16], opponent: &[i16], weights: &[i16], activation: Activation) -> i32 {
    use std::arch::x86_64::{
        __m256i, _mm256_add_epi16, _mm256_add_epi32, _mm256_loadu_si256, _mm256_madd_epi16, _mm256_mullo_epi16,
        _mm256_set1_epi16, _mm256_setzero_si256, _mm256_srli_epi16,
    };

    debug_assert_eq!(own.len() % 16, 0);
    debug_assert_eq!(own.len(), opponent.len());
    debug_assert_eq!(weights.len(), own.len() + opponent.len());

    let mut sum = unsafe { _mm256_setzero_si256() };
    for (start, lanes) in [(0, own), (own.len(), opponent)] {
        for chunk in 0..lanes.len() / 16 {
            let offset = chunk * 16;
            unsafe {
                let a = activations_16(lanes.as_ptr().add(offset), activation);
                let squares = _mm256_srli_epi16(_mm256_add_epi16(_mm256_mullo_epi16(a, a), _mm256_set1_epi16(256)), 9);
                let weight = _mm256_loadu_si256(weights.as_ptr().add(start + offset) as *const __m256i);
                sum = _mm256_add_epi32(sum, _mm256_madd_epi16(squares, weight));
            }
        }
    }
    horizontal_sum_epi32(sum)
}

/// `screlu_dot_i16_avx2` for two weight sets at once, a taper's middlegame and endgame: each square
/// computed once and multiplied by both sets' weights, so the second set costs its multiply-add
/// alone. Running `screlu_dot_i16_avx2` twice put 67 ns on a 317 ns head at width 256 (2026-10-25).
/// The same integer sums as two calls, so exact.
pub(crate) fn screlu_dot2_i16_avx2(own: &[i16], opponent: &[i16], first: &[i16], second: &[i16], activation: Activation) -> [i32; 2] {
    use std::arch::x86_64::{
        __m256i, _mm256_add_epi16, _mm256_add_epi32, _mm256_loadu_si256, _mm256_madd_epi16, _mm256_mullo_epi16,
        _mm256_set1_epi16, _mm256_setzero_si256, _mm256_srli_epi16,
    };

    debug_assert_eq!(own.len() % 16, 0);
    debug_assert_eq!(own.len(), opponent.len());
    debug_assert_eq!(first.len(), own.len() + opponent.len());
    debug_assert_eq!(second.len(), first.len());

    let mut sums = unsafe { [_mm256_setzero_si256(), _mm256_setzero_si256()] };
    for (start, lanes) in [(0, own), (own.len(), opponent)] {
        for chunk in 0..lanes.len() / 16 {
            let offset = chunk * 16;
            unsafe {
                let a = activations_16(lanes.as_ptr().add(offset), activation);
                let squares = _mm256_srli_epi16(_mm256_add_epi16(_mm256_mullo_epi16(a, a), _mm256_set1_epi16(256)), 9);
                for (sum, weights) in sums.iter_mut().zip([first, second]) {
                    let weight = _mm256_loadu_si256(weights.as_ptr().add(start + offset) as *const __m256i);
                    *sum = _mm256_add_epi32(*sum, _mm256_madd_epi16(squares, weight));
                }
            }
        }
    }
    sums.map(horizontal_sum_epi32)
}

/// `sum(activations[i] * row[i])` for eight consecutive rows of a layer's row-major `i16` weights
/// at once, `activations` from `activations_i16_avx2`: each block of activations loaded once for
/// all eight rows, and the eight sums reduced together. Integer arithmetic, so exact.
pub(crate) fn dots_i16_8_avx2(rows: &[i16], activations: &[i16]) -> [i32; 8] {
    use std::arch::x86_64::{__m256i, _mm256_add_epi32, _mm256_loadu_si256, _mm256_madd_epi16, _mm256_setzero_si256};

    let in_dim = activations.len();
    debug_assert_eq!(rows.len(), 8 * in_dim);
    debug_assert_eq!(in_dim % 16, 0);

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

    reduce_8_epi32(sums)
}

/// `sum(inputs[i] * row[i])` for eight consecutive rows of a layer's row-major `i8` weights at once,
/// over unsigned 8-bit inputs: `dots_i16_8_avx2` at twice the multiply-adds an instruction.
/// `maddubs` adds two `u8 * i8` products into an `i16`, saturating; with inputs in `0..=127` a pair
/// reaches at most `2 * 127 * 127 = 32,258`, so it never saturates, and the sums are exact. A
/// multiple of 32 inputs.
pub(crate) fn dots_u8_i8_8_avx2(rows: &[i8], inputs: &[u8]) -> [i32; 8] {
    use std::arch::x86_64::{
        __m256i, _mm256_add_epi32, _mm256_loadu_si256, _mm256_madd_epi16, _mm256_maddubs_epi16, _mm256_set1_epi16,
        _mm256_setzero_si256,
    };

    let in_dim = inputs.len();
    debug_assert_eq!(rows.len(), 8 * in_dim);
    debug_assert_eq!(in_dim % 32, 0);
    debug_assert!(inputs.iter().all(|&input| input <= 127));

    let mut sums = [unsafe { _mm256_setzero_si256() }; 8];
    let ones = unsafe { _mm256_set1_epi16(1) };
    for chunk in 0..in_dim / 32 {
        let offset = chunk * 32;
        unsafe {
            let input = _mm256_loadu_si256(inputs.as_ptr().add(offset) as *const __m256i);
            for (row, sum) in sums.iter_mut().enumerate() {
                let weight = _mm256_loadu_si256(rows.as_ptr().add(row * in_dim + offset) as *const __m256i);
                let pairs = _mm256_maddubs_epi16(input, weight);
                *sum = _mm256_add_epi32(*sum, _mm256_madd_epi16(pairs, ones));
            }
        }
    }
    reduce_8_epi32(sums)
}

/// Eight registers of `i32` lanes reduced to their eight sums, in order: the same reduction as
/// `clipped_dot_products_8_avx2`'s, on `i32` lanes.
fn reduce_8_epi32(sums: [std::arch::x86_64::__m256i; 8]) -> [i32; 8] {
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

/// `values[i] = (biases[i] + sums[i] as f32 / denominators[i]).clamp(0.0, 1.0)`, eight lanes at a
/// time, bit for bit what the scalar expression gives: the conversion, division and addition are
/// each correctly rounded per lane, as the scalar ones are, and the clamp is `max(0, x)` then
/// `min(1, x)` with the constant first, so that a `-0.0` or a NaN passes through as it does in
/// `f32::clamp`. A multiple of 8 long.
pub(crate) fn hidden_values_avx2(sums: &[i32], denominators: &[f32], biases: &[f32], values: &mut [f32]) {
    use std::arch::x86_64::{
        __m256i, _mm256_add_ps, _mm256_cvtepi32_ps, _mm256_div_ps, _mm256_loadu_ps, _mm256_loadu_si256, _mm256_max_ps,
        _mm256_min_ps, _mm256_set1_ps, _mm256_setzero_ps, _mm256_storeu_ps,
    };

    debug_assert_eq!(sums.len() % 8, 0);
    debug_assert!(denominators.len() == sums.len() && biases.len() == sums.len() && values.len() == sums.len());
    for chunk in 0..sums.len() / 8 {
        let offset = chunk * 8;
        unsafe {
            let sum = _mm256_cvtepi32_ps(_mm256_loadu_si256(sums.as_ptr().add(offset) as *const __m256i));
            let quotient = _mm256_div_ps(sum, _mm256_loadu_ps(denominators.as_ptr().add(offset)));
            let value = _mm256_add_ps(_mm256_loadu_ps(biases.as_ptr().add(offset)), quotient);
            let clamped = _mm256_min_ps(_mm256_set1_ps(1.0), _mm256_max_ps(_mm256_setzero_ps(), value));
            _mm256_storeu_ps(values.as_mut_ptr().add(offset), clamped);
        }
    }
}

/// `destination[i] += source[i]`, vectorized. The feature transform is a gather -- summing one
/// weight row per active feature onto the accumulator -- so this, not a dot product, is the kernel
/// that layer needs. Lengths must match, same reasoning as `dot_product_avx2`.
pub(crate) fn add_assign_avx2(destination: &mut [f32], source: &[f32]) {
    use std::arch::x86_64::{_mm256_add_ps, _mm256_loadu_ps, _mm256_storeu_ps};

    debug_assert_eq!(destination.len(), source.len());

    let chunks = destination.len() / 8;
    for chunk in 0..chunks {
        let offset = chunk * 8;
        unsafe {
            let left = _mm256_loadu_ps(destination.as_ptr().add(offset));
            let right = _mm256_loadu_ps(source.as_ptr().add(offset));
            _mm256_storeu_ps(destination.as_mut_ptr().add(offset), _mm256_add_ps(left, right));
        }
    }
    for i in (chunks * 8)..destination.len() {
        destination[i] += source[i];
    }
}

/// `clamp(value, 0, 1)` in place, vectorized -- psq's clipped ReLU.
///
/// Worth vectorizing even though it is only one pass over the accumulator: written as scalar
/// comparisons it is a data-dependent branch, mispredicted often on real weights, where
/// `_mm256_max_ps`/`_mm256_min_ps` are branchless by construction.
pub(crate) fn clipped_relu_avx2(values: &mut [f32]) {
    use std::arch::x86_64::{_mm256_loadu_ps, _mm256_max_ps, _mm256_min_ps, _mm256_set1_ps, _mm256_setzero_ps, _mm256_storeu_ps};

    let zero = unsafe { _mm256_setzero_ps() };
    let one = unsafe { _mm256_set1_ps(1.0) };
    let chunks = values.len() / 8;
    for chunk in 0..chunks {
        let offset = chunk * 8;
        unsafe {
            let loaded = _mm256_loadu_ps(values.as_ptr().add(offset));
            _mm256_storeu_ps(values.as_mut_ptr().add(offset), _mm256_min_ps(_mm256_max_ps(loaded, zero), one));
        }
    }
    for value in values[(chunks * 8)..].iter_mut() {
        *value = value.clamp(0.0, 1.0);
    }
}

/// Reduces one `__m256` register's 8 lanes down to a single `f32` -- the standard AVX "hsum"
/// pattern, since there's no single instruction that sums a whole vector register directly: split
/// into two 128-bit halves and add them (`8 lanes -> 4`), then repeatedly shuffle-and-add within
/// the remaining 128 bits (`4 -> 2 -> 1`). Verified step by step against real intrinsic semantics
/// (Rust's own `std::arch` docs for `_mm256_extractf128_ps` and `_mm_movehl_ps` specifically)
/// rather than recalled from memory, since a mis-shuffled lane here would be exactly the kind of
/// bug that's easy to get subtly wrong and hard to catch by eye.
///
/// Worked through explicitly, `v = [v0..v7]`:
/// 1. `low = [v0,v1,v2,v3]` (a free reinterpretation of the low half, not a real instruction),
///    `high = [v4,v5,v6,v7]` (the actual upper-half extract). `sum4 = low+high` elementwise, so
///    `sum4 = [v0+v4, v1+v5, v2+v6, v3+v7]`.
/// 2. `odd = movehdup(sum4) = [sum4_1, sum4_1, sum4_3, sum4_3]` (duplicates each odd lane
///    downward). `sum2 = sum4+odd`, so `sum2[0] = sum4_0+sum4_1` and `sum2[2] = sum4_2+sum4_3`
///    (lanes 1 and 3 are unused garbage from here on).
/// 3. `movehl_ps(a, b)` returns `[b2, b3, a2, a3]` (confirmed against its own docs) -- so
///    `upper = movehl_ps(odd, sum2) = [sum2_2, sum2_3, odd_2, odd_3]`, putting `sum2[2]` (the
///    second partial sum) into lane 0.
/// 4. `add_ss` adds only lane 0, leaving the rest of the first operand untouched: `total[0] =
///    sum2[0] + upper[0] = (sum4_0+sum4_1) + (sum4_2+sum4_3)` -- the full sum of all 8 original
///    lanes. `cvtss_f32` reads that lane out as a plain `f32`.
fn horizontal_sum_avx2(v: __m256) -> f32 {
    use std::arch::x86_64::{
        _mm256_castps256_ps128, _mm256_extractf128_ps, _mm_add_ps, _mm_add_ss, _mm_cvtss_f32,
        _mm_movehdup_ps, _mm_movehl_ps,
    };

    unsafe {
        let low = _mm256_castps256_ps128(v);
        let high = _mm256_extractf128_ps(v, 1);
        let sum4 = _mm_add_ps(low, high);

        let odd = _mm_movehdup_ps(sum4);
        let sum2 = _mm_add_ps(sum4, odd);

        let upper = _mm_movehl_ps(odd, sum2);
        let total = _mm_add_ss(sum2, upper);
        _mm_cvtss_f32(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_product_avx2_matches_a_hand_computed_sum() {
        let a: Vec<f32> = (1..=9).map(|i| i as f32).collect();
        let b = vec![1.0f32; 9];
        assert_eq!(dot_product_avx2(&a, &b), 45.0); // 1+2+...+9
    }

    #[test]
    fn dot_product_avx2_handles_a_length_with_no_full_chunk() {
        let a: Vec<f32> = (1..=3).map(|i| i as f32).collect();
        let b = vec![2.0f32; 3];
        assert_eq!(dot_product_avx2(&a, &b), 12.0); // 1*2 + 2*2 + 3*2
    }

    #[test]
    fn dot_product_avx2_is_exact_at_every_length_across_the_block_and_chunk_boundaries() {
        // With `ACCUMULATORS` blocks of 8, a length can fall in three different places: whole
        // blocks, leftover whole 8-lane chunks, and a scalar tail. All three paths have to agree,
        // and the lengths where they change over are exactly where an off-by-one would hide. Values
        // are small integers so the sum is exact in `f32` whatever order it accumulates in.
        for length in 0..(8 * ACCUMULATORS * 3 + 9) {
            let a: Vec<f32> = (0..length).map(|i| (i % 7) as f32).collect();
            let b: Vec<f32> = (0..length).map(|i| (i % 3) as f32).collect();
            let expected: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
            assert_eq!(dot_product_avx2(&a, &b), expected, "length {length}");
        }
    }

    #[test]
    fn the_fused_clipped_pair_dot_product_is_bit_for_bit_the_dot_product_of_the_clipped_concatenation() {
        // Values spread across and beyond [0, 1], non-round, so both clamps fire and any change of
        // summation order would show in the last bits.
        for half in [DOT_PRODUCT_BLOCK, 4 * DOT_PRODUCT_BLOCK, 8 * DOT_PRODUCT_BLOCK] {
            let value = |i: usize, scale: f32| -> f32 { ((i as f32 * 0.618).fract() - 0.3) * scale };
            let own: Vec<f32> = (0..half).map(|i| value(i, 1.7)).collect();
            let opponent: Vec<f32> = (0..half).map(|i| value(i + 7, 2.3)).collect();
            let weights: Vec<f32> = (0..2 * half).map(|i| value(i + 3, 0.9)).collect();
            let mut concatenated: Vec<f32> = own.iter().chain(&opponent).copied().collect();
            clipped_relu_avx2(&mut concatenated);
            let expected = dot_product_avx2(&weights, &concatenated);
            assert_eq!(clipped_dot_product_pair_avx2(&own, &opponent, &weights).to_bits(), expected.to_bits(), "half {half}");
        }
    }

    #[test]
    fn eight_clipped_dot_products_at_once_match_a_scalar_reference() {
        // Each output on its own row of distinct values, so a reduction that put a sum in the wrong
        // lane, or mixed two registers, could not pass.
        for half in [8, 16, 256] {
            let value = |i: usize, scale: f32| -> f32 { ((i as f32 * 0.618).fract() - 0.3) * scale };
            let own: Vec<f32> = (0..half).map(|i| value(i, 1.7)).collect();
            let opponent: Vec<f32> = (0..half).map(|i| value(i + 7, 2.3)).collect();
            let rows: Vec<f32> = (0..8 * 2 * half).map(|i| value(i + 3, 0.9)).collect();
            let input: Vec<f32> = own.iter().chain(&opponent).map(|x| x.clamp(0.0, 1.0)).collect();
            let sums = clipped_dot_products_8_avx2(&rows, &own, &opponent);
            for output in 0..8 {
                let row = &rows[output * 2 * half..(output + 1) * 2 * half];
                let expected: f64 = row.iter().zip(&input).map(|(w, x)| *w as f64 * *x as f64).sum();
                assert!((sums[output] as f64 - expected).abs() <= 1e-4, "half {half}, output {output}: {} against {expected}", sums[output]);
            }
        }
    }

    #[test]
    fn clipped_copy_avx2_matches_a_copy_then_clipped_relu_at_every_length() {
        for length in 0..40 {
            let source: Vec<f32> = (0..length).map(|i| i as f32 / 10.0 - 1.5).collect();
            let mut expected = source.clone();
            clipped_relu_avx2(&mut expected);
            let mut destination = vec![9.0f32; length];
            clipped_copy_avx2(&mut destination, &source);
            assert_eq!(destination, expected, "length {length}");
        }
    }

    #[test]
    fn add_assign_avx2_adds_elementwise_at_every_length() {
        for length in 0..40 {
            let mut destination: Vec<f32> = (0..length).map(|i| i as f32).collect();
            let source: Vec<f32> = (0..length).map(|i| (2 * i) as f32).collect();
            add_assign_avx2(&mut destination, &source);
            for (i, value) in destination.iter().enumerate() {
                assert_eq!(*value, (3 * i) as f32, "length {length}, index {i}");
            }
        }
    }

    #[test]
    fn i16_add_is_elementwise_at_every_length() {
        for length in 0..40 {
            let original: Vec<i16> = (0..length).map(|i| i as i16 * 7 - 100).collect();
            let source: Vec<i16> = (0..length).map(|i| i as i16 * 3 - 50).collect();
            let mut destination = original.clone();
            add_assign_i16_avx2(&mut destination, &source);
            for i in 0..length {
                assert_eq!(destination[i], original[i] + source[i], "length {length}, index {i}");
            }
        }
    }

    #[test]
    fn i16_update_matches_scalar_sums_at_every_length() {
        // Lengths below, at and past a chunk and a block of four chunks (64 lanes), up to 256 and
        // past it, with no row, the quiet move's and the capture's special cases, and a general
        // count, so the blocks, the single chunks and the scalar tail all meet every case.
        for length in (0..80).chain([127, 128, 255, 256, 257, 300]) {
            let parent: Vec<i16> = (0..length).map(|i| (i % 300) as i16 * 7 - 1000).collect();
            let rows: Vec<i16> = (0..5 * length).map(|i| ((i * 31) % 200) as i16 - 100).collect();
            let cases: [(&[usize], &[usize]); 5] =
                [(&[], &[]), (&[1], &[0]), (&[2], &[0, 3]), (&[4, 2], &[0, 3, 1]), (&[3], &[])];
            for (added, removed) in cases {
                let mut child = vec![9i16; length];
                update_i16_avx2(&mut child, &parent, &rows, added, removed);
                for i in 0..length {
                    let mut expected = parent[i];
                    for &row in removed {
                        expected -= rows[row * length + i];
                    }
                    for &row in added {
                        expected += rows[row * length + i];
                    }
                    assert_eq!(child[i], expected, "length {length}, index {i}, added {added:?}, removed {removed:?}");
                }
            }
        }
    }

    #[test]
    fn the_integer_head_kernels_match_scalar_sums_exactly() {
        // Lanes below 0, inside, and above `one`, so the clamp, the rounding and the shift all
        // matter; eight rows of distinct weights, so a lane mixed up in the reduction shows.
        let activation = Activation { one: 127 << 2, rounding: 2, shift: 2 };
        let scalar_activation = |lane: i16| -> i32 { ((lane as i32).clamp(0, 127 << 2) + 2) >> 2 };
        for half in [16, 32, 256] {
            let own: Vec<i16> = (0..half).map(|i| ((i * 37) % 700) as i16 - 100).collect();
            let opponent: Vec<i16> = (0..half).map(|i| ((i * 53) % 650) as i16 - 80).collect();
            let rows: Vec<i16> = (0..8 * 2 * half).map(|i| ((i * 7919) % 65000) as i16).collect();
            let expected = |row: &[i16]| -> i32 {
                own.iter().chain(&opponent).zip(row).map(|(&lane, &weight)| scalar_activation(lane) * weight as i32).sum()
            };
            assert_eq!(activation_dot_i16_avx2(&own, &opponent, &rows[..2 * half], activation), expected(&rows[..2 * half]));
            let mut activations = vec![0i16; 2 * half];
            activations_i16_avx2(&mut activations[..half], &own, activation);
            activations_i16_avx2(&mut activations[half..], &opponent, activation);
            for (index, &lane) in own.iter().chain(&opponent).enumerate() {
                assert_eq!(activations[index] as i32, scalar_activation(lane), "half {half}, lane {index}");
            }
            let sums = dots_i16_8_avx2(&rows, &activations);
            for row in 0..8 {
                assert_eq!(sums[row], expected(&rows[row * 2 * half..(row + 1) * 2 * half]), "half {half}, row {row}");
            }
        }
    }

    #[test]
    fn the_u8_i8_kernel_matches_scalar_arithmetic_exactly() {
        // Inputs over the whole 0..=127 and weights over the whole i8 range, both extremes paired, so
        // that a saturating pair or a sign mistake would show; 32, 64 and 512 inputs.
        for in_dim in [32, 64, 512] {
            let inputs: Vec<u8> = (0..in_dim).map(|i| if i % 7 == 0 { 127 } else { ((i * 37) % 128) as u8 }).collect();
            let rows: Vec<i8> = (0..8 * in_dim).map(|i| if i % 5 == 0 { i8::MIN } else if i % 5 == 1 { i8::MAX } else { ((i * 7919) % 256) as u8 as i8 }).collect();
            let sums = dots_u8_i8_8_avx2(&rows, &inputs);
            for row in 0..8 {
                let expected: i32 = inputs.iter().zip(&rows[row * in_dim..(row + 1) * in_dim]).map(|(&x, &w)| x as i32 * w as i32).sum();
                assert_eq!(sums[row], expected, "in_dim {in_dim}, row {row}");
            }
        }
    }

    #[test]
    fn the_pairwise_kernels_match_scalar_arithmetic_exactly() {
        // Halves at 255 << 7, the widest shift a pairwise network can take: lanes below 0, at the
        // edges (0, one step, 254, 255 steps), above the clamp, and in between, paired every way.
        let activation = Activation { one: 255 << 7, rounding: 64, shift: 7 };
        let scalar_activation = |lane: i16| -> i32 { ((lane as i32).clamp(0, 255 << 7) + 64) >> 7 };
        let edges: [i16; 8] = [-32768, -1, 0, 128, 254 << 7, 255 << 7, 32767, 12_345];
        for half in [16, 32, 128] {
            let lanes: Vec<i16> = (0..2 * half).map(|i| edges[(i * 3 + i / 8) % 8]).collect();
            let mut products = vec![-1i16; half];
            pairwise_activations_i16_avx2(&mut products, &lanes, activation);
            for i in 0..half {
                let expected = (scalar_activation(lanes[i]) * scalar_activation(lanes[i + half]) + 256) >> 9;
                assert!((0..=127).contains(&expected), "a product beyond the head's range: {expected}");
                assert_eq!(products[i] as i32, expected, "half {half}, lane {i}");
            }
            // The fused head: two perspectives of `2 * half` lanes, the second shifted so the two differ.
            let opponent: Vec<i16> = (0..2 * half).map(|i| edges[(i * 5 + 1) % 8]).collect();
            let mut opponent_products = vec![-1i16; half];
            pairwise_activations_i16_avx2(&mut opponent_products, &opponent, activation);
            let weights: Vec<i16> = (0..2 * half).map(|i| ((i * 7919) % 65000) as i16).collect();
            let expected: i32 =
                products.iter().chain(&opponent_products).zip(&weights).map(|(&p, &w)| p as i32 * w as i32).sum();
            assert_eq!(pairwise_dot_i16_avx2(&lanes, &opponent, &weights, activation), expected, "half {half}");
        }
        // Both halves at 1.0: the largest product, 127.
        let full = vec![255i16 << 7; 32];
        let mut products = vec![0i16; 16];
        pairwise_activations_i16_avx2(&mut products, &full, activation);
        assert!(products.iter().all(|&p| p == 127), "{products:?}");
    }

    #[test]
    fn hidden_values_avx2_equal_the_scalar_arithmetic_bit_for_bit() {
        // Sums and biases that land below 0, inside, exactly on both ends and above 1, and a -0.0,
        // which a clamp written the other way round would turn into +0.0.
        let scalar = |bias: f32, sum: i32, denominator: f32| -> f32 { (bias + sum as f32 / denominator).clamp(0.0, 1.0) };
        let sums: Vec<i32> = vec![-5000, -1, 0, 0, 1, 127, 4064, 300_000, 12_345, -127, 254, 1_000_000, 77, 3, -2, 508];
        let biases: Vec<f32> = vec![0.1, -0.0, -0.0, 0.0, -0.5, 0.25, 0.0, 0.9, -0.3, 1.0, 0.5, -2.0, 0.01, 0.0, 0.0, -1.0];
        let denominators: Vec<f32> = (0..16).map(|i| 127.0 * (1.5 + i as f32 * 3.7)).collect();
        let mut values = vec![9.0f32; 16];
        hidden_values_avx2(&sums, &denominators, &biases, &mut values);
        for i in 0..16 {
            assert_eq!(values[i].to_bits(), scalar(biases[i], sums[i], denominators[i]).to_bits(), "lane {i}");
        }
    }

    #[test]
    fn clipped_relu_avx2_clamps_to_the_unit_interval_at_every_length() {
        for length in 0..40 {
            let mut values: Vec<f32> = (0..length).map(|i| i as f32 / 10.0 - 1.5).collect();
            clipped_relu_avx2(&mut values);
            for (i, value) in values.iter().enumerate() {
                assert_eq!(*value, (i as f32 / 10.0 - 1.5).clamp(0.0, 1.0), "length {length}, index {i}");
            }
        }
    }
}
