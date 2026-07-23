//! Exact-count compact scene encoding for the production resident path.
//!
//! The encoding is one-to-one and stays in source order. Positions and alpha
//! remain float32; the exact canonical world covariance is stored instead of
//! lossy scale/rotation quantization, while DC and SH use chunk-local compact
//! encodings. No function in this module samples or drops splats.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use gsplat_core::{SceneBuffers, Vec3f};
use thiserror::Error;

pub const RESIDENT_CHUNK_SPLATS: usize = 256;
pub const RESIDENT_COVARIANCE0_FLOATS: usize = 4;
pub const RESIDENT_COVARIANCE1_FLOATS: usize = 2;
pub const RESIDENT_COLOR_AUX_WORDS: usize = 2;
pub const RESIDENT_SH_PLANES: usize = 4;
pub const RESIDENT_SH_WORDS_PER_PLANE: usize = 4;
const RESIDENT_SH_BITS: usize = 11;
const RESIDENT_SH_POINT_SCALE_BITS: usize = 5;
const RESIDENT_SH_POINT_SCALE_MAX: u32 = (1 << RESIDENT_SH_POINT_SCALE_BITS) - 1;
pub const RESIDENT_CHUNK_META_BYTES: usize = std::mem::size_of::<ResidentChunkMeta>();

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

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ResidentPositionAlpha {
    pub position_alpha: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ResidentCovariance0 {
    /// Canonical world-covariance terms xx, xy, xz and yy.
    pub values: [f32; RESIDENT_COVARIANCE0_FLOATS],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ResidentCovariance1 {
    /// Canonical world-covariance terms yz and zz.
    pub values: [f32; RESIDENT_COVARIANCE1_FLOATS],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
pub struct ResidentColorAux {
    /// Chunk-local SH DC u16x3. The high half of the second word is reserved.
    pub words: [u32; RESIDENT_COLOR_AUX_WORDS],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
pub struct ResidentShPlane {
    /// Four packed words. Across all four planes, 45 SH3 values use signed
    /// 11-bit quantization in coefficient-major RGB order. Three 5-bit
    /// per-point band-scale ratios occupy bits 495 through 509; the final two
    /// bits stay reserved. Lower degrees place their active scale ratios
    /// immediately after their last coefficient in the final active plane.
    pub words: [u32; RESIDENT_SH_WORDS_PER_PLANE],
}

/// Five `vec4<f32>` values. The fourth lane is reserved and kept zero so the
/// Rust layout exactly matches WGSL storage-buffer alignment.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ResidentChunkMeta {
    pub dc_min: [f32; 4],
    pub dc_extent: [f32; 4],
    pub sh_scale_l1: [f32; 4],
    pub sh_scale_l2: [f32; 4],
    pub sh_scale_l3: [f32; 4],
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

/// Logical CPU payload bytes owned by an exact resident scene.
///
/// This intentionally reports payload bytes rather than allocator overhead.
/// `upload_staging_bytes` becomes zero after a Surface presenter has copied
/// the compact planes into durable GPU buffers; exact positions remain for
/// CPU ordering, camera framing, and benchmark receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResidentCpuByteAccounting {
    pub exact_position_bytes: u64,
    pub upload_staging_bytes: u64,
    pub total_payload_bytes: u64,
}

impl ResidentCpuByteAccounting {
    pub fn for_count(splat_count: usize, sh_degree: u8) -> Result<Self, ResidentSceneError> {
        if sh_degree > 3 {
            return Err(ResidentSceneError::UnsupportedShDegree(sh_degree));
        }
        let count = u64::try_from(splat_count).map_err(|_| ResidentSceneError::SizeOverflow)?;
        let sh_plane_count = u64::try_from(resident_sh_plane_count(sh_degree))
            .map_err(|_| ResidentSceneError::SizeOverflow)?;
        let chunk_count = count.div_ceil(RESIDENT_CHUNK_SPLATS as u64);
        let exact_position_bytes = count
            .checked_mul(std::mem::size_of::<Vec3f>() as u64)
            .ok_or(ResidentSceneError::SizeOverflow)?;
        let upload_staging_bytes = count
            .checked_mul(std::mem::size_of::<ResidentPositionAlpha>() as u64)
            .and_then(|bytes| {
                bytes.checked_add(
                    count.checked_mul(std::mem::size_of::<ResidentCovariance0>() as u64)?,
                )
            })
            .and_then(|bytes| {
                bytes
                    .checked_add(count.checked_mul(std::mem::size_of::<ResidentColorAux>() as u64)?)
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    count.checked_mul(std::mem::size_of::<ResidentCovariance1>() as u64)?,
                )
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    count
                        .checked_mul(std::mem::size_of::<ResidentShPlane>() as u64)?
                        .checked_mul(sh_plane_count)?,
                )
            })
            .and_then(|bytes| {
                bytes.checked_add(
                    chunk_count.checked_mul(std::mem::size_of::<ResidentChunkMeta>() as u64)?,
                )
            })
            .ok_or(ResidentSceneError::SizeOverflow)?;
        let total_payload_bytes = exact_position_bytes
            .checked_add(upload_staging_bytes)
            .ok_or(ResidentSceneError::SizeOverflow)?;
        Ok(Self {
            exact_position_bytes,
            upload_staging_bytes,
            total_payload_bytes,
        })
    }
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
    upload_staging: Option<ResidentUploadStaging>,
    pub sh_degree: u8,
    pub report: ResidentEncodingReport,
}

/// Transactional exact-count encoder for loaders that produce one splat at a
/// time. At most one 256-splat source chunk is retained after construction;
/// completed chunks are immediately appended to the final resident planes.
#[derive(Debug)]
pub struct ResidentSceneBuilder {
    expected_count: usize,
    accepted_count: usize,
    sh_degree: u8,
    coeffs_per_channel: usize,
    sh_plane_count: usize,
    retain_positions: bool,
    pending: Vec<ResidentSourceSplat>,
    positions: Vec<Vec3f>,
    position_alpha: Vec<ResidentPositionAlpha>,
    covariance0: Vec<ResidentCovariance0>,
    covariance1: Vec<ResidentCovariance1>,
    color_aux: Vec<ResidentColorAux>,
    sh_planes: [Vec<ResidentShPlane>; RESIDENT_SH_PLANES],
    chunks: Vec<ResidentChunkMeta>,
    report: ResidentEncodingReport,
}

impl ResidentSceneBuilder {
    pub fn new(expected_count: usize, sh_degree: u8) -> Result<Self, ResidentSceneError> {
        Self::new_internal(expected_count, sh_degree, true)
    }

    fn new_internal(
        expected_count: usize,
        sh_degree: u8,
        retain_positions: bool,
    ) -> Result<Self, ResidentSceneError> {
        if sh_degree > 3 {
            return Err(ResidentSceneError::UnsupportedShDegree(sh_degree));
        }

        let coeffs_per_channel = sh_coeffs_per_channel(sh_degree);
        let sh_plane_count = resident_sh_plane_count(sh_degree);
        let chunk_count = expected_count.div_ceil(RESIDENT_CHUNK_SPLATS);

        let mut pending = Vec::new();
        try_reserve_exact(
            &mut pending,
            expected_count.min(RESIDENT_CHUNK_SPLATS),
            "resident source chunk",
        )?;
        let mut positions = Vec::new();
        if retain_positions {
            try_reserve_exact(&mut positions, expected_count, "resident CPU positions")?;
        }
        let mut position_alpha = Vec::new();
        try_reserve_exact(
            &mut position_alpha,
            expected_count,
            "resident position/alpha plane",
        )?;
        let mut covariance0 = Vec::new();
        try_reserve_exact(
            &mut covariance0,
            expected_count,
            "resident covariance 0 plane",
        )?;
        let mut color_aux = Vec::new();
        try_reserve_exact(
            &mut color_aux,
            expected_count,
            "resident color auxiliary plane",
        )?;
        let mut covariance1 = Vec::new();
        try_reserve_exact(
            &mut covariance1,
            expected_count,
            "resident covariance 1 plane",
        )?;
        let mut sh_planes = std::array::from_fn(|_| Vec::new());
        for (plane, destination) in sh_planes.iter_mut().enumerate().take(sh_plane_count) {
            let resource = match plane {
                0 => "resident SH plane 0",
                1 => "resident SH plane 1",
                2 => "resident SH plane 2",
                _ => "resident SH plane 3",
            };
            try_reserve_exact(destination, expected_count, resource)?;
        }
        let mut chunks = Vec::new();
        try_reserve_exact(&mut chunks, chunk_count, "resident chunk metadata")?;

        Ok(Self {
            expected_count,
            accepted_count: 0,
            sh_degree,
            coeffs_per_channel,
            sh_plane_count,
            retain_positions,
            pending,
            positions,
            position_alpha,
            covariance0,
            covariance1,
            color_aux,
            sh_planes,
            chunks,
            report: ResidentEncodingReport {
                source_count: expected_count,
                ..ResidentEncodingReport::default()
            },
        })
    }

    /// Accepts exactly one source splat without allocating per splat.
    /// Validation happens before the builder is mutated.
    pub fn push(&mut self, splat: ResidentSourceSplat) -> Result<(), ResidentSceneError> {
        let next_count = self
            .accepted_count
            .checked_add(1)
            .ok_or(ResidentSceneError::SizeOverflow)?;
        if next_count > self.expected_count {
            return Err(ResidentSceneError::CountMismatch {
                expected: self.expected_count,
                actual: next_count,
            });
        }
        self.validate_splat(&splat, self.accepted_count)?;

        self.pending.push(splat);
        self.accepted_count = next_count;
        if self.pending.len() == RESIDENT_CHUNK_SPLATS {
            self.flush_pending_chunk()?;
        }
        Ok(())
    }

    /// Completes the resident scene only when the declared source count was
    /// received. Partial scenes are never returned.
    pub fn finish(self) -> Result<ResidentSceneCpu, ResidentSceneError> {
        self.finish_internal(None)
    }

    fn finish_with_positions(
        self,
        positions: Vec<Vec3f>,
    ) -> Result<ResidentSceneCpu, ResidentSceneError> {
        self.finish_internal(Some(positions))
    }

    fn finish_internal(
        mut self,
        external_positions: Option<Vec<Vec3f>>,
    ) -> Result<ResidentSceneCpu, ResidentSceneError> {
        if self.accepted_count != self.expected_count {
            return Err(ResidentSceneError::CountMismatch {
                expected: self.expected_count,
                actual: self.accepted_count,
            });
        }
        self.flush_pending_chunk()?;

        let positions = match external_positions {
            Some(positions) if !self.retain_positions => {
                if positions.len() != self.expected_count {
                    return Err(ResidentSceneError::CountMismatch {
                        expected: self.expected_count,
                        actual: positions.len(),
                    });
                }
                positions
            }
            None if self.retain_positions => self.positions,
            _ => return Err(ResidentSceneError::InvalidScene),
        };
        self.report.chunk_count = self.chunks.len();

        let resident = ResidentSceneCpu {
            positions: Arc::from(positions.into_boxed_slice()),
            upload_staging: Some(ResidentUploadStaging {
                position_alpha: self.position_alpha,
                covariance0: self.covariance0,
                covariance1: self.covariance1,
                color_aux: self.color_aux,
                sh_planes: self.sh_planes,
                chunks: self.chunks,
            }),
            sh_degree: self.sh_degree,
            report: self.report,
        };
        resident.validate_complete()?;
        Ok(resident)
    }

    fn validate_splat(
        &self,
        splat: &ResidentSourceSplat,
        index: usize,
    ) -> Result<(), ResidentSceneError> {
        let expected_sh_len = self
            .coeffs_per_channel
            .checked_mul(3)
            .ok_or(ResidentSceneError::SizeOverflow)?;
        if splat.sh_degree != self.sh_degree {
            return Err(ResidentSceneError::InvalidSplat {
                index,
                field: "SH degree does not match the scene",
            });
        }
        if usize::from(splat.sh_len) != expected_sh_len {
            return Err(ResidentSceneError::InvalidSplat {
                index,
                field: "SH coefficient count does not match the degree",
            });
        }
        if !splat.position.is_finite() {
            return Err(ResidentSceneError::InvalidSplat {
                index,
                field: "position is not finite",
            });
        }
        if !splat.opacity_logit.is_finite() {
            return Err(ResidentSceneError::InvalidSplat {
                index,
                field: "opacity is not finite",
            });
        }
        if !splat.log_scale.iter().all(|value| value.is_finite()) {
            return Err(ResidentSceneError::InvalidSplat {
                index,
                field: "log scale is not finite",
            });
        }
        if !crate::log_scale_has_finite_nonzero_covariance(splat.log_scale) {
            return Err(ResidentSceneError::InvalidSplat {
                index,
                field: "log scale cannot be represented as a finite nonzero covariance",
            });
        }
        if !splat.rotation_xyzw.iter().all(|value| value.is_finite()) {
            return Err(ResidentSceneError::InvalidSplat {
                index,
                field: "rotation is not finite",
            });
        }
        if !crate::rotation_has_finite_nonzero_norm(splat.rotation_xyzw) {
            return Err(ResidentSceneError::InvalidSplat {
                index,
                field: "rotation has no finite nonzero norm",
            });
        }
        if !splat.color_dc.iter().all(|value| value.is_finite()) {
            return Err(ResidentSceneError::InvalidSplat {
                index,
                field: "DC color is not finite",
            });
        }
        if !splat.sh_rest[..expected_sh_len]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err(ResidentSceneError::InvalidSplat {
                index,
                field: "SH coefficient is not finite",
            });
        }
        Ok(())
    }

    fn flush_pending_chunk(&mut self) -> Result<(), ResidentSceneError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let encoded_after_chunk = self
            .report
            .encoded_count
            .checked_add(self.pending.len())
            .ok_or(ResidentSceneError::SizeOverflow)?;
        if encoded_after_chunk > self.expected_count {
            return Err(ResidentSceneError::CountMismatch {
                expected: self.expected_count,
                actual: encoded_after_chunk,
            });
        }

        let meta = build_chunk_meta(
            &self.pending,
            self.coeffs_per_channel,
            self.report.encoded_count,
        )?;
        self.chunks.push(meta);
        for splat in &self.pending {
            if self.retain_positions {
                self.positions.push(splat.position);
            }
            self.position_alpha.push(ResidentPositionAlpha {
                position_alpha: [
                    splat.position.x,
                    splat.position.y,
                    splat.position.z,
                    sigmoid(splat.opacity_logit),
                ],
            });

            let covariance =
                crate::world_covariance_from_source(splat.log_scale, splat.rotation_xyzw);
            let covariance0 = [
                covariance[0][0],
                covariance[0][1],
                covariance[0][2],
                covariance[1][1],
            ];
            let covariance1 = [covariance[1][2], covariance[2][2]];
            let dc_bits = pack_vec3_u16(splat.color_dc, meta.dc_min, meta.dc_extent);
            self.covariance0.push(ResidentCovariance0 {
                values: covariance0,
            });
            self.covariance1.push(ResidentCovariance1 {
                values: covariance1,
            });
            self.color_aux.push(ResidentColorAux { words: dc_bits });

            update_base_error_report(&mut self.report, splat.color_dc, &meta, dc_bits);

            pack_sh_planes(
                &splat.sh_rest[..usize::from(splat.sh_len)],
                self.coeffs_per_channel,
                self.sh_plane_count,
                &meta,
                &mut self.sh_planes,
                &mut self.report,
            );
        }
        self.report.encoded_count = encoded_after_chunk;
        self.pending.clear();
        Ok(())
    }
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
    fn decode_covariance(&self, index: usize) -> [f32; 6] {
        let staging = self.upload_staging().expect("decode requires staging");
        let first = staging.covariance0[index].values;
        let second = staging.covariance1[index].values;
        [first[0], first[1], first[2], first[3], second[0], second[1]]
    }

    #[cfg(test)]
    fn decode_dc(&self, index: usize) -> [f32; 3] {
        let staging = self.upload_staging().expect("decode requires staging");
        let meta = staging.chunks[index / RESIDENT_CHUNK_SPLATS];
        unpack_vec3_u16(staging.color_aux[index].words, meta.dc_min, meta.dc_extent)
    }

    #[cfg(test)]
    fn decode_sh_channel(&self, index: usize, channel: usize) -> Vec<f32> {
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

fn try_reserve_exact<T>(
    values: &mut Vec<T>,
    additional: usize,
    resource: &'static str,
) -> Result<(), ResidentSceneError> {
    let bytes = additional
        .checked_mul(std::mem::size_of::<T>())
        .ok_or(ResidentSceneError::SizeOverflow)?;
    if bytes > isize::MAX as usize {
        return Err(ResidentSceneError::SizeOverflow);
    }
    values
        .try_reserve_exact(additional)
        .map_err(|_| ResidentSceneError::AllocationFailed { resource })
}

fn push_scene_into_builder(
    builder: &mut ResidentSceneBuilder,
    scene: &SceneBuffers,
) -> Result<(), ResidentSceneError> {
    let coeffs_per_channel = sh_coeffs_per_channel(scene.sh_degree);
    let sh_len = coeffs_per_channel
        .checked_mul(3)
        .ok_or(ResidentSceneError::SizeOverflow)?;
    let sh_len_u8 = u8::try_from(sh_len).map_err(|_| ResidentSceneError::SizeOverflow)?;

    for index in 0..scene.len() {
        let mut sh_rest = [0.0_f32; 45];
        if sh_len > 0 {
            let source = scene
                .sh_rest
                .as_deref()
                .ok_or(ResidentSceneError::InvalidScene)?;
            let start = index
                .checked_mul(sh_len)
                .ok_or(ResidentSceneError::SizeOverflow)?;
            let end = start
                .checked_add(sh_len)
                .ok_or(ResidentSceneError::SizeOverflow)?;
            let coefficients = source
                .get(start..end)
                .ok_or(ResidentSceneError::InvalidScene)?;
            sh_rest[..sh_len].copy_from_slice(coefficients);
        }
        builder.push(ResidentSourceSplat {
            position: scene.positions[index],
            opacity_logit: scene.opacity[index],
            log_scale: scene.scale_xyz[index],
            rotation_xyzw: scene.rotation_xyzw[index],
            color_dc: scene.color_dc[index],
            sh_rest,
            sh_len: sh_len_u8,
            sh_degree: scene.sh_degree,
        })?;
    }
    Ok(())
}

pub const fn sh_coeffs_per_channel(degree: u8) -> usize {
    match degree {
        0 => 0,
        1 => 3,
        2 => 8,
        _ => 15,
    }
}

pub const fn resident_sh_plane_count(degree: u8) -> usize {
    let coeffs_per_channel = sh_coeffs_per_channel(degree);
    let value_count = coeffs_per_channel * 3;
    let active_band_count = if coeffs_per_channel == 0 {
        0
    } else if coeffs_per_channel <= 3 {
        1
    } else if coeffs_per_channel <= 8 {
        2
    } else {
        3
    };
    let bit_count =
        value_count * RESIDENT_SH_BITS + active_band_count * RESIDENT_SH_POINT_SCALE_BITS;
    let word_count = bit_count.div_ceil(32);
    word_count.div_ceil(RESIDENT_SH_WORDS_PER_PLANE)
}

fn build_chunk_meta(
    splats: &[ResidentSourceSplat],
    coeffs_per_channel: usize,
    source_start: usize,
) -> Result<ResidentChunkMeta, ResidentSceneError> {
    let mut dc_min = [f32::INFINITY; 3];
    let mut dc_max = [f32::NEG_INFINITY; 3];
    let mut sh_scales = [[0.0_f32; 3]; 3];
    for splat in splats {
        for axis in 0..3 {
            dc_min[axis] = dc_min[axis].min(splat.color_dc[axis]);
            dc_max[axis] = dc_max[axis].max(splat.color_dc[axis]);
        }
        if coeffs_per_channel > 0 {
            for (channel, coefficients) in splat.sh_rest[..usize::from(splat.sh_len)]
                .chunks_exact(coeffs_per_channel)
                .enumerate()
            {
                for (lane, value) in coefficients.iter().enumerate() {
                    let band = sh_band(lane);
                    sh_scales[band][channel] = sh_scales[band][channel].max(value.abs());
                }
            }
        }
    }

    let mut meta = ResidentChunkMeta::zeroed();
    for lane in 0..3 {
        let extent = dc_max[lane] - dc_min[lane];
        if !dc_min[lane].is_finite() || !dc_max[lane].is_finite() || !extent.is_finite() {
            return Err(ResidentSceneError::InvalidSplat {
                index: source_start,
                field: "chunk DC color range cannot be represented as finite f32",
            });
        }
        meta.dc_min[lane] = dc_min[lane];
        meta.dc_extent[lane] = extent.max(0.0);
        meta.sh_scale_l1[lane] = nonzero_sh_scale(sh_scales[0][lane]);
        meta.sh_scale_l2[lane] = nonzero_sh_scale(sh_scales[1][lane]);
        meta.sh_scale_l3[lane] = nonzero_sh_scale(sh_scales[2][lane]);
    }
    Ok(meta)
}

fn nonzero_sh_scale(value: f32) -> f32 {
    if value > 0.0 { value } else { 1.0 }
}

#[cfg(test)]
fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
fn finite_extent(min: f32, max: f32) -> f32 {
    let extent = max - min;
    if extent.is_finite() && extent > 0.0 {
        extent
    } else {
        0.0
    }
}

fn encode_u16(value: f32, min: f32, extent: f32) -> u32 {
    if extent <= 0.0 {
        return 0;
    }
    (((value - min) / extent).clamp(0.0, 1.0) * 65_535.0).round() as u32
}

fn pack_vec3_u16(value: [f32; 3], min: [f32; 4], extent: [f32; 4]) -> [u32; 2] {
    [
        encode_u16(value[0], min[0], extent[0]) | (encode_u16(value[1], min[1], extent[1]) << 16),
        encode_u16(value[2], min[2], extent[2]),
    ]
}

fn unpack_vec3_u16(bits: [u32; 2], min: [f32; 4], extent: [f32; 4]) -> [f32; 3] {
    [
        min[0] + f32::from((bits[0] & 0xffff) as u16) / 65_535.0 * extent[0],
        min[1] + f32::from((bits[0] >> 16) as u16) / 65_535.0 * extent[1],
        min[2] + f32::from((bits[1] & 0xffff) as u16) / 65_535.0 * extent[2],
    ]
}

fn pack_sh_planes(
    source: &[f32],
    coeffs_per_channel: usize,
    plane_count: usize,
    meta: &ResidentChunkMeta,
    destination: &mut [Vec<ResidentShPlane>; RESIDENT_SH_PLANES],
    report: &mut ResidentEncodingReport,
) {
    let mut words = [0_u32; RESIDENT_SH_PLANES * RESIDENT_SH_WORDS_PER_PLANE];
    let mut point_scale_codes = [0_u32; 3];
    for lane in 0..coeffs_per_channel {
        for channel in 0..3 {
            let source_index = channel * coeffs_per_channel + lane;
            let band = sh_band(lane);
            let chunk_scale = sh_band_scale(meta, channel, lane);
            let ratio = (source[source_index].abs() / chunk_scale).clamp(0.0, 1.0);
            point_scale_codes[band] = point_scale_codes[band].max(
                (ratio * RESIDENT_SH_POINT_SCALE_MAX as f32)
                    .ceil()
                    .clamp(0.0, RESIDENT_SH_POINT_SCALE_MAX as f32) as u32,
            );
        }
    }
    let active_band_count = usize::from(coeffs_per_channel > 0)
        + usize::from(coeffs_per_channel > 3)
        + usize::from(coeffs_per_channel > 8);
    let point_scale_bit_base = coeffs_per_channel * 3 * RESIDENT_SH_BITS;
    for (band, code) in point_scale_codes
        .iter()
        .copied()
        .enumerate()
        .take(active_band_count)
    {
        pack_unsigned_bits(
            &mut words,
            point_scale_bit_base + band * RESIDENT_SH_POINT_SCALE_BITS,
            RESIDENT_SH_POINT_SCALE_BITS,
            code,
        );
    }

    for lane in 0..coeffs_per_channel {
        for channel in 0..3 {
            let source_index = channel * coeffs_per_channel + lane;
            let band = sh_band(lane);
            let point_scale = point_scale_codes[band] as f32 / RESIDENT_SH_POINT_SCALE_MAX as f32;
            let scale = sh_band_scale(meta, channel, lane) * point_scale;
            if point_scale_codes[band] == 0 {
                debug_assert_eq!(source[source_index], 0.0);
                continue;
            }
            let encoded = (source[source_index] / scale * 1023.0)
                .round()
                .clamp(-1023.0, 1023.0) as i32;
            let logical_value = lane * 3 + channel;
            pack_signed_11(&mut words, logical_value, encoded);
            let decoded = encoded as f32 / 1023.0 * scale;
            report.max_sh_error_by_band[band] =
                report.max_sh_error_by_band[band].max((decoded - source[source_index]).abs());
        }
    }
    for (plane, output) in destination.iter_mut().enumerate().take(plane_count) {
        let start = plane * RESIDENT_SH_WORDS_PER_PLANE;
        output.push(ResidentShPlane {
            words: words[start..start + RESIDENT_SH_WORDS_PER_PLANE]
                .try_into()
                .expect("fixed resident SH plane width"),
        });
    }
}

fn pack_unsigned_bits(words: &mut [u32; 16], bit_offset: usize, bit_count: usize, value: u32) {
    debug_assert!(bit_count > 0 && bit_count < 32);
    debug_assert!(value < (1_u32 << bit_count));
    let word = bit_offset / 32;
    let shift = bit_offset % 32;
    words[word] |= value << shift;
    if shift + bit_count > 32 {
        words[word + 1] |= value >> (32 - shift);
    }
}

fn pack_signed_11(words: &mut [u32; 16], logical_value: usize, value: i32) {
    let bits = (value as u32) & 0x7ff;
    let bit_offset = logical_value * RESIDENT_SH_BITS;
    let word = bit_offset / 32;
    let shift = bit_offset % 32;
    words[word] |= bits << shift;
    if shift > 21 {
        words[word + 1] |= bits >> (32 - shift);
    }
}

#[cfg(test)]
fn unpack_signed_11(
    planes: &[Vec<ResidentShPlane>; RESIDENT_SH_PLANES],
    index: usize,
    logical_value: usize,
) -> i32 {
    let bit_offset = logical_value * RESIDENT_SH_BITS;
    let word_index = bit_offset / 32;
    let shift = bit_offset % 32;
    let load_word = |word_index: usize| {
        let plane = word_index / RESIDENT_SH_WORDS_PER_PLANE;
        let lane = word_index % RESIDENT_SH_WORDS_PER_PLANE;
        planes[plane][index].words[lane]
    };
    let mut bits = load_word(word_index) >> shift;
    if shift > 21 {
        bits |= load_word(word_index + 1) << (32 - shift);
    }
    ((bits & 0x7ff) << 21) as i32 >> 21
}

#[cfg(test)]
fn unpack_unsigned_bits(
    planes: &[Vec<ResidentShPlane>; RESIDENT_SH_PLANES],
    index: usize,
    bit_offset: usize,
    bit_count: usize,
) -> u32 {
    let word_index = bit_offset / 32;
    let shift = bit_offset % 32;
    let load_word = |word_index: usize| {
        let plane = word_index / RESIDENT_SH_WORDS_PER_PLANE;
        let lane = word_index % RESIDENT_SH_WORDS_PER_PLANE;
        planes[plane][index].words[lane]
    };
    let mut bits = load_word(word_index) >> shift;
    if shift + bit_count > 32 {
        bits |= load_word(word_index + 1) << (32 - shift);
    }
    bits & ((1_u32 << bit_count) - 1)
}

fn sh_band(lane: usize) -> usize {
    match lane {
        0..=2 => 0,
        3..=7 => 1,
        _ => 2,
    }
}

fn sh_band_scale(meta: &ResidentChunkMeta, channel: usize, lane: usize) -> f32 {
    match sh_band(lane) {
        0 => meta.sh_scale_l1[channel],
        1 => meta.sh_scale_l2[channel],
        _ => meta.sh_scale_l3[channel],
    }
}

#[allow(clippy::too_many_arguments)]
fn update_base_error_report(
    report: &mut ResidentEncodingReport,
    dc: [f32; 3],
    meta: &ResidentChunkMeta,
    dc_bits: [u32; 2],
) {
    let decoded_dc = unpack_vec3_u16(dc_bits, meta.dc_min, meta.dc_extent);
    for lane in 0..3 {
        report.max_dc_error = report.max_dc_error.max((decoded_dc[lane] - dc[lane]).abs());
    }
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(degree: u8, count: usize) -> SceneBuffers {
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

    fn source_splat(scene: &SceneBuffers, index: usize) -> ResidentSourceSplat {
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

    fn legacy_encode(scene: &SceneBuffers) -> ResidentSceneCpu {
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

    fn assert_resident_bits_eq(left: &ResidentSceneCpu, right: &ResidentSceneCpu) {
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
        let mut builder =
            ResidentSceneBuilder::new(source.len(), source.sh_degree).expect("builder");
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
        let expected: [i32; 45] =
            std::array::from_fn(|logical_value| logical_value as i32 * 31 - 700);
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
}
