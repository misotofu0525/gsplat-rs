//! Cohesive stable radix mechanics for portable and qualified GPU profiles.
//!
//! This module intentionally knows nothing about projection, scene storage,
//! presentation, or target-selection policy. It owns descending full32
//! profiles for external-prefix portable nibble-aligned LSD passes, Direct and the
//! four-binding fallback 8x4-bit passes, and qualified Resident/ResidentVisible
//! 4x8-bit passes. Every profile preserves stable source-ID order. Full32
//! profiles leave final keys and IDs in the original A buffers; external-prefix
//! profiles expose the actual parity-selected final buffers. The external-prefix profile
//! consumes `{key, source_id}[0..C)` plus one [`ExternalPrefixControl`];
//! capacity is a reusable high-water mark and C may vary from zero through
//! capacity every submission.

use std::{mem::size_of, num::NonZeroU64};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::scan::{
    Dispatch2d, GpuPrefixScan, GpuPrefixScanGraph, GpuPrefixScanGraphProfile, GpuPrefixScanKernel,
    GpuPrefixScanKernelProfile, GpuPrefixScanPassLabels, SCAN_ITEMS_PER_GROUP, SCAN_WORKGROUP_SIZE,
    WORD_BYTES, create_compute_pipeline, entry, scan_level_counts, scan_sum_plan, storage_buffer,
    storage_layout, uniform_layout,
};
use crate::gpu_error::ResidentGpuError;
use crate::{GpuSortPair, wgpu_label};

pub(crate) const EXTERNAL_RADIX_WORKGROUP_SIZE: u32 = 128;
pub(crate) const EXTERNAL_RADIX_ITEMS_PER_THREAD: u32 = 8;
pub(crate) const EXTERNAL_RADIX_TILE_SIZE: u32 =
    EXTERNAL_RADIX_WORKGROUP_SIZE * EXTERNAL_RADIX_ITEMS_PER_THREAD;
pub(crate) const EXTERNAL_RADIX: u32 = 16;
pub(crate) const EXTERNAL_RADIX_PASSES: u32 = 8;
const EXTERNAL_RADIX_STORAGE_BINDINGS: u32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExternalPrefixRadixProfile {
    first_shift: u32,
    pass_count: u32,
}

impl ExternalPrefixRadixProfile {
    pub(crate) const EXACT_FULL32: Self = Self {
        first_shift: 0,
        pass_count: EXTERNAL_RADIX_PASSES,
    };
    pub(crate) const CANDIDATE_STABLE20: Self = Self {
        first_shift: 12,
        pass_count: 5,
    };

    const fn new(first_shift: u32, pass_count: u32) -> Self {
        Self {
            first_shift,
            pass_count,
        }
    }

    fn validate(self) -> Result<Self, ResidentGpuError> {
        let covered_bits = self
            .pass_count
            .checked_mul(4)
            .and_then(|bits| self.first_shift.checked_add(bits));
        if self.pass_count == 0
            || !self.first_shift.is_multiple_of(4)
            || covered_bits.is_none_or(|bits| bits > u32::BITS)
        {
            return Err(ResidentGpuError::GpuOrderInitialization(
                "external-prefix radix profile must cover a non-empty nibble-aligned u32 range"
                    .into(),
            ));
        }
        Ok(self)
    }

    const fn shift_for_pass(self, pass: u32) -> u32 {
        self.first_shift + pass * 4
    }

    const fn final_is_a(self) -> bool {
        self.pass_count.is_multiple_of(2)
    }
}

pub(crate) const FULL32_WORKGROUP_SIZE: u32 = 128;
pub(crate) const FULL32_ITEMS_PER_THREAD: u32 = 8;
pub(crate) const FULL32_TILE_SIZE: u32 = FULL32_WORKGROUP_SIZE * FULL32_ITEMS_PER_THREAD;
pub(crate) const FULL32_DIRECT_RADIX: u32 = 16;
pub(crate) const FULL32_DIRECT_PASSES: u32 = 8;
pub(crate) const FULL32_RESIDENT_RADIX: u32 = 256;
pub(crate) const FULL32_RESIDENT_PASSES: u32 = 4;
pub(crate) const FULL32_RESIDENT_STORAGE_BINDINGS: u32 = 5;
// histogram_counts (256 atomics), digit_masks (256 * 4 atomics), and
// digit_prior (256 u32s). Keeping the conservative module-wide total here
// also covers implementations that account all workgroup globals together.
pub(crate) const FULL32_RESIDENT_WORKGROUP_STORAGE_BYTES: u32 = (256 + 1_024 + 256) * 4;
pub(crate) const FULL32_SCAN_WORKGROUP_SIZE: u32 = SCAN_WORKGROUP_SIZE;
pub(crate) const FULL32_SCAN_WORKGROUP_STORAGE_BYTES: u32 =
    SCAN_ITEMS_PER_GROUP * size_of::<u32>() as u32;

pub(crate) fn full32_workgroup_count(count: u32) -> u32 {
    count.div_ceil(FULL32_TILE_SIZE).max(1)
}

