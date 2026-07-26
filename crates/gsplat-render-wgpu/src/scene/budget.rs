use gsplat_core::Vec3f;

#[cfg(all(test, feature = "diagnostic-resident-sh-mantissa8"))]
use crate::data::RESIDENT_SH_PLANES;
use crate::data::{
    RESIDENT_CHUNK_SPLATS, ResidentChunkMeta, ResidentColorAux, ResidentCovariance0,
    ResidentCovariance1, ResidentPositionAlpha, ResidentShPlane,
};
use crate::gpu_error::ResidentGpuError;

use super::{ResidentSceneCpu, ResidentSceneError, resident_sh_plane_count};

pub(crate) const RESIDENT_COLOR_STORAGE_BINDINGS: u32 = 8;
pub(crate) const PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT: u64 = 16;
pub(crate) const PROJECTED_CACHE_BYTES_PER_SPLAT: u64 = 2 * PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT;
pub(crate) const PROJECT_WORKGROUP_SIZE: u32 = 128;
pub(crate) const SCAN_WORKGROUP_SIZE: u32 = 256;
pub(crate) const SCAN_ITEMS_PER_GROUP: u32 = SCAN_WORKGROUP_SIZE * 2;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResidentGpuBytePlan {
    pub position_alpha: u64,
    pub covariance0: u64,
    pub covariance1: u64,
    pub color_auxiliary: u64,
    pub sh_plane: u64,
    pub sh_plane_count: u32,
    pub chunk_metadata: u64,
    pub resolved_color: u64,
    pub order: u64,
    /// Rank-indexed center/alpha/source-ID plane used by ProjectedQuadsExact.
    pub projected_center_source: u64,
    /// Rank-indexed pair of screen-space ellipse axes used by ProjectedQuadsExact.
    pub projected_axes: u64,
    /// One count per projection workgroup plus the global contributor sentinel.
    pub projected_contributor_group_offsets: u64,
    /// Stable candidate-rank indirection used by exact compacted drawing.
    pub projected_contributor_ranks: u64,
    /// Sum of all hierarchical scan-sum buffer descriptors.
    pub projected_contributor_scan_sums: u64,
    /// Largest individual hierarchical scan-sum storage binding.
    pub projected_contributor_largest_scan_sum: u64,
    /// Dynamically aligned scan parameter buffer.
    pub projected_contributor_scan_params: u64,
    /// Independent four-word indirect draw arguments for contributors.
    pub projected_contributor_args: u64,
    pub total_static: u64,
}

struct ProjectedContributorBytePlan {
    group_offsets: u64,
    ranks: u64,
    scan_sums: u64,
    largest_scan_sum: u64,
    scan_params: u64,
    args: u64,
}

fn projected_contributor_byte_plan(
    splat_count: u64,
) -> Result<ProjectedContributorBytePlan, ResidentGpuError> {
    let word_bytes = std::mem::size_of::<u32>() as u64;
    let project_groups = splat_count.div_ceil(u64::from(PROJECT_WORKGROUP_SIZE));
    let offset_count = project_groups
        .checked_add(1)
        .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
    let group_offsets = offset_count
        .checked_mul(word_bytes)
        .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
    let ranks = splat_count
        .checked_mul(word_bytes)
        .ok_or(ResidentGpuError::AddressSpaceExceeded)?;

    let mut scan_count = offset_count;
    let mut scan_sums = 0_u64;
    let mut largest_scan_sum = 0_u64;
    let mut scan_levels = 0_u64;
    loop {
        let groups = scan_count.div_ceil(u64::from(SCAN_ITEMS_PER_GROUP)).max(1);
        let bytes = groups
            .checked_mul(word_bytes)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        scan_sums = scan_sums
            .checked_add(bytes)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        largest_scan_sum = largest_scan_sum.max(bytes);
        scan_levels = scan_levels
            .checked_add(1)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        if groups == 1 {
            break;
        }
        scan_count = groups;
    }
    // Surface devices request downlevel defaults; their dynamic-uniform
    // alignment is therefore the portable 256-byte stride used by the actual
    // compactor (or a smaller physical alignment rounded up to this request).
    let scan_param_stride = u64::from(
        wgpu::Limits::downlevel_defaults()
            .min_uniform_buffer_offset_alignment
            .max(16),
    );
    let scan_params = scan_levels
        .checked_mul(scan_param_stride)
        .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
    Ok(ProjectedContributorBytePlan {
        group_offsets,
        ranks,
        scan_sums,
        largest_scan_sum,
        scan_params,
        args: std::mem::size_of::<[u32; 4]>() as u64,
    })
}

