use std::fs;
use std::hint::black_box;
use std::time::Instant;

use super::{CpuSortBackend, SortError, pack_sort_pair, unpack_values_neon, unpack_values_scalar};
use crate::radix::{
    RADIX_PARALLEL_COUNT_SLOTS, RADIX_SORT_BUCKETS, radix_sort_desc_u64_key_bits_neon_for_test,
    radix_sort_desc_u64_key_bits_scalar_for_test,
};

const INPUT_ID: &str = "q3-packed-exact-lcg-v1-200003";
const INPUT_LEN: usize = 200_003;
const WARMUP_PAIRS: usize = 3;
const SAMPLE_PAIRS: usize = 11;

#[derive(Clone, Copy)]
enum ForcedKernel {
    Scalar,
    Neon,
}

#[derive(Default)]
struct Parity {
    key: bool,
    source_id: bool,
    nan: bool,
    boundary: bool,
    fma: bool,
    stable_tie: bool,
}

impl Parity {
    fn all(&self) -> bool {
        self.key && self.source_id && self.nan && self.boundary && self.fma && self.stable_tie
    }
}

fn lcg_next(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    *state
}

fn sort_prepacked_forced(
    backend: &mut CpuSortBackend,
    packed: &mut [u64],
    values: &mut [u32],
    kernel: ForcedKernel,
) -> Result<(), SortError> {
    if packed.len() != values.len() {
        return Err(SortError::LengthMismatch);
    }

    let len = packed.len();
    if len > 1 {
        backend.prepare_radix_scratch(len);
        match kernel {
            ForcedKernel::Scalar => radix_sort_desc_u64_key_bits_scalar_for_test(
                packed,
                &mut backend.scratch[..len],
                &mut backend.counts[..RADIX_SORT_BUCKETS],
                &mut backend.parallel_counts[..RADIX_PARALLEL_COUNT_SLOTS],
            ),
            ForcedKernel::Neon => radix_sort_desc_u64_key_bits_neon_for_test(
                packed,
                &mut backend.scratch[..len],
                &mut backend.counts[..RADIX_SORT_BUCKETS],
                &mut backend.parallel_counts[..RADIX_PARALLEL_COUNT_SLOTS],
            ),
        }
    }
    match kernel {
        ForcedKernel::Scalar => unpack_values_scalar(packed, values),
        ForcedKernel::Neon => {
            // SAFETY: this module is compiled only for AArch64, where Neon is guaranteed.
            unsafe { unpack_values_neon(packed, values) };
        }
    }
    Ok(())
}

fn parity_case(keys: &[u32], source_ids: &[u32]) -> bool {
    let mut expected = keys
        .iter()
        .copied()
        .zip(source_ids.iter().copied())
        .collect::<Vec<_>>();
    expected.sort_by(|left, right| right.0.cmp(&left.0));
    let expected = expected
        .into_iter()
        .map(|(_, source_id)| source_id)
        .collect::<Vec<_>>();

    let packed = keys
        .iter()
        .copied()
        .zip(source_ids.iter().copied())
        .map(|(key, source_id)| pack_sort_pair(key, source_id))
        .collect::<Vec<_>>();
    let mut scalar_packed = packed.clone();
    let mut neon_packed = packed;
    let mut scalar = vec![u32::MAX; source_ids.len()];
    let mut neon = vec![u32::MAX; source_ids.len()];
    sort_prepacked_forced(
        &mut CpuSortBackend::default(),
        &mut scalar_packed,
        &mut scalar,
        ForcedKernel::Scalar,
    )
    .expect("forced Scalar sort");
    sort_prepacked_forced(
        &mut CpuSortBackend::default(),
        &mut neon_packed,
        &mut neon,
        ForcedKernel::Neon,
    )
    .expect("forced Neon sort");
    scalar == expected && neon == scalar
}

