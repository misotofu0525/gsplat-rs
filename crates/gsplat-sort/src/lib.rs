//! Sort backend abstraction for depth ordering.

use std::sync::mpsc;

use bytemuck::{Pod, Zeroable};
use thiserror::Error;
use wgpu::util::DeviceExt;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum SortError {
    #[error("key/value length mismatch")]
    LengthMismatch,
    #[error("gpu backend unavailable")]
    BackendUnavailable,
    #[error("sort backend failure")]
    BackendFailure,
}

pub trait SortBackend {
    fn name(&self) -> &'static str;

    fn sort_pairs(&mut self, keys: &mut [u32], values: &mut [u32]) -> Result<(), SortError>;
}

#[derive(Default)]
pub struct CpuSortBackend;

impl SortBackend for CpuSortBackend {
    fn name(&self) -> &'static str {
        "cpu-fallback"
    }

    fn sort_pairs(&mut self, keys: &mut [u32], values: &mut [u32]) -> Result<(), SortError> {
        if keys.len() != values.len() {
            return Err(SortError::LengthMismatch);
        }

        let mut zipped: Vec<(u32, u32)> =
            keys.iter().copied().zip(values.iter().copied()).collect();
        // Stable deterministic ordering: deeper key first; tie-breaker keeps smaller original index first.
        zipped.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        for (i, (k, v)) in zipped.into_iter().enumerate() {
            keys[i] = k;
            values[i] = v;
        }

        Ok(())
    }
}

pub struct GpuRadixSortBackend {
    runtime: Option<GpuSortRuntime>,
    init_attempted: bool,
}

impl Default for GpuRadixSortBackend {
    fn default() -> Self {
        Self {
            runtime: None,
            init_attempted: false,
        }
    }
}

impl SortBackend for GpuRadixSortBackend {
    fn name(&self) -> &'static str {
        "gpu-radix-odd-even"
    }

    fn sort_pairs(&mut self, keys: &mut [u32], values: &mut [u32]) -> Result<(), SortError> {
        if keys.len() != values.len() {
            return Err(SortError::LengthMismatch);
        }

        if keys.len() <= 1 {
            return Ok(());
        }

        if !self.init_attempted {
            self.runtime = GpuSortRuntime::create().ok();
            self.init_attempted = true;
        }

        let runtime = self.runtime.as_mut().ok_or(SortError::BackendUnavailable)?;
        runtime.sort_pairs(keys, values)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SortPair {
    key: u32,
    value: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SortParams {
    pass_index: u32,
    len: u32,
    _pad0: u32,
    _pad1: u32,
}

struct GpuSortRuntime {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl GpuSortRuntime {
    fn create() -> Result<Self, SortError> {
        pollster::block_on(Self::create_async())
    }

    async fn create_async() -> Result<Self, SortError> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| SortError::BackendUnavailable)?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("gsplat-sort-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|_| SortError::BackendUnavailable)?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("odd-even-sort-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/odd_even_sort.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("odd-even-sort-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("odd-even-sort-pl"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("odd-even-sort-pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            cache: None,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        });

        Ok(Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
        })
    }

    fn sort_pairs(&mut self, keys: &mut [u32], values: &mut [u32]) -> Result<(), SortError> {
        if keys.len() != values.len() {
            return Err(SortError::LengthMismatch);
        }

        let len = keys.len();
        if len <= 1 {
            return Ok(());
        }

        let pairs: Vec<SortPair> = keys
            .iter()
            .copied()
            .zip(values.iter().copied())
            .map(|(key, value)| SortPair { key, value })
            .collect();

        let storage_size = (pairs.len() * std::mem::size_of::<SortPair>()) as u64;
        let storage_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("odd-even-sort-storage"),
                contents: bytemuck::cast_slice(&pairs),
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            });

        let initial_params = SortParams {
            pass_index: 0,
            len: len as u32,
            _pad0: 0,
            _pad1: 0,
        };

        let params_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("odd-even-sort-params"),
                contents: bytemuck::bytes_of(&initial_params),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("odd-even-sort-bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: storage_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });

        let pair_count = (len as u32) / 2 + ((len as u32) % 2);
        let workgroups = pair_count.div_ceil(64).max(1);

        for pass_index in 0..(len as u32) {
            let params = SortParams {
                pass_index,
                len: len as u32,
                _pad0: 0,
                _pad1: 0,
            };
            self.queue
                .write_buffer(&params_buffer, 0, bytemuck::bytes_of(&params));

            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("odd-even-sort-encoder"),
                });

            {
                let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("odd-even-sort-pass"),
                    timestamp_writes: None,
                });
                cpass.set_pipeline(&self.pipeline);
                cpass.set_bind_group(0, &bind_group, &[]);
                cpass.dispatch_workgroups(workgroups, 1, 1);
            }

            self.queue.submit(Some(encoder.finish()));
        }

        let readback_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("odd-even-sort-readback"),
            size: storage_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("odd-even-sort-copy-encoder"),
                });
            encoder.copy_buffer_to_buffer(&storage_buffer, 0, &readback_buffer, 0, storage_size);
            self.queue.submit(Some(encoder.finish()));
        }

        let slice = readback_buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());

        match rx.recv() {
            Ok(Ok(())) => {}
            _ => return Err(SortError::BackendFailure),
        }

        {
            let mapped = slice.get_mapped_range();
            let sorted_pairs: &[SortPair] = bytemuck::cast_slice(&mapped);
            if sorted_pairs.len() != len {
                return Err(SortError::BackendFailure);
            }

            for (idx, pair) in sorted_pairs.iter().enumerate() {
                keys[idx] = pair.key;
                values[idx] = pair.value;
            }
        }

        readback_buffer.unmap();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CpuSortBackend, GpuRadixSortBackend, SortBackend, SortError};

    #[test]
    fn cpu_backend_sorts_descending_by_key() {
        let mut backend = CpuSortBackend;
        let mut keys = [20_u32, 5, 12];
        let mut values = [0_u32, 1, 2];

        backend.sort_pairs(&mut keys, &mut values).unwrap();

        assert_eq!(keys, [20, 12, 5]);
        assert_eq!(values, [0, 2, 1]);
    }

    #[test]
    fn cpu_backend_rejects_mismatch() {
        let mut backend = CpuSortBackend;
        let mut keys = [1_u32, 2];
        let mut values = [1_u32];

        let err = backend.sort_pairs(&mut keys, &mut values).unwrap_err();
        assert_eq!(err, SortError::LengthMismatch);
    }

    #[test]
    fn gpu_backend_sorts_or_reports_unavailable() {
        let mut backend = GpuRadixSortBackend::default();
        let mut keys = [10_u32, 2, 7, 7, 100, 1];
        let mut values = [0_u32, 1, 2, 3, 4, 5];

        match backend.sort_pairs(&mut keys, &mut values) {
            Ok(()) => {
                assert_eq!(keys, [100, 10, 7, 7, 2, 1]);
                assert_eq!(values, [4, 0, 2, 3, 1, 5]);
            }
            Err(SortError::BackendUnavailable) => {
                // Acceptable in headless environments with no GPU adapter.
            }
            Err(err) => panic!("unexpected sort error: {err}"),
        }
    }
}
