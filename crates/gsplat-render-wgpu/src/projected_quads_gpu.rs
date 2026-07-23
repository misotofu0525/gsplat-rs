//! Exact preprojected hardware-quad raster path for Resident scenes.
//!
//! CPU and GPU ordering remain authoritative. A compute pass follows the
//! selected back-to-front order once, writes two independent 16-byte planes
//! per visible rank, and the existing premultiplied SortedAlpha hardware draw
//! consumes those planes without a readback.

use std::{mem::size_of, num::NonZeroU64};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::draw_pass::{SplatPipeline, create_splat_pipeline};
use crate::gpu::{GpuPrefixScan, GpuPrefixScanProfile};
use crate::gpu_error::ResidentGpuError;
use crate::projected_draw_telemetry::SurfaceProjectedDrawExecution;
use crate::resident_gpu::{RESIDENT_QUAD_VERTEX_COUNT, ResidentGpuResources};
use crate::scene::{
    PROJECT_WORKGROUP_SIZE, PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT, SCAN_ITEMS_PER_GROUP,
    SCAN_WORKGROUP_SIZE,
};
use crate::wgpu_label;

#[cfg(test)]
use crate::scene::PROJECTED_CACHE_BYTES_PER_SPLAT;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawIndirectArgs {
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Dispatch2d {
    x: u32,
    y: u32,
}

impl Dispatch2d {
    fn for_items(items: u32, limit: u32) -> Result<Self, ResidentGpuError> {
        if items == 0 {
            return Ok(Self { x: 1, y: 1 });
        }
        let logical_groups = items.div_ceil(PROJECT_WORKGROUP_SIZE);
        let limit = limit.max(1);
        let x = logical_groups.min(limit);
        let y = logical_groups.div_ceil(x);
        if y > limit {
            return Err(ResidentGpuError::DispatchLimitExceeded);
        }
        Ok(Self { x, y })
    }
}

pub(crate) type ProjectedDrawExecution = SurfaceProjectedDrawExecution;

#[cfg_attr(not(test), allow(dead_code))]
struct ProjectedContributorCounter {
    scan: GpuPrefixScan,
}

#[cfg_attr(not(test), allow(dead_code))]
struct ProjectedContributorCompaction {
    compact_pipeline: wgpu::ComputePipeline,
    finalize_pipeline: wgpu::ComputePipeline,
    compact_bind_group: wgpu::BindGroup,
    contributor_ranks: wgpu::Buffer,
    contributor_args: wgpu::Buffer,
    draw_pipeline: wgpu::RenderPipeline,
    draw_bind_group: wgpu::BindGroup,
}

/// Complete but unpublished optional Compact graph. Keeping the candidate
/// opaque lets the presenter validate it in a dedicated WebGPU error scope;
/// a failed candidate is simply dropped while the already-published exact
/// Candidate path remains usable.
pub(crate) struct PreparedProjectedContributorCompaction(ProjectedContributorCompaction);

pub(crate) struct ProjectedQuadsGpu {
    capacity: u32,
    project_pipeline: wgpu::ComputePipeline,
    project_layout: wgpu::BindGroupLayout,
    cpu_project_bind_group: wgpu::BindGroup,
    gpu_project_bind_group: Option<wgpu::BindGroup>,
    draw_pipeline: wgpu::RenderPipeline,
    draw_bind_group: wgpu::BindGroup,
    projected_center_source: wgpu::Buffer,
    projected_axes: wgpu::Buffer,
    contributor_group_offsets: wgpu::Buffer,
    contributor_counter: Option<ProjectedContributorCounter>,
    #[cfg_attr(not(test), allow(dead_code))]
    contributor_compaction: Option<ProjectedContributorCompaction>,
    cpu_draw_args: wgpu::Buffer,
    dispatch_limit: u32,
}

impl ProjectedQuadsGpu {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        resident: &ResidentGpuResources,
    ) -> Result<Self, ResidentGpuError> {
        Self::new_with_indirect_execution(device, target_format, resident, false)
    }

    /// Builds the optional exact contributor compactor only when the adapter
    /// exposes indirect execution. The default constructor deliberately keeps
    /// the existing direct guarded path for downlevel adapters such as the iOS
    /// simulator.
    pub(crate) fn new_with_indirect_execution(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        resident: &ResidentGpuResources,
        indirect_execution_supported: bool,
    ) -> Result<Self, ResidentGpuError> {
        let mut projected = Self::new_candidate(device, target_format, resident)?;
        if indirect_execution_supported
            && let Some(compaction) = projected.create_contributor_compaction_candidate(
                device,
                target_format,
                resident,
            )?
        {
            projected.publish_contributor_compaction(compaction);
        }
        Ok(projected)
    }

    /// Constructs only the complete exact Candidate graph. Optional Compact
    /// resources are admitted separately so their asynchronous validation or
    /// memory failure can never make Candidate unavailable.
    fn new_candidate(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        resident: &ResidentGpuResources,
    ) -> Result<Self, ResidentGpuError> {
        let capacity =
            u32::try_from(resident.capacity).map_err(|_| ResidentGpuError::AddressSpaceExceeded)?;
        let plane_bytes = u64::from(capacity)
            .checked_mul(PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let binding_limit = u64::from(device.limits().max_storage_buffer_binding_size)
            .min(device.limits().max_buffer_size);
        for (resource, bytes, minimum) in [
            (
                "projected center/source plane",
                plane_bytes,
                PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT,
            ),
            (
                "projected axes plane",
                plane_bytes,
                PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT,
            ),
        ] {
            if bytes.max(minimum) > binding_limit {
                return Err(ResidentGpuError::BindingLimitExceeded {
                    resource,
                    required_bytes: bytes.max(minimum),
                    limit_bytes: binding_limit,
                });
            }
        }
        Dispatch2d::for_items(
            capacity,
            device.limits().max_compute_workgroups_per_dimension,
        )?;

        let projected_center_source = storage_buffer(
            device,
            "gsplat-projected-quads-center-source",
            plane_bytes.max(PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT),
        );
        let projected_axes = storage_buffer(
            device,
            "gsplat-projected-quads-axes",
            plane_bytes.max(PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT),
        );
        let contributor_group_count = capacity.div_ceil(PROJECT_WORKGROUP_SIZE);
        let contributor_group_offset_count = contributor_group_count
            .checked_add(1)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let contributor_group_offsets = storage_buffer_with_usage(
            device,
            "gsplat-projected-quads-contributor-group-offsets",
            u64::from(contributor_group_offset_count) * size_of::<u32>() as u64,
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        );
        let cpu_draw_args = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-projected-quads-cpu-draw-args"),
            contents: bytemuck::bytes_of(&DrawIndirectArgs {
                vertex_count: RESIDENT_QUAD_VERTEX_COUNT,
                instance_count: 0,
                first_vertex: 0,
                first_instance: 0,
            }),
            // CPU-order drawing is direct; this buffer only supplies the
            // authoritative visible-count guard to the projection shader.
            // Requiring INDIRECT here rejects otherwise valid adapters (for
            // example the iOS simulator's Metal adapter) for no semantic gain.
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        });

        let project_layout = create_project_layout(device);
        let project_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("gsplat-projected-quads-project-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/projected_quads_project.wgsl").into(),
            ),
        });
        let project_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: wgpu_label("gsplat-projected-quads-project-pipeline-layout"),
                bind_group_layouts: &[&project_layout],
                immediate_size: 0,
            });
        let project_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: wgpu_label("gsplat-projected-quads-project-pipeline"),
            layout: Some(&project_pipeline_layout),
            module: &project_shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let cpu_project_bind_group = create_project_bind_group(
            device,
            &project_layout,
            "gsplat-projected-quads-cpu-project-bg",
            ProjectBindGroupResources {
                order: &resident.order_buffer,
                resident,
                projected_center_source: &projected_center_source,
                projected_axes: &projected_axes,
                draw_args: &cpu_draw_args,
                contributor_group_offsets: &contributor_group_offsets,
            },
        );

        let draw_layout = create_draw_layout(device);
        let draw_pipeline = create_splat_pipeline(
            device,
            &draw_layout,
            target_format,
            SplatPipeline {
                shader_label: "gsplat-projected-quads-draw-shader",
                shader_source: include_str!("../shaders/projected_quads_draw.wgsl"),
                layout_label: "gsplat-projected-quads-draw-pipeline-layout",
                pipeline_label: "gsplat-projected-quads-draw-pipeline",
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
        );
        let draw_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-projected-quads-draw-bg"),
            layout: &draw_layout,
            entries: &[
                entry(0, &projected_center_source),
                entry(1, &projected_axes),
                entry(2, &resident.resolved_color_buffer),
            ],
        });
        // Counting is useful to both execution modes and is intentionally
        // admitted independently from the much larger compact rank plane.
        // Adapters below the portable scan floor retain the exact sentinel
        // fallback rather than losing the complete Candidate draw path.
        let contributor_counter = contributor_scan_supported(device).then(|| {
            create_contributor_counter(
                device,
                contributor_group_offset_count,
                &contributor_group_offsets,
            )
        });
        Ok(Self {
            capacity,
            project_pipeline,
            project_layout,
            cpu_project_bind_group,
            gpu_project_bind_group: None,
            draw_pipeline,
            draw_bind_group,
            projected_center_source,
            projected_axes,
            contributor_group_offsets,
            contributor_counter,
            contributor_compaction: None,
            cpu_draw_args,
            dispatch_limit: device.limits().max_compute_workgroups_per_dimension,
        })
    }

    pub(crate) fn create_contributor_compaction_candidate(
        &self,
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        resident: &ResidentGpuResources,
    ) -> Result<Option<PreparedProjectedContributorCompaction>, ResidentGpuError> {
        if self.contributor_counter.is_none() || self.contributor_compaction.is_some() {
            return Ok(None);
        }
        let contributor_rank_bytes = u64::from(self.capacity)
            .checked_mul(size_of::<u32>() as u64)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let binding_limit = u64::from(device.limits().max_storage_buffer_binding_size)
            .min(device.limits().max_buffer_size);
        if contributor_rank_bytes.max(size_of::<u32>() as u64) > binding_limit {
            return Ok(None);
        }
        create_contributor_compaction(
            device,
            target_format,
            resident,
            contributor_rank_bytes,
            &self.projected_center_source,
            &self.projected_axes,
            &self.contributor_group_offsets,
        )
        .map(PreparedProjectedContributorCompaction)
        .map(Some)
    }

    pub(crate) fn publish_contributor_compaction(
        &mut self,
        prepared: PreparedProjectedContributorCompaction,
    ) {
        debug_assert!(self.contributor_compaction.is_none());
        self.contributor_compaction = Some(prepared.0);
    }

    pub(crate) fn create_gpu_order_bind_group_candidate(
        &self,
        device: &wgpu::Device,
        resident: &ResidentGpuResources,
        order: &wgpu::Buffer,
        indirect_args: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        create_project_bind_group(
            device,
            &self.project_layout,
            "gsplat-projected-quads-gpu-project-bg",
            ProjectBindGroupResources {
                order,
                resident,
                projected_center_source: &self.projected_center_source,
                projected_axes: &self.projected_axes,
                draw_args: indirect_args,
                contributor_group_offsets: &self.contributor_group_offsets,
            },
        )
    }

    pub(crate) fn publish_gpu_order_bind_group(&mut self, prepared: wgpu::BindGroup) {
        debug_assert!(self.gpu_project_bind_group.is_none());
        self.gpu_project_bind_group = Some(prepared);
    }

    pub(crate) fn gpu_order_bind_group_is_prepared(&self) -> bool {
        self.gpu_project_bind_group.is_some()
    }

    pub(crate) fn ensure_gpu_order_bind_group(
        &mut self,
        device: &wgpu::Device,
        resident: &ResidentGpuResources,
        order: &wgpu::Buffer,
        indirect_args: &wgpu::Buffer,
    ) -> Result<(), ResidentGpuError> {
        if self.gpu_project_bind_group.is_some() {
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (device, resident, order, indirect_args);
            Err(ResidentGpuError::GpuOrderInitialization(
                "browser projected GPU-order binding must be prepared asynchronously before use"
                    .into(),
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let prepared =
                self.create_gpu_order_bind_group_candidate(device, resident, order, indirect_args);
            self.publish_gpu_order_bind_group(prepared);
            Ok(())
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn encode_cpu_projection(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        visible_count: u32,
    ) -> Result<(), ResidentGpuError> {
        self.encode_cpu_projection_for_draw(
            queue,
            encoder,
            visible_count,
            ProjectedDrawExecution::Candidate,
        )?;
        Ok(())
    }

    pub(crate) fn encode_cpu_projection_for_draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        visible_count: u32,
        requested: ProjectedDrawExecution,
    ) -> Result<ProjectedDrawExecution, ResidentGpuError> {
        queue.write_buffer(
            &self.cpu_draw_args,
            0,
            bytemuck::bytes_of(&DrawIndirectArgs {
                vertex_count: RESIDENT_QUAD_VERTEX_COUNT,
                instance_count: visible_count,
                first_vertex: 0,
                // The flag is internal to projection and is never consumed as
                // an indirect draw for CPU ordering. It enables the exact
                // low-limit sentinel fallback when hierarchical scan is absent.
                first_instance: u32::from(self.contributor_counter.is_none()),
            }),
        );
        self.encode_projection_for_draw(
            encoder,
            &self.cpu_project_bind_group,
            visible_count,
            requested,
        )
    }

    /// Projects the complete CPU-sorted candidate prefix and, when indirect
    /// execution was admitted at construction, emits a stable compact prefix
    /// of candidate ranks plus independent draw arguments. Returning `false`
    /// means the exact legacy direct guarded projection was encoded instead.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn encode_cpu_projection_compacted(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        visible_count: u32,
    ) -> Result<bool, ResidentGpuError> {
        Ok(self.encode_cpu_projection_for_draw(
            queue,
            encoder,
            visible_count,
            ProjectedDrawExecution::Compact,
        )? == ProjectedDrawExecution::Compact)
    }

    /// GPU-order counterpart of [`Self::encode_cpu_projection_compacted`].
    /// The sorter-owned candidate args remain read-only and authoritative;
    /// compaction writes only this object's contributor args and rank buffer.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn encode_gpu_projection_compacted(
        &self,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<bool, ResidentGpuError> {
        Ok(
            self.encode_gpu_projection_for_draw(encoder, ProjectedDrawExecution::Compact)?
                == ProjectedDrawExecution::Compact,
        )
    }

    pub(crate) fn encode_gpu_projection_for_draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        requested: ProjectedDrawExecution,
    ) -> Result<ProjectedDrawExecution, ResidentGpuError> {
        let bind_group = self.gpu_project_bind_group.as_ref().ok_or_else(|| {
            ResidentGpuError::GpuOrderInternal("projected quad GPU binding is unavailable".into())
        })?;
        self.encode_projection_for_draw(encoder, bind_group, self.capacity, requested)
    }

    fn encode_projection_for_draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        project_bind_group: &wgpu::BindGroup,
        dispatch_items: u32,
        requested: ProjectedDrawExecution,
    ) -> Result<ProjectedDrawExecution, ResidentGpuError> {
        let execution = if requested == ProjectedDrawExecution::Compact
            && self.contributor_compaction.is_some()
            && self.contributor_counter.is_some()
        {
            ProjectedDrawExecution::Compact
        } else {
            ProjectedDrawExecution::Candidate
        };
        if execution == ProjectedDrawExecution::Compact {
            let compaction = self.contributor_compaction.as_ref().expect("checked above");
            encoder.clear_buffer(
                &compaction.contributor_args,
                size_of::<u32>() as u64,
                Some(size_of::<u32>() as u64),
            );
        }
        self.encode_projection(encoder, project_bind_group, dispatch_items)?;
        if let Some(counter) = &self.contributor_counter {
            counter.encode_forward_scan(encoder);
            if execution == ProjectedDrawExecution::Compact {
                counter.encode_reverse_offsets(encoder);
                self.contributor_compaction
                    .as_ref()
                    .expect("checked above")
                    .encode_compaction(encoder, dispatch_items, self.dispatch_limit)?;
            }
        }
        Ok(execution)
    }

    fn encode_projection(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        bind_group: &wgpu::BindGroup,
        dispatch_items: u32,
    ) -> Result<(), ResidentGpuError> {
        // Clear the full fixed-capacity count/offset plane on every projection,
        // including the downlevel direct-draw path. CPU ordering may project a
        // shorter prefix than the previous frame, while GPU ordering writes all
        // capacity groups. The final sentinel is also the exact contributor
        // count available to same-command-buffer telemetry before or after the
        // optional in-place exclusive scan.
        encoder.clear_buffer(&self.contributor_group_offsets, 0, None);
        if dispatch_items == 0 {
            return Ok(());
        }
        let dispatch = Dispatch2d::for_items(dispatch_items, self.dispatch_limit)?;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: wgpu_label("gsplat-projected-quads-project-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.project_pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.dispatch_workgroups(dispatch.x, dispatch.y, 1);
        Ok(())
    }

    pub(crate) fn draw_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.draw_pipeline
    }

    pub(crate) fn draw_bind_group(&self) -> &wgpu::BindGroup {
        &self.draw_bind_group
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn contributor_indirect_available(&self) -> bool {
        self.contributor_compaction.is_some()
    }

    pub(crate) fn resolve_draw_execution(
        &self,
        requested: ProjectedDrawExecution,
    ) -> ProjectedDrawExecution {
        if requested == ProjectedDrawExecution::Compact
            && self.contributor_compaction.is_some()
            && self.contributor_counter.is_some()
        {
            ProjectedDrawExecution::Compact
        } else {
            ProjectedDrawExecution::Candidate
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn contributor_draw_pipeline(&self) -> Option<&wgpu::RenderPipeline> {
        self.contributor_compaction
            .as_ref()
            .map(|compaction| &compaction.draw_pipeline)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn contributor_draw_bind_group(&self) -> Option<&wgpu::BindGroup> {
        self.contributor_compaction
            .as_ref()
            .map(|compaction| &compaction.draw_bind_group)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn contributor_indirect_args(&self) -> Option<&wgpu::Buffer> {
        self.contributor_compaction
            .as_ref()
            .map(|compaction| &compaction.contributor_args)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn contributor_ranks(&self) -> Option<&wgpu::Buffer> {
        self.contributor_compaction
            .as_ref()
            .map(|compaction| &compaction.contributor_ranks)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cpu_candidate_args(&self) -> &wgpu::Buffer {
        &self.cpu_draw_args
    }

    pub(crate) fn contributor_count_buffer_and_offset(&self) -> (&wgpu::Buffer, u64) {
        if let Some(counter) = &self.contributor_counter {
            return counter.exact_count_buffer_and_offset();
        }
        let sentinel = self.capacity.div_ceil(PROJECT_WORKGROUP_SIZE);
        (&self.contributor_group_offsets, u64::from(sentinel) * 4)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl ProjectedContributorCounter {
    fn encode_forward_scan(&self, encoder: &mut wgpu::CommandEncoder) {
        self.scan.encode_forward(encoder);
    }

    fn encode_reverse_offsets(&self, encoder: &mut wgpu::CommandEncoder) {
        self.scan.encode_reverse(encoder);
    }

    fn exact_count_buffer_and_offset(&self) -> (&wgpu::Buffer, u64) {
        self.scan.exact_count_buffer_and_offset()
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl ProjectedContributorCompaction {
    fn encode_compaction(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        dispatch_items: u32,
        dispatch_limit: u32,
    ) -> Result<(), ResidentGpuError> {
        if dispatch_items > 0 {
            encode_compute_stage(
                encoder,
                &self.compact_pipeline,
                &self.compact_bind_group,
                &[],
                Dispatch2d::for_items(dispatch_items, dispatch_limit)?,
                "gsplat-projected-contributor-compact-pass",
            );
        }
        encode_compute_stage(
            encoder,
            &self.finalize_pipeline,
            &self.compact_bind_group,
            &[],
            Dispatch2d { x: 1, y: 1 },
            "gsplat-projected-contributor-finalize-pass",
        );
        Ok(())
    }
}

fn create_project_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: wgpu_label("gsplat-projected-quads-project-bgl"),
        entries: &[
            storage_layout(0, true, wgpu::ShaderStages::COMPUTE),
            storage_layout(1, true, wgpu::ShaderStages::COMPUTE),
            storage_layout(2, true, wgpu::ShaderStages::COMPUTE),
            storage_layout(3, true, wgpu::ShaderStages::COMPUTE),
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            storage_layout(5, false, wgpu::ShaderStages::COMPUTE),
            storage_layout(6, false, wgpu::ShaderStages::COMPUTE),
            storage_layout(7, false, wgpu::ShaderStages::COMPUTE),
            storage_layout(8, false, wgpu::ShaderStages::COMPUTE),
        ],
    })
}

fn create_draw_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: wgpu_label("gsplat-projected-quads-draw-bgl"),
        entries: &[
            storage_layout(0, true, wgpu::ShaderStages::VERTEX),
            storage_layout(1, true, wgpu::ShaderStages::VERTEX),
            storage_layout(2, true, wgpu::ShaderStages::VERTEX),
        ],
    })
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

struct ProjectBindGroupResources<'a> {
    order: &'a wgpu::Buffer,
    resident: &'a ResidentGpuResources,
    projected_center_source: &'a wgpu::Buffer,
    projected_axes: &'a wgpu::Buffer,
    draw_args: &'a wgpu::Buffer,
    contributor_group_offsets: &'a wgpu::Buffer,
}

fn create_project_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &'static str,
    resources: ProjectBindGroupResources<'_>,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label(label),
        layout,
        entries: &[
            entry(0, resources.order),
            entry(1, &resources.resident.position_alpha_buffer),
            entry(2, &resources.resident.covariance0_buffer),
            entry(3, &resources.resident.covariance1_buffer),
            entry(4, &resources.resident.draw_params_buffer),
            entry(5, resources.projected_center_source),
            entry(6, resources.projected_axes),
            entry(7, resources.draw_args),
            entry(8, resources.contributor_group_offsets),
        ],
    })
}

fn contributor_scan_supported(device: &wgpu::Device) -> bool {
    device.limits().max_compute_invocations_per_workgroup >= SCAN_WORKGROUP_SIZE
        && device.limits().max_compute_workgroup_size_x >= SCAN_WORKGROUP_SIZE
        && device.limits().max_compute_workgroup_storage_size
            >= SCAN_ITEMS_PER_GROUP * size_of::<u32>() as u32
}

fn create_contributor_counter(
    device: &wgpu::Device,
    offset_count: u32,
    contributor_group_offsets: &wgpu::Buffer,
) -> ProjectedContributorCounter {
    debug_assert!(contributor_scan_supported(device));
    let scan = GpuPrefixScan::new_profiled(
        device,
        contributor_group_offsets,
        offset_count,
        device.limits().max_compute_workgroups_per_dimension,
        GpuPrefixScanProfile {
            bind_group_layout: "gsplat-projected-contributor-scan-bgl",
            shader: "gsplat-projected-contributor-scan-shader",
            pipeline_layout: "gsplat-projected-contributor-scan-pipeline-layout",
            scan_pipeline: "gsplat-projected-contributor-scan-pipeline",
            add_offsets_pipeline: "gsplat-projected-contributor-add-offsets-pipeline",
            sums: "gsplat-projected-contributor-scan-sums",
            params: "gsplat-projected-contributor-scan-params",
            bind_group: "gsplat-projected-contributor-scan-bg",
            sums_usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            scan_pass: "gsplat-projected-contributor-scan-pass",
            add_offsets_pass: "gsplat-projected-contributor-add-offsets-pass",
        },
    )
    .expect("scan hierarchy was admitted by the projection dispatch guard");
    ProjectedContributorCounter { scan }
}

#[allow(clippy::too_many_arguments)]
fn create_contributor_compaction(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
    resident: &ResidentGpuResources,
    contributor_rank_bytes: u64,
    projected_center_source: &wgpu::Buffer,
    projected_axes: &wgpu::Buffer,
    contributor_group_offsets: &wgpu::Buffer,
) -> Result<ProjectedContributorCompaction, ResidentGpuError> {
    let contributor_ranks = storage_buffer_with_usage(
        device,
        "gsplat-projected-contributor-ranks",
        contributor_rank_bytes.max(size_of::<u32>() as u64),
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    );
    let contributor_args = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: wgpu_label("gsplat-projected-contributor-draw-args"),
        contents: bytemuck::bytes_of(&DrawIndirectArgs {
            vertex_count: RESIDENT_QUAD_VERTEX_COUNT,
            instance_count: 0,
            first_vertex: 0,
            first_instance: 0,
        }),
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::INDIRECT
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
    });

    let compact_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: wgpu_label("gsplat-projected-contributor-compact-bgl"),
        entries: &[
            storage_layout(0, true, wgpu::ShaderStages::COMPUTE),
            storage_layout(1, true, wgpu::ShaderStages::COMPUTE),
            storage_layout(2, false, wgpu::ShaderStages::COMPUTE),
            storage_layout(3, false, wgpu::ShaderStages::COMPUTE),
            uniform_layout(
                4,
                false,
                NonZeroU64::new(size_of::<crate::GpuSurfaceRenderParams>() as u64),
            ),
        ],
    });
    let compact_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: wgpu_label("gsplat-projected-contributor-compact-shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../shaders/projected_quads_compact.wgsl").into(),
        ),
    });
    let compact_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: wgpu_label("gsplat-projected-contributor-compact-pipeline-layout"),
        bind_group_layouts: &[&compact_layout],
        immediate_size: 0,
    });
    let compact_pipeline = create_compute_pipeline(
        device,
        &compact_shader,
        &compact_pipeline_layout,
        "compact_contributor_ranks",
        "gsplat-projected-contributor-compact-pipeline",
    );
    let finalize_pipeline = create_compute_pipeline(
        device,
        &compact_shader,
        &compact_pipeline_layout,
        "finalize_contributor_args",
        "gsplat-projected-contributor-finalize-pipeline",
    );
    let compact_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label("gsplat-projected-contributor-compact-bg"),
        layout: &compact_layout,
        entries: &[
            entry(0, projected_center_source),
            entry(1, contributor_group_offsets),
            entry(2, &contributor_ranks),
            entry(3, &contributor_args),
            entry(4, &resident.draw_params_buffer),
        ],
    });

    let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: wgpu_label("gsplat-projected-contributor-draw-bgl"),
        entries: &[
            storage_layout(0, true, wgpu::ShaderStages::VERTEX),
            storage_layout(1, true, wgpu::ShaderStages::VERTEX),
            storage_layout(2, true, wgpu::ShaderStages::VERTEX),
            storage_layout(3, true, wgpu::ShaderStages::VERTEX),
        ],
    });
    let draw_pipeline = create_splat_pipeline(
        device,
        &draw_layout,
        target_format,
        SplatPipeline {
            shader_label: "gsplat-projected-contributor-draw-shader",
            shader_source: include_str!("../shaders/projected_quads_draw_compacted.wgsl"),
            layout_label: "gsplat-projected-contributor-draw-pipeline-layout",
            pipeline_label: "gsplat-projected-contributor-draw-pipeline",
            topology: wgpu::PrimitiveTopology::TriangleStrip,
        },
    );
    let draw_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label("gsplat-projected-contributor-draw-bg"),
        layout: &draw_layout,
        entries: &[
            entry(0, projected_center_source),
            entry(1, projected_axes),
            entry(2, &resident.resolved_color_buffer),
            entry(3, &contributor_ranks),
        ],
    });

    Ok(ProjectedContributorCompaction {
        compact_pipeline,
        finalize_pipeline,
        compact_bind_group,
        contributor_ranks,
        contributor_args,
        draw_pipeline,
        draw_bind_group,
    })
}

