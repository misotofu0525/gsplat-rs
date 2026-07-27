use crate::radix::{
    RADIX_PARALLEL_COUNT_SLOTS, RADIX_SORT_BUCKETS, radix_sort_desc_u64,
    radix_sort_desc_u64_key_bits,
};
use crate::{SortBackend, SortError};

#[derive(Default)]
pub struct CpuSortBackend {
    packed: Vec<u64>,
    scratch: Vec<u64>,
    counts: Vec<usize>,
    parallel_counts: Vec<usize>,
}

impl CpuSortBackend {
    /// Packs one key/value pair in the representation consumed by
    /// [`Self::sort_prepacked_values`].
    #[cfg(not(target_arch = "wasm32"))]
    #[inline]
    pub const fn pack_key_value(key: u32, value: u32) -> u64 {
        pack_sort_pair(key, value)
    }

    /// Stably orders `values` by the complete 32-bit keys, descending.
    ///
    /// Equal keys retain their input order. Renderer inputs enumerate source
    /// IDs in ascending order, so exact-depth ties remain source-ID ascending
    /// on both the serial and parallel radix paths.
    pub fn sort_values_by_keys(
        &mut self,
        keys: &[u32],
        values: &mut [u32],
    ) -> Result<(), SortError> {
        if keys.len() != values.len() {
            return Err(SortError::LengthMismatch);
        }

        let len = keys.len();
        if len <= 1 {
            return Ok(());
        }

        self.prepare_pair_scratch(len);
        let packed = &mut self.packed[..len];
        pack_pairs(keys, values, packed);
        // Production path packs ascending indices; key-bit-only stable radix is enough.
        radix_sort_desc_u64_key_bits(
            packed,
            &mut self.scratch[..len],
            &mut self.counts[..RADIX_SORT_BUCKETS],
            &mut self.parallel_counts[..RADIX_PARALLEL_COUNT_SLOTS],
        );
        unpack_values(packed, values);
        Ok(())
    }

    /// Stably orders already packed `(key, value)` pairs by the complete
    /// 32-bit key, descending, and writes the resulting values.
    ///
    /// The packed input must use [`Self::pack_key_value`] and enumerate equal
    /// keys in the required stable input order. This entry reuses the same
    /// high-key radix passes as [`Self::sort_values_by_keys`] without first
    /// copying split keys and values into another packed buffer.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn sort_prepacked_values(
        &mut self,
        packed: &mut [u64],
        values: &mut [u32],
    ) -> Result<(), SortError> {
        if packed.len() != values.len() {
            return Err(SortError::LengthMismatch);
        }

        let len = packed.len();
        if len > 1 {
            self.prepare_radix_scratch(len);
            radix_sort_desc_u64_key_bits(
                packed,
                &mut self.scratch[..len],
                &mut self.counts[..RADIX_SORT_BUCKETS],
                &mut self.parallel_counts[..RADIX_PARALLEL_COUNT_SLOTS],
            );
        }
        unpack_values(packed, values);
        Ok(())
    }

    fn prepare_pair_scratch(&mut self, len: usize) {
        if self.packed.len() < len {
            self.packed.resize(len, 0);
        }
        self.prepare_radix_scratch(len);
    }

    fn prepare_radix_scratch(&mut self, len: usize) {
        if self.scratch.len() < len {
            self.scratch.resize(len, 0);
        }
        if self.counts.len() < RADIX_SORT_BUCKETS {
            self.counts.resize(RADIX_SORT_BUCKETS, 0);
        }
        if self.parallel_counts.len() < RADIX_PARALLEL_COUNT_SLOTS {
            self.parallel_counts.resize(RADIX_PARALLEL_COUNT_SLOTS, 0);
        }
    }
}