fn verify_parity() -> Parity {
    let mut seed = 0x5133_d00d_u32;
    let keys = (0..4_099).map(|_| lcg_next(&mut seed)).collect::<Vec<_>>();
    let key_case_ids = (0..keys.len())
        .map(|index| (index as u32).reverse_bits())
        .collect::<Vec<_>>();

    let source_keys = [9_u32, 2, 9, 7, 2, 9, 0, u32::MAX, 7, 7, 2];
    let source_case_ids = [u32::MAX, 17, 3, 0, 91, 42, 8, 5, 77, 11, 6];

    let nan_keys = [
        f32::NAN.to_bits(),
        f32::from_bits(0x7fc0_0001).to_bits(),
        f32::from_bits(0x7fff_ffff).to_bits(),
        f32::from_bits(0xffc0_0001).to_bits(),
        1.0_f32.to_bits(),
        2.0_f32.to_bits(),
    ];
    let nan_ids = [19_u32, 5, 71, 2, 29, 31];

    let near = 1.0_f32;
    let far = 3.0_f32;
    let boundary_keys = [
        near.to_bits() - 1,
        near.to_bits(),
        near.to_bits() + 1,
        far.to_bits() - 1,
        far.to_bits(),
        far.to_bits() + 1,
        0.0_f32.to_bits(),
        (-0.0_f32).to_bits(),
        f32::INFINITY.to_bits(),
        f32::NEG_INFINITY.to_bits(),
    ];
    let boundary_ids = [13_u32, 1, 89, 3, 55, 8, 144, 21, 34, 2];

    let position = [
        f32::from_bits(0x4b00_0001),
        f32::from_bits(0xcb00_0000),
        f32::from_bits(0x3f80_0001),
    ];
    let camera_position = [1.0_f32, -1.0, f32::from_bits(1)];
    let row = [
        f32::from_bits(0x3f00_0001),
        0.5,
        f32::from_bits(0x3f7f_ffff),
    ];
    let relative = [
        position[0] - camera_position[0],
        position[1] - camera_position[1],
        position[2] - camera_position[2],
    ];
    let fma_depth = row[2].mul_add(
        relative[2],
        row[1].mul_add(relative[1], row[0] * relative[0]),
    );
    let fma_keys = [
        fma_depth.max(0.0).to_bits(),
        fma_depth.max(0.0).to_bits().wrapping_sub(1),
        fma_depth.max(0.0).to_bits().wrapping_add(1),
    ];
    let fma_ids = [7_u32, 99, 4];

    let tie_keys = [0x7fc0_0001_u32; 11];
    let tie_ids = [91_u32, 3, 77, 5, 19, 8, 42, 1, 0, u32::MAX, 17];

    Parity {
        key: parity_case(&keys, &key_case_ids),
        source_id: parity_case(&source_keys, &source_case_ids),
        nan: parity_case(&nan_keys, &nan_ids),
        boundary: parity_case(&boundary_keys, &boundary_ids),
        fma: parity_case(&fma_keys, &fma_ids),
        stable_tie: parity_case(&tie_keys, &tie_ids),
    }
}

fn fixed_input() -> Vec<u64> {
    let mut seed = 0x6d34_5133_u32;
    let mut keys = (0..INPUT_LEN)
        .map(|_| lcg_next(&mut seed))
        .collect::<Vec<_>>();
    keys[..16].copy_from_slice(&[
        0,
        u32::MAX,
        1.0_f32.to_bits() - 1,
        1.0_f32.to_bits(),
        1.0_f32.to_bits() + 1,
        3.0_f32.to_bits() - 1,
        3.0_f32.to_bits(),
        3.0_f32.to_bits() + 1,
        f32::NAN.to_bits(),
        0x7fc0_0001,
        f32::INFINITY.to_bits(),
        f32::NEG_INFINITY.to_bits(),
        0.0_f32.to_bits(),
        (-0.0_f32).to_bits(),
        0x3f80_0001,
        0x3f80_0001,
    ]);
    let source_ids = (0..INPUT_LEN as u32).collect::<Vec<_>>();
    keys.into_iter()
        .zip(source_ids.iter().copied())
        .map(|(key, source_id)| pack_sort_pair(key, source_id))
        .collect()
}

