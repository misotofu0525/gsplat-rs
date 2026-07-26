//! Exact source-order projection and contributor production for Resident scenes.
//!
//! The graph projects every resident source once, retains two source-indexed
//! cache planes, compacts true contributors in stable source order, and hands
//! the resulting `{full32 depth key, source_id}[0..C)` prefix to the reusable
//! external-prefix radix sorter. Surface publication remains diagnostic and
//! transactionally selected; the qualified post-sort graph stays the default.

use std::{mem::size_of, num::NonZeroU64};

use gsplat_core::Camera;

use crate::gpu::{
    ExternalPrefixRadix, ExternalPrefixRadixBytePlan, GpuPrefixScan,
    PREPROJECT_DRAW_INDIRECT_ARGS_BYTES, PreprojectKeyIdCompactor,
};
#[cfg(test)]
use crate::raster::QUAD_VERTEX_COUNT;
use crate::resident_gpu::{
    RESIDENT_COLOR_STORAGE_BINDINGS, ResidentGpuError, ResidentGpuResources,
};
use crate::{make_surface_render_params, wgpu_label};

pub(crate) const PREPROJECT_WORKGROUP_SIZE: u32 = 128;
const SOURCE_CENTER_ALPHA_KEY_BYTES: u64 = 16;
const SOURCE_AXES32_BYTES: u64 = 16;
const SOURCE_AXES16_BYTES: u64 = 8;
const WORD_BYTES: u64 = size_of::<u32>() as u64;
const SCAN_ITEMS_PER_GROUP: u32 = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Dispatch2d {
    x: u32,
    y: u32,
}

impl Dispatch2d {
    fn for_workgroups(workgroups: u32, limit: u32) -> Result<Self, ResidentGpuError> {
        if workgroups == 0 {
            return Ok(Self { x: 0, y: 1 });
        }
        let limit = limit.max(1);
        let x = workgroups.min(limit);
        let y = workgroups.div_ceil(x);
        if y > limit {
            return Err(ResidentGpuError::DispatchLimitExceeded);
        }
        Ok(Self { x, y })
    }

    fn for_items(items: u32, items_per_group: u32, limit: u32) -> Result<Self, ResidentGpuError> {
        Self::for_workgroups(items.div_ceil(items_per_group), limit)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScanBytePlan {
    sums: u64,
    largest_sum: u64,
    params: u64,
}

/// Exact static allocation owned by the direct S -> C graph.
///
/// The radix fields already include their two key planes, two source-ID
/// planes, fixed-capacity digit prefix, hierarchical scan, pass uniforms, and
/// 32-byte producer control record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PreprojectGpuBytePlan {
    pub(crate) source_center_alpha_key: u64,
    pub(crate) source_axes: u64,
    pub(crate) candidate_group_offsets: u64,
    pub(crate) candidate_scan_sums: u64,
    pub(crate) candidate_largest_scan_sum: u64,
    pub(crate) candidate_scan_params: u64,
    pub(crate) contributor_group_offsets: u64,
    pub(crate) contributor_scan_sums: u64,
    pub(crate) contributor_largest_scan_sum: u64,
    pub(crate) contributor_scan_params: u64,
    pub(crate) radix: ExternalPrefixRadixBytePlan,
    pub(crate) draw_args: u64,
    pub(crate) total_static: u64,
}

impl PreprojectGpuBytePlan {
    pub(crate) fn for_capacity(
        capacity: u32,
        limits: &wgpu::Limits,
    ) -> Result<Self, ResidentGpuError> {
        Self::for_capacity_with_axes_record_bytes(capacity, limits, SOURCE_AXES32_BYTES)
    }

    #[cfg(feature = "diagnostic-surface-projected-axes16")]
    pub(crate) fn for_capacity_axes16(
        capacity: u32,
        limits: &wgpu::Limits,
    ) -> Result<Self, ResidentGpuError> {
        Self::for_capacity_with_axes_record_bytes(capacity, limits, SOURCE_AXES16_BYTES)
    }

    fn for_capacity_with_axes_record_bytes(
        capacity: u32,
        limits: &wgpu::Limits,
        axes_record_bytes: u64,
    ) -> Result<Self, ResidentGpuError> {
        let center_plane = u64::from(capacity)
            .checked_mul(SOURCE_CENTER_ALPHA_KEY_BYTES)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?
            .max(SOURCE_CENTER_ALPHA_KEY_BYTES);
        let axes_plane = u64::from(capacity)
            .checked_mul(axes_record_bytes)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?
            .max(axes_record_bytes);
        let contributor_groups = capacity.div_ceil(PREPROJECT_WORKGROUP_SIZE);
        let offset_count = contributor_groups
            .checked_add(1)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let group_offsets = u64::from(offset_count)
            .checked_mul(WORD_BYTES)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let scan = scan_byte_plan(
            offset_count,
            limits.min_uniform_buffer_offset_alignment.max(16),
        )?;
        let radix = ExternalPrefixRadixBytePlan::for_capacity(capacity, limits)?;
        let draw_args = PREPROJECT_DRAW_INDIRECT_ARGS_BYTES;
        let total_static = [
            center_plane,
            axes_plane,
            group_offsets,
            scan.sums,
            scan.params,
            group_offsets,
            scan.sums,
            scan.params,
            radix.total_static,
            draw_args,
        ]
        .into_iter()
        .try_fold(0_u64, |total, bytes| {
            total
                .checked_add(bytes)
                .ok_or(ResidentGpuError::AddressSpaceExceeded)
        })?;
        Ok(Self {
            source_center_alpha_key: center_plane,
            source_axes: axes_plane,
            candidate_group_offsets: group_offsets,
            candidate_scan_sums: scan.sums,
            candidate_largest_scan_sum: scan.largest_sum,
            candidate_scan_params: scan.params,
            contributor_group_offsets: group_offsets,
            contributor_scan_sums: scan.sums,
            contributor_largest_scan_sum: scan.largest_sum,
            contributor_scan_params: scan.params,
            radix,
            draw_args,
            total_static,
        })
    }

