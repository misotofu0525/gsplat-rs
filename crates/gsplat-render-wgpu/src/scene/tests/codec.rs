use crate::data::{
    RESIDENT_SH_PLANES, RESIDENT_SH_WORDS_PER_PLANE, ResidentChunkMeta, ResidentColorAux,
    ResidentCovariance0, ResidentCovariance1, ResidentPositionAlpha, ResidentShPlane,
};

use super::super::codec::{
    RESIDENT_SH_BITS, RESIDENT_SH_POINT_SCALE_BITS, pack_signed_11, pack_unsigned_bits,
    resident_sh_plane_count, sh_coeffs_per_channel, unpack_signed_11, unpack_unsigned_bits,
};
use super::super::{ResidentSceneCpu, ResidentSourceSplat};
use super::support::scene;

#[test]
fn resident_struct_sizes_match_wgsl_contract() {
    assert_eq!(std::mem::size_of::<ResidentPositionAlpha>(), 16);
    assert_eq!(std::mem::size_of::<ResidentCovariance0>(), 16);
    assert_eq!(std::mem::size_of::<ResidentCovariance1>(), 8);
    assert_eq!(std::mem::size_of::<ResidentColorAux>(), 8);
    assert_eq!(std::mem::size_of::<ResidentShPlane>(), 16);
    assert_eq!(std::mem::size_of::<ResidentChunkMeta>(), 80);
    assert_eq!(std::mem::size_of::<ResidentSourceSplat>(), 240);
}

fn packed_words_as_planes(
    words: [u32; RESIDENT_SH_PLANES * RESIDENT_SH_WORDS_PER_PLANE],
) -> [Vec<ResidentShPlane>; RESIDENT_SH_PLANES] {
    std::array::from_fn(|plane| {
        let start = plane * RESIDENT_SH_WORDS_PER_PLANE;
        vec![ResidentShPlane {
            words: words[start..start + RESIDENT_SH_WORDS_PER_PLANE]
                .try_into()
                .expect("fixed resident SH plane width"),
        }]
    })
}

#[test]
fn signed11_round_trips_extrema_without_neighbor_corruption() {
    let mut words = [0_u32; RESIDENT_SH_PLANES * RESIDENT_SH_WORDS_PER_PLANE];
    let expected: [i32; 45] =
        std::array::from_fn(
            |logical_value| {
                if logical_value % 2 == 0 { -1023 } else { 1023 }
            },
        );
    for (logical_value, value) in expected.iter().copied().enumerate() {
        pack_signed_11(&mut words, logical_value, value);
    }
    let planes = packed_words_as_planes(words);
    for (logical_value, expected) in expected.iter().copied().enumerate() {
        assert_eq!(
            unpack_signed_11(&planes, 0, logical_value),
            expected,
            "signed-11 logical value {logical_value}"
        );
    }
}

#[test]
fn signed11_cross_word_and_plane_boundaries_match_exact_bit_layout() {
    let cross_word_values: Vec<_> = (0..45)
        .filter(|logical_value| (logical_value * RESIDENT_SH_BITS) % 32 > 21)
        .collect();
    assert_eq!(
        cross_word_values,
        vec![2, 5, 8, 11, 14, 17, 20, 23, 26, 29, 34, 37, 40, 43]
    );
    let cross_plane_values: Vec<_> = cross_word_values
        .iter()
        .copied()
        .filter(|logical_value| {
            let bit_offset = logical_value * RESIDENT_SH_BITS;
            let word = bit_offset / 32;
            word / RESIDENT_SH_WORDS_PER_PLANE != (word + 1) / RESIDENT_SH_WORDS_PER_PLANE
        })
        .collect();
    assert_eq!(cross_plane_values, vec![11, 23, 34]);

    for logical_value in cross_word_values {
        for value in [-1023_i32, -1, 1, 1023] {
            let mut words = [0_u32; RESIDENT_SH_PLANES * RESIDENT_SH_WORDS_PER_PLANE];
            pack_signed_11(&mut words, logical_value, value);
            let bits = (value as u32) & 0x7ff;
            let bit_offset = logical_value * RESIDENT_SH_BITS;
            let word = bit_offset / 32;
            let shift = bit_offset % 32;
            assert_eq!(words[word], bits << shift);
            assert_eq!(words[word + 1], bits >> (32 - shift));
            assert!(words[..word].iter().all(|word| *word == 0));
            assert!(words[word + 2..].iter().all(|word| *word == 0));

            let planes = packed_words_as_planes(words);
            assert_eq!(
                unpack_signed_11(&planes, 0, logical_value),
                value,
                "logical value {logical_value} at shift {shift}"
            );
        }
    }
}