fn measure_once(backend: &mut CpuSortBackend, input: &[u64], kernel: ForcedKernel) -> u64 {
    let mut packed = input.to_vec();
    let mut values = vec![u32::MAX; input.len()];
    let started = Instant::now();
    sort_prepacked_forced(backend, &mut packed, &mut values, kernel)
        .expect("forced benchmark sort");
    let elapsed = started.elapsed().as_nanos();
    black_box(values.first().copied());
    u64::try_from(elapsed).unwrap_or(u64::MAX)
}

fn median(samples: &mut [u64]) -> u64 {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn receipt(parity: &Parity, scalar_ns: Option<u64>, neon_ns: Option<u64>) -> String {
    let (decision, reason) = if parity.all() && scalar_ns.is_some() && neon_ns.is_some() {
        (
            "Deferred",
            "microbenchmark_only_whole_plan_terminal_pending",
        )
    } else {
        ("Rejected", "scalar_neon_element_parity_failed")
    };
    let scalar_ns = scalar_ns.map_or_else(|| "null".to_owned(), |value| value.to_string());
    let neon_ns = neon_ns.map_or_else(|| "null".to_owned(), |value| value.to_string());
    format!(
        "{{\"schema\":\"gsplat-q3-simd-cell/v1\",\"cell\":\"Q3.M4.PackedCpuExact.ScalarVsNeon\",\"decision\":\"{decision}\",\"reason\":\"{reason}\",\"input_id\":\"{INPUT_ID}\",\"input_len\":{INPUT_LEN},\"sample_pairs\":{SAMPLE_PAIRS},\"correctness\":{{\"key\":{},\"source_id\":{},\"nan_bits\":{},\"boundary_bits\":{},\"fma_derived_key\":{},\"stable_tie\":{}}},\"timing\":{{\"scalar_median_ns\":{scalar_ns},\"neon_median_ns\":{neon_ns},\"interleaved\":true}},\"whole_plan_promotion\":false}}",
        parity.key, parity.source_id, parity.nan, parity.boundary, parity.fma, parity.stable_tie,
    )
}

fn write_receipt(contents: &str) {
    let path = std::env::var_os("GSPLAT_Q3_SIMD_RECEIPT")
        .expect("GSPLAT_Q3_SIMD_RECEIPT is required for the collector");
    fs::write(path, contents).expect("write Q3 SIMD receipt");
}

#[test]
#[ignore = "manual Apple M4 diagnostic; use tests/perf/collect-q3-m4-simd.py"]
fn q3_m4_forced_scalar_neon_microbenchmark() {
    let parity = verify_parity();
    if !parity.all() {
        write_receipt(&receipt(&parity, None, None));
        panic!("forced Neon sorting differs from the Scalar oracle");
    }

    // Timing starts only after every semantic cell has matched the Scalar oracle.
    let input = fixed_input();
    let mut scalar_backend = CpuSortBackend::default();
    let mut neon_backend = CpuSortBackend::default();
    for _ in 0..WARMUP_PAIRS {
        black_box(measure_once(
            &mut scalar_backend,
            &input,
            ForcedKernel::Scalar,
        ));
        black_box(measure_once(&mut neon_backend, &input, ForcedKernel::Neon));
    }

    let mut scalar_samples = [0_u64; SAMPLE_PAIRS];
    let mut neon_samples = [0_u64; SAMPLE_PAIRS];
    for sample in 0..SAMPLE_PAIRS {
        let order = if sample % 2 == 0 {
            [ForcedKernel::Scalar, ForcedKernel::Neon]
        } else {
            [ForcedKernel::Neon, ForcedKernel::Scalar]
        };
        for kernel in order {
            let elapsed = match kernel {
                ForcedKernel::Scalar => measure_once(&mut scalar_backend, &input, kernel),
                ForcedKernel::Neon => measure_once(&mut neon_backend, &input, kernel),
            };
            match kernel {
                ForcedKernel::Scalar => scalar_samples[sample] = elapsed,
                ForcedKernel::Neon => neon_samples[sample] = elapsed,
            }
        }
    }

    let scalar_ns = median(&mut scalar_samples);
    let neon_ns = median(&mut neon_samples);
    write_receipt(&receipt(&parity, Some(scalar_ns), Some(neon_ns)));
}
