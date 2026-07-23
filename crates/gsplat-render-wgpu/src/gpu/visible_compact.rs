//! Direct Resident-visible stable compaction mechanics.
//!
//! The seed owns the control and group-offset resources while the caller
//! constructs the existing radix graph. Binding then transfers those resources
//! into the compactor that encodes the unchanged reset, keygen, compact and
//! finalize stages. Admission, scan interleave and pass ordering stay with the
//! caller.

use std::{mem::size_of, num::NonZeroU64};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::{GpuSurfaceRenderParams, wgpu_label};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct ResidentOrderControl {
    pub(crate) visible_count: u32,
    pub(crate) active_group_count: u32,
    pub(crate) dispatch_x: u32,
    pub(crate) dispatch_y: u32,
    pub(crate) dispatch_z: u32,
    pub(crate) _pad0: u32,
    pub(crate) _pad1: u32,
    pub(crate) _pad2: u32,
}

const _: [(); 32] = [(); size_of::<ResidentOrderControl>()];

pub(crate) struct ResidentVisibleCompactionSeed {
    control: wgpu::Buffer,
    group_offsets: wgpu::Buffer,
    group_offset_count: u32,
}

pub(crate) struct ResidentVisibleCompactionBindings<'a> {
    source: &'a wgpu::Buffer,
    render_params: &'a wgpu::Buffer,
    spare_keys: &'a wgpu::Buffer,
    input_keys: &'a wgpu::Buffer,
    input_source_ids: &'a wgpu::Buffer,
    indirect_args: &'a wgpu::Buffer,
}

impl<'a> ResidentVisibleCompactionBindings<'a> {
    pub(crate) const fn new(
        source: &'a wgpu::Buffer,
        render_params: &'a wgpu::Buffer,
        spare_keys: &'a wgpu::Buffer,
        input_keys: &'a wgpu::Buffer,
        input_source_ids: &'a wgpu::Buffer,
        indirect_args: &'a wgpu::Buffer,
    ) -> Self {
        Self {
            source,
            render_params,
            spare_keys,
            input_keys,
            input_source_ids,
            indirect_args,
        }
    }
}

impl ResidentVisibleCompactionSeed {
    pub(crate) fn new(device: &wgpu::Device, group_count: u32) -> Self {
        let control = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-control"),
            contents: bytemuck::bytes_of(&ResidentOrderControl {
                visible_count: 0,
                active_group_count: 0,
                dispatch_x: 0,
                dispatch_y: 1,
                dispatch_z: 1,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            }),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        });
        let group_offset_count = group_count + 1;
        let group_offsets = storage_buffer(
            device,
            "gsplat-resident-visible-group-offsets",
            u64::from(group_offset_count) * size_of::<u32>() as u64,
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        );
        Self {
            control,
            group_offsets,
            group_offset_count,
        }
    }

    pub(crate) fn control(&self) -> &wgpu::Buffer {
        &self.control
    }

    pub(crate) fn group_offsets(&self) -> &wgpu::Buffer {
        &self.group_offsets
    }

    pub(crate) const fn group_offset_count(&self) -> u32 {
        self.group_offset_count
    }

    pub(crate) fn bind(
        self,
        device: &wgpu::Device,
        bindings: ResidentVisibleCompactionBindings<'_>,
        dispatch_x: u32,
        dispatch_y: u32,
    ) -> ResidentVisibleCompaction {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("gsplat-resident-visible-compaction-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../shaders/resident_gpu_order_compact.wgsl").into(),
            ),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-resident-visible-compaction-bgl"),
            entries: &[
                storage_entry(0, true),
                uniform_entry(
                    1,
                    false,
                    NonZeroU64::new(size_of::<GpuSurfaceRenderParams>() as u64),
                ),
                storage_entry(2, false),
                storage_entry(3, false),
                storage_entry(4, false),
                storage_entry(5, false),
                storage_entry(6, false),
                storage_entry(9, false),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: wgpu_label("gsplat-resident-visible-compaction-layout"),
            bind_group_layouts: &[&layout],
            immediate_size: 0,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-resident-visible-compaction-bg"),
            layout: &layout,
            entries: &[
                entry(0, bindings.source),
                entry(1, bindings.render_params),
                entry(2, bindings.spare_keys),
                entry(3, &self.group_offsets),
                entry(4, bindings.input_keys),
                entry(5, bindings.input_source_ids),
                entry(6, &self.control),
                entry(9, bindings.indirect_args),
            ],
        });
        let keygen_pipeline = create_compute_pipeline(
            device,
            &shader,
            &pipeline_layout,
            "generate_keys_and_group_counts",
            "gsplat-resident-visible-keygen-pipeline",
        );
        let compact_pipeline = create_compute_pipeline(
            device,
            &shader,
            &pipeline_layout,
            "compact_visible_keys_ids",
            "gsplat-resident-visible-compact-pipeline",
        );
        let finalize_pipeline = create_compute_pipeline(
            device,
            &shader,
            &pipeline_layout,
            "finalize_visible_compaction",
            "gsplat-resident-visible-finalize-pipeline",
        );

        ResidentVisibleCompaction {
            keygen_pipeline,
            compact_pipeline,
            finalize_pipeline,
            bind_group,
            group_offsets: self.group_offsets,
            group_offset_count: self.group_offset_count,
            dispatch_x,
            dispatch_y,
            control: self.control,
        }
    }
}

pub(crate) struct ResidentVisibleCompaction {
    keygen_pipeline: wgpu::ComputePipeline,
    compact_pipeline: wgpu::ComputePipeline,
    finalize_pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    group_offsets: wgpu::Buffer,
    group_offset_count: u32,
    dispatch_x: u32,
    dispatch_y: u32,
    control: wgpu::Buffer,
}

impl ResidentVisibleCompaction {
    pub(crate) fn reset_sentinel(&self, encoder: &mut wgpu::CommandEncoder) {
        let sentinel_offset = u64::from(self.group_offset_count - 1) * size_of::<u32>() as u64;
        encoder.clear_buffer(
            &self.group_offsets,
            sentinel_offset,
            Some(size_of::<u32>() as u64),
        );
    }

    pub(crate) fn encode_keygen(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        self.encode_stage(
            encoder,
            &self.keygen_pipeline,
            self.dispatch_x,
            self.dispatch_y,
            "gsplat-resident-visible-keygen-pass",
            timestamp_writes,
        );
    }

    pub(crate) fn encode_compact(&self, encoder: &mut wgpu::CommandEncoder) {
        self.encode_stage(
            encoder,
            &self.compact_pipeline,
            self.dispatch_x,
            self.dispatch_y,
            "gsplat-resident-visible-compact-pass",
            None,
        );
    }

    pub(crate) fn encode_finalize(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        self.encode_stage(
            encoder,
            &self.finalize_pipeline,
            1,
            1,
            "gsplat-resident-visible-finalize-pass",
            timestamp_writes,
        );
    }

    pub(crate) fn control(&self) -> &wgpu::Buffer {
        &self.control
    }

    fn encode_stage(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        dispatch_x: u32,
        dispatch_y: u32,
        label: &'static str,
        timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: wgpu_label(label),
            timestamp_writes,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.dispatch_workgroups(dispatch_x, dispatch_y, 1);
    }
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

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
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

fn uniform_entry(
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

fn entry<'a>(binding: u32, buffer: &'a wgpu::Buffer) -> wgpu::BindGroupEntry<'a> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}
