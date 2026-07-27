//! 8-bit descending radix sort for packed `(key, value)` pairs.
//!
//! Histogram fits in L1 (256 buckets). Count uses multi-histogram SIMD on
//! AArch64 NEON and x86_64 AVX2 for small inputs. Large native inputs use a
//! bounded stable parallel pass: per-chunk histograms, bucket/chunk prefix
//! offsets, then disjoint parallel scatter.

#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

pub const RADIX_SORT_BITS: usize = 8;
pub const RADIX_SORT_BUCKETS: usize = 1 << RADIX_SORT_BITS;
pub const RADIX_SORT_MASK: u64 = (RADIX_SORT_BUCKETS as u64) - 1;
/// Four chunks saturate the memory-bound pass on current desktop/mobile CPUs
/// without creating one task (and one histogram) per logical core.
const MAX_PARALLEL_CHUNKS: usize = 4;
/// Below this size, Rayon dispatch costs more than the stable scatter saves.
#[cfg(not(target_arch = "wasm32"))]
const PARALLEL_SORT_THRESHOLD: usize = 256 * 1024;
pub const RADIX_PARALLEL_COUNT_SLOTS: usize = MAX_PARALLEL_CHUNKS * RADIX_SORT_BUCKETS;
#[cfg(any(
    target_arch = "x86_64",
    all(
        target_arch = "aarch64",
        any(test, not(feature = "qualification-q3-cpu-scalar"))
    )
))]
const HIST_LANES: usize = 4;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
struct DisjointOutput(*mut u64);

// SAFETY: `DisjointOutput` is private and is shared only while a radix prefix
// assigns every worker a disjoint range. It never grants reads or references.
#[cfg(not(target_arch = "wasm32"))]
unsafe impl Send for DisjointOutput {}
// SAFETY: see the `Send` implementation above.
#[cfg(not(target_arch = "wasm32"))]
unsafe impl Sync for DisjointOutput {}

#[cfg(not(target_arch = "wasm32"))]
impl DisjointOutput {
    /// # Safety
    ///
    /// `index` must be in bounds and unique among all concurrent calls.
    unsafe fn write_unique(self, index: usize, value: u64) {
        // SAFETY: required by this method's caller contract.
        unsafe { self.0.add(index).write(value) };
    }
}

pub fn radix_sort_desc_u64(
    values: &mut [u64],
    scratch: &mut [u64],
    counts: &mut [usize],
    parallel_counts: &mut [usize],
) {
    radix_sort_desc_u64_passes(values, scratch, counts, parallel_counts, 0..64)
}

/// Stable descending sort on the high 32-bit key only.
///
/// Packed pairs are `key << 32 | !index`. LSD radix is stable, so skipping the
/// low 32 bits preserves ascending index order among equal keys when the input
/// was packed in ascending-index order (the `sort_values_by_keys` production
/// path).
pub fn radix_sort_desc_u64_key_bits(
    values: &mut [u64],
    scratch: &mut [u64],
    counts: &mut [usize],
    parallel_counts: &mut [usize],
) {
    radix_sort_desc_u64_passes(values, scratch, counts, parallel_counts, 32..64)
}

fn radix_sort_desc_u64_passes(
    values: &mut [u64],
    scratch: &mut [u64],
    counts: &mut [usize],
    parallel_counts: &mut [usize],
    shifts: std::ops::Range<usize>,
) {
    debug_assert_eq!(values.len(), scratch.len());
    debug_assert_eq!(counts.len(), RADIX_SORT_BUCKETS);
    debug_assert_eq!(parallel_counts.len(), RADIX_PARALLEL_COUNT_SLOTS);
    debug_assert!(shifts.start.is_multiple_of(RADIX_SORT_BITS));
    debug_assert!(shifts.end.is_multiple_of(RADIX_SORT_BITS));
    debug_assert!(shifts.end <= 64);

    let pass_count = (shifts.end - shifts.start) / RADIX_SORT_BITS;
    debug_assert!(pass_count > 0);

    let mut values_to_scratch = true;
    for shift in shifts.step_by(RADIX_SORT_BITS) {
        if values_to_scratch {
            count_and_scatter_radix_digits(values, scratch, shift, counts, parallel_counts);
        } else {
            count_and_scatter_radix_digits(scratch, values, shift, counts, parallel_counts);
        }
        values_to_scratch = !values_to_scratch;
    }

    // Odd pass counts leave the result in `scratch`; copy back.
    if !values_to_scratch {
        values.copy_from_slice(scratch);
    }
}