    fn validate_limits(self, limits: &wgpu::Limits) -> Result<Self, ResidentGpuError> {
        if limits.max_storage_buffers_per_shader_stage < RESIDENT_COLOR_STORAGE_BINDINGS {
            return Err(ResidentGpuError::StorageBindingCountUnsupported(
                limits.max_storage_buffers_per_shader_stage,
            ));
        }
        self.radix.validate_limits(limits)?;
        let binding_limit =
            u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size);
        for (resource, bytes) in [
            (
                "preproject source center/alpha/key",
                self.source_center_alpha_key,
            ),
            ("preproject source axes", self.source_axes),
            ("preproject candidate offsets", self.candidate_group_offsets),
            (
                "preproject candidate scan sums",
                self.candidate_largest_scan_sum,
            ),
            (
                "preproject contributor offsets",
                self.contributor_group_offsets,
            ),
            (
                "preproject contributor scan sums",
                self.contributor_largest_scan_sum,
            ),
            ("preproject draw args", self.draw_args),
        ] {
            if bytes > binding_limit {
                return Err(ResidentGpuError::BindingLimitExceeded {
                    resource,
                    required_bytes: bytes,
                    limit_bytes: binding_limit,
                });
            }
        }
        for (resource, bytes) in [
            (
                "preproject candidate scan params",
                self.candidate_scan_params,
            ),
            (
                "preproject contributor scan params",
                self.contributor_scan_params,
            ),
        ] {
            if bytes > limits.max_buffer_size {
                return Err(ResidentGpuError::BindingLimitExceeded {
                    resource,
                    required_bytes: bytes,
                    limit_bytes: limits.max_buffer_size,
                });
            }
        }
        Ok(self)
    }
}

fn scan_byte_plan(count: u32, uniform_stride: u32) -> Result<ScanBytePlan, ResidentGpuError> {
    debug_assert!(count > 0);
    let mut level_count = count;
    let mut sums = 0_u64;
    let mut largest_sum = 0_u64;
    let mut levels = 0_u64;
    loop {
        let groups = level_count.div_ceil(SCAN_ITEMS_PER_GROUP).max(1);
        let bytes = u64::from(groups)
            .checked_mul(WORD_BYTES)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        sums = sums
            .checked_add(bytes)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        largest_sum = largest_sum.max(bytes);
        levels = levels
            .checked_add(1)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        if groups == 1 {
            break;
        }
        level_count = groups;
    }
    let params = levels
        .checked_mul(u64::from(uniform_stride))
        .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
    Ok(ScanBytePlan {
        sums,
        largest_sum,
        params,
    })
}

/// Target-independent owner of the Exact `S -> V/C -> stable full32` compute
/// graph. It owns no render pipeline, texture format, target or presentation
/// state, so the same graph can be staged with the device-owned scene.
pub(crate) struct PreprojectedGpuCompute {
    capacity: u32,
    project_dispatch: Dispatch2d,
    project_pipeline: wgpu::ComputePipeline,
    project_bind_group: wgpu::BindGroup,
    key_id_compactor: PreprojectKeyIdCompactor,
    candidate_offsets: wgpu::Buffer,
    candidate_count_offset: u64,
    candidate_scan: GpuPrefixScan,
    contributor_offsets: wgpu::Buffer,
    contributor_count_offset: u64,
    contributor_scan: GpuPrefixScan,
    source_center_alpha_key: wgpu::Buffer,
    source_axes: wgpu::Buffer,
    radix: ExternalPrefixRadix,
    _byte_plan: PreprojectGpuBytePlan,
}

impl PreprojectedGpuCompute {
    pub(crate) fn new(
        device: &wgpu::Device,
        resident: &ResidentGpuResources,
        indirect_vertex_count: u32,
    ) -> Result<Self, ResidentGpuError> {
        Self::new_with_axes_record_bytes(
            device,
            resident,
            indirect_vertex_count,
            SOURCE_AXES32_BYTES,
            include_str!("../shaders/preproject_contributors.wgsl"),
        )
    }

    pub(crate) fn new_axes16(
        device: &wgpu::Device,
        resident: &ResidentGpuResources,
        indirect_vertex_count: u32,
    ) -> Result<Self, ResidentGpuError> {
        Self::new_with_axes_record_bytes(
            device,
            resident,
            indirect_vertex_count,
            SOURCE_AXES16_BYTES,
            include_str!("../shaders/preproject_contributors_axes16.wgsl"),
        )
    }

