//! Executable GPU reference for exact-count tiled Gaussian rasterization.
//!
//! The production integration can retain these pipelines and high-water
//! buffers across frames. This initial owner keeps the two allocation phases
//! explicit: GPU count/prefix is read back before entry allocation, so a scene
//! is either represented completely or rejected with a structured capacity or
//! allocation error. Scatter never truncates to a budget.

#![cfg(not(target_arch = "wasm32"))]

use std::{mem::size_of, sync::mpsc};

use bytemuck::{Pod, Zeroable};
use thiserror::Error;
use wgpu::util::DeviceExt;

use crate::tiled_raster::{ProjectedScene, ProjectedSplat, TileContribution, TiledRgbaImage};

const WORKGEN_WORKGROUP_SIZE: u32 = 128;
const SORT_WORKGROUP_SIZE: u32 = 128;
const SORT_ITEMS_PER_THREAD: u32 = 8;
const SORT_ITEMS_PER_GROUP: u32 = SORT_WORKGROUP_SIZE * SORT_ITEMS_PER_THREAD;
const RADIX: u32 = 16;
const RADIX_PASSES: u32 = 8;
const SCAN_WORKGROUP_SIZE: u32 = 256;
const SCAN_ITEMS_PER_GROUP: u32 = SCAN_WORKGROUP_SIZE * 2;
const SCAN_WORKGROUP_STORAGE_BYTES: u32 = SCAN_ITEMS_PER_GROUP * size_of::<u32>() as u32;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct GpuTiledParams {
    width: u32,
    height: u32,
    tiles_x: u32,
    tiles_y: u32,
    splat_count: u32,
    tile_count: u32,
    entry_capacity: u32,
    entry_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
struct GpuTiledStatus {
    total_entries: u32,
    overflow: u32,
    written_entries: u32,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct RadixPassParams {
    shift: u32,
    count: u32,
    group_count: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ScanParams {
    count: u32,
    _pad: [u32; 3],
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

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TiledGpuError {
    #[error("GPU tiled raster requires non-zero dimensions")]
    InvalidDimensions,
    #[error("GPU tiled raster source or tile count overflowed u32")]
    AddressSpaceExceeded,
    #[error(
        "GPU tiled raster resource {resource} needs {required_bytes} bytes; binding limit is {limit_bytes} bytes"
    )]
    CapacityExceeded {
        resource: &'static str,
        required_bytes: u64,
        limit_bytes: u64,
    },
    #[error("GPU tiled raster device limits are unsupported: {0}")]
    Unsupported(String),
    #[error("GPU tiled contribution count overflowed u32")]
    EntryCountOverflow,
    #[error("GPU tiled scatter reported overflow or an incomplete write")]
    ScatterOverflow,
    #[error("GPU tiled resource allocation ran out of memory")]
    OutOfMemory,
    #[error("GPU tiled resource or shader validation failed: {0}")]
    Validation(String),
    #[error("GPU tiled internal backend failure: {0}")]
    Internal(String),
    #[error("GPU tiled readback failed")]
    Readback,
    #[error("GPU tiled output decoding failed")]
    OutputDecode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TiledGpuRasterResult {
    pub image: TiledRgbaImage,
    pub entry_count: u32,
    /// Debug/evidence output in final raster order. Production callers can omit
    /// this readback while retaining the same GPU buffers and pipelines.
    pub ordered_entries: Vec<TileContribution>,
}

struct TiledGpuPipelines {
    count: wgpu::ComputePipeline,
    copy_splat_counts: wgpu::ComputePipeline,
    finalize_entry_count: wgpu::ComputePipeline,
    scatter: wgpu::ComputePipeline,
    copy_tile_counts: wgpu::ComputePipeline,
    finalize_tile_offsets: wgpu::ComputePipeline,
    init_source_keys: wgpu::ComputePipeline,
    rekey_depth: wgpu::ComputePipeline,
    rekey_tile: wgpu::ComputePipeline,
    radix_histogram: wgpu::ComputePipeline,
    radix_scatter: wgpu::ComputePipeline,
    scan_blocks: wgpu::ComputePipeline,
    scan_add_offsets: wgpu::ComputePipeline,
    scan_layout: wgpu::BindGroupLayout,
    raster: wgpu::ComputePipeline,
}

impl TiledGpuPipelines {
    fn create(device: &wgpu::Device) -> Self {
        let workgen_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gsplat-tiled-work-generation-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/tiled_work_generation.wgsl").into(),
            ),
        });
        let radix_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gsplat-tiled-radix-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/tiled_radix_sort.wgsl").into(),
            ),
        });
        let scan_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gsplat-tiled-prefix-scan-shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/gpu_prefix_scan.wgsl").into(),
            ),
        });
        let raster_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gsplat-tiled-raster-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/tiled_raster.wgsl").into()),
        });

        let scan_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gsplat-tiled-scan-bgl"),
            entries: &[
                storage_layout_entry(0, false),
                storage_layout_entry(1, false),
                uniform_layout_entry(2),
            ],
        });
        let scan_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gsplat-tiled-scan-pipeline-layout"),
            bind_group_layouts: &[&scan_layout],
            immediate_size: 0,
        });

        Self {
            count: auto_compute_pipeline(
                device,
                &workgen_shader,
                "count_contributions",
                "gsplat-tiled-count-pipeline",
            ),
            copy_splat_counts: auto_compute_pipeline(
                device,
                &workgen_shader,
                "copy_splat_counts",
                "gsplat-tiled-copy-splat-counts-pipeline",
            ),
            finalize_entry_count: auto_compute_pipeline(
                device,
                &workgen_shader,
                "finalize_entry_count",
                "gsplat-tiled-finalize-entry-count-pipeline",
            ),
            scatter: auto_compute_pipeline(
                device,
                &workgen_shader,
                "scatter_contributions",
                "gsplat-tiled-scatter-pipeline",
            ),
            copy_tile_counts: auto_compute_pipeline(
                device,
                &workgen_shader,
                "copy_tile_counts",
                "gsplat-tiled-copy-tile-counts-pipeline",
            ),
            finalize_tile_offsets: auto_compute_pipeline(
                device,
                &workgen_shader,
                "finalize_tile_offsets",
                "gsplat-tiled-finalize-tile-offsets-pipeline",
            ),
            init_source_keys: auto_compute_pipeline(
                device,
                &radix_shader,
                "init_source_keys",
                "gsplat-tiled-init-source-keys-pipeline",
            ),
            rekey_depth: auto_compute_pipeline(
                device,
                &radix_shader,
                "rekey_depth",
                "gsplat-tiled-rekey-depth-pipeline",
            ),
            rekey_tile: auto_compute_pipeline(
                device,
                &radix_shader,
                "rekey_tile",
                "gsplat-tiled-rekey-tile-pipeline",
            ),
            radix_histogram: auto_compute_pipeline(
                device,
                &radix_shader,
                "histogram",
                "gsplat-tiled-radix-histogram-pipeline",
            ),
            radix_scatter: auto_compute_pipeline(
                device,
                &radix_shader,
                "scatter",
                "gsplat-tiled-radix-scatter-pipeline",
            ),
            scan_blocks: compute_pipeline(
                device,
                &scan_shader,
                Some(&scan_pipeline_layout),
                "scan_blocks",
                "gsplat-tiled-scan-blocks-pipeline",
            ),
            scan_add_offsets: compute_pipeline(
                device,
                &scan_shader,
                Some(&scan_pipeline_layout),
                "add_block_offsets",
                "gsplat-tiled-scan-add-offsets-pipeline",
            ),
            scan_layout,
            raster: auto_compute_pipeline(
                device,
                &raster_shader,
                "rasterize_tile",
                "gsplat-tiled-raster-pipeline",
            ),
        }
    }
}

