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

use crate::{DirectSceneError, GpuSortPair, GpuSurfaceRenderParams, wgpu_label};

const WORKGROUP_SIZE: u32 = 128;
const ITEMS_PER_THREAD: u32 = 8;
const TILE_SIZE: u32 = WORKGROUP_SIZE * ITEMS_PER_THREAD;
const DIRECT_RADIX: u32 = 16;
const DIRECT_RADIX_PASSES: u32 = 8;
const RESIDENT_RADIX: u32 = 256;
const RESIDENT_RADIX_PASSES: u32 = 4;
const RESIDENT_RADIX_STORAGE_BINDINGS: u32 = 5;
const RESIDENT_COMPACTION_STORAGE_BINDINGS: u32 = 7;
const RESIDENT_CONTROL_INDIRECT_OFFSET: u64 = 2 * size_of::<u32>() as u64;
// histogram_counts (256 atomics), digit_masks (256 * 4 atomics), and
// digit_prior (256 u32s). Keeping the conservative module-wide total here
// also covers implementations that account all workgroup globals together.
const RESIDENT_RADIX_WORKGROUP_STORAGE_BYTES: u32 = (256 + 1_024 + 256) * 4;
const SCAN_WORKGROUP_SIZE: u32 = 256;
const SCAN_ITEMS_PER_GROUP: u32 = SCAN_WORKGROUP_SIZE * 2;
const SCAN_WORKGROUP_STORAGE_BYTES: u32 = SCAN_ITEMS_PER_GROUP * size_of::<u32>() as u32;

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
    count.div_ceil(TILE_SIZE).max(1)
}

fn scan_workgroup_count(count: u32) -> u32 {
    count.div_ceil(SCAN_ITEMS_PER_GROUP).max(1)
}

