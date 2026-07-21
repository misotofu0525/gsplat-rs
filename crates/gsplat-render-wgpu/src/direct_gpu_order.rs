//! Stable GPU depth ordering for the resident Direct scene.
//!
//! The sorter records into a caller-owned encoder and never submits, maps, or
//! polls. Eight stable 4-bit LSD passes leave the final pairs in `pairs_a`.

use std::num::NonZeroU64;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::{DirectSceneError, GpuSurfaceRenderParams, wgpu_label};

const WORKGROUP_SIZE: u32 = 64;
const ITEMS_PER_THREAD: u32 = 4;
const TILE_SIZE: u32 = WORKGROUP_SIZE * ITEMS_PER_THREAD;
const RADIX_PASSES: u32 = 8;

fn workgroup_count(count: u32) -> u32 {
    count.div_ceil(TILE_SIZE).max(1)
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
    _pad: u32,
}

pub(crate) struct DirectGpuOrder {
    count: u32,
    group_count: u32,
    keygen_group_count: u32,
    pairs_a: wgpu::Buffer,
    _pairs_b: wgpu::Buffer,
    _meta: wgpu::Buffer,
    _pass_params: wgpu::Buffer,
    keygen_bind_group: wgpu::BindGroup,
    radix_a_to_b: wgpu::BindGroup,
    radix_b_to_a: wgpu::BindGroup,
    keygen_pipeline: wgpu::ComputePipeline,
    histogram_pipeline: wgpu::ComputePipeline,
    prefix_pipeline: wgpu::ComputePipeline,
    scatter_pipeline: wgpu::ComputePipeline,
    pass_stride: u32,
}