pub(crate) fn full32_scan_level_counts(count: u32) -> Vec<(u32, u32)> {
    scan_level_counts(count)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct ExternalPrefixControl {
    pub(crate) count: u32,
    pub(crate) active_group_count: u32,
    pub(crate) dispatch_x: u32,
    pub(crate) dispatch_y: u32,
    pub(crate) dispatch_z: u32,
    pub(crate) dispatch_limit: u32,
    pub(crate) capacity_count: u32,
    pub(crate) _pad0: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct RadixPassParams {
    shift: u32,
    capacity_count: u32,
    capacity_group_count: u32,
    _pad: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExternalPrefixRadixBytePlan {
    pub(crate) key_ping_pong: u64,
    pub(crate) source_id_ping_pong: u64,
    pub(crate) radix_prefix: u64,
    pub(crate) scan_sums: u64,
    pub(crate) largest_scan_sum: u64,
    pub(crate) scan_params: u64,
    pub(crate) pass_params: u64,
    pub(crate) control: u64,
    pub(crate) total_static: u64,
}

impl ExternalPrefixRadixBytePlan {
    pub(crate) fn for_capacity(
        capacity: u32,
        limits: &wgpu::Limits,
    ) -> Result<Self, ResidentGpuError> {
        Self::for_capacity_with_profile(capacity, limits, ExternalPrefixRadixProfile::EXACT_FULL32)
    }

    fn for_capacity_with_profile(
        capacity: u32,
        limits: &wgpu::Limits,
        profile: ExternalPrefixRadixProfile,
    ) -> Result<Self, ResidentGpuError> {
        let profile = profile.validate()?;
        let one_plane = u64::from(capacity)
            .checked_mul(WORD_BYTES)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?
            .max(WORD_BYTES);
        let key_ping_pong = one_plane
            .checked_mul(2)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let source_id_ping_pong = key_ping_pong;
        let capacity_groups = capacity.div_ceil(EXTERNAL_RADIX_TILE_SIZE).max(1);
        let prefix_count = capacity_groups
            .checked_mul(EXTERNAL_RADIX)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let radix_prefix = u64::from(prefix_count)
            .checked_mul(WORD_BYTES)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let (scan_sums, largest_scan_sum, scan_levels) = scan_sum_plan(prefix_count)?;
        let uniform_stride = limits.min_uniform_buffer_offset_alignment.max(16);
        let scan_params = u64::from(scan_levels)
            .checked_mul(u64::from(uniform_stride))
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let pass_params = u64::from(profile.pass_count)
            .checked_mul(u64::from(uniform_stride))
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let control = size_of::<ExternalPrefixControl>() as u64;
        let total_static = [
            key_ping_pong,
            source_id_ping_pong,
            radix_prefix,
            scan_sums,
            scan_params,
            pass_params,
            control,
        ]
        .into_iter()
        .try_fold(0_u64, |total, bytes| {
            total
                .checked_add(bytes)
                .ok_or(ResidentGpuError::AddressSpaceExceeded)
        })?;
        Ok(Self {
            key_ping_pong,
            source_id_ping_pong,
            radix_prefix,
            scan_sums,
            largest_scan_sum,
            scan_params,
            pass_params,
            control,
            total_static,
        })
    }

    pub(crate) fn validate_limits(self, limits: &wgpu::Limits) -> Result<Self, ResidentGpuError> {
        if limits.max_storage_buffers_per_shader_stage < EXTERNAL_RADIX_STORAGE_BINDINGS {
            return Err(ResidentGpuError::StorageBindingCountUnsupported(
                limits.max_storage_buffers_per_shader_stage,
            ));
        }
        if limits.max_compute_invocations_per_workgroup < SCAN_WORKGROUP_SIZE
            || limits.max_compute_workgroup_size_x < SCAN_WORKGROUP_SIZE
            || limits.max_compute_workgroup_storage_size
                < SCAN_ITEMS_PER_GROUP * size_of::<u32>() as u32
        {
            return Err(ResidentGpuError::GpuOrderInitialization(
                "external-prefix radix requires the portable 256-lane/512-word scan floor".into(),
            ));
        }
        let binding_limit =
            u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size);
        for (resource, bytes) in [
            ("external radix key plane", self.key_ping_pong / 2),
            (
                "external radix source-ID plane",
                self.source_id_ping_pong / 2,
            ),
            ("external radix prefix", self.radix_prefix),
            ("external radix scan sums", self.largest_scan_sum),
            ("external radix control", self.control),
        ] {
            if bytes > binding_limit {
                return Err(ResidentGpuError::BindingLimitExceeded {
                    resource,
                    required_bytes: bytes,
                    limit_bytes: binding_limit,
                });
            }
        }
        for (resource, bytes) in [
            ("external radix scan params", self.scan_params),
            ("external radix pass params", self.pass_params),
        ] {
            if bytes > limits.max_buffer_size {
                return Err(ResidentGpuError::BindingLimitExceeded {
                    resource,
                    required_bytes: bytes,
                    limit_bytes: limits.max_buffer_size,
                });
            }
        }
        Ok(self)
    }
}

pub(crate) struct ExternalPrefixRadix {
    capacity: u32,
    capacity_groups: u32,
    dispatch_limit: u32,
    pass_stride: u32,
    profile: ExternalPrefixRadixProfile,
    keys_a: wgpu::Buffer,
    keys_b: wgpu::Buffer,
    ids_a: wgpu::Buffer,
    ids_b: wgpu::Buffer,
    prefix: wgpu::Buffer,
    control: wgpu::Buffer,
    scan: GpuPrefixScan,
    histogram_pipeline: wgpu::ComputePipeline,
    scatter_pipeline: wgpu::ComputePipeline,
    bind_groups: [wgpu::BindGroup; 2],
    _pass_params: wgpu::Buffer,
    _byte_plan: ExternalPrefixRadixBytePlan,
}

impl ExternalPrefixRadix {
    pub(crate) fn new(device: &wgpu::Device, capacity: u32) -> Result<Self, ResidentGpuError> {
        Self::new_with_profile(device, capacity, ExternalPrefixRadixProfile::EXACT_FULL32)
    }

    pub(crate) fn new_with_profile(
        device: &wgpu::Device,
        capacity: u32,
        profile: ExternalPrefixRadixProfile,
    ) -> Result<Self, ResidentGpuError> {
        Self::new_with_dispatch_limit(
            device,
            capacity,
            device.limits().max_compute_workgroups_per_dimension,
            profile,
        )
    }

    fn new_with_dispatch_limit(
        device: &wgpu::Device,
        capacity: u32,
        dispatch_limit: u32,
        profile: ExternalPrefixRadixProfile,
    ) -> Result<Self, ResidentGpuError> {
        let profile = profile.validate()?;
        let physical_limit = device.limits().max_compute_workgroups_per_dimension;
        if dispatch_limit == 0 || dispatch_limit > physical_limit {
            return Err(ResidentGpuError::DispatchLimitExceeded);
        }
        let limits = device.limits();
        let byte_plan =
            ExternalPrefixRadixBytePlan::for_capacity_with_profile(capacity, &limits, profile)?
                .validate_limits(&limits)?;
        let capacity_groups = capacity.div_ceil(EXTERNAL_RADIX_TILE_SIZE).max(1);
        Dispatch2d::for_workgroups(capacity_groups, dispatch_limit)?;
        let one_key_plane = byte_plan.key_ping_pong / 2;
        let one_id_plane = byte_plan.source_id_ping_pong / 2;
        let keys_a = storage_buffer(
            device,
            "gsplat-external-radix-keys-a",
            one_key_plane,
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        );
        let keys_b = storage_buffer(
            device,
            "gsplat-external-radix-keys-b",
            one_key_plane,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let ids_a = storage_buffer(
            device,
            "gsplat-external-radix-ids-a",
            one_id_plane,
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        );
        let ids_b = storage_buffer(
            device,
            "gsplat-external-radix-ids-b",
            one_id_plane,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let prefix = storage_buffer(
            device,
            "gsplat-external-radix-prefix",
            byte_plan.radix_prefix,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let control = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-external-radix-control"),
            contents: bytemuck::bytes_of(&ExternalPrefixControl {
                count: 0,
                active_group_count: 0,
                dispatch_x: 0,
                dispatch_y: 1,
                dispatch_z: 1,
                dispatch_limit,
                capacity_count: capacity,
                _pad0: 0,
            }),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        });

        let pass_stride = limits.min_uniform_buffer_offset_alignment.max(16);
        let mut pass_params_bytes = vec![0_u8; pass_stride as usize * profile.pass_count as usize];
        for pass in 0..profile.pass_count {
            let params = RadixPassParams {
                shift: profile.shift_for_pass(pass),
                capacity_count: capacity,
                capacity_group_count: capacity_groups,
                _pad: 0,
            };
            let offset = pass as usize * pass_stride as usize;
            pass_params_bytes[offset..offset + size_of::<RadixPassParams>()]
                .copy_from_slice(bytemuck::bytes_of(&params));
        }
        let pass_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-external-radix-pass-params"),
            contents: &pass_params_bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-external-radix-bgl"),
            entries: &[
                storage_layout_read_only(0),
                storage_layout_read_only(1),
                storage_layout(2, false),
                storage_layout(3, false),
                uniform_layout(
                    4,
                    true,
                    NonZeroU64::new(size_of::<RadixPassParams>() as u64),
                ),
                storage_layout(5, false),
                storage_layout_read_only(6),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: wgpu_label("gsplat-external-radix-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../shaders/external_prefix_radix.wgsl").into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: wgpu_label("gsplat-external-radix-pipeline-layout"),
            bind_group_layouts: &[&layout],
            immediate_size: 0,
        });
        let histogram_pipeline = create_compute_pipeline(
            device,
            &shader,
            &pipeline_layout,
            "histogram_dynamic",
            "gsplat-external-radix-histogram-pipeline",
        );
        let scatter_pipeline = create_compute_pipeline(
            device,
            &shader,
            &pipeline_layout,
            "scatter_dynamic",
            "gsplat-external-radix-scatter-pipeline",
        );
        let bind_groups = [
            create_radix_bind_group(
                device,
                &layout,
                &keys_a,
                &ids_a,
                &keys_b,
                &ids_b,
                &prefix,
                &pass_params,
                &control,
            ),
            create_radix_bind_group(
                device,
                &layout,
                &keys_b,
                &ids_b,
                &keys_a,
                &ids_a,
                &prefix,
                &pass_params,
                &control,
            ),
        ];
        let prefix_count = capacity_groups
            .checked_mul(EXTERNAL_RADIX)
            .ok_or(ResidentGpuError::AddressSpaceExceeded)?;
        let scan = GpuPrefixScan::new(device, &prefix, prefix_count, dispatch_limit)?;
        Ok(Self {
            capacity,
            capacity_groups,
            dispatch_limit,
            pass_stride,
            profile,
            keys_a,
            keys_b,
            ids_a,
            ids_b,
            prefix,
            control,
            scan,
            histogram_pipeline,
            scatter_pipeline,
            bind_groups,
            _pass_params: pass_params,
            _byte_plan: byte_plan,
        })
    }

    /// Sorts only the prefix described by the current GPU control record.
    /// The full capacity prefix is cleared before every histogram because the
    /// hierarchical scan intentionally remains a fixed-capacity operation.
    pub(crate) fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        for pass in 0..self.profile.pass_count {
            encoder.clear_buffer(&self.prefix, 0, None);
            encode_indirect_compute(
                encoder,
                &self.histogram_pipeline,
                &self.bind_groups[(pass & 1) as usize],
                &[pass * self.pass_stride],
                &self.control,
                2 * WORD_BYTES,
                "gsplat-external-radix-histogram-pass",
            );
            self.scan.encode(encoder);
            encode_indirect_compute(
                encoder,
                &self.scatter_pipeline,
                &self.bind_groups[(pass & 1) as usize],
                &[pass * self.pass_stride],
                &self.control,
                2 * WORD_BYTES,
                "gsplat-external-radix-scatter-pass",
            );
        }
    }

    pub(crate) const fn capacity(&self) -> u32 {
        self.capacity
    }

    pub(crate) const fn capacity_groups(&self) -> u32 {
        self.capacity_groups
    }

    pub(crate) const fn profile(&self) -> ExternalPrefixRadixProfile {
        self.profile
    }

    pub(crate) fn input_keys(&self) -> &wgpu::Buffer {
        &self.keys_a
    }

    pub(crate) fn input_source_ids(&self) -> &wgpu::Buffer {
        &self.ids_a
    }

    pub(crate) fn control(&self) -> &wgpu::Buffer {
        &self.control
    }

    pub(crate) fn final_keys(&self) -> &wgpu::Buffer {
        if self.profile.final_is_a() {
            &self.keys_a
        } else {
            &self.keys_b
        }
    }

    pub(crate) fn final_source_ids(&self) -> &wgpu::Buffer {
        if self.profile.final_is_a() {
            &self.ids_a
        } else {
            &self.ids_b
        }
    }

    #[cfg(test)]
    fn upload_prefix(
        &self,
        queue: &wgpu::Queue,
        keys: &[u32],
        source_ids: &[u32],
    ) -> Result<ExternalPrefixControl, ResidentGpuError> {
        if keys.len() != source_ids.len() || keys.len() > self.capacity as usize {
            return Err(ResidentGpuError::OrderCapacityExceeded);
        }
        if !keys.is_empty() {
            queue.write_buffer(self.input_keys(), 0, bytemuck::cast_slice(keys));
            queue.write_buffer(self.input_source_ids(), 0, bytemuck::cast_slice(source_ids));
        }
        let count =
            u32::try_from(keys.len()).map_err(|_| ResidentGpuError::AddressSpaceExceeded)?;
        let active_group_count = count.div_ceil(EXTERNAL_RADIX_TILE_SIZE);
        let dispatch = Dispatch2d::for_workgroups(active_group_count, self.dispatch_limit)?;
        let control = ExternalPrefixControl {
            count,
            active_group_count,
            dispatch_x: dispatch.x,
            dispatch_y: dispatch.y,
            dispatch_z: 1,
            dispatch_limit: self.dispatch_limit,
            capacity_count: self.capacity,
            _pad0: 0,
        };
        queue.write_buffer(&self.control, 0, bytemuck::bytes_of(&control));
        Ok(control)
    }
}

#[allow(clippy::too_many_arguments)]
fn create_radix_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    keys_src: &wgpu::Buffer,
    ids_src: &wgpu::Buffer,
    keys_dst: &wgpu::Buffer,
    ids_dst: &wgpu::Buffer,
    prefix: &wgpu::Buffer,
    params: &wgpu::Buffer,
    control: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label("gsplat-external-radix-bg"),
        layout,
        entries: &[
            entry(0, keys_src),
            entry(1, ids_src),
            entry(2, ids_dst),
            entry(3, prefix),
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: params,
                    offset: 0,
                    size: NonZeroU64::new(size_of::<RadixPassParams>() as u64),
                }),
            },
            entry(5, keys_dst),
            entry(6, control),
        ],
    })
}