impl SortBackend for CpuSortBackend {
    fn name(&self) -> &'static str {
        "cpu-fallback"
    }

    fn sort_pairs(&mut self, keys: &mut [u32], values: &mut [u32]) -> Result<(), SortError> {
        if keys.len() != values.len() {
            return Err(SortError::LengthMismatch);
        }

        let len = keys.len();
        if len <= 1 {
            return Ok(());
        }

        self.prepare_pair_scratch(len);
        let packed = &mut self.packed[..len];
        pack_pairs(keys, values, packed);
        radix_sort_desc_u64(
            packed,
            &mut self.scratch[..len],
            &mut self.counts[..RADIX_SORT_BUCKETS],
            &mut self.parallel_counts[..RADIX_PARALLEL_COUNT_SLOTS],
        );
        unpack_pairs(packed, keys, values);
        Ok(())
    }
}

#[inline]
const fn pack_sort_pair(key: u32, value: u32) -> u64 {
    ((key as u64) << 32) | ((!value) as u64)
}

#[inline]
const fn unpack_sort_pair(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, !(packed as u32))
}

fn pack_pairs(keys: &[u32], values: &[u32], out: &mut [u64]) {
    debug_assert_eq!(keys.len(), values.len());
    debug_assert_eq!(keys.len(), out.len());

    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: guarded by runtime AVX2 detection and length-validated slices.
            unsafe {
                pack_pairs_avx2(keys, values, out);
            }
            return;
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: AArch64 guarantees Neon availability.
        unsafe {
            pack_pairs_neon(keys, values, out);
        }
    }

    #[cfg(not(target_arch = "aarch64"))]
    for i in 0..keys.len() {
        out[i] = pack_sort_pair(keys[i], values[i]);
    }
}

fn unpack_pairs(packed: &[u64], keys: &mut [u32], values: &mut [u32]) {
    debug_assert_eq!(packed.len(), keys.len());
    debug_assert_eq!(packed.len(), values.len());

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: AArch64 guarantees Neon availability and slices are length-validated above.
        unsafe {
            unpack_pairs_neon(packed, keys, values);
        }
    }

    #[cfg(not(target_arch = "aarch64"))]
    for i in 0..packed.len() {
        let (key, value) = unpack_sort_pair(packed[i]);
        keys[i] = key;
        values[i] = value;
    }
}

fn unpack_values(packed: &[u64], values: &mut [u32]) {
    debug_assert_eq!(packed.len(), values.len());

    #[cfg(all(target_arch = "aarch64", not(feature = "qualification-q3-cpu-scalar")))]
    {
        // SAFETY: AArch64 guarantees Neon availability and slices are length-validated above.
        unsafe {
            unpack_values_neon(packed, values);
        }
    }

    #[cfg(any(not(target_arch = "aarch64"), feature = "qualification-q3-cpu-scalar"))]
    unpack_values_scalar(packed, values);
}

