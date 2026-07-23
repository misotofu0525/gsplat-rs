//! GPU resources for the exact-count compact resident scene.

use bytemuck::{Pod, Zeroable};
use gsplat_core::Camera;
use thiserror::Error;
use wgpu::util::DeviceExt;

use crate::data::{
    RESIDENT_CHUNK_SPLATS, RESIDENT_SH_PLANES, ResidentChunkMeta, ResidentColorAux,
    ResidentCovariance0, ResidentCovariance1, ResidentPositionAlpha, ResidentShPlane,
};
use crate::direct_gpu_order::DirectGpuOrder;
use crate::draw_pass::{SplatPipeline, create_splat_bind_group_layout, create_splat_pipeline};
use crate::scene::ResidentSceneCpu;
use crate::{GpuSurfaceRenderParams, make_surface_render_params, wgpu_label};

pub const RESIDENT_QUAD_VERTEX_COUNT: u32 = 4;
pub const RESIDENT_COLOR_STORAGE_BINDINGS: u32 = 8;
const COLOR_WORKGROUP_SIZE: u32 = 128;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResidentGpuError {
    #[error("resident scene is incomplete")]
    IncompleteScene,
    #[error("resident upload staging has already been released")]
    UploadStagingUnavailable,
    #[error("resident scene exceeds u32 addressing")]
    AddressSpaceExceeded,
    #[error("resident SH plane count must be one of 0, 1, 3, or 4; got {0}")]
    UnsupportedShPlaneCount(u32),
    #[error(
        "resident resource {resource} requires {required_bytes} bytes but the effective binding limit is {limit_bytes} bytes"
    )]
    BindingLimitExceeded {
        resource: &'static str,
        required_bytes: u64,
        limit_bytes: u64,
    },
    #[error("resident color resolve needs eight compute storage buffers, device exposes {0}")]
    StorageBindingCountUnsupported(u32),
    #[error("resident dispatch exceeds device workgroup dimensions")]
    DispatchLimitExceeded,
    #[error("resident order upload exceeds scene capacity")]
    OrderCapacityExceeded,
    #[error("resident GPU ordering initialization failed: {0}")]
    GpuOrderInitialization(String),
    #[error("resident GPU ordering allocation failed: {0}")]
    GpuOrderOutOfMemory(String),
    #[error("resident GPU ordering validation failed: {0}")]
    GpuOrderValidation(String),
    #[error("resident GPU ordering backend failed: {0}")]
    GpuOrderInternal(String),
}