fn storage_layout_read_only(binding: u32) -> wgpu::BindGroupLayoutEntry {
    storage_layout(binding, true)
}

fn encode_indirect_compute(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    dynamic_offsets: &[u32],
    indirect: &wgpu::Buffer,
    indirect_offset: u64,
    label: &'static str,
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: wgpu_label(label),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, dynamic_offsets);
    pass.dispatch_workgroups_indirect(indirect, indirect_offset);
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Full32PassParams {
    shift: u32,
    count: u32,
    group_count: u32,
    _pad: u32,
}

#[derive(Clone, Copy)]
pub(crate) enum StableFull32RadixProfile<'a> {
    Direct,
    Resident,
    ResidentVisible {
        control: &'a wgpu::Buffer,
        group_offsets: &'a wgpu::Buffer,
        group_offset_count: u32,
    },
}

impl StableFull32RadixProfile<'_> {
    const fn uses_resident_radix8(self) -> bool {
        !matches!(self, Self::Direct)
    }

    const fn uses_visible_control(self) -> bool {
        matches!(self, Self::ResidentVisible { .. })
    }

    const fn radix(self) -> u32 {
        if self.uses_resident_radix8() {
            FULL32_RESIDENT_RADIX
        } else {
            FULL32_DIRECT_RADIX
        }
    }

    const fn passes(self) -> u32 {
        if self.uses_resident_radix8() {
            FULL32_RESIDENT_PASSES
        } else {
            FULL32_DIRECT_PASSES
        }
    }

    const fn shift(self) -> u32 {
        if self.uses_resident_radix8() { 8 } else { 4 }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct StableFull32RadixTimestampRange<'a> {
    pub(crate) query_set: &'a wgpu::QuerySet,
    pub(crate) begin_index: u32,
    pub(crate) end_index: u32,
}