#[test]
fn per_point_band_scales_use_only_degree3_spare_bits() {
    let mut words = [0_u32; RESIDENT_SH_PLANES * RESIDENT_SH_WORDS_PER_PLANE];
    let expected: [i32; 45] = std::array::from_fn(|logical_value| logical_value as i32 * 31 - 700);
    for (logical_value, value) in expected.iter().copied().enumerate() {
        pack_signed_11(&mut words, logical_value, value);
    }
    let scale_bit_base = expected.len() * RESIDENT_SH_BITS;
    let scale_codes = [1_u32, 17, 31];
    for (band, code) in scale_codes.iter().copied().enumerate() {
        pack_unsigned_bits(
            &mut words,
            scale_bit_base + band * RESIDENT_SH_POINT_SCALE_BITS,
            RESIDENT_SH_POINT_SCALE_BITS,
            code,
        );
    }
    let planes = packed_words_as_planes(words);

    for (logical_value, expected) in expected.iter().copied().enumerate() {
        assert_eq!(unpack_signed_11(&planes, 0, logical_value), expected);
    }
    for (band, expected) in scale_codes.iter().copied().enumerate() {
        assert_eq!(
            unpack_unsigned_bits(
                &planes,
                0,
                scale_bit_base + band * RESIDENT_SH_POINT_SCALE_BITS,
                RESIDENT_SH_POINT_SCALE_BITS,
            ),
            expected
        );
    }
    assert_eq!(words[15] >> 30, 0, "top two spare bits stay reserved");
}

#[test]
fn per_point_band_scale_isolates_chunk_outlier_without_clipping() {
    let mut source = scene(3, 2);
    let sh = source.sh_rest.as_mut().expect("degree 3 SH");
    sh[..45].fill(1.0);
    sh[45..].fill(0.001);

    let resident = ResidentSceneCpu::encode(&source).expect("encode");
    for channel in 0..3 {
        let outlier = resident.decode_sh_channel(0, channel);
        let small = resident.decode_sh_channel(1, channel);
        assert!(outlier.iter().all(|value| (*value - 1.0).abs() <= 1e-7));
        assert!(small.iter().all(|value| (*value - 0.001).abs() < 0.00002));
    }
    assert!(
        resident
            .report
            .max_sh_error_by_band
            .iter()
            .all(|error| *error < 0.00002)
    );
}

#[test]
fn degree_specific_point_scales_fit_the_existing_final_active_plane() {
    for degree in 1..=3 {
        let coeffs_per_channel = sh_coeffs_per_channel(degree);
        let active_band_count = degree as usize;
        let encoded_bits = coeffs_per_channel * 3 * RESIDENT_SH_BITS
            + active_band_count * RESIDENT_SH_POINT_SCALE_BITS;
        let plane_count = resident_sh_plane_count(degree);
        assert!(encoded_bits <= plane_count * RESIDENT_SH_WORDS_PER_PLANE * u32::BITS as usize);

        let mut source = scene(degree, 2);
        let sh = source.sh_rest.as_mut().expect("active SH");
        let stride = coeffs_per_channel * 3;
        sh[..stride].fill(1.0);
        sh[stride..].fill(0.001);
        let resident = ResidentSceneCpu::encode(&source).expect("encode");
        assert_eq!(resident.sh_plane_count(), plane_count as u32);
        for channel in 0..3 {
            assert!(
                resident
                    .decode_sh_channel(1, channel)
                    .iter()
                    .all(|value| (*value - 0.001).abs() < 0.00002)
            );
        }
    }
}
