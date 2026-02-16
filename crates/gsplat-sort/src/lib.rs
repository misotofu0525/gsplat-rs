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
pub struct CpuSortBackend {
    packed: Vec<u64>,
}

impl SortBackend for CpuSortBackend {
    fn name(&self) -> &'static str {
        "cpu-fallback"
    }

    fn sort_pairs(&mut self, keys: &mut [u32], values: &mut [u32]) -> Result<(), SortError> {
        if keys.len() != values.len() {
            return Err(SortError::LengthMismatch);
        }

        let len = keys.len();
        if len <= 1 {
            return Ok(());
        }

        if self.packed.len() < len {
            self.packed.resize(len, 0);
        }
        let packed = &mut self.packed[..len];
        pack_pairs(keys, values, packed);
        packed.sort_unstable_by(|a, b| b.cmp(a));
        unpack_pairs(packed, keys, values);
        Ok(())
    }
}

#[inline]
const fn pack_sort_pair(key: u32, value: u32) -> u64 {
    ((key as u64) << 32) | ((!value) as u64)
}

#[inline]
const fn unpack_sort_pair(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, !(packed as u32))
}

fn pack_pairs(keys: &[u32], values: &[u32], out: &mut [u64]) {
    debug_assert_eq!(keys.len(), values.len());
    debug_assert_eq!(keys.len(), out.len());

    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: guarded by runtime AVX2 detection and length-validated slices.
            unsafe {
                pack_pairs_avx2(keys, values, out);
            }
            return;
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: AArch64 guarantees Neon availability.
        unsafe {
            pack_pairs_neon(keys, values, out);
        }
        return;
    }

    #[cfg(not(target_arch = "aarch64"))]
    for i in 0..keys.len() {
        out[i] = pack_sort_pair(keys[i], values[i]);
    }
}