impl ResidentGpuBytePlan {
    pub fn for_scene(scene: &ResidentSceneCpu) -> Result<Self, ResidentGpuError> {
        Self::for_count(scene.len(), scene.sh_plane_count())
    }

    pub fn for_count(splat_count: usize, sh_plane_count: u32) -> Result<Self, ResidentGpuError> {
        if !(0..=3).any(|degree| resident_sh_plane_count(degree) as u32 == sh_plane_count) {
            return Err(ResidentGpuError::UnsupportedShPlaneCount(sh_plane_count));
        }
        let n = u64::try_from(splat_count).map_err(|_| ResidentGpuError::AddressSpaceExceeded)?;
        let chunks = n.div_ceil(RESIDENT_CHUNK_SPLATS as u64);
        let position_alpha = n
            .checked_mul(std::mem::size_of::<ResidentPositionAlpha>() as u64)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let covariance0 = n
            .checked_mul(std::mem::size_of::<ResidentCovariance0>() as u64)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let covariance1 = n
            .checked_mul(std::mem::size_of::<ResidentCovariance1>() as u64)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let color_auxiliary = n
            .checked_mul(std::mem::size_of::<ResidentColorAux>() as u64)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let sh_plane = n
            .checked_mul(std::mem::size_of::<ResidentShPlane>() as u64)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let chunk_metadata = chunks
            .checked_mul(std::mem::size_of::<ResidentChunkMeta>() as u64)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let resolved_color = n
            .checked_mul(8)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let order = n
            .checked_mul(4)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let projected_center_source = n
            .checked_mul(PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let projected_axes = n
            .checked_mul(PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let projected_cache = n
            .checked_mul(PROJECTED_CACHE_BYTES_PER_SPLAT)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let contributor = projected_contributor_byte_plan(n)?;
        let total_static = position_alpha
            .checked_add(covariance0)
            .and_then(|bytes| bytes.checked_add(covariance1))
            .and_then(|bytes| bytes.checked_add(color_auxiliary))
            .and_then(|bytes| bytes.checked_add(sh_plane.checked_mul(u64::from(sh_plane_count))?))
            .and_then(|bytes| bytes.checked_add(chunk_metadata))
            .and_then(|bytes| bytes.checked_add(resolved_color))
            .and_then(|bytes| bytes.checked_add(order))
            .and_then(|bytes| bytes.checked_add(projected_cache))
            .and_then(|bytes| bytes.checked_add(contributor.group_offsets))
            .and_then(|bytes| bytes.checked_add(contributor.ranks))
            .and_then(|bytes| bytes.checked_add(contributor.scan_sums))
            .and_then(|bytes| bytes.checked_add(contributor.scan_params))
            .and_then(|bytes| bytes.checked_add(contributor.args))
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        Ok(Self {
            position_alpha,
            covariance0,
            covariance1,
            color_auxiliary,
            sh_plane,
            sh_plane_count,
            chunk_metadata,
            resolved_color,
            order,
            projected_center_source,
            projected_axes,
            projected_contributor_group_offsets: contributor.group_offsets,
            projected_contributor_ranks: contributor.ranks,
            projected_contributor_scan_sums: contributor.scan_sums,
            projected_contributor_largest_scan_sum: contributor.largest_scan_sum,
            projected_contributor_scan_params: contributor.scan_params,
            projected_contributor_args: contributor.args,
            total_static,
        })
    }

    pub fn validate_limits(self, limits: &wgpu::Limits) -> Result<Self, ResidentGpuError> {
        if limits.max_storage_buffers_per_shader_stage < RESIDENT_COLOR_STORAGE_BINDINGS {
            return Err(ResidentGpuError::StorageBindingCountUnsupported(
                limits.max_storage_buffers_per_shader_stage,
            ));
        }
        let binding_limit =
            u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size);
        for (resource, bytes, minimum_bytes) in [
            ("position+alpha", self.position_alpha, 16),
            ("covariance 0", self.covariance0, 16),
            ("covariance 1", self.covariance1, 8),
            ("color auxiliary", self.color_auxiliary, 8),
            (
                "SH plane",
                if self.sh_plane_count == 0 {
                    0
                } else {
                    self.sh_plane
                },
                16,
            ),
            (
                "chunk metadata",
                self.chunk_metadata,
                std::mem::size_of::<ResidentChunkMeta>() as u64,
            ),
            ("resolved color", self.resolved_color, 8),
            ("draw order", self.order, 4),
            (
                "projected center/source plane",
                self.projected_center_source,
                16,
            ),
            ("projected axes plane", self.projected_axes, 16),
            (
                "projected contributor group offsets",
                self.projected_contributor_group_offsets,
                4,
            ),
            (
                "projected contributor ranks",
                self.projected_contributor_ranks,
                4,
            ),
            (
                "projected contributor scan sums",
                self.projected_contributor_largest_scan_sum,
                4,
            ),
            (
                "projected contributor args",
                self.projected_contributor_args,
                16,
            ),
        ] {
            let required_bytes = bytes.max(minimum_bytes);
            if required_bytes > binding_limit {
                return Err(ResidentGpuError::BindingLimitExceeded {
                    resource,
                    required_bytes,
                    limit_bytes: binding_limit,
                });
            }
        }
        if self.projected_contributor_scan_params > limits.max_buffer_size {
            return Err(ResidentGpuError::BindingLimitExceeded {
                resource: "projected contributor scan params",
                required_bytes: self.projected_contributor_scan_params,
                limit_bytes: limits.max_buffer_size,
            });
        }
        Ok(self)
    }

    /// Largest storage-buffer descriptor created by the resident path.
    ///
    /// Empty logical planes still receive their minimum non-zero descriptor
    /// size because wgpu rejects zero-sized buffers. Inactive SH planes are
    /// therefore 16-byte bindings, not `16 * splat_count` bindings.
    pub const fn largest_storage_binding_bytes(self) -> u64 {
        let sh_plane = if self.sh_plane_count == 0 {
            16
        } else {
            max_u64(self.sh_plane, 16)
        };
        max_u64(
            max_u64(self.position_alpha, 16),
            max_u64(
                max_u64(self.covariance0, 16),
                max_u64(
                    max_u64(self.covariance1, 8),
                    max_u64(
                        max_u64(self.color_auxiliary, 8),
                        max_u64(
                            sh_plane,
                            max_u64(
                                max_u64(
                                    self.chunk_metadata,
                                    std::mem::size_of::<ResidentChunkMeta>() as u64,
                                ),
                                max_u64(
                                    max_u64(self.resolved_color, 8),
                                    max_u64(
                                        max_u64(self.order, 4),
                                        max_u64(
                                            max_u64(self.projected_center_source, 16),
                                            max_u64(
                                                max_u64(self.projected_axes, 16),
                                                max_u64(
                                                    max_u64(
                                                        self.projected_contributor_group_offsets,
                                                        4,
                                                    ),
                                                    max_u64(
                                                        max_u64(
                                                            self.projected_contributor_ranks,
                                                            4,
                                                        ),
                                                        max_u64(
                                                            max_u64(
                                                                self.projected_contributor_largest_scan_sum,
                                                                4,
                                                            ),
                                                            max_u64(
                                                                self.projected_contributor_args,
                                                                16,
                                                            ),
                                                        ),
                                                    ),
                                                ),
                                            ),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        )
    }

    /// Bytes requested from wgpu for the four fixed SH bindings, including
    /// one 16-byte placeholder for every inactive logical plane.
    #[cfg(all(test, feature = "diagnostic-resident-sh-mantissa8"))]
    pub(crate) const fn allocated_sh_buffer_bytes(self) -> u64 {
        self.sh_plane * self.sh_plane_count as u64
            + 16 * (RESIDENT_SH_PLANES as u64 - self.sh_plane_count as u64)
    }
}

const fn max_u64(left: u64, right: u64) -> u64 {
    if left > right { left } else { right }
}
