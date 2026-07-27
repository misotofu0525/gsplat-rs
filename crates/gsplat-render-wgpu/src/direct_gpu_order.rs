//! Stable GPU depth ordering for the resident Direct scene.
//!
//! Sorting stays entirely on the GPU: keys and source IDs live in separate
//! buffers and a hierarchical scan supplies global offsets without
//! cross-workgroup spin loops. Direct and the four-storage-binding fallback
//! keep eight stable 4-bit LSD passes. Resident/Projected may use four stable
//! 8-bit LSD passes on validated targets while retaining all 32 key bits. The
//! final pack pass preserves the existing `GpuSortPair` renderer contract.

use std::{mem::size_of, num::NonZeroU64};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::cpu_order::DepthKeyPrecision;
use crate::gpu::{
    FULL32_DIRECT_RADIX as DIRECT_RADIX, FULL32_RESIDENT_RADIX as RESIDENT_RADIX,
    FULL32_RESIDENT_STORAGE_BINDINGS as RESIDENT_RADIX_STORAGE_BINDINGS,
    FULL32_RESIDENT_WORKGROUP_STORAGE_BYTES as RESIDENT_RADIX_WORKGROUP_STORAGE_BYTES,
    FULL32_SCAN_WORKGROUP_SIZE as SCAN_WORKGROUP_SIZE,
    FULL32_SCAN_WORKGROUP_STORAGE_BYTES as SCAN_WORKGROUP_STORAGE_BYTES, ResidentVisibleCompaction,
    ResidentVisibleCompactionBindings, ResidentVisibleCompactionSeed, StableFull32Radix,
    StableFull32RadixProfile, StableFull32RadixTimestampRange, full32_scan_level_counts,
    full32_workgroup_count,
};
use crate::{DirectSceneError, GpuSortPair, GpuSurfaceRenderParams, wgpu_label};

#[cfg(all(test, not(target_arch = "wasm32")))]
use crate::gpu::ResidentOrderControl;

#[cfg(test)]
use crate::gpu::{
    FULL32_DIRECT_PASSES as DIRECT_RADIX_PASSES, FULL32_RESIDENT_PASSES as RESIDENT_RADIX_PASSES,
    FULL32_TILE_SIZE as TILE_SIZE,
};

const RESIDENT_COMPACTION_STORAGE_BINDINGS: u32 = 7;

#[derive(Clone, Copy)]
struct DirectGpuOrderProfile {
    compatibility_output: bool,
    prefer_resident_radix8: bool,
    depth_key_precision: DepthKeyPrecision,
}

/// The fused byte-radix shader has a complete source-ID readback qualification
/// on native macOS Metal only. Adreno Vulkan produced corrupt full-count ID
/// permutations even though the same shader passes the Metal oracle, and the
/// other native/Web targets have not yet cleared that gate. Default every
/// unqualified target to the portable exact nibble-radix path. This changes
/// work only, not image quality, source membership, or key precision.
const fn prefer_resident_radix8_for_target() -> bool {
    cfg!(all(target_os = "macos", not(target_arch = "wasm32")))
}

/// Keep the first visible-only experiment on macOS Metal. Other Metal/WebGPU
/// targets retain their already-qualified order path until this architecture
/// clears correctness and performance gates here.
const fn prefer_resident_visible_compaction_for_target() -> bool {
    cfg!(target_os = "macos")
}

fn workgroup_count(count: u32) -> u32 {
    full32_workgroup_count(count)
}