fn count_and_scatter_radix_digits(
    input: &[u64],
    output: &mut [u64],
    shift: usize,
    counts: &mut [usize],
    parallel_counts: &mut [usize],
) {
    debug_assert_eq!(input.len(), output.len());

    #[cfg(target_arch = "wasm32")]
    let _ = parallel_counts;

    #[cfg(not(target_arch = "wasm32"))]
    if let Some(chunk_count) = parallel_chunk_count(input.len()) {
        count_and_scatter_radix_digits_parallel(
            input,
            output,
            shift,
            counts,
            parallel_counts,
            chunk_count,
        );
        return;
    }

    count_radix_digits(input, shift, counts);
    descending_prefix_offsets(counts);
    scatter_radix_digits(input, output, shift, counts);
}

#[cfg(not(target_arch = "wasm32"))]
fn parallel_chunk_count(len: usize) -> Option<usize> {
    if len < PARALLEL_SORT_THRESHOLD {
        return None;
    }
    let available = rayon::current_num_threads().min(MAX_PARALLEL_CHUNKS);
    (available >= 2).then_some(available)
}

#[cfg(not(target_arch = "wasm32"))]
fn count_and_scatter_radix_digits_parallel(
    input: &[u64],
    output: &mut [u64],
    shift: usize,
    counts: &mut [usize],
    parallel_counts: &mut [usize],
    requested_chunk_count: usize,
) {
    debug_assert!((2..=MAX_PARALLEL_CHUNKS).contains(&requested_chunk_count));
    let chunk_len = input.len().div_ceil(requested_chunk_count);
    let actual_chunk_count = input.len().div_ceil(chunk_len);
    debug_assert!(actual_chunk_count <= requested_chunk_count);
    let local_counts = &mut parallel_counts[..actual_chunk_count * RADIX_SORT_BUCKETS];

    local_counts
        .par_chunks_mut(RADIX_SORT_BUCKETS)
        .zip(input.par_chunks(chunk_len))
        .for_each(|(histogram, chunk)| count_radix_digits(chunk, shift, histogram));

    counts.fill(0);
    for histogram in local_counts.chunks_exact(RADIX_SORT_BUCKETS) {
        for (total, &count) in counts.iter_mut().zip(histogram) {
            *total += count;
        }
    }
    descending_prefix_offsets(counts);

    // Each bucket reserves input-ordered, non-overlapping spans for every
    // chunk. Sequential scatter inside a chunk plus chunk-ordered spans makes
    // this exactly the same stable LSD pass as the scalar implementation.
    for (digit, &bucket_start) in counts.iter().enumerate() {
        let mut offset = bucket_start;
        for histogram in local_counts.chunks_exact_mut(RADIX_SORT_BUCKETS) {
            let chunk_count = histogram[digit];
            histogram[digit] = offset;
            offset += chunk_count;
        }
        let expected_end = if digit == 0 {
            input.len()
        } else {
            counts[digit - 1]
        };
        debug_assert_eq!(offset, expected_end);
    }

    // Rayon cannot express disjoint bucket spans as ordinary mutable slices.
    // The prefix construction above assigns every input element one unique
    // in-bounds output slot, so sharing only the pointer value between tasks
    // is sound: no two tasks read or write the same output element.
    let output_ptr = DisjointOutput(output.as_mut_ptr());
    input
        .par_chunks(chunk_len)
        .zip(local_counts.par_chunks_mut(RADIX_SORT_BUCKETS))
        .for_each(|(chunk, offsets)| {
            for &value in chunk {
                let digit = ((value >> shift) & RADIX_SORT_MASK) as usize;
                let output_index = offsets[digit];
                // SAFETY: per-bucket/per-chunk prefix spans are disjoint,
                // complete, and bounded by `output.len()` as proved above.
                unsafe { output_ptr.write_unique(output_index, value) };
                offsets[digit] = output_index + 1;
            }
        });
}