#[cfg(any(not(target_arch = "wasm32"), test))]
fn classify_gpu_order_scope_errors(
    internal: Option<String>,
    out_of_memory: Option<String>,
    validation: Option<String>,
) -> Option<ResidentGpuError> {
    out_of_memory
        .map(ResidentGpuError::GpuOrderOutOfMemory)
        .or_else(|| internal.map(ResidentGpuError::GpuOrderInternal))
        .or_else(|| validation.map(ResidentGpuError::GpuOrderValidation))
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuResidentColorParams {
    camera_pos: [f32; 4],
    len: u32,
    sh_degree: u32,
    _reserved0: u32,
    _pad: u32,
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
    let project_groups = splat_count.div_ceil(u64::from(
        crate::projected_quads_gpu::PROJECT_WORKGROUP_SIZE,
    ));
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
        let groups = scan_count
            .div_ceil(u64::from(crate::projected_quads_gpu::SCAN_ITEMS_PER_GROUP))
            .max(1);
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
        if !matches!(sh_plane_count, 0 | 1 | 3 | 4) {
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
            .checked_mul(crate::projected_quads_gpu::PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let projected_axes = n
            .checked_mul(crate::projected_quads_gpu::PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let projected_cache = n
            .checked_mul(crate::projected_quads_gpu::PROJECTED_CACHE_BYTES_PER_SPLAT)
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
}

const fn max_u64(left: u64, right: u64) -> u64 {
    if left > right { left } else { right }
}

pub struct ResidentGpuResources {
    pub order_buffer: wgpu::Buffer,
    pub position_alpha_buffer: wgpu::Buffer,
    pub covariance0_buffer: wgpu::Buffer,
    pub covariance1_buffer: wgpu::Buffer,
    // Retained because the color bind group references this plane.
    _color_auxiliary_buffer: wgpu::Buffer,
    // Retained because the color bind group references all four planes.
    _sh_buffers: [wgpu::Buffer; RESIDENT_SH_PLANES],
    // Retained because the color bind group references chunk-local DC/SH ranges.
    _chunk_metadata_buffer: wgpu::Buffer,
    pub resolved_color_buffer: wgpu::Buffer,
    pub draw_params_buffer: wgpu::Buffer,
    color_params_buffer: wgpu::Buffer,
    pub draw_bind_group: wgpu::BindGroup,
    color_bind_group: wgpu::BindGroup,
    pub capacity: usize,
    pub sh_degree: u32,
    _byte_plan: ResidentGpuBytePlan,
    last_resolved_camera_position: Option<[f32; 3]>,
    gpu_order: Option<ResidentGpuSceneOrder>,
}

pub(crate) struct ResidentGpuOrderDraw<'a> {
    pub(crate) camera: &'a Camera,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) instance_count: u32,
    pub(crate) order_stride_words: u32,
    pub(crate) order_id_offset_words: u32,
}

pub(crate) struct ResidentGpuSceneOrder {
    pub(crate) sorter: DirectGpuOrder,
    pub(crate) draw_bind_group: wgpu::BindGroup,
}

impl ResidentGpuResources {
    pub fn new(
        device: &wgpu::Device,
        draw_layout: &wgpu::BindGroupLayout,
        color_layout: &wgpu::BindGroupLayout,
        scene: &ResidentSceneCpu,
    ) -> Result<Self, ResidentGpuError> {
        scene.validate_complete().map_err(|error| match error {
            crate::ResidentSceneError::UploadStagingReleased => {
                ResidentGpuError::UploadStagingUnavailable
            }
            _ => ResidentGpuError::IncompleteScene,
        })?;
        let staging = scene
            .upload_staging()
            .map_err(|_| ResidentGpuError::UploadStagingUnavailable)?;
        let byte_plan = ResidentGpuBytePlan::for_scene(scene)?.validate_limits(&device.limits())?;
        let capacity = scene.len();
        let _ = u32::try_from(capacity).map_err(|_| ResidentGpuError::AddressSpaceExceeded)?;

        let order_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-resident-order"),
            size: byte_plan.order.max(4),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let position_alpha_buffer = create_storage_init(
            device,
            "gsplat-resident-position-alpha",
            bytemuck::cast_slice(&staging.position_alpha),
            16,
        );
        let covariance0_buffer = create_storage_init(
            device,
            "gsplat-resident-covariance-0",
            bytemuck::cast_slice(&staging.covariance0),
            16,
        );
        let covariance1_buffer = create_storage_init(
            device,
            "gsplat-resident-covariance-1",
            bytemuck::cast_slice(&staging.covariance1),
            8,
        );
        let color_auxiliary_buffer = create_storage_init(
            device,
            "gsplat-resident-color-auxiliary",
            bytemuck::cast_slice(&staging.color_aux),
            8,
        );
        let sh_buffers = std::array::from_fn(|plane| {
            create_storage_init(
                device,
                match plane {
                    0 => "gsplat-resident-sh-0",
                    1 => "gsplat-resident-sh-1",
                    2 => "gsplat-resident-sh-2",
                    _ => "gsplat-resident-sh-3",
                },
                bytemuck::cast_slice(&staging.sh_planes[plane]),
                16,
            )
        });
        let chunk_metadata_buffer = create_storage_init(
            device,
            "gsplat-resident-chunk-metadata",
            bytemuck::cast_slice(&staging.chunks),
            std::mem::size_of::<ResidentChunkMeta>(),
        );
        let resolved_color_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-resident-resolved-color"),
            size: byte_plan.resolved_color.max(8),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let draw_params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-resident-draw-params"),
            contents: bytemuck::bytes_of(&GpuSurfaceRenderParams::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let color_params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-resident-color-params"),
            contents: bytemuck::bytes_of(&GpuResidentColorParams::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let draw_bind_group = create_resident_draw_bind_group(
            device,
            draw_layout,
            "gsplat-resident-draw-bind-group",
            &order_buffer,
            &position_alpha_buffer,
            &covariance0_buffer,
            &covariance1_buffer,
            &resolved_color_buffer,
            &draw_params_buffer,
        );
        let color_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-resident-color-bind-group"),
            layout: color_layout,
            entries: &[
                entry(0, &position_alpha_buffer),
                entry(1, &color_auxiliary_buffer),
                entry(2, &sh_buffers[0]),
                entry(3, &sh_buffers[1]),
                entry(4, &sh_buffers[2]),
                entry(5, &sh_buffers[3]),
                entry(6, &chunk_metadata_buffer),
                entry(7, &resolved_color_buffer),
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: color_params_buffer.as_entire_binding(),
                },
            ],
        });

        Ok(Self {
            order_buffer,
            position_alpha_buffer,
            covariance0_buffer,
            covariance1_buffer,
            _color_auxiliary_buffer: color_auxiliary_buffer,
            _sh_buffers: sh_buffers,
            _chunk_metadata_buffer: chunk_metadata_buffer,
            resolved_color_buffer,
            draw_params_buffer,
            color_params_buffer,
            draw_bind_group,
            color_bind_group,
            capacity,
            sh_degree: u32::from(scene.sh_degree),
            _byte_plan: byte_plan,
            last_resolved_camera_position: None,
            gpu_order: None,
        })
    }

    pub fn prepare_cpu_order(
        &self,
        queue: &wgpu::Queue,
        sorted_indices: &[u32],
        camera: &Camera,
        width: u32,
        height: u32,
        upload_order: bool,
    ) -> Result<u32, ResidentGpuError> {
        if sorted_indices.len() > self.capacity {
            return Err(ResidentGpuError::OrderCapacityExceeded);
        }
        if upload_order && !sorted_indices.is_empty() {
            queue.write_buffer(&self.order_buffer, 0, bytemuck::cast_slice(sorted_indices));
        }
        let instance_count = u32::try_from(sorted_indices.len())
            .map_err(|_| ResidentGpuError::AddressSpaceExceeded)?;
        let mut params =
            make_surface_render_params(camera, width, height, instance_count, self.sh_degree);
        params.source_position_stride_words = 4;
        queue.write_buffer(&self.draw_params_buffer, 0, bytemuck::bytes_of(&params));
        Ok(instance_count)
    }

    pub fn prepare_gpu_order_draw(
        &mut self,
        device: &wgpu::Device,
        draw_layout: &wgpu::BindGroupLayout,
        queue: &wgpu::Queue,
        draw: ResidentGpuOrderDraw<'_>,
    ) -> Result<(), ResidentGpuError> {
        self.ensure_gpu_order(device, draw_layout)?;
        let mut params = make_surface_render_params(
            draw.camera,
            draw.width,
            draw.height,
            draw.instance_count,
            self.sh_degree,
        );
        params.order_stride_words = draw.order_stride_words;
        params.order_id_offset_words = draw.order_id_offset_words;
        params.source_position_stride_words = 4;
        queue.write_buffer(&self.draw_params_buffer, 0, bytemuck::bytes_of(&params));
        Ok(())
    }

    pub(crate) fn create_gpu_order_candidate(
        &self,
        device: &wgpu::Device,
        draw_layout: &wgpu::BindGroupLayout,
    ) -> Result<ResidentGpuSceneOrder, ResidentGpuError> {
        let count =
            u32::try_from(self.capacity).map_err(|_| ResidentGpuError::AddressSpaceExceeded)?;
        DirectGpuOrder::validate_resident_soa_dispatch_limits(device, count.max(1), count)
            .map_err(|error| ResidentGpuError::GpuOrderInitialization(error.to_string()))?;
        let sorter = DirectGpuOrder::new_resident_soa(
            device,
            &self.position_alpha_buffer,
            &self.draw_params_buffer,
            count.max(1),
            count,
        )
        .map_err(|error| ResidentGpuError::GpuOrderInitialization(error.to_string()))?;
        let draw_bind_group =
            self.create_draw_bind_group_for_order(device, draw_layout, sorter.final_ids());
        Ok(ResidentGpuSceneOrder {
            sorter,
            draw_bind_group,
        })
    }

    pub(crate) fn publish_gpu_order(&mut self, prepared: ResidentGpuSceneOrder) {
        debug_assert!(self.gpu_order.is_none());
        self.gpu_order = Some(prepared);
    }

    pub(crate) fn ensure_gpu_order(
        &mut self,
        device: &wgpu::Device,
        draw_layout: &wgpu::BindGroupLayout,
    ) -> Result<(), ResidentGpuError> {
        if self.gpu_order.is_some() {
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (device, draw_layout);
            Err(ResidentGpuError::GpuOrderInitialization(
                "browser GPU ordering must be prepared asynchronously before selection".into(),
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (validation_scope, oom_scope, internal_scope) = (
                device.push_error_scope(wgpu::ErrorFilter::Validation),
                device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
                device.push_error_scope(wgpu::ErrorFilter::Internal),
            );
            let prepared = self.create_gpu_order_candidate(device, draw_layout);
            let internal_error = pollster::block_on(internal_scope.pop());
            let oom_error = pollster::block_on(oom_scope.pop());
            let validation_error = pollster::block_on(validation_scope.pop());
            if let Some(error) = classify_gpu_order_scope_errors(
                internal_error.map(|error| error.to_string()),
                oom_error.map(|error| error.to_string()),
                validation_error.map(|error| error.to_string()),
            ) {
                return Err(error);
            }
            self.publish_gpu_order(prepared?);
            Ok(())
        }
    }

    pub(crate) fn gpu_order(&self) -> Option<&ResidentGpuSceneOrder> {
        self.gpu_order.as_ref()
    }

    pub fn create_draw_bind_group_for_order(
        &self,
        device: &wgpu::Device,
        draw_layout: &wgpu::BindGroupLayout,
        order_buffer: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        create_resident_draw_bind_group(
            device,
            draw_layout,
            "gsplat-resident-external-order-bind-group",
            order_buffer,
            &self.position_alpha_buffer,
            &self.covariance0_buffer,
            &self.covariance1_buffer,
            &self.resolved_color_buffer,
            &self.draw_params_buffer,
        )
    }

    /// Encodes one coherent all-point SH resolve if the camera position
    /// changed. Returns whether a compute pass was emitted.
    pub fn encode_color_resolve_if_needed(
        &mut self,
        queue: &wgpu::Queue,
        pipeline: &wgpu::ComputePipeline,
        encoder: &mut wgpu::CommandEncoder,
        camera: &Camera,
        max_workgroups_per_dimension: u32,
    ) -> Result<bool, ResidentGpuError> {
        let position = [
            camera.pose.position.x,
            camera.pose.position.y,
            camera.pose.position.z,
        ];
        if self.last_resolved_camera_position == Some(position) {
            return Ok(false);
        }
        let params = GpuResidentColorParams {
            camera_pos: [position[0], position[1], position[2], 0.0],
            len: u32::try_from(self.capacity)
                .map_err(|_| ResidentGpuError::AddressSpaceExceeded)?,
            sh_degree: self.sh_degree,
            _reserved0: 0,
            _pad: 0,
        };
        queue.write_buffer(&self.color_params_buffer, 0, bytemuck::bytes_of(&params));
        let (groups_x, groups_y) = dispatch_2d(
            self.capacity.div_ceil(COLOR_WORKGROUP_SIZE as usize),
            max_workgroups_per_dimension,
        )?;
        if self.capacity > 0 {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: wgpu_label("gsplat-resident-color-resolve-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.color_bind_group, &[]);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
        self.last_resolved_camera_position = Some(position);
        Ok(true)
    }
}

fn dispatch_2d(groups: usize, limit: u32) -> Result<(u32, u32), ResidentGpuError> {
    if groups == 0 {
        return Ok((1, 1));
    }
    let limit = limit.max(1) as usize;
    let x = groups.min(limit);
    let y = groups.div_ceil(x);
    if y > limit {
        return Err(ResidentGpuError::DispatchLimitExceeded);
    }
    Ok((x as u32, y as u32))
}

fn create_storage_init(
    device: &wgpu::Device,
    label: &'static str,
    bytes: &[u8],
    minimum_size: usize,
) -> wgpu::Buffer {
    const PLACEHOLDER: [u8; 80] = [0; 80];
    debug_assert!(minimum_size <= PLACEHOLDER.len());
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: wgpu_label(label),
        contents: if bytes.is_empty() {
            &PLACEHOLDER[..minimum_size]
        } else {
            bytes
        },
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    })
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

#[allow(clippy::too_many_arguments)]
fn create_resident_draw_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &'static str,
    order: &wgpu::Buffer,
    position_alpha: &wgpu::Buffer,
    covariance0: &wgpu::Buffer,
    covariance1: &wgpu::Buffer,
    resolved_color: &wgpu::Buffer,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label(label),
        layout,
        entries: &[
            entry(0, order),
            entry(1, position_alpha),
            entry(2, covariance0),
            entry(3, covariance1),
            entry(4, resolved_color),
            wgpu::BindGroupEntry {
                binding: 5,
                resource: params.as_entire_binding(),
            },
        ],
    })
}

pub fn create_resident_draw_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    create_splat_bind_group_layout(device, "gsplat-resident-draw-bgl", 5)
}

pub fn create_resident_draw_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    target_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    create_splat_pipeline(
        device,
        bind_group_layout,
        target_format,
        SplatPipeline {
            shader_label: "gsplat-resident-draw-shader",
            shader_source: include_str!("../shaders/splat_surface_resident.wgsl"),
            layout_label: "gsplat-resident-draw-pipeline-layout",
            pipeline_label: "gsplat-resident-draw-pipeline",
            topology: wgpu::PrimitiveTopology::TriangleStrip,
        },
    )
}

