//! Strategy-free rank-indexed projection mechanics.
//!
//! Callers retain execution-plan admission, scan/compaction policy, binding
//! publication, rasterization and telemetry. This leaf owns the fixed project
//! ABI, rank-indexed output resources, project pipeline/bind groups, portable
//! dispatch and the single projection compute pass.

use std::mem::size_of;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::gpu_error::ResidentGpuError;
use crate::wgpu_label;

const PROJECT_WORKGROUP_SIZE: u32 = 128;
const PROJECTED_CENTER_SOURCE_BYTES_PER_ITEM: u64 = 16;
const PROJECTED_AXES32_BYTES_PER_ITEM: u64 = 16;
const PROJECTED_AXES16_BYTES_PER_ITEM: u64 = 8;

#[derive(Clone, Copy)]
enum ProjectedAxesEncoding {
    Exact32,
    Binary16,
}

impl ProjectedAxesEncoding {
    const fn record_bytes(self) -> u64 {
        match self {
            Self::Exact32 => PROJECTED_AXES32_BYTES_PER_ITEM,
            Self::Binary16 => PROJECTED_AXES16_BYTES_PER_ITEM,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawIndirectArgs {
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
}

const _: [(); 16] = [(); size_of::<DrawIndirectArgs>()];

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

#[derive(Clone, Copy)]
pub(crate) struct ProjectedRankSourceBindings<'a> {
    pub(crate) position_alpha: &'a wgpu::Buffer,
    pub(crate) covariance0: &'a wgpu::Buffer,
    pub(crate) covariance1: &'a wgpu::Buffer,
    pub(crate) draw_params: &'a wgpu::Buffer,
}

pub(crate) struct ProjectedRankProjector {
    capacity: u32,
    project_pipeline: wgpu::ComputePipeline,
    project_layout: wgpu::BindGroupLayout,
    cpu_project_bind_group: wgpu::BindGroup,
    projected_center_source: wgpu::Buffer,
    projected_axes: wgpu::Buffer,
    projected_axes_record_bytes: u64,
    contributor_group_offsets: wgpu::Buffer,
    contributor_group_offset_count: u32,
    cpu_draw_args: wgpu::Buffer,
    dispatch_limit: u32,
}

impl ProjectedRankProjector {
    pub(crate) fn new(
        device: &wgpu::Device,
        capacity: u32,
        vertex_count: u32,
        cpu_order: &wgpu::Buffer,
        source: ProjectedRankSourceBindings<'_>,
    ) -> Result<Self, ResidentGpuError> {
        Self::new_with_axes_encoding(
            device,
            capacity,
            vertex_count,
            cpu_order,
            source,
            ProjectedAxesEncoding::Exact32,
        )
    }

    pub(crate) fn new_axes16(
        device: &wgpu::Device,
        capacity: u32,
        vertex_count: u32,
        cpu_order: &wgpu::Buffer,
        source: ProjectedRankSourceBindings<'_>,
    ) -> Result<Self, ResidentGpuError> {
        Self::new_with_axes_encoding(
            device,
            capacity,
            vertex_count,
            cpu_order,
            source,
            ProjectedAxesEncoding::Binary16,
        )
    }

    fn new_with_axes_encoding(
        device: &wgpu::Device,
        capacity: u32,
        vertex_count: u32,
        cpu_order: &wgpu::Buffer,
        source: ProjectedRankSourceBindings<'_>,
        axes_encoding: ProjectedAxesEncoding,
    ) -> Result<Self, ResidentGpuError> {
        let center_plane_bytes =
            projected_plane_bytes(capacity, PROJECTED_CENTER_SOURCE_BYTES_PER_ITEM)?;
        let axes_record_bytes = axes_encoding.record_bytes();
        let axes_plane_bytes = projected_plane_bytes(capacity, axes_record_bytes)?;
        let binding_limit = u64::from(device.limits().max_storage_buffer_binding_size)
            .min(device.limits().max_buffer_size);
        for (resource, bytes, minimum) in [
            (
                "projected center/source plane",
                center_plane_bytes,
                PROJECTED_CENTER_SOURCE_BYTES_PER_ITEM,
            ),
            ("projected axes plane", axes_plane_bytes, axes_record_bytes),
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
            center_plane_bytes.max(PROJECTED_CENTER_SOURCE_BYTES_PER_ITEM),
        );
        let projected_axes = storage_buffer(
            device,
            "gsplat-projected-quads-axes",
            axes_plane_bytes.max(axes_record_bytes),
        );
        let contributor_group_offset_count = capacity
            .div_ceil(PROJECT_WORKGROUP_SIZE)
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
                vertex_count,
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
        let project_shader_source = match axes_encoding {
            ProjectedAxesEncoding::Exact32 => {
                include_str!("../../shaders/projected_quads_project.wgsl")
            }
            ProjectedAxesEncoding::Binary16 => {
                include_str!("../../shaders/projected_quads_project_axes16.wgsl")
            }
        };
        let project_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("gsplat-projected-quads-project-shader"),
            source: wgpu::ShaderSource::Wgsl(project_shader_source.into()),
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
                order: cpu_order,
                source,
                projected_center_source: &projected_center_source,
                projected_axes: &projected_axes,
                draw_args: &cpu_draw_args,
                contributor_group_offsets: &contributor_group_offsets,
            },
        );

        Ok(Self {
            capacity,
            project_pipeline,
            project_layout,
            cpu_project_bind_group,
            projected_center_source,
            projected_axes,
            projected_axes_record_bytes: axes_record_bytes,
            contributor_group_offsets,
            contributor_group_offset_count,
            cpu_draw_args,
            dispatch_limit: device.limits().max_compute_workgroups_per_dimension,
        })
    }