struct StableFull32DirectPipelines {
    histogram: wgpu::ComputePipeline,
    scatter: wgpu::ComputePipeline,
    a_to_b_keys: wgpu::BindGroup,
    a_to_b_ids: wgpu::BindGroup,
    b_to_a_keys: wgpu::BindGroup,
    b_to_a_ids: wgpu::BindGroup,
}

struct StableFull32ResidentPipelines {
    histogram: wgpu::ComputePipeline,
    scatter: wgpu::ComputePipeline,
    prefix_clear: Option<wgpu::ComputePipeline>,
    a_to_b: wgpu::BindGroup,
    b_to_a: wgpu::BindGroup,
    prefix_clear_dispatch: Option<Dispatch2d>,
}

#[cfg_attr(not(test), allow(dead_code))]
struct StableFull32CompatibilityOutput {
    pairs: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::ComputePipeline,
}

/// Stable full32 radix resources shared by Direct and qualified Resident
/// consumers. The owner contains no camera, visibility, target-selection, or
/// scheduling policy; callers supply the already selected graph profile and
/// encode it at the existing orchestration boundary.
pub(crate) struct StableFull32Radix {
    count: u32,
    profile_is_resident: bool,
    profile_uses_visible_control: bool,
    dispatch: Dispatch2d,
    pass_stride: u32,
    passes: u32,
    keys_a: wgpu::Buffer,
    keys_b: wgpu::Buffer,
    ids_a: wgpu::Buffer,
    _ids_b: wgpu::Buffer,
    prefix: wgpu::Buffer,
    _pass_params: wgpu::Buffer,
    scan_kernel: GpuPrefixScanKernel,
    scan: GpuPrefixScanGraph,
    visible_compaction_scan: Option<GpuPrefixScanGraph>,
    direct: StableFull32DirectPipelines,
    resident: Option<StableFull32ResidentPipelines>,
    compatibility_output: Option<StableFull32CompatibilityOutput>,
}

