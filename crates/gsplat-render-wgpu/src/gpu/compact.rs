//! Strategy-free stable contributor compaction mechanics.
//!
//! Callers retain admission, scan and render policy. This leaf owns only the
//! rank/indirect resources and the compact/finalize compute stages that turn
//! caller-provided contributor flags and exclusive group offsets into a stable
//! rank prefix.

use std::{mem::size_of, num::NonZeroU64};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::wgpu_label;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawIndirectArgs {
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
}

const _: [(); 16] = [(); size_of::<DrawIndirectArgs>()];

pub(crate) struct StableContributorCompactor {
    compact_pipeline: wgpu::ComputePipeline,
    finalize_pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    contributor_ranks: wgpu::Buffer,
    indirect_args: wgpu::Buffer,
}

impl StableContributorCompactor {
    pub(crate) fn new(
        device: &wgpu::Device,
        contributor_rank_bytes: u64,
        vertex_count: u32,
        projected_center_source: &wgpu::Buffer,
        contributor_group_offsets: &wgpu::Buffer,
        draw_params: &wgpu::Buffer,
    ) -> Self {
        let contributor_ranks = storage_buffer_with_usage(
            device,
            "gsplat-projected-contributor-ranks",
            contributor_rank_bytes.max(size_of::<u32>() as u64),
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let indirect_args = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-projected-contributor-draw-args"),
            contents: bytemuck::bytes_of(&DrawIndirectArgs {
                vertex_count,
                instance_count: 0,
                first_vertex: 0,
                first_instance: 0,
            }),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("gsplat-projected-contributor-compact-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../shaders/projected_quads_compact.wgsl").into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: wgpu_label("gsplat-projected-contributor-compact-pipeline-layout"),
            bind_group_layouts: &[&layout],
            immediate_size: 0,
        });
        let compact_pipeline = create_compute_pipeline(
            device,
            &shader,
            &pipeline_layout,
            "compact_contributor_ranks",
            "gsplat-projected-contributor-compact-pipeline",
        );
        let finalize_pipeline = create_compute_pipeline(
            device,
            &shader,
            &pipeline_layout,
            "finalize_contributor_args",
            "gsplat-projected-contributor-finalize-pipeline",
        );
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-projected-contributor-compact-bg"),
            layout: &layout,
            entries: &[
                entry(0, projected_center_source),
                entry(1, contributor_group_offsets),
                entry(2, &contributor_ranks),
                entry(3, &indirect_args),
                entry(4, draw_params),
            ],
        });

        Self {
            compact_pipeline,
            finalize_pipeline,
            bind_group,
            contributor_ranks,
            indirect_args,
        }
    }

    pub(crate) fn reset_instance_count(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(
            &self.indirect_args,
            size_of::<u32>() as u64,
            Some(size_of::<u32>() as u64),
        );
    }

    pub(crate) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        compact_dispatch: Option<(u32, u32)>,
    ) {
        if let Some((x, y)) = compact_dispatch {
            encode_compute_stage(
                encoder,
                &self.compact_pipeline,
                &self.bind_group,
                x,
                y,
                "gsplat-projected-contributor-compact-pass",
            );
        }
        encode_compute_stage(
            encoder,
            &self.finalize_pipeline,
            &self.bind_group,
            1,
            1,
            "gsplat-projected-contributor-finalize-pass",
        );
    }

    pub(crate) fn contributor_ranks(&self) -> &wgpu::Buffer {
        &self.contributor_ranks
    }

    pub(crate) fn indirect_args(&self) -> &wgpu::Buffer {
        &self.indirect_args
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

fn encode_compute_stage(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    x: u32,
    y: u32,
    label: &'static str,
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: wgpu_label(label),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.dispatch_workgroups(x, y, 1);
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