fn auto_compute_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    entry_point: &'static str,
    label: &'static str,
) -> wgpu::ComputePipeline {
    compute_pipeline(device, shader, None, entry_point, label)
}

fn compute_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: Option<&wgpu::PipelineLayout>,
    entry_point: &'static str,
    label: &'static str,
) -> wgpu::ComputePipeline {
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout,
        module: shader,
        entry_point: Some(entry_point),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    })
}

fn storage_layout_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
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

fn uniform_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

struct ScanLevel {
    bind_group: wgpu::BindGroup,
    dispatch: Dispatch2d,
}

struct ScanResources {
    _sums: Vec<wgpu::Buffer>,
    _params: Vec<wgpu::Buffer>,
    levels: Vec<ScanLevel>,
}

impl ScanResources {
    fn create(
        device: &wgpu::Device,
        pipelines: &TiledGpuPipelines,
        root: &wgpu::Buffer,
        count: u32,
        dispatch_limit: u32,
    ) -> Result<Self, TiledGpuError> {
        if count == 0 {
            return Ok(Self {
                _sums: Vec::new(),
                _params: Vec::new(),
                levels: Vec::new(),
            });
        }
        let level_counts = scan_level_counts(count);
        let sums = level_counts
            .iter()
            .map(|&(_, groups)| {
                storage_buffer(
                    device,
                    "gsplat-tiled-scan-sums",
                    u64::from(groups) * size_of::<u32>() as u64,
                    wgpu::BufferUsages::STORAGE,
                )
            })
            .collect::<Vec<_>>();
        let params = level_counts
            .iter()
            .map(|&(level_count, _)| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("gsplat-tiled-scan-params"),
                    contents: bytemuck::bytes_of(&ScanParams {
                        count: level_count,
                        _pad: [0; 3],
                    }),
                    usage: wgpu::BufferUsages::UNIFORM,
                })
            })
            .collect::<Vec<_>>();

        let mut levels = Vec::with_capacity(level_counts.len());
        for (index, &(_, groups)) in level_counts.iter().enumerate() {
            let data = if index == 0 { root } else { &sums[index - 1] };
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gsplat-tiled-scan-bind-group"),
                layout: &pipelines.scan_layout,
                entries: &[
                    buffer_entry(0, data),
                    buffer_entry(1, &sums[index]),
                    buffer_entry(2, &params[index]),
                ],
            });
            let dispatch = Dispatch2d::for_workgroups(groups, dispatch_limit).ok_or_else(|| {
                TiledGpuError::Unsupported(format!(
                    "prefix scan needs {groups} workgroups beyond {dispatch_limit}x{dispatch_limit}"
                ))
            })?;
            levels.push(ScanLevel {
                bind_group,
                dispatch,
            });
        }
        Ok(Self {
            _sums: sums,
            _params: params,
            levels,
        })
    }

    fn encode(&self, encoder: &mut wgpu::CommandEncoder, pipelines: &TiledGpuPipelines) {
        for level in &self.levels {
            encode_compute(
                encoder,
                &pipelines.scan_blocks,
                &level.bind_group,
                level.dispatch,
                "gsplat-tiled-scan-blocks-pass",
            );
        }
        if self.levels.len() > 1 {
            for level in self.levels[..self.levels.len() - 1].iter().rev() {
                encode_compute(
                    encoder,
                    &pipelines.scan_add_offsets,
                    &level.bind_group,
                    level.dispatch,
                    "gsplat-tiled-scan-add-offsets-pass",
                );
            }
        }
    }
}