pub fn create_resident_color_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let mut entries = Vec::with_capacity(9);
    for binding in 0..8 {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: binding != 7,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
    }
    entries.push(wgpu::BindGroupLayoutEntry {
        binding: 8,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    });
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: wgpu_label("gsplat-resident-color-bgl"),
        entries: &entries,
    })
}

pub fn create_resident_color_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::ComputePipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: wgpu_label("gsplat-resident-color-shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../shaders/resident_color_resolve.wgsl").into(),
        ),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: wgpu_label("gsplat-resident-color-pipeline-layout"),
        bind_group_layouts: &[bind_group_layout],
        immediate_size: 0,
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: wgpu_label("gsplat-resident-color-pipeline"),
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    fn test_device() -> Option<wgpu::Device> {
        pollster::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .ok()?;
            let mut limits = wgpu::Limits::downlevel_defaults();
            limits.max_storage_buffers_per_shader_stage = RESIDENT_COLOR_STORAGE_BINDINGS;
            if !limits.check_limits(&adapter.limits()) {
                return None;
            }
            adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("resident-gpu-test-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::Performance,
                    trace: wgpu::Trace::Off,
                })
                .await
                .ok()
                .map(|(device, _)| device)
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn tiny_resident_scene() -> ResidentSceneCpu {
        let source = gsplat_core::SceneBuffers {
            positions: vec![gsplat_core::Vec3f::new(0.0, 0.0, 1.0)],
            opacity: vec![1.0],
            scale_xyz: vec![[-3.0; 3]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]],
            color_dc: vec![[0.0; 3]],
            sh_degree: 0,
            sh_rest: None,
        };
        ResidentSceneCpu::encode(&source).expect("tiny resident scene")
    }

    #[test]
    fn byte_plan_is_degree_specific_and_exact_count() {
        let n = 2_541_226_u64;
        let plan = ResidentGpuBytePlan::for_count(n as usize, 4).expect("plan");
        assert_eq!(plan.position_alpha, 16 * n);
        assert_eq!(plan.covariance0, 16 * n);
        assert_eq!(plan.covariance1, 8 * n);
        assert_eq!(plan.color_auxiliary, 8 * n);
        assert_eq!(plan.sh_plane, 16 * n);
        assert_eq!(plan.sh_plane_count, 4);
        assert_eq!(plan.resolved_color, 8 * n);
        assert_eq!(plan.order, 4 * n);
        assert_eq!(plan.projected_center_source, 16 * n);
        assert_eq!(plan.projected_axes, 16 * n);
        assert_eq!(plan.projected_contributor_ranks, 4 * n);
        assert_eq!(
            plan.projected_contributor_group_offsets,
            (n.div_ceil(u64::from(
                crate::projected_quads_gpu::PROJECT_WORKGROUP_SIZE,
            )) + 1)
                * 4
        );
        assert!(plan.projected_contributor_scan_sums > 0);
        assert!(plan.projected_contributor_largest_scan_sum > 0);
        assert!(plan.projected_contributor_scan_params >= 256);
        assert_eq!(plan.projected_contributor_args, 16);
        assert_eq!(
            plan.chunk_metadata,
            80 * n.div_ceil(RESIDENT_CHUNK_SPLATS as u64)
        );
        assert_eq!(
            plan.total_static,
            (16 + 16 + 8 + 8 + 4 * 16 + 8 + 4 + 16 + 16) * n
                + 80 * n.div_ceil(RESIDENT_CHUNK_SPLATS as u64)
                + plan.projected_contributor_group_offsets
                + plan.projected_contributor_ranks
                + plan.projected_contributor_scan_sums
                + plan.projected_contributor_scan_params
                + plan.projected_contributor_args
        );
    }

    #[test]
    fn byte_plan_rejects_non_degree_sh_plane_counts() {
        assert_eq!(
            ResidentGpuBytePlan::for_count(1, 2),
            Err(ResidentGpuError::UnsupportedShPlaneCount(2))
        );
        assert_eq!(
            ResidentGpuBytePlan::for_count(1, RESIDENT_SH_PLANES as u32 + 1),
            Err(ResidentGpuError::UnsupportedShPlaneCount(5))
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn gpu_order_scope_errors_are_structured_and_prioritize_oom() {
        assert_eq!(
            classify_gpu_order_scope_errors(
                Some("internal".into()),
                Some("oom".into()),
                Some("validation".into()),
            ),
            Some(ResidentGpuError::GpuOrderOutOfMemory("oom".into()))
        );
        assert_eq!(
            classify_gpu_order_scope_errors(
                Some("internal".into()),
                None,
                Some("validation".into()),
            ),
            Some(ResidentGpuError::GpuOrderInternal("internal".into()))
        );
        assert_eq!(
            classify_gpu_order_scope_errors(None, None, Some("validation".into())),
            Some(ResidentGpuError::GpuOrderValidation("validation".into()))
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn failed_lazy_gpu_order_validation_is_transactional_and_retryable() {
        let Some(device) = test_device() else {
            return;
        };
        let scene = tiny_resident_scene();
        let draw_layout = create_resident_draw_bind_group_layout(&device);
        let color_layout = create_resident_color_bind_group_layout(&device);
        let mut resources = ResidentGpuResources::new(&device, &draw_layout, &color_layout, &scene)
            .expect("resident resources");
        let incompatible_layout =
            create_splat_bind_group_layout(&device, "resident-invalid-order-bgl", 4);

        let error = resources
            .ensure_gpu_order(&device, &incompatible_layout)
            .expect_err("incompatible order layout must be captured");

        assert!(matches!(error, ResidentGpuError::GpuOrderValidation(_)));
        assert!(resources.gpu_order().is_none());
        resources
            .ensure_gpu_order(&device, &draw_layout)
            .expect("a clean retry with the correct layout must succeed");
        assert!(resources.gpu_order().is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn complete_gpu_order_candidate_stays_unpublished_until_commit() {
        let Some(device) = test_device() else {
            return;
        };
        let scene = tiny_resident_scene();
        let draw_layout = create_resident_draw_bind_group_layout(&device);
        let color_layout = create_resident_color_bind_group_layout(&device);
        let mut resources = ResidentGpuResources::new(&device, &draw_layout, &color_layout, &scene)
            .expect("resident resources");

        let candidate = resources
            .create_gpu_order_candidate(&device, &draw_layout)
            .expect("candidate");
        assert!(resources.gpu_order().is_none());

        resources.publish_gpu_order(candidate);
        assert!(resources.gpu_order().is_some());
    }

    #[test]
    fn portable_128_mib_binding_admits_known_six_million_scene() {
        let plan = ResidentGpuBytePlan::for_count(6_132_000, 4).expect("plan");
        let limit = 128_u64 << 20;
        assert!(plan.position_alpha < limit);
        assert!(plan.sh_plane < limit);
        assert!(plan.covariance0 < limit);
        assert!(plan.covariance1 < limit);
        assert!(plan.color_auxiliary < limit);
        assert!(plan.projected_contributor_ranks < limit);
    }

    fn portable_resident_limits() -> wgpu::Limits {
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_storage_buffer_binding_size = 128 << 20;
        limits.max_buffer_size = 128 << 20;
        limits.max_storage_buffers_per_shader_stage = RESIDENT_COLOR_STORAGE_BINDINGS;
        limits
    }

    #[test]
    fn portable_binding_boundary_is_exactly_8_388_608_splats() {
        let limits = portable_resident_limits();
        let at_limit = ResidentGpuBytePlan::for_count(8_388_608, 4).expect("boundary plan");
        let above_limit = ResidentGpuBytePlan::for_count(8_388_609, 4).expect("overflow plan");

        assert_eq!(at_limit.position_alpha, 128 << 20);
        assert_eq!(at_limit.covariance0, 128 << 20);
        assert_eq!(at_limit.sh_plane, 128 << 20);
        assert_eq!(at_limit.largest_storage_binding_bytes(), 128 << 20);
        assert_eq!(at_limit.validate_limits(&limits), Ok(at_limit));
        assert_eq!(
            above_limit.largest_storage_binding_bytes(),
            (128 << 20) + 16
        );
        assert_eq!(
            above_limit.validate_limits(&limits),
            Err(ResidentGpuError::BindingLimitExceeded {
                resource: "position+alpha",
                required_bytes: (128 << 20) + 16,
                limit_bytes: 128 << 20,
            })
        );
    }

    #[test]
    fn resident_limit_validation_uses_smaller_max_buffer_size() {
        let plan = ResidentGpuBytePlan::for_count(8_388_608, 4).expect("plan");
        let mut limits = portable_resident_limits();
        limits.max_storage_buffer_binding_size = 256 << 20;
        limits.max_buffer_size = (128 << 20) - 1;

        assert_eq!(
            plan.validate_limits(&limits),
            Err(ResidentGpuError::BindingLimitExceeded {
                resource: "position+alpha",
                required_bytes: 128 << 20,
                limit_bytes: (128 << 20) - 1,
            })
        );
    }

    #[test]
    fn resident_limit_validation_requires_eight_storage_bindings() {
        let plan = ResidentGpuBytePlan::for_count(1, 4).expect("plan");
        let mut limits = portable_resident_limits();
        limits.max_storage_buffers_per_shader_stage = RESIDENT_COLOR_STORAGE_BINDINGS - 1;

        assert_eq!(
            plan.validate_limits(&limits),
            Err(ResidentGpuError::StorageBindingCountUnsupported(7))
        );
    }

    #[test]
    fn degree_zero_does_not_charge_inactive_sh_planes_per_splat() {
        let plan = ResidentGpuBytePlan::for_count(8_388_608, 0).expect("plan");

        assert_eq!(plan.sh_plane_count, 0);
        assert_eq!(plan.sh_plane, 128 << 20);
        assert_eq!(plan.largest_storage_binding_bytes(), 128 << 20);
        assert_eq!(plan.validate_limits(&portable_resident_limits()), Ok(plan));
    }

    #[test]
    fn dispatch_flattens_across_two_dimensions() {
        assert_eq!(dispatch_2d(0, 7).unwrap(), (1, 1));
        assert_eq!(dispatch_2d(7, 7).unwrap(), (7, 1));
        assert_eq!(dispatch_2d(8, 7).unwrap(), (7, 2));
        assert_eq!(dispatch_2d(49, 7).unwrap(), (7, 7));
        assert!(matches!(
            dispatch_2d(50, 7),
            Err(ResidentGpuError::DispatchLimitExceeded)
        ));
    }
}
