use std::sync::Arc;

use bytemuck::Zeroable;
use gsplat_core::{SceneBuffers, Vec3f};

use crate::data::{
    RESIDENT_CHUNK_SPLATS, ResidentChunkMeta, ResidentColorAux, ResidentCovariance0,
    ResidentCovariance1, ResidentPositionAlpha,
};

use super::super::codec::{
    finite_extent, finite_or_zero, nonzero_sh_scale, pack_sh_planes, pack_vec3_u16, sh_band,
    sh_coeffs_per_channel, sigmoid, update_base_error_report,
};
use super::super::resident::{
    ResidentEncodingReport, ResidentSceneCpu, ResidentSourceSplat, ResidentUploadStaging,
};
use super::super::resident_sh_plane_count;

pub(super) fn scene(degree: u8, count: usize) -> SceneBuffers {
    let coeffs = sh_coeffs_per_channel(degree);
    let mut sh_rest = vec![0.0; count * coeffs * 3];
    for point in 0..count {
        for channel in 0..3 {
            for lane in 0..coeffs {
                sh_rest[point * coeffs * 3 + channel * coeffs + lane] =
                    ((point * 17 + channel * 7 + lane * 3) as f32 * 0.013).sin() * 0.8;
            }
        }
    }
    SceneBuffers {
        positions: (0..count)
            .map(|i| Vec3f::new(i as f32 * 0.01, -(i as f32) * 0.02, 1.0 + i as f32))
            .collect(),
        opacity: (0..count).map(|i| i as f32 * 0.01 - 1.0).collect(),
        scale_xyz: (0..count)
            .map(|i| {
                let base = i as f32 * 0.001;
                [-3.0 + base, -2.0 - base, -1.0 + base * 2.0]
            })
            .collect(),
        rotation_xyzw: (0..count)
            .map(|i| crate::quat_normalize([0.001 * i as f32, 0.2, -0.1, 0.97]))
            .collect(),
        color_dc: (0..count)
            .map(|i| [i as f32 * 0.002 - 0.2, 0.4, -(i as f32) * 0.001])
            .collect(),
        sh_degree: degree,
        sh_rest: (degree > 0).then_some(sh_rest),
    }
}

pub(super) fn source_splat(scene: &SceneBuffers, index: usize) -> ResidentSourceSplat {
    let coeffs = sh_coeffs_per_channel(scene.sh_degree);
    let sh_len = coeffs * 3;
    let mut sh_rest = [0.0; 45];
    if let Some(source) = scene.sh_rest.as_deref() {
        let start = index * sh_len;
        sh_rest[..sh_len].copy_from_slice(&source[start..start + sh_len]);
    }
    ResidentSourceSplat {
        position: scene.positions[index],
        opacity_logit: scene.opacity[index],
        log_scale: scene.scale_xyz[index],
        rotation_xyzw: scene.rotation_xyzw[index],
        color_dc: scene.color_dc[index],
        sh_rest,
        sh_len: sh_len as u8,
        sh_degree: scene.sh_degree,
    }
}

fn legacy_chunk_meta(
    scene: &SceneBuffers,
    start: usize,
    end: usize,
    coeffs_per_channel: usize,
) -> ResidentChunkMeta {
    let mut dc_min = [f32::INFINITY; 3];
    let mut dc_max = [f32::NEG_INFINITY; 3];
    let mut sh_scales = [[0.0_f32; 3]; 3];
    let rest = scene.sh_rest.as_deref().unwrap_or(&[]);
    let stride = coeffs_per_channel * 3;

    for index in start..end {
        for axis in 0..3 {
            dc_min[axis] = dc_min[axis].min(scene.color_dc[index][axis]);
            dc_max[axis] = dc_max[axis].max(scene.color_dc[index][axis]);
        }
        for channel in 0..3 {
            for lane in 0..coeffs_per_channel {
                let band = sh_band(lane);
                let value = rest[index * stride + channel * coeffs_per_channel + lane].abs();
                sh_scales[band][channel] = sh_scales[band][channel].max(value);
            }
        }
    }

    let mut meta = ResidentChunkMeta::zeroed();
    for lane in 0..3 {
        meta.dc_min[lane] = finite_or_zero(dc_min[lane]);
        meta.dc_extent[lane] = finite_extent(dc_min[lane], dc_max[lane]);
        meta.sh_scale_l1[lane] = nonzero_sh_scale(sh_scales[0][lane]);
        meta.sh_scale_l2[lane] = nonzero_sh_scale(sh_scales[1][lane]);
        meta.sh_scale_l3[lane] = nonzero_sh_scale(sh_scales[2][lane]);
    }
    meta
}