struct RadixPass {
    histogram: wgpu::BindGroup,
    scatter_keys: wgpu::BindGroup,
    scatter_ids: wgpu::BindGroup,
}

struct RadixResources {
    keys_a: wgpu::Buffer,
    _keys_b: wgpu::Buffer,
    ids_a: wgpu::Buffer,
    _ids_b: wgpu::Buffer,
    _prefix: wgpu::Buffer,
    _pass_params: Vec<wgpu::Buffer>,
    prefix_scan: ScanResources,
    passes: Vec<RadixPass>,
    dispatch: Dispatch2d,
}

impl RadixResources {
    fn create(
        device: &wgpu::Device,
        pipelines: &TiledGpuPipelines,
        count: u32,
        dispatch_limit: u32,
    ) -> Result<Self, TiledGpuError> {
        debug_assert!(count > 0);
        let element_bytes = u64::from(count) * size_of::<u32>() as u64;
        let usage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let keys_a = storage_buffer(device, "gsplat-tiled-keys-a", element_bytes, usage);
        let keys_b = storage_buffer(device, "gsplat-tiled-keys-b", element_bytes, usage);
        let ids_a = storage_buffer(device, "gsplat-tiled-ids-a", element_bytes, usage);
        let ids_b = storage_buffer(device, "gsplat-tiled-ids-b", element_bytes, usage);
        let group_count = count.div_ceil(SORT_ITEMS_PER_GROUP).max(1);
        let prefix_count = group_count
            .checked_mul(RADIX)
            .ok_or(TiledGpuError::AddressSpaceExceeded)?;
        let prefix = storage_buffer(
            device,
            "gsplat-tiled-radix-prefix",
            u64::from(prefix_count) * size_of::<u32>() as u64,
            wgpu::BufferUsages::STORAGE,
        );
        let prefix_scan =
            ScanResources::create(device, pipelines, &prefix, prefix_count, dispatch_limit)?;
        let dispatch = Dispatch2d::for_workgroups(group_count, dispatch_limit).ok_or_else(|| {
            TiledGpuError::Unsupported(format!(
                "radix sort needs {group_count} workgroups beyond {dispatch_limit}x{dispatch_limit}"
            ))
        })?;

        let pass_params = (0..RADIX_PASSES)
            .map(|pass| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("gsplat-tiled-radix-pass-params"),
                    contents: bytemuck::bytes_of(&RadixPassParams {
                        shift: pass * 4,
                        count,
                        group_count,
                        _pad: 0,
                    }),
                    usage: wgpu::BufferUsages::UNIFORM,
                })
            })
            .collect::<Vec<_>>();

        let histogram_layout = pipelines.radix_histogram.get_bind_group_layout(0);
        let scatter_layout = pipelines.radix_scatter.get_bind_group_layout(0);
        let mut passes = Vec::with_capacity(RADIX_PASSES as usize);
        for (pass, pass_param) in pass_params.iter().enumerate() {
            let (keys_src, keys_dst, ids_src, ids_dst) = if pass % 2 == 0 {
                (&keys_a, &keys_b, &ids_a, &ids_b)
            } else {
                (&keys_b, &keys_a, &ids_b, &ids_a)
            };
            let histogram = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gsplat-tiled-radix-histogram-bind-group"),
                layout: &histogram_layout,
                entries: &[
                    buffer_entry(4, keys_src),
                    buffer_entry(7, &prefix),
                    buffer_entry(8, pass_param),
                ],
            });
            let scatter_keys = radix_scatter_bind_group(
                device,
                &scatter_layout,
                keys_src,
                keys_src,
                keys_dst,
                &prefix,
                pass_param,
            );
            let scatter_ids = radix_scatter_bind_group(
                device,
                &scatter_layout,
                keys_src,
                ids_src,
                ids_dst,
                &prefix,
                pass_param,
            );
            passes.push(RadixPass {
                histogram,
                scatter_keys,
                scatter_ids,
            });
        }

        Ok(Self {
            keys_a,
            _keys_b: keys_b,
            ids_a,
            _ids_b: ids_b,
            _prefix: prefix,
            _pass_params: pass_params,
            prefix_scan,
            passes,
            dispatch,
        })
    }

    fn encode_field(&self, encoder: &mut wgpu::CommandEncoder, pipelines: &TiledGpuPipelines) {
        for pass in &self.passes {
            encode_compute(
                encoder,
                &pipelines.radix_histogram,
                &pass.histogram,
                self.dispatch,
                "gsplat-tiled-radix-histogram-pass",
            );
            self.prefix_scan.encode(encoder, pipelines);
            encode_compute(
                encoder,
                &pipelines.radix_scatter,
                &pass.scatter_keys,
                self.dispatch,
                "gsplat-tiled-radix-scatter-keys-pass",
            );
            encode_compute(
                encoder,
                &pipelines.radix_scatter,
                &pass.scatter_ids,
                self.dispatch,
                "gsplat-tiled-radix-scatter-ids-pass",
            );
        }
    }
}