fn count_radix_digits(input: &[u64], shift: usize, counts: &mut [usize]) {
    debug_assert_eq!(counts.len(), RADIX_SORT_BUCKETS);
    debug_assert!(shift < 64 && shift.is_multiple_of(RADIX_SORT_BITS));

    #[cfg(all(target_arch = "aarch64", not(feature = "qualification-q3-cpu-scalar")))]
    {
        // SAFETY: AArch64 guarantees Neon; slices are length-validated by callers.
        unsafe {
            count_histograms_neon(input, shift, counts);
        }
    }

    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: guarded by runtime AVX2 detection.
            unsafe {
                count_histograms_avx2(input, shift, counts);
            }
        } else {
            count_histograms_scalar(input, shift, counts);
        }
    }

    #[cfg(any(
        all(target_arch = "aarch64", feature = "qualification-q3-cpu-scalar"),
        not(any(target_arch = "aarch64", target_arch = "x86_64"))
    ))]
    {
        count_histograms_scalar(input, shift, counts);
    }
}

/// Portable scalar histogram. Used as fallback on non-NEON/AVX2 hosts and as
/// the test oracle for SIMD count paths.
#[cfg_attr(all(target_arch = "aarch64", not(test)), allow(dead_code))]
fn count_histograms_scalar(input: &[u64], shift: usize, counts: &mut [usize]) {
    counts.fill(0);
    for &value in input {
        counts[((value >> shift) & RADIX_SORT_MASK) as usize] += 1;
    }
}

#[cfg(any(
    target_arch = "x86_64",
    all(
        target_arch = "aarch64",
        any(test, not(feature = "qualification-q3-cpu-scalar"))
    )
))]
fn merge_histograms(hist: &[[u32; RADIX_SORT_BUCKETS]; HIST_LANES], counts: &mut [usize]) {
    for digit in 0..RADIX_SORT_BUCKETS {
        let mut sum = 0_usize;
        for lane in hist {
            sum += lane[digit] as usize;
        }
        counts[digit] = sum;
    }
}

fn descending_prefix_offsets(counts: &mut [usize]) {
    let mut offset = 0_usize;
    for count in counts.iter_mut().rev() {
        let bucket_len = *count;
        *count = offset;
        offset += bucket_len;
    }
}

fn scatter_radix_digits(input: &[u64], output: &mut [u64], shift: usize, offsets: &mut [usize]) {
    for &value in input {
        let digit = ((value >> shift) & RADIX_SORT_MASK) as usize;
        let output_index = offsets[digit];
        output[output_index] = value;
        offsets[digit] = output_index + 1;
    }
}

