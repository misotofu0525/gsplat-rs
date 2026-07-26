use std::sync::Arc;

use gsplat_core::{SceneBuffers, Vec3f};

use crate::data::{
    RESIDENT_CHUNK_SPLATS, RESIDENT_SH_PLANES, ResidentChunkMeta, ResidentColorAux,
    ResidentCovariance0, ResidentCovariance1, ResidentPositionAlpha, ResidentShPlane,
};

use super::codec::{
    build_chunk_meta, pack_sh_planes, pack_vec3_u16, resident_sh_plane_count,
    sh_coeffs_per_channel, sigmoid, update_base_error_report,
};
use super::resident::{
    ResidentEncodingReport, ResidentSceneCpu, ResidentSceneError, ResidentShEncodingDiagnostic,
    ResidentSourceSplat, ResidentUploadStaging,
};

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
    pub(super) pending: Vec<ResidentSourceSplat>,
    positions: Vec<Vec3f>,
    position_alpha: Vec<ResidentPositionAlpha>,
    covariance0: Vec<ResidentCovariance0>,
    covariance1: Vec<ResidentCovariance1>,
    color_aux: Vec<ResidentColorAux>,
    sh_planes: [Vec<ResidentShPlane>; RESIDENT_SH_PLANES],
    pub(super) chunks: Vec<ResidentChunkMeta>,
    report: ResidentEncodingReport,
    sh_encoding_diagnostic: ResidentShEncodingDiagnostic,
}

impl ResidentSceneBuilder {
    pub fn new(expected_count: usize, sh_degree: u8) -> Result<Self, ResidentSceneError> {
        Self::new_internal(expected_count, sh_degree, true)
    }

    pub(super) fn new_internal(
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
            sh_encoding_diagnostic: ResidentShEncodingDiagnostic::configured(),
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

    pub(super) fn finish_with_positions(
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
            sh_encoding_diagnostic: self.sh_encoding_diagnostic,
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
                &mut self.sh_encoding_diagnostic,
            );
        }
        self.report.encoded_count = encoded_after_chunk;
        self.pending.clear();
        Ok(())
    }
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

pub(super) fn push_scene_into_builder(
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