fn radix_scatter_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    keys: &wgpu::Buffer,
    payload: &wgpu::Buffer,
    output: &wgpu::Buffer,
    prefix: &wgpu::Buffer,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gsplat-tiled-radix-scatter-bind-group"),
        layout,
        entries: &[
            buffer_entry(4, keys),
            buffer_entry(5, payload),
            buffer_entry(6, output),
            buffer_entry(7, prefix),
            buffer_entry(8, params),
        ],
    })
}

fn scan_level_counts(mut count: u32) -> Vec<(u32, u32)> {
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

/// Execute the complete two-phase GPU tile path and read back RGBA16F.
///
/// This blocking native entrypoint is a correctness/offscreen integration seam.
/// A Surface owner should cache the pipelines and entry high-water capacity,
/// then replace the readbacks with asynchronous capacity growth and a blit.
pub fn rasterize_projected_scene_gpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &ProjectedScene,
) -> Result<TiledGpuRasterResult, TiledGpuError> {
    validate_device_limits(device, scene)?;
    let limits = device.limits();
    let dispatch_limit = limits.max_compute_workgroups_per_dimension;
    let splat_count =
        u32::try_from(scene.splats.len()).map_err(|_| TiledGpuError::AddressSpaceExceeded)?;
    let tiles_x = scene.width.div_ceil(16);
    let tiles_y = scene.height.div_ceil(16);
    let tile_count = tiles_x
        .checked_mul(tiles_y)
        .ok_or(TiledGpuError::AddressSpaceExceeded)?;
    let workgen_groups = splat_count.div_ceil(WORKGEN_WORKGROUP_SIZE).max(1);
    let workgen_dispatch = Dispatch2d::for_workgroups(workgen_groups, dispatch_limit)
        .ok_or_else(|| {
            TiledGpuError::Unsupported(format!(
                "work generation needs {workgen_groups} workgroups beyond {dispatch_limit}x{dispatch_limit}"
            ))
        })?;

    let pipelines = create_scoped(device, || TiledGpuPipelines::create(device))?;
    let splat_bytes = u64::from(splat_count.max(1)) * size_of::<ProjectedSplat>() as u64;
    let count_bytes = u64::from(splat_count.max(1)) * size_of::<u32>() as u64;
    validate_binding(device, "projected splats", splat_bytes)?;
    validate_binding(device, "per-splat counts", count_bytes)?;

    let (splat_buffer, params_phase1, counts, offsets, status, status_readback) =
        create_scoped(device, || {
            let splat_buffer = if scene.splats.is_empty() {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("gsplat-tiled-projected-splats-empty"),
                    size: size_of::<ProjectedSplat>() as u64,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                })
            } else {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("gsplat-tiled-projected-splats"),
                    contents: bytemuck::cast_slice(&scene.splats),
                    usage: wgpu::BufferUsages::STORAGE,
                })
            };
            let params_phase1 = uniform_buffer(
                device,
                "gsplat-tiled-params-phase1",
                &GpuTiledParams {
                    width: scene.width,
                    height: scene.height,
                    tiles_x,
                    tiles_y,
                    splat_count,
                    tile_count,
                    entry_capacity: 0,
                    entry_count: 0,
                },
            );
            let counts = storage_buffer(
                device,
                "gsplat-tiled-splat-counts",
                count_bytes,
                wgpu::BufferUsages::STORAGE,
            );
            let offsets = storage_buffer(
                device,
                "gsplat-tiled-splat-offsets",
                count_bytes,
                wgpu::BufferUsages::STORAGE,
            );
            let status = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("gsplat-tiled-status"),
                contents: bytemuck::bytes_of(&GpuTiledStatus::zeroed()),
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            });
            let status_readback = readback_buffer(
                device,
                "gsplat-tiled-status-readback",
                size_of::<GpuTiledStatus>() as u64,
            );
            (
                splat_buffer,
                params_phase1,
                counts,
                offsets,
                status,
                status_readback,
            )
        })?;

    let count_bind_group = bind_group(
        device,
        &pipelines.count,
        "gsplat-tiled-count-bind-group",
        &[
            buffer_entry(0, &splat_buffer),
            buffer_entry(1, &params_phase1),
            buffer_entry(2, &counts),
            buffer_entry(5, &status),
        ],
    );
    let copy_counts_bind_group = bind_group(
        device,
        &pipelines.copy_splat_counts,
        "gsplat-tiled-copy-splat-counts-bind-group",
        &[
            buffer_entry(1, &params_phase1),
            buffer_entry(2, &counts),
            buffer_entry(3, &offsets),
        ],
    );
    let finalize_count_bind_group = bind_group(
        device,
        &pipelines.finalize_entry_count,
        "gsplat-tiled-finalize-count-bind-group",
        &[
            buffer_entry(1, &params_phase1),
            buffer_entry(2, &counts),
            buffer_entry(3, &offsets),
            buffer_entry(5, &status),
        ],
    );
    let splat_scan =
        ScanResources::create(device, &pipelines, &offsets, splat_count, dispatch_limit)?;

    let mut count_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("gsplat-tiled-count-encoder"),
    });
    if splat_count > 0 {
        encode_compute(
            &mut count_encoder,
            &pipelines.count,
            &count_bind_group,
            workgen_dispatch,
            "gsplat-tiled-count-pass",
        );
        encode_compute(
            &mut count_encoder,
            &pipelines.copy_splat_counts,
            &copy_counts_bind_group,
            workgen_dispatch,
            "gsplat-tiled-copy-splat-counts-pass",
        );
        splat_scan.encode(&mut count_encoder, &pipelines);
    }
    encode_compute(
        &mut count_encoder,
        &pipelines.finalize_entry_count,
        &finalize_count_bind_group,
        Dispatch2d { x: 1, y: 1 },
        "gsplat-tiled-finalize-entry-count-pass",
    );
    count_encoder.copy_buffer_to_buffer(
        &status,
        0,
        &status_readback,
        0,
        size_of::<GpuTiledStatus>() as u64,
    );
    queue.submit(Some(count_encoder.finish()));
    let count_status = read_pod::<GpuTiledStatus>(device, &status_readback)?;
    if count_status.overflow != 0 {
        return Err(TiledGpuError::EntryCountOverflow);
    }
    let entry_count = count_status.total_entries;

    let entry_bytes = u64::from(entry_count.max(1)) * size_of::<TileContribution>() as u64;
    let key_bytes = u64::from(entry_count.max(1)) * size_of::<u32>() as u64;
    let tile_offsets_bytes = u64::from(tile_count)
        .checked_add(1)
        .and_then(|count| count.checked_mul(size_of::<u32>() as u64))
        .ok_or(TiledGpuError::AddressSpaceExceeded)?;
    validate_binding(device, "tile contributions", entry_bytes)?;
    validate_binding(device, "tile radix keys", key_bytes)?;
    validate_binding(device, "tile offsets", tile_offsets_bytes)?;

    let params_phase2 = uniform_buffer(
        device,
        "gsplat-tiled-params-phase2",
        &GpuTiledParams {
            width: scene.width,
            height: scene.height,
            tiles_x,
            tiles_y,
            splat_count,
            tile_count,
            entry_capacity: entry_count,
            entry_count,
        },
    );
    queue.write_buffer(&status, 0, bytemuck::bytes_of(&GpuTiledStatus::zeroed()));

    let phase2 = create_scoped(device, || {
        Phase2Resources::create(
            device,
            &pipelines,
            scene,
            entry_count,
            tile_count,
            tile_offsets_bytes,
            dispatch_limit,
            &splat_buffer,
            &params_phase2,
        )
    })??;

    let scatter_bind_group = bind_group(
        device,
        &pipelines.scatter,
        "gsplat-tiled-scatter-bind-group",
        &[
            buffer_entry(0, &splat_buffer),
            buffer_entry(1, &params_phase2),
            buffer_entry(2, &counts),
            buffer_entry(3, &offsets),
            buffer_entry(4, &phase2.entries),
            buffer_entry(5, &status),
            buffer_entry(6, &phase2.tile_counts),
        ],
    );
    let copy_tile_counts_bind_group = bind_group(
        device,
        &pipelines.copy_tile_counts,
        "gsplat-tiled-copy-tile-counts-bind-group",
        &[
            buffer_entry(1, &params_phase2),
            buffer_entry(3, &phase2.tile_offsets),
            buffer_entry(6, &phase2.tile_counts),
        ],
    );
    let finalize_tile_offsets_bind_group = bind_group(
        device,
        &pipelines.finalize_tile_offsets,
        "gsplat-tiled-finalize-tile-offsets-bind-group",
        &[
            buffer_entry(1, &params_phase2),
            buffer_entry(3, &phase2.tile_offsets),
            buffer_entry(5, &status),
        ],
    );

    let tile_groups = tile_count.div_ceil(WORKGEN_WORKGROUP_SIZE).max(1);
    let tile_dispatch = Dispatch2d::for_workgroups(tile_groups, dispatch_limit).ok_or_else(|| {
        TiledGpuError::Unsupported(format!(
            "tile prefix input needs {tile_groups} workgroups beyond {dispatch_limit}x{dispatch_limit}"
        ))
    })?;
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("gsplat-tiled-render-encoder"),
    });
    if splat_count > 0 {
        encode_compute(
            &mut encoder,
            &pipelines.scatter,
            &scatter_bind_group,
            workgen_dispatch,
            "gsplat-tiled-scatter-pass",
        );
    }
    encode_compute(
        &mut encoder,
        &pipelines.copy_tile_counts,
        &copy_tile_counts_bind_group,
        tile_dispatch,
        "gsplat-tiled-copy-tile-counts-pass",
    );
    phase2.tile_scan.encode(&mut encoder, &pipelines);
    encode_compute(
        &mut encoder,
        &pipelines.finalize_tile_offsets,
        &finalize_tile_offsets_bind_group,
        Dispatch2d { x: 1, y: 1 },
        "gsplat-tiled-finalize-tile-offsets-pass",
    );

    if entry_count > 0 {
        let radix = &phase2.radix;
        let entry_dispatch = radix.dispatch;
        encode_compute(
            &mut encoder,
            &pipelines.init_source_keys,
            &phase2.init_source_bind_group,
            entry_dispatch,
            "gsplat-tiled-init-source-keys-pass",
        );
        radix.encode_field(&mut encoder, &pipelines);
        encode_compute(
            &mut encoder,
            &pipelines.rekey_depth,
            &phase2.rekey_depth_bind_group,
            entry_dispatch,
            "gsplat-tiled-rekey-depth-pass",
        );
        radix.encode_field(&mut encoder, &pipelines);
        encode_compute(
            &mut encoder,
            &pipelines.rekey_tile,
            &phase2.rekey_tile_bind_group,
            entry_dispatch,
            "gsplat-tiled-rekey-tile-pass",
        );
        radix.encode_field(&mut encoder, &pipelines);
    }

    encode_compute_xy(
        &mut encoder,
        &pipelines.raster,
        &phase2.raster_bind_group,
        tiles_x,
        tiles_y,
        "gsplat-tiled-raster-pass",
    );
    phase2.encode_readbacks(
        &mut encoder,
        &status,
        entry_count,
        scene.width,
        scene.height,
    );
    queue.submit(Some(encoder.finish()));

    let phase2_status = read_pod::<GpuTiledStatus>(device, &phase2.status_readback)?;
    if phase2_status.overflow != 0
        || phase2_status.total_entries != entry_count
        || phase2_status.written_entries != entry_count
    {
        return Err(TiledGpuError::ScatterOverflow);
    }
    let ordered_entries = phase2.read_ordered_entries(device, entry_count)?;
    let image = phase2.read_image(device, scene.width, scene.height)?;
    Ok(TiledGpuRasterResult {
        image,
        entry_count,
        ordered_entries,
    })
}