impl StableFull32Radix {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        device: &wgpu::Device,
        direct_shader: &wgpu::ShaderModule,
        capacity: u32,
        count: u32,
        dispatch_limit: u32,
        profile: StableFull32RadixProfile<'_>,
        compatibility_output: bool,
    ) -> Self {
        let allocation_count = capacity.max(1);
        let group_count = full32_workgroup_count(count);
        let dispatch = Dispatch2d::for_workgroups(group_count, dispatch_limit)
            .expect("radix dispatch limits were validated");
        let radix = profile.radix();
        let passes = profile.passes();
        let element_bytes = u64::from(allocation_count) * WORD_BYTES;
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
        let prefix = storage_buffer(
            device,
            "gsplat-direct-gpu-order-prefix",
            u64::from(prefix_count) * WORD_BYTES,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );

        let pass_stride = device.limits().min_uniform_buffer_offset_alignment.max(16);
        let mut pass_params_bytes = vec![0_u8; pass_stride as usize * passes as usize];
        for pass in 0..passes {
            let params = Full32PassParams {
                shift: pass * profile.shift(),
                count,
                group_count,
                _pad: 0,
            };
            let offset = pass_stride as usize * pass as usize;
            pass_params_bytes[offset..offset + size_of::<Full32PassParams>()]
                .copy_from_slice(bytemuck::bytes_of(&params));
        }
        let pass_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-pass-params"),
            contents: &pass_params_bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let direct_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: wgpu_label("gsplat-direct-gpu-order-radix-bgl"),
            entries: &[
                storage_layout(4, true),
                storage_layout(5, true),
                storage_layout(6, false),
                storage_layout(7, false),
                uniform_layout(
                    8,
                    true,
                    NonZeroU64::new(size_of::<Full32PassParams>() as u64),
                ),
            ],
        });
        let direct_pipeline_layout = full32_pipeline_layout(
            device,
            "gsplat-direct-gpu-order-radix-layout",
            &direct_layout,
        );
        let direct = StableFull32DirectPipelines {
            histogram: create_compute_pipeline(
                device,
                direct_shader,
                &direct_pipeline_layout,
                "histogram",
                "gsplat-direct-gpu-order-histogram-pipeline",
            ),
            scatter: create_compute_pipeline(
                device,
                direct_shader,
                &direct_pipeline_layout,
                "scatter",
                "gsplat-direct-gpu-order-scatter-pipeline",
            ),
            a_to_b_keys: full32_radix_bind_group(
                device,
                &direct_layout,
                "gsplat-direct-gpu-order-a-to-b-keys-bg",
                &keys_a,
                &keys_a,
                &keys_b,
                &prefix,
                &pass_params,
            ),
            a_to_b_ids: full32_radix_bind_group(
                device,
                &direct_layout,
                "gsplat-direct-gpu-order-a-to-b-ids-bg",
                &keys_a,
                &ids_a,
                &ids_b,
                &prefix,
                &pass_params,
            ),
            b_to_a_keys: full32_radix_bind_group(
                device,
                &direct_layout,
                "gsplat-direct-gpu-order-b-to-a-keys-bg",
                &keys_b,
                &keys_b,
                &keys_a,
                &prefix,
                &pass_params,
            ),
            b_to_a_ids: full32_radix_bind_group(
                device,
                &direct_layout,
                "gsplat-direct-gpu-order-b-to-a-ids-bg",
                &keys_b,
                &ids_b,
                &ids_a,
                &prefix,
                &pass_params,
            ),
        };

        let scan_kernel = GpuPrefixScanKernel::new(
            device,
            GpuPrefixScanKernelProfile {
                bind_group_layout: "gsplat-direct-gpu-order-scan-bgl",
                shader: "gsplat-direct-gpu-order-scan-shader",
                pipeline_layout: "gsplat-direct-gpu-order-scan-layout",
                scan_pipeline: "gsplat-direct-gpu-order-scan-pipeline",
                add_offsets_pipeline: "gsplat-direct-gpu-order-add-offsets-pipeline",
            },
        );
        let scan = scan_kernel
            .create_graph(
                device,
                &prefix,
                prefix_count,
                dispatch_limit,
                GpuPrefixScanGraphProfile {
                    sums: "gsplat-direct-gpu-order-scan-sums",
                    params: "gsplat-direct-gpu-order-scan-params",
                    bind_group: "gsplat-direct-gpu-order-scan-bg",
                    sums_usage: wgpu::BufferUsages::STORAGE,
                },
            )
            .expect("scan dispatch limits were validated");
        let visible_compaction_scan = match profile {
            StableFull32RadixProfile::ResidentVisible {
                group_offsets,
                group_offset_count,
                ..
            } => Some(
                scan_kernel
                    .create_graph(
                        device,
                        group_offsets,
                        group_offset_count,
                        dispatch_limit,
                        GpuPrefixScanGraphProfile {
                            sums: "gsplat-resident-visible-scan-sums",
                            params: "gsplat-resident-visible-scan-params",
                            bind_group: "gsplat-direct-gpu-order-scan-bg",
                            sums_usage: wgpu::BufferUsages::STORAGE,
                        },
                    )
                    .expect("visible compaction scan limits were validated"),
            ),
            StableFull32RadixProfile::Direct | StableFull32RadixProfile::Resident => None,
        };

        let resident = match profile {
            StableFull32RadixProfile::Direct => None,
            StableFull32RadixProfile::Resident => Some(full32_resident_pipelines(
                device,
                &keys_a,
                &keys_b,
                &ids_a,
                &ids_b,
                &prefix,
                &pass_params,
                None,
                None,
            )),
            StableFull32RadixProfile::ResidentVisible { control, .. } => {
                let prefix_clear_dispatch = Dispatch2d::for_workgroups(
                    full32_workgroup_count(prefix_count),
                    dispatch_limit,
                )
                .expect("visible radix prefix-clear limits were validated");
                Some(full32_resident_pipelines(
                    device,
                    &keys_a,
                    &keys_b,
                    &ids_a,
                    &ids_b,
                    &prefix,
                    &pass_params,
                    Some(control),
                    Some(prefix_clear_dispatch),
                ))
            }
        };

        let compatibility_output = compatibility_output.then(|| {
            let pairs = storage_buffer(
                device,
                "gsplat-direct-gpu-order-compatibility-pairs",
                u64::from(allocation_count) * size_of::<GpuSortPair>() as u64,
                data_usage,
            );
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: wgpu_label("gsplat-direct-gpu-order-pack-bgl"),
                entries: &[
                    uniform_layout(
                        8,
                        true,
                        NonZeroU64::new(size_of::<Full32PassParams>() as u64),
                    ),
                    storage_layout(10, true),
                    storage_layout(11, true),
                    storage_layout(12, false),
                ],
            });
            let pipeline_layout =
                full32_pipeline_layout(device, "gsplat-direct-gpu-order-pack-layout", &layout);
            let pipeline = create_compute_pipeline(
                device,
                direct_shader,
                &pipeline_layout,
                "pack_pairs",
                "gsplat-direct-gpu-order-pack-pipeline",
            );
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: wgpu_label("gsplat-direct-gpu-order-pack-bg"),
                layout: &layout,
                entries: &[
                    full32_sized_entry(8, &pass_params, size_of::<Full32PassParams>() as u64),
                    entry(10, &keys_a),
                    entry(11, &ids_a),
                    entry(12, &pairs),
                ],
            });
            StableFull32CompatibilityOutput {
                pairs,
                bind_group,
                pipeline,
            }
        });

        Self {
            count,
            profile_is_resident: profile.uses_resident_radix8(),
            profile_uses_visible_control: profile.uses_visible_control(),
            dispatch,
            pass_stride,
            passes,
            keys_a,
            keys_b,
            ids_a,
            _ids_b: ids_b,
            prefix,
            _pass_params: pass_params,
            scan_kernel,
            scan,
            visible_compaction_scan,
            direct,
            resident,
            compatibility_output,
        }
    }

    pub(crate) fn input_keys(&self) -> &wgpu::Buffer {
        &self.keys_a
    }

    pub(crate) fn spare_keys(&self) -> &wgpu::Buffer {
        &self.keys_b
    }

    pub(crate) fn input_source_ids(&self) -> &wgpu::Buffer {
        &self.ids_a
    }

    pub(crate) fn final_source_ids(&self) -> &wgpu::Buffer {
        &self.ids_a
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn final_pairs(&self) -> &wgpu::Buffer {
        &self
            .compatibility_output
            .as_ref()
            .expect("final_pairs requires the compatibility output")
            .pairs
    }

    pub(crate) fn encode_visible_compaction_scan(&self, encoder: &mut wgpu::CommandEncoder) {
        let graph = self
            .visible_compaction_scan
            .as_ref()
            .expect("visible compaction scan requires the visible Resident profile");
        self.scan_kernel.encode(
            encoder,
            graph,
            GpuPrefixScanPassLabels {
                scan: "gsplat-resident-visible-scan-pass",
                add_offsets: "gsplat-resident-visible-add-offsets-pass",
            },
        );
    }

    pub(crate) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        visible_control: Option<&wgpu::Buffer>,
        timestamps: Option<StableFull32RadixTimestampRange<'_>>,
    ) {
        if self.count == 0 {
            return;
        }
        debug_assert_eq!(self.profile_uses_visible_control, visible_control.is_some());
        for radix_pass in 0..self.passes {
            let dynamic_offset = radix_pass * self.pass_stride;
            let radix_begin_timestamp = (radix_pass == 0).then(|| {
                timestamps.map(|range| wgpu::ComputePassTimestampWrites {
                    query_set: range.query_set,
                    beginning_of_pass_write_index: Some(range.begin_index),
                    end_of_pass_write_index: None,
                })
            });
            let radix_begin_timestamp = radix_begin_timestamp.flatten();
            if let Some(resident) = &self.resident {
                let bind_group = if radix_pass % 2 == 0 {
                    &resident.a_to_b
                } else {
                    &resident.b_to_a
                };
                if let Some(prefix_clear) = &resident.prefix_clear {
                    full32_encode_stage(
                        encoder,
                        prefix_clear,
                        bind_group,
                        &[dynamic_offset],
                        resident
                            .prefix_clear_dispatch
                            .expect("visible profile has prefix-clear dispatch"),
                        "gsplat-resident-visible-radix8-prefix-clear-pass",
                        radix_begin_timestamp,
                    );
                    full32_encode_stage_indirect(
                        encoder,
                        &resident.histogram,
                        bind_group,
                        &[dynamic_offset],
                        visible_control.expect("visible profile has control"),
                        2 * WORD_BYTES,
                        "gsplat-resident-visible-radix8-histogram-pass",
                        None,
                    );
                } else {
                    full32_encode_stage(
                        encoder,
                        &resident.histogram,
                        bind_group,
                        &[dynamic_offset],
                        self.dispatch,
                        "gsplat-resident-gpu-order-radix8-histogram-pass",
                        radix_begin_timestamp,
                    );
                }
            } else {
                let bind_group = if radix_pass % 2 == 0 {
                    &self.direct.a_to_b_keys
                } else {
                    &self.direct.b_to_a_keys
                };
                full32_encode_stage(
                    encoder,
                    &self.direct.histogram,
                    bind_group,
                    &[dynamic_offset],
                    self.dispatch,
                    "gsplat-direct-gpu-order-histogram-pass",
                    radix_begin_timestamp,
                );
            }

            self.scan_kernel.encode(
                encoder,
                &self.scan,
                GpuPrefixScanPassLabels {
                    scan: "gsplat-direct-gpu-order-scan-pass",
                    add_offsets: "gsplat-direct-gpu-order-add-offsets-pass",
                },
            );

            let final_timestamp = (radix_pass + 1 == self.passes).then(|| {
                timestamps.map(|range| wgpu::ComputePassTimestampWrites {
                    query_set: range.query_set,
                    beginning_of_pass_write_index: None,
                    end_of_pass_write_index: Some(range.end_index),
                })
            });
            let final_timestamp = final_timestamp.flatten();
            if let Some(resident) = &self.resident {
                let bind_group = if radix_pass % 2 == 0 {
                    &resident.a_to_b
                } else {
                    &resident.b_to_a
                };
                if self.profile_uses_visible_control {
                    full32_encode_stage_indirect(
                        encoder,
                        &resident.scatter,
                        bind_group,
                        &[dynamic_offset],
                        visible_control.expect("visible profile has control"),
                        2 * WORD_BYTES,
                        "gsplat-resident-visible-radix8-scatter-pass",
                        final_timestamp,
                    );
                } else {
                    full32_encode_stage(
                        encoder,
                        &resident.scatter,
                        bind_group,
                        &[dynamic_offset],
                        self.dispatch,
                        "gsplat-resident-gpu-order-radix8-scatter-pass",
                        final_timestamp,
                    );
                }
            } else {
                let (keys_bind_group, ids_bind_group) = if radix_pass % 2 == 0 {
                    (&self.direct.a_to_b_keys, &self.direct.a_to_b_ids)
                } else {
                    (&self.direct.b_to_a_keys, &self.direct.b_to_a_ids)
                };
                full32_encode_stage(
                    encoder,
                    &self.direct.scatter,
                    keys_bind_group,
                    &[dynamic_offset],
                    self.dispatch,
                    "gsplat-direct-gpu-order-scatter-pass",
                    None,
                );
                full32_encode_stage(
                    encoder,
                    &self.direct.scatter,
                    ids_bind_group,
                    &[dynamic_offset],
                    self.dispatch,
                    "gsplat-direct-gpu-order-scatter-ids-pass",
                    final_timestamp,
                );
            }
        }

        if let Some(output) = &self.compatibility_output {
            full32_encode_stage(
                encoder,
                &output.pipeline,
                &output.bind_group,
                &[0],
                self.dispatch,
                "gsplat-direct-gpu-order-pack-pass",
                None,
            );
        }
    }

    #[cfg(test)]
    pub(crate) const fn radix_passes(&self) -> u32 {
        self.passes
    }

    #[cfg(test)]
    pub(crate) const fn uses_resident_radix8(&self) -> bool {
        self.profile_is_resident
    }

    #[cfg(test)]
    pub(crate) fn prefix(&self) -> &wgpu::Buffer {
        &self.prefix
    }

    #[cfg(test)]
    pub(crate) const fn has_compatibility_output(&self) -> bool {
        self.compatibility_output.is_some()
    }
}

