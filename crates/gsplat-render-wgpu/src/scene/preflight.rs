use thiserror::Error;

use crate::data::GpuSurfaceSourceElem;

use super::{RESIDENT_COLOR_STORAGE_BINDINGS, ResidentGpuBytePlan, resident_sh_plane_count};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectSceneResource {
    SortedIndices,
    Source,
    ShRest,
    DrawInstances,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectSceneResourceRequirement {
    pub resource: DirectSceneResource,
    pub required_bytes: u64,
    pub limit_bytes: u64,
    pub fits: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectScenePath {
    Direct,
    ActiveAtlasRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectSceneRemediation {
    None,
    UseActiveAtlasOrReduce { max_direct_splats: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectScenePreflight {
    pub splat_count: u64,
    pub sh_degree: u8,
    pub path: DirectScenePath,
    pub effective_storage_binding_limit: u64,
    pub effective_max_buffer_size: u64,
    pub requirements: [DirectSceneResourceRequirement; 3],
    pub limiting_resource: DirectSceneResource,
    pub max_direct_splats: u64,
    pub remediation: DirectSceneRemediation,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DirectSceneError {
    #[error("sorted index buffer capacity exceeded")]
    SortedIndexCapacityExceeded,
    #[error("gpu order initialization failed: {0}")]
    GpuOrderInitialization(String),
    #[error("direct scene resource size overflow")]
    ResourceSizeOverflow,
    #[error("direct scene resources exceed effective device limits: {0:?}")]
    ResourceLimitExceeded(Box<DirectScenePreflight>),
    #[error("packed scene resources require paging or exceed effective device limits: {0:?}")]
    PackedResourceLimitExceeded(Box<PackedScenePreflight>),
    #[error(
        "paged compact atlas requires {hot_record_storage_bytes} hot-record bytes and {sorted_indices_bytes} order bytes, exceeding the effective {limit_bytes}-byte binding limit"
    )]
    PagedAtlasResourceLimitExceeded {
        sorted_indices_bytes: u64,
        hot_record_storage_bytes: u64,
        limit_bytes: u64,
    },
}

pub fn direct_scene_preflight(
    splat_count: usize,
    sh_degree: u8,
    limits: &wgpu::Limits,
) -> Result<DirectScenePreflight, DirectSceneError> {
    let splat_count =
        u64::try_from(splat_count).map_err(|_| DirectSceneError::ResourceSizeOverflow)?;
    let capacity = splat_count.max(1);
    let binding_limit = u64::from(limits.max_storage_buffer_binding_size);
    let buffer_limit = limits.max_buffer_size;
    let effective_limit = binding_limit.min(buffer_limit);

    let order_stride = std::mem::size_of::<u32>() as u64;
    let source_stride = std::mem::size_of::<GpuSurfaceSourceElem>() as u64;
    let degree = u64::from(sh_degree);
    let sh_stride = degree
        .checked_add(1)
        .and_then(|value| value.checked_mul(value))
        .and_then(|value| value.checked_sub(1))
        .and_then(|value| value.checked_mul(3))
        .and_then(|value| value.checked_mul(std::mem::size_of::<f32>() as u64))
        .ok_or(DirectSceneError::ResourceSizeOverflow)?;

    let order_bytes = capacity
        .checked_mul(order_stride)
        .ok_or(DirectSceneError::ResourceSizeOverflow)?;
    let source_bytes = capacity
        .checked_mul(source_stride)
        .ok_or(DirectSceneError::ResourceSizeOverflow)?;
    let sh_bytes = if sh_stride == 0 {
        std::mem::size_of::<f32>() as u64
    } else {
        splat_count
            .checked_mul(sh_stride)
            .ok_or(DirectSceneError::ResourceSizeOverflow)?
    };

    let requirements = [
        DirectSceneResourceRequirement {
            resource: DirectSceneResource::SortedIndices,
            required_bytes: order_bytes,
            limit_bytes: effective_limit,
            fits: order_bytes <= effective_limit,
        },
        DirectSceneResourceRequirement {
            resource: DirectSceneResource::Source,
            required_bytes: source_bytes,
            limit_bytes: effective_limit,
            fits: source_bytes <= effective_limit,
        },
        DirectSceneResourceRequirement {
            resource: DirectSceneResource::ShRest,
            required_bytes: sh_bytes,
            limit_bytes: effective_limit,
            fits: sh_bytes <= effective_limit,
        },
    ];

    let capacities = [
        (
            DirectSceneResource::SortedIndices,
            effective_limit / order_stride,
        ),
        (DirectSceneResource::Source, effective_limit / source_stride),
        (
            DirectSceneResource::ShRest,
            if sh_stride == 0 {
                u64::MAX
            } else {
                effective_limit / sh_stride
            },
        ),
        (DirectSceneResource::DrawInstances, u64::from(u32::MAX)),
    ];
    let (limiting_resource, max_direct_splats) = capacities
        .into_iter()
        .min_by_key(|(_, capacity)| *capacity)
        .expect("direct resource capacity list is non-empty");
    let fits = requirements.iter().all(|requirement| requirement.fits)
        && splat_count <= u64::from(u32::MAX);
    let path = if fits {
        DirectScenePath::Direct
    } else {
        DirectScenePath::ActiveAtlasRequired
    };
    let remediation = if fits {
        DirectSceneRemediation::None
    } else {
        DirectSceneRemediation::UseActiveAtlasOrReduce { max_direct_splats }
    };

    Ok(DirectScenePreflight {
        splat_count,
        sh_degree,
        path,
        effective_storage_binding_limit: binding_limit,
        effective_max_buffer_size: buffer_limit,
        requirements,
        limiting_resource,
        max_direct_splats,
        remediation,
    })
}

/// Whether the complete resident Packed representation fits the supplied
/// device limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackedScenePath {
    PackedAtlas,
    /// The complete resident representation does not fit the supplied device
    /// limits. Production Packed loading must return the structured preflight
    /// failure; it never silently switches to partial paging.
    PagingRequired,
}

/// Device limits relevant to the complete resident Packed representation.
///
/// `From<u64>` preserves the original public preflight call shape: the value
/// is treated as both the storage-binding and buffer-size limit, with the
/// portable eight storage bindings available. New code should call
/// [`packed_scene_preflight_with_limits`] so every independently relevant
/// limit is reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedScenePreflightLimits {
    pub max_storage_buffer_binding_size: u64,
    pub max_buffer_size: u64,
    pub max_storage_buffers_per_shader_stage: u32,
}

impl From<u64> for PackedScenePreflightLimits {
    fn from(effective_binding_limit: u64) -> Self {
        Self {
            max_storage_buffer_binding_size: effective_binding_limit,
            max_buffer_size: effective_binding_limit,
            max_storage_buffers_per_shader_stage: RESIDENT_COLOR_STORAGE_BINDINGS,
        }
    }
}

impl From<&wgpu::Limits> for PackedScenePreflightLimits {
    fn from(limits: &wgpu::Limits) -> Self {
        Self {
            max_storage_buffer_binding_size: u64::from(limits.max_storage_buffer_binding_size),
            max_buffer_size: limits.max_buffer_size,
            max_storage_buffers_per_shader_stage: limits.max_storage_buffers_per_shader_stage,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackedScenePreflightFailure {
    UnsupportedShDegree {
        requested: u8,
        maximum: u8,
    },
    StorageBindingCount {
        required: u32,
        available: u32,
    },
    StorageBindingSize {
        required_bytes: u64,
        limit_bytes: u64,
    },
    DrawInstanceCount {
        required: u64,
        maximum: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedScenePreflight {
    pub splat_count: u64,
    pub sh_degree: u8,
    pub path: PackedScenePath,
    /// Exact logical bytes for every independently allocated resident plane.
    pub resident_gpu: ResidentGpuBytePlan,
    pub effective_storage_binding_limit: u64,
    pub required_storage_buffers_per_shader_stage: u32,
    pub available_storage_buffers_per_shader_stage: u32,
    pub largest_storage_binding_bytes: u64,
    pub storage_binding_size_fits: bool,
    pub storage_binding_count_fits: bool,
    pub draw_instance_count_fits: bool,
    pub failure: Option<PackedScenePreflightFailure>,
    // Compatibility fields retained for callers of the former hot-record
    // preflight. They now describe the final resident representation; no
    // production Packed hot record exists.
    pub sorted_indices_bytes: u64,
    /// Total logical resident bytes other than the draw-order plane.
    pub declared_attribute_resource_bytes: u64,
    /// Compatibility alias for [`Self::largest_storage_binding_bytes`].
    pub hot_record_storage_bytes: u64,
    pub sorted_indices_fits_storage_binding: bool,
    /// Compatibility alias for [`Self::storage_binding_size_fits`].
    pub hot_record_fits_storage_binding: bool,
    /// Always false for the final representation: attributes intentionally use
    /// several bounded storage planes instead of one monolithic binding.
    pub attributes_avoid_storage_binding: bool,
}

/// Complete resident Packed-path resource preflight without allocating scene
/// data or silently changing the requested geometry path.
///
/// This retains the original `u64` call shape. The supplied value is treated
/// as the effective storage-buffer limit and the portable eight storage
/// bindings are assumed. Call [`packed_scene_preflight_with_limits`] when the
/// binding-size, buffer-size, and binding-count limits are available
/// separately.
pub fn packed_scene_preflight(
    splat_count: usize,
    sh_degree: u8,
    effective_storage_binding_limit: u64,
) -> Result<PackedScenePreflight, DirectSceneError> {
    packed_scene_preflight_for_limits(
        splat_count,
        sh_degree,
        effective_storage_binding_limit.into(),
    )
}

/// Complete resident Packed preflight using all relevant wgpu limits.
pub fn packed_scene_preflight_with_limits(
    splat_count: usize,
    sh_degree: u8,
    limits: &wgpu::Limits,
) -> Result<PackedScenePreflight, DirectSceneError> {
    packed_scene_preflight_for_limits(splat_count, sh_degree, limits.into())
}

fn packed_scene_preflight_for_limits(
    splat_count: usize,
    sh_degree: u8,
    limits: PackedScenePreflightLimits,
) -> Result<PackedScenePreflight, DirectSceneError> {
    let splat_count_u64 =
        u64::try_from(splat_count).map_err(|_| DirectSceneError::ResourceSizeOverflow)?;
    let resident_gpu =
        ResidentGpuBytePlan::for_count(splat_count, resident_sh_plane_count(sh_degree) as u32)
            .map_err(|_| DirectSceneError::ResourceSizeOverflow)?;
    let effective_storage_binding_limit = limits
        .max_storage_buffer_binding_size
        .min(limits.max_buffer_size);
    let largest_storage_binding_bytes = resident_gpu.largest_storage_binding_bytes();
    let required_storage_buffers_per_shader_stage = RESIDENT_COLOR_STORAGE_BINDINGS;
    let storage_binding_size_fits =
        largest_storage_binding_bytes <= effective_storage_binding_limit;
    let storage_binding_count_fits =
        limits.max_storage_buffers_per_shader_stage >= required_storage_buffers_per_shader_stage;
    let draw_instance_count_fits = splat_count_u64 <= u64::from(u32::MAX);
    let failure = if sh_degree > 3 {
        Some(PackedScenePreflightFailure::UnsupportedShDegree {
            requested: sh_degree,
            maximum: 3,
        })
    } else if !storage_binding_count_fits {
        Some(PackedScenePreflightFailure::StorageBindingCount {
            required: required_storage_buffers_per_shader_stage,
            available: limits.max_storage_buffers_per_shader_stage,
        })
    } else if !storage_binding_size_fits {
        Some(PackedScenePreflightFailure::StorageBindingSize {
            required_bytes: largest_storage_binding_bytes,
            limit_bytes: effective_storage_binding_limit,
        })
    } else if !draw_instance_count_fits {
        Some(PackedScenePreflightFailure::DrawInstanceCount {
            required: splat_count_u64,
            maximum: u64::from(u32::MAX),
        })
    } else {
        None
    };
    let sorted_indices_bytes = resident_gpu.order.max(std::mem::size_of::<u32>() as u64);
    let declared_attribute_resource_bytes = resident_gpu
        .total_static
        .checked_sub(resident_gpu.order)
        .ok_or(DirectSceneError::ResourceSizeOverflow)?;
    Ok(PackedScenePreflight {
        splat_count: splat_count_u64,
        sh_degree,
        path: if failure.is_none() {
            PackedScenePath::PackedAtlas
        } else {
            PackedScenePath::PagingRequired
        },
        resident_gpu,
        effective_storage_binding_limit,
        required_storage_buffers_per_shader_stage,
        available_storage_buffers_per_shader_stage: limits.max_storage_buffers_per_shader_stage,
        largest_storage_binding_bytes,
        storage_binding_size_fits,
        storage_binding_count_fits,
        draw_instance_count_fits,
        failure,
        sorted_indices_bytes,
        declared_attribute_resource_bytes,
        hot_record_storage_bytes: largest_storage_binding_bytes,
        sorted_indices_fits_storage_binding: sorted_indices_bytes
            <= effective_storage_binding_limit,
        hot_record_fits_storage_binding: storage_binding_size_fits,
        attributes_avoid_storage_binding: false,
    })
}