struct Phase2Resources {
    entries: wgpu::Buffer,
    tile_counts: wgpu::Buffer,
    tile_offsets: wgpu::Buffer,
    tile_scan: ScanResources,
    radix: RadixResources,
    init_source_bind_group: wgpu::BindGroup,
    rekey_depth_bind_group: wgpu::BindGroup,
    rekey_tile_bind_group: wgpu::BindGroup,
    _output_texture: wgpu::Texture,
    raster_bind_group: wgpu::BindGroup,
    status_readback: wgpu::Buffer,
    entries_readback: wgpu::Buffer,
    ids_readback: wgpu::Buffer,
    image_readback: wgpu::Buffer,
    padded_image_bytes_per_row: u32,
}

impl Phase2Resources {
    #[allow(clippy::too_many_arguments)]
    fn create(
        device: &wgpu::Device,
        pipelines: &TiledGpuPipelines,
        scene: &ProjectedScene,
        entry_count: u32,
        tile_count: u32,
        tile_offsets_bytes: u64,
        dispatch_limit: u32,
        splat_buffer: &wgpu::Buffer,
        params: &wgpu::Buffer,
    ) -> Result<Self, TiledGpuError> {
        let entry_bytes = u64::from(entry_count.max(1)) * size_of::<TileContribution>() as u64;
        let key_bytes = u64::from(entry_count.max(1)) * size_of::<u32>() as u64;
        let tile_count_bytes = u64::from(tile_count.max(1)) * size_of::<u32>() as u64;
        let entries = storage_buffer(
            device,
            "gsplat-tiled-entries",
            entry_bytes,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let tile_counts = storage_buffer(
            device,
            "gsplat-tiled-tile-counts",
            tile_count_bytes,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let tile_offsets = storage_buffer(
            device,
            "gsplat-tiled-tile-offsets",
            tile_offsets_bytes,
            wgpu::BufferUsages::STORAGE,
        );
        let tile_scan =
            ScanResources::create(device, pipelines, &tile_offsets, tile_count, dispatch_limit)?;
        let radix = RadixResources::create(device, pipelines, entry_count.max(1), dispatch_limit)?;

        let init_source_bind_group = bind_group(
            device,
            &pipelines.init_source_keys,
            "gsplat-tiled-init-source-bind-group",
            &[
                buffer_entry(0, &entries),
                buffer_entry(1, params),
                buffer_entry(2, &radix.keys_a),
                buffer_entry(3, &radix.ids_a),
            ],
        );
        let rekey_depth_bind_group = bind_group(
            device,
            &pipelines.rekey_depth,
            "gsplat-tiled-rekey-depth-bind-group",
            &[
                buffer_entry(0, &entries),
                buffer_entry(1, params),
                buffer_entry(2, &radix.keys_a),
                buffer_entry(3, &radix.ids_a),
            ],
        );
        let rekey_tile_bind_group = bind_group(
            device,
            &pipelines.rekey_tile,
            "gsplat-tiled-rekey-tile-bind-group",
            &[
                buffer_entry(0, &entries),
                buffer_entry(1, params),
                buffer_entry(2, &radix.keys_a),
                buffer_entry(3, &radix.ids_a),
            ],
        );

        let output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gsplat-tiled-rgba16f-output"),
            size: wgpu::Extent3d {
                width: scene.width,
                height: scene.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let raster_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gsplat-tiled-raster-bind-group"),
            layout: &pipelines.raster.get_bind_group_layout(0),
            entries: &[
                buffer_entry(0, splat_buffer),
                buffer_entry(1, &entries),
                buffer_entry(2, &radix.ids_a),
                buffer_entry(3, &tile_offsets),
                buffer_entry(4, params),
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&output_view),
                },
            ],
        });