/// Scalar reference used by tests to validate SIMD count paths.
#[cfg(test)]
pub(crate) fn count_radix_digits_scalar_for_test(
    input: &[u64],
    shift: usize,
    counts: &mut [usize],
) {
    count_histograms_scalar(input, shift, counts);
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn radix_sort_desc_u64_key_bits_with_counter_for_test(
    values: &mut [u64],
    scratch: &mut [u64],
    counts: &mut [usize],
    parallel_counts: &mut [usize],
    count: impl Fn(&[u64], usize, &mut [usize]),
) {
    debug_assert_eq!(values.len(), scratch.len());
    debug_assert_eq!(counts.len(), RADIX_SORT_BUCKETS);
    debug_assert_eq!(parallel_counts.len(), RADIX_PARALLEL_COUNT_SLOTS);
    // The Q3 fixed microbenchmark is intentionally below the production
    // parallel threshold so it isolates the selected histogram kernel.
    debug_assert!(values.len() < PARALLEL_SORT_THRESHOLD);

    let mut values_to_scratch = true;
    for shift in (32..64).step_by(RADIX_SORT_BITS) {
        let (input, output) = if values_to_scratch {
            (&*values, &mut *scratch)
        } else {
            (&*scratch, &mut *values)
        };
        count(input, shift, counts);
        descending_prefix_offsets(counts);
        scatter_radix_digits(input, output, shift, counts);
        values_to_scratch = !values_to_scratch;
    }
    debug_assert!(values_to_scratch, "four key passes finish in values");
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn radix_sort_desc_u64_key_bits_scalar_for_test(
    values: &mut [u64],
    scratch: &mut [u64],
    counts: &mut [usize],
    parallel_counts: &mut [usize],
) {
    radix_sort_desc_u64_key_bits_with_counter_for_test(
        values,
        scratch,
        counts,
        parallel_counts,
        count_histograms_scalar,
    );
}

#[cfg(all(test, target_arch = "aarch64"))]
pub(crate) fn radix_sort_desc_u64_key_bits_neon_for_test(
    values: &mut [u64],
    scratch: &mut [u64],
    counts: &mut [usize],
    parallel_counts: &mut [usize],
) {
    radix_sort_desc_u64_key_bits_with_counter_for_test(
        values,
        scratch,
        counts,
        parallel_counts,
        |input, shift, counts| {
            // SAFETY: this helper is compiled only for AArch64, where Neon is guaranteed.
            unsafe { count_histograms_neon(input, shift, counts) };
        },
    );
}

#[cfg(all(
    target_arch = "aarch64",
    any(test, not(feature = "qualification-q3-cpu-scalar"))
))]
#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
unsafe fn count_histograms_neon(input: &[u64], shift: usize, counts: &mut [usize]) {
    use std::arch::aarch64::*;

    let mut hist = [[0_u32; RADIX_SORT_BUCKETS]; HIST_LANES];
    let mut i = 0_usize;
    let shift_vec = vdupq_n_s64(-(shift as i64));
    let mask_vec = vdupq_n_u64(RADIX_SORT_MASK);

    while i + 4 <= input.len() {
        let packed_lo = unsafe { vld1q_u64(input.as_ptr().add(i)) };
        let packed_hi = unsafe { vld1q_u64(input.as_ptr().add(i + 2)) };
        let shifted_lo = vshlq_u64(packed_lo, shift_vec);
        let shifted_hi = vshlq_u64(packed_hi, shift_vec);
        let digit_lo = vandq_u64(shifted_lo, mask_vec);
        let digit_hi = vandq_u64(shifted_hi, mask_vec);

        let mut digits = [0_u64; 4];
        unsafe { vst1q_u64(digits.as_mut_ptr(), digit_lo) };
        unsafe { vst1q_u64(digits.as_mut_ptr().add(2), digit_hi) };

        hist[0][digits[0] as usize] += 1;
        hist[1][digits[1] as usize] += 1;
        hist[2][digits[2] as usize] += 1;
        hist[3][digits[3] as usize] += 1;
        i += 4;
    }

    while i < input.len() {
        hist[0][((input[i] >> shift) & RADIX_SORT_MASK) as usize] += 1;
        i += 1;
    }

    merge_histograms(&hist, counts);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn count_histograms_avx2(input: &[u64], shift: usize, counts: &mut [usize]) {
    use std::arch::x86_64::*;

    let mut hist = [[0_u32; RADIX_SORT_BUCKETS]; HIST_LANES];
    let mut i = 0_usize;
    let mask = _mm256_set1_epi64x(RADIX_SORT_MASK as i64);

    while i + 4 <= input.len() {
        // SAFETY: i + 4 <= len guarantees a valid 32-byte load.
        let packed = unsafe { _mm256_loadu_si256(input.as_ptr().add(i) as *const __m256i) };
        // `_mm256_srli_epi64` requires an immediate shift count.
        let shifted = match shift {
            0 => packed,
            8 => _mm256_srli_epi64(packed, 8),
            16 => _mm256_srli_epi64(packed, 16),
            24 => _mm256_srli_epi64(packed, 24),
            32 => _mm256_srli_epi64(packed, 32),
            40 => _mm256_srli_epi64(packed, 40),
            48 => _mm256_srli_epi64(packed, 48),
            56 => _mm256_srli_epi64(packed, 56),
            _ => unreachable!("radix8 shift must be 0..=56 step 8"),
        };
        let digits_vec = _mm256_and_si256(shifted, mask);

        let mut digits = [0_u64; 4];
        // SAFETY: digits is 32 bytes.
        unsafe { _mm256_storeu_si256(digits.as_mut_ptr() as *mut __m256i, digits_vec) };

        hist[0][digits[0] as usize] += 1;
        hist[1][digits[1] as usize] += 1;
        hist[2][digits[2] as usize] += 1;
        hist[3][digits[3] as usize] += 1;
        i += 4;
    }

    while i < input.len() {
        hist[0][((input[i] >> shift) & RADIX_SORT_MASK) as usize] += 1;
        i += 1;
    }

    merge_histograms(&hist, counts);
}

#[cfg(test)]
mod tests {
    use super::{
        RADIX_PARALLEL_COUNT_SLOTS, RADIX_SORT_BUCKETS, count_radix_digits,
        count_radix_digits_scalar_for_test, radix_sort_desc_u64,
    };
    #[cfg(not(target_arch = "wasm32"))]
    use super::{
        count_and_scatter_radix_digits_parallel, descending_prefix_offsets, scatter_radix_digits,
    };

    fn lcg_next(state: &mut u32) -> u32 {
        *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        *state
    }

    #[test]
    fn radix8_sorts_u64_descending() {
        let mut values = [3_u64, 1, 4, 1, 5, 9, 2, 6];
        let mut scratch = [0_u64; 8];
        let mut counts = [0_usize; RADIX_SORT_BUCKETS];
        let mut parallel_counts = [0_usize; RADIX_PARALLEL_COUNT_SLOTS];
        radix_sort_desc_u64(&mut values, &mut scratch, &mut counts, &mut parallel_counts);
        let mut expected = [3_u64, 1, 4, 1, 5, 9, 2, 6];
        expected.sort_by(|a, b| b.cmp(a));
        assert_eq!(values, expected);
    }

    #[test]
    fn count_matches_scalar_reference() {
        let len = 10_003;
        let mut seed = 11_u32;
        let input: Vec<u64> = (0..len)
            .map(|_| {
                let hi = lcg_next(&mut seed) as u64;
                let lo = lcg_next(&mut seed) as u64;
                (hi << 32) | lo
            })
            .collect();

        for shift in (0..64).step_by(8) {
            let mut simd_counts = [0_usize; RADIX_SORT_BUCKETS];
            let mut scalar_counts = [0_usize; RADIX_SORT_BUCKETS];
            count_radix_digits(&input, shift, &mut simd_counts);
            count_radix_digits_scalar_for_test(&input, shift, &mut scalar_counts);
            assert_eq!(simd_counts, scalar_counts, "shift={shift}");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn stable_parallel_pass_matches_scalar_for_every_digit() {
        let len = 10_003;
        let mut seed = 17_u32;
        let input: Vec<u64> = (0..len)
            .map(|index| {
                // Deliberately keep only 32 distinct high keys so equal-key
                // stability crosses every chunk boundary.
                let key = (lcg_next(&mut seed) & 31) as u64;
                (key << 32) | index as u64
            })
            .collect();

        for shift in (0..64).step_by(8) {
            let mut expected = vec![0_u64; len];
            let mut expected_counts = [0_usize; RADIX_SORT_BUCKETS];
            count_radix_digits_scalar_for_test(&input, shift, &mut expected_counts);
            descending_prefix_offsets(&mut expected_counts);
            scatter_radix_digits(&input, &mut expected, shift, &mut expected_counts);

            let mut actual = vec![0_u64; len];
            let mut actual_counts = [0_usize; RADIX_SORT_BUCKETS];
            let mut parallel_counts = [0_usize; RADIX_PARALLEL_COUNT_SLOTS];
            count_and_scatter_radix_digits_parallel(
                &input,
                &mut actual,
                shift,
                &mut actual_counts,
                &mut parallel_counts,
                4,
            );
            assert_eq!(actual, expected, "shift={shift}");
        }
    }
}
