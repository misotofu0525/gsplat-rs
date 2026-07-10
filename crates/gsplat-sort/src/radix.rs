//! 8-bit descending radix sort for packed `(key, value)` pairs.
//!
//! Histogram fits in L1 (256 buckets). Count uses multi-histogram SIMD on
//! AArch64 NEON and x86_64 AVX2; scatter stays scalar.

pub const RADIX_SORT_BITS: usize = 8;
pub const RADIX_SORT_BUCKETS: usize = 1 << RADIX_SORT_BITS;
pub const RADIX_SORT_MASK: u64 = (RADIX_SORT_BUCKETS as u64) - 1;
const HIST_LANES: usize = 4;

pub fn radix_sort_desc_u64(values: &mut [u64], scratch: &mut [u64], counts: &mut [usize]) {
    radix_sort_desc_u64_passes(values, scratch, counts, 0..64)
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
) {
    radix_sort_desc_u64_passes(values, scratch, counts, 32..64)
}

fn radix_sort_desc_u64_passes(
    values: &mut [u64],
    scratch: &mut [u64],
    counts: &mut [usize],
    shifts: std::ops::Range<usize>,
) {
    debug_assert_eq!(values.len(), scratch.len());
    debug_assert_eq!(counts.len(), RADIX_SORT_BUCKETS);
    debug_assert!(shifts.start.is_multiple_of(RADIX_SORT_BITS));
    debug_assert!(shifts.end.is_multiple_of(RADIX_SORT_BITS));
    debug_assert!(shifts.end <= 64);

    let pass_count = (shifts.end - shifts.start) / RADIX_SORT_BITS;
    debug_assert!(pass_count > 0);

    let mut values_to_scratch = true;
    for shift in shifts.step_by(RADIX_SORT_BITS) {
        if values_to_scratch {
            count_radix_digits(values, shift, counts);
            descending_prefix_offsets(counts);
            scatter_radix_digits(values, scratch, shift, counts);
        } else {
            count_radix_digits(scratch, shift, counts);
            descending_prefix_offsets(counts);
            scatter_radix_digits(scratch, values, shift, counts);
        }
        values_to_scratch = !values_to_scratch;
    }

    // Odd pass counts leave the result in `scratch`; copy back.
    if !values_to_scratch {
        values.copy_from_slice(scratch);
    }
}

fn count_radix_digits(input: &[u64], shift: usize, counts: &mut [usize]) {
    debug_assert_eq!(counts.len(), RADIX_SORT_BUCKETS);
    debug_assert!(shift < 64 && shift.is_multiple_of(RADIX_SORT_BITS));

    #[cfg(target_arch = "aarch64")]
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

    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
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

#[cfg(target_arch = "aarch64")]
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
        RADIX_SORT_BUCKETS, count_radix_digits, count_radix_digits_scalar_for_test,
        radix_sort_desc_u64,
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
        radix_sort_desc_u64(&mut values, &mut scratch, &mut counts);
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
}