#[allow(clippy::too_many_arguments)]
fn full32_resident_pipelines(
    device: &wgpu::Device,
    keys_a: &wgpu::Buffer,
    keys_b: &wgpu::Buffer,
    ids_a: &wgpu::Buffer,
    ids_b: &wgpu::Buffer,
    prefix: &wgpu::Buffer,
    pass_params: &wgpu::Buffer,
    control: Option<&wgpu::Buffer>,
    prefix_clear_dispatch: Option<Dispatch2d>,
) -> StableFull32ResidentPipelines {
    let visible = control.is_some();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: wgpu_label(if visible {
            "gsplat-resident-visible-gpu-order-radix8-shader"
        } else {
            "gsplat-resident-gpu-order-radix8-shader"
        }),
        source: wgpu::ShaderSource::Wgsl(
            if visible {
                include_str!("../../shaders/resident_gpu_order_visible_radix8.wgsl")
            } else {
                include_str!("../../shaders/resident_gpu_order_radix8.wgsl")
            }
            .into(),
        ),
    });
    let mut entries = vec![
        storage_layout(4, true),
        storage_layout(5, true),
        storage_layout(6, false),
        storage_layout(7, false),
        uniform_layout(
            8,
            true,
            NonZeroU64::new(size_of::<Full32PassParams>() as u64),
        ),
        storage_layout(13, false),
    ];
    if visible {
        entries.push(storage_layout(14, true));
    }
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: wgpu_label(if visible {
            "gsplat-resident-visible-gpu-order-radix8-bgl"
        } else {
            "gsplat-resident-gpu-order-radix8-bgl"
        }),
        entries: &entries,
    });
    let pipeline_layout = full32_pipeline_layout(
        device,
        if visible {
            "gsplat-resident-visible-gpu-order-radix8-layout"
        } else {
            "gsplat-resident-gpu-order-radix8-layout"
        },
        &layout,
    );
    let histogram = create_compute_pipeline(
        device,
        &shader,
        &pipeline_layout,
        if visible {
            "histogram_visible_radix8"
        } else {
            "histogram_radix8"
        },
        if visible {
            "gsplat-resident-visible-gpu-order-radix8-histogram-pipeline"
        } else {
            "gsplat-resident-gpu-order-radix8-histogram-pipeline"
        },
    );
    let scatter = create_compute_pipeline(
        device,
        &shader,
        &pipeline_layout,
        if visible {
            "scatter_visible_keys_ids_radix8"
        } else {
            "scatter_keys_ids_radix8"
        },
        if visible {
            "gsplat-resident-visible-gpu-order-radix8-scatter-pipeline"
        } else {
            "gsplat-resident-gpu-order-radix8-scatter-pipeline"
        },
    );
    let prefix_clear = visible.then(|| {
        create_compute_pipeline(
            device,
            &shader,
            &pipeline_layout,
            "clear_prefix_radix8",
            "gsplat-resident-visible-gpu-order-radix8-prefix-clear-pipeline",
        )
    });
    StableFull32ResidentPipelines {
        histogram,
        scatter,
        prefix_clear,
        a_to_b: full32_fused_bind_group(
            device,
            &layout,
            if visible {
                "gsplat-resident-visible-gpu-order-radix8-a-to-b-bg"
            } else {
                "gsplat-resident-gpu-order-radix8-a-to-b-bg"
            },
            keys_a,
            keys_b,
            ids_a,
            ids_b,
            prefix,
            pass_params,
            control,
        ),
        b_to_a: full32_fused_bind_group(
            device,
            &layout,
            if visible {
                "gsplat-resident-visible-gpu-order-radix8-b-to-a-bg"
            } else {
                "gsplat-resident-gpu-order-radix8-b-to-a-bg"
            },
            keys_b,
            keys_a,
            ids_b,
            ids_a,
            prefix,
            pass_params,
            control,
        ),
        prefix_clear_dispatch,
    }
}