        let status_readback = readback_buffer(
            device,
            "gsplat-tiled-phase2-status-readback",
            size_of::<GpuTiledStatus>() as u64,
        );
        let entries_readback =
            readback_buffer(device, "gsplat-tiled-entries-readback", entry_bytes);
        let ids_readback = readback_buffer(device, "gsplat-tiled-ids-readback", key_bytes);
        let unpadded_image_bytes_per_row = scene
            .width
            .checked_mul(8)
            .ok_or(TiledGpuError::AddressSpaceExceeded)?;
        let padded_image_bytes_per_row = unpadded_image_bytes_per_row
            .div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            .checked_mul(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            .ok_or(TiledGpuError::AddressSpaceExceeded)?;
        let image_readback_bytes = u64::from(padded_image_bytes_per_row)
            .checked_mul(u64::from(scene.height))
            .ok_or(TiledGpuError::AddressSpaceExceeded)?;
        let image_readback =
            readback_buffer(device, "gsplat-tiled-image-readback", image_readback_bytes);

        Ok(Self {
            entries,
            tile_counts,
            tile_offsets,
            tile_scan,
            radix,
            init_source_bind_group,
            rekey_depth_bind_group,
            rekey_tile_bind_group,
            _output_texture: output_texture,
            raster_bind_group,
            status_readback,
            entries_readback,
            ids_readback,
            image_readback,
            padded_image_bytes_per_row,
        })
    }