#[cfg(any(
    not(target_arch = "aarch64"),
    test,
    feature = "qualification-q3-cpu-scalar"
))]
fn unpack_values_scalar(packed: &[u64], values: &mut [u32]) {
    debug_assert_eq!(packed.len(), values.len());
    for i in 0..packed.len() {
        values[i] = !(packed[i] as u32);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn pack_pairs_avx2(keys: &[u32], values: &[u32], out: &mut [u64]) {
    use std::arch::x86_64::*;

    let len = keys.len();
    let mut i = 0_usize;
    let all_ones_u32 = _mm_set1_epi32(-1);

    while i + 4 <= len {
        // SAFETY: i + 4 <= len guarantees valid 16-byte loads.
        let key_128 = unsafe { _mm_loadu_si128(keys.as_ptr().add(i) as *const __m128i) };
        // SAFETY: i + 4 <= len guarantees valid 16-byte loads.
        let val_128 = unsafe { _mm_loadu_si128(values.as_ptr().add(i) as *const __m128i) };
        let inv_val_128 = _mm_xor_si128(val_128, all_ones_u32);

        let key_64 = _mm256_cvtepu32_epi64(key_128);
        let inv_val_64 = _mm256_cvtepu32_epi64(inv_val_128);
        let key_hi_64 = _mm256_slli_epi64(key_64, 32);
        let packed_64 = _mm256_or_si256(key_hi_64, inv_val_64);

        // SAFETY: i + 4 <= len guarantees valid 32-byte stores.
        unsafe { _mm256_storeu_si256(out.as_mut_ptr().add(i) as *mut __m256i, packed_64) };
        i += 4;
    }

    while i < len {
        out[i] = pack_sort_pair(keys[i], values[i]);
        i += 1;
    }
}

#[cfg(target_arch = "aarch64")]
#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
unsafe fn pack_pairs_neon(keys: &[u32], values: &[u32], out: &mut [u64]) {
    use std::arch::aarch64::*;

    let len = keys.len();
    let mut i = 0_usize;

    while i + 4 <= len {
        // SAFETY: i + 4 <= len guarantees valid loads/stores.
        let key_4 = unsafe { vld1q_u32(keys.as_ptr().add(i)) };
        // SAFETY: i + 4 <= len guarantees valid loads/stores.
        let val_4 = unsafe { vld1q_u32(values.as_ptr().add(i)) };
        let inv_val_4 = vmvnq_u32(val_4);

        let key_lo = vmovl_u32(vget_low_u32(key_4));
        let key_hi = vmovl_u32(vget_high_u32(key_4));
        let val_lo = vmovl_u32(vget_low_u32(inv_val_4));
        let val_hi = vmovl_u32(vget_high_u32(inv_val_4));

        let key_hi_lo = vshlq_n_u64(key_lo, 32);
        let key_hi_hi = vshlq_n_u64(key_hi, 32);
        let packed_lo = vorrq_u64(key_hi_lo, val_lo);
        let packed_hi = vorrq_u64(key_hi_hi, val_hi);

        // SAFETY: i + 4 <= len guarantees valid stores.
        unsafe { vst1q_u64(out.as_mut_ptr().add(i), packed_lo) };
        // SAFETY: i + 4 <= len guarantees valid stores.
        unsafe { vst1q_u64(out.as_mut_ptr().add(i + 2), packed_hi) };
        i += 4;
    }

    while i < len {
        out[i] = pack_sort_pair(keys[i], values[i]);
        i += 1;
    }
}

#[cfg(target_arch = "aarch64")]
#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
unsafe fn unpack_pairs_neon(packed: &[u64], keys: &mut [u32], values: &mut [u32]) {
    use std::arch::aarch64::*;

    let len = packed.len();
    let mut i = 0_usize;

    while i + 4 <= len {
        // SAFETY: i + 4 <= len guarantees valid loads/stores.
        let packed_lo = unsafe { vld1q_u64(packed.as_ptr().add(i)) };
        let packed_hi = unsafe { vld1q_u64(packed.as_ptr().add(i + 2)) };

        let key_lo = vmovn_u64(vshrq_n_u64(packed_lo, 32));
        let key_hi = vmovn_u64(vshrq_n_u64(packed_hi, 32));
        let key_4 = vcombine_u32(key_lo, key_hi);

        let val_lo = vmovn_u64(packed_lo);
        let val_hi = vmovn_u64(packed_hi);
        let value_4 = vmvnq_u32(vcombine_u32(val_lo, val_hi));

        unsafe { vst1q_u32(keys.as_mut_ptr().add(i), key_4) };
        unsafe { vst1q_u32(values.as_mut_ptr().add(i), value_4) };
        i += 4;
    }

    while i < len {
        let (key, value) = unpack_sort_pair(packed[i]);
        keys[i] = key;
        values[i] = value;
        i += 1;
    }
}

#[cfg(all(
    target_arch = "aarch64",
    any(test, not(feature = "qualification-q3-cpu-scalar"))
))]
#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
unsafe fn unpack_values_neon(packed: &[u64], values: &mut [u32]) {
    use std::arch::aarch64::*;

    let len = packed.len();
    let mut i = 0_usize;

    while i + 4 <= len {
        // SAFETY: i + 4 <= len guarantees valid loads/stores.
        let packed_lo = unsafe { vld1q_u64(packed.as_ptr().add(i)) };
        let packed_hi = unsafe { vld1q_u64(packed.as_ptr().add(i + 2)) };
        let val_lo = vmovn_u64(packed_lo);
        let val_hi = vmovn_u64(packed_hi);
        let value_4 = vmvnq_u32(vcombine_u32(val_lo, val_hi));

        unsafe { vst1q_u32(values.as_mut_ptr().add(i), value_4) };
        i += 4;
    }

    while i < len {
        values[i] = !(packed[i] as u32);
        i += 1;
    }
}

