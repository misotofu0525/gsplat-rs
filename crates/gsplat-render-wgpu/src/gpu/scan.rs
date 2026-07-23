//! Portable hierarchical exclusive prefix scan for GPU-owned buffers.

use std::{mem::size_of, num::NonZeroU64};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::gpu_error::ResidentGpuError;
use crate::wgpu_label;

pub(super) const SCAN_WORKGROUP_SIZE: u32 = 256;
pub(super) const SCAN_ITEMS_PER_GROUP: u32 = SCAN_WORKGROUP_SIZE * 2;
pub(super) const WORD_BYTES: u64 = size_of::<u32>() as u64;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ScanParams {
    count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

pub(super) fn scan_sum_plan(mut count: u32) -> Result<(u64, u64, u32), ResidentGpuError> {
    debug_assert!(count > 0);
    let mut total = 0_u64;
    let mut largest = 0_u64;
    let mut levels = 0_u32;
    loop {
        let groups = count.div_ceil(SCAN_ITEMS_PER_GROUP).max(1);
        let bytes = u64::from(groups)
            .checked_mul(WORD_BYTES)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        total = total
            .checked_add(bytes)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        largest = largest.max(bytes);
        levels = levels
            .checked_add(1)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        if groups == 1 {
            return Ok((total, largest, levels));
        }
        count = groups;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Dispatch2d {
    pub(super) x: u32,
    pub(super) y: u32,
}

impl Dispatch2d {
    pub(super) fn for_workgroups(workgroups: u32, limit: u32) -> Result<Self, ResidentGpuError> {
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
}

struct ScanLevel {
    bind_group: wgpu::BindGroup,
    dispatch: Dispatch2d,
    dynamic_offset: u32,
}

pub(crate) struct GpuPrefixScan {
    scan_pipeline: wgpu::ComputePipeline,
    add_offsets_pipeline: wgpu::ComputePipeline,
    levels: Vec<ScanLevel>,
    _sums: Vec<wgpu::Buffer>,
    _params: wgpu::Buffer,
}

impl GpuPrefixScan {
    pub(crate) fn new(
        device: &wgpu::Device,
        data: &wgpu::Buffer,
        count: u32,
        dispatch_limit: u32,
    ) -> Result<Self, ResidentGpuError> {
        let level_counts = scan_level_counts(count);
        let sums = level_counts
            .iter()
            .map(|&(_, groups)| {
                storage_buffer(
                    device,
                    "gsplat-external-radix-scan-sums",
                    u64::from(groups) * WORD_BYTES,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                )
            })
            .collect::<Vec<_>>();
        let stride = device.limits().min_uniform_buffer_offset_alignment.max(16);
        let mut params_bytes = vec![0_u8; stride as usize * level_counts.len()];
        for (level, &(level_count, _)) in level_counts.iter().enumerate() {
            let params = ScanParams {
                count: level_count,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            };
            let offset = level * stride as usize;
            params_bytes[offset..offset + size_of::<ScanParams>()]
                .copy_from_slice(bytemuck::bytes_of(&params));
        }
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-external-radix-scan-params"),
            contents: &params_bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-external-radix-scan-bgl"),
            entries: &[
                storage_layout(0, false),
                storage_layout(1, false),
                uniform_layout(2, true, NonZeroU64::new(size_of::<ScanParams>() as u64)),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("gsplat-external-radix-scan-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../shaders/gpu_prefix_scan.wgsl").into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: wgpu_label("gsplat-external-radix-scan-pipeline-layout"),
            bind_group_layouts: &[&layout],
            immediate_size: 0,
        });
        let scan_pipeline = create_compute_pipeline(
            device,
            &shader,
            &pipeline_layout,
            "scan_blocks",
            "gsplat-external-radix-scan-pipeline",
        );
        let add_offsets_pipeline = create_compute_pipeline(
            device,
            &shader,
            &pipeline_layout,
            "add_block_offsets",
            "gsplat-external-radix-add-offsets-pipeline",
        );
        let levels = level_counts
            .iter()
            .enumerate()
            .map(|(level, &(_, groups))| {
                let scan_data = if level == 0 { data } else { &sums[level - 1] };
                Ok(ScanLevel {
                    bind_group: create_scan_bind_group(
                        device,
                        &layout,
                        scan_data,
                        &sums[level],
                        &params,
                    ),
                    dispatch: Dispatch2d::for_workgroups(groups, dispatch_limit)?,
                    dynamic_offset: level as u32 * stride,
                })
            })
            .collect::<Result<Vec<_>, ResidentGpuError>>()?;
        Ok(Self {
            scan_pipeline,
            add_offsets_pipeline,
            levels,
            _sums: sums,
            _params: params,
        })
    }

    pub(crate) fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        for level in &self.levels {
            encode_compute(
                encoder,
                &self.scan_pipeline,
                &level.bind_group,
                &[level.dynamic_offset],
                level.dispatch,
                "gsplat-external-radix-scan-pass",
            );
        }
        for level in self.levels[..self.levels.len() - 1].iter().rev() {
            encode_compute(
                encoder,
                &self.add_offsets_pipeline,
                &level.bind_group,
                &[level.dynamic_offset],
                level.dispatch,
                "gsplat-external-radix-add-offsets-pass",
            );
        }
    }
}

fn scan_level_counts(mut count: u32) -> Vec<(u32, u32)> {
    debug_assert!(count > 0);
    let mut levels = Vec::new();
    loop {
        let groups = count.div_ceil(SCAN_ITEMS_PER_GROUP).max(1);
        levels.push((count, groups));
        if groups == 1 {
            return levels;
        }
        count = groups;
    }
}

fn create_scan_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    data: &wgpu::Buffer,
    sums: &wgpu::Buffer,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label("gsplat-external-radix-scan-bg"),
        layout,
        entries: &[
            entry(0, data),
            entry(1, sums),
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: params,
                    offset: 0,
                    size: NonZeroU64::new(size_of::<ScanParams>() as u64),
                }),
            },
        ],
    })
}

pub(super) fn storage_layout(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
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

pub(super) fn uniform_layout(
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

pub(super) fn create_compute_pipeline(
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

pub(super) fn storage_buffer(
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

pub(super) fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}