fn uniform_layout(
    binding: u32,
    dynamic: bool,
    min_binding_size: Option<NonZeroU64>,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
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

#[cfg_attr(not(test), allow(dead_code))]
fn encode_compute_stage(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    dynamic_offsets: &[u32],
    dispatch: Dispatch2d,
    label: &'static str,
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: wgpu_label(label),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, dynamic_offsets);
    pass.dispatch_workgroups(dispatch.x, dispatch.y, 1);
}

fn storage_buffer(device: &wgpu::Device, label: &'static str, size: u64) -> wgpu::Buffer {
    storage_buffer_with_usage(device, label, size, wgpu::BufferUsages::STORAGE)
}

fn storage_buffer_with_usage(
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
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    use crate::draw_pass::{
        SplatDraw, SplatIndirectDraw, encode_splat_draw_into, encode_splat_indirect_draw_into,
    };

    #[cfg(not(target_arch = "wasm32"))]
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
            limits.max_storage_buffers_per_shader_stage =
                crate::resident_gpu::RESIDENT_COLOR_STORAGE_BINDINGS;
            if !limits.check_limits(&adapter.limits()) {
                return None;
            }
            adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("projected-quads-test-device"),
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

    #[cfg(not(target_arch = "wasm32"))]
    fn comparison_scene() -> crate::ResidentSceneCpu {
        let source = gsplat_core::SceneBuffers {
            positions: vec![
                gsplat_core::Vec3f::new(-0.08, 0.02, 2.0),
                gsplat_core::Vec3f::new(0.10, -0.04, 2.7),
                gsplat_core::Vec3f::new(0.0, 0.08, 3.4),
            ],
            // Include low-opacity splats so the conservative alpha-support
            // bound and fragment cutoff remain part of the byte-for-byte
            // Projected-vs-Global oracle.
            opacity: vec![2.0, -1.0, -3.0],
            scale_xyz: vec![[-2.1, -2.3, -2.2], [-2.0, -2.2, -2.4], [-1.9, -2.4, -2.1]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 3],
            color_dc: vec![[0.8, -0.2, -0.3], [-0.2, 0.7, -0.1], [-0.3, -0.1, 0.9]],
            sh_degree: 0,
            sh_rest: None,
        };
        crate::ResidentSceneCpu::encode(&source).expect("comparison resident scene")
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn contributor_scene(count: usize) -> (crate::ResidentSceneCpu, Vec<bool>) {
        let mut positions = Vec::with_capacity(count);
        let mut opacity = Vec::with_capacity(count);
        let mut expected_by_source = Vec::with_capacity(count);
        for source_id in 0..count {
            let class = source_id % 5;
            let contributes = matches!(class, 0 | 2 | 4);
            positions.push(if class == 1 {
                // Far enough outside the alpha-bounded clip quad that the
                // conservative two-ULP expansion still proves zero coverage.
                gsplat_core::Vec3f::new(100.0, 0.0, 2.0)
            } else {
                gsplat_core::Vec3f::new(
                    (source_id % 7) as f32 * 0.002 - 0.006,
                    (source_id % 11) as f32 * 0.001 - 0.005,
                    2.0,
                )
            });
            // Class 3 has maximum alpha far below 1/255. Other classes use a
            // comfortably contributing post-sigmoid alpha.
            opacity.push(if class == 3 { -20.0 } else { 4.0 });
            expected_by_source.push(contributes);
        }
        let source = gsplat_core::SceneBuffers {
            positions,
            opacity,
            scale_xyz: vec![[-3.0, -3.0, -3.0]; count],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; count],
            color_dc: vec![[0.2, 0.1, -0.1]; count],
            sh_degree: 0,
            sh_rest: None,
        };
        (
            crate::ResidentSceneCpu::encode(&source).expect("contributor resident scene"),
            expected_by_source,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn adversarial_clip_scene() -> crate::ResidentSceneCpu {
        let huge_x = 1_000_000.0_f32;
        let cancelling_x_scale = (huge_x / 3.0).ln();
        let source = gsplat_core::SceneBuffers {
            positions: vec![
                gsplat_core::Vec3f::new(0.0, 0.0, 2.0),
                gsplat_core::Vec3f::new(100.0, 0.0, 2.0),
                gsplat_core::Vec3f::new(huge_x, 0.0, 2.0),
                gsplat_core::Vec3f::new(1.20, 0.0, 2.0),
                gsplat_core::Vec3f::new(0.0, 0.0, 2.0),
            ],
            opacity: vec![4.0, 4.0, 4.0, 4.0, -20.0],
            scale_xyz: vec![
                [-2.5, -2.5, -2.5],
                [-4.0, -4.0, -4.0],
                [cancelling_x_scale, -4.0, -4.0],
                [-2.4, -3.0, -3.0],
                [-2.5, -2.5, -2.5],
            ],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 5],
            color_dc: vec![
                [0.8, -0.2, -0.3],
                [-0.2, 0.8, -0.3],
                [-0.3, -0.2, 0.8],
                [0.7, 0.6, -0.2],
                [0.8, 0.8, 0.8],
            ],
            sh_degree: 0,
            sh_rest: None,
        };
        crate::ResidentSceneCpu::encode(&source).expect("adversarial clip resident scene")
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn target(device: &wgpu::Device, label: &'static str) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn copy_target(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        label: &'static str,
    ) -> wgpu::Buffer {
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: 64 * 64 * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(64 * 4),
                    rows_per_image: Some(64),
                },
            },
            wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
        );
        readback
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_buffer(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        receiver.recv().expect("map callback").expect("map result");
        slice.get_mapped_range().to_vec()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn copy_buffer(
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Buffer,
        size: u64,
        label: &'static str,
    ) -> wgpu::Buffer {
        copy_buffer_range(device, encoder, source, 0, size, label)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn copy_buffer_range(
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

    #[cfg(not(target_arch = "wasm32"))]
    fn read_u32s(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u32> {
        read_buffer(device, buffer)
            .chunks_exact(size_of::<u32>())
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("u32 readback")))
            .collect()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn compact_cpu_frame(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        resident: &ResidentGpuResources,
        projected: &ProjectedQuadsGpu,
        order: &[u32],
    ) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
        let camera = gsplat_core::Camera::default();
        resident
            .prepare_cpu_order(queue, order, &camera, 64, 64, true)
            .expect("CPU candidate order");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("projected-contributor-readback-encoder"),
        });
        assert!(
            projected
                .encode_cpu_projection_compacted(queue, &mut encoder, order.len() as u32)
                .expect("CPU contributor projection")
        );
        let candidate_readback = copy_buffer(
            device,
            &mut encoder,
            projected.cpu_candidate_args(),
            size_of::<DrawIndirectArgs>() as u64,
            "projected-candidate-args-readback",
        );
        let contributor_readback = copy_buffer(
            device,
            &mut encoder,
            projected
                .contributor_indirect_args()
                .expect("contributor args"),
            size_of::<DrawIndirectArgs>() as u64,
            "projected-contributor-args-readback",
        );
        let ranks_readback = copy_buffer(
            device,
            &mut encoder,
            projected.contributor_ranks().expect("contributor ranks"),
            (resident.capacity.max(1) * size_of::<u32>()) as u64,
            "projected-contributor-ranks-readback",
        );
        queue.submit(Some(encoder.finish()));
        (
            read_u32s(device, &candidate_readback),
            read_u32s(device, &contributor_readback),
            read_u32s(device, &ranks_readback),
        )
    }

    #[test]
    fn dispatch_flattens_across_two_dimensions() {
        assert_eq!(
            Dispatch2d::for_items(0, 7).unwrap(),
            Dispatch2d { x: 1, y: 1 }
        );
        assert_eq!(
            Dispatch2d::for_items(128 * 7, 7).unwrap(),
            Dispatch2d { x: 7, y: 1 }
        );
        assert_eq!(
            Dispatch2d::for_items(128 * 8, 7).unwrap(),
            Dispatch2d { x: 7, y: 2 }
        );
        assert_eq!(
            Dispatch2d::for_items(128 * 49, 7).unwrap(),
            Dispatch2d { x: 7, y: 7 }
        );
        assert!(matches!(
            Dispatch2d::for_items(128 * 50, 7),
            Err(ResidentGpuError::DispatchLimitExceeded)
        ));

        let portable_binding_boundary = 8_388_608_u32;
        let boundary_dispatch = Dispatch2d::for_items(portable_binding_boundary, 65_535)
            .expect("portable boundary dispatch");
        let logical_groups = portable_binding_boundary.div_ceil(PROJECT_WORKGROUP_SIZE);
        assert_eq!(logical_groups, 65_536);
        assert_eq!(boundary_dispatch, Dispatch2d { x: 65_535, y: 2 });
        assert!(boundary_dispatch.x * boundary_dispatch.y > logical_groups);
        for shader in [
            include_str!("../shaders/projected_quads_project.wgsl"),
            include_str!("../shaders/projected_quads_compact.wgsl"),
        ] {
            assert!(shader.contains("if (group >= capacity_group_count)"));
        }
    }

    #[test]
    fn projected_cache_keeps_each_binding_at_sixteen_bytes_per_splat() {
        let count = 8_388_608_u64;
        assert_eq!(count * PROJECTED_CACHE_PLANE_BYTES_PER_SPLAT, 128_u64 << 20);
        assert_eq!(count * PROJECTED_CACHE_BYTES_PER_SPLAT, 256_u64 << 20);
    }

    fn alpha_extent_scale(alpha: f32) -> f32 {
        ((alpha.max(1.0e-12) * 256.0).ln() / 4.5)
            .clamp(0.0, 1.0)
            .sqrt()
    }

    #[test]
    fn conservative_alpha_support_never_removes_a_contributing_fragment() {
        let fragment_cutoff = 1.0 / 255.0;
        let bound_cutoff = 1.0 / 256.0;
        assert!(alpha_extent_scale(fragment_cutoff) > 0.0);
        assert_eq!(alpha_extent_scale(bound_cutoff), 0.0);

        for alpha in [
            0.0,
            bound_cutoff,
            fragment_cutoff,
            0.01,
            0.1,
            0.35,
            0.5,
            1.0,
        ] {
            let extent = alpha_extent_scale(alpha);
            assert!((0.0..=1.0).contains(&extent));
            if extent < 1.0 {
                let just_outside_r2 = extent.mul_add(extent, 1.0e-5);
                let outside_alpha = alpha * (-4.5 * just_outside_r2).exp();
                assert!(
                    outside_alpha < fragment_cutoff,
                    "alpha={alpha} extent={extent} outside_alpha={outside_alpha}"
                );
            }
        }

        // Equality is retained by the shader's strict `< 1/255` discard.
        assert_eq!(fragment_cutoff * (-4.5_f32 * 0.0).exp(), fragment_cutoff);
    }

    #[test]
    fn contributor_alpha_predicate_keeps_the_exact_fragment_boundary() {
        let threshold = 1.0_f32 / 255.0;
        let below = f32::from_bits(threshold.to_bits() - 1);
        let above = f32::from_bits(threshold.to_bits() + 1);
        let contributes =
            |alpha: f32| alpha.partial_cmp(&threshold) != Some(std::cmp::Ordering::Less);

        assert!(!contributes(below));
        assert!(contributes(threshold));
        assert!(contributes(above));
        assert!(contributes(f32::NAN), "ambiguous alpha must fail open");
    }

    #[test]
    fn conservative_full_quad_clip_envelope_keeps_boundaries_and_huge_cancellation() {
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
        fn conservative_maximum(center: f32, axis_u: f32, axis_v: f32) -> f32 {
            let extent = next_up(axis_u.abs() + axis_v.abs());
            next_up(center + extent)
        }

        assert!(conservative_maximum(-2.0, 1.0, 0.0) >= -1.0);
        assert!(conservative_maximum(-4.0, 1.0, 0.0) < -1.0);

        // At this magnitude one f32 ULP is two clip-space units. Expanding
        // each positive extent operation outwards retains a cancellation case
        // instead of manufacturing proof that the quad is offscreen.
        let huge = 16_777_216.0_f32;
        assert!(conservative_maximum(-huge, huge, 0.0) >= -1.0);

        let project_shader = include_str!("../shaders/projected_quads_project.wgsl");
        assert!(project_shader.contains("*unscaled* 3-sigma quad"));
        assert!(!project_shader.contains("fn alpha_extent_scale"));
        assert!(project_shader.contains("|| !is_finite(alpha)"));
    }

    #[test]
    fn all_exact_quad_shaders_share_the_guarded_support_contract() {
        for source in [
            include_str!("../shaders/projected_quads_draw.wgsl"),
            include_str!("../shaders/projected_quads_draw_compacted.wgsl"),
            include_str!("../shaders/splat_surface_resident.wgsl"),
            include_str!("../shaders/splat_surface_direct.wgsl"),
        ] {
            assert!(source.contains("max(alpha, 1e-12) * 256.0"));
            assert!(source.contains("alpha < (1.0 / 255.0)"));
        }
        for source in [
            include_str!("../shaders/projected_quads_draw.wgsl"),
            include_str!("../shaders/projected_quads_draw_compacted.wgsl"),
        ] {
            assert!(source.contains("let offset = axes.xy * local.x + axes.zw * local.y;"));
            assert!(source.contains("center_source.xy + offset"));
        }
        assert_eq!(RESIDENT_QUAD_VERTEX_COUNT, 4);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn candidate_only_construction_keeps_exact_counting_without_compact_resources() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let (scene, expected_by_source) = contributor_scene(259);
        let draw_layout = crate::resident_gpu::create_resident_draw_bind_group_layout(&device);
        let color_layout = crate::resident_gpu::create_resident_color_bind_group_layout(&device);
        let resident = ResidentGpuResources::new(&device, &draw_layout, &color_layout, &scene)
            .expect("resident resources");
        let projected = ProjectedQuadsGpu::new(&device, wgpu::TextureFormat::Rgba8Unorm, &resident)
            .expect("downlevel projected quads");

        assert!(!projected.contributor_indirect_available());
        assert!(projected.contributor_indirect_args().is_none());
        assert!(projected.contributor_draw_pipeline().is_none());
        assert!(projected.contributor_draw_bind_group().is_none());

        let order = (0..resident.capacity as u32).rev().collect::<Vec<_>>();
        let expected_contributors = order
            .iter()
            .filter(|&&source_id| expected_by_source[source_id as usize])
            .count() as u32;
        let visible = resident
            .prepare_cpu_order(
                &queue,
                &order,
                &gsplat_core::Camera::default(),
                64,
                64,
                true,
            )
            .expect("downlevel CPU candidate order");
        assert_eq!(visible, order.len() as u32);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("projected-downlevel-count-readback-encoder"),
        });
        assert!(
            !projected
                .encode_cpu_projection_compacted(&queue, &mut encoder, visible)
                .expect("downlevel projection")
        );
        let candidate_readback = copy_buffer(
            &device,
            &mut encoder,
            projected.cpu_candidate_args(),
            size_of::<DrawIndirectArgs>() as u64,
            "projected-downlevel-candidate-readback",
        );
        let (contributor_buffer, contributor_offset) =
            projected.contributor_count_buffer_and_offset();
        let contributor_readback = copy_buffer_range(
            &device,
            &mut encoder,
            contributor_buffer,
            contributor_offset,
            size_of::<u32>() as u64,
            "projected-downlevel-contributor-readback",
        );
        queue.submit(Some(encoder.finish()));

        assert_eq!(
            read_u32s(&device, &candidate_readback),
            [RESIDENT_QUAD_VERTEX_COUNT, visible, 0, 0],
        );
        assert_eq!(
            read_u32s(&device, &contributor_readback),
            [expected_contributors],
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn candidate_and_compact_report_exact_v_c_d_invariants() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let (scene, expected_by_source) = contributor_scene(259);
        let draw_layout = crate::resident_gpu::create_resident_draw_bind_group_layout(&device);
        let color_layout = crate::resident_gpu::create_resident_color_bind_group_layout(&device);
        let resident = ResidentGpuResources::new(&device, &draw_layout, &color_layout, &scene)
            .expect("resident resources");
        let projected = ProjectedQuadsGpu::new_with_indirect_execution(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            &resident,
            true,
        )
        .expect("dual-mode projected quads");
        let order = (0..resident.capacity as u32).rev().collect::<Vec<_>>();
        let visible = resident
            .prepare_cpu_order(
                &queue,
                &order,
                &gsplat_core::Camera::default(),
                64,
                64,
                true,
            )
            .expect("CPU order");
        let expected_contributors = order
            .iter()
            .filter(|&&source_id| expected_by_source[source_id as usize])
            .count() as u32;

        let mut candidate_encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("projected-candidate-count-invariants"),
            });
        assert_eq!(
            projected
                .encode_cpu_projection_for_draw(
                    &queue,
                    &mut candidate_encoder,
                    visible,
                    ProjectedDrawExecution::Candidate,
                )
                .expect("Candidate projection"),
            ProjectedDrawExecution::Candidate,
        );
        let candidate_v = copy_buffer_range(
            &device,
            &mut candidate_encoder,
            projected.cpu_candidate_args(),
            size_of::<u32>() as u64,
            size_of::<u32>() as u64,
            "projected-candidate-v-readback",
        );
        let (candidate_c_buffer, candidate_c_offset) =
            projected.contributor_count_buffer_and_offset();
        let candidate_c = copy_buffer_range(
            &device,
            &mut candidate_encoder,
            candidate_c_buffer,
            candidate_c_offset,
            size_of::<u32>() as u64,
            "projected-candidate-c-readback",
        );
        queue.submit(Some(candidate_encoder.finish()));
        assert_eq!(read_u32s(&device, &candidate_v), [visible]);
        assert_eq!(read_u32s(&device, &candidate_c), [expected_contributors]);
        // Candidate issues the authoritative V instances, even though C is
        // known exactly and may be smaller.
        assert!(expected_contributors < visible);

        let mut compact_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("projected-compact-count-invariants"),
        });
        assert_eq!(
            projected
                .encode_cpu_projection_for_draw(
                    &queue,
                    &mut compact_encoder,
                    visible,
                    ProjectedDrawExecution::Compact,
                )
                .expect("Compact projection"),
            ProjectedDrawExecution::Compact,
        );
        let (compact_c_buffer, compact_c_offset) = projected.contributor_count_buffer_and_offset();
        let compact_c = copy_buffer_range(
            &device,
            &mut compact_encoder,
            compact_c_buffer,
            compact_c_offset,
            size_of::<u32>() as u64,
            "projected-compact-c-readback",
        );
        let compact_d = copy_buffer_range(
            &device,
            &mut compact_encoder,
            projected
                .contributor_indirect_args()
                .expect("Compact draw args"),
            size_of::<u32>() as u64,
            size_of::<u32>() as u64,
            "projected-compact-d-readback",
        );
        queue.submit(Some(compact_encoder.finish()));
        assert_eq!(read_u32s(&device, &compact_c), [expected_contributors]);
        assert_eq!(read_u32s(&device, &compact_d), [expected_contributors]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn stable_contributor_ranks_cross_group_boundaries_and_ignore_stale_tails() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let (scene, expected_by_source) = contributor_scene(259);
        let draw_layout = crate::resident_gpu::create_resident_draw_bind_group_layout(&device);
        let color_layout = crate::resident_gpu::create_resident_color_bind_group_layout(&device);
        let resident = ResidentGpuResources::new(&device, &draw_layout, &color_layout, &scene)
            .expect("resident resources");
        let projected = ProjectedQuadsGpu::new_with_indirect_execution(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            &resident,
            true,
        )
        .expect("compacted projected quads");
        assert!(projected.contributor_indirect_available());

        let full_order = (0..resident.capacity as u32).rev().collect::<Vec<_>>();
        let expected_full = full_order
            .iter()
            .enumerate()
            .filter_map(|(rank, &source_id)| {
                expected_by_source[source_id as usize].then_some(rank as u32)
            })
            .collect::<Vec<_>>();
        let (candidate_args, contributor_args, ranks) =
            compact_cpu_frame(&device, &queue, &resident, &projected, &full_order);
        assert_eq!(candidate_args, [4, full_order.len() as u32, 0, 0]);
        assert_eq!(contributor_args, [4, expected_full.len() as u32, 0, 0]);
        assert_eq!(&ranks[..expected_full.len()], expected_full);
        assert!(
            expected_full.windows(2).all(|pair| pair[0] < pair[1]),
            "stable compaction must preserve candidate rank order",
        );
        assert!(expected_full.iter().any(|&rank| rank < 128));
        assert!(expected_full.iter().any(|&rank| rank >= 128));
        assert!(expected_full.iter().any(|&rank| rank >= 256));

        let (candidate_args, contributor_args, _) =
            compact_cpu_frame(&device, &queue, &resident, &projected, &[]);
        assert_eq!(candidate_args, [4, 0, 0, 0]);
        assert_eq!(contributor_args, [4, 0, 0, 0]);

        let short_order = &full_order[..73];
        let expected_short = short_order
            .iter()
            .enumerate()
            .filter_map(|(rank, &source_id)| {
                expected_by_source[source_id as usize].then_some(rank as u32)
            })
            .collect::<Vec<_>>();
        let (candidate_args, contributor_args, ranks) =
            compact_cpu_frame(&device, &queue, &resident, &projected, short_order);
        assert_eq!(candidate_args, [4, short_order.len() as u32, 0, 0]);
        assert_eq!(contributor_args, [4, expected_short.len() as u32, 0, 0]);
        assert_eq!(&ranks[..expected_short.len()], expected_short);

        let (_, contributor_args, ranks) =
            compact_cpu_frame(&device, &queue, &resident, &projected, &full_order);
        assert_eq!(contributor_args, [4, expected_full.len() as u32, 0, 0]);
        assert_eq!(&ranks[..expected_full.len()], expected_full);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn projected_quads_match_resident_global_quads_byte_for_byte() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let scene = comparison_scene();
        let draw_layout = crate::resident_gpu::create_resident_draw_bind_group_layout(&device);
        let color_layout = crate::resident_gpu::create_resident_color_bind_group_layout(&device);
        let global_pipeline = crate::resident_gpu::create_resident_draw_pipeline(
            &device,
            &draw_layout,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let color_pipeline =
            crate::resident_gpu::create_resident_color_pipeline(&device, &color_layout);
        let mut resident = ResidentGpuResources::new(&device, &draw_layout, &color_layout, &scene)
            .expect("resident resources");
        let projected = ProjectedQuadsGpu::new(&device, wgpu::TextureFormat::Rgba8Unorm, &resident)
            .expect("projected quads");
        assert_eq!(
            (
                projected.projected_center_source.size(),
                projected.projected_axes.size(),
            ),
            (3 * 16, 3 * 16),
        );

        let camera = gsplat_core::Camera::default();
        let order = [2_u32, 1, 0];
        let visible = resident
            .prepare_cpu_order(&queue, &order, &camera, 64, 64, true)
            .expect("CPU order");
        let (global_texture, global_view) = target(&device, "global-quad-target");
        let (projected_texture, projected_view) = target(&device, "projected-quad-target");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("projected-quad-image-gate-encoder"),
        });
        resident
            .encode_color_resolve_if_needed(
                &queue,
                &color_pipeline,
                &mut encoder,
                &camera,
                device.limits().max_compute_workgroups_per_dimension,
            )
            .expect("color resolve");
        encode_splat_draw_into(
            &mut encoder,
            &SplatDraw {
                pass_label: "global-quad-comparison-pass",
                view: &global_view,
                pipeline: &global_pipeline,
                bind_group: &resident.draw_bind_group,
                clear: wgpu::Color::TRANSPARENT,
                vertex_count: RESIDENT_QUAD_VERTEX_COUNT,
                instance_count: visible,
            },
        );
        projected
            .encode_cpu_projection(&queue, &mut encoder, visible)
            .expect("projection");
        encode_splat_draw_into(
            &mut encoder,
            &SplatDraw {
                pass_label: "projected-quad-comparison-pass",
                view: &projected_view,
                pipeline: projected.draw_pipeline(),
                bind_group: projected.draw_bind_group(),
                clear: wgpu::Color::TRANSPARENT,
                vertex_count: RESIDENT_QUAD_VERTEX_COUNT,
                instance_count: visible,
            },
        );
        let global_readback = copy_target(
            &device,
            &mut encoder,
            &global_texture,
            "global-quad-readback",
        );
        let projected_readback = copy_target(
            &device,
            &mut encoder,
            &projected_texture,
            "projected-quad-readback",
        );
        queue.submit(Some(encoder.finish()));

        assert_eq!(
            read_buffer(&device, &projected_readback),
            read_buffer(&device, &global_readback),
            "moving projection to compute must preserve every RGBA8 pixel",
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn compacted_and_direct_projected_draws_match_byte_for_byte() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let (scene, _) = contributor_scene(15);
        let draw_layout = crate::resident_gpu::create_resident_draw_bind_group_layout(&device);
        let color_layout = crate::resident_gpu::create_resident_color_bind_group_layout(&device);
        let color_pipeline =
            crate::resident_gpu::create_resident_color_pipeline(&device, &color_layout);
        let mut resident = ResidentGpuResources::new(&device, &draw_layout, &color_layout, &scene)
            .expect("resident resources");
        let projected = ProjectedQuadsGpu::new_with_indirect_execution(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            &resident,
            true,
        )
        .expect("compacted projected quads");
        let camera = gsplat_core::Camera::default();
        let order = (0..resident.capacity as u32).rev().collect::<Vec<_>>();
        let visible = resident
            .prepare_cpu_order(&queue, &order, &camera, 64, 64, true)
            .expect("CPU candidate order");
        let (direct_texture, direct_view) = target(&device, "direct-projected-target");
        let (compact_texture, compact_view) = target(&device, "compact-projected-target");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("projected-contributor-image-gate-encoder"),
        });
        resident
            .encode_color_resolve_if_needed(
                &queue,
                &color_pipeline,
                &mut encoder,
                &camera,
                device.limits().max_compute_workgroups_per_dimension,
            )
            .expect("color resolve");
        assert_eq!(
            projected
                .encode_cpu_projection_for_draw(
                    &queue,
                    &mut encoder,
                    visible,
                    ProjectedDrawExecution::Candidate,
                )
                .expect("Candidate projection"),
            ProjectedDrawExecution::Candidate,
        );
        encode_splat_draw_into(
            &mut encoder,
            &SplatDraw {
                pass_label: "direct-projected-comparison-pass",
                view: &direct_view,
                pipeline: projected.draw_pipeline(),
                bind_group: projected.draw_bind_group(),
                clear: wgpu::Color::TRANSPARENT,
                vertex_count: RESIDENT_QUAD_VERTEX_COUNT,
                instance_count: visible,
            },
        );
        assert_eq!(
            projected
                .encode_cpu_projection_for_draw(
                    &queue,
                    &mut encoder,
                    visible,
                    ProjectedDrawExecution::Compact,
                )
                .expect("Compact projection"),
            ProjectedDrawExecution::Compact,
        );
        encode_splat_indirect_draw_into(
            &mut encoder,
            &SplatIndirectDraw {
                pass_label: "compact-projected-comparison-pass",
                view: &compact_view,
                pipeline: projected
                    .contributor_draw_pipeline()
                    .expect("contributor draw pipeline"),
                bind_group: projected
                    .contributor_draw_bind_group()
                    .expect("contributor draw bind group"),
                clear: wgpu::Color::TRANSPARENT,
                indirect_args: projected
                    .contributor_indirect_args()
                    .expect("contributor indirect args"),
            },
        );
        let direct_readback = copy_target(
            &device,
            &mut encoder,
            &direct_texture,
            "direct-projected-readback",
        );
        let compact_readback = copy_target(
            &device,
            &mut encoder,
            &compact_texture,
            "compact-projected-readback",
        );
        let args_readback = copy_buffer(
            &device,
            &mut encoder,
            projected
                .contributor_indirect_args()
                .expect("contributor indirect args"),
            size_of::<DrawIndirectArgs>() as u64,
            "compact-projected-args-readback",
        );
        queue.submit(Some(encoder.finish()));

        let direct_bytes = read_buffer(&device, &direct_readback);
        let compact_bytes = read_buffer(&device, &compact_readback);
        assert!(
            direct_bytes.chunks_exact(4).any(|pixel| pixel[3] != 0),
            "fixture must produce contributing fragments",
        );
        assert_eq!(
            compact_bytes, direct_bytes,
            "removing only zero-contribution ranks must preserve every RGBA8 byte",
        );
        let args = read_u32s(&device, &args_readback);
        assert!(args[1] < visible, "fixture must exercise actual compaction");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn huge_cancellation_and_clip_boundary_match_global_direct_and_compacted_images() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let scene = adversarial_clip_scene();
        assert!(scene.positions[2].x.abs() > 100_000.0);
        let draw_layout = crate::resident_gpu::create_resident_draw_bind_group_layout(&device);
        let color_layout = crate::resident_gpu::create_resident_color_bind_group_layout(&device);
        let global_pipeline = crate::resident_gpu::create_resident_draw_pipeline(
            &device,
            &draw_layout,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let color_pipeline =
            crate::resident_gpu::create_resident_color_pipeline(&device, &color_layout);
        let mut resident = ResidentGpuResources::new(&device, &draw_layout, &color_layout, &scene)
            .expect("resident resources");
        let projected = ProjectedQuadsGpu::new_with_indirect_execution(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            &resident,
            true,
        )
        .expect("compacted projected quads");
        let camera = gsplat_core::Camera::default();
        let order = (0..resident.capacity as u32).rev().collect::<Vec<_>>();
        let visible = resident
            .prepare_cpu_order(&queue, &order, &camera, 64, 64, true)
            .expect("CPU candidate order");
        let (global_texture, global_view) = target(&device, "adversarial-global-target");
        let (direct_texture, direct_view) = target(&device, "adversarial-direct-target");
        let (compact_texture, compact_view) = target(&device, "adversarial-compact-target");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("projected-adversarial-image-gate-encoder"),
        });
        resident
            .encode_color_resolve_if_needed(
                &queue,
                &color_pipeline,
                &mut encoder,
                &camera,
                device.limits().max_compute_workgroups_per_dimension,
            )
            .expect("color resolve");
        assert!(
            projected
                .encode_cpu_projection_compacted(&queue, &mut encoder, visible)
                .expect("project and compact")
        );
        encode_splat_draw_into(
            &mut encoder,
            &SplatDraw {
                pass_label: "adversarial-global-pass",
                view: &global_view,
                pipeline: &global_pipeline,
                bind_group: &resident.draw_bind_group,
                clear: wgpu::Color::TRANSPARENT,
                vertex_count: RESIDENT_QUAD_VERTEX_COUNT,
                instance_count: visible,
            },
        );
        encode_splat_draw_into(
            &mut encoder,
            &SplatDraw {
                pass_label: "adversarial-direct-projected-pass",
                view: &direct_view,
                pipeline: projected.draw_pipeline(),
                bind_group: projected.draw_bind_group(),
                clear: wgpu::Color::TRANSPARENT,
                vertex_count: RESIDENT_QUAD_VERTEX_COUNT,
                instance_count: visible,
            },
        );
        encode_splat_indirect_draw_into(
            &mut encoder,
            &SplatIndirectDraw {
                pass_label: "adversarial-compact-projected-pass",
                view: &compact_view,
                pipeline: projected
                    .contributor_draw_pipeline()
                    .expect("contributor draw pipeline"),
                bind_group: projected
                    .contributor_draw_bind_group()
                    .expect("contributor draw bind group"),
                clear: wgpu::Color::TRANSPARENT,
                indirect_args: projected
                    .contributor_indirect_args()
                    .expect("contributor indirect args"),
            },
        );
        let global_readback = copy_target(
            &device,
            &mut encoder,
            &global_texture,
            "adversarial-global-readback",
        );
        let direct_readback = copy_target(
            &device,
            &mut encoder,
            &direct_texture,
            "adversarial-direct-readback",
        );
        let compact_readback = copy_target(
            &device,
            &mut encoder,
            &compact_texture,
            "adversarial-compact-readback",
        );
        let args_readback = copy_buffer(
            &device,
            &mut encoder,
            projected
                .contributor_indirect_args()
                .expect("contributor indirect args"),
            size_of::<DrawIndirectArgs>() as u64,
            "adversarial-contributor-args-readback",
        );
        queue.submit(Some(encoder.finish()));

        let global = read_buffer(&device, &global_readback);
        let direct = read_buffer(&device, &direct_readback);
        let compact = read_buffer(&device, &compact_readback);
        assert!(
            global.chunks_exact(4).any(|pixel| pixel[3] != 0),
            "adversarial fixture must render visible fragments",
        );
        assert_eq!(direct, global, "preprojection must preserve global pixels");
        assert_eq!(compact, direct, "compaction must preserve projected pixels");

        let args = read_u32s(&device, &args_readback);
        assert!(args[1] < visible, "fixture must also exercise compaction");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn gpu_order_candidate_args_stay_independent_from_compacted_draw() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let (scene, _) = contributor_scene(15);
        let draw_layout = crate::resident_gpu::create_resident_draw_bind_group_layout(&device);
        let color_layout = crate::resident_gpu::create_resident_color_bind_group_layout(&device);
        let global_pipeline = crate::resident_gpu::create_resident_draw_pipeline(
            &device,
            &draw_layout,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let color_pipeline =
            crate::resident_gpu::create_resident_color_pipeline(&device, &color_layout);
        let mut resident = ResidentGpuResources::new(&device, &draw_layout, &color_layout, &scene)
            .expect("resident resources");
        let mut projected = ProjectedQuadsGpu::new_with_indirect_execution(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            &resident,
            true,
        )
        .expect("projected quads");
        let camera = gsplat_core::Camera::default();
        resident
            .prepare_gpu_order_draw(
                &device,
                &draw_layout,
                &queue,
                crate::resident_gpu::ResidentGpuOrderDraw {
                    camera: &camera,
                    width: 64,
                    height: 64,
                    instance_count: 15,
                    order_stride_words: 1,
                    order_id_offset_words: 0,
                },
            )
            .expect("GPU order preparation");
        {
            let order = resident.gpu_order().expect("GPU order");
            projected
                .ensure_gpu_order_bind_group(
                    &device,
                    &resident,
                    order.sorter.final_ids(),
                    order.sorter.indirect_args(),
                )
                .expect("GPU projection binding");
            order
                .sorter
                .set_indirect_vertex_count(&queue, RESIDENT_QUAD_VERTEX_COUNT);
        }

        let (global_texture, global_view) = target(&device, "gpu-global-quad-target");
        let (projected_texture, projected_view) = target(&device, "gpu-projected-quad-target");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu-projected-quad-image-gate-encoder"),
        });
        resident
            .encode_color_resolve_if_needed(
                &queue,
                &color_pipeline,
                &mut encoder,
                &camera,
                device.limits().max_compute_workgroups_per_dimension,
            )
            .expect("color resolve");
        resident
            .gpu_order()
            .expect("GPU order")
            .sorter
            .encode(&mut encoder);
        assert!(
            projected
                .encode_gpu_projection_compacted(&mut encoder)
                .expect("GPU projection and compaction")
        );
        let order = resident.gpu_order().expect("GPU order");
        encode_splat_indirect_draw_into(
            &mut encoder,
            &SplatIndirectDraw {
                pass_label: "gpu-global-quad-comparison-pass",
                view: &global_view,
                pipeline: &global_pipeline,
                bind_group: &order.draw_bind_group,
                clear: wgpu::Color::TRANSPARENT,
                indirect_args: order.sorter.indirect_args(),
            },
        );
        encode_splat_indirect_draw_into(
            &mut encoder,
            &SplatIndirectDraw {
                pass_label: "gpu-projected-quad-comparison-pass",
                view: &projected_view,
                pipeline: projected
                    .contributor_draw_pipeline()
                    .expect("contributor draw pipeline"),
                bind_group: projected
                    .contributor_draw_bind_group()
                    .expect("contributor draw bind group"),
                clear: wgpu::Color::TRANSPARENT,
                indirect_args: projected
                    .contributor_indirect_args()
                    .expect("contributor indirect args"),
            },
        );
        let global_readback = copy_target(
            &device,
            &mut encoder,
            &global_texture,
            "gpu-global-quad-readback",
        );
        let projected_readback = copy_target(
            &device,
            &mut encoder,
            &projected_texture,
            "gpu-projected-quad-readback",
        );
        let candidate_args_readback = copy_buffer(
            &device,
            &mut encoder,
            order.sorter.indirect_args(),
            size_of::<DrawIndirectArgs>() as u64,
            "gpu-candidate-args-readback",
        );
        let contributor_args_readback = copy_buffer(
            &device,
            &mut encoder,
            projected
                .contributor_indirect_args()
                .expect("contributor indirect args"),
            size_of::<DrawIndirectArgs>() as u64,
            "gpu-contributor-args-readback",
        );
        queue.submit(Some(encoder.finish()));

        let projected_bytes = read_buffer(&device, &projected_readback);
        let global_bytes = read_buffer(&device, &global_readback);
        assert!(
            global_bytes.chunks_exact(4).any(|pixel| pixel[3] != 0),
            "fixture must exercise a non-empty indirect draw",
        );
        assert_eq!(
            projected_bytes, global_bytes,
            "GPU candidate ordering plus zero-contribution compaction must preserve RGBA8",
        );
        let candidate_args = read_u32s(&device, &candidate_args_readback);
        let contributor_args = read_u32s(&device, &contributor_args_readback);
        assert_eq!(candidate_args, [4, 15, 0, 0]);
        assert_eq!(contributor_args[0], 4);
        assert!(contributor_args[1] < candidate_args[1]);
        assert_eq!(&contributor_args[2..], [0, 0]);
    }
}