    pub(crate) fn create_external_bind_group(
        &self,
        device: &wgpu::Device,
        source: ProjectedRankSourceBindings<'_>,
        order: &wgpu::Buffer,
        indirect_args: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        create_project_bind_group(
            device,
            &self.project_layout,
            "gsplat-projected-quads-gpu-project-bg",
            ProjectBindGroupResources {
                order,
                source,
                projected_center_source: &self.projected_center_source,
                projected_axes: &self.projected_axes,
                draw_args: indirect_args,
                contributor_group_offsets: &self.contributor_group_offsets,
            },
        )
    }

    pub(crate) fn write_cpu_draw_args(
        &self,
        queue: &wgpu::Queue,
        vertex_count: u32,
        visible_count: u32,
        contributor_sentinel: bool,
    ) {
        queue.write_buffer(
            &self.cpu_draw_args,
            0,
            bytemuck::bytes_of(&DrawIndirectArgs {
                vertex_count,
                instance_count: visible_count,
                first_vertex: 0,
                first_instance: u32::from(contributor_sentinel),
            }),
        );
    }

    pub(crate) fn encode_cpu(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        dispatch_items: u32,
    ) -> Result<(), ResidentGpuError> {
        self.encode(encoder, &self.cpu_project_bind_group, dispatch_items)
    }

    pub(crate) fn encode_external(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        bind_group: &wgpu::BindGroup,
        dispatch_items: u32,
    ) -> Result<(), ResidentGpuError> {
        self.encode(encoder, bind_group, dispatch_items)
    }

    fn encode(
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

    pub(crate) fn capacity(&self) -> u32 {
        self.capacity
    }

    pub(crate) fn projected_center_source(&self) -> &wgpu::Buffer {
        &self.projected_center_source
    }

    pub(crate) fn projected_axes(&self) -> &wgpu::Buffer {
        &self.projected_axes
    }

    pub(crate) const fn projected_axes_record_bytes(&self) -> u64 {
        self.projected_axes_record_bytes
    }

    pub(crate) fn contributor_group_offsets(&self) -> &wgpu::Buffer {
        &self.contributor_group_offsets
    }

    pub(crate) fn contributor_group_offset_count(&self) -> u32 {
        self.contributor_group_offset_count
    }

    pub(crate) fn cpu_draw_args(&self) -> &wgpu::Buffer {
        &self.cpu_draw_args
    }
}

fn create_project_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: wgpu_label("gsplat-projected-quads-project-bgl"),
        entries: &[
            storage_layout(0, true),
            storage_layout(1, true),
            storage_layout(2, true),
            storage_layout(3, true),
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
            storage_layout(5, false),
            storage_layout(6, false),
            storage_layout(7, false),
            storage_layout(8, false),
        ],
    })
}

fn storage_layout(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

#[derive(Clone, Copy)]
struct ProjectBindGroupResources<'a> {
    order: &'a wgpu::Buffer,
    source: ProjectedRankSourceBindings<'a>,
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
            entry(1, resources.source.position_alpha),
            entry(2, resources.source.covariance0),
            entry(3, resources.source.covariance1),
            entry(4, resources.source.draw_params),
            entry(5, resources.projected_center_source),
            entry(6, resources.projected_axes),
            entry(7, resources.draw_args),
            entry(8, resources.contributor_group_offsets),
        ],
    })
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

fn projected_plane_bytes(capacity: u32, record_bytes: u64) -> Result<u64, ResidentGpuError> {
    u64::from(capacity)
        .checked_mul(record_bytes)
        .ok_or(ResidentGpuError::AddressSpaceExceeded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_and_axes16_rank_cache_byte_contracts_are_finite() {
        let capacity = 4_099;
        assert_eq!(
            projected_plane_bytes(capacity, PROJECTED_CENTER_SOURCE_BYTES_PER_ITEM),
            Ok(16 * u64::from(capacity))
        );
        assert_eq!(
            projected_plane_bytes(capacity, ProjectedAxesEncoding::Exact32.record_bytes()),
            Ok(16 * u64::from(capacity))
        );
        assert_eq!(
            projected_plane_bytes(capacity, ProjectedAxesEncoding::Binary16.record_bytes()),
            Ok(8 * u64::from(capacity))
        );
    }
}