    fn new_with_axes_record_bytes(
        device: &wgpu::Device,
        resident: &ResidentGpuResources,
        indirect_vertex_count: u32,
        axes_record_bytes: u64,
        shader_source: &'static str,
    ) -> Result<Self, ResidentGpuError> {
        let capacity =
            u32::try_from(resident.capacity).map_err(|_| ResidentGpuError::AddressSpaceExceeded)?;
        let limits = device.limits();
        let byte_plan = PreprojectGpuBytePlan::for_capacity_with_axes_record_bytes(
            capacity,
            &limits,
            axes_record_bytes,
        )?
        .validate_limits(&limits)?;
        let dispatch_limit = limits.max_compute_workgroups_per_dimension;
        let project_dispatch =
            Dispatch2d::for_items(capacity, PREPROJECT_WORKGROUP_SIZE, dispatch_limit)?;

        let source_center_alpha_key = storage_buffer(
            device,
            "gsplat-preproject-center-alpha-key",
            byte_plan.source_center_alpha_key,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let source_axes = storage_buffer(
            device,
            "gsplat-preproject-axes",
            byte_plan.source_axes,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let candidate_offsets = storage_buffer(
            device,
            "gsplat-preproject-candidate-offsets",
            byte_plan.candidate_group_offsets,
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        );
        let contributor_offsets = storage_buffer(
            device,
            "gsplat-preproject-contributor-offsets",
            byte_plan.contributor_group_offsets,
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        );
        let radix = ExternalPrefixRadix::new(device, capacity)?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("gsplat-preproject-contributor-shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });
        let project_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-preproject-project-bgl"),
            entries: &[
                storage_layout(0, true, wgpu::ShaderStages::COMPUTE),
                storage_layout(1, true, wgpu::ShaderStages::COMPUTE),
                storage_layout(2, true, wgpu::ShaderStages::COMPUTE),
                uniform_layout(
                    3,
                    NonZeroU64::new(size_of::<crate::GpuSurfaceRenderParams>() as u64),
                ),
                storage_layout(4, false, wgpu::ShaderStages::COMPUTE),
                storage_layout(5, false, wgpu::ShaderStages::COMPUTE),
                storage_layout(6, false, wgpu::ShaderStages::COMPUTE),
                storage_layout(7, false, wgpu::ShaderStages::COMPUTE),
            ],
        });
        let project_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: wgpu_label("gsplat-preproject-project-pipeline-layout"),
                bind_group_layouts: &[&project_layout],
                immediate_size: 0,
            });
        let project_pipeline = create_compute_pipeline(
            device,
            &shader,
            &project_pipeline_layout,
            "project_count",
            "gsplat-preproject-project-pipeline",
        );
        let project_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-preproject-project-bg"),
            layout: &project_layout,
            entries: &[
                entry(0, &resident.position_alpha_buffer),
                entry(1, &resident.covariance0_buffer),
                entry(2, &resident.covariance1_buffer),
                entry(3, &resident.draw_params_buffer),
                entry(4, &source_center_alpha_key),
                entry(5, &source_axes),
                entry(6, &contributor_offsets),
                entry(7, &candidate_offsets),
            ],
        });
        let key_id_compactor = PreprojectKeyIdCompactor::new(
            device,
            &shader,
            &source_center_alpha_key,
            &contributor_offsets,
            &radix,
            &resident.draw_params_buffer,
            indirect_vertex_count,
        );
        let offset_count = capacity
            .div_ceil(PREPROJECT_WORKGROUP_SIZE)
            .checked_add(1)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let candidate_scan =
            GpuPrefixScan::new(device, &candidate_offsets, offset_count, dispatch_limit)?;
        let contributor_scan =
            GpuPrefixScan::new(device, &contributor_offsets, offset_count, dispatch_limit)?;
        let count_offset = u64::from(offset_count - 1) * WORD_BYTES;

        Ok(Self {
            capacity,
            project_dispatch,
            project_pipeline,
            project_bind_group,
            key_id_compactor,
            candidate_offsets,
            candidate_count_offset: count_offset,
            candidate_scan,
            contributor_offsets,
            contributor_count_offset: count_offset,
            contributor_scan,
            source_center_alpha_key,
            source_axes,
            radix,
            _byte_plan: byte_plan,
        })
    }

    /// Encodes one coherent `S -> C -> stable full32 descending order` graph.
    /// Every call refreshes all S projections and the complete order prefix;
    /// camera, viewport, or scene revisions never reuse an older producer.
    pub(crate) fn encode(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        resident: &ResidentGpuResources,
        camera: &Camera,
        width: u32,
        height: u32,
    ) {
        self.encode_projection_and_count(queue, encoder, resident, camera, width, height);
        self.key_id_compactor.encode(
            encoder,
            (self.capacity > 0).then_some((self.project_dispatch.x, self.project_dispatch.y)),
        );
        self.radix.encode(encoder);
    }

    /// Refreshes source-indexed projected geometry and computes current C from
    /// the complete source set without changing the previously published
    /// sorted ID prefix or its indirect draw count. This is the non-refresh
    /// sort-interval path: geometry/count are current, order is explicitly
    /// stale until the next full [`Self::encode`].
    pub(crate) fn encode_projection_and_count(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        resident: &ResidentGpuResources,
        camera: &Camera,
        width: u32,
        height: u32,
    ) {
        let mut params =
            make_surface_render_params(camera, width, height, self.capacity, resident.sh_degree);
        params.source_position_stride_words = 4;
        queue.write_buffer(&resident.draw_params_buffer, 0, bytemuck::bytes_of(&params));

        // Clear both complete fixed-capacity count/offset planes, sentinels
        // included. Their exclusive-scan sentinels are current full-S V/C.
        encoder.clear_buffer(&self.candidate_offsets, 0, None);
        encoder.clear_buffer(&self.contributor_offsets, 0, None);
        if self.capacity > 0 {
            encode_compute(
                encoder,
                &self.project_pipeline,
                &self.project_bind_group,
                0,
                self.project_dispatch,
                "gsplat-preproject-project-pass",
            );
        }
        self.candidate_scan.encode(encoder);
        self.contributor_scan.encode(encoder);
    }

    pub(crate) fn draw_args(&self) -> &wgpu::Buffer {
        self.key_id_compactor.draw_args()
    }

    pub(crate) const fn capacity(&self) -> u32 {
        self.capacity
    }

    pub(crate) fn final_keys(&self) -> &wgpu::Buffer {
        self.radix.final_keys()
    }

    pub(crate) fn final_source_ids(&self) -> &wgpu::Buffer {
        self.radix.final_source_ids()
    }

    pub(crate) fn source_center_alpha_key(&self) -> &wgpu::Buffer {
        &self.source_center_alpha_key
    }

    pub(crate) fn source_axes(&self) -> &wgpu::Buffer {
        &self.source_axes
    }

    pub(crate) fn source_axes_record_bytes(&self) -> u64 {
        self._byte_plan.source_axes / u64::from(self.capacity.max(1))
    }

    pub(crate) fn order_control(&self) -> &wgpu::Buffer {
        self.radix.control()
    }

    /// Current-camera complete-S near/far/alpha candidate count V produced by
    /// the sentinel element of the candidate group-count exclusive scan.
    pub(crate) fn candidate_count_buffer_and_offset(&self) -> (&wgpu::Buffer, u64) {
        (&self.candidate_offsets, self.candidate_count_offset)
    }

    /// Current-camera complete-S contributor count produced by the sentinel
    /// element of the group-count exclusive scan.
    pub(crate) fn contributor_count_buffer_and_offset(&self) -> (&wgpu::Buffer, u64) {
        (&self.contributor_offsets, self.contributor_count_offset)
    }
}

fn storage_layout(
    binding: u32,
    read_only: bool,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform_layout(
    binding: u32,
    min_binding_size: Option<NonZeroU64>,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size,
        },
        count: None,
    }
}

fn create_compute_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    entry_point: &'static str,
    label: &'static str,
) -> wgpu::ComputePipeline {
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: wgpu_label(label),
        layout: Some(layout),
        module: shader,
        entry_point: Some(entry_point),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    })
}

fn encode_compute(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    bind_group_index: u32,
    dispatch: Dispatch2d,
    label: &'static str,
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: wgpu_label(label),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(bind_group_index, bind_group, &[]);
    pass.dispatch_workgroups(dispatch.x, dispatch.y, 1);
}