pub(super) fn legacy_encode(scene: &SceneBuffers) -> ResidentSceneCpu {
    let count = scene.len();
    let coeffs_per_channel = sh_coeffs_per_channel(scene.sh_degree);
    let sh_plane_count = resident_sh_plane_count(scene.sh_degree);
    let chunk_count = count.div_ceil(RESIDENT_CHUNK_SPLATS);
    let mut position_alpha = Vec::with_capacity(count);
    let mut covariance0_plane = Vec::with_capacity(count);
    let mut covariance1_plane = Vec::with_capacity(count);
    let mut color_aux = Vec::with_capacity(count);
    let mut sh_planes = std::array::from_fn(|plane| {
        Vec::with_capacity(if plane < sh_plane_count { count } else { 0 })
    });
    let mut chunks = Vec::with_capacity(chunk_count);
    let mut report = ResidentEncodingReport {
        source_count: count,
        encoded_count: count,
        chunk_count,
        ..ResidentEncodingReport::default()
    };

    for chunk_index in 0..chunk_count {
        let start = chunk_index * RESIDENT_CHUNK_SPLATS;
        let end = (start + RESIDENT_CHUNK_SPLATS).min(count);
        let meta = legacy_chunk_meta(scene, start, end, coeffs_per_channel);
        chunks.push(meta);
        for index in start..end {
            let position = scene.positions[index];
            position_alpha.push(ResidentPositionAlpha {
                position_alpha: [
                    position.x,
                    position.y,
                    position.z,
                    sigmoid(scene.opacity[index]),
                ],
            });
            let covariance = crate::world_covariance_from_source(
                scene.scale_xyz[index],
                scene.rotation_xyzw[index],
            );
            let covariance0 = [
                covariance[0][0],
                covariance[0][1],
                covariance[0][2],
                covariance[1][1],
            ];
            let covariance1 = [covariance[1][2], covariance[2][2]];
            let dc_bits = pack_vec3_u16(scene.color_dc[index], meta.dc_min, meta.dc_extent);
            covariance0_plane.push(ResidentCovariance0 {
                values: covariance0,
            });
            covariance1_plane.push(ResidentCovariance1 {
                values: covariance1,
            });
            color_aux.push(ResidentColorAux { words: dc_bits });
            update_base_error_report(&mut report, scene.color_dc[index], &meta, dc_bits);

            if coeffs_per_channel > 0 {
                let source = scene.sh_rest.as_deref().expect("validated SH");
                let stride = coeffs_per_channel * 3;
                let point_base = index * stride;
                pack_sh_planes(
                    &source[point_base..point_base + stride],
                    coeffs_per_channel,
                    sh_plane_count,
                    &meta,
                    &mut sh_planes,
                    &mut report,
                );
            }
        }
    }

    ResidentSceneCpu {
        positions: Arc::from(scene.positions.clone().into_boxed_slice()),
        upload_staging: Some(ResidentUploadStaging {
            position_alpha,
            covariance0: covariance0_plane,
            covariance1: covariance1_plane,
            color_aux,
            sh_planes,
            chunks,
        }),
        sh_degree: scene.sh_degree,
        report,
    }
}

fn assert_f32_bits_eq(left: f32, right: f32) {
    assert_eq!(left.to_bits(), right.to_bits());
}

pub(super) fn assert_resident_bits_eq(left: &ResidentSceneCpu, right: &ResidentSceneCpu) {
    assert_eq!(left.len(), right.len());
    let left_staging = left.upload_staging().expect("left staging");
    let right_staging = right.upload_staging().expect("right staging");
    for (left, right) in left.positions.iter().zip(right.positions.iter()) {
        assert_f32_bits_eq(left.x, right.x);
        assert_f32_bits_eq(left.y, right.y);
        assert_f32_bits_eq(left.z, right.z);
    }
    assert_eq!(
        bytemuck::cast_slice::<ResidentPositionAlpha, u8>(&left_staging.position_alpha),
        bytemuck::cast_slice::<ResidentPositionAlpha, u8>(&right_staging.position_alpha)
    );
    assert_eq!(
        bytemuck::cast_slice::<ResidentCovariance0, u8>(&left_staging.covariance0),
        bytemuck::cast_slice::<ResidentCovariance0, u8>(&right_staging.covariance0)
    );
    assert_eq!(
        bytemuck::cast_slice::<ResidentCovariance1, u8>(&left_staging.covariance1),
        bytemuck::cast_slice::<ResidentCovariance1, u8>(&right_staging.covariance1)
    );
    assert_eq!(
        bytemuck::cast_slice::<ResidentColorAux, u8>(&left_staging.color_aux),
        bytemuck::cast_slice::<ResidentColorAux, u8>(&right_staging.color_aux)
    );
    assert_eq!(left_staging.sh_planes, right_staging.sh_planes);
    assert_eq!(
        bytemuck::cast_slice::<ResidentChunkMeta, u8>(&left_staging.chunks),
        bytemuck::cast_slice::<ResidentChunkMeta, u8>(&right_staging.chunks)
    );
    assert_eq!(left.sh_degree, right.sh_degree);
    assert_eq!(left.sh_coeffs_per_channel(), right.sh_coeffs_per_channel());
    assert_eq!(left.sh_plane_count(), right.sh_plane_count());
    assert_eq!(left.report, right.report);
}