    fn encode_readbacks(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        status: &wgpu::Buffer,
        entry_count: u32,
        width: u32,
        height: u32,
    ) {
        encoder.copy_buffer_to_buffer(
            status,
            0,
            &self.status_readback,
            0,
            size_of::<GpuTiledStatus>() as u64,
        );
        if entry_count > 0 {
            encoder.copy_buffer_to_buffer(
                &self.entries,
                0,
                &self.entries_readback,
                0,
                u64::from(entry_count) * size_of::<TileContribution>() as u64,
            );
            encoder.copy_buffer_to_buffer(
                &self.radix.ids_a,
                0,
                &self.ids_readback,
                0,
                u64::from(entry_count) * size_of::<u32>() as u64,
            );
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self._output_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.image_readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_image_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    fn read_ordered_entries(
        &self,
        device: &wgpu::Device,
        entry_count: u32,
    ) -> Result<Vec<TileContribution>, TiledGpuError> {
        if entry_count == 0 {
            return Ok(Vec::new());
        }
        let entry_bytes = read_bytes(device, &self.entries_readback, u64::from(entry_count) * 16)?;
        let id_bytes = read_bytes(
            device,
            &self.ids_readback,
            u64::from(entry_count) * size_of::<u32>() as u64,
        )?;
        let entries = bytemuck::try_cast_slice::<u8, TileContribution>(&entry_bytes)
            .map_err(|_| TiledGpuError::OutputDecode)?;
        let ids = bytemuck::try_cast_slice::<u8, u32>(&id_bytes)
            .map_err(|_| TiledGpuError::OutputDecode)?;
        let mut ordered = Vec::new();
        ordered
            .try_reserve_exact(entry_count as usize)
            .map_err(|_| TiledGpuError::OutOfMemory)?;
        for &id in ids {
            let entry = entries
                .get(id as usize)
                .copied()
                .ok_or(TiledGpuError::OutputDecode)?;
            ordered.push(entry);
        }
        Ok(ordered)
    }

    fn read_image(
        &self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<TiledRgbaImage, TiledGpuError> {
        let byte_count = u64::from(self.padded_image_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or(TiledGpuError::AddressSpaceExceeded)?;
        let bytes = read_bytes(device, &self.image_readback, byte_count)?;
        let pixel_count = usize::try_from(u64::from(width) * u64::from(height))
            .map_err(|_| TiledGpuError::AddressSpaceExceeded)?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(pixel_count)
            .map_err(|_| TiledGpuError::OutOfMemory)?;
        for y in 0..height as usize {
            let row = &bytes[y * self.padded_image_bytes_per_row as usize
                ..y * self.padded_image_bytes_per_row as usize + width as usize * 8];
            for pixel in row.chunks_exact(8) {
                rgba.push([
                    f16_to_f32(u16::from_le_bytes([pixel[0], pixel[1]])),
                    f16_to_f32(u16::from_le_bytes([pixel[2], pixel[3]])),
                    f16_to_f32(u16::from_le_bytes([pixel[4], pixel[5]])),
                    f16_to_f32(u16::from_le_bytes([pixel[6], pixel[7]])),
                ]);
            }
        }
        if rgba.len() != pixel_count {
            return Err(TiledGpuError::OutputDecode);
        }
        Ok(TiledRgbaImage {
            width,
            height,
            rgba,
        })
    }
}

fn validate_device_limits(
    device: &wgpu::Device,
    scene: &ProjectedScene,
) -> Result<(), TiledGpuError> {
    if scene.width == 0 || scene.height == 0 {
        return Err(TiledGpuError::InvalidDimensions);
    }
    let limits = device.limits();
    if scene.width > limits.max_texture_dimension_2d
        || scene.height > limits.max_texture_dimension_2d
    {
        return Err(TiledGpuError::Unsupported(format!(
            "{}x{} output exceeds max texture dimension {}",
            scene.width, scene.height, limits.max_texture_dimension_2d
        )));
    }
    if limits.max_storage_buffers_per_shader_stage < 6 {
        return Err(TiledGpuError::Unsupported(format!(
            "work generation requires 6 storage buffers; device exposes {}",
            limits.max_storage_buffers_per_shader_stage
        )));
    }
    if limits.max_compute_invocations_per_workgroup < SCAN_WORKGROUP_SIZE
        || limits.max_compute_workgroup_size_x < SCAN_WORKGROUP_SIZE
        || limits.max_compute_workgroup_size_y < 16
        || limits.max_compute_workgroup_storage_size < SCAN_WORKGROUP_STORAGE_BYTES
    {
        return Err(TiledGpuError::Unsupported(format!(
            "requires 256 compute invocations, 256x16 workgroup dimensions, and {SCAN_WORKGROUP_STORAGE_BYTES} bytes workgroup storage"
        )));
    }
    Ok(())
}

fn validate_binding(
    device: &wgpu::Device,
    resource: &'static str,
    required_bytes: u64,
) -> Result<(), TiledGpuError> {
    let limits = device.limits();
    let limit_bytes = limits
        .max_buffer_size
        .min(u64::from(limits.max_storage_buffer_binding_size));
    if required_bytes > limit_bytes {
        return Err(TiledGpuError::CapacityExceeded {
            resource,
            required_bytes,
            limit_bytes,
        });
    }
    Ok(())
}

fn create_scoped<T>(device: &wgpu::Device, create: impl FnOnce() -> T) -> Result<T, TiledGpuError> {
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let result = create();
    let internal_error = pollster::block_on(internal.pop());
    let oom_error = pollster::block_on(oom.pop());
    let validation_error = pollster::block_on(validation.pop());
    if oom_error.is_some() {
        return Err(TiledGpuError::OutOfMemory);
    }
    if let Some(error) = validation_error {
        return Err(TiledGpuError::Validation(error.to_string()));
    }
    if let Some(error) = internal_error {
        return Err(TiledGpuError::Internal(error.to_string()));
    }
    Ok(result)
}

fn storage_buffer(
    device: &wgpu::Device,
    label: &'static str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(4),
        usage,
        mapped_at_creation: false,
    })
}

fn uniform_buffer<T: Pod>(device: &wgpu::Device, label: &'static str, value: &T) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::bytes_of(value),
        usage: wgpu::BufferUsages::UNIFORM,
    })
}