fn scan_level_counts(mut count: u32) -> Vec<(u32, u32)> {
    debug_assert!(count > 0);
    let mut levels = Vec::new();
    loop {
        let groups = scan_workgroup_count(count);
        levels.push((count, groups));
        if groups == 1 {
            return levels;
        }
        count = groups;
    }
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
#[derive(Clone, Copy, Pod, Zeroable)]
struct PassParams {
    shift: u32,
    count: u32,
    group_count: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ScanParams {
    count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
struct GpuDrawIndirectArgs {
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
struct ResidentOrderControl {
    visible_count: u32,
    active_group_count: u32,
    dispatch_x: u32,
    dispatch_y: u32,
    dispatch_z: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct GpuOrderTimestampRange<'a> {
    pub(crate) query_set: &'a wgpu::QuerySet,
    pub(crate) keygen_begin_index: u32,
    pub(crate) keygen_end_index: u32,
    pub(crate) radix_begin_index: u32,
    pub(crate) radix_end_index: u32,
}

struct ScanLevel {
    bind_group: wgpu::BindGroup,
    dispatch: Dispatch2d,
    dynamic_offset: u32,
}

#[cfg_attr(not(test), allow(dead_code))]
struct CompatibilityOutput {
    pairs: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::ComputePipeline,
}

/// Resident SoA scenes negotiate at least eight storage bindings for their
/// color pipeline, so they can scatter keys and IDs together. Direct keeps the
/// four-binding two-dispatch fallback for downlevel compatibility.
struct ResidentRadix8 {
    histogram_pipeline: wgpu::ComputePipeline,
    scatter_pipeline: wgpu::ComputePipeline,
    a_to_b: wgpu::BindGroup,
    b_to_a: wgpu::BindGroup,
    visible_compaction: Option<ResidentVisibleCompaction>,
}

struct ResidentVisibleCompaction {
    keygen_pipeline: wgpu::ComputePipeline,
    compact_pipeline: wgpu::ComputePipeline,
    finalize_pipeline: wgpu::ComputePipeline,
    prefix_clear_pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    group_offsets: wgpu::Buffer,
    _scan_sums: Vec<wgpu::Buffer>,
    _scan_params: wgpu::Buffer,
    scan_levels: Vec<ScanLevel>,
    dispatch: Dispatch2d,
    prefix_clear_dispatch: Dispatch2d,
    control: wgpu::Buffer,
}

pub(crate) struct DirectGpuOrder {
    count: u32,
    keygen_dispatch: Dispatch2d,
    radix_dispatch: Dispatch2d,
    #[cfg_attr(not(test), allow(dead_code))]
    keys_a: wgpu::Buffer,
    _keys_b: wgpu::Buffer,
    ids_a: wgpu::Buffer,
    _ids_b: wgpu::Buffer,
    _block_prefix: wgpu::Buffer,
    _scan_sums: Vec<wgpu::Buffer>,
    _pass_params: wgpu::Buffer,
    _scan_params: wgpu::Buffer,
    indirect_args: wgpu::Buffer,
    keygen_bind_group: wgpu::BindGroup,
    radix_a_to_b_keys: wgpu::BindGroup,
    radix_a_to_b_ids: wgpu::BindGroup,
    radix_b_to_a_keys: wgpu::BindGroup,
    radix_b_to_a_ids: wgpu::BindGroup,
    resident_radix8: Option<ResidentRadix8>,
    scan_levels: Vec<ScanLevel>,
    compatibility_output: Option<CompatibilityOutput>,
    keygen_pipeline: wgpu::ComputePipeline,
    histogram_pipeline: wgpu::ComputePipeline,
    scan_pipeline: wgpu::ComputePipeline,
    add_offsets_pipeline: wgpu::ComputePipeline,
    scatter_pipeline: wgpu::ComputePipeline,
    pass_stride: u32,
    radix_passes: u32,
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
            true,
            false,
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
            false,
            false,
        )
    }

    /// Constructs the full-resident SoA path. Unlike Direct, this path may use
    /// the fused stable byte radix negotiated by the Resident resource plan.
    pub(crate) fn new_resident_soa(
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
            false,
            prefer_resident_radix8_for_target(),
        )
    }

    fn new_inner(
        device: &wgpu::Device,
        source_buffer: &wgpu::Buffer,
        render_params_buffer: &wgpu::Buffer,
        capacity: u32,
        count: u32,
        compatibility_output: bool,
        prefer_resident_radix8: bool,
    ) -> Result<Self, DirectSceneError> {
        Self::validate_limits(
            device,
            capacity,
            count,
            compatibility_output,
            prefer_resident_radix8,
        )?;

        let allocation_count = capacity.max(1);
        let resident_radix8_enabled = prefer_resident_radix8
            && !compatibility_output
            && device.limits().max_storage_buffers_per_shader_stage
                >= RESIDENT_RADIX_STORAGE_BINDINGS;
        let resident_visible_compaction_enabled = resident_radix8_enabled
            && prefer_resident_visible_compaction_for_target()
            && device.limits().max_storage_buffers_per_shader_stage
                >= RESIDENT_COMPACTION_STORAGE_BINDINGS;
        let group_count = workgroup_count(count);
        let radix = if resident_radix8_enabled {
            RESIDENT_RADIX
        } else {
            DIRECT_RADIX
        };
        let radix_passes = if resident_radix8_enabled {
            RESIDENT_RADIX_PASSES
        } else {
            DIRECT_RADIX_PASSES
        };
        let radix_shift = if resident_radix8_enabled { 8 } else { 4 };
        let dispatch_limit = device.limits().max_compute_workgroups_per_dimension;
        let keygen_dispatch = Dispatch2d::for_workgroups(group_count, dispatch_limit)
            .expect("dispatch limits were validated");
        let radix_dispatch = keygen_dispatch;
        let element_bytes = u64::from(allocation_count) * size_of::<u32>() as u64;
        let data_usage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let keys_a = storage_buffer(
            device,
            "gsplat-direct-gpu-order-keys-a",
            element_bytes,
            data_usage,
        );
        let keys_b = storage_buffer(
            device,
            "gsplat-direct-gpu-order-keys-b",
            element_bytes,
            data_usage,
        );
        let ids_a = storage_buffer(
            device,
            "gsplat-direct-gpu-order-ids-a",
            element_bytes,
            data_usage,
        );
        let ids_b = storage_buffer(
            device,
            "gsplat-direct-gpu-order-ids-b",
            element_bytes,
            data_usage,
        );
        let prefix_count = group_count * radix;
        let block_prefix = storage_buffer(
            device,
            "gsplat-direct-gpu-order-prefix",
            u64::from(prefix_count) * size_of::<u32>() as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let level_counts = scan_level_counts(prefix_count);
        let scan_sums = level_counts
            .iter()
            .map(|&(_, groups)| {
                storage_buffer(
                    device,
                    "gsplat-direct-gpu-order-scan-sums",
                    u64::from(groups) * size_of::<u32>() as u64,
                    wgpu::BufferUsages::STORAGE,
                )
            })
            .collect::<Vec<_>>();

        let pass_stride = device.limits().min_uniform_buffer_offset_alignment.max(16);
        let mut pass_params_bytes = vec![0_u8; pass_stride as usize * radix_passes as usize];
        for pass in 0..radix_passes {
            let params = PassParams {
                shift: pass * radix_shift,
                count,
                group_count,
                _pad: 0,
            };
            let offset = pass_stride as usize * pass as usize;
            pass_params_bytes[offset..offset + size_of::<PassParams>()]
                .copy_from_slice(bytemuck::bytes_of(&params));
        }
        let pass_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-pass-params"),
            contents: &pass_params_bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let scan_stride = device.limits().min_uniform_buffer_offset_alignment.max(16);
        let mut scan_params_bytes = vec![0_u8; scan_stride as usize * level_counts.len()];
        for (level, &(level_count, _)) in level_counts.iter().enumerate() {
            let params = ScanParams {
                count: level_count,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            };
            let offset = scan_stride as usize * level;
            scan_params_bytes[offset..offset + size_of::<ScanParams>()]
                .copy_from_slice(bytemuck::bytes_of(&params));
        }
        let scan_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-scan-params"),
            contents: &scan_params_bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
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
        let scan_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-scan-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/gpu_prefix_scan.wgsl").into(),
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
        let radix_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-radix-bgl"),
            entries: &[
                storage_entry(4, true),
                storage_entry(5, true),
                storage_entry(6, false),
                storage_entry(7, false),
                uniform_entry(8, true, NonZeroU64::new(size_of::<PassParams>() as u64)),
            ],
        });
        let scan_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-scan-bgl"),
            entries: &[
                storage_entry(0, false),
                storage_entry(1, false),
                uniform_entry(2, true, NonZeroU64::new(size_of::<ScanParams>() as u64)),
            ],
        });
        let keygen_pipeline_layout = pipeline_layout(
            device,
            "gsplat-direct-gpu-order-keygen-layout",
            &keygen_layout,
        );
        let radix_pipeline_layout = pipeline_layout(
            device,
            "gsplat-direct-gpu-order-radix-layout",
            &radix_layout,
        );
        let scan_pipeline_layout =
            pipeline_layout(device, "gsplat-direct-gpu-order-scan-layout", &scan_layout);
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
        let scatter_pipeline = compute_pipeline(
            device,
            &shader,
            &radix_pipeline_layout,
            "scatter",
            "gsplat-direct-gpu-order-scatter-pipeline",
        );
        let scan_pipeline = compute_pipeline(
            device,
            &scan_shader,
            &scan_pipeline_layout,
            "scan_blocks",
            "gsplat-direct-gpu-order-scan-pipeline",
        );
        let add_offsets_pipeline = compute_pipeline(
            device,
            &scan_shader,
            &scan_pipeline_layout,
            "add_block_offsets",
            "gsplat-direct-gpu-order-add-offsets-pipeline",
        );
        let keygen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-keygen-bg"),
            layout: &keygen_layout,
            entries: &[
                entire_buffer_entry(0, source_buffer),
                entire_buffer_entry(1, render_params_buffer),
                entire_buffer_entry(2, &keys_a),
                entire_buffer_entry(3, &ids_a),
                entire_buffer_entry(9, &indirect_args),
            ],
        });
        let radix_a_to_b_keys = radix_bind_group(
            device,
            &radix_layout,
            "gsplat-direct-gpu-order-a-to-b-keys-bg",
            &keys_a,
            &keys_a,
            &keys_b,
            &block_prefix,
            &pass_params,
        );
        let radix_a_to_b_ids = radix_bind_group(
            device,
            &radix_layout,
            "gsplat-direct-gpu-order-a-to-b-ids-bg",
            &keys_a,
            &ids_a,
            &ids_b,
            &block_prefix,
            &pass_params,
        );
        let radix_b_to_a_keys = radix_bind_group(
            device,
            &radix_layout,
            "gsplat-direct-gpu-order-b-to-a-keys-bg",
            &keys_b,
            &keys_b,
            &keys_a,
            &block_prefix,
            &pass_params,
        );
        let radix_b_to_a_ids = radix_bind_group(
            device,
            &radix_layout,
            "gsplat-direct-gpu-order-b-to-a-ids-bg",
            &keys_b,
            &ids_b,
            &ids_a,
            &block_prefix,
            &pass_params,
        );
        let resident_radix8 = if resident_visible_compaction_enabled {
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
            let resident_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: wgpu_label("gsplat-resident-visible-gpu-order-radix8-shader"),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!("../shaders/resident_gpu_order_visible_radix8.wgsl").into(),
                ),
            });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: wgpu_label("gsplat-resident-visible-gpu-order-radix8-bgl"),
                entries: &[
                    storage_entry(4, true),
                    storage_entry(5, true),
                    storage_entry(6, false),
                    storage_entry(7, false),
                    uniform_entry(8, true, NonZeroU64::new(size_of::<PassParams>() as u64)),
                    storage_entry(13, false),
                    storage_entry(14, true),
                ],
            });
            let resident_pipeline_layout = pipeline_layout(
                device,
                "gsplat-resident-visible-gpu-order-radix8-layout",
                &layout,
            );
            let histogram_pipeline = compute_pipeline(
                device,
                &resident_shader,
                &resident_pipeline_layout,
                "histogram_visible_radix8",
                "gsplat-resident-visible-gpu-order-radix8-histogram-pipeline",
            );
            let scatter_pipeline = compute_pipeline(
                device,
                &resident_shader,
                &resident_pipeline_layout,
                "scatter_visible_keys_ids_radix8",
                "gsplat-resident-visible-gpu-order-radix8-scatter-pipeline",
            );
            let prefix_clear_pipeline = compute_pipeline(
                device,
                &resident_shader,
                &resident_pipeline_layout,
                "clear_prefix_radix8",
                "gsplat-resident-visible-gpu-order-radix8-prefix-clear-pipeline",
            );

            let group_offset_count = group_count + 1;
            let group_offsets = storage_buffer(
                device,
                "gsplat-resident-visible-group-offsets",
                u64::from(group_offset_count) * size_of::<u32>() as u64,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            );
            let compact_level_counts = scan_level_counts(group_offset_count);
            let compact_scan_sums = compact_level_counts
                .iter()
                .map(|&(_, groups)| {
                    storage_buffer(
                        device,
                        "gsplat-resident-visible-scan-sums",
                        u64::from(groups) * size_of::<u32>() as u64,
                        wgpu::BufferUsages::STORAGE,
                    )
                })
                .collect::<Vec<_>>();
            let mut compact_scan_params_bytes =
                vec![0_u8; scan_stride as usize * compact_level_counts.len()];
            for (level, &(level_count, _)) in compact_level_counts.iter().enumerate() {
                let params = ScanParams {
                    count: level_count,
                    _pad0: 0,
                    _pad1: 0,
                    _pad2: 0,
                };
                let offset = scan_stride as usize * level;
                compact_scan_params_bytes[offset..offset + size_of::<ScanParams>()]
                    .copy_from_slice(bytemuck::bytes_of(&params));
            }
            let compact_scan_params =
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: wgpu_label("gsplat-resident-visible-scan-params"),
                    contents: &compact_scan_params_bytes,
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let compact_scan_levels = compact_level_counts
                .iter()
                .enumerate()
                .map(|(level, &(_, groups))| {
                    let data = if level == 0 {
                        &group_offsets
                    } else {
                        &compact_scan_sums[level - 1]
                    };
                    ScanLevel {
                        bind_group: scan_bind_group(
                            device,
                            &scan_layout,
                            data,
                            &compact_scan_sums[level],
                            &compact_scan_params,
                        ),
                        dispatch: Dispatch2d::for_workgroups(groups, dispatch_limit)
                            .expect("visible compaction scan limits were validated"),
                        dynamic_offset: level as u32 * scan_stride,
                    }
                })
                .collect::<Vec<_>>();

            let compact_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: wgpu_label("gsplat-resident-visible-compaction-shader"),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!("../shaders/resident_gpu_order_compact.wgsl").into(),
                ),
            });
            let compact_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
            let compact_pipeline_layout = pipeline_layout(
                device,
                "gsplat-resident-visible-compaction-layout",
                &compact_layout,
            );
            let compact_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: wgpu_label("gsplat-resident-visible-compaction-bg"),
                layout: &compact_layout,
                entries: &[
                    entire_buffer_entry(0, source_buffer),
                    entire_buffer_entry(1, render_params_buffer),
                    entire_buffer_entry(2, &keys_b),
                    entire_buffer_entry(3, &group_offsets),
                    entire_buffer_entry(4, &keys_a),
                    entire_buffer_entry(5, &ids_a),
                    entire_buffer_entry(6, &control),
                    entire_buffer_entry(9, &indirect_args),
                ],
            });
            let visible_compaction = ResidentVisibleCompaction {
                keygen_pipeline: compute_pipeline(
                    device,
                    &compact_shader,
                    &compact_pipeline_layout,
                    "generate_keys_and_group_counts",
                    "gsplat-resident-visible-keygen-pipeline",
                ),
                compact_pipeline: compute_pipeline(
                    device,
                    &compact_shader,
                    &compact_pipeline_layout,
                    "compact_visible_keys_ids",
                    "gsplat-resident-visible-compact-pipeline",
                ),
                finalize_pipeline: compute_pipeline(
                    device,
                    &compact_shader,
                    &compact_pipeline_layout,
                    "finalize_visible_compaction",
                    "gsplat-resident-visible-finalize-pipeline",
                ),
                prefix_clear_pipeline,
                bind_group: compact_bind_group,
                group_offsets,
                _scan_sums: compact_scan_sums,
                _scan_params: compact_scan_params,
                scan_levels: compact_scan_levels,
                dispatch: keygen_dispatch,
                prefix_clear_dispatch: Dispatch2d::for_workgroups(
                    workgroup_count(prefix_count),
                    dispatch_limit,
                )
                .expect("visible radix prefix-clear limits were validated"),
                control,
            };
            Some(ResidentRadix8 {
                histogram_pipeline,
                scatter_pipeline,
                a_to_b: fused_scatter_bind_group_with_control(
                    device,
                    &layout,
                    "gsplat-resident-visible-gpu-order-radix8-a-to-b-bg",
                    &keys_a,
                    &keys_b,
                    &ids_a,
                    &ids_b,
                    &block_prefix,
                    &pass_params,
                    &visible_compaction.control,
                ),
                b_to_a: fused_scatter_bind_group_with_control(
                    device,
                    &layout,
                    "gsplat-resident-visible-gpu-order-radix8-b-to-a-bg",
                    &keys_b,
                    &keys_a,
                    &ids_b,
                    &ids_a,
                    &block_prefix,
                    &pass_params,
                    &visible_compaction.control,
                ),
                visible_compaction: Some(visible_compaction),
            })
        } else if resident_radix8_enabled {
            let resident_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: wgpu_label("gsplat-resident-gpu-order-radix8-shader"),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!("../shaders/resident_gpu_order_radix8.wgsl").into(),
                ),
            });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: wgpu_label("gsplat-resident-gpu-order-radix8-bgl"),
                entries: &[
                    storage_entry(4, true),
                    storage_entry(5, true),
                    storage_entry(6, false),
                    storage_entry(7, false),
                    uniform_entry(8, true, NonZeroU64::new(size_of::<PassParams>() as u64)),
                    storage_entry(13, false),
                ],
            });
            let pipeline_layout =
                pipeline_layout(device, "gsplat-resident-gpu-order-radix8-layout", &layout);
            let histogram_pipeline = compute_pipeline(
                device,
                &resident_shader,
                &pipeline_layout,
                "histogram_radix8",
                "gsplat-resident-gpu-order-radix8-histogram-pipeline",
            );
            let scatter_pipeline = compute_pipeline(
                device,
                &resident_shader,
                &pipeline_layout,
                "scatter_keys_ids_radix8",
                "gsplat-resident-gpu-order-radix8-scatter-pipeline",
            );
            Some(ResidentRadix8 {
                histogram_pipeline,
                scatter_pipeline,
                a_to_b: fused_scatter_bind_group(
                    device,
                    &layout,
                    "gsplat-resident-gpu-order-radix8-a-to-b-bg",
                    &keys_a,
                    &keys_b,
                    &ids_a,
                    &ids_b,
                    &block_prefix,
                    &pass_params,
                ),
                b_to_a: fused_scatter_bind_group(
                    device,
                    &layout,
                    "gsplat-resident-gpu-order-radix8-b-to-a-bg",
                    &keys_b,
                    &keys_a,
                    &ids_b,
                    &ids_a,
                    &block_prefix,
                    &pass_params,
                ),
                visible_compaction: None,
            })
        } else {
            None
        };
        let scan_levels = level_counts
            .iter()
            .enumerate()
            .map(|(level, &(_, groups))| {
                let data = if level == 0 {
                    &block_prefix
                } else {
                    &scan_sums[level - 1]
                };
                ScanLevel {
                    bind_group: scan_bind_group(
                        device,
                        &scan_layout,
                        data,
                        &scan_sums[level],
                        &scan_params,
                    ),
                    dispatch: Dispatch2d::for_workgroups(groups, dispatch_limit)
                        .expect("scan dispatch limits were validated"),
                    dynamic_offset: level as u32 * scan_stride,
                }
            })
            .collect();
        let compatibility_output = if compatibility_output {
            let pairs = storage_buffer(
                device,
                "gsplat-direct-gpu-order-compatibility-pairs",
                u64::from(allocation_count) * size_of::<GpuSortPair>() as u64,
                data_usage,
            );
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: wgpu_label("gsplat-direct-gpu-order-pack-bgl"),
                entries: &[
                    uniform_entry(8, true, NonZeroU64::new(size_of::<PassParams>() as u64)),
                    storage_entry(10, true),
                    storage_entry(11, true),
                    storage_entry(12, false),
                ],
            });
            let pipeline_layout =
                pipeline_layout(device, "gsplat-direct-gpu-order-pack-layout", &layout);
            let pipeline = compute_pipeline(
                device,
                &shader,
                &pipeline_layout,
                "pack_pairs",
                "gsplat-direct-gpu-order-pack-pipeline",
            );
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: wgpu_label("gsplat-direct-gpu-order-pack-bg"),
                layout: &layout,
                entries: &[
                    sized_buffer_entry(8, &pass_params, size_of::<PassParams>() as u64),
                    entire_buffer_entry(10, &keys_a),
                    entire_buffer_entry(11, &ids_a),
                    entire_buffer_entry(12, &pairs),
                ],
            });
            Some(CompatibilityOutput {
                pairs,
                bind_group,
                pipeline,
            })
        } else {
            None
        };

        Ok(Self {
            count,
            keygen_dispatch,
            radix_dispatch,
            keys_a,
            _keys_b: keys_b,
            ids_a,
            _ids_b: ids_b,
            _block_prefix: block_prefix,
            _scan_sums: scan_sums,
            _pass_params: pass_params,
            _scan_params: scan_params,
            indirect_args,
            keygen_bind_group,
            radix_a_to_b_keys,
            radix_a_to_b_ids,
            radix_b_to_a_keys,
            radix_b_to_a_ids,
            resident_radix8,
            scan_levels,
            compatibility_output,
            keygen_pipeline,
            histogram_pipeline,
            scan_pipeline,
            add_offsets_pipeline,
            scatter_pipeline,
            pass_stride,
            radix_passes,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn final_pairs(&self) -> &wgpu::Buffer {
        &self
            .compatibility_output
            .as_ref()
            .expect("final_pairs requires DirectGpuOrder::new")
            .pairs
    }

    /// SoA output for the renderer integration that removes the compatibility
    /// pair pack. Both four byte passes and eight nibble passes leave the final
    /// IDs in buffer A.
    #[allow(dead_code)]
    pub(crate) fn final_ids(&self) -> &wgpu::Buffer {
        &self.ids_a
    }

    pub(crate) fn indirect_args(&self) -> &wgpu::Buffer {
        &self.indirect_args
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.count == 0
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
        if let Some(compaction) = self
            .resident_radix8
            .as_ref()
            .and_then(|resident| resident.visible_compaction.as_ref())
        {
            let sentinel_offset = u64::from(workgroup_count(self.count)) * size_of::<u32>() as u64;
            encoder.clear_buffer(
                &compaction.group_offsets,
                sentinel_offset,
                Some(size_of::<u32>() as u64),
            );
            Self::encode_stage(
                encoder,
                &compaction.keygen_pipeline,
                &compaction.bind_group,
                &[],
                compaction.dispatch,
                "gsplat-resident-visible-keygen-pass",
                timestamps.map(|range| wgpu::ComputePassTimestampWrites {
                    query_set: range.query_set,
                    beginning_of_pass_write_index: Some(range.keygen_begin_index),
                    end_of_pass_write_index: None,
                }),
            );
            for level in &compaction.scan_levels {
                Self::encode_stage(
                    encoder,
                    &self.scan_pipeline,
                    &level.bind_group,
                    &[level.dynamic_offset],
                    level.dispatch,
                    "gsplat-resident-visible-scan-pass",
                    None,
                );
            }
            for level in compaction.scan_levels[..compaction.scan_levels.len() - 1]
                .iter()
                .rev()
            {
                Self::encode_stage(
                    encoder,
                    &self.add_offsets_pipeline,
                    &level.bind_group,
                    &[level.dynamic_offset],
                    level.dispatch,
                    "gsplat-resident-visible-add-offsets-pass",
                    None,
                );
            }
            Self::encode_stage(
                encoder,
                &compaction.compact_pipeline,
                &compaction.bind_group,
                &[],
                compaction.dispatch,
                "gsplat-resident-visible-compact-pass",
                None,
            );
            Self::encode_stage(
                encoder,
                &compaction.finalize_pipeline,
                &compaction.bind_group,
                &[],
                Dispatch2d { x: 1, y: 1 },
                "gsplat-resident-visible-finalize-pass",
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
        if self.count == 0 {
            return;
        }
        for radix_pass in 0..self.radix_passes {
            let (keys_bind_group, ids_bind_group) = if radix_pass % 2 == 0 {
                (&self.radix_a_to_b_keys, &self.radix_a_to_b_ids)
            } else {
                (&self.radix_b_to_a_keys, &self.radix_b_to_a_ids)
            };
            let dynamic_offset = radix_pass * self.pass_stride;
            let radix_begin_timestamp = if radix_pass == 0 {
                timestamps.map(|range| wgpu::ComputePassTimestampWrites {
                    query_set: range.query_set,
                    beginning_of_pass_write_index: Some(range.radix_begin_index),
                    end_of_pass_write_index: None,
                })
            } else {
                None
            };
            if let Some(resident) = &self.resident_radix8 {
                let bind_group = if radix_pass % 2 == 0 {
                    &resident.a_to_b
                } else {
                    &resident.b_to_a
                };
                if let Some(compaction) = &resident.visible_compaction {
                    Self::encode_stage(
                        encoder,
                        &compaction.prefix_clear_pipeline,
                        bind_group,
                        &[dynamic_offset],
                        compaction.prefix_clear_dispatch,
                        "gsplat-resident-visible-radix8-prefix-clear-pass",
                        radix_begin_timestamp,
                    );
                    Self::encode_stage_indirect(
                        encoder,
                        &resident.histogram_pipeline,
                        bind_group,
                        &[dynamic_offset],
                        &compaction.control,
                        RESIDENT_CONTROL_INDIRECT_OFFSET,
                        "gsplat-resident-visible-radix8-histogram-pass",
                        None,
                    );
                } else {
                    Self::encode_stage(
                        encoder,
                        &resident.histogram_pipeline,
                        bind_group,
                        &[dynamic_offset],
                        self.radix_dispatch,
                        "gsplat-resident-gpu-order-radix8-histogram-pass",
                        radix_begin_timestamp,
                    );
                }
            } else {
                Self::encode_stage(
                    encoder,
                    &self.histogram_pipeline,
                    keys_bind_group,
                    &[dynamic_offset],
                    self.radix_dispatch,
                    "gsplat-direct-gpu-order-histogram-pass",
                    radix_begin_timestamp,
                );
            }
            for level in &self.scan_levels {
                Self::encode_stage(
                    encoder,
                    &self.scan_pipeline,
                    &level.bind_group,
                    &[level.dynamic_offset],
                    level.dispatch,
                    "gsplat-direct-gpu-order-scan-pass",
                    None,
                );
            }
            for level in self.scan_levels[..self.scan_levels.len() - 1].iter().rev() {
                Self::encode_stage(
                    encoder,
                    &self.add_offsets_pipeline,
                    &level.bind_group,
                    &[level.dynamic_offset],
                    level.dispatch,
                    "gsplat-direct-gpu-order-add-offsets-pass",
                    None,
                );
            }
            let final_timestamp = if radix_pass + 1 == self.radix_passes {
                timestamps.map(|range| wgpu::ComputePassTimestampWrites {
                    query_set: range.query_set,
                    beginning_of_pass_write_index: None,
                    end_of_pass_write_index: Some(range.radix_end_index),
                })
            } else {
                None
            };
            if let Some(resident) = &self.resident_radix8 {
                let bind_group = if radix_pass % 2 == 0 {
                    &resident.a_to_b
                } else {
                    &resident.b_to_a
                };
                if let Some(compaction) = &resident.visible_compaction {
                    Self::encode_stage_indirect(
                        encoder,
                        &resident.scatter_pipeline,
                        bind_group,
                        &[dynamic_offset],
                        &compaction.control,
                        RESIDENT_CONTROL_INDIRECT_OFFSET,
                        "gsplat-resident-visible-radix8-scatter-pass",
                        final_timestamp,
                    );
                } else {
                    Self::encode_stage(
                        encoder,
                        &resident.scatter_pipeline,
                        bind_group,
                        &[dynamic_offset],
                        self.radix_dispatch,
                        "gsplat-resident-gpu-order-radix8-scatter-pass",
                        final_timestamp,
                    );
                }
            } else {
                Self::encode_stage(
                    encoder,
                    &self.scatter_pipeline,
                    keys_bind_group,
                    &[dynamic_offset],
                    self.radix_dispatch,
                    "gsplat-direct-gpu-order-scatter-pass",
                    None,
                );
                // Direct retains this exact fallback on devices exposing only
                // the four-binding compute baseline.
                Self::encode_stage(
                    encoder,
                    &self.scatter_pipeline,
                    ids_bind_group,
                    &[dynamic_offset],
                    self.radix_dispatch,
                    "gsplat-direct-gpu-order-scatter-ids-pass",
                    final_timestamp,
                );
            }
        }
        if let Some(output) = &self.compatibility_output {
            Self::encode_stage(
                encoder,
                &output.pipeline,
                &output.bind_group,
                &[0],
                self.radix_dispatch,
                "gsplat-direct-gpu-order-pack-pass",
                None,
            );
        }
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

    #[allow(clippy::too_many_arguments)]
    fn encode_stage_indirect(
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        bind_group: &wgpu::BindGroup,
        dynamic_offsets: &[u32],
        indirect_buffer: &wgpu::Buffer,
        indirect_offset: u64,
        label: &'static str,
        timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: wgpu_label(label),
            timestamp_writes,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, dynamic_offsets);
        pass.dispatch_workgroups_indirect(indirect_buffer, indirect_offset);
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

fn sized_buffer_entry<'a>(
    binding: u32,
    buffer: &'a wgpu::Buffer,
    size: u64,
) -> wgpu::BindGroupEntry<'a> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer,
            offset: 0,
            size: NonZeroU64::new(size),
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn radix_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &'static str,
    keys_src: &wgpu::Buffer,
    payload_src: &wgpu::Buffer,
    payload_dst: &wgpu::Buffer,
    prefix: &wgpu::Buffer,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label(label),
        layout,
        entries: &[
            entire_buffer_entry(4, keys_src),
            entire_buffer_entry(5, payload_src),
            entire_buffer_entry(6, payload_dst),
            entire_buffer_entry(7, prefix),
            sized_buffer_entry(8, params, size_of::<PassParams>() as u64),
        ],
    })
}

#[allow(clippy::too_many_arguments)]
fn fused_scatter_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &'static str,
    keys_src: &wgpu::Buffer,
    keys_dst: &wgpu::Buffer,
    ids_src: &wgpu::Buffer,
    ids_dst: &wgpu::Buffer,
    prefix: &wgpu::Buffer,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label(label),
        layout,
        entries: &[
            entire_buffer_entry(4, keys_src),
            entire_buffer_entry(5, ids_src),
            entire_buffer_entry(6, ids_dst),
            entire_buffer_entry(7, prefix),
            sized_buffer_entry(8, params, size_of::<PassParams>() as u64),
            entire_buffer_entry(13, keys_dst),
        ],
    })
}

