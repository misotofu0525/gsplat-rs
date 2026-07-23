use gsplat_core::SceneBuffers;

use crate::data::RESIDENT_CHUNK_SPLATS;

use super::super::codec::{sh_band, sh_coeffs_per_channel, sigmoid};
use super::super::{
    ResidentCpuByteAccounting, ResidentSceneBuilder, ResidentSceneCpu, ResidentSceneError,
};
use super::support::{assert_resident_bits_eq, legacy_encode, scene, source_splat};

#[test]
fn exact_count_and_degree_specific_sh_planes() {
    for (degree, planes) in [(0, 0), (1, 1), (2, 3), (3, 4)] {
        let source = scene(degree, 513);
        let resident = ResidentSceneCpu::encode(&source).expect("encode");
        resident.validate_complete().expect("complete");
        assert_eq!(resident.len(), source.len());
        assert_eq!(resident.report.source_count, source.len());
        assert_eq!(resident.report.encoded_count, source.len());
        let staging = resident.upload_staging().expect("staging");
        assert_eq!(
            staging.chunks.len(),
            source.len().div_ceil(RESIDENT_CHUNK_SPLATS)
        );
        assert_eq!(resident.sh_plane_count(), planes);
        for (plane, values) in staging.sh_planes.iter().enumerate() {
            assert_eq!(
                values.len(),
                if plane < planes as usize {
                    source.len()
                } else {
                    0
                }
            );
        }
    }
}

#[test]
fn streaming_builder_matches_legacy_scene_encoding_bit_for_bit_for_sh0_through_sh3() {
    for degree in 0..=3 {
        let source = scene(degree, 257);
        let expected = legacy_encode(&source);
        let mut builder = ResidentSceneBuilder::new(source.len(), degree).expect("builder");
        for index in 0..source.len() {
            builder
                .push(source_splat(&source, index))
                .expect("valid source splat");
        }
        let streamed = builder.finish().expect("complete stream");
        let compatibility = ResidentSceneCpu::encode(&source).expect("compatibility encode");
        let owned = ResidentSceneCpu::encode_owned(source.clone()).expect("owned encode");

        assert_resident_bits_eq(&streamed, &expected);
        assert_resident_bits_eq(&compatibility, &expected);
        assert_resident_bits_eq(&owned, &expected);
    }
}

#[test]
fn builder_handles_empty_partial_and_excess_streams_explicitly() {
    let empty = ResidentSceneBuilder::new(0, 0)
        .expect("empty builder")
        .finish()
        .expect("empty scene");
    empty.validate_complete().expect("valid empty scene");
    assert!(empty.is_empty());
    assert_eq!(empty.report.source_count, 0);
    assert_eq!(empty.report.encoded_count, 0);
    assert_eq!(empty.report.chunk_count, 0);

    let source = scene(1, 2);
    let mut partial = ResidentSceneBuilder::new(2, 1).expect("partial builder");
    partial.push(source_splat(&source, 0)).expect("first splat");
    assert!(matches!(
        partial.finish(),
        Err(ResidentSceneError::CountMismatch {
            expected: 2,
            actual: 1,
        })
    ));

    let mut exact = ResidentSceneBuilder::new(1, 1).expect("exact builder");
    exact
        .push(source_splat(&source, 0))
        .expect("declared splat");
    assert_eq!(
        exact.push(source_splat(&source, 1)),
        Err(ResidentSceneError::CountMismatch {
            expected: 1,
            actual: 2,
        })
    );
    let complete = exact.finish().expect("excess push did not mutate builder");
    assert_eq!(complete.len(), 1);
}

#[test]
fn builder_rejects_bad_sh_contract_and_non_finite_values_before_mutation() {
    let source = scene(3, 1);
    let valid = source_splat(&source, 0);
    let mut builder = ResidentSceneBuilder::new(1, 3).expect("builder");

    let mut bad_len = valid;
    bad_len.sh_len = 44;
    assert!(matches!(
        builder.push(bad_len),
        Err(ResidentSceneError::InvalidSplat {
            index: 0,
            field: "SH coefficient count does not match the degree",
        })
    ));

    let mut bad_degree = valid;
    bad_degree.sh_degree = 2;
    assert!(matches!(
        builder.push(bad_degree),
        Err(ResidentSceneError::InvalidSplat {
            index: 0,
            field: "SH degree does not match the scene",
        })
    ));

    let mut bad_sh = valid;
    bad_sh.sh_rest[44] = f32::NAN;
    assert!(matches!(
        builder.push(bad_sh),
        Err(ResidentSceneError::InvalidSplat {
            index: 0,
            field: "SH coefficient is not finite",
        })
    ));

    let mut underflowed_scale = valid;
    underflowed_scale.log_scale[0] = -200.0;
    assert!(matches!(
        builder.push(underflowed_scale),
        Err(ResidentSceneError::InvalidSplat {
            index: 0,
            field: "log scale cannot be represented as a finite nonzero covariance",
        })
    ));

    let mut zero_rotation = valid;
    zero_rotation.rotation_xyzw = [0.0; 4];
    assert!(matches!(
        builder.push(zero_rotation),
        Err(ResidentSceneError::InvalidSplat {
            index: 0,
            field: "rotation has no finite nonzero norm",
        })
    ));

    builder.push(valid).expect("valid retry");
    assert_eq!(builder.finish().expect("complete").len(), 1);
}

