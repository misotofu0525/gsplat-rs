//! Portable stable full32 radix over a GPU- or CPU-produced dynamic prefix.
//!
//! This module intentionally knows nothing about projection, scene storage,
//! or presentation. Its sole contract is: an external producer writes
//! `{key, source_id}[0..C)` and one [`ExternalPrefixControl`], then eight
//! stable 4-bit LSD passes produce descending full32 keys and stable IDs in
//! the original A buffers. Capacity is a reusable high-water mark; C may vary
//! from zero through capacity every submission.

use std::{mem::size_of, num::NonZeroU64};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::resident_gpu::{RESIDENT_COLOR_STORAGE_BINDINGS, ResidentGpuError};
use crate::wgpu_label;

pub(crate) const EXTERNAL_RADIX_WORKGROUP_SIZE: u32 = 128;
pub(crate) const EXTERNAL_RADIX_ITEMS_PER_THREAD: u32 = 8;
pub(crate) const EXTERNAL_RADIX_TILE_SIZE: u32 =
    EXTERNAL_RADIX_WORKGROUP_SIZE * EXTERNAL_RADIX_ITEMS_PER_THREAD;
pub(crate) const EXTERNAL_RADIX: u32 = 16;
pub(crate) const EXTERNAL_RADIX_PASSES: u32 = 8;
const SCAN_WORKGROUP_SIZE: u32 = 256;
const SCAN_ITEMS_PER_GROUP: u32 = SCAN_WORKGROUP_SIZE * 2;
const WORD_BYTES: u64 = size_of::<u32>() as u64;

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

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ScanParams {
    count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
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
        let pass_params = u64::from(EXTERNAL_RADIX_PASSES)
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
        // The module is admitted only inside the Resident/Packed capability
        // envelope. Its radix entry point itself uses six storage bindings,
        // while the enclosing exact Resident graph already requires eight.
        if limits.max_storage_buffers_per_shader_stage < RESIDENT_COLOR_STORAGE_BINDINGS {
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

fn scan_sum_plan(mut count: u32) -> Result<(u64, u64, u32), ResidentGpuError> {
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
struct Dispatch2d {
    x: u32,
    y: u32,
}

impl Dispatch2d {
    fn for_workgroups(workgroups: u32, limit: u32) -> Result<Self, ResidentGpuError> {
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
                include_str!("../shaders/gpu_prefix_scan.wgsl").into(),
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

pub(crate) struct ExternalPrefixRadix {
    capacity: u32,
    capacity_groups: u32,
    dispatch_limit: u32,
    pass_stride: u32,
    keys_a: wgpu::Buffer,
    _keys_b: wgpu::Buffer,
    ids_a: wgpu::Buffer,
    _ids_b: wgpu::Buffer,
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
        Self::new_with_dispatch_limit(
            device,
            capacity,
            device.limits().max_compute_workgroups_per_dimension,
        )
    }

    fn new_with_dispatch_limit(
        device: &wgpu::Device,
        capacity: u32,
        dispatch_limit: u32,
    ) -> Result<Self, ResidentGpuError> {
        let physical_limit = device.limits().max_compute_workgroups_per_dimension;
        if dispatch_limit == 0 || dispatch_limit > physical_limit {
            return Err(ResidentGpuError::DispatchLimitExceeded);
        }
        let limits = device.limits();
        let byte_plan = ExternalPrefixRadixBytePlan::for_capacity(capacity, &limits)?
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
        let mut pass_params_bytes =
            vec![0_u8; pass_stride as usize * EXTERNAL_RADIX_PASSES as usize];
        for pass in 0..EXTERNAL_RADIX_PASSES {
            let params = RadixPassParams {
                shift: pass * 4,
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
                include_str!("../shaders/external_prefix_radix.wgsl").into(),
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
            keys_a,
            _keys_b: keys_b,
            ids_a,
            _ids_b: ids_b,
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
        for pass in 0..EXTERNAL_RADIX_PASSES {
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
        &self.keys_a
    }

    pub(crate) fn final_source_ids(&self) -> &wgpu::Buffer {
        &self.ids_a
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

fn storage_layout_read_only(binding: u32) -> wgpu::BindGroupLayoutEntry {
    storage_layout(binding, true)
}

fn uniform_layout(
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

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
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
            limits.max_storage_buffers_per_shader_stage = RESIDENT_COLOR_STORAGE_BINDINGS;
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
        let (actual, _) = run_and_read(&device, &queue, &radix, &keys, &ids);
        assert_eq!(actual, expected_pairs(&keys, &ids));
        let equal_ids = actual
            .iter()
            .filter_map(|&(key, id)| (key == 0x8000_0001).then_some(id))
            .collect::<Vec<_>>();
        assert_eq!(equal_ids, vec![9_005, 9_006]);
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
        let radix =
            ExternalPrefixRadix::new_with_dispatch_limit(&device, count as u32, dispatch_limit)
                .expect("2D radix graph");
        let (keys, ids) = generated_case(count, 91);
        let (actual, control) = run_and_read(&device, &queue, &radix, &keys, &ids);
        assert_eq!(control.active_group_count, 8);
        assert_eq!((control.dispatch_x, control.dispatch_y), (7, 2));
        assert_eq!(actual, expected_pairs(&keys, &ids));
    }
}