#[allow(clippy::too_many_arguments)]
fn fused_scatter_bind_group_with_control(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &'static str,
    keys_src: &wgpu::Buffer,
    keys_dst: &wgpu::Buffer,
    ids_src: &wgpu::Buffer,
    ids_dst: &wgpu::Buffer,
    prefix: &wgpu::Buffer,
    params: &wgpu::Buffer,
    control: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label(label),
        layout,
        entries: &[
            entire_buffer_entry(4, keys_src),
            entire_buffer_entry(5, ids_src),
            entire_buffer_entry(6, ids_dst),
            entire_buffer_entry(7, prefix),
            sized_buffer_entry(8, params, size_of::<PassParams>() as u64),
            entire_buffer_entry(13, keys_dst),
            entire_buffer_entry(14, control),
        ],
    })
}

fn scan_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    data: &wgpu::Buffer,
    sums: &wgpu::Buffer,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label("gsplat-direct-gpu-order-scan-bg"),
        layout,
        entries: &[
            entire_buffer_entry(0, data),
            entire_buffer_entry(1, sums),
            sized_buffer_entry(2, params, size_of::<ScanParams>() as u64),
        ],
    })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::{path::PathBuf, sync::mpsc};

    use super::*;
    use crate::{GpuSurfaceSourceElem, make_surface_render_params};
    use gsplat_core::camera_trace::CameraTrace;
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
        queue.write_buffer(&order.keys_a, 0, bytemuck::cast_slice(&keys));
        queue.write_buffer(&order.ids_a, 0, bytemuck::cast_slice(&ids));

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
        assert!(order.resident_radix8.is_some());
        assert_eq!(order.radix_passes, RESIDENT_RADIX_PASSES);
        queue.write_buffer(&order.keys_a, 0, bytemuck::cast_slice(&keys));
        queue.write_buffer(&order.ids_a, 0, bytemuck::cast_slice(&ids));

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
            .resident_radix8
            .as_ref()
            .and_then(|resident| resident.visible_compaction.as_ref())
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
            &compaction.control,
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
        assert!(
            order
                .resident_radix8
                .as_ref()
                .and_then(|resident| resident.visible_compaction.as_ref())
                .is_some()
        );
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
        assert!(order.resident_radix8.is_some());
        assert_eq!(order.radix_passes, RESIDENT_RADIX_PASSES);
        assert_eq!(
            order._block_prefix.size(),
            u64::from(workgroup_count(count) * RESIDENT_RADIX) * size_of::<u32>() as u64
        );
        queue.write_buffer(&order.keys_a, 0, bytemuck::cast_slice(&keys));
        queue.write_buffer(&order.ids_a, 0, bytemuck::cast_slice(&ids));

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
        assert!(direct.resident_radix8.is_none());
        assert_eq!(direct.radix_passes, DIRECT_RADIX_PASSES);
        assert_eq!(
            direct._block_prefix.size(),
            u64::from(DIRECT_RADIX) * size_of::<u32>() as u64
        );

        let soa_fallback = DirectGpuOrder::new_soa(&device, &source, &params, 1, 0)
            .expect("SoA low-binding fallback must initialize");
        assert!(soa_fallback.resident_radix8.is_none());
        assert_eq!(soa_fallback.radix_passes, DIRECT_RADIX_PASSES);

        let resident_fallback = DirectGpuOrder::new_resident_soa(&device, &source, &params, 1, 0)
            .expect("Resident low-binding fallback must initialize");
        assert!(resident_fallback.resident_radix8.is_none());
        assert_eq!(resident_fallback.radix_passes, DIRECT_RADIX_PASSES);
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
        assert!(order.compatibility_output.is_none());
        assert!(order.resident_radix8.is_none());
        assert_eq!(order.radix_passes, DIRECT_RADIX_PASSES);
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