#[cfg(all(test, target_arch = "aarch64"))]
mod q3_m4;

#[cfg(all(test, target_arch = "aarch64"))]
mod q3_a065;

#[cfg(test)]
mod tests {
    use super::{CpuSortBackend, SortBackend, SortError};

    fn lcg_next(state: &mut u32) -> u32 {
        *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        *state
    }

    #[test]
    fn cpu_backend_sorts_descending_by_key() {
        let mut backend = CpuSortBackend::default();
        let mut keys = [20_u32, 5, 12];
        let mut values = [0_u32, 1, 2];

        backend.sort_pairs(&mut keys, &mut values).unwrap();

        assert_eq!(keys, [20, 12, 5]);
        assert_eq!(values, [0, 2, 1]);
    }

    #[test]
    fn cpu_backend_matches_reference_order_for_large_input() {
        let mut backend = CpuSortBackend::default();
        let len = 4099;
        let mut seed = 1_u32;

        let mut keys: Vec<u32> = (0..len).map(|_| lcg_next(&mut seed)).collect();
        let mut values: Vec<u32> = (0..len as u32).collect();

        let mut expected: Vec<(u32, u32)> =
            keys.iter().copied().zip(values.iter().copied()).collect();
        expected.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        backend.sort_pairs(&mut keys, &mut values).unwrap();

        let actual: Vec<(u32, u32)> = keys.into_iter().zip(values).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn cpu_backend_matches_reference_across_edge_lengths() {
        let mut backend = CpuSortBackend::default();
        for &len in &[0_usize, 1, 2, 255, 256, 257, 1024, 65537] {
            let mut seed = 42_u32.wrapping_add(len as u32);
            let mut keys: Vec<u32> = (0..len).map(|_| lcg_next(&mut seed)).collect();
            let mut values: Vec<u32> = (0..len as u32).collect();
            let mut expected: Vec<(u32, u32)> =
                keys.iter().copied().zip(values.iter().copied()).collect();
            expected.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            backend.sort_pairs(&mut keys, &mut values).unwrap();
            let actual: Vec<(u32, u32)> = keys.into_iter().zip(values).collect();
            assert_eq!(actual, expected, "len={len}");
        }
    }

    #[test]
    fn cpu_backend_parallel_key_sort_is_stable_for_nonsequential_values() {
        let len = 300_017_usize;
        let mut seed = 123_u32;
        let keys: Vec<u32> = (0..len).map(|_| lcg_next(&mut seed) & 1023).collect();
        let mut values: Vec<u32> = (0..len as u32).map(|index| index.reverse_bits()).collect();
        let mut expected: Vec<(u32, u32)> =
            keys.iter().copied().zip(values.iter().copied()).collect();
        // Stable key-only reference: equal depths retain input/source order.
        expected.sort_by(|a, b| b.0.cmp(&a.0));

        CpuSortBackend::default()
            .sort_values_by_keys(&keys, &mut values)
            .unwrap();

        let actual: Vec<(u32, u32)> = values
            .into_iter()
            .map(|value| {
                let input_index = value.reverse_bits() as usize;
                (keys[input_index], value)
            })
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn cpu_backend_parallel_pair_sort_matches_full_total_order() {
        let len = 300_017_usize;
        let mut seed = 456_u32;
        let mut keys: Vec<u32> = (0..len).map(|_| lcg_next(&mut seed) & 4095).collect();
        let mut values: Vec<u32> = (0..len as u32).map(|index| index.reverse_bits()).collect();
        let mut expected: Vec<(u32, u32)> =
            keys.iter().copied().zip(values.iter().copied()).collect();
        expected.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        CpuSortBackend::default()
            .sort_pairs(&mut keys, &mut values)
            .unwrap();

        let actual: Vec<(u32, u32)> = keys.into_iter().zip(values).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn cpu_backend_parallel_equal_key_tie_is_source_id_ascending() {
        let len = 300_017_usize;
        let mut keys = vec![0x7fc0_0001_u32; len];
        let mut values: Vec<u32> = (0..len as u32).rev().collect();

        CpuSortBackend::default()
            .sort_pairs(&mut keys, &mut values)
            .unwrap();

        assert!(keys.iter().all(|&key| key == 0x7fc0_0001));
        assert_eq!(values, (0..len as u32).collect::<Vec<_>>());
    }

    #[test]
    fn cpu_backend_sort_values_microbench_200k() {
        let len = 200_000_usize;
        let mut seed = 99_u32;
        let keys: Vec<u32> = (0..len).map(|_| lcg_next(&mut seed)).collect();
        let mut values: Vec<u32> = (0..len as u32).collect();
        let mut backend = CpuSortBackend::default();

        // Warmup
        backend.sort_values_by_keys(&keys, &mut values).unwrap();
        values = (0..len as u32).collect();

        let start = std::time::Instant::now();
        const ITERS: u32 = 8;
        for _ in 0..ITERS {
            values = (0..len as u32).collect();
            backend.sort_values_by_keys(&keys, &mut values).unwrap();
        }
        let avg_ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(ITERS);
        eprintln!("cpu_sort_values_by_keys n={len} avg_ms={avg_ms:.3}");
        assert!(avg_ms.is_finite());
        // Sanity: still descending by key.
        for window in values.windows(2) {
            let left = keys[window[0] as usize];
            let right = keys[window[1] as usize];
            assert!(left >= right);
        }
    }

    #[test]
    #[ignore = "manual native CPU radix crossover benchmark"]
    fn cpu_backend_sort_values_size_ladder_microbench() {
        const ITERS: usize = 9;
        for len in [
            200_000_usize,
            300_000,
            500_000,
            700_000,
            1_000_000,
            1_800_000,
        ] {
            let mut seed = 99_u32.wrapping_add(len as u32);
            let keys: Vec<u32> = (0..len).map(|_| lcg_next(&mut seed)).collect();
            let mut values: Vec<u32> = (0..len as u32).collect();
            let mut backend = CpuSortBackend::default();
            backend.sort_values_by_keys(&keys, &mut values).unwrap();

            let mut samples_ms = [0.0_f64; ITERS];
            for sample in &mut samples_ms {
                for (index, value) in values.iter_mut().enumerate() {
                    *value = index as u32;
                }
                let started = std::time::Instant::now();
                backend.sort_values_by_keys(&keys, &mut values).unwrap();
                *sample = started.elapsed().as_secs_f64() * 1000.0;
            }
            samples_ms.sort_by(f64::total_cmp);
            eprintln!(
                "cpu_sort_values_by_keys_ladder n={len} median_ms={:.3}",
                samples_ms[ITERS / 2]
            );
        }
    }

    #[test]
    fn cpu_backend_sorts_values_by_immutable_keys() {
        let mut backend = CpuSortBackend::default();
        let keys = [20_u32, 5, 12, 12];
        let mut values = [0_u32, 1, 2, 3];

        backend.sort_values_by_keys(&keys, &mut values).unwrap();

        assert_eq!(keys, [20, 5, 12, 12]);
        assert_eq!(values, [0, 2, 3, 1]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cpu_backend_sorts_prepacked_values_with_stable_source_ties() {
        let keys = [20_u32, 12, 12, 5];
        let source_ids = [7_u32, 11, 13, 17];
        let mut packed = keys
            .into_iter()
            .zip(source_ids)
            .map(|(key, value)| CpuSortBackend::pack_key_value(key, value))
            .collect::<Vec<_>>();
        let mut values = vec![u32::MAX; packed.len()];

        CpuSortBackend::default()
            .sort_prepacked_values(&mut packed, &mut values)
            .unwrap();

        assert_eq!(values, [7, 11, 13, 17]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cpu_backend_prepacked_entry_matches_split_entry_across_edge_lengths() {
        let mut seed = 0x5eed_u32;
        for &len in &[0_usize, 1, 2, 255, 256, 257, 65_537, 300_017] {
            let keys = (0..len)
                .map(|_| lcg_next(&mut seed) & 4095)
                .collect::<Vec<_>>();
            let source_ids = (0..len as u32).collect::<Vec<_>>();
            let mut expected = source_ids.clone();
            CpuSortBackend::default()
                .sort_values_by_keys(&keys, &mut expected)
                .unwrap();

            let mut packed = keys
                .iter()
                .copied()
                .zip(source_ids)
                .map(|(key, value)| CpuSortBackend::pack_key_value(key, value))
                .collect::<Vec<_>>();
            let mut actual = vec![u32::MAX; len];
            CpuSortBackend::default()
                .sort_prepacked_values(&mut packed, &mut actual)
                .unwrap();

            assert_eq!(actual, expected, "len={len}");
        }
    }

    #[test]
    fn cpu_backend_keeps_empty_and_singleton_inputs() {
        let mut backend = CpuSortBackend::default();
        let mut empty_keys: [u32; 0] = [];
        let mut empty_values: [u32; 0] = [];
        backend
            .sort_pairs(&mut empty_keys, &mut empty_values)
            .unwrap();

        let mut singleton_keys = [42_u32];
        let mut singleton_values = [7_u32];
        backend
            .sort_pairs(&mut singleton_keys, &mut singleton_values)
            .unwrap();

        assert_eq!(singleton_keys, [42]);
        assert_eq!(singleton_values, [7]);
    }

    #[test]
    fn cpu_backend_rejects_value_sort_mismatch() {
        let mut backend = CpuSortBackend::default();
        let keys = [1_u32, 2];
        let mut values = [1_u32];

        let err = backend.sort_values_by_keys(&keys, &mut values).unwrap_err();
        assert_eq!(err, SortError::LengthMismatch);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cpu_backend_rejects_prepacked_value_mismatch() {
        let mut packed = [
            CpuSortBackend::pack_key_value(1, 0),
            CpuSortBackend::pack_key_value(2, 1),
        ];
        let mut values = [u32::MAX];

        let err = CpuSortBackend::default()
            .sort_prepacked_values(&mut packed, &mut values)
            .unwrap_err();
        assert_eq!(err, SortError::LengthMismatch);
    }

    #[test]
    fn packed_pairs_match_scalar_reference() {
        let len = 257;
        let mut seed = 7_u32;
        let keys: Vec<u32> = (0..len).map(|_| lcg_next(&mut seed)).collect();
        let values: Vec<u32> = (0..len).map(|_| lcg_next(&mut seed)).collect();
        let mut packed = vec![0_u64; len];

        super::pack_pairs(&keys, &values, &mut packed);
        let expected: Vec<u64> = keys
            .iter()
            .copied()
            .zip(values.iter().copied())
            .map(|(key, value)| super::pack_sort_pair(key, value))
            .collect();
        assert_eq!(packed, expected);
    }

    #[test]
    fn cpu_backend_rejects_mismatch() {
        let mut backend = CpuSortBackend::default();
        let mut keys = [1_u32, 2];
        let mut values = [1_u32];

        let err = backend.sort_pairs(&mut keys, &mut values).unwrap_err();
        assert_eq!(err, SortError::LengthMismatch);
    }
}
