use std::{mem::size_of, num::NonZeroU64};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::ExternalPrefixRadix;
use crate::{GpuSurfaceRenderParams, wgpu_label};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct PreprojectDrawIndirectArgs {
    pub(crate) vertex_count: u32,
    pub(crate) instance_count: u32,
    pub(crate) first_vertex: u32,
    pub(crate) first_instance: u32,
}

pub(crate) const PREPROJECT_DRAW_INDIRECT_ARGS_BYTES: u64 =
    size_of::<PreprojectDrawIndirectArgs>() as u64;

/// Private owner of Preproject's key/ID compaction and draw-count finalization.
pub(crate) struct PreprojectKeyIdCompactor {
    compact_pipeline: wgpu::ComputePipeline,
    finalize_pipeline: wgpu::ComputePipeline,
    empty_bind_group: wgpu::BindGroup,
    compact_bind_group: wgpu::BindGroup,
    draw_args: wgpu::Buffer,
}

impl PreprojectKeyIdCompactor {
    pub(crate) fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        source_center_alpha_key: &wgpu::Buffer,
        contributor_offsets: &wgpu::Buffer,
        radix: &ExternalPrefixRadix,
        draw_params: &wgpu::Buffer,
        vertex_count: u32,
    ) -> Self {
        let draw_args = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-preproject-draw-args"),
            contents: bytemuck::bytes_of(&PreprojectDrawIndirectArgs {
                vertex_count,
                instance_count: 0,
                first_vertex: 0,
                first_instance: 0,
            }),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        });
        let compact_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-preproject-compact-bgl"),
            entries: &[
                storage_layout(0, true),
                storage_layout(1, true),
                storage_layout(2, false),
                storage_layout(3, false),
                storage_layout(4, false),
                storage_layout(5, false),
                uniform_layout(
                    6,
                    NonZeroU64::new(size_of::<GpuSurfaceRenderParams>() as u64),
                ),
            ],
        });
        let empty_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-preproject-empty-bgl"),
            entries: &[],
        });
        let empty_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-preproject-empty-bg"),
            layout: &empty_layout,
            entries: &[],
        });
        let compact_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: wgpu_label("gsplat-preproject-compact-pipeline-layout"),
                bind_group_layouts: &[&empty_layout, &compact_layout],
                immediate_size: 0,
            });
        let compact_pipeline = create_compute_pipeline(
            device,
            shader,
            &compact_pipeline_layout,
            "compact_key_id",
            "gsplat-preproject-compact-pipeline",
        );
        let finalize_pipeline = create_compute_pipeline(
            device,
            shader,
            &compact_pipeline_layout,
            "finalize_compaction",
            "gsplat-preproject-finalize-pipeline",
        );
        let compact_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-preproject-compact-bg"),
            layout: &compact_layout,
            entries: &[
                entry(0, source_center_alpha_key),
                entry(1, contributor_offsets),
                entry(2, radix.input_keys()),
                entry(3, radix.input_source_ids()),
                entry(4, radix.control()),
                entry(5, &draw_args),
                entry(6, draw_params),
            ],
        });

        Self {
            compact_pipeline,
            finalize_pipeline,
            empty_bind_group,
            compact_bind_group,
            draw_args,
        }
    }

    pub(crate) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        compact_dispatch: Option<(u32, u32)>,
    ) {
        if let Some((x, y)) = compact_dispatch {
            self.encode_pass(
                encoder,
                &self.compact_pipeline,
                x,
                y,
                "gsplat-preproject-compact-pass",
            );
        }
        self.encode_pass(
            encoder,
            &self.finalize_pipeline,
            1,
            1,
            "gsplat-preproject-finalize-pass",
        );
    }

    pub(crate) fn draw_args(&self) -> &wgpu::Buffer {
        &self.draw_args
    }

    fn encode_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        dispatch_x: u32,
        dispatch_y: u32,
        label: &'static str,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: wgpu_label(label),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.empty_bind_group, &[]);
        pass.set_bind_group(1, &self.compact_bind_group, &[]);
        pass.dispatch_workgroups(dispatch_x, dispatch_y, 1);
    }
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

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}