fn readback_buffer(device: &wgpu::Device, label: &'static str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(4),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn bind_group(
    device: &wgpu::Device,
    pipeline: &wgpu::ComputePipeline,
    label: &'static str,
    entries: &[wgpu::BindGroupEntry<'_>],
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout: &pipeline.get_bind_group_layout(0),
        entries,
    })
}

fn buffer_entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn encode_compute(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    dispatch: Dispatch2d,
    label: &'static str,
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some(label),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.dispatch_workgroups(dispatch.x, dispatch.y, 1);
}

fn encode_compute_xy(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    x: u32,
    y: u32,
    label: &'static str,
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some(label),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.dispatch_workgroups(x, y, 1);
}

fn read_pod<T: Pod + Copy>(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
) -> Result<T, TiledGpuError> {
    let bytes = read_bytes(device, buffer, size_of::<T>() as u64)?;
    bytemuck::try_from_bytes::<T>(&bytes)
        .copied()
        .map_err(|_| TiledGpuError::OutputDecode)
}

fn read_bytes(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
    size: u64,
) -> Result<Vec<u8>, TiledGpuError> {
    let slice = buffer.slice(0..size);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|_| TiledGpuError::Readback)?;
    receiver
        .recv()
        .map_err(|_| TiledGpuError::Readback)?
        .map_err(|_| TiledGpuError::Readback)?;
    let bytes = slice.get_mapped_range().to_vec();
    buffer.unmap();
    Ok(bytes)
}

fn f16_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x03ff);
    let value = match exponent {
        0 => {
            if mantissa == 0 {
                sign
            } else {
                let leading = mantissa.leading_zeros() - 22;
                let normalized_mantissa = (mantissa << (leading + 1)) & 0x03ff;
                let f32_exponent = 127_u32 - 15 - leading;
                sign | (f32_exponent << 23) | (normalized_mantissa << 13)
            }
        }
        0x1f => sign | 0x7f80_0000 | (mantissa << 13),
        _ => sign | ((exponent + (127 - 15)) << 23) | (mantissa << 13),
    };
    f32::from_bits(value)
}