fn scan_level_counts(count: u32) -> Vec<(u32, u32)> {
    full32_scan_level_counts(count)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Dispatch2d {
    x: u32,
    y: u32,
}

impl Dispatch2d {
    fn for_workgroups(workgroups: u32, limit: u32) -> Option<Self> {
        if limit == 0 {
            return None;
        }
        let workgroups = workgroups.max(1);
        let x = workgroups.min(limit);
        let y = workgroups.div_ceil(x);
        (y <= limit).then_some(Self { x, y })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
struct GpuDrawIndirectArgs {
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct GpuOrderTimestampRange<'a> {
    pub(crate) query_set: &'a wgpu::QuerySet,
    pub(crate) keygen_begin_index: u32,
    pub(crate) keygen_end_index: u32,
    pub(crate) radix_begin_index: u32,
    pub(crate) radix_end_index: u32,
}

pub(crate) struct DirectGpuOrder {
    count: u32,
    #[cfg(test)]
    depth_key_precision: DepthKeyPrecision,
    keygen_dispatch: Dispatch2d,
    radix: StableFull32Radix,
    indirect_args: wgpu::Buffer,
    keygen_bind_group: wgpu::BindGroup,
    resident_visible_compaction: Option<ResidentVisibleCompaction>,
    keygen_pipeline: wgpu::ComputePipeline,
}

impl DirectGpuOrder {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn validate_dispatch_limits(
        device: &wgpu::Device,
        capacity: u32,
        count: u32,
    ) -> Result<(), DirectSceneError> {
        Self::validate_limits(device, capacity, count, true, false)
    }

    /// Limit check for consumers that bind `final_ids` directly and therefore
    /// do not need the legacy AoS compatibility buffer.
    #[allow(dead_code)]
    pub(crate) fn validate_soa_dispatch_limits(
        device: &wgpu::Device,
        capacity: u32,
        count: u32,
    ) -> Result<(), DirectSceneError> {
        Self::validate_limits(device, capacity, count, false, false)
    }

    /// Limit check for the Resident/Projected SoA path. It uses the byte radix
    /// when bindings permit, while the Direct SoA contract remains unchanged.
    pub(crate) fn validate_resident_soa_dispatch_limits(
        device: &wgpu::Device,
        capacity: u32,
        count: u32,
    ) -> Result<(), DirectSceneError> {
        Self::validate_limits(
            device,
            capacity,
            count,
            false,
            prefer_resident_radix8_for_target(),
        )
    }

    fn validate_limits(
        device: &wgpu::Device,
        capacity: u32,
        count: u32,
        compatibility_output: bool,
        prefer_resident_radix8: bool,
    ) -> Result<(), DirectSceneError> {
        if count > capacity {
            return Err(DirectSceneError::GpuOrderInitialization(format!(
                "direct GPU order count {count} exceeds capacity {capacity}"
            )));
        }

        let limits = device.limits();
        let resident_radix8 = prefer_resident_radix8
            && !compatibility_output
            && limits.max_storage_buffers_per_shader_stage >= RESIDENT_RADIX_STORAGE_BINDINGS;
        let resident_visible_compaction = resident_radix8
            && prefer_resident_visible_compaction_for_target()
            && limits.max_storage_buffers_per_shader_stage >= RESIDENT_COMPACTION_STORAGE_BINDINGS;
        let required_workgroup_storage = if resident_radix8 {
            SCAN_WORKGROUP_STORAGE_BYTES.max(RESIDENT_RADIX_WORKGROUP_STORAGE_BYTES)
        } else {
            SCAN_WORKGROUP_STORAGE_BYTES
        };
        if limits.max_compute_invocations_per_workgroup < SCAN_WORKGROUP_SIZE
            || limits.max_compute_workgroup_size_x < SCAN_WORKGROUP_SIZE
            || limits.max_compute_workgroup_storage_size < required_workgroup_storage
        {
            return Err(DirectSceneError::GpuOrderInitialization(format!(
                "direct GPU order requires a {SCAN_WORKGROUP_SIZE}-thread compute workgroup and {required_workgroup_storage} bytes of workgroup storage"
            )));
        }
        if limits.max_storage_buffers_per_shader_stage < 4 {
            return Err(DirectSceneError::GpuOrderInitialization(format!(
                "direct GPU order requires 4 storage buffers per compute stage; device limit is {}",
                limits.max_storage_buffers_per_shader_stage
            )));
        }

        let group_count = workgroup_count(count);
        let radix = if resident_radix8 {
            RESIDENT_RADIX
        } else {
            DIRECT_RADIX
        };
        let prefix_count = group_count.checked_mul(radix).ok_or_else(|| {
            DirectSceneError::GpuOrderInitialization(
                "direct GPU order histogram size overflowed u32".into(),
            )
        })?;
        let dispatch_limit = limits.max_compute_workgroups_per_dimension;
        let mut dispatch_counts = vec![group_count];
        if resident_visible_compaction {
            let compact_scan_count = group_count.checked_add(1).ok_or_else(|| {
                DirectSceneError::GpuOrderInitialization(
                    "Resident visible compaction group count overflowed u32".into(),
                )
            })?;
            dispatch_counts.push(workgroup_count(prefix_count));
            dispatch_counts.extend(
                scan_level_counts(compact_scan_count)
                    .into_iter()
                    .map(|(_, groups)| groups),
            );
        }
        dispatch_counts.extend(
            scan_level_counts(prefix_count)
                .into_iter()
                .map(|(_, groups)| groups),
        );
        if let Some(required) = dispatch_counts
            .into_iter()
            .find(|&required| Dispatch2d::for_workgroups(required, dispatch_limit).is_none())
        {
            return Err(DirectSceneError::GpuOrderInitialization(format!(
                "direct GPU order requires {required} logical workgroups; the device's 2D dispatch limit is {dispatch_limit}x{dispatch_limit}"
            )));
        }

        let allocation_count = u64::from(capacity.max(1));
        let u32_bytes = allocation_count * size_of::<u32>() as u64;
        let pair_bytes = allocation_count * size_of::<GpuSortPair>() as u64;
        let prefix_bytes = u64::from(prefix_count) * size_of::<u32>() as u64;
        let storage_limit = limits
            .max_buffer_size
            .min(u64::from(limits.max_storage_buffer_binding_size));
        if let Some((name, required)) = [("key/id", u32_bytes), ("radix prefix", prefix_bytes)]
            .into_iter()
            .find(|(_, required)| *required > storage_limit)
        {
            return Err(DirectSceneError::GpuOrderInitialization(format!(
                "direct GPU order {name} buffer needs {required} bytes; device storage-binding limit is {storage_limit} bytes"
            )));
        }
        if compatibility_output && pair_bytes > storage_limit {
            return Err(DirectSceneError::GpuOrderInitialization(format!(
                "direct GPU order compatibility pair buffer needs {pair_bytes} bytes; device storage-binding limit is {storage_limit} bytes"
            )));
        }
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(
        device: &wgpu::Device,
        source_buffer: &wgpu::Buffer,
        render_params_buffer: &wgpu::Buffer,
        capacity: u32,
        count: u32,
    ) -> Result<Self, DirectSceneError> {
        Self::new_inner(
            device,
            source_buffer,
            render_params_buffer,
            capacity,
            count,
            DirectGpuOrderProfile {
                compatibility_output: true,
                prefer_resident_radix8: false,
                depth_key_precision: DepthKeyPrecision::ExactFull32,
            },
        )
    }

    /// Constructs the native SoA path without allocating or packing the legacy
    /// AoS pair buffer. The renderer should bind `final_ids` with stride 1.
    #[allow(dead_code)]
    pub(crate) fn new_soa(
        device: &wgpu::Device,
        source_buffer: &wgpu::Buffer,
        render_params_buffer: &wgpu::Buffer,
        capacity: u32,
        count: u32,
    ) -> Result<Self, DirectSceneError> {
        Self::new_inner(
            device,
            source_buffer,
            render_params_buffer,
            capacity,
            count,
            DirectGpuOrderProfile {
                compatibility_output: false,
                prefer_resident_radix8: false,
                depth_key_precision: DepthKeyPrecision::ExactFull32,
            },
        )
    }

    /// Constructs the full-resident SoA path. Unlike Direct, this path may use
    /// the fused stable byte radix negotiated by the Resident resource plan.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new_resident_soa(
        device: &wgpu::Device,
        source_buffer: &wgpu::Buffer,
        render_params_buffer: &wgpu::Buffer,
        capacity: u32,
        count: u32,
    ) -> Result<Self, DirectSceneError> {
        Self::new_resident_soa_with_depth_key_precision(
            device,
            source_buffer,
            render_params_buffer,
            capacity,
            count,
            DepthKeyPrecision::ExactFull32,
        )
    }

    pub(crate) fn new_resident_soa_with_depth_key_precision(
        device: &wgpu::Device,
        source_buffer: &wgpu::Buffer,
        render_params_buffer: &wgpu::Buffer,
        capacity: u32,
        count: u32,
        depth_key_precision: DepthKeyPrecision,
    ) -> Result<Self, DirectSceneError> {
        Self::new_inner(
            device,
            source_buffer,
            render_params_buffer,
            capacity,
            count,
            DirectGpuOrderProfile {
                compatibility_output: false,
                prefer_resident_radix8: prefer_resident_radix8_for_target(),
                depth_key_precision,
            },
        )
    }

    #[cfg(test)]
    fn new_with_depth_key_precision(
        device: &wgpu::Device,
        source_buffer: &wgpu::Buffer,
        render_params_buffer: &wgpu::Buffer,
        capacity: u32,
        count: u32,
        precision: DepthKeyPrecision,
    ) -> Result<Self, DirectSceneError> {
        Self::new_inner(
            device,
            source_buffer,
            render_params_buffer,
            capacity,
            count,
            DirectGpuOrderProfile {
                compatibility_output: true,
                prefer_resident_radix8: false,
                depth_key_precision: precision,
            },
        )
    }

    fn new_inner(
        device: &wgpu::Device,
        source_buffer: &wgpu::Buffer,
        render_params_buffer: &wgpu::Buffer,
        capacity: u32,
        count: u32,
        profile: DirectGpuOrderProfile,
    ) -> Result<Self, DirectSceneError> {
        let DirectGpuOrderProfile {
            compatibility_output,
            prefer_resident_radix8,
            depth_key_precision,
        } = profile;
        Self::validate_limits(
            device,
            capacity,
            count,
            compatibility_output,
            prefer_resident_radix8,
        )?;

        let resident_radix8_enabled = prefer_resident_radix8
            && !compatibility_output
            && device.limits().max_storage_buffers_per_shader_stage
                >= RESIDENT_RADIX_STORAGE_BINDINGS;
        let resident_visible_compaction_enabled = resident_radix8_enabled
            && prefer_resident_visible_compaction_for_target()
            && device.limits().max_storage_buffers_per_shader_stage
                >= RESIDENT_COMPACTION_STORAGE_BINDINGS;
        let group_count = workgroup_count(count);
        let dispatch_limit = device.limits().max_compute_workgroups_per_dimension;
        let keygen_dispatch = Dispatch2d::for_workgroups(group_count, dispatch_limit)
            .expect("dispatch limits were validated");
        let visible_seed = resident_visible_compaction_enabled
            .then(|| ResidentVisibleCompactionSeed::new(device, group_count));
        let indirect_args = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-indirect-args"),
            contents: bytemuck::bytes_of(&GpuDrawIndirectArgs {
                // Callers update this to the selected geometry's quad size.
                vertex_count: 0,
                instance_count: 0,
                first_vertex: 0,
                first_instance: 0,
            }),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
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
                    NonZeroU64::new(size_of::<GpuSurfaceRenderParams>() as u64),
                ),
                storage_entry(2, false),
                storage_entry(3, false),
                storage_entry(9, false),
            ],
        });
        let keygen_pipeline_layout = pipeline_layout(
            device,
            "gsplat-direct-gpu-order-keygen-layout",
            &keygen_layout,
        );
        let keygen_pipeline = compute_pipeline(
            device,
            &shader,
            &keygen_pipeline_layout,
            "generate_pairs",
            "gsplat-direct-gpu-order-keygen-pipeline",
            depth_key_precision,
        );
        let radix_profile = if let Some(seed) = visible_seed.as_ref() {
            StableFull32RadixProfile::ResidentVisible {
                control: seed.control(),
                group_offsets: seed.group_offsets(),
                group_offset_count: seed.group_offset_count(),
            }
        } else if resident_radix8_enabled {
            StableFull32RadixProfile::Resident
        } else {
            StableFull32RadixProfile::Direct
        };
        let radix = StableFull32Radix::new(
            device,
            &shader,
            capacity,
            count,
            dispatch_limit,
            radix_profile,
            compatibility_output,
        );
        let keygen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-keygen-bg"),
            layout: &keygen_layout,
            entries: &[
                entire_buffer_entry(0, source_buffer),
                entire_buffer_entry(1, render_params_buffer),
                entire_buffer_entry(2, radix.input_keys()),
                entire_buffer_entry(3, radix.input_source_ids()),
                entire_buffer_entry(9, &indirect_args),
            ],
        });
        let resident_visible_compaction = visible_seed.map(|seed| {
            seed.bind(
                device,
                ResidentVisibleCompactionBindings::new(
                    source_buffer,
                    render_params_buffer,
                    radix.spare_keys(),
                    radix.input_keys(),
                    radix.input_source_ids(),
                    &indirect_args,
                ),
                keygen_dispatch.x,
                keygen_dispatch.y,
            )
        });
        Ok(Self {
            count,
            #[cfg(test)]
            depth_key_precision,
            keygen_dispatch,
            radix,
            indirect_args,
            keygen_bind_group,
            resident_visible_compaction,
            keygen_pipeline,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn final_pairs(&self) -> &wgpu::Buffer {
        self.radix.final_pairs()
    }

    /// SoA output for the renderer integration that removes the compatibility
    /// pair pack. Both four byte passes and eight nibble passes leave the final
    /// IDs in buffer A.
    #[allow(dead_code)]
    pub(crate) fn final_ids(&self) -> &wgpu::Buffer {
        self.radix.final_source_ids()
    }

    pub(crate) fn indirect_args(&self) -> &wgpu::Buffer {
        &self.indirect_args
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[cfg(test)]
    pub(crate) const fn depth_key_precision(&self) -> DepthKeyPrecision {
        self.depth_key_precision
    }

    pub(crate) fn set_indirect_vertex_count(&self, queue: &wgpu::Queue, vertex_count: u32) {
        queue.write_buffer(&self.indirect_args, 0, bytemuck::bytes_of(&vertex_count));
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        self.encode_with_timestamps(encoder, None);
    }

    pub(crate) fn encode_with_timestamps(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timestamps: Option<GpuOrderTimestampRange<'_>>,
    ) {
        if self.count == 0 {
            return;
        }
        // Preserve vertex/first fields while resetting only the atomically
        // generated exact visible instance count.
        encoder.clear_buffer(&self.indirect_args, 4, Some(4));
        if let Some(compaction) = &self.resident_visible_compaction {
            compaction.reset_sentinel(encoder);
            compaction.encode_keygen(
                encoder,
                timestamps.map(|range| wgpu::ComputePassTimestampWrites {
                    query_set: range.query_set,
                    beginning_of_pass_write_index: Some(range.keygen_begin_index),
                    end_of_pass_write_index: None,
                }),
            );
            self.radix.encode_visible_compaction_scan(encoder);
            compaction.encode_compact(encoder);
            compaction.encode_finalize(
                encoder,
                timestamps.map(|range| wgpu::ComputePassTimestampWrites {
                    query_set: range.query_set,
                    beginning_of_pass_write_index: None,
                    end_of_pass_write_index: Some(range.keygen_end_index),
                }),
            );
        } else {
            Self::encode_stage(
                encoder,
                &self.keygen_pipeline,
                &self.keygen_bind_group,
                &[],
                self.keygen_dispatch,
                "gsplat-direct-gpu-order-keygen-pass",
                timestamps.map(|range| wgpu::ComputePassTimestampWrites {
                    query_set: range.query_set,
                    beginning_of_pass_write_index: Some(range.keygen_begin_index),
                    end_of_pass_write_index: Some(range.keygen_end_index),
                }),
            );
        }
        self.encode_radix(encoder, timestamps);
    }

    fn encode_radix(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timestamps: Option<GpuOrderTimestampRange<'_>>,
    ) {
        self.radix.encode(
            encoder,
            self.resident_visible_compaction
                .as_ref()
                .map(ResidentVisibleCompaction::control),
            timestamps.map(|range| StableFull32RadixTimestampRange {
                query_set: range.query_set,
                begin_index: range.radix_begin_index,
                end_index: range.radix_end_index,
            }),
        );
    }

    fn encode_stage(
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        bind_group: &wgpu::BindGroup,
        dynamic_offsets: &[u32],
        dispatch: Dispatch2d,
        label: &'static str,
        timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: wgpu_label(label),
            timestamp_writes,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, dynamic_offsets);
        pass.dispatch_workgroups(dispatch.x, dispatch.y, 1);
    }
}

fn pipeline_layout(
    device: &wgpu::Device,
    label: &'static str,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::PipelineLayout {
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: wgpu_label(label),
        bind_group_layouts: &[bind_group_layout],
        immediate_size: 0,
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

fn entire_buffer_entry<'a>(binding: u32, buffer: &'a wgpu::Buffer) -> wgpu::BindGroupEntry<'a> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::{path::PathBuf, sync::mpsc};

    use super::*;
    use crate::data::GpuSurfaceSourceElem;
    use crate::make_surface_render_params;
    use gsplat_core::{Camera, Vec3f, camera_trace::CameraTrace};
    use gsplat_io_ply::visit_ply_splats;
    use gsplat_sort::CpuSortBackend;

    fn test_device_with_storage_bindings(
        storage_bindings: u32,
    ) -> Option<(wgpu::Device, wgpu::Queue)> {
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
            let mut limits = wgpu::Limits::downlevel_defaults();
            limits.max_storage_buffers_per_shader_stage = storage_bindings;
            if !limits.check_limits(&adapter.limits()) {
                return None;
            }
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

    fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        test_device_with_storage_bindings(
            wgpu::Limits::downlevel_defaults().max_storage_buffers_per_shader_stage,
        )
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

    fn readback_resident_generated_keys(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sources: &[[f32; 4]],
        params: &GpuSurfaceRenderParams,
        precision: DepthKeyPrecision,
    ) -> (Vec<u32>, u32) {
        let count = u32::try_from(sources.len()).expect("test source count fits u32");
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-candidate-keygen-source"),
            contents: bytemuck::cast_slice(sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-candidate-keygen-params"),
            contents: bytemuck::bytes_of(params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let output_bytes = u64::from(count.max(1)) * size_of::<u32>() as u64;
        let raw_keys = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-candidate-raw-keys"),
            size: output_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let group_count = workgroup_count(count);
        let group_offsets = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-candidate-group-counts"),
            size: u64::from(group_count + 1) * size_of::<u32>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("resident-candidate-keygen-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/resident_gpu_order_compact.wgsl").into(),
            ),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("resident-candidate-keygen-bgl"),
            entries: &[
                storage_entry(0, true),
                uniform_entry(
                    1,
                    false,
                    NonZeroU64::new(size_of::<GpuSurfaceRenderParams>() as u64),
                ),
                storage_entry(2, false),
                storage_entry(3, false),
            ],
        });
        let pipeline_layout = pipeline_layout(device, "resident-candidate-keygen-layout", &layout);
        let constants = [(
            "DEPTH_KEY_LOW_BITS_TO_CLEAR",
            f64::from(precision.low_bits_to_clear()),
        )];
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("resident-candidate-keygen-pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("generate_keys_and_group_counts"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &constants,
                ..Default::default()
            },
            cache: None,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident-candidate-keygen-bg"),
            layout: &layout,
            entries: &[
                entire_buffer_entry(0, &source_buffer),
                entire_buffer_entry(1, &params_buffer),
                entire_buffer_entry(2, &raw_keys),
                entire_buffer_entry(3, &group_offsets),
            ],
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-candidate-keygen-readback"),
            size: output_bytes + size_of::<u32>() as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dispatch = Dispatch2d::for_workgroups(
            group_count,
            device.limits().max_compute_workgroups_per_dimension,
        )
        .expect("test dispatch fits adapter limits");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-candidate-keygen-encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("resident-candidate-keygen-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(dispatch.x, dispatch.y, 1);
        }
        encoder.copy_buffer_to_buffer(&raw_keys, 0, &readback, 0, output_bytes);
        encoder.copy_buffer_to_buffer(
            &group_offsets,
            0,
            &readback,
            output_bytes,
            size_of::<u32>() as u64,
        );
        queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll Resident candidate keygen");
        rx.recv()
            .expect("receive Resident candidate callback")
            .expect("map Resident candidate output");
        let words = {
            let mapped = slice.get_mapped_range();
            bytemuck::cast_slice::<u8, u32>(&mapped).to_vec()
        };
        readback.unmap();
        let visible_count = words[count as usize];
        (words[..count as usize].to_vec(), visible_count)
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
        let keys = pairs.iter().map(|pair| pair.key).collect::<Vec<_>>();
        let ids = pairs.iter().map(|pair| pair.id).collect::<Vec<_>>();
        queue.write_buffer(order.radix.input_keys(), 0, bytemuck::cast_slice(&keys));
        queue.write_buffer(
            order.radix.input_source_ids(),
            0,
            bytemuck::cast_slice(&ids),
        );

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
        order.encode_radix(&mut encoder, None);
        encoder.copy_buffer_to_buffer(order.final_pairs(), 0, &readback, 0, output_bytes);
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

    fn sorted_ids_on_resident_gpu(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pairs: &[GpuSortPair],
    ) -> Vec<u32> {
        let count = u32::try_from(pairs.len()).expect("test pair count fits u32");
        let keys = pairs.iter().map(|pair| pair.key).collect::<Vec<_>>();
        let ids = pairs.iter().map(|pair| pair.id).collect::<Vec<_>>();
        let (source, params) = dummy_inputs(device);
        let order = DirectGpuOrder::new_resident_soa(device, &source, &params, count, count)
            .expect("Resident GPU order must initialize");
        assert!(order.radix.uses_resident_radix8());
        assert_eq!(order.radix.radix_passes(), RESIDENT_RADIX_PASSES);
        queue.write_buffer(order.radix.input_keys(), 0, bytemuck::cast_slice(&keys));
        queue.write_buffer(
            order.radix.input_source_ids(),
            0,
            bytemuck::cast_slice(&ids),
        );

        let output_bytes = u64::from(count) * size_of::<u32>() as u64;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-radix8-test-readback"),
            size: output_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-radix8-test-encoder"),
        });
        order.encode_radix(&mut encoder, None);
        encoder.copy_buffer_to_buffer(order.final_ids(), 0, &readback, 0, output_bytes);
        queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll Resident radix8 test");
        rx.recv()
            .expect("receive Resident radix8 map callback")
            .expect("map Resident radix8 result");
        let actual = {
            let mapped = slice.get_mapped_range();
            bytemuck::cast_slice::<u8, u32>(&mapped).to_vec()
        };
        readback.unmap();
        actual
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

    fn readback_indirect_args(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        order: &DirectGpuOrder,
        vertex_count: u32,
    ) -> GpuDrawIndirectArgs {
        order.set_indirect_vertex_count(queue, vertex_count);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("direct-gpu-order-indirect-test-readback"),
            size: size_of::<GpuDrawIndirectArgs>() as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("direct-gpu-order-indirect-test-encoder"),
        });
        order.encode(&mut encoder);
        encoder.copy_buffer_to_buffer(
            order.indirect_args(),
            0,
            &readback,
            0,
            size_of::<GpuDrawIndirectArgs>() as u64,
        );
        queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll indirect args test GPU");
        rx.recv()
            .expect("receive indirect args map callback")
            .expect("map indirect args");
        let result = {
            let mapped = slice.get_mapped_range();
            *bytemuck::from_bytes::<GpuDrawIndirectArgs>(&mapped)
        };
        readback.unmap();
        result
    }

    fn readback_compacted_resident_order(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        order: &DirectGpuOrder,
    ) -> (Vec<u32>, GpuDrawIndirectArgs, ResidentOrderControl) {
        let compaction = order
            .resident_visible_compaction
            .as_ref()
            .expect("test requires Resident visible compaction");
        order.set_indirect_vertex_count(queue, 4);
        let ids_bytes = u64::from(order.count) * size_of::<u32>() as u64;
        let ids_readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-visible-test-ids-readback"),
            size: ids_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let args_readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-visible-test-args-readback"),
            size: size_of::<GpuDrawIndirectArgs>() as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let control_readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-visible-test-control-readback"),
            size: size_of::<ResidentOrderControl>() as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-visible-test-encoder"),
        });
        order.encode(&mut encoder);
        encoder.copy_buffer_to_buffer(order.final_ids(), 0, &ids_readback, 0, ids_bytes);
        encoder.copy_buffer_to_buffer(
            order.indirect_args(),
            0,
            &args_readback,
            0,
            size_of::<GpuDrawIndirectArgs>() as u64,
        );
        encoder.copy_buffer_to_buffer(
            compaction.control(),
            0,
            &control_readback,
            0,
            size_of::<ResidentOrderControl>() as u64,
        );
        queue.submit(Some(encoder.finish()));

        let ids_slice = ids_readback.slice(..);
        let (ids_tx, ids_rx) = mpsc::channel();
        ids_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = ids_tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll compacted Resident IDs");
        ids_rx
            .recv()
            .expect("receive compacted Resident ID map callback")
            .expect("map compacted Resident IDs");
        let ids = {
            let mapped = ids_slice.get_mapped_range();
            bytemuck::cast_slice::<u8, u32>(&mapped).to_vec()
        };
        ids_readback.unmap();

        let args_slice = args_readback.slice(..);
        let (args_tx, args_rx) = mpsc::channel();
        args_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = args_tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll compacted Resident draw args");
        args_rx
            .recv()
            .expect("receive compacted Resident args map callback")
            .expect("map compacted Resident draw args");
        let args = {
            let mapped = args_slice.get_mapped_range();
            *bytemuck::from_bytes::<GpuDrawIndirectArgs>(&mapped)
        };
        args_readback.unmap();

        let control_slice = control_readback.slice(..);
        let (control_tx, control_rx) = mpsc::channel();
        control_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = control_tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll Resident order control");
        control_rx
            .recv()
            .expect("receive Resident order control map callback")
            .expect("map Resident order control");
        let control = {
            let mapped = control_slice.get_mapped_range();
            *bytemuck::from_bytes::<ResidentOrderControl>(&mapped)
        };
        control_readback.unmap();
        (ids, args, control)
    }

    fn assert_visible_compaction_case(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        depths: &[f32],
        near_plane: f32,
        far_plane: f32,
    ) {
        let sources = depths
            .iter()
            .map(|&depth| [0.0_f32, 0.0, depth, 0.0])
            .collect::<Vec<_>>();
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-visible-test-source"),
            contents: bytemuck::cast_slice(&sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let count = u32::try_from(depths.len()).expect("test depth count fits u32");
        let mut params = GpuSurfaceRenderParams::zeroed();
        params.view_rot_row2 = [0.0, 0.0, 1.0, 0.0];
        params.near_plane = near_plane;
        params.far_plane = far_plane;
        params.len = count;
        params.source_position_stride_words = 4;
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-visible-test-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let order =
            DirectGpuOrder::new_resident_soa(device, &source_buffer, &params_buffer, count, count)
                .expect("Resident visible compaction must initialize");
        assert!(order.resident_visible_compaction.is_some());
        let (actual_ids, args, control) = readback_compacted_resident_order(device, queue, &order);
        let mut expected = depths
            .iter()
            .enumerate()
            .filter_map(|(id, &depth)| {
                (depth >= near_plane && depth <= far_plane).then_some(GpuSortPair {
                    key: depth.to_bits(),
                    id: id as u32,
                })
            })
            .collect::<Vec<_>>();
        expected.sort_by(|left, right| right.key.cmp(&left.key));
        let expected_ids = expected.iter().map(|pair| pair.id).collect::<Vec<_>>();
        assert_eq!(&actual_ids[..expected_ids.len()], expected_ids);
        assert_eq!(args.instance_count as usize, expected_ids.len());
        assert_eq!(control.visible_count as usize, expected_ids.len());
        let expected_groups = u32::try_from(expected_ids.len())
            .expect("visible test count fits u32")
            .div_ceil(TILE_SIZE);
        assert_eq!(control.active_group_count, expected_groups);
        assert_eq!(control.dispatch_x, expected_groups);
        assert_eq!(control.dispatch_y, 1);
        assert_eq!(control.dispatch_z, 1);
    }

    fn assert_visible_compaction_refresh_sequence(device: &wgpu::Device, queue: &wgpu::Queue) {
        let depths = (0..4_099)
            .map(|index| 1.0 + (index % 97) as f32)
            .collect::<Vec<_>>();
        let sources = depths
            .iter()
            .map(|&depth| [0.0_f32, 0.0, depth, 0.0])
            .collect::<Vec<_>>();
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-visible-refresh-test-source"),
            contents: bytemuck::cast_slice(&sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let count = u32::try_from(depths.len()).expect("refresh depth count fits u32");
        let mut params = GpuSurfaceRenderParams::zeroed();
        params.view_rot_row2 = [0.0, 0.0, 1.0, 0.0];
        params.near_plane = 0.1;
        params.far_plane = 100.0;
        params.len = count;
        params.source_position_stride_words = 4;
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-visible-refresh-test-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let order =
            DirectGpuOrder::new_resident_soa(device, &source_buffer, &params_buffer, count, count)
                .expect("Resident visible refresh sequence must initialize");

        for &(near_plane, far_plane) in &[(0.1, 100.0), (30.0, 31.0), (200.0, 300.0), (0.1, 100.0)]
        {
            params.near_plane = near_plane;
            params.far_plane = far_plane;
            queue.write_buffer(&params_buffer, 0, bytemuck::bytes_of(&params));
            let (actual_ids, args, control) =
                readback_compacted_resident_order(device, queue, &order);
            let mut expected = depths
                .iter()
                .enumerate()
                .filter_map(|(id, &depth)| {
                    (depth >= near_plane && depth <= far_plane).then_some(GpuSortPair {
                        key: depth.to_bits(),
                        id: id as u32,
                    })
                })
                .collect::<Vec<_>>();
            expected.sort_by(|left, right| right.key.cmp(&left.key));
            let expected_ids = expected.iter().map(|pair| pair.id).collect::<Vec<_>>();
            assert_eq!(&actual_ids[..expected_ids.len()], expected_ids);
            assert_eq!(args.instance_count as usize, expected_ids.len());
            assert_eq!(control.visible_count as usize, expected_ids.len());
            let expected_groups = u32::try_from(expected_ids.len())
                .expect("visible refresh count fits u32")
                .div_ceil(TILE_SIZE);
            assert_eq!(control.active_group_count, expected_groups);
            assert_eq!(control.dispatch_x, expected_groups);
        }
    }

    fn assert_external_resident_visible_order(
        ply_path: &std::path::Path,
        trace_path: &std::path::Path,
        frame_index: usize,
        expected_trace_sha256: &str,
        expected_source_count: usize,
        expected_visible_count: usize,
        label: &str,
    ) {
        if !ply_path.is_file() {
            eprintln!(
                "skipping external {label} Resident visible-order regression; missing {}",
                ply_path.display()
            );
            return;
        }
        let Some((device, queue)) =
            test_device_with_storage_bindings(RESIDENT_COMPACTION_STORAGE_BINDINGS)
        else {
            eprintln!(
                "skipping external {label} Resident visible-order regression; adapter unavailable"
            );
            return;
        };
        if !prefer_resident_visible_compaction_for_target() {
            eprintln!("skipping external {label} Resident visible-order regression outside macOS");
            return;
        }

        let trace = CameraTrace::from_json_slice(
            &std::fs::read(trace_path).expect("read external scene camera trace"),
        )
        .expect("validate external scene camera trace");
        assert_eq!(trace.content_sha256, expected_trace_sha256);
        let camera = trace.frames[frame_index]
            .camera()
            .expect("external scene trace camera");
        let mut positions = Vec::new();
        let summary = visit_ply_splats(ply_path, |splat| {
            positions.push(splat.position_ruf);
        })
        .expect("stream external scene positions");
        assert_eq!(positions.len(), summary.gaussians);
        assert_eq!(positions.len(), expected_source_count);

        let sources = positions
            .iter()
            .map(|position| [position.x, position.y, position.z, 0.0_f32])
            .collect::<Vec<_>>();
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("external-resident-visible-order-source"),
            contents: bytemuck::cast_slice(&sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let count = u32::try_from(positions.len()).expect("external source count fits u32");
        let mut params = make_surface_render_params(
            &camera,
            trace.display.width,
            trace.display.height,
            count,
            u32::from(summary.sh_degree),
        );
        params.source_position_stride_words = 4;
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("external-resident-visible-order-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let order =
            DirectGpuOrder::new_resident_soa(&device, &source_buffer, &params_buffer, count, count)
                .expect("external Resident visible order must initialize");
        let (gpu_ids, args, control) = readback_compacted_resident_order(&device, &queue, &order);

        let mut cpu_depth_keys = Vec::with_capacity(positions.len());
        let mut cpu_ids = Vec::with_capacity(positions.len());
        crate::preprocess_positions_visible_into(
            &positions,
            &camera,
            &mut cpu_depth_keys,
            &mut cpu_ids,
        )
        .expect("CPU preprocess external visible order");
        CpuSortBackend::default()
            .sort_values_by_keys(&cpu_depth_keys, &mut cpu_ids)
            .expect("CPU sort external visible order");

        assert_eq!(cpu_ids.len(), expected_visible_count);
        assert_eq!(args.instance_count as usize, expected_visible_count);
        assert_eq!(control.visible_count as usize, expected_visible_count);
        assert_eq!(&gpu_ids[..expected_visible_count], cpu_ids);
        eprintln!(
            "RESIDENT_VISIBLE_COMPACTION_EXTERNAL_ORDER scene={label} trace_sha256={} source_count={} visible_count={} differing_ranks=0 full_32_bit=true stable_source_id_ties=true",
            trace.content_sha256,
            positions.len(),
            expected_visible_count,
        );
    }

    #[test]
    fn stable_radix_matches_cpu_or_skips_without_adapter() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping stable GPU radix test; adapter unavailable");
            return;
        };

        assert_case(&device, &queue, Vec::new());
        assert_case(&device, &queue, vec![GpuSortPair { key: 7, id: 91 }]);
        for count in [127_usize, 128, 129, 1023, 1024, 1025, 4099, 32_769] {
            let pairs = (0..count)
                .map(|index| GpuSortPair {
                    key: match index % 11 {
                        0 => 0,
                        1 => u32::MAX,
                        _ => (index as u32).wrapping_mul(2_654_435_761_u32),
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
    fn resident_radix8_preserves_full_32_bit_stable_order() {
        let Some((device, queue)) = test_device_with_storage_bindings(5) else {
            eprintln!("skipping fused Resident GPU radix test; five storage bindings unavailable");
            return;
        };
        let count = 4_099_u32;
        let pairs = (0..count)
            .map(|index| GpuSortPair {
                key: match index % 13 {
                    0 => 0,
                    1 => u32::MAX,
                    2 | 3 => 0x7f80_0000,
                    _ => index.wrapping_mul(2_654_435_761),
                },
                id: count - index,
            })
            .collect::<Vec<_>>();
        let keys = pairs.iter().map(|pair| pair.key).collect::<Vec<_>>();
        let ids = pairs.iter().map(|pair| pair.id).collect::<Vec<_>>();
        let mut expected = pairs.clone();
        expected.sort_by(|left, right| right.key.cmp(&left.key));

        let (source, params) = dummy_inputs(&device);
        let order = DirectGpuOrder::new_resident_soa(&device, &source, &params, count, count)
            .expect("fused Resident GPU order must initialize");
        assert!(order.radix.uses_resident_radix8());
        assert_eq!(order.radix.radix_passes(), RESIDENT_RADIX_PASSES);
        assert_eq!(
            order.radix.prefix().size(),
            u64::from(workgroup_count(count) * RESIDENT_RADIX) * size_of::<u32>() as u64
        );
        queue.write_buffer(order.radix.input_keys(), 0, bytemuck::cast_slice(&keys));
        queue.write_buffer(
            order.radix.input_source_ids(),
            0,
            bytemuck::cast_slice(&ids),
        );

        let output_bytes = u64::from(count) * size_of::<u32>() as u64;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-fused-radix-test-readback"),
            size: output_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-fused-radix-test-encoder"),
        });
        order.encode_radix(&mut encoder, None);
        encoder.copy_buffer_to_buffer(order.final_ids(), 0, &readback, 0, output_bytes);
        queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll fused GPU radix test");
        rx.recv()
            .expect("receive fused radix map callback")
            .expect("map fused radix result");
        let actual = {
            let mapped = slice.get_mapped_range();
            bytemuck::cast_slice::<u8, u32>(&mapped).to_vec()
        };
        readback.unmap();
        assert_eq!(
            actual,
            expected.iter().map(|pair| pair.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn resident_visible_compaction_is_stable_at_predicate_and_tile_boundaries() {
        let Some((device, queue)) =
            test_device_with_storage_bindings(RESIDENT_COMPACTION_STORAGE_BINDINGS)
        else {
            eprintln!("skipping Resident visible compaction test; adapter unavailable");
            return;
        };
        if !prefer_resident_visible_compaction_for_target() {
            eprintln!("skipping Resident visible compaction test outside macOS experiment");
            return;
        }

        assert_visible_compaction_case(
            &device,
            &queue,
            &(0..2_051)
                .map(|index| 1.0 + (index % 31) as f32)
                .collect::<Vec<_>>(),
            0.1,
            100.0,
        );
        assert_visible_compaction_case(
            &device,
            &queue,
            &(0..2_051)
                .map(|index| if index % 2 == 0 { 0.05 } else { 101.0 })
                .collect::<Vec<_>>(),
            0.1,
            100.0,
        );
        assert_visible_compaction_case(
            &device,
            &queue,
            &[
                0.1,
                f32::from_bits(0.1_f32.to_bits() - 1),
                10.0,
                f32::from_bits(10.0_f32.to_bits() + 1),
                4.0,
                4.0,
                2.0,
                4.0,
            ],
            0.1,
            10.0,
        );
        assert_visible_compaction_case(
            &device,
            &queue,
            &(0..4_099)
                .map(|index| match index % 11 {
                    0..=4 => 7.0,
                    5 => 0.05,
                    6 => 101.0,
                    _ => 1.0 + (index % 97) as f32,
                })
                .collect::<Vec<_>>(),
            0.1,
            100.0,
        );
        assert_visible_compaction_refresh_sequence(&device, &queue);
    }

    #[test]
    fn direct_and_four_binding_fallback_keep_eight_nibble_passes() {
        let Some((device, _queue)) = test_device_with_storage_bindings(4) else {
            eprintln!("skipping low-binding GPU radix test; adapter unavailable");
            return;
        };
        let (source, params) = dummy_inputs(&device);
        let direct = DirectGpuOrder::new(&device, &source, &params, 1, 0)
            .expect("Direct low-binding order must initialize");
        assert!(!direct.radix.uses_resident_radix8());
        assert_eq!(direct.radix.radix_passes(), DIRECT_RADIX_PASSES);
        assert_eq!(
            direct.radix.prefix().size(),
            u64::from(DIRECT_RADIX) * size_of::<u32>() as u64
        );

        let soa_fallback = DirectGpuOrder::new_soa(&device, &source, &params, 1, 0)
            .expect("SoA low-binding fallback must initialize");
        assert!(!soa_fallback.radix.uses_resident_radix8());
        assert_eq!(soa_fallback.radix.radix_passes(), DIRECT_RADIX_PASSES);

        let resident_fallback = DirectGpuOrder::new_resident_soa(&device, &source, &params, 1, 0)
            .expect("Resident low-binding fallback must initialize");
        assert!(!resident_fallback.radix.uses_resident_radix8());
        assert_eq!(resident_fallback.radix.radix_passes(), DIRECT_RADIX_PASSES);
    }

    #[test]
    fn resident_radix8_default_is_a_native_macos_allowlist() {
        assert_eq!(
            prefer_resident_radix8_for_target(),
            cfg!(all(target_os = "macos", not(target_arch = "wasm32")))
        );
    }

    #[test]
    #[ignore = "native GPU pressure regression matching the complete Truck source count"]
    fn resident_radix8_matches_cpu_at_full_truck_count() {
        let Some((device, queue)) = test_device_with_storage_bindings(5) else {
            eprintln!("skipping full-Truck Resident radix8 test; adapter unavailable");
            return;
        };
        let count = 2_541_226_u32;
        let pairs = (0..count)
            .map(|index| GpuSortPair {
                // Exercise every key byte and many stable ties. These are raw
                // key bits on purpose; a separate key-generation gate owns
                // positive-f32 depth classification.
                key: if index % 97 == 0 {
                    0x40a0_0000
                } else if index % 193 == 0 {
                    0
                } else {
                    index.wrapping_mul(2_654_435_761).rotate_left(index & 31)
                },
                id: count - index,
            })
            .collect::<Vec<_>>();
        let mut expected = pairs.clone();
        expected.sort_by(|left, right| right.key.cmp(&left.key));
        let actual = sorted_ids_on_resident_gpu(&device, &queue, &pairs);
        assert_eq!(
            actual,
            expected.iter().map(|pair| pair.id).collect::<Vec<_>>()
        );
        eprintln!(
            "RESIDENT_RADIX8_FULL_TRUCK_ORDER count={count} passes={} radix={} stable_exact=true",
            RESIDENT_RADIX_PASSES, RESIDENT_RADIX
        );
    }

    #[test]
    #[ignore = "requires the external full Truck PLY and a native Metal GPU"]
    fn external_truck_view0_resident_visible_order_matches_cpu() {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert_external_resident_visible_order(
            &workspace.join("tests/datasets/external/inria_3dgs/truck/point_cloud.ply"),
            &workspace.join(
                "tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json",
            ),
            0,
            "34d47dbddf73d915bfd55431b33da9430882767a40d9d74c636c508f7d7a5ab3",
            2_541_226,
            1_886_298,
            "truck-view0",
        );
    }

    #[test]
    #[ignore = "requires the external full Garden PLY and a native Metal GPU"]
    fn external_garden_view1_resident_visible_order_matches_cpu() {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert_external_resident_visible_order(
            &workspace.join("tests/datasets/external/inria_3dgs/garden/point_cloud.ply"),
            &workspace.join(
                "tests/perf/trace/fixtures/quality/candidate-garden-quality-1920x1080-v1.json",
            ),
            1,
            "1b1aa0a0e1f4a4b9744a4c681fa46a7cd0f50d5390a7260812712e81721f5518",
            5_834_784,
            4_226_208,
            "garden-view1",
        );
    }

    #[test]
    fn key_generation_dispatch_covers_large_scene_ladder() {
        assert_eq!(workgroup_count(0), 1);
        assert_eq!(workgroup_count(1024), 1);
        assert_eq!(workgroup_count(1025), 2);
        assert!(workgroup_count(6_131_954) < 65_535);
    }

    #[test]
    fn two_dimensional_dispatch_covers_each_logical_group() {
        for logical_groups in [1, 7, 8, 47, 48, 49] {
            let dispatch = Dispatch2d::for_workgroups(logical_groups, 7)
                .expect("logical workgroups must fit 7x7");
            assert!(dispatch.x <= 7);
            assert!(dispatch.y <= 7);
            assert!(dispatch.x * dispatch.y >= logical_groups);
        }
        assert_eq!(Dispatch2d::for_workgroups(50, 7), None);
    }

    #[test]
    fn scan_hierarchy_reduces_until_one_workgroup() {
        assert_eq!(scan_level_counts(16), vec![(16, 1)]);
        assert_eq!(scan_level_counts(528), vec![(528, 2), (2, 1)]);
        assert_eq!(
            scan_level_counts(262_145),
            vec![(262_145, 513), (513, 2), (2, 1)]
        );
    }

    #[test]
    fn soa_path_omits_the_legacy_pair_buffer_and_its_size_limit() {
        let Some((device, _)) = test_device() else {
            eprintln!("skipping SoA GPU order test; adapter unavailable");
            return;
        };
        let storage_limit = device
            .limits()
            .max_buffer_size
            .min(u64::from(device.limits().max_storage_buffer_binding_size));
        let capacity = u32::try_from(storage_limit / size_of::<GpuSortPair>() as u64 + 1)
            .expect("wgpu storage binding limits fit u32 element counts");
        assert!(DirectGpuOrder::validate_soa_dispatch_limits(&device, capacity, capacity).is_ok());
        assert!(DirectGpuOrder::validate_dispatch_limits(&device, capacity, capacity).is_err());

        let (source, params) = dummy_inputs(&device);
        let order = DirectGpuOrder::new_soa(&device, &source, &params, 1, 0)
            .expect("small SoA order must initialize");
        assert!(!order.radix.has_compatibility_output());
        assert!(!order.radix.uses_resident_radix8());
        assert_eq!(order.radix.radix_passes(), DIRECT_RADIX_PASSES);
        assert_eq!(order.final_ids().size(), size_of::<u32>() as u64);
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
        params.source_position_stride_words = 16;
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

    #[test]
    fn candidate_high24_cpu_and_both_gpu_key_generators_match() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping candidate depth-key GPU test; adapter unavailable");
            return;
        };
        let depths = [
            f32::from_bits(0x3f80_0001),
            f32::from_bits(0x3f80_00fe),
            1.5,
            1.0,
            2.0,
            f32::MIN_POSITIVE,
            0.0,
            f32::from_bits(2.0_f32.to_bits() + 1),
        ];
        let positions = depths
            .iter()
            .copied()
            .map(|depth| Vec3f::new(0.0, 0.0, depth))
            .collect::<Vec<_>>();
        let sources = depths
            .iter()
            .copied()
            .map(|depth| [0.0, 0.0, depth, 0.0])
            .collect::<Vec<_>>();
        let mut camera = Camera::default();
        camera.intrinsics.near_plane = f32::MIN_POSITIVE;
        camera.intrinsics.far_plane = 2.0;
        let mut params = GpuSurfaceRenderParams::zeroed();
        params.view_rot_row2 = [0.0, 0.0, 1.0, 0.0];
        params.near_plane = camera.intrinsics.near_plane;
        params.far_plane = camera.intrinsics.far_plane;
        params.len = depths.len() as u32;
        params.source_position_stride_words = 4;
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("candidate-high24-direct-source"),
            contents: bytemuck::cast_slice(&sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("candidate-high24-direct-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let exact = DirectGpuOrder::new(
            &device,
            &source_buffer,
            &params_buffer,
            depths.len() as u32,
            depths.len() as u32,
        )
        .expect("Exact key generator");
        let candidate = DirectGpuOrder::new_with_depth_key_precision(
            &device,
            &source_buffer,
            &params_buffer,
            depths.len() as u32,
            depths.len() as u32,
            DepthKeyPrecision::CandidateStable24,
        )
        .expect("candidate key generator");
        let exact_pairs = readback_pairs(&device, &queue, &exact);
        let candidate_pairs = readback_pairs(&device, &queue, &candidate);

        let mut cpu_keys = Vec::new();
        let mut cpu_ids = Vec::new();
        crate::cpu_order::preprocess_positions_visible_into_with_precision(
            &positions,
            &camera,
            DepthKeyPrecision::CandidateStable24,
            &mut cpu_keys,
            &mut cpu_ids,
        )
        .expect("CPU candidate preprocess");
        let visible_count = cpu_ids.len();
        CpuSortBackend::default()
            .sort_values_by_keys(&cpu_keys, &mut cpu_ids)
            .expect("CPU candidate radix");
        let expected_visible = cpu_ids
            .iter()
            .map(|&id| GpuSortPair {
                key: crate::cpu_order::depth_to_key_with_precision(
                    depths[id as usize],
                    DepthKeyPrecision::CandidateStable24,
                ),
                id,
            })
            .collect::<Vec<_>>();

        assert_eq!(&candidate_pairs[..visible_count], expected_visible);
        assert!(
            candidate_pairs[visible_count..]
                .iter()
                .all(|pair| pair.key == 0),
            "visible_count={visible_count} candidate_pairs={candidate_pairs:?}"
        );
        assert_eq!(candidate_pairs[0].id, 4);
        assert_eq!(
            &candidate_pairs[2..5],
            [
                GpuSortPair {
                    key: 0x3f80_0000,
                    id: 0,
                },
                GpuSortPair {
                    key: 0x3f80_0000,
                    id: 1,
                },
                GpuSortPair {
                    key: 0x3f80_0000,
                    id: 3,
                },
            ]
        );
        assert_eq!(
            &exact_pairs[2..5],
            [
                GpuSortPair {
                    key: 0x3f80_00fe,
                    id: 1,
                },
                GpuSortPair {
                    key: 0x3f80_0001,
                    id: 0,
                },
                GpuSortPair {
                    key: 0x3f80_0000,
                    id: 3,
                },
            ]
        );
        assert_eq!(
            readback_indirect_args(&device, &queue, &candidate, 4).instance_count,
            visible_count as u32
        );
        assert_eq!(
            candidate_pairs[visible_count - 1],
            GpuSortPair {
                key: f32::MIN_POSITIVE.to_bits(),
                id: 5,
            }
        );

        let (resident_keys, resident_visible_count) = readback_resident_generated_keys(
            &device,
            &queue,
            &sources,
            &params,
            DepthKeyPrecision::CandidateStable24,
        );
        let expected_raw = depths
            .iter()
            .copied()
            .map(|depth| {
                if (params.near_plane..=params.far_plane).contains(&depth) {
                    crate::cpu_order::depth_to_key_with_precision(
                        depth,
                        DepthKeyPrecision::CandidateStable24,
                    )
                } else {
                    0
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(resident_keys, expected_raw);
        assert_eq!(resident_visible_count, visible_count as u32);
    }

    #[test]
    fn candidate_high20_cpu_and_both_gpu_key_generators_match() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping candidate depth-key GPU test; adapter unavailable");
            return;
        };
        let depths = [
            f32::from_bits(0x3f80_0001),
            f32::from_bits(0x3f80_0ffe),
            1.5,
            2.0,
            f32::MIN_POSITIVE,
            0.0,
        ];
        let positions = depths
            .iter()
            .copied()
            .map(|depth| Vec3f::new(0.0, 0.0, depth))
            .collect::<Vec<_>>();
        let sources = depths
            .iter()
            .copied()
            .map(|depth| [0.0, 0.0, depth, 0.0])
            .collect::<Vec<_>>();
        let mut camera = Camera::default();
        camera.intrinsics.near_plane = f32::MIN_POSITIVE;
        camera.intrinsics.far_plane = 2.0;
        let mut params = GpuSurfaceRenderParams::zeroed();
        params.view_rot_row2 = [0.0, 0.0, 1.0, 0.0];
        params.near_plane = camera.intrinsics.near_plane;
        params.far_plane = camera.intrinsics.far_plane;
        params.len = depths.len() as u32;
        params.source_position_stride_words = 4;
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("candidate-high20-direct-source"),
            contents: bytemuck::cast_slice(&sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("candidate-high20-direct-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let candidate = DirectGpuOrder::new_with_depth_key_precision(
            &device,
            &source_buffer,
            &params_buffer,
            depths.len() as u32,
            depths.len() as u32,
            DepthKeyPrecision::CandidateStable20,
        )
        .expect("candidate key generator");
        let candidate_pairs = readback_pairs(&device, &queue, &candidate);

        let mut cpu_keys = Vec::new();
        let mut cpu_ids = Vec::new();
        crate::cpu_order::preprocess_positions_visible_into_with_precision(
            &positions,
            &camera,
            DepthKeyPrecision::CandidateStable20,
            &mut cpu_keys,
            &mut cpu_ids,
        )
        .expect("CPU candidate preprocess");
        let visible_count = cpu_ids.len();
        CpuSortBackend::default()
            .sort_values_by_keys(&cpu_keys, &mut cpu_ids)
            .expect("CPU candidate radix");
        let expected_visible = cpu_ids
            .iter()
            .map(|&id| GpuSortPair {
                key: crate::cpu_order::depth_to_key_with_precision(
                    depths[id as usize],
                    DepthKeyPrecision::CandidateStable20,
                ),
                id,
            })
            .collect::<Vec<_>>();
        assert_eq!(&candidate_pairs[..visible_count], expected_visible);
        assert!(
            candidate_pairs[visible_count..]
                .iter()
                .all(|pair| pair.key == 0)
        );
        assert!(
            candidate_pairs[..visible_count]
                .iter()
                .all(|pair| pair.key & 0xfff == 0)
        );

        let (resident_keys, resident_visible_count) = readback_resident_generated_keys(
            &device,
            &queue,
            &sources,
            &params,
            DepthKeyPrecision::CandidateStable20,
        );
        let expected_raw = depths
            .iter()
            .copied()
            .map(|depth| {
                if (params.near_plane..=params.far_plane).contains(&depth) {
                    crate::cpu_order::depth_to_key_with_precision(
                        depth,
                        DepthKeyPrecision::CandidateStable20,
                    )
                } else {
                    0
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(resident_keys, expected_raw);
        assert_eq!(resident_visible_count, visible_count as u32);
    }

    #[test]
    fn resident_position_stride_generates_the_same_complete_order() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping resident GPU key-generation test; adapter unavailable");
            return;
        };
        let depths = [9.0_f32, 0.05, 4.0, 100.0, 4.0, 2.0, 7.0];
        let sources = depths
            .iter()
            .map(|&depth| [0.0_f32, 0.0, depth, 0.75])
            .collect::<Vec<_>>();
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-keygen-test-source"),
            contents: bytemuck::cast_slice(&sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let mut params = GpuSurfaceRenderParams::zeroed();
        params.view_rot_row2 = [0.0, 0.0, 1.0, 0.0];
        params.near_plane = 0.1;
        params.far_plane = 10.0;
        params.len = depths.len() as u32;
        params.source_position_stride_words = 4;
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-gpu-order-keygen-test-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let order = DirectGpuOrder::new(
            &device,
            &source_buffer,
            &params_buffer,
            depths.len() as u32,
            depths.len() as u32,
        )
        .expect("resident-stride key generation must initialize");
        let actual = readback_pairs(&device, &queue, &order);
        let mut expected = depths
            .iter()
            .enumerate()
            .map(|(id, &depth)| GpuSortPair {
                key: if (0.1..=10.0).contains(&depth) {
                    depth.to_bits()
                } else {
                    0
                },
                id: id as u32,
            })
            .collect::<Vec<_>>();
        expected.sort_by(|left, right| right.key.cmp(&left.key));
        assert_eq!(actual, expected);
        assert_eq!(
            readback_indirect_args(&device, &queue, &order, 4),
            GpuDrawIndirectArgs {
                vertex_count: 4,
                instance_count: 5,
                first_vertex: 0,
                first_instance: 0,
            }
        );
    }

    #[test]
    fn canonical_fma_matches_gpu_at_an_adversarial_near_plane_boundary() {
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping adversarial GPU key-generation test; adapter unavailable");
            return;
        };
        // Garden view 1 source 4,244,161. Independent rounded products put it
        // just outside the near plane, while the more accurate canonical FMA
        // sequence puts it just inside. Both ordering backends must use the
        // latter result instead of depending on native-dot contraction.
        let position = [
            f32::from_bits(0xc076_6a37),
            f32::from_bits(0xbf2f_b163),
            f32::from_bits(0x40a4_6659),
            0.0,
        ];
        let camera_position = [
            f32::from_bits(0xc04e_aea7),
            f32::from_bits(0x3ebe_32f8),
            f32::from_bits(0x3f93_6d4b),
            0.0,
        ];
        let depth_row = [
            f32::from_bits(0x3f61_0df6),
            f32::from_bits(0xbef3_ede7),
            f32::from_bits(0x3c55_07c0),
            0.0,
        ];
        let near_plane = f32::from_bits(0x3c23_d70a);
        let relative = [
            position[0] - camera_position[0],
            position[1] - camera_position[1],
            position[2] - camera_position[2],
        ];
        let unfused_depth =
            (depth_row[0] * relative[0] + depth_row[1] * relative[1]) + depth_row[2] * relative[2];
        let canonical_depth = depth_row[2].mul_add(
            relative[2],
            depth_row[1].mul_add(relative[1], depth_row[0] * relative[0]),
        );
        assert_eq!(unfused_depth.to_bits(), 0x3c23_d704);
        assert_eq!(canonical_depth.to_bits(), 0x3c23_d71c);
        assert!(unfused_depth < near_plane);
        assert!(canonical_depth >= near_plane);

        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("adversarial-near-plane-source"),
            contents: bytemuck::cast_slice(&[position]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let mut params = GpuSurfaceRenderParams::zeroed();
        params.camera_pos = camera_position;
        params.view_rot_row2 = depth_row;
        params.near_plane = near_plane;
        params.far_plane = 10.0;
        params.len = 1;
        params.source_position_stride_words = 4;
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("adversarial-near-plane-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let order = DirectGpuOrder::new(&device, &source_buffer, &params_buffer, 1, 1)
            .expect("adversarial key generation must initialize");
        assert_eq!(
            readback_pairs(&device, &queue, &order),
            vec![GpuSortPair {
                key: canonical_depth.to_bits(),
                id: 0,
            }]
        );
        assert_eq!(
            readback_indirect_args(&device, &queue, &order, 4).instance_count,
            1
        );
    }

    #[test]
    #[ignore = "requires the external 1.35 GiB Garden PLY and a native GPU"]
    fn external_garden_view1_cpu_gpu_visible_sets_are_identical() {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let ply_path = workspace.join("tests/datasets/external/inria_3dgs/garden/point_cloud.ply");
        let trace_path = workspace
            .join("tests/perf/trace/fixtures/quality/candidate-garden-quality-640x360-v1.json");
        if !ply_path.is_file() {
            eprintln!(
                "skipping external Garden parity regression; missing {}",
                ply_path.display()
            );
            return;
        }
        let Some((device, queue)) = test_device() else {
            eprintln!("skipping external Garden parity regression; adapter unavailable");
            return;
        };

        let trace = CameraTrace::from_json_slice(
            &std::fs::read(&trace_path).expect("read Garden camera trace"),
        )
        .expect("validate Garden camera trace");
        let camera = trace.frames[1].camera().expect("Garden view 1 camera");
        let mut positions = Vec::new();
        let summary = visit_ply_splats(&ply_path, |splat| {
            positions.push(splat.position_ruf);
        })
        .expect("stream Garden positions");
        assert_eq!(positions.len(), summary.gaussians);

        let sources = positions
            .iter()
            .map(|position| [position.x, position.y, position.z, 0.0_f32])
            .collect::<Vec<_>>();
        let source_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("external-garden-parity-source"),
            contents: bytemuck::cast_slice(&sources),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let count = u32::try_from(positions.len()).expect("Garden source count fits u32");
        let mut params = make_surface_render_params(
            &camera,
            trace.display.width,
            trace.display.height,
            count,
            u32::from(summary.sh_degree),
        );
        params.source_position_stride_words = 4;
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("external-garden-parity-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let order = DirectGpuOrder::new(&device, &source_buffer, &params_buffer, count, count)
            .expect("Garden direct GPU order must initialize");
        let gpu_pairs = readback_pairs(&device, &queue, &order);
        let gpu_visible_count = readback_indirect_args(&device, &queue, &order, 4).instance_count;

        let mut cpu_depth_keys = Vec::with_capacity(positions.len());
        let mut cpu_ids = Vec::with_capacity(positions.len());
        crate::preprocess_positions_visible_into(
            &positions,
            &camera,
            &mut cpu_depth_keys,
            &mut cpu_ids,
        )
        .expect("CPU preprocess Garden view 1");

        let mut cpu_visible = vec![false; positions.len()];
        for &id in &cpu_ids {
            cpu_visible[id as usize] = true;
        }
        let mut differences = Vec::new();
        for pair in &gpu_pairs {
            let gpu_visible = pair.key != 0;
            if gpu_visible != cpu_visible[pair.id as usize] {
                let position = positions[pair.id as usize];
                let cpu_depth = crate::world_to_camera_depth_with_view_row(
                    position,
                    camera.pose.position,
                    params.view_rot_row2[..3]
                        .try_into()
                        .expect("three-component depth row"),
                );
                let relative = [
                    position.x - camera.pose.position.x,
                    position.y - camera.pose.position.y,
                    position.z - camera.pose.position.z,
                ];
                let row: [f32; 3] = params.view_rot_row2[..3]
                    .try_into()
                    .expect("three-component depth row");
                let fma_depth = row[2].mul_add(
                    relative[2],
                    row[1].mul_add(relative[1], row[0] * relative[0]),
                );
                differences.push((
                    pair.id,
                    position,
                    cpu_depth,
                    f32::from_bits(pair.key),
                    relative,
                    row,
                    [
                        row[0] * relative[0],
                        row[1] * relative[1],
                        row[2] * relative[2],
                    ],
                    fma_depth,
                ));
            }
        }

        assert_eq!(
            gpu_visible_count as usize,
            gpu_pairs.iter().filter(|pair| pair.key != 0).count()
        );
        assert!(
            differences.is_empty(),
            "Garden view 1 CPU/GPU visible-set mismatch: cpu_count={}, gpu_count={}, near={:?}, far={:?}, differences={differences:?}",
            cpu_ids.len(),
            gpu_visible_count,
            camera.intrinsics.near_plane,
            camera.intrinsics.far_plane,
        );
        assert_eq!(
            trace.content_sha256,
            "9dd7c8abc4ccfd74f54ae863df2123f3ff28817b4962048a02cafb4bf99a08ec"
        );
        assert_eq!(cpu_ids.len(), 4_226_208);
        assert_eq!(gpu_visible_count, 4_226_208);
        eprintln!(
            "GARDEN_VIEW1_PARITY trace_sha256={} source_count={} cpu_visible={} gpu_visible={} differing_source_ids=0",
            trace.content_sha256,
            positions.len(),
            cpu_ids.len(),
            gpu_visible_count,
        );
    }
}

fn compute_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    entry_point: &'static str,
    label: &'static str,
    depth_key_precision: DepthKeyPrecision,
) -> wgpu::ComputePipeline {
    let constants = [(
        "DEPTH_KEY_LOW_BITS_TO_CLEAR",
        f64::from(depth_key_precision.low_bits_to_clear()),
    )];
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: wgpu_label(label),
        layout: Some(layout),
        module: shader,
        entry_point: Some(entry_point),
        compilation_options: wgpu::PipelineCompilationOptions {
            constants: &constants,
            ..Default::default()
        },
        cache: None,
    })
}