fn unpack_pairs(packed: &[u64], keys: &mut [u32], values: &mut [u32]) {
    debug_assert_eq!(packed.len(), keys.len());
    debug_assert_eq!(packed.len(), values.len());

    for i in 0..packed.len() {
        let (key, value) = unpack_sort_pair(packed[i]);
        keys[i] = key;
        values[i] = value;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn pack_pairs_avx2(keys: &[u32], values: &[u32], out: &mut [u64]) {
    use std::arch::x86_64::*;

    let len = keys.len();
    let mut i = 0_usize;
    let all_ones_u32 = _mm_set1_epi32(-1);

    while i + 4 <= len {
        // SAFETY: i + 4 <= len guarantees valid 16-byte loads.
        let key_128 = unsafe { _mm_loadu_si128(keys.as_ptr().add(i) as *const __m128i) };
        // SAFETY: i + 4 <= len guarantees valid 16-byte loads.
        let val_128 = unsafe { _mm_loadu_si128(values.as_ptr().add(i) as *const __m128i) };
        let inv_val_128 = _mm_xor_si128(val_128, all_ones_u32);

        let key_64 = _mm256_cvtepu32_epi64(key_128);
        let inv_val_64 = _mm256_cvtepu32_epi64(inv_val_128);
        let key_hi_64 = _mm256_slli_epi64(key_64, 32);
        let packed_64 = _mm256_or_si256(key_hi_64, inv_val_64);

        // SAFETY: i + 4 <= len guarantees valid 32-byte stores.
        unsafe { _mm256_storeu_si256(out.as_mut_ptr().add(i) as *mut __m256i, packed_64) };
        i += 4;
    }

    while i < len {
        out[i] = pack_sort_pair(keys[i], values[i]);
        i += 1;
    }
}

#[cfg(target_arch = "aarch64")]
#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
unsafe fn pack_pairs_neon(keys: &[u32], values: &[u32], out: &mut [u64]) {
    use std::arch::aarch64::*;

    let len = keys.len();
    let mut i = 0_usize;

    while i + 4 <= len {
        // SAFETY: i + 4 <= len guarantees valid loads/stores.
        let key_4 = unsafe { vld1q_u32(keys.as_ptr().add(i)) };
        // SAFETY: i + 4 <= len guarantees valid loads/stores.
        let val_4 = unsafe { vld1q_u32(values.as_ptr().add(i)) };
        let inv_val_4 = vmvnq_u32(val_4);

        let key_lo = vmovl_u32(vget_low_u32(key_4));
        let key_hi = vmovl_u32(vget_high_u32(key_4));
        let val_lo = vmovl_u32(vget_low_u32(inv_val_4));
        let val_hi = vmovl_u32(vget_high_u32(inv_val_4));

        let key_hi_lo = vshlq_n_u64(key_lo, 32);
        let key_hi_hi = vshlq_n_u64(key_hi, 32);
        let packed_lo = vorrq_u64(key_hi_lo, val_lo);
        let packed_hi = vorrq_u64(key_hi_hi, val_hi);

        // SAFETY: i + 4 <= len guarantees valid stores.
        unsafe { vst1q_u64(out.as_mut_ptr().add(i), packed_lo) };
        // SAFETY: i + 4 <= len guarantees valid stores.
        unsafe { vst1q_u64(out.as_mut_ptr().add(i + 2), packed_hi) };
        i += 4;
    }

    while i < len {
        out[i] = pack_sort_pair(keys[i], values[i]);
        i += 1;
    }
}

pub struct GpuOddEvenSortBackend {
    runtime: Option<GpuSortRuntime>,
    init_attempted: bool,
}

impl Default for GpuOddEvenSortBackend {
    fn default() -> Self {
        Self {
            runtime: None,
            init_attempted: false,
        }
    }
}

impl SortBackend for GpuOddEvenSortBackend {
    fn name(&self) -> &'static str {
        "gpu-odd-even"
    }

    fn sort_pairs(&mut self, keys: &mut [u32], values: &mut [u32]) -> Result<(), SortError> {
        if keys.len() != values.len() {
            return Err(SortError::LengthMismatch);
        }

        if keys.len() <= 1 {
            return Ok(());
        }

        // This backend uses an O(n^2) odd-even swap network. It is intended as an executable GPU
        // path for small conformance datasets only; fall back to CPU for large scenes to avoid
        // catastrophic runtimes.
        const ODD_EVEN_MAX_LEN: usize = 4096;
        if keys.len() > ODD_EVEN_MAX_LEN {
            return Err(SortError::BackendUnavailable);
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
    use super::{CpuSortBackend, GpuOddEvenSortBackend, SortBackend, SortError};

    fn lcg_next(state: &mut u32) -> u32 {
        *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        *state
    }

    #[test]
    fn cpu_backend_sorts_descending_by_key() {
        let mut backend = CpuSortBackend::default();
        let mut keys = [20_u32, 5, 12];
        let mut values = [0_u32, 1, 2];

        backend.sort_pairs(&mut keys, &mut values).unwrap();

        assert_eq!(keys, [20, 12, 5]);
        assert_eq!(values, [0, 2, 1]);
    }

    #[test]
    fn cpu_backend_matches_reference_order_for_large_input() {
        let mut backend = CpuSortBackend::default();
        let len = 4099;
        let mut seed = 1_u32;

        let mut keys: Vec<u32> = (0..len).map(|_| lcg_next(&mut seed)).collect();
        let mut values: Vec<u32> = (0..len as u32).collect();

        let mut expected: Vec<(u32, u32)> = keys.iter().copied().zip(values.iter().copied()).collect();
        expected.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        backend.sort_pairs(&mut keys, &mut values).unwrap();

        let actual: Vec<(u32, u32)> = keys.into_iter().zip(values).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn packed_pairs_match_scalar_reference() {
        let len = 257;
        let mut seed = 7_u32;
        let keys: Vec<u32> = (0..len).map(|_| lcg_next(&mut seed)).collect();
        let values: Vec<u32> = (0..len).map(|_| lcg_next(&mut seed)).collect();
        let mut packed = vec![0_u64; len];

        super::pack_pairs(&keys, &values, &mut packed);
        let expected: Vec<u64> = keys
            .iter()
            .copied()
            .zip(values.iter().copied())
            .map(|(key, value)| super::pack_sort_pair(key, value))
            .collect();
        assert_eq!(packed, expected);
    }

    #[test]
    fn cpu_backend_rejects_mismatch() {
        let mut backend = CpuSortBackend::default();
        let mut keys = [1_u32, 2];
        let mut values = [1_u32];

        let err = backend.sort_pairs(&mut keys, &mut values).unwrap_err();
        assert_eq!(err, SortError::LengthMismatch);
    }

    #[test]
    fn gpu_backend_sorts_or_reports_unavailable() {
        let mut backend = GpuOddEvenSortBackend::default();
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