fn full32_pipeline_layout(
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

fn full32_sized_entry<'a>(
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
fn full32_radix_bind_group(
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
            entry(4, keys_src),
            entry(5, payload_src),
            entry(6, payload_dst),
            entry(7, prefix),
            full32_sized_entry(8, params, size_of::<Full32PassParams>() as u64),
        ],
    })
}

#[allow(clippy::too_many_arguments)]
fn full32_fused_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &'static str,
    keys_src: &wgpu::Buffer,
    keys_dst: &wgpu::Buffer,
    ids_src: &wgpu::Buffer,
    ids_dst: &wgpu::Buffer,
    prefix: &wgpu::Buffer,
    params: &wgpu::Buffer,
    control: Option<&wgpu::Buffer>,
) -> wgpu::BindGroup {
    let mut entries = vec![
        entry(4, keys_src),
        entry(5, ids_src),
        entry(6, ids_dst),
        entry(7, prefix),
        full32_sized_entry(8, params, size_of::<Full32PassParams>() as u64),
        entry(13, keys_dst),
    ];
    if let Some(control) = control {
        entries.push(entry(14, control));
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: wgpu_label(label),
        layout,
        entries: &entries,
    })
}

fn full32_encode_stage(
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
fn full32_encode_stage_indirect(
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        pollster::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .ok()?;
            let mut limits = wgpu::Limits::downlevel_defaults();
            limits.max_storage_buffers_per_shader_stage = EXTERNAL_RADIX_STORAGE_BINDINGS;
            if !limits.check_limits(&adapter.limits()) {
                return None;
            }
            adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("external-prefix-radix-test-device"),
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

    fn expected_pairs(keys: &[u32], ids: &[u32]) -> Vec<(u32, u32)> {
        let mut pairs = keys
            .iter()
            .copied()
            .zip(ids.iter().copied())
            .collect::<Vec<_>>();
        // Slice sort is stable. Equal full32 keys must retain the producer's
        // source order, represented by the incoming ID sequence.
        pairs.sort_by(|left, right| right.0.cmp(&left.0));
        pairs
    }

    fn expected_profile_pairs(
        keys: &[u32],
        ids: &[u32],
        profile: ExternalPrefixRadixProfile,
    ) -> Vec<(u32, u32)> {
        let covered_bits = profile.pass_count * 4;
        let mask = if covered_bits == u32::BITS {
            u32::MAX
        } else {
            ((1_u32 << covered_bits) - 1) << profile.first_shift
        };
        let mut pairs = keys
            .iter()
            .copied()
            .zip(ids.iter().copied())
            .collect::<Vec<_>>();
        pairs.sort_by(|left, right| (right.0 & mask).cmp(&(left.0 & mask)));
        pairs
    }

    fn generated_case(count: usize, salt: u32) -> (Vec<u32>, Vec<u32>) {
        let mut keys = Vec::with_capacity(count);
        let mut ids = Vec::with_capacity(count);
        for index in 0..count as u32 {
            let mixed = index
                .wrapping_mul(0x9e37_79b9)
                .rotate_left((index.wrapping_add(salt) & 31) + 1)
                ^ salt.wrapping_mul(0x85eb_ca6b);
            // Frequent equal keys exercise stability across lanes, rounds,
            // radix tiles, and prefix-scan workgroups.
            let key = if index % 11 == 0 {
                0x8000_0001
            } else if index % 17 == 0 {
                0xffff_ffff
            } else if index % 23 == 0 {
                0
            } else {
                mixed
            };
            keys.push(key);
            ids.push(index.wrapping_add(salt.wrapping_mul(1_000_003)));
        }
        (keys, ids)
    }

    fn run_and_read(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        radix: &ExternalPrefixRadix,
        keys: &[u32],
        ids: &[u32],
    ) -> (Vec<(u32, u32)>, ExternalPrefixControl) {
        let control = radix
            .upload_prefix(queue, keys, ids)
            .expect("upload prefix");
        let output_bytes = (keys.len().max(1) * size_of::<u32>()) as u64;
        let key_readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("external-prefix-radix-key-readback"),
            size: output_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let id_readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("external-prefix-radix-id-readback"),
            size: output_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let control_readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("external-prefix-radix-control-readback"),
            size: size_of::<ExternalPrefixControl>() as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("external-prefix-radix-test-encoder"),
        });
        radix.encode(&mut encoder);
        if !keys.is_empty() {
            let logical_bytes = keys.len() as u64 * WORD_BYTES;
            encoder.copy_buffer_to_buffer(radix.final_keys(), 0, &key_readback, 0, logical_bytes);
            encoder.copy_buffer_to_buffer(
                radix.final_source_ids(),
                0,
                &id_readback,
                0,
                logical_bytes,
            );
        }
        encoder.copy_buffer_to_buffer(
            radix.control(),
            0,
            &control_readback,
            0,
            size_of::<ExternalPrefixControl>() as u64,
        );
        queue.submit(Some(encoder.finish()));

        let read_words = |buffer: &wgpu::Buffer, count: usize, label: &'static str| {
            if count == 0 {
                return Vec::new();
            }
            let slice = buffer.slice(..count as u64 * WORD_BYTES);
            let (tx, rx) = mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect(label);
            rx.recv().expect("map callback").expect("map result");
            let words = {
                let mapped = slice.get_mapped_range();
                bytemuck::cast_slice::<u8, u32>(&mapped).to_vec()
            };
            buffer.unmap();
            words
        };
        let actual_keys = read_words(&key_readback, keys.len(), "poll external keys");
        let actual_ids = read_words(&id_readback, ids.len(), "poll external IDs");

        let control_slice = control_readback.slice(..);
        let (tx, rx) = mpsc::channel();
        control_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll external control");
        rx.recv()
            .expect("control map callback")
            .expect("control map result");
        let actual_control = {
            let mapped = control_slice.get_mapped_range();
            *bytemuck::from_bytes::<ExternalPrefixControl>(&mapped)
        };
        control_readback.unmap();
        assert_eq!(actual_control, control);
        (
            actual_keys.into_iter().zip(actual_ids).collect(),
            actual_control,
        )
    }

    #[test]
    fn byte_plan_matches_the_exact_capacity_formula() {
        let limits = wgpu::Limits::downlevel_defaults();
        let capacity = 4_099_u32;
        let groups = capacity.div_ceil(EXTERNAL_RADIX_TILE_SIZE);
        let plan = ExternalPrefixRadixBytePlan::for_capacity(capacity, &limits).expect("plan");
        assert_eq!(plan.key_ping_pong, 8 * u64::from(capacity));
        assert_eq!(plan.source_id_ping_pong, 8 * u64::from(capacity));
        assert_eq!(plan.radix_prefix, 64 * u64::from(groups));
        assert_eq!(plan.control, 32);
        assert_eq!(
            plan.pass_params,
            u64::from(EXTERNAL_RADIX_PASSES)
                * u64::from(limits.min_uniform_buffer_offset_alignment.max(16)),
        );
        assert_eq!(
            plan.total_static,
            plan.key_ping_pong
                + plan.source_id_ping_pong
                + plan.radix_prefix
                + plan.scan_sums
                + plan.scan_params
                + plan.pass_params
                + plan.control,
        );

        let candidate_plan = ExternalPrefixRadixBytePlan::for_capacity_with_profile(
            capacity,
            &limits,
            ExternalPrefixRadixProfile::CANDIDATE_STABLE20,
        )
        .expect("candidate plan");
        assert_eq!(
            candidate_plan.pass_params,
            5 * u64::from(limits.min_uniform_buffer_offset_alignment.max(16)),
        );
        assert_eq!(
            plan.total_static - candidate_plan.total_static,
            3 * u64::from(limits.min_uniform_buffer_offset_alignment.max(16)),
        );
    }

    #[test]
    fn radix_profiles_reject_empty_unaligned_or_overflowing_bit_ranges() {
        assert!(ExternalPrefixRadixProfile::new(0, 0).validate().is_err());
        assert!(ExternalPrefixRadixProfile::new(3, 5).validate().is_err());
        assert!(ExternalPrefixRadixProfile::new(16, 5).validate().is_err());
        assert_eq!(
            ExternalPrefixRadixProfile::new(12, 5).validate(),
            Ok(ExternalPrefixRadixProfile::CANDIDATE_STABLE20),
        );
    }

    #[test]
    fn cpu_uploaded_prefix_matches_full_order_oracle_at_all_boundaries() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        for count in [0_usize, 1, 127, 128, 129, 1_023, 1_024, 1_025, 4_099] {
            let radix = ExternalPrefixRadix::new(&device, count as u32).expect("radix graph");
            let (keys, ids) = generated_case(count, count as u32 + 7);
            let (actual, control) = run_and_read(&device, &queue, &radix, &keys, &ids);
            assert_eq!(actual, expected_pairs(&keys, &ids), "count={count}");
            assert_eq!(control.count, count as u32);
            assert_eq!(
                control.active_group_count,
                (count as u32).div_ceil(EXTERNAL_RADIX_TILE_SIZE),
            );
        }
    }

    #[test]
    fn all_32_bits_and_equal_keys_preserve_stable_input_order() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let keys = vec![
            0,
            0xffff_ffff,
            0x0000_0001,
            0x8000_0000,
            0x7fff_ffff,
            0x8000_0001,
            0x8000_0001,
            0x0000_0010,
            0x1000_0000,
            0x0100_0000,
            0x0010_0000,
            0x0001_0000,
            0x0000_1000,
            0x0000_0100,
            0x0000_0010,
            0x0000_0001,
            0xffff_ffff,
        ];
        let ids = (0..keys.len() as u32)
            .map(|id| 9_000 + id)
            .collect::<Vec<_>>();
        let radix = ExternalPrefixRadix::new(&device, keys.len() as u32).expect("radix graph");
        assert_eq!(radix.profile(), ExternalPrefixRadixProfile::EXACT_FULL32);
        let (actual, _) = run_and_read(&device, &queue, &radix, &keys, &ids);
        assert_eq!(actual, expected_pairs(&keys, &ids));
        let equal_ids = actual
            .iter()
            .filter_map(|&(key, id)| (key == 0x8000_0001).then_some(id))
            .collect::<Vec<_>>();
        assert_eq!(equal_ids, vec![9_005, 9_006]);
    }

    #[test]
    fn candidate20_odd_pass_profile_selects_b_and_preserves_stable_pairs() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let profile = ExternalPrefixRadixProfile::CANDIDATE_STABLE20;
        for count in [0_usize, 1, 1_025] {
            let (mut keys, ids) = generated_case(count, 0x20);
            for (index, key) in keys.iter_mut().enumerate() {
                // Candidate20 observes only bits 12..31. Deliberately retain
                // distinct low bits to prove that equal retained keys remain
                // in producer order and that key/ID pairs stay intact.
                *key = (*key & 0xffff_f000) | (index as u32 & 0x0fff);
            }
            if count > 4 {
                keys[1] = 0x7654_3001;
                keys[2] = 0x7654_3ffe;
                keys[3] = 0x7654_3080;
            }
            let radix = ExternalPrefixRadix::new_with_profile(&device, count as u32, profile)
                .expect("candidate20 radix graph");
            assert_eq!(radix.profile(), profile);
            let (actual, control) = run_and_read(&device, &queue, &radix, &keys, &ids);
            assert_eq!(
                actual,
                expected_profile_pairs(&keys, &ids, profile),
                "count={count}",
            );
            assert_eq!(control.count, count as u32);
        }
    }

    #[test]
    fn exact_default_still_orders_low_twelve_bits_across_eight_passes() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let keys = vec![0x7654_3001, 0x7654_3ffe, 0x7654_3080, 0x7654_3000];
        let ids = vec![10, 11, 12, 13];
        let radix =
            ExternalPrefixRadix::new(&device, keys.len() as u32).expect("exact radix graph");
        assert_eq!(radix.profile(), ExternalPrefixRadixProfile::EXACT_FULL32);
        let (actual, _) = run_and_read(&device, &queue, &radix, &keys, &ids);
        assert_eq!(actual, expected_pairs(&keys, &ids));
        assert_eq!(
            actual,
            vec![
                (0x7654_3ffe, 11),
                (0x7654_3080, 12),
                (0x7654_3001, 10),
                (0x7654_3000, 13),
            ],
        );
    }

    #[test]
    fn one_high_water_graph_survives_full_sparse_zero_full_reuse() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let capacity = 4_099_usize;
        let radix = ExternalPrefixRadix::new(&device, capacity as u32).expect("radix graph");
        assert_eq!(radix.capacity(), capacity as u32);
        assert_eq!(radix.capacity_groups(), 5);
        for (count, salt) in [(capacity, 1_u32), (129, 2), (0, 3), (capacity, 4)] {
            let (keys, ids) = generated_case(count, salt);
            let (actual, control) = run_and_read(&device, &queue, &radix, &keys, &ids);
            assert_eq!(actual, expected_pairs(&keys, &ids), "count={count}");
            assert_eq!(control.count, count as u32);
        }
    }

    #[test]
    fn indirect_dispatch_flattens_across_two_dimensions_without_order_loss() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let dispatch_limit = 7_u32;
        let count = (dispatch_limit * EXTERNAL_RADIX_TILE_SIZE + 137) as usize;
        let radix = ExternalPrefixRadix::new_with_dispatch_limit(
            &device,
            count as u32,
            dispatch_limit,
            ExternalPrefixRadixProfile::EXACT_FULL32,
        )
        .expect("2D radix graph");
        let (keys, ids) = generated_case(count, 91);
        let (actual, control) = run_and_read(&device, &queue, &radix, &keys, &ids);
        assert_eq!(control.active_group_count, 8);
        assert_eq!((control.dispatch_x, control.dispatch_y), (7, 2));
        assert_eq!(actual, expected_pairs(&keys, &ids));
    }
}
