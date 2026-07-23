use std::sync::Arc;

use gsplat_core::{SceneBuffers, Vec3f};
use thiserror::Error;

use crate::data::{
    RESIDENT_CHUNK_SPLATS, RESIDENT_SH_PLANES, ResidentChunkMeta, ResidentColorAux,
    ResidentCovariance0, ResidentCovariance1, ResidentPositionAlpha, ResidentShPlane,
};

use super::budget::ResidentCpuByteAccounting;
use super::builder::{ResidentSceneBuilder, push_scene_into_builder};
#[cfg(test)]
use super::codec::{
    RESIDENT_SH_BITS, RESIDENT_SH_POINT_SCALE_BITS, RESIDENT_SH_POINT_SCALE_MAX, sh_band,
    sh_band_scale, unpack_signed_11, unpack_unsigned_bits, unpack_vec3_u16,
};
use super::codec::{resident_sh_plane_count, sh_coeffs_per_channel};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResidentSceneError {
    #[error("invalid source scene")]
    InvalidScene,
    #[error("resident allocation failed for {resource}")]
    AllocationFailed { resource: &'static str },
    #[error("resident source count mismatch: expected {expected}, got {actual}")]
    CountMismatch { expected: usize, actual: usize },
    #[error("invalid resident source splat {index}: {field}")]
    InvalidSplat { index: usize, field: &'static str },
    #[error("resident path supports SH degree 0 through 3, got {0}")]
    UnsupportedShDegree(u8),
    #[error("resident scene size overflows addressable storage")]
    SizeOverflow,
    #[error("resident upload staging has already been released")]
    UploadStagingReleased,
}

/// Heap-free source value accepted by [`ResidentSceneBuilder`].
///
/// SH coefficients use the same per-splat layout as [`SceneBuffers`]: all R
/// coefficients, then G, then B, excluding DC. Only the first `sh_len` values
/// are active. Degree 0/1/2/3 therefore require 0/9/24/45 values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResidentSourceSplat {
    pub position: Vec3f,
    pub opacity_logit: f32,
    pub log_scale: [f32; 3],
    pub rotation_xyzw: [f32; 4],
    pub color_dc: [f32; 3],
    pub sh_rest: [f32; 45],
    pub sh_len: u8,
    pub sh_degree: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ResidentEncodingReport {
    pub source_count: usize,
    pub encoded_count: usize,
    pub chunk_count: usize,
    pub max_position_error: f32,
    pub max_alpha_error: f32,
    pub max_covariance_error: f32,
    pub max_dc_error: f32,
    pub max_sh_error_by_band: [f32; 3],
}

#[derive(Debug, Clone)]
pub(crate) struct ResidentUploadStaging {
    pub(crate) position_alpha: Vec<ResidentPositionAlpha>,
    pub(crate) covariance0: Vec<ResidentCovariance0>,
    pub(crate) covariance1: Vec<ResidentCovariance1>,
    pub(crate) color_aux: Vec<ResidentColorAux>,
    pub(crate) sh_planes: [Vec<ResidentShPlane>; RESIDENT_SH_PLANES],
    pub(crate) chunks: Vec<ResidentChunkMeta>,
}

#[derive(Debug, Clone)]
pub struct ResidentSceneCpu {
    /// Exact source-order positions retained for CPU ordering.
    pub positions: Arc<[Vec3f]>,
    /// GPU-upload-only planes. This is deliberately absent after a successful
    /// Surface handoff; the renderer remains fully sortable through `positions`.
    pub(super) upload_staging: Option<ResidentUploadStaging>,
    pub sh_degree: u8,
    pub report: ResidentEncodingReport,
}

impl ResidentSceneCpu {
    /// Consumes the compatibility scene and retains its position allocation as
    /// the exact CPU-order source. Other wide float attribute arrays are
    /// released after their compact planes have been produced.
    pub fn encode_owned(scene: SceneBuffers) -> Result<Self, ResidentSceneError> {
        scene
            .validate()
            .map_err(|_| ResidentSceneError::InvalidScene)?;
        let mut builder = ResidentSceneBuilder::new_internal(scene.len(), scene.sh_degree, false)?;
        push_scene_into_builder(&mut builder, &scene)?;
        builder.finish_with_positions(scene.positions)
    }

    pub fn encode(scene: &SceneBuffers) -> Result<Self, ResidentSceneError> {
        scene
            .validate()
            .map_err(|_| ResidentSceneError::InvalidScene)?;
        let mut builder = ResidentSceneBuilder::new(scene.len(), scene.sh_degree)?;
        push_scene_into_builder(&mut builder, scene)?;
        builder.finish()
    }

    pub fn len(&self) -> usize {
        self.report.source_count
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn sh_coeffs_per_channel(&self) -> u32 {
        sh_coeffs_per_channel(self.sh_degree) as u32
    }

    pub const fn sh_plane_count(&self) -> u32 {
        resident_sh_plane_count(self.sh_degree) as u32
    }

    /// True while compact attribute planes are still available for one GPU
    /// upload. A Surface session clears this only after its presenter has
    /// successfully created all durable Packed GPU resources.
    pub fn has_upload_staging(&self) -> bool {
        self.upload_staging.is_some()
    }

    pub fn cpu_byte_accounting(&self) -> Result<ResidentCpuByteAccounting, ResidentSceneError> {
        let mut accounting = ResidentCpuByteAccounting::for_count(self.len(), self.sh_degree)?;
        if self.upload_staging.is_none() {
            accounting.upload_staging_bytes = 0;
            accounting.total_payload_bytes = accounting.exact_position_bytes;
        }
        Ok(accounting)
    }

    /// Drops all GPU-upload-only compact planes after validating them one last
    /// time. Validation completes before mutation, so a failure retains every
    /// staging allocation and can be retried or reported transactionally.
    pub(crate) fn release_upload_staging(&mut self) -> Result<u64, ResidentSceneError> {
        self.validate_complete()?;
        let released_bytes = self.cpu_byte_accounting()?.upload_staging_bytes;
        drop(self.upload_staging.take());
        Ok(released_bytes)
    }

    pub(crate) fn upload_staging(&self) -> Result<&ResidentUploadStaging, ResidentSceneError> {
        self.upload_staging
            .as_ref()
            .ok_or(ResidentSceneError::UploadStagingReleased)
    }

    /// Validates the complete uploadable representation. Retained post-upload
    /// scenes intentionally return [`ResidentSceneError::UploadStagingReleased`]
    /// because they cannot be used to construct a second GPU resident copy.
    pub fn validate_complete(&self) -> Result<(), ResidentSceneError> {
        let count = self.len();
        let sh_plane_count =
            usize::try_from(self.sh_plane_count()).map_err(|_| ResidentSceneError::SizeOverflow)?;
        if self.report.source_count != self.report.encoded_count {
            return Err(ResidentSceneError::CountMismatch {
                expected: self.report.source_count,
                actual: self.report.encoded_count,
            });
        }
        if self.report.source_count != count {
            return Err(ResidentSceneError::CountMismatch {
                expected: self.report.source_count,
                actual: count,
            });
        }
        if self.positions.len() != count
            || self.report.chunk_count != count.div_ceil(RESIDENT_CHUNK_SPLATS)
            || self.sh_degree > 3
        {
            return Err(ResidentSceneError::InvalidScene);
        }
        let errors = [
            self.report.max_position_error,
            self.report.max_alpha_error,
            self.report.max_covariance_error,
            self.report.max_dc_error,
            self.report.max_sh_error_by_band[0],
            self.report.max_sh_error_by_band[1],
            self.report.max_sh_error_by_band[2],
        ];
        if self.positions.iter().any(|position| !position.is_finite())
            || !errors
                .iter()
                .all(|error| error.is_finite() && *error >= 0.0)
        {
            return Err(ResidentSceneError::InvalidScene);
        }

        let staging = self.upload_staging()?;
        if staging.position_alpha.len() != count
            || staging.covariance0.len() != count
            || staging.covariance1.len() != count
            || staging.color_aux.len() != count
            || staging.chunks.len() != self.report.chunk_count
            || staging.sh_planes.iter().enumerate().any(|(plane, values)| {
                values.len() != if plane < sh_plane_count { count } else { 0 }
            })
        {
            return Err(ResidentSceneError::InvalidScene);
        }
        for (position, encoded) in self.positions.iter().zip(&staging.position_alpha) {
            let [x, y, z, alpha] = encoded.position_alpha;
            if !encoded.position_alpha.iter().all(|value| value.is_finite())
                || x.to_bits() != position.x.to_bits()
                || y.to_bits() != position.y.to_bits()
                || z.to_bits() != position.z.to_bits()
                || !(0.0..=1.0).contains(&alpha)
            {
                return Err(ResidentSceneError::InvalidScene);
            }
        }
        if staging
            .covariance0
            .iter()
            .any(|entry| !entry.values.iter().all(|value| value.is_finite()))
            || staging
                .covariance1
                .iter()
                .any(|entry| !entry.values.iter().all(|value| value.is_finite()))
        {
            return Err(ResidentSceneError::InvalidScene);
        }
        for chunk in &staging.chunks {
            if !chunk_meta_is_valid(chunk) {
                return Err(ResidentSceneError::InvalidScene);
            }
        }
        Ok(())
    }

    pub fn static_attribute_bytes(&self) -> u64 {
        self.cpu_byte_accounting()
            .map(|accounting| accounting.upload_staging_bytes)
            .unwrap_or(u64::MAX)
    }

    #[cfg(test)]
    pub(super) fn decode_covariance(&self, index: usize) -> [f32; 6] {
        let staging = self.upload_staging().expect("decode requires staging");
        let first = staging.covariance0[index].values;
        let second = staging.covariance1[index].values;
        [first[0], first[1], first[2], first[3], second[0], second[1]]
    }

    #[cfg(test)]
    pub(super) fn decode_dc(&self, index: usize) -> [f32; 3] {
        let staging = self.upload_staging().expect("decode requires staging");
        let meta = staging.chunks[index / RESIDENT_CHUNK_SPLATS];
        unpack_vec3_u16(staging.color_aux[index].words, meta.dc_min, meta.dc_extent)
    }

    #[cfg(test)]
    pub(super) fn decode_sh_channel(&self, index: usize, channel: usize) -> Vec<f32> {
        let staging = self.upload_staging().expect("decode requires staging");
        let coeffs = self.sh_coeffs_per_channel() as usize;
        let meta = staging.chunks[index / RESIDENT_CHUNK_SPLATS];
        let mut out = Vec::with_capacity(coeffs);
        for lane in 0..coeffs {
            let logical_value = lane * 3 + channel;
            let value = unpack_signed_11(&staging.sh_planes, index, logical_value);
            let point_scale_code = unpack_unsigned_bits(
                &staging.sh_planes,
                index,
                coeffs * 3 * RESIDENT_SH_BITS + sh_band(lane) * RESIDENT_SH_POINT_SCALE_BITS,
                RESIDENT_SH_POINT_SCALE_BITS,
            );
            let point_scale = point_scale_code as f32 / RESIDENT_SH_POINT_SCALE_MAX as f32;
            out.push(value as f32 / 1023.0 * sh_band_scale(&meta, channel, lane) * point_scale);
        }
        out
    }
}

fn chunk_meta_is_valid(meta: &ResidentChunkMeta) -> bool {
    let all_finite = [
        meta.dc_min,
        meta.dc_extent,
        meta.sh_scale_l1,
        meta.sh_scale_l2,
        meta.sh_scale_l3,
    ]
    .into_iter()
    .flatten()
    .all(f32::is_finite);
    all_finite
        && meta.dc_extent[..3].iter().all(|extent| *extent >= 0.0)
        && meta.sh_scale_l1[..3]
            .iter()
            .chain(&meta.sh_scale_l2[..3])
            .chain(&meta.sh_scale_l3[..3])
            .all(|scale| *scale > 0.0)
}
