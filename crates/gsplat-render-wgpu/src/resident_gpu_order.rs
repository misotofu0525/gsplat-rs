//! Stable GPU depth ordering for a resident scene.
//!
//! The sorter records into a caller-owned encoder and never submits, maps, or
//! polls. Visibility flags are compacted without CPU readback, eight stable
//! 4-bit LSD passes sort only the visible count, and sort/draw consume GPU
//! written indirect arguments. Each radix pass uses a two-level exclusive
//! prefix scan so the global prefix is not serialized in one workgroup.

use std::num::NonZeroU64;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::{GpuSurfaceRenderParams, ResidentSceneError, wgpu_label};

const WORKGROUP_SIZE: u32 = 64;
const ITEMS_PER_THREAD: u32 = 4;
const TILE_SIZE: u32 = WORKGROUP_SIZE * ITEMS_PER_THREAD;
const SCAN_BLOCK: u32 = TILE_SIZE;
const RADIX_PASSES: u32 = 8;

fn workgroup_count(count: u32) -> u32 {
    count.div_ceil(TILE_SIZE).max(1)
}

fn scan_block_count(group_count: u32) -> u32 {
    group_count.div_ceil(SCAN_BLOCK).max(1)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct GpuSortPair {
    pub(crate) key: u32,
    pub(crate) id: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PassParams {
    shift: u32,
    count: u32,
    group_count: u32,
    block_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CompactParams {
    count: u32,
    tile_count: u32,
    block_count: u32,
    tile_sum_offset: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct GpuOrderMeta {
    pub(crate) visible_count: u32,
    pub(crate) group_count: u32,
    pub(crate) block_count: u32,
    pub(crate) _pad0: u32,
    pub(crate) dispatch_x: u32,
    pub(crate) dispatch_y: u32,
    pub(crate) dispatch_z: u32,
    pub(crate) _pad1: u32,
    pub(crate) vertex_count: u32,
    pub(crate) instance_count: u32,
    pub(crate) first_vertex: u32,
    pub(crate) first_instance: u32,
}

pub(crate) const ORDER_META_DISPATCH_OFFSET: u64 = 16;
pub(crate) const ORDER_META_DRAW_OFFSET: u64 = 32;

fn initial_order_meta(count: u32, group_count: u32, block_count: u32) -> GpuOrderMeta {
    let dispatch_groups = if count == 0 { 0 } else { group_count };
    GpuOrderMeta {
        visible_count: count,
        group_count: dispatch_groups,
        block_count: if count == 0 { 0 } else { block_count },
        _pad0: 0,
        dispatch_x: dispatch_groups,
        dispatch_y: 1,
        dispatch_z: 1,
        _pad1: 0,
        vertex_count: 6,
        instance_count: count,
        first_vertex: 0,
        first_instance: 0,
    }
}

fn gpu_order_visibility_source() -> &'static str {
    include_str!("../shaders/gpu_order_visibility.wgsl")
}

fn radix_shader_source() -> String {
    format!(
        "{}\n{}",
        include_str!("../shaders/resident_gpu_order.wgsl"),
        gpu_order_visibility_source()
    )
}

fn quantized_keygen_shader_source() -> String {
    format!(
        "{}\n{}",
        include_str!("../shaders/resident_gpu_order_keygen_quantized.wgsl"),
        gpu_order_visibility_source()
    )
}

pub(crate) struct ResidentGpuOrder {
    count: u32,
    _group_count: u32,
    block_count: u32,
    keygen_group_count: u32,
    compact_block_count: u32,
    pairs_a: wgpu::Buffer,
    _pairs_b: wgpu::Buffer,
    order_meta: wgpu::Buffer,
    _scratch: wgpu::Buffer,
    _meta: wgpu::Buffer,
    _pass_params: wgpu::Buffer,
    _compact_params: wgpu::Buffer,
    keygen_bind_group: wgpu::BindGroup,
    compact_bind_group: wgpu::BindGroup,
    radix_a_to_b: wgpu::BindGroup,
    radix_b_to_a: wgpu::BindGroup,
    keygen_pipeline: wgpu::ComputePipeline,
    compact_histogram_pipeline: wgpu::ComputePipeline,
    compact_prefix_block_pipeline: wgpu::ComputePipeline,
    compact_prefix_top_pipeline: wgpu::ComputePipeline,
    compact_prefix_add_pipeline: wgpu::ComputePipeline,
    compact_scatter_pipeline: wgpu::ComputePipeline,
    write_indirect_pipeline: wgpu::ComputePipeline,
    histogram_pipeline: wgpu::ComputePipeline,
    prefix_block_pipeline: wgpu::ComputePipeline,
    prefix_top_pipeline: wgpu::ComputePipeline,
    prefix_add_pipeline: wgpu::ComputePipeline,
    scatter_pipeline: wgpu::ComputePipeline,
    pass_stride: u32,
}

impl ResidentGpuOrder {
    pub(crate) fn validate_dispatch_limits(
        device: &wgpu::Device,
        capacity: u32,
        count: u32,
    ) -> Result<(), ResidentSceneError> {
        let group_count = workgroup_count(capacity.max(1));
        let keygen_group_count = workgroup_count(count);
        let block_count = scan_block_count(group_count);
        let compact_block_count = scan_block_count(keygen_group_count);
        let dispatch_limit = device.limits().max_compute_workgroups_per_dimension;
        if group_count > dispatch_limit
            || keygen_group_count > dispatch_limit
            || block_count > dispatch_limit
            || compact_block_count > dispatch_limit
        {
            return Err(ResidentSceneError::GpuOrderInitialization(format!(
                "resident GPU order requires {group_count} radix, {keygen_group_count} key-generation, {block_count} radix-scan, and {compact_block_count} compact-scan workgroups; device limit is {dispatch_limit}"
            )));
        }
        Ok(())
    }

    pub(crate) fn new(
        device: &wgpu::Device,
        source_buffer: &wgpu::Buffer,
        render_params_buffer: &wgpu::Buffer,
        capacity: u32,
        count: u32,
        profile: crate::ResidentStorageProfile,
    ) -> Result<Self, ResidentSceneError> {
        debug_assert!(count <= capacity);
        let allocation_count = capacity.max(1);
        let group_count = workgroup_count(allocation_count);
        let keygen_group_count = workgroup_count(count);
        let block_count = scan_block_count(group_count);
        let compact_block_count = scan_block_count(keygen_group_count);
        Self::validate_dispatch_limits(device, capacity, count)?;
        let pair_bytes = u64::from(allocation_count) * std::mem::size_of::<GpuSortPair>() as u64;
        let pair_usage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let pairs_a = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-a"),
            size: pair_bytes,
            usage: pair_usage,
            mapped_at_creation: false,
        });
        let pairs_b = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-b"),
            size: pair_bytes,
            usage: pair_usage,
            mapped_at_creation: false,
        });

        let meta_words = 16_u64 + 16_u64 * u64::from(group_count) + 16_u64 * u64::from(block_count);
        let meta = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-meta"),
            size: meta_words * std::mem::size_of::<u32>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let scratch_words = u64::from(allocation_count)
            + u64::from(keygen_group_count)
            + u64::from(compact_block_count);
        let scratch = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-scratch"),
            size: scratch_words * std::mem::size_of::<u32>() as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let order_meta = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-indirect"),
            contents: bytemuck::bytes_of(&initial_order_meta(count, group_count, block_count)),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        });
        let compact_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-compact-params"),
            contents: bytemuck::bytes_of(&CompactParams {
                count,
                tile_count: keygen_group_count,
                block_count: compact_block_count,
                tile_sum_offset: allocation_count,
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let pass_stride = device.limits().min_uniform_buffer_offset_alignment.max(16);
        let mut params_bytes = vec![0_u8; pass_stride as usize * RADIX_PASSES as usize];
        for pass in 0..RADIX_PASSES {
            let params = PassParams {
                shift: pass * 4,
                count,
                group_count,
                block_count,
            };
            let offset = pass_stride as usize * pass as usize;
            params_bytes[offset..offset + std::mem::size_of::<PassParams>()]
                .copy_from_slice(bytemuck::bytes_of(&params));
        }
        let pass_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-pass-params"),
            contents: &params_bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let radix_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-shader"),
            source: wgpu::ShaderSource::Wgsl(radix_shader_source().into()),
        });
        let compact_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-compact-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/resident_gpu_order_compact.wgsl").into(),
            ),
        });
        let quantized_keygen = (profile == crate::ResidentStorageProfile::Quantized).then(|| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: wgpu_label("gsplat-resident-gpu-order-quantized-keygen"),
                source: wgpu::ShaderSource::Wgsl(quantized_keygen_shader_source().into()),
            })
        });
        let keygen_shader = quantized_keygen.as_ref().unwrap_or(&radix_shader);
        let keygen_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-keygen-bgl"),
            entries: &[
                storage_entry(0, true),
                uniform_entry(
                    1,
                    false,
                    NonZeroU64::new(std::mem::size_of::<GpuSurfaceRenderParams>() as u64),
                ),
                storage_entry(2, false),
                storage_entry(3, false),
            ],
        });
        let compact_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-compact-bgl"),
            entries: &[
                storage_entry(0, false),
                storage_entry(1, true),
                storage_entry(2, false),
                storage_entry(3, false),
                uniform_entry(
                    4,
                    false,
                    NonZeroU64::new(std::mem::size_of::<CompactParams>() as u64),
                ),
            ],
        });
        let radix_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-radix-bgl"),
            entries: &[
                storage_entry(4, true),
                storage_entry(5, false),
                storage_entry(6, false),
                uniform_entry(
                    7,
                    true,
                    NonZeroU64::new(std::mem::size_of::<PassParams>() as u64),
                ),
                storage_entry(8, true),
            ],
        });

        let keygen_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: wgpu_label("gsplat-resident-gpu-order-keygen-layout"),
                bind_group_layouts: &[&keygen_layout],
                immediate_size: 0,
            });
        let compact_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: wgpu_label("gsplat-resident-gpu-order-compact-layout"),
                bind_group_layouts: &[&compact_layout],
                immediate_size: 0,
            });
        let radix_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: wgpu_label("gsplat-resident-gpu-order-radix-layout"),
                bind_group_layouts: &[&radix_layout],
                immediate_size: 0,
            });
        let keygen_pipeline = compute_pipeline(
            device,
            keygen_shader,
            &keygen_pipeline_layout,
            "generate_pairs",
            "gsplat-resident-gpu-order-keygen-pipeline",
        );
        let compact_histogram_pipeline = compute_pipeline(
            device,
            &compact_shader,
            &compact_pipeline_layout,
            "compact_histogram",
            "gsplat-resident-gpu-order-compact-histogram-pipeline",
        );
        let compact_prefix_block_pipeline = compute_pipeline(
            device,
            &compact_shader,
            &compact_pipeline_layout,
            "compact_prefix_block",
            "gsplat-resident-gpu-order-compact-prefix-block-pipeline",
        );
        let compact_prefix_top_pipeline = compute_pipeline(
            device,
            &compact_shader,
            &compact_pipeline_layout,
            "compact_prefix_top",
            "gsplat-resident-gpu-order-compact-prefix-top-pipeline",
        );
        let compact_prefix_add_pipeline = compute_pipeline(
            device,
            &compact_shader,
            &compact_pipeline_layout,
            "compact_prefix_add",
            "gsplat-resident-gpu-order-compact-prefix-add-pipeline",
        );
        let compact_scatter_pipeline = compute_pipeline(
            device,
            &compact_shader,
            &compact_pipeline_layout,
            "compact_scatter",
            "gsplat-resident-gpu-order-compact-scatter-pipeline",
        );
        let write_indirect_pipeline = compute_pipeline(
            device,
            &compact_shader,
            &compact_pipeline_layout,
            "write_indirect_args",
            "gsplat-resident-gpu-order-write-indirect-pipeline",
        );
        let histogram_pipeline = compute_pipeline(
            device,
            &radix_shader,
            &radix_pipeline_layout,
            "histogram",
            "gsplat-resident-gpu-order-histogram-pipeline",
        );
        let prefix_block_pipeline = compute_pipeline(
            device,
            &radix_shader,
            &radix_pipeline_layout,
            "prefix_block",
            "gsplat-resident-gpu-order-prefix-block-pipeline",
        );
        let prefix_top_pipeline = compute_pipeline(
            device,
            &radix_shader,
            &radix_pipeline_layout,
            "prefix_top",
            "gsplat-resident-gpu-order-prefix-top-pipeline",
        );
        let prefix_add_pipeline = compute_pipeline(
            device,
            &radix_shader,
            &radix_pipeline_layout,
            "prefix_add",
            "gsplat-resident-gpu-order-prefix-add-pipeline",
        );
        let scatter_pipeline = compute_pipeline(
            device,
            &radix_shader,
            &radix_pipeline_layout,
            "scatter",
            "gsplat-resident-gpu-order-scatter-pipeline",
        );

        let keygen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-keygen-bg"),
            layout: &keygen_layout,
            entries: &[
                entire_buffer_entry(0, source_buffer),
                entire_buffer_entry(1, render_params_buffer),
                entire_buffer_entry(2, &pairs_b),
                entire_buffer_entry(3, &scratch),
            ],
        });
        let compact_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-resident-gpu-order-compact-bg"),
            layout: &compact_layout,
            entries: &[
                entire_buffer_entry(0, &scratch),
                entire_buffer_entry(1, &pairs_b),
                entire_buffer_entry(2, &pairs_a),
                entire_buffer_entry(3, &order_meta),
                entire_buffer_entry(4, &compact_params),
            ],
        });
        let radix_a_to_b = radix_bind_group(
            device,
            &radix_layout,
            "gsplat-resident-gpu-order-a-to-b-bg",
            &pairs_a,
            &pairs_b,
            &meta,
            &pass_params,
            &order_meta,
        );
        let radix_b_to_a = radix_bind_group(
            device,
            &radix_layout,
            "gsplat-resident-gpu-order-b-to-a-bg",
            &pairs_b,
            &pairs_a,
            &meta,
            &pass_params,
            &order_meta,
        );

        Ok(Self {
            count,
            _group_count: group_count,
            block_count,
            keygen_group_count,
            compact_block_count,
            pairs_a,
            _pairs_b: pairs_b,
            order_meta,
            _scratch: scratch,
            _meta: meta,
            _pass_params: pass_params,
            _compact_params: compact_params,
            keygen_bind_group,
            compact_bind_group,
            radix_a_to_b,
            radix_b_to_a,
            keygen_pipeline,
            compact_histogram_pipeline,
            compact_prefix_block_pipeline,
            compact_prefix_top_pipeline,
            compact_prefix_add_pipeline,
            compact_scatter_pipeline,
            write_indirect_pipeline,
            histogram_pipeline,
            prefix_block_pipeline,
            prefix_top_pipeline,
            prefix_add_pipeline,
            scatter_pipeline,
            pass_stride,
        })
    }

    pub(crate) fn final_pairs(&self) -> &wgpu::Buffer {
        &self.pairs_a
    }

    pub(crate) fn indirect_args(&self) -> &wgpu::Buffer {
        &self.order_meta
    }

    pub(crate) fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.count == 0 {
            return;
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: wgpu_label("gsplat-resident-gpu-order-keygen-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.keygen_pipeline);
            pass.set_bind_group(0, &self.keygen_bind_group, &[]);
            pass.dispatch_workgroups(self.keygen_group_count, 1, 1);
        }

        self.encode_compact(encoder);
        self.encode_radix(encoder);
    }

    fn encode_compact(&self, encoder: &mut wgpu::CommandEncoder) {
        self.encode_compact_stage(
            encoder,
            &self.compact_histogram_pipeline,
            self.keygen_group_count,
            "gsplat-resident-gpu-order-compact-histogram-pass",
        );
        self.encode_compact_stage(
            encoder,
            &self.compact_prefix_block_pipeline,
            self.compact_block_count,
            "gsplat-resident-gpu-order-compact-prefix-block-pass",
        );
        self.encode_compact_stage(
            encoder,
            &self.compact_prefix_top_pipeline,
            1,
            "gsplat-resident-gpu-order-compact-prefix-top-pass",
        );
        self.encode_compact_stage(
            encoder,
            &self.compact_prefix_add_pipeline,
            self.compact_block_count,
            "gsplat-resident-gpu-order-compact-prefix-add-pass",
        );
        self.encode_compact_stage(
            encoder,
            &self.compact_scatter_pipeline,
            self.keygen_group_count,
            "gsplat-resident-gpu-order-compact-scatter-pass",
        );
        self.encode_compact_stage(
            encoder,
            &self.write_indirect_pipeline,
            1,
            "gsplat-resident-gpu-order-write-indirect-pass",
        );
    }

    fn encode_compact_stage(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        workgroups: u32,
        label: &'static str,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: wgpu_label(label),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.compact_bind_group, &[]);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }

    fn encode_radix(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.count == 0 {
            return;
        }
        for radix_pass in 0..RADIX_PASSES {
            let bind_group = if radix_pass % 2 == 0 {
                &self.radix_a_to_b
            } else {
                &self.radix_b_to_a
            };
            let dynamic_offset = radix_pass * self.pass_stride;
            self.encode_radix_stage_indirect(
                encoder,
                &self.histogram_pipeline,
                bind_group,
                dynamic_offset,
                "gsplat-resident-gpu-order-histogram-pass",
            );
            self.encode_radix_stage(
                encoder,
                &self.prefix_block_pipeline,
                bind_group,
                dynamic_offset,
                self.block_count,
                "gsplat-resident-gpu-order-prefix-block-pass",
            );
            self.encode_radix_stage(
                encoder,
                &self.prefix_top_pipeline,
                bind_group,
                dynamic_offset,
                1,
                "gsplat-resident-gpu-order-prefix-top-pass",
            );
            self.encode_radix_stage(
                encoder,
                &self.prefix_add_pipeline,
                bind_group,
                dynamic_offset,
                self.block_count,
                "gsplat-resident-gpu-order-prefix-add-pass",
            );
            self.encode_radix_stage_indirect(
                encoder,
                &self.scatter_pipeline,
                bind_group,
                dynamic_offset,
                "gsplat-resident-gpu-order-scatter-pass",
            );
        }
    }

    fn encode_radix_stage(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        bind_group: &wgpu::BindGroup,
        dynamic_offset: u32,
        workgroups: u32,
        label: &'static str,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: wgpu_label(label),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[dynamic_offset]);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }

    fn encode_radix_stage_indirect(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        bind_group: &wgpu::BindGroup,
        dynamic_offset: u32,
        label: &'static str,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: wgpu_label(label),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[dynamic_offset]);
        pass.dispatch_workgroups_indirect(&self.order_meta, ORDER_META_DISPATCH_OFFSET);
    }
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

fn entire_buffer_entry<'a>(binding: u32, buffer: &'a wgpu::Buffer) -> wgpu::BindGroupEntry<'a> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

#[allow(clippy::too_many_arguments)]
fn radix_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &'static str,
    src: &wgpu::Buffer,
    dst: &wgpu::Buffer,
    meta: &wgpu::Buffer,
    params: &wgpu::Buffer,
    order_meta: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label(label),
        layout,
        entries: &[
            entire_buffer_entry(4, src),
            entire_buffer_entry(5, dst),
            entire_buffer_entry(6, meta),
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: params,
                    offset: 0,
                    size: NonZeroU64::new(std::mem::size_of::<PassParams>() as u64),
                }),
            },
            entire_buffer_entry(8, order_meta),
        ],
    })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::resident::GpuSurfaceSourceElem;

    fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        pollster::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = match instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
            {
                Ok(adapter) => adapter,
                Err(_) => return None,
            };
            let limits = wgpu::Limits::downlevel_defaults();
            adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("resident-gpu-order-test-device"),
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

    fn dummy_inputs(device: &wgpu::Device) -> (wgpu::Buffer, wgpu::Buffer) {
        let source = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-gpu-order-test-source"),
            size: 64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-test-render-params"),
            contents: bytemuck::bytes_of(&GpuSurfaceRenderParams::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        (source, params)
    }

    fn sorted_on_gpu(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pairs: &[GpuSortPair],
        capacity: u32,
    ) -> Vec<GpuSortPair> {
        let (source, params) = dummy_inputs(device);
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let order = ResidentGpuOrder::new(
            device,
            &source,
            &params,
            capacity,
            pairs.len() as u32,
            crate::ResidentStorageProfile::FullF32,
        )
        .expect("test GPU sort capacity must fit the adapter dispatch limit");
        let validation_error = pollster::block_on(error_scope.pop());
        assert!(
            validation_error.is_none(),
            "GPU radix pipeline validation failed: {validation_error:?}"
        );
        if pairs.is_empty() {
            return Vec::new();
        }
        queue.write_buffer(&order.pairs_a, 0, bytemuck::cast_slice(pairs));
        if capacity as usize > pairs.len() {
            let poison = vec![
                GpuSortPair {
                    key: u32::MAX,
                    id: 0xdead_beef,
                };
                capacity as usize - pairs.len()
            ];
            queue.write_buffer(
                &order.pairs_a,
                pairs.len() as u64 * std::mem::size_of::<GpuSortPair>() as u64,
                bytemuck::cast_slice(&poison),
            );
        }

        let output_bytes = pairs.len() as u64 * std::mem::size_of::<GpuSortPair>() as u64;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-gpu-order-test-readback"),
            size: output_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-gpu-order-test-encoder"),
        });
        order.encode_radix(&mut encoder);
        encoder.copy_buffer_to_buffer(&order.pairs_a, 0, &readback, 0, output_bytes);
        queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll test GPU");
        rx.recv()
            .expect("receive map callback")
            .expect("map result");
        let result = {
            let mapped = slice.get_mapped_range();
            bytemuck::cast_slice::<u8, GpuSortPair>(&mapped).to_vec()
        };
        readback.unmap();
        result
    }

    fn assert_case(device: &wgpu::Device, queue: &wgpu::Queue, pairs: Vec<GpuSortPair>) {
        let capacity = (pairs.len() as u32 + 17).max(1);
        assert_sorted(device, queue, pairs, capacity);
    }

    fn assert_sorted(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pairs: Vec<GpuSortPair>,
        capacity: u32,
    ) {
        let mut expected = pairs.clone();
        expected.sort_by(|left, right| right.key.cmp(&left.key));
        assert_eq!(sorted_on_gpu(device, queue, &pairs, capacity), expected);
    }

    fn readback_order_meta(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        order: &ResidentGpuOrder,
    ) -> GpuOrderMeta {
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-gpu-order-meta-readback"),
            size: std::mem::size_of::<GpuOrderMeta>() as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-gpu-order-meta-readback-encoder"),
        });
        encoder.copy_buffer_to_buffer(
            &order.order_meta,
            0,
            &readback,
            0,
            std::mem::size_of::<GpuOrderMeta>() as u64,
        );
        queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll order-meta readback");
        rx.recv()
            .expect("receive map callback")
            .expect("map result");
        let meta = {
            let mapped = slice.get_mapped_range();
            *bytemuck::from_bytes::<GpuOrderMeta>(&mapped)
        };
        readback.unmap();
        meta
    }

    fn readback_pairs(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        order: &ResidentGpuOrder,
    ) -> Vec<GpuSortPair> {
        if order.count == 0 {
            return Vec::new();
        }
        let output_bytes = u64::from(order.count) * std::mem::size_of::<GpuSortPair>() as u64;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-gpu-order-keygen-test-readback"),
            size: output_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-gpu-order-keygen-test-encoder"),
        });
        order.encode(&mut encoder);
        encoder.copy_buffer_to_buffer(order.final_pairs(), 0, &readback, 0, output_bytes);
        queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll keygen test GPU");
        rx.recv()
            .expect("receive map callback")
            .expect("map result");
        let pairs = {
            let mapped = slice.get_mapped_range();
            bytemuck::cast_slice::<u8, GpuSortPair>(&mapped).to_vec()
        };
        readback.unmap();
        pairs
    }

    #[test]
    fn stable_radix_matches_cpu_or_skips_without_adapter() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping stable GPU radix test; adapter unavailable");
            return;
        };

        assert_case(&device, &queue, Vec::new());
        assert_case(&device, &queue, vec![GpuSortPair { key: 7, id: 91 }]);
        for count in [255_usize, 256, 257, 4099] {
            let pairs = (0..count)
                .map(|index| GpuSortPair {
                    key: match index % 11 {
                        0 => 0,
                        1 => u32::MAX,
                        _ => ((index * 2_654_435_761_usize) as u32) & 0x00ff_ffff,
                    },
                    id: (count - index) as u32,
                })
                .collect();
            assert_case(&device, &queue, pairs);
        }
        assert_case(
            &device,
            &queue,
            (0..257).map(|id| GpuSortPair { key: 42, id }).collect(),
        );
        // 257 radix groups require two hierarchical scan blocks.
        let multi_block_capacity = TILE_SIZE * SCAN_BLOCK + 1;
        assert_eq!(scan_block_count(workgroup_count(multi_block_capacity)), 2);
        assert_sorted(
            &device,
            &queue,
            (0..4099)
                .map(|index| GpuSortPair {
                    key: match index % 11 {
                        0 => 0,
                        1 => u32::MAX,
                        _ => ((index * 2_654_435_761_usize) as u32) & 0x00ff_ffff,
                    },
                    id: (4099 - index) as u32,
                })
                .collect(),
            multi_block_capacity,
        );
    }

    #[test]
    fn key_generation_dispatch_covers_large_scene_ladder() {
        assert_eq!(workgroup_count(0), 1);
        assert_eq!(workgroup_count(256), 1);
        assert_eq!(workgroup_count(257), 2);
        assert_eq!(scan_block_count(1), 1);
        assert_eq!(scan_block_count(256), 1);
        assert_eq!(scan_block_count(257), 2);
        assert!(workgroup_count(6_131_954) < 65_535);
        assert!(scan_block_count(workgroup_count(6_131_954)) < 65_535);
    }

    #[test]
    fn key_generation_and_radix_cover_every_source_element() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping GPU key-generation test; adapter unavailable");
            return;
        };
        let count = 257_u32;
        let sources = (0..count)
            .map(|id| {
                let mut source = GpuSurfaceSourceElem::zeroed();
                source.position = [0.0, 0.0, 1.0 + (id % 31) as f32, 0.0];
                source
            })
            .collect::<Vec<_>>();
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-keygen-test-source"),
            contents: bytemuck::cast_slice(&sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let mut params = GpuSurfaceRenderParams::zeroed();
        params.view_rot_row2 = [0.0, 0.0, 1.0, 0.0];
        params.near_plane = 0.1;
        params.far_plane = 100.0;
        params.len = count;
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-keygen-test-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let order = ResidentGpuOrder::new(
            &device,
            &source_buffer,
            &params_buffer,
            count,
            count,
            crate::ResidentStorageProfile::FullF32,
        )
        .expect("257-source key-generation test must fit the adapter dispatch limit");
        let actual = readback_pairs(&device, &queue, &order);
        let mut expected = (0..count)
            .map(|id| GpuSortPair {
                key: (1.0 + (id % 31) as f32).to_bits(),
                id,
            })
            .collect::<Vec<_>>();
        expected.sort_by(|left, right| right.key.cmp(&left.key));
        assert_eq!(actual, expected);
    }

    #[test]
    fn quantized_key_generation_reads_packed_positions() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping quantized GPU key-generation test; adapter unavailable");
            return;
        };
        let count = 257_u32;
        let sources = (0..count)
            .map(|id| {
                let mut source = crate::quantized::GpuQuantizedSource::zeroed();
                let z = 1.0 + (id % 31) as f32;
                source.pos_xy = crate::quantized::pack2x16float(0.0, 0.0);
                source.pos_z_alpha = crate::quantized::pack2x16float(z, 0.0);
                source
            })
            .collect::<Vec<_>>();
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-quantized-keygen-source"),
            contents: bytemuck::cast_slice(&sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let mut params = GpuSurfaceRenderParams::zeroed();
        params.view_rot_row2 = [0.0, 0.0, 1.0, 0.0];
        params.near_plane = 0.1;
        params.far_plane = 100.0;
        params.len = count;
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-quantized-keygen-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let order = ResidentGpuOrder::new(
            &device,
            &source_buffer,
            &params_buffer,
            count,
            count,
            crate::ResidentStorageProfile::Quantized,
        )
        .expect("quantized key-generation test must fit the adapter dispatch limit");
        let actual = readback_pairs(&device, &queue, &order);
        let mut expected = (0..count)
            .map(|id| GpuSortPair {
                key: (1.0 + (id % 31) as f32).to_bits(),
                id,
            })
            .collect::<Vec<_>>();
        expected.sort_by(|left, right| right.key.cmp(&left.key));
        assert_eq!(actual, expected);
    }

    #[test]
    fn compact_omits_near_far_culled_sources() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping GPU compact test; adapter unavailable");
            return;
        };
        let sources = [
            (0.0_f32, -1.0_f32),
            (0.0, 2.0),
            (0.0, 1000.0),
            (0.0, 4.0),
            (0.0, 0.05),
        ]
        .into_iter()
        .map(|(x, z)| {
            let mut source = GpuSurfaceSourceElem::zeroed();
            source.position = [x, 0.0, z, 0.0];
            source
        })
        .collect::<Vec<_>>();
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-compact-source"),
            contents: bytemuck::cast_slice(&sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let mut params = GpuSurfaceRenderParams::zeroed();
        params.view_rot_row2 = [0.0, 0.0, 1.0, 0.0];
        params.near_plane = 0.1;
        params.far_plane = 100.0;
        params.len = sources.len() as u32;
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-compact-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let order = ResidentGpuOrder::new(
            &device,
            &source_buffer,
            &params_buffer,
            sources.len() as u32,
            sources.len() as u32,
            crate::ResidentStorageProfile::FullF32,
        )
        .expect("compact visibility test must fit the adapter dispatch limit");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-gpu-order-compact-encoder"),
        });
        order.encode(&mut encoder);
        queue.submit(Some(encoder.finish()));
        let meta = readback_order_meta(&device, &queue, &order);
        assert_eq!(meta.visible_count, 2);
        assert_eq!(meta.instance_count, 2);
        assert_eq!(meta.vertex_count, 6);
        assert_eq!(meta.dispatch_x, 1);
        let pairs = readback_pairs(&device, &queue, &order);
        assert_eq!(
            &pairs[..2],
            &[
                GpuSortPair {
                    key: 4.0_f32.to_bits(),
                    id: 3,
                },
                GpuSortPair {
                    key: 2.0_f32.to_bits(),
                    id: 1,
                },
            ]
        );
    }

    #[test]
    fn compact_all_culled_writes_zero_indirect_args() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping GPU compact empty-visible test; adapter unavailable");
            return;
        };
        let mut source = GpuSurfaceSourceElem::zeroed();
        source.position = [0.0, 0.0, -2.0, 0.0];
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-compact-empty-source"),
            contents: bytemuck::bytes_of(&source),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let mut params = GpuSurfaceRenderParams::zeroed();
        params.view_rot_row2 = [0.0, 0.0, 1.0, 0.0];
        params.near_plane = 0.1;
        params.far_plane = 100.0;
        params.len = 1;
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-compact-empty-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let order = ResidentGpuOrder::new(
            &device,
            &source_buffer,
            &params_buffer,
            1,
            1,
            crate::ResidentStorageProfile::FullF32,
        )
        .expect("all-culled compact test must fit the adapter dispatch limit");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-gpu-order-compact-empty-encoder"),
        });
        order.encode(&mut encoder);
        queue.submit(Some(encoder.finish()));
        let meta = readback_order_meta(&device, &queue, &order);
        assert_eq!(meta.visible_count, 0);
        assert_eq!(meta.instance_count, 0);
        assert_eq!(meta.dispatch_x, 0);
        assert_eq!(meta.group_count, 0);
    }

    #[test]
    fn compact_omits_offscreen_footprints_when_fov_is_set() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping GPU footprint compact test; adapter unavailable");
            return;
        };
        let sources = [[0.0_f32, 2.0_f32], [80.0, 2.0]]
            .into_iter()
            .map(|[x, z]| {
                let mut source = GpuSurfaceSourceElem::zeroed();
                source.position = [x, 0.0, z, 0.0];
                source
            })
            .collect::<Vec<_>>();
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-footprint-source"),
            contents: bytemuck::cast_slice(&sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let mut params = GpuSurfaceRenderParams::zeroed();
        params.view_rot_row0 = [1.0, 0.0, 0.0, 0.0];
        params.view_rot_row1 = [0.0, 1.0, 0.0, 0.0];
        params.view_rot_row2 = [0.0, 0.0, 1.0, 0.0];
        params.vertical_fov_radians = std::f32::consts::FRAC_PI_3;
        params.near_plane = 0.1;
        params.far_plane = 100.0;
        params.aspect = 1.0;
        params.width = 128;
        params.height = 128;
        params.len = sources.len() as u32;
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-footprint-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let order = ResidentGpuOrder::new(
            &device,
            &source_buffer,
            &params_buffer,
            sources.len() as u32,
            sources.len() as u32,
            crate::ResidentStorageProfile::FullF32,
        )
        .expect("footprint compact test must fit the adapter dispatch limit");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-gpu-order-footprint-encoder"),
        });
        order.encode(&mut encoder);
        queue.submit(Some(encoder.finish()));
        let meta = readback_order_meta(&device, &queue, &order);
        assert_eq!(meta.visible_count, 1);
        let pairs = readback_pairs(&device, &queue, &order);
        assert_eq!(
            pairs[0],
            GpuSortPair {
                key: 2.0_f32.to_bits(),
                id: 0,
            }
        );
    }

    #[test]
    fn quantized_compact_omits_offscreen_footprints_when_fov_is_set() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping quantized GPU footprint compact test; adapter unavailable");
            return;
        };
        let scale = crate::quantized::quantize_log_scale(-1.2);
        let sources = [[0.0_f32, 2.0_f32], [80.0, 2.0]]
            .into_iter()
            .map(|[x, z]| {
                let mut source = crate::quantized::GpuQuantizedSource::zeroed();
                source.pos_xy = crate::quantized::pack2x16float(x, 0.0);
                source.pos_z_alpha = crate::quantized::pack2x16float(z, 0.0);
                source.rotation = crate::quantized::encode_smallest_three([0.0, 0.0, 0.0, 1.0]);
                source.scale_rgb =
                    u32::from(scale) | (u32::from(scale) << 8) | (u32::from(scale) << 16);
                source
            })
            .collect::<Vec<_>>();
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-quantized-footprint-source"),
            contents: bytemuck::cast_slice(&sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let mut params = GpuSurfaceRenderParams::zeroed();
        params.view_rot_row0 = [1.0, 0.0, 0.0, 0.0];
        params.view_rot_row1 = [0.0, 1.0, 0.0, 0.0];
        params.view_rot_row2 = [0.0, 0.0, 1.0, 0.0];
        params.vertical_fov_radians = std::f32::consts::FRAC_PI_3;
        params.near_plane = 0.1;
        params.far_plane = 100.0;
        params.aspect = 1.0;
        params.width = 128;
        params.height = 128;
        params.len = sources.len() as u32;
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-quantized-footprint-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let order = ResidentGpuOrder::new(
            &device,
            &source_buffer,
            &params_buffer,
            sources.len() as u32,
            sources.len() as u32,
            crate::ResidentStorageProfile::Quantized,
        )
        .expect("quantized footprint compact test must fit the adapter dispatch limit");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-gpu-order-quantized-footprint-encoder"),
        });
        order.encode(&mut encoder);
        queue.submit(Some(encoder.finish()));
        let meta = readback_order_meta(&device, &queue, &order);
        assert_eq!(meta.visible_count, 1);
        let pairs = readback_pairs(&device, &queue, &order);
        assert_eq!(
            pairs[0],
            GpuSortPair {
                key: 2.0_f32.to_bits(),
                id: 0,
            }
        );
    }
}

fn compute_pipeline(
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