#[test]
fn builder_rejects_finite_dc_range_that_overflows_f32_metadata() {
    let source = scene(0, 2);
    let mut low = source_splat(&source, 0);
    low.color_dc = [-f32::MAX, 0.0, 0.0];
    let mut high = source_splat(&source, 1);
    high.color_dc = [f32::MAX, 0.0, 0.0];
    let mut builder = ResidentSceneBuilder::new(2, 0).expect("builder");
    builder.push(low).expect("finite low endpoint");
    builder.push(high).expect("finite high endpoint");

    assert!(matches!(
        builder.finish(),
        Err(ResidentSceneError::InvalidSplat {
            index: 0,
            field: "chunk DC color range cannot be represented as finite f32",
        })
    ));
}

#[test]
fn chunk_boundary_keeps_every_splat_in_source_order() {
    let source = scene(2, RESIDENT_CHUNK_SPLATS + 1);
    let mut builder = ResidentSceneBuilder::new(source.len(), source.sh_degree).expect("builder");
    for index in 0..source.len() {
        builder
            .push(source_splat(&source, index))
            .expect("source splat");
    }
    assert_eq!(builder.chunks.len(), 1);
    assert_eq!(builder.pending.len(), 1);

    let resident = builder.finish().expect("complete");
    assert_eq!(resident.len(), RESIDENT_CHUNK_SPLATS + 1);
    let staging = resident.upload_staging().expect("staging");
    assert_eq!(staging.chunks.len(), 2);
    assert_eq!(resident.report.chunk_count, 2);
    assert_eq!(
        resident.positions[RESIDENT_CHUNK_SPLATS - 1],
        source.positions[RESIDENT_CHUNK_SPLATS - 1]
    );
    assert_eq!(
        resident.positions[RESIDENT_CHUNK_SPLATS],
        source.positions[RESIDENT_CHUNK_SPLATS]
    );
    assert_eq!(
        staging.position_alpha[RESIDENT_CHUNK_SPLATS].position_alpha[0],
        source.positions[RESIDENT_CHUNK_SPLATS].x
    );
}

#[test]
fn builder_reports_checked_size_overflow() {
    assert!(matches!(
        ResidentSceneBuilder::new(usize::MAX, 3),
        Err(ResidentSceneError::SizeOverflow)
    ));
}

#[test]
fn complete_validation_rejects_incoherent_or_non_finite_public_planes() {
    let source = scene(3, 1);

    let mut incoherent = ResidentSceneCpu::encode(&source).unwrap();
    incoherent.upload_staging.as_mut().unwrap().position_alpha[0].position_alpha[0] += 1.0;
    assert_eq!(
        incoherent.validate_complete(),
        Err(ResidentSceneError::InvalidScene)
    );

    let mut non_finite = ResidentSceneCpu::encode(&source).unwrap();
    non_finite.upload_staging.as_mut().unwrap().chunks[0].dc_min[0] = f32::NAN;
    assert_eq!(
        non_finite.validate_complete(),
        Err(ResidentSceneError::InvalidScene)
    );
}

#[test]
fn staging_release_is_exact_and_retains_sort_positions_and_receipts() {
    let source = scene(3, 513);
    let expected_positions = source.positions.clone();
    let mut resident = ResidentSceneCpu::encode(&source).expect("encode");
    let report = resident.report;
    let before = resident.cpu_byte_accounting().expect("before accounting");
    let expected = ResidentCpuByteAccounting::for_count(source.len(), 3).expect("plan");

    assert_eq!(before, expected);
    assert!(resident.has_upload_staging());
    let released = resident.release_upload_staging().expect("release staging");
    let after = resident.cpu_byte_accounting().expect("after accounting");

    assert_eq!(released, before.upload_staging_bytes);
    assert_eq!(after.exact_position_bytes, before.exact_position_bytes);
    assert_eq!(after.upload_staging_bytes, 0);
    assert_eq!(after.total_payload_bytes, before.exact_position_bytes);
    assert!(!resident.has_upload_staging());
    assert_eq!(resident.positions.as_ref(), expected_positions.as_slice());
    assert_eq!(resident.len(), source.len());
    assert_eq!(resident.sh_degree, 3);
    assert_eq!(resident.report, report);
    assert_eq!(
        resident.validate_complete(),
        Err(ResidentSceneError::UploadStagingReleased)
    );
}