impl DirectGpuOrder {
    pub(crate) fn validate_dispatch_limits(
        device: &wgpu::Device,
        capacity: u32,
        count: u32,
    ) -> Result<(), DirectSceneError> {
        let group_count = workgroup_count(capacity.max(1));
        let keygen_group_count = workgroup_count(count);
        let dispatch_limit = device.limits().max_compute_workgroups_per_dimension;
        if group_count > dispatch_limit || keygen_group_count > dispatch_limit {
            return Err(DirectSceneError::GpuOrderInitialization(format!(
                "direct GPU order requires {group_count} radix and {keygen_group_count} key-generation workgroups; device limit is {dispatch_limit}"
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
    ) -> Result<Self, DirectSceneError> {
        debug_assert!(count <= capacity);
        let allocation_count = capacity.max(1);
        let group_count = workgroup_count(allocation_count);
        let keygen_group_count = workgroup_count(count);
        Self::validate_dispatch_limits(device, capacity, count)?;
        let pair_bytes = u64::from(allocation_count) * std::mem::size_of::<GpuSortPair>() as u64;
        let pair_usage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let pairs_a = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-a"),
            size: pair_bytes,
            usage: pair_usage,
            mapped_at_creation: false,
        });
        let pairs_b = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-b"),
            size: pair_bytes,
            usage: pair_usage,
            mapped_at_creation: false,
        });

        let meta_words = 16_u64 + 16_u64 * u64::from(group_count);
        let meta = device.create_buffer(&wgpu::BufferDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-meta"),
            size: meta_words * std::mem::size_of::<u32>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let pass_stride = device.limits().min_uniform_buffer_offset_alignment.max(16);
        let mut params_bytes = vec![0_u8; pass_stride as usize * RADIX_PASSES as usize];
        for pass in 0..RADIX_PASSES {
            let params = PassParams {
                shift: pass * 4,
                count,
                group_count,
                _pad: 0,
            };
            let offset = pass_stride as usize * pass as usize;
            params_bytes[offset..offset + std::mem::size_of::<PassParams>()]
                .copy_from_slice(bytemuck::bytes_of(&params));
        }
        let pass_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-pass-params"),
            contents: &params_bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/direct_gpu_order.wgsl").into(),
            ),
        });
        let keygen_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-keygen-bgl"),
            entries: &[
                storage_entry(0, true),
                uniform_entry(
                    1,
                    false,
                    NonZeroU64::new(std::mem::size_of::<GpuSurfaceRenderParams>() as u64),
                ),
                storage_entry(2, false),
            ],
        });
        let radix_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-radix-bgl"),
            entries: &[
                storage_entry(4, true),
                storage_entry(5, false),
                storage_entry(6, false),
                uniform_entry(
                    7,
                    true,
                    NonZeroU64::new(std::mem::size_of::<PassParams>() as u64),
                ),
            ],
        });

        let keygen_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: wgpu_label("gsplat-direct-gpu-order-keygen-layout"),
                bind_group_layouts: &[&keygen_layout],
                immediate_size: 0,
            });
        let radix_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: wgpu_label("gsplat-direct-gpu-order-radix-layout"),
                bind_group_layouts: &[&radix_layout],
                immediate_size: 0,
            });
        let keygen_pipeline = compute_pipeline(
            device,
            &shader,
            &keygen_pipeline_layout,
            "generate_pairs",
            "gsplat-direct-gpu-order-keygen-pipeline",
        );
        let histogram_pipeline = compute_pipeline(
            device,
            &shader,
            &radix_pipeline_layout,
            "histogram",
            "gsplat-direct-gpu-order-histogram-pipeline",
        );
        let prefix_pipeline = compute_pipeline(
            device,
            &shader,
            &radix_pipeline_layout,
            "prefix",
            "gsplat-direct-gpu-order-prefix-pipeline",
        );
        let scatter_pipeline = compute_pipeline(
            device,
            &shader,
            &radix_pipeline_layout,
            "scatter",
            "gsplat-direct-gpu-order-scatter-pipeline",
        );

        let keygen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-keygen-bg"),
            layout: &keygen_layout,
            entries: &[
                entire_buffer_entry(0, source_buffer),
                entire_buffer_entry(1, render_params_buffer),
                entire_buffer_entry(2, &pairs_a),
            ],
        });
        let radix_a_to_b = radix_bind_group(
            device,
            &radix_layout,
            "gsplat-direct-gpu-order-a-to-b-bg",
            &pairs_a,
            &pairs_b,
            &meta,
            &pass_params,
        );
        let radix_b_to_a = radix_bind_group(
            device,
            &radix_layout,
            "gsplat-direct-gpu-order-b-to-a-bg",
            &pairs_b,
            &pairs_a,
            &meta,
            &pass_params,
        );

        Ok(Self {
            count,
            group_count,
            keygen_group_count,
            pairs_a,
            _pairs_b: pairs_b,
            _meta: meta,
            _pass_params: pass_params,
            keygen_bind_group,
            radix_a_to_b,
            radix_b_to_a,
            keygen_pipeline,
            histogram_pipeline,
            prefix_pipeline,
            scatter_pipeline,
            pass_stride,
        })
    }

    pub(crate) fn final_pairs(&self) -> &wgpu::Buffer {
        &self.pairs_a
    }

    pub(crate) fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.count == 0 {
            return;
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: wgpu_label("gsplat-direct-gpu-order-keygen-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.keygen_pipeline);
            pass.set_bind_group(0, &self.keygen_bind_group, &[]);
            pass.dispatch_workgroups(self.keygen_group_count, 1, 1);
        }

        self.encode_radix(encoder);
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
            self.encode_radix_stage(
                encoder,
                &self.histogram_pipeline,
                bind_group,
                dynamic_offset,
                self.group_count,
                "gsplat-direct-gpu-order-histogram-pass",
            );
            self.encode_radix_stage(
                encoder,
                &self.prefix_pipeline,
                bind_group,
                dynamic_offset,
                1,
                "gsplat-direct-gpu-order-prefix-pass",
            );
            self.encode_radix_stage(
                encoder,
                &self.scatter_pipeline,
                bind_group,
                dynamic_offset,
                self.group_count,
                "gsplat-direct-gpu-order-scatter-pass",
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

fn radix_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &'static str,
    src: &wgpu::Buffer,
    dst: &wgpu::Buffer,
    meta: &wgpu::Buffer,
    params: &wgpu::Buffer,
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
        ],
    })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::GpuSurfaceSourceElem;

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
                    label: Some("direct-gpu-order-test-device"),
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
            label: Some("direct-gpu-order-test-source"),
            size: 64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("direct-gpu-order-test-render-params"),
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
        let order = DirectGpuOrder::new(device, &source, &params, capacity, pairs.len() as u32)
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
            label: Some("direct-gpu-order-test-readback"),
            size: output_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("direct-gpu-order-test-encoder"),
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
        let mut expected = pairs.clone();
        expected.sort_by(|left, right| right.key.cmp(&left.key));
        let capacity = (pairs.len() as u32 + 17).max(1);
        assert_eq!(sorted_on_gpu(device, queue, &pairs, capacity), expected);
    }

    fn readback_pairs(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        order: &DirectGpuOrder,
    ) -> Vec<GpuSortPair> {
        if order.count == 0 {
            return Vec::new();
        }
        let output_bytes = u64::from(order.count) * std::mem::size_of::<GpuSortPair>() as u64;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("direct-gpu-order-keygen-test-readback"),
            size: output_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("direct-gpu-order-keygen-test-encoder"),
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
    }

    #[test]
    fn key_generation_dispatch_covers_large_scene_ladder() {
        assert_eq!(workgroup_count(0), 1);
        assert_eq!(workgroup_count(256), 1);
        assert_eq!(workgroup_count(257), 2);
        assert!(workgroup_count(6_131_954) < 65_535);
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
            label: Some("direct-gpu-order-keygen-test-source"),
            contents: bytemuck::cast_slice(&sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let mut params = GpuSurfaceRenderParams::zeroed();
        params.view_rot_row2 = [0.0, 0.0, 1.0, 0.0];
        params.near_plane = 0.1;
        params.far_plane = 100.0;
        params.len = count;
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("direct-gpu-order-keygen-test-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let order = DirectGpuOrder::new(&device, &source_buffer, &params_buffer, count, count)
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