fn storage_buffer(
    device: &wgpu::Device,
    label: &'static str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: wgpu_label(label),
        size,
        usage,
        mapped_at_creation: false,
    })
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use gsplat_core::{CameraIntrinsics, RenderMode, RendererConfig, SceneBuffers, Vec3f};

    use super::*;
    use crate::gpu::{EXTERNAL_RADIX_TILE_SIZE, ExternalPrefixControl, PreprojectDrawIndirectArgs};
    use crate::{
        ResidentCovariance0, ResidentCovariance1, ResidentPositionAlpha, ResidentSceneCpu,
    };

    const ORACLE_WIDTH: u32 = 64;
    const ORACLE_HEIGHT: u32 = 64;
    const ALPHA_THRESHOLD: f32 = 1.0 / 255.0;

    #[derive(Clone, Copy, Debug)]
    struct CpuProjection {
        center_alpha_key: [f32; 4],
        axes: [f32; 4],
    }

    fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
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
                    label: Some("preproject-oracle-test-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::Performance,
                    trace: wgpu::Trace::Off,
                })
                .await
                .ok()
        })
    }

    fn base_scene(count: usize) -> ResidentSceneCpu {
        ResidentSceneCpu::encode(&SceneBuffers {
            positions: (0..count)
                .map(|index| Vec3f::new(index as f32 * 0.001, 0.0, 2.0))
                .collect(),
            opacity: vec![2.0; count],
            scale_xyz: vec![[-2.5, -2.5, -2.5]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.2, 0.1, -0.1]; count],
            sh_degree: 0,
            sh_rest: None,
        })
        .expect("resident scene")
    }

    fn resident_resources(device: &wgpu::Device, scene: &ResidentSceneCpu) -> ResidentGpuResources {
        let color_layout = crate::resident_gpu::create_resident_color_bind_group_layout(device);
        ResidentGpuResources::new(device, &color_layout, scene).expect("resident resources")
    }

    fn camera() -> Camera {
        Camera {
            intrinsics: CameraIntrinsics {
                vertical_fov_radians: 60.0_f32.to_radians(),
                near_plane: 0.5,
                far_plane: 5.0,
            },
            ..Camera::default()
        }
    }

    fn next_up(value: f32) -> f32 {
        if value.is_nan() || value == f32::INFINITY {
            return value;
        }
        if value == 0.0 {
            return f32::from_bits(1);
        }
        f32::from_bits(if value > 0.0 {
            value.to_bits() + 1
        } else {
            value.to_bits() - 1
        })
    }

    fn next_down(value: f32) -> f32 {
        if value.is_nan() || value == f32::NEG_INFINITY {
            return value;
        }
        if value == 0.0 {
            return f32::from_bits(0x8000_0001);
        }
        f32::from_bits(if value > 0.0 {
            value.to_bits() - 1
        } else {
            value.to_bits() + 1
        })
    }

    fn full_quad_is_strictly_outside_clip(
        center: [f32; 2],
        axis_u: [f32; 2],
        axis_v: [f32; 2],
        alpha: f32,
    ) -> bool {
        if !center.into_iter().all(f32::is_finite)
            || !axis_u.into_iter().all(f32::is_finite)
            || !axis_v.into_iter().all(f32::is_finite)
            || !alpha.is_finite()
        {
            return false;
        }
        let extent_x = next_up(axis_u[0].abs() + axis_v[0].abs());
        let extent_y = next_up(axis_u[1].abs() + axis_v[1].abs());
        let minimum = [
            next_down(center[0] - extent_x),
            next_down(center[1] - extent_y),
        ];
        let maximum = [next_up(center[0] + extent_x), next_up(center[1] + extent_y)];
        maximum[0] < -1.0 || minimum[0] > 1.0 || maximum[1] < -1.0 || minimum[1] > 1.0
    }

    /// CPU oracle deliberately composes the renderer's established canonical
    /// view transform, covariance projection, and ellipse construction. Only
    /// the conservative two-ULP full-quad test mirrors the current exact GPU
    /// release shader because the older CPU instance path predates that guard.
    fn cpu_projection(
        position_alpha: ResidentPositionAlpha,
        covariance0: ResidentCovariance0,
        covariance1: ResidentCovariance1,
        camera: &Camera,
    ) -> Option<CpuProjection> {
        let config = RendererConfig {
            width: ORACLE_WIDTH,
            height: ORACLE_HEIGHT,
            mode: RenderMode::SortedAlpha,
        };
        let params = crate::InstanceBuildParams::new(camera, config)?;
        let position = Vec3f::new(
            position_alpha.position_alpha[0],
            position_alpha.position_alpha[1],
            position_alpha.position_alpha[2],
        );
        let p_cam =
            crate::world_to_camera_with_view_rot(position, camera.pose.position, params.view_rot);
        if !crate::is_visible(p_cam.z, camera) || position_alpha.position_alpha[3] < ALPHA_THRESHOLD
        {
            return None;
        }
        let center = [
            (p_cam.x * params.f) / p_cam.z / params.aspect,
            (p_cam.y * params.f) / p_cam.z,
        ];
        let world_covariance = crate::CameraCovarianceTerms {
            xx: covariance0.values[0],
            xy: covariance0.values[1],
            xz: covariance0.values[2],
            yy: covariance0.values[3],
            yz: covariance1.values[0],
            zz: covariance1.values[1],
        };
        let cov2 = crate::project_world_covariance_terms_to_ndc(p_cam, world_covariance, &params)?;
        let (axis_u, axis_v) = crate::ellipse_axes_from_covariance(cov2)?;
        let alpha = position_alpha.position_alpha[3];
        if full_quad_is_strictly_outside_clip(center, axis_u, axis_v, alpha) {
            return None;
        }
        Some(CpuProjection {
            center_alpha_key: [
                center[0],
                center[1],
                alpha,
                f32::from_bits(p_cam.z.max(0.0).to_bits()),
            ],
            axes: [axis_u[0], axis_u[1], axis_v[0], axis_v[1]],
        })
    }

    fn upload_source_planes(
        queue: &wgpu::Queue,
        resident: &ResidentGpuResources,
        position_alpha: &[ResidentPositionAlpha],
        covariance0: &[ResidentCovariance0],
        covariance1: &[ResidentCovariance1],
    ) {
        queue.write_buffer(
            &resident.position_alpha_buffer,
            0,
            bytemuck::cast_slice(position_alpha),
        );
        queue.write_buffer(
            &resident.covariance0_buffer,
            0,
            bytemuck::cast_slice(covariance0),
        );
        queue.write_buffer(
            &resident.covariance1_buffer,
            0,
            bytemuck::cast_slice(covariance1),
        );
    }

    fn copy_buffer(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Buffer,
        size: u64,
        label: &'static str,
    ) -> wgpu::Buffer {
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(source, 0, &readback, 0, size);
        readback
    }

    fn copy_buffer_at(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Buffer,
        source_offset: u64,
        size: u64,
        label: &'static str,
    ) -> wgpu::Buffer {
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(source, source_offset, &readback, 0, size);
        readback
    }

    fn read_bytes(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
        let slice = buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll readback");
        rx.recv().expect("map callback").expect("map result");
        let bytes = slice.get_mapped_range().to_vec();
        buffer.unmap();
        bytes
    }

    struct ProducerReadback {
        candidate_count: u32,
        control: ExternalPrefixControl,
        draw: PreprojectDrawIndirectArgs,
        keys: Vec<u32>,
        ids: Vec<u32>,
        center: Vec<[f32; 4]>,
        axes: Vec<[f32; 4]>,
    }

    fn half_to_f32(bits: u16) -> f32 {
        let sign = u32::from(bits & 0x8000) << 16;
        let exponent = u32::from((bits >> 10) & 0x1f);
        let fraction = u32::from(bits & 0x03ff);
        let value = match exponent {
            0 if fraction == 0 => sign,
            0 => {
                let leading = 31 - fraction.leading_zeros();
                let normalized_fraction = (fraction << (10 - leading)) & 0x03ff;
                let f32_exponent = 113 - (10 - leading);
                sign | (f32_exponent << 23) | (normalized_fraction << 13)
            }
            0x1f => sign | 0x7f80_0000 | (fraction << 13),
            _ => sign | ((exponent + 112) << 23) | (fraction << 13),
        };
        f32::from_bits(value)
    }

    fn decode_axes(bytes: &[u8], capacity: usize, record_bytes: u64) -> Vec<[f32; 4]> {
        match record_bytes {
            SOURCE_AXES32_BYTES => bytemuck::cast_slice::<u8, [f32; 4]>(bytes)[..capacity].to_vec(),
            SOURCE_AXES16_BYTES => bytes
                .chunks_exact(SOURCE_AXES16_BYTES as usize)
                .take(capacity)
                .map(|record| {
                    let first =
                        u32::from_ne_bytes(record[..4].try_into().expect("first axis word"));
                    let second =
                        u32::from_ne_bytes(record[4..8].try_into().expect("second axis word"));
                    [
                        half_to_f32(first as u16),
                        half_to_f32((first >> 16) as u16),
                        half_to_f32(second as u16),
                        half_to_f32((second >> 16) as u16),
                    ]
                })
                .collect(),
            other => panic!("unexpected projected-axis record size {other}"),
        }
    }

    fn run_and_read(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        resident: &ResidentGpuResources,
        producer: &PreprojectedGpuCompute,
        camera: &Camera,
    ) -> ProducerReadback {
        let capacity = producer.capacity() as usize;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("preproject-oracle-encoder"),
        });
        producer.encode(
            queue,
            &mut encoder,
            resident,
            camera,
            ORACLE_WIDTH,
            ORACLE_HEIGHT,
        );
        let (candidate_buffer, candidate_offset) = producer.candidate_count_buffer_and_offset();
        let candidate_readback = copy_buffer_at(
            device,
            &mut encoder,
            candidate_buffer,
            candidate_offset,
            size_of::<u32>() as u64,
            "preproject-candidate-count-readback",
        );
        let control_readback = copy_buffer(
            device,
            &mut encoder,
            producer.order_control(),
            size_of::<ExternalPrefixControl>() as u64,
            "preproject-control-readback",
        );
        let draw_readback = copy_buffer(
            device,
            &mut encoder,
            producer.draw_args(),
            size_of::<PreprojectDrawIndirectArgs>() as u64,
            "preproject-draw-readback",
        );
        let word_bytes = (capacity.max(1) * size_of::<u32>()) as u64;
        let center_cache_bytes = (capacity.max(1) * size_of::<[f32; 4]>()) as u64;
        let axes_cache_bytes = capacity.max(1) as u64 * producer.source_axes_record_bytes();
        let keys_readback = copy_buffer(
            device,
            &mut encoder,
            producer.final_keys(),
            word_bytes,
            "preproject-keys-readback",
        );
        let ids_readback = copy_buffer(
            device,
            &mut encoder,
            producer.final_source_ids(),
            word_bytes,
            "preproject-ids-readback",
        );
        let center_readback = copy_buffer(
            device,
            &mut encoder,
            producer.source_center_alpha_key(),
            center_cache_bytes,
            "preproject-center-readback",
        );
        let axes_readback = copy_buffer(
            device,
            &mut encoder,
            producer.source_axes(),
            axes_cache_bytes,
            "preproject-axes-readback",
        );
        queue.submit(Some(encoder.finish()));

        let candidate_count =
            bytemuck::cast_slice::<u8, u32>(&read_bytes(device, &candidate_readback))[0];
        let control_bytes = read_bytes(device, &control_readback);
        let draw_bytes = read_bytes(device, &draw_readback);
        let control = *bytemuck::from_bytes::<ExternalPrefixControl>(&control_bytes);
        let draw = *bytemuck::from_bytes::<PreprojectDrawIndirectArgs>(&draw_bytes);
        let count = control.count as usize;
        let keys =
            bytemuck::cast_slice::<u8, u32>(&read_bytes(device, &keys_readback))[..count].to_vec();
        let ids =
            bytemuck::cast_slice::<u8, u32>(&read_bytes(device, &ids_readback))[..count].to_vec();
        let center = bytemuck::cast_slice::<u8, [f32; 4]>(&read_bytes(device, &center_readback))
            [..capacity]
            .to_vec();
        let axes = decode_axes(
            &read_bytes(device, &axes_readback),
            capacity,
            producer.source_axes_record_bytes(),
        );
        ProducerReadback {
            candidate_count,
            control,
            draw,
            keys,
            ids,
            center,
            axes,
        }
    }

    fn run_projection_count_and_read(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        resident: &ResidentGpuResources,
        producer: &PreprojectedGpuCompute,
        camera: &Camera,
    ) -> (
        u32,
        ExternalPrefixControl,
        PreprojectDrawIndirectArgs,
        Vec<[f32; 4]>,
    ) {
        let capacity = producer.capacity() as usize;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("preproject-projection-only-oracle-encoder"),
        });
        producer.encode_projection_and_count(
            queue,
            &mut encoder,
            resident,
            camera,
            ORACLE_WIDTH,
            ORACLE_HEIGHT,
        );
        let (count_buffer, count_offset) = producer.contributor_count_buffer_and_offset();
        let count_readback = copy_buffer_at(
            device,
            &mut encoder,
            count_buffer,
            count_offset,
            size_of::<u32>() as u64,
            "preproject-current-count-readback",
        );
        let control_readback = copy_buffer(
            device,
            &mut encoder,
            producer.order_control(),
            size_of::<ExternalPrefixControl>() as u64,
            "preproject-stale-control-readback",
        );
        let draw_readback = copy_buffer(
            device,
            &mut encoder,
            producer.draw_args(),
            size_of::<PreprojectDrawIndirectArgs>() as u64,
            "preproject-stale-draw-readback",
        );
        let cache_readback = copy_buffer(
            device,
            &mut encoder,
            producer.source_center_alpha_key(),
            (capacity.max(1) * size_of::<[f32; 4]>()) as u64,
            "preproject-current-cache-readback",
        );
        queue.submit(Some(encoder.finish()));
        let count = bytemuck::cast_slice::<u8, u32>(&read_bytes(device, &count_readback))[0];
        let control =
            *bytemuck::from_bytes::<ExternalPrefixControl>(&read_bytes(device, &control_readback));
        let draw = *bytemuck::from_bytes::<PreprojectDrawIndirectArgs>(&read_bytes(
            device,
            &draw_readback,
        ));
        let cache = bytemuck::cast_slice::<u8, [f32; 4]>(&read_bytes(device, &cache_readback))
            [..capacity]
            .to_vec();
        (count, control, draw, cache)
    }

    fn assert_near(actual: f32, expected: f32, source_id: usize, component: &str) {
        let tolerance = 4.0e-5_f32.max(expected.abs() * 4.0e-5);
        assert!(
            (actual - expected).abs() <= tolerance,
            "source {source_id} {component}: actual={actual:?} expected={expected:?} tolerance={tolerance:?}",
        );
    }

    #[test]
    fn byte_plan_is_source_cache_plus_shared_dynamic_prefix_sorter() {
        let limits = wgpu::Limits::downlevel_defaults();
        let capacity = 4_099_u32;
        let plan = PreprojectGpuBytePlan::for_capacity(capacity, &limits).expect("byte plan");
        let offset_count = capacity.div_ceil(PREPROJECT_WORKGROUP_SIZE) + 1;
        assert_eq!(plan.source_center_alpha_key, 16 * u64::from(capacity));
        assert_eq!(plan.source_axes, 16 * u64::from(capacity));
        assert_eq!(plan.candidate_group_offsets, 4 * u64::from(offset_count));
        assert_eq!(plan.contributor_group_offsets, 4 * u64::from(offset_count));
        assert_eq!(
            plan.radix.radix_prefix,
            64 * u64::from(capacity.div_ceil(EXTERNAL_RADIX_TILE_SIZE))
        );
        assert_eq!(plan.radix.control, 32);
        assert_eq!(plan.draw_args, 16);
        assert_eq!(
            plan.total_static,
            plan.source_center_alpha_key
                + plan.source_axes
                + plan.candidate_group_offsets
                + plan.candidate_scan_sums
                + plan.candidate_scan_params
                + plan.contributor_group_offsets
                + plan.contributor_scan_sums
                + plan.contributor_scan_params
                + plan.radix.total_static
                + plan.draw_args
        );
    }

    #[cfg(feature = "diagnostic-surface-projected-axes16")]
    #[test]
    fn axes16_byte_plan_changes_only_the_source_axis_plane() {
        let limits = wgpu::Limits::downlevel_defaults();
        let capacity = 4_099_u32;
        let exact = PreprojectGpuBytePlan::for_capacity(capacity, &limits).expect("exact plan");
        let candidate =
            PreprojectGpuBytePlan::for_capacity_axes16(capacity, &limits).expect("axes16 plan");

        assert_eq!(exact.source_center_alpha_key, 16 * u64::from(capacity));
        assert_eq!(
            candidate.source_center_alpha_key,
            exact.source_center_alpha_key
        );
        assert_eq!(exact.source_axes, 16 * u64::from(capacity));
        assert_eq!(candidate.source_axes, 8 * u64::from(capacity));
        assert_eq!(candidate.radix, exact.radix);
        assert_eq!(candidate.draw_args, exact.draw_args);
        assert_eq!(
            exact.total_static - candidate.total_static,
            8 * u64::from(capacity)
        );
    }

    #[cfg(feature = "diagnostic-surface-projected-axes16")]
    #[test]
    fn axes16_source_projection_executes_with_exact_centers_keys_ids_and_counts() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let scene = base_scene(3);
        let resident = resident_resources(&device, &scene);
        let producer = PreprojectedGpuCompute::new_axes16(&device, &resident, QUAD_VERTEX_COUNT)
            .expect("axes16 preproject graph");
        let camera = camera();
        let position_alpha = [
            ResidentPositionAlpha {
                position_alpha: [0.0, 0.0, 2.0, 0.8],
            },
            ResidentPositionAlpha {
                position_alpha: [0.01, 0.0, 2.0, 0.8],
            },
            ResidentPositionAlpha {
                position_alpha: [0.02, 0.0, 2.0, 0.8],
            },
        ];
        let covariance0 = [ResidentCovariance0 {
            values: [0.01, 0.0, 0.0, 0.01],
        }; 3];
        let covariance1 = [ResidentCovariance1 {
            values: [0.0, 0.01],
        }; 3];
        upload_source_planes(
            &queue,
            &resident,
            &position_alpha,
            &covariance0,
            &covariance1,
        );

        assert_eq!(producer.source_center_alpha_key().size(), 3 * 16);
        assert_eq!(producer.source_axes().size(), 3 * 8);
        assert_eq!(producer.source_axes_record_bytes(), 8);

        let actual = run_and_read(&device, &queue, &resident, &producer, &camera);
        assert_eq!(actual.candidate_count, 3);
        assert_eq!(actual.control.count, 3);
        assert_eq!(actual.draw.instance_count, 3);
        assert_eq!(actual.ids, vec![0, 1, 2]);
        assert_eq!(actual.keys, vec![2.0_f32.to_bits(); 3]);

        for source_id in 0..3 {
            let expected = cpu_projection(
                position_alpha[source_id],
                covariance0[source_id],
                covariance1[source_id],
                &camera,
            )
            .expect("visible source");
            assert_eq!(
                actual.center[source_id][3].to_bits(),
                expected.center_alpha_key[3].to_bits()
            );
            for component in 0..3 {
                assert_near(
                    actual.center[source_id][component],
                    expected.center_alpha_key[component],
                    source_id,
                    "center/alpha",
                );
            }
            for component in 0..4 {
                let tolerance = 0.000_5_f32.max(expected.axes[component].abs() * 0.001);
                assert!(
                    (actual.axes[source_id][component] - expected.axes[component]).abs()
                        <= tolerance,
                    "source {source_id} axis {component}: actual={:?} expected={:?} tolerance={tolerance:?}",
                    actual.axes[source_id][component],
                    expected.axes[component],
                );
            }
        }
    }

    #[test]
    fn seven_storage_bindings_remain_below_resident_admission_floor() {
        let mut limits = wgpu::Limits::downlevel_defaults();
        limits.max_storage_buffers_per_shader_stage = 7;
        let plan = PreprojectGpuBytePlan::for_capacity(1, &limits).expect("byte plan");
        assert_eq!(
            plan.validate_limits(&limits),
            Err(ResidentGpuError::StorageBindingCountUnsupported(7))
        );
    }

    #[test]
    fn source_projection_compaction_and_full32_order_match_cpu_oracle() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let capacity = 14_usize;
        let scene = base_scene(capacity);
        let resident = resident_resources(&device, &scene);
        let producer = PreprojectedGpuCompute::new(&device, &resident, QUAD_VERTEX_COUNT)
            .expect("preproject graph");
        let camera = camera();
        let below_near = f32::from_bits(camera.intrinsics.near_plane.to_bits() - 1);
        let above_far = f32::from_bits(camera.intrinsics.far_plane.to_bits() + 1);
        let below_alpha = f32::from_bits(ALPHA_THRESHOLD.to_bits() - 1);
        let above_alpha = f32::from_bits(ALPHA_THRESHOLD.to_bits() + 1);
        let position_alpha = [
            [0.0, 0.0, 2.0, 0.8],
            [0.1, -0.1, 4.0, 0.7],
            [100.0, 0.0, 2.0, 0.8],
            [4.0, 0.0, 2.0, 0.8],
            [0.0, 0.0, camera.intrinsics.near_plane, 0.8],
            [0.0, 0.0, below_near, 0.8],
            [0.0, 0.0, camera.intrinsics.far_plane, 0.8],
            [0.0, 0.0, above_far, 0.8],
            [0.0, 0.0, 3.0, below_alpha],
            [0.0, 0.0, 3.0, ALPHA_THRESHOLD],
            [0.0, 0.0, 3.0, above_alpha],
            [0.0, 0.0, f32::NAN, 0.8],
            [0.0, 0.0, f32::INFINITY, 0.8],
            [0.0, 0.0, f32::NEG_INFINITY, 0.8],
        ]
        .map(|position_alpha| ResidentPositionAlpha { position_alpha });
        let mut covariance0 = vec![
            ResidentCovariance0 {
                values: [0.01, 0.0, 0.0, 0.01],
            };
            capacity
        ];
        let covariance1 = vec![
            ResidentCovariance1 {
                values: [0.0, 0.01],
            };
            capacity
        ];
        // A center well outside clip remains a contributor because its full
        // unscaled ellipse overlaps the viewport.
        covariance0[3].values[0] = 16.0;
        upload_source_planes(
            &queue,
            &resident,
            &position_alpha,
            &covariance0,
            &covariance1,
        );

        let actual = run_and_read(&device, &queue, &resident, &producer, &camera);
        let expected_candidate_count = position_alpha
            .iter()
            .filter(|source| {
                let position = Vec3f::new(
                    source.position_alpha[0],
                    source.position_alpha[1],
                    source.position_alpha[2],
                );
                let depth = crate::world_to_camera_depth_with_view_row(
                    position,
                    camera.pose.position,
                    [0.0, 0.0, 1.0],
                );
                crate::is_visible(depth, &camera)
                    && source.position_alpha[3].partial_cmp(&ALPHA_THRESHOLD)
                        != Some(std::cmp::Ordering::Less)
            })
            .count() as u32;
        let expected_by_source = (0..capacity)
            .map(|source_id| {
                cpu_projection(
                    position_alpha[source_id],
                    covariance0[source_id],
                    covariance1[source_id],
                    &camera,
                )
            })
            .collect::<Vec<_>>();
        let mut expected_order = expected_by_source
            .iter()
            .enumerate()
            .filter_map(|(source_id, projected)| {
                projected
                    .map(|projected| (projected.center_alpha_key[3].to_bits(), source_id as u32))
            })
            .collect::<Vec<_>>();
        expected_order.sort_by(|left, right| right.0.cmp(&left.0));

        assert_eq!(actual.candidate_count, expected_candidate_count);
        assert_eq!(actual.control.count as usize, expected_order.len());
        assert!(
            actual.candidate_count > actual.control.count,
            "the strictly offscreen candidate must remain in V but not C",
        );
        assert_eq!(actual.control.capacity_count, capacity as u32);
        assert_eq!(
            actual.control.active_group_count,
            actual.control.count.div_ceil(EXTERNAL_RADIX_TILE_SIZE)
        );
        assert_eq!(actual.draw.instance_count, actual.control.count);
        assert_eq!(actual.draw.vertex_count, QUAD_VERTEX_COUNT);
        assert_eq!(
            actual
                .keys
                .iter()
                .copied()
                .zip(actual.ids.iter().copied())
                .collect::<Vec<_>>(),
            expected_order
        );

        for (source_id, expected) in expected_by_source.into_iter().enumerate() {
            match expected {
                Some(expected) => {
                    for component in 0..3 {
                        assert_near(
                            actual.center[source_id][component],
                            expected.center_alpha_key[component],
                            source_id,
                            "center/alpha",
                        );
                    }
                    assert_eq!(
                        actual.center[source_id][3].to_bits(),
                        expected.center_alpha_key[3].to_bits(),
                        "source {source_id} full32 key cache",
                    );
                    for component in 0..4 {
                        assert_near(
                            actual.axes[source_id][component],
                            expected.axes[component],
                            source_id,
                            "axis",
                        );
                    }
                }
                None => {
                    assert_eq!(actual.center[source_id], [2.0, 2.0, 0.0, 0.0]);
                    assert_eq!(actual.axes[source_id], [0.0; 4]);
                }
            }
        }
        // Positive near/far inclusion is the established source-order
        // producer rule; a NaN depth must not enter C even though isolated
        // post-sort projection comparisons are intentionally fail-open.
        assert!(!actual.ids.contains(&11));
        assert!(!actual.ids.contains(&12));
        assert!(!actual.ids.contains(&13));
    }

    #[test]
    fn one_graph_reuses_capacity_across_full_zero_and_full_contributor_frames() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let capacity = 1_025_usize;
        let scene = base_scene(capacity);
        let resident = resident_resources(&device, &scene);
        let producer = PreprojectedGpuCompute::new(&device, &resident, QUAD_VERTEX_COUNT)
            .expect("preproject graph");
        let camera = camera();
        let covariance0 = vec![
            ResidentCovariance0 {
                values: [0.001, 0.0, 0.0, 0.001],
            };
            capacity
        ];
        let covariance1 = vec![
            ResidentCovariance1 {
                values: [0.0, 0.001],
            };
            capacity
        ];
        let full = (0..capacity)
            .map(|source_id| ResidentPositionAlpha {
                position_alpha: [
                    (source_id % 17) as f32 * 0.001 - 0.008,
                    (source_id % 19) as f32 * 0.001 - 0.009,
                    1.0 + (source_id % 97) as f32 * 0.01,
                    0.8,
                ],
            })
            .collect::<Vec<_>>();
        let zero = full
            .iter()
            .map(|source| ResidentPositionAlpha {
                position_alpha: [
                    source.position_alpha[0],
                    source.position_alpha[1],
                    source.position_alpha[2],
                    0.0,
                ],
            })
            .collect::<Vec<_>>();

        upload_source_planes(&queue, &resident, &full, &covariance0, &covariance1);
        let first = run_and_read(&device, &queue, &resident, &producer, &camera);
        assert_eq!(first.control.count, capacity as u32);
        let first_pairs = first
            .keys
            .iter()
            .copied()
            .zip(first.ids.iter().copied())
            .collect::<Vec<_>>();

        upload_source_planes(&queue, &resident, &zero, &covariance0, &covariance1);
        let empty = run_and_read(&device, &queue, &resident, &producer, &camera);
        assert_eq!(empty.control.count, 0);
        assert_eq!(empty.control.active_group_count, 0);
        assert_eq!(empty.control.dispatch_x, 0);
        assert_eq!(empty.draw.instance_count, 0);
        assert!(empty.keys.is_empty());
        assert!(empty.ids.is_empty());

        upload_source_planes(&queue, &resident, &full, &covariance0, &covariance1);
        let second = run_and_read(&device, &queue, &resident, &producer, &camera);
        assert_eq!(second.control.count, capacity as u32);
        assert_eq!(
            second
                .keys
                .iter()
                .copied()
                .zip(second.ids.iter().copied())
                .collect::<Vec<_>>(),
            first_pairs,
        );

        let all_equal = full
            .iter()
            .map(|source| ResidentPositionAlpha {
                position_alpha: [
                    source.position_alpha[0],
                    source.position_alpha[1],
                    2.0,
                    source.position_alpha[3],
                ],
            })
            .collect::<Vec<_>>();
        upload_source_planes(&queue, &resident, &all_equal, &covariance0, &covariance1);
        let equal = run_and_read(&device, &queue, &resident, &producer, &camera);
        assert_eq!(equal.control.count, capacity as u32);
        assert_eq!(equal.ids, (0..capacity as u32).collect::<Vec<_>>());
    }

    #[test]
    fn non_refresh_reprojects_complete_s_and_reports_current_c_with_stale_d() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let capacity = 129_usize;
        let scene = base_scene(capacity);
        let resident = resident_resources(&device, &scene);
        let producer = PreprojectedGpuCompute::new(&device, &resident, QUAD_VERTEX_COUNT)
            .expect("preproject graph");
        let base_camera = camera();
        let covariance0 = vec![
            ResidentCovariance0 {
                values: [0.001, 0.0, 0.0, 0.001],
            };
            capacity
        ];
        let covariance1 = vec![
            ResidentCovariance1 {
                values: [0.0, 0.001],
            };
            capacity
        ];
        let full = (0..capacity)
            .map(|source_id| ResidentPositionAlpha {
                position_alpha: [
                    (source_id % 17) as f32 * 0.001 - 0.008,
                    (source_id % 19) as f32 * 0.001 - 0.009,
                    1.0 + (source_id % 31) as f32 * 0.01,
                    0.8,
                ],
            })
            .collect::<Vec<_>>();
        upload_source_planes(&queue, &resident, &full, &covariance0, &covariance1);
        let refreshed = run_and_read(&device, &queue, &resident, &producer, &base_camera);
        assert_eq!(refreshed.control.count, capacity as u32);
        assert_eq!(refreshed.draw.instance_count, capacity as u32);

        let sparse = full
            .iter()
            .enumerate()
            .map(|(source_id, source)| ResidentPositionAlpha {
                position_alpha: [
                    source.position_alpha[0],
                    source.position_alpha[1],
                    source.position_alpha[2],
                    if source_id % 2 == 0 { 0.8 } else { 0.0 },
                ],
            })
            .collect::<Vec<_>>();
        upload_source_planes(&queue, &resident, &sparse, &covariance0, &covariance1);
        let mut moved_camera = base_camera;
        moved_camera.pose.position.x += 0.1;
        let (current_c, stale_control, stale_draw, current_cache) =
            run_projection_count_and_read(&device, &queue, &resident, &producer, &moved_camera);
        let expected_current_c = sparse
            .iter()
            .filter(|source| source.position_alpha[3] >= ALPHA_THRESHOLD)
            .count() as u32;
        assert_eq!(current_c, expected_current_c);
        assert_eq!(stale_control.count, capacity as u32);
        assert_eq!(stale_draw.instance_count, capacity as u32);
        assert_eq!(current_cache[1], [2.0, 2.0, 0.0, 0.0]);
        assert_ne!(current_cache[0][0], refreshed.center[0][0]);

        let refreshed_sparse = run_and_read(&device, &queue, &resident, &producer, &moved_camera);
        assert_eq!(refreshed_sparse.control.count, expected_current_c);
        assert_eq!(refreshed_sparse.draw.instance_count, expected_current_c);
    }
}