#[test]
fn failed_release_validation_does_not_drop_staging() {
    let source = scene(3, 17);
    let mut resident = ResidentSceneCpu::encode(&source).expect("encode");
    let before = resident.cpu_byte_accounting().expect("before accounting");
    resident.report.encoded_count -= 1;

    assert_eq!(
        resident.release_upload_staging(),
        Err(ResidentSceneError::CountMismatch {
            expected: source.len(),
            actual: source.len() - 1,
        })
    );
    assert!(resident.has_upload_staging());
    assert_eq!(resident.cpu_byte_accounting().unwrap(), before);
}

#[test]
fn large_scene_staging_release_receipts_are_stable() {
    // Pinned counts from the full-quality matrix. All three are SH3, so
    // SH3 staging is 112 bytes/splat plus 80 bytes per 256-splat chunk.
    // Canonical covariance is exact and SH uses four 16-byte signed-11
    // planes, preserving the portable per-binding ceiling.
    for (label, count, expected_release) in [
        ("Truck", 2_541_226, 285_411_472),
        ("Garden", 5_834_784, 655_319_248),
        ("Bicycle", 6_131_954, 688_695_088),
    ] {
        let accounting = ResidentCpuByteAccounting::for_count(count, 3).expect(label);
        assert_eq!(
            accounting.upload_staging_bytes, expected_release,
            "{label} staging receipt"
        );
        assert_eq!(
            accounting.exact_position_bytes,
            12_u64 * count as u64,
            "{label} retained positions"
        );
        assert_eq!(
            accounting.total_payload_bytes,
            accounting.exact_position_bytes + expected_release,
            "{label} total pre-upload payload"
        );
    }
}

#[test]
fn source_order_and_exact_position_alpha_are_preserved() {
    let source = scene(3, 300);
    let resident = ResidentSceneCpu::encode(&source).expect("encode");
    let staging = resident.upload_staging().expect("staging");
    for index in 0..source.len() {
        assert_eq!(resident.positions[index], source.positions[index]);
        assert_eq!(
            staging.position_alpha[index].position_alpha,
            [
                source.positions[index].x,
                source.positions[index].y,
                source.positions[index].z,
                sigmoid(source.opacity[index]),
            ]
        );
    }
}

#[test]
fn chunk_local_round_trip_matches_reported_errors() {
    let source = scene(3, 700);
    let resident = ResidentSceneCpu::encode(&source).expect("encode");
    let coeffs = sh_coeffs_per_channel(3);
    let source_sh = source.sh_rest.as_deref().expect("sh");
    for index in 0..source.len() {
        let covariance = resident.decode_covariance(index);
        let expected_covariance = crate::world_covariance_from_source(
            source.scale_xyz[index],
            source.rotation_xyzw[index],
        );
        let expected_covariance = [
            expected_covariance[0][0],
            expected_covariance[0][1],
            expected_covariance[0][2],
            expected_covariance[1][1],
            expected_covariance[1][2],
            expected_covariance[2][2],
        ];
        let dc = resident.decode_dc(index);
        for lane in 0..6 {
            assert_eq!(
                covariance[lane].to_bits(),
                expected_covariance[lane].to_bits()
            );
        }
        for (decoded_dc, source_dc) in dc.iter().zip(source.color_dc[index].iter()) {
            assert!((decoded_dc - source_dc).abs() <= resident.report.max_dc_error + 1e-7);
        }
        for channel in 0..3 {
            let decoded = resident.decode_sh_channel(index, channel);
            for lane in 0..coeffs {
                let original = source_sh[index * coeffs * 3 + channel * coeffs + lane];
                let band = sh_band(lane);
                assert!(
                    (decoded[lane] - original).abs()
                        <= resident.report.max_sh_error_by_band[band] + 1e-7
                );
            }
        }
    }
    assert_eq!(resident.report.max_covariance_error, 0.0);
    assert!(resident.report.max_dc_error < 0.001);
    assert!(
        resident
            .report
            .max_sh_error_by_band
            .iter()
            .all(|error| *error < 0.01)
    );
}

#[test]
fn rejects_degree_four_instead_of_dropping_coefficients() {
    let source = SceneBuffers {
        sh_degree: 4,
        sh_rest: Some(vec![0.0; 24 * 3]),
        ..scene(0, 1)
    };
    assert!(matches!(
        ResidentSceneCpu::encode(&source),
        Err(ResidentSceneError::UnsupportedShDegree(4))
    ));
}
