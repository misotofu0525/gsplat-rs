//! Production-oriented exact tiled raster owner for compact Resident scenes.
//!
//! Existing CPU or GPU depth sorting remains authoritative. Its BTF source-ID
//! buffer is traversed in reverse to create deterministic FTB tile entries;
//! one stable tile-ID radix then makes every tile contiguous without changing
//! depth/tie order. Resident geometry and resolved SH color stay in their
//! existing planes.

use std::mem::size_of;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc;
#[cfg(target_arch = "wasm32")]
use std::sync::{Arc, Mutex};

use bytemuck::{Pod, Zeroable};
use thiserror::Error;
use wgpu::util::DeviceExt;

use crate::resident_gpu::ResidentGpuResources;

const PROJECT_WORKGROUP_SIZE: u32 = 128;
const SORT_WORKGROUP_SIZE: u32 = 128;
const SORT_ITEMS_PER_THREAD: u32 = 8;
const SORT_ITEMS_PER_GROUP: u32 = SORT_WORKGROUP_SIZE * SORT_ITEMS_PER_THREAD;
const RADIX: u32 = 16;
const RADIX_PASSES: u32 = 8;
const SCAN_WORKGROUP_SIZE: u32 = 256;
const SCAN_ITEMS_PER_GROUP: u32 = SCAN_WORKGROUP_SIZE * 2;
const SCAN_STORAGE_BYTES: u32 = SCAN_ITEMS_PER_GROUP * 4;
const PHASE_QUERY_COUNT: u32 = 12;
const PHASE_QUERY_BYTES: u64 = PHASE_QUERY_COUNT as u64 * size_of::<u64>() as u64;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TiledPhaseTimings {
    pub project_ms: Option<f32>,
    pub count_scan_ms: Option<f32>,
    pub scatter_tile_scan_ms: Option<f32>,
    pub tile_radix_ms: Option<f32>,
    pub raster_ms: Option<f32>,
    pub blit_ms: Option<f32>,
    pub raw_ticks: [u64; 12],
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResidentTiledError {
    #[error("resident tiled raster dimensions are invalid")]
    InvalidDimensions,
    #[error("resident tiled raster exceeds u32 addressing")]
    AddressSpaceExceeded,
    #[error(
        "resident tiled resource {resource} needs {required_bytes} bytes; binding limit is {limit_bytes} bytes"
    )]
    BindingLimitExceeded {
        resource: &'static str,
        required_bytes: u64,
        limit_bytes: u64,
    },
    #[error("resident tiled raster device limits are unsupported: {0}")]
    Unsupported(String),
    #[error("resident tiled entry count overflowed u32")]
    EntryCountOverflow,
    #[error("resident tiled scatter did not publish the complete entry set")]
    IncompleteScatter,
    #[error("resident tiled allocation failed")]
    OutOfMemory,
    #[error("resident tiled validation failed: {0}")]
    Validation(String),
    #[error("resident tiled backend failed: {0}")]
    Internal(String),
    #[error("resident tiled GPU readback failed")]
    Readback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceRasterExecutionPlan {
    GlobalQuads,
    ProjectedQuadsExact,
    TiledExact,
}

/// Per-frame inputs consumed by the exact scatter/raster phase after its
/// count has been resolved and capacity prepared.
pub(crate) struct ResidentTiledFinish<'a> {
    pub(crate) source_order_btf: &'a wgpu::Buffer,
    pub(crate) work_count: u32,
    pub(crate) target: &'a wgpu::TextureView,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TiledParams {
    width: u32,
    height: u32,
    tiles_x: u32,
    tiles_y: u32,
    source_count: u32,
    work_count: u32,
    tile_count: u32,
    entry_capacity: u32,
    entry_count: u32,
    _pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
struct Status {
    total_entries: u32,
    overflow: u32,
    written_entries: u32,
    reserved: u32,
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
    _pad: [u32; 3],
}

#[derive(Clone, Copy)]
struct Dispatch2d {
    x: u32,
    y: u32,
}

impl Dispatch2d {
    fn new(count: u32, limit: u32) -> Result<Self, ResidentTiledError> {
        if limit == 0 {
            return Err(ResidentTiledError::Unsupported(
                "zero compute dispatch limit".into(),
            ));
        }
        let count = count.max(1);
        let x = count.min(limit);
        let y = count.div_ceil(x);
        if y > limit {
            return Err(ResidentTiledError::Unsupported(format!(
                "{count} logical workgroups exceed {limit}x{limit}"
            )));
        }
        Ok(Self { x, y })
    }
}

struct Pipelines {
    project: wgpu::ComputePipeline,
    count: wgpu::ComputePipeline,
    copy_work_counts: wgpu::ComputePipeline,
    finalize_count: wgpu::ComputePipeline,
    scatter: wgpu::ComputePipeline,
    copy_tile_counts: wgpu::ComputePipeline,
    finalize_tiles: wgpu::ComputePipeline,
    init_tile_keys: wgpu::ComputePipeline,
    histogram: wgpu::ComputePipeline,
    scatter_radix: wgpu::ComputePipeline,
    scan_blocks: wgpu::ComputePipeline,
    scan_add: wgpu::ComputePipeline,
    scan_layout: wgpu::BindGroupLayout,
    raster: wgpu::ComputePipeline,
    blit: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
}

struct PhaseQueries {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
}

impl Pipelines {
    fn create(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let project_shader = shader(
            device,
            "gsplat-resident-tiled-project-shader",
            include_str!("../shaders/tiled_resident_project.wgsl"),
        );
        let work_shader = shader(
            device,
            "gsplat-resident-tiled-work-shader",
            include_str!("../shaders/tiled_resident_work_generation.wgsl"),
        );
        let sort_shader = shader(
            device,
            "gsplat-resident-tiled-sort-shader",
            include_str!("../shaders/tiled_resident_sort.wgsl"),
        );
        let scan_shader = shader(
            device,
            "gsplat-resident-tiled-scan-shader",
            include_str!("../shaders/gpu_prefix_scan.wgsl"),
        );
        let raster_shader = shader(
            device,
            "gsplat-resident-tiled-raster-shader",
            include_str!("../shaders/tiled_resident_raster.wgsl"),
        );
        let blit_shader = shader(
            device,
            "gsplat-resident-tiled-blit-shader",
            include_str!("../shaders/tiled_blit.wgsl"),
        );
        let scan_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gsplat-resident-tiled-scan-bgl"),
            entries: &[
                storage_layout(0, false),
                storage_layout(1, false),
                uniform_layout(2),
            ],
        });
        let scan_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gsplat-resident-tiled-scan-layout"),
            bind_group_layouts: &[&scan_layout],
            immediate_size: 0,
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gsplat-resident-tiled-blit-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gsplat-resident-tiled-blit-layout"),
            bind_group_layouts: &[&blit_layout],
            immediate_size: 0,
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gsplat-resident-tiled-blit-pipeline"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            project: compute(device, &project_shader, None, "project_resident"),
            count: compute(device, &work_shader, None, "count_entries"),
            copy_work_counts: compute(device, &work_shader, None, "copy_work_counts"),
            finalize_count: compute(device, &work_shader, None, "finalize_entry_count"),
            scatter: compute(device, &work_shader, None, "scatter_entries"),
            copy_tile_counts: compute(device, &work_shader, None, "copy_tile_counts"),
            finalize_tiles: compute(device, &work_shader, None, "finalize_tile_offsets"),
            init_tile_keys: compute(device, &sort_shader, None, "init_tile_keys"),
            histogram: compute(device, &sort_shader, None, "histogram"),
            scatter_radix: compute(device, &sort_shader, None, "scatter"),
            scan_blocks: compute(
                device,
                &scan_shader,
                Some(&scan_pipeline_layout),
                "scan_blocks",
            ),
            scan_add: compute(
                device,
                &scan_shader,
                Some(&scan_pipeline_layout),
                "add_block_offsets",
            ),
            scan_layout,
            raster: compute(device, &raster_shader, None, "rasterize_tile"),
            blit,
            blit_layout,
        }
    }
}

struct ScanLevel {
    bind_group: wgpu::BindGroup,
    dispatch: Dispatch2d,
}

struct Scan {
    _sums: Vec<wgpu::Buffer>,
    _params: Vec<wgpu::Buffer>,
    levels: Vec<ScanLevel>,
}

impl Scan {
    fn create(
        device: &wgpu::Device,
        pipelines: &Pipelines,
        root: &wgpu::Buffer,
        count: u32,
        dispatch_limit: u32,
    ) -> Result<Self, ResidentTiledError> {
        if count == 0 {
            return Ok(Self {
                _sums: Vec::new(),
                _params: Vec::new(),
                levels: Vec::new(),
            });
        }
        let mut level_counts = Vec::new();
        let mut level_count = count;
        loop {
            let groups = level_count.div_ceil(SCAN_ITEMS_PER_GROUP).max(1);
            level_counts.push((level_count, groups));
            if groups == 1 {
                break;
            }
            level_count = groups;
        }
        let sums = level_counts
            .iter()
            .map(|&(_, groups)| {
                buffer(
                    device,
                    "gsplat-resident-tiled-scan-sums",
                    u64::from(groups) * 4,
                    wgpu::BufferUsages::STORAGE,
                )
            })
            .collect::<Vec<_>>();
        let params = level_counts
            .iter()
            .map(|&(count, _)| {
                uniform(
                    device,
                    "gsplat-resident-tiled-scan-params",
                    &ScanParams {
                        count,
                        _pad: [0; 3],
                    },
                )
            })
            .collect::<Vec<_>>();
        let mut levels = Vec::new();
        for (index, &(_, groups)) in level_counts.iter().enumerate() {
            let data = if index == 0 { root } else { &sums[index - 1] };
            levels.push(ScanLevel {
                bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("gsplat-resident-tiled-scan-bg"),
                    layout: &pipelines.scan_layout,
                    entries: &[
                        entry(0, data),
                        entry(1, &sums[index]),
                        entry(2, &params[index]),
                    ],
                }),
                dispatch: Dispatch2d::new(groups, dispatch_limit)?,
            });
        }
        Ok(Self {
            _sums: sums,
            _params: params,
            levels,
        })
    }

    fn encode(&self, encoder: &mut wgpu::CommandEncoder, pipelines: &Pipelines) {
        for level in &self.levels {
            dispatch(
                encoder,
                &pipelines.scan_blocks,
                &level.bind_group,
                level.dispatch,
                "gsplat-resident-tiled-scan-pass",
            );
        }
        if self.levels.len() > 1 {
            for level in self.levels[..self.levels.len() - 1].iter().rev() {
                dispatch(
                    encoder,
                    &pipelines.scan_add,
                    &level.bind_group,
                    level.dispatch,
                    "gsplat-resident-tiled-scan-add-pass",
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

struct EntryResources {
    capacity: u32,
    entries: wgpu::Buffer,
    tile_counts: wgpu::Buffer,
    tile_offsets: wgpu::Buffer,
    tile_scan: Scan,
    keys_a: wgpu::Buffer,
    _keys_b: wgpu::Buffer,
    ids_a: wgpu::Buffer,
    ids_b: wgpu::Buffer,
    _prefix: wgpu::Buffer,
    prefix_scan: Scan,
    _pass_params: Vec<wgpu::Buffer>,
    radix_passes: Vec<RadixPass>,
    radix_dispatch: Dispatch2d,
    pass_count: usize,
}

impl EntryResources {
    fn create(
        device: &wgpu::Device,
        pipelines: &Pipelines,
        capacity: u32,
        tile_count: u32,
        dispatch_limit: u32,
    ) -> Result<Self, ResidentTiledError> {
        let allocation_count = capacity.max(1);
        let entry_bytes = u64::from(allocation_count) * 8;
        let word_bytes = u64::from(allocation_count) * 4;
        validate_binding(device, "tile entries", entry_bytes)?;
        validate_binding(device, "tile radix", word_bytes)?;
        let entries = buffer(
            device,
            "gsplat-resident-tiled-entries",
            entry_bytes,
            wgpu::BufferUsages::STORAGE,
        );
        let tile_counts = buffer(
            device,
            "gsplat-resident-tiled-tile-counts",
            u64::from(tile_count.max(1)) * 4,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let tile_offset_count = tile_count
            .checked_add(1)
            .ok_or(ResidentTiledError::AddressSpaceExceeded)?;
        let tile_offsets = buffer(
            device,
            "gsplat-resident-tiled-tile-offsets",
            u64::from(tile_offset_count) * 4,
            wgpu::BufferUsages::STORAGE,
        );
        let tile_scan = Scan::create(device, pipelines, &tile_offsets, tile_count, dispatch_limit)?;
        let usage = wgpu::BufferUsages::STORAGE;
        let keys_a = buffer(device, "gsplat-resident-tiled-keys-a", word_bytes, usage);
        let keys_b = buffer(device, "gsplat-resident-tiled-keys-b", word_bytes, usage);
        let ids_a = buffer(device, "gsplat-resident-tiled-ids-a", word_bytes, usage);
        let ids_b = buffer(device, "gsplat-resident-tiled-ids-b", word_bytes, usage);
        let group_count = allocation_count.div_ceil(SORT_ITEMS_PER_GROUP).max(1);
        let pass_count = radix_pass_count(tile_count);
        let prefix_count = group_count
            .checked_mul(RADIX)
            .ok_or(ResidentTiledError::AddressSpaceExceeded)?;
        validate_binding(device, "tile radix prefix", u64::from(prefix_count) * 4)?;
        let prefix = buffer(
            device,
            "gsplat-resident-tiled-prefix",
            u64::from(prefix_count) * 4,
            wgpu::BufferUsages::STORAGE,
        );
        let prefix_scan = Scan::create(device, pipelines, &prefix, prefix_count, dispatch_limit)?;
        // `group_count` deliberately describes the high-water allocation,
        // while `count` is rewritten to the exact active entry count for each
        // frame. Empty high-water groups publish zero histograms, so one
        // stable radix layout can be retained until capacity grows.
        let pass_params = (0..RADIX_PASSES)
            .map(|pass| {
                uniform_copy_dst(
                    device,
                    "gsplat-resident-tiled-pass",
                    &PassParams {
                        shift: pass * 4,
                        count: 0,
                        group_count,
                        _pad: 0,
                    },
                )
            })
            .collect::<Vec<_>>();
        let histogram_layout = pipelines.histogram.get_bind_group_layout(0);
        let scatter_layout = pipelines.scatter_radix.get_bind_group_layout(0);
        let mut radix_passes = Vec::new();
        for (pass, params) in pass_params.iter().enumerate() {
            let (keys_src, keys_dst, ids_src, ids_dst) = if pass % 2 == 0 {
                (&keys_a, &keys_b, &ids_a, &ids_b)
            } else {
                (&keys_b, &keys_a, &ids_b, &ids_a)
            };
            radix_passes.push(RadixPass {
                histogram: device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("gsplat-resident-tiled-histogram-bg"),
                    layout: &histogram_layout,
                    entries: &[entry(4, keys_src), entry(7, &prefix), entry(8, params)],
                }),
                scatter_keys: radix_bg(
                    device,
                    &scatter_layout,
                    keys_src,
                    keys_src,
                    keys_dst,
                    &prefix,
                    params,
                ),
                scatter_ids: radix_bg(
                    device,
                    &scatter_layout,
                    keys_src,
                    ids_src,
                    ids_dst,
                    &prefix,
                    params,
                ),
            });
        }
        Ok(Self {
            capacity,
            entries,
            tile_counts,
            tile_offsets,
            tile_scan,
            keys_a,
            _keys_b: keys_b,
            ids_a,
            ids_b,
            _prefix: prefix,
            prefix_scan,
            _pass_params: pass_params,
            radix_passes,
            radix_dispatch: Dispatch2d::new(group_count, dispatch_limit)?,
            pass_count,
        })
    }

    fn set_active_count(&self, queue: &wgpu::Queue, count: u32) {
        debug_assert!(count <= self.capacity);
        let group_count = self.capacity.max(1).div_ceil(SORT_ITEMS_PER_GROUP).max(1);
        for (pass, params) in self._pass_params.iter().enumerate() {
            queue.write_buffer(
                params,
                0,
                bytemuck::bytes_of(&PassParams {
                    shift: u32::try_from(pass).unwrap_or(0) * 4,
                    count,
                    group_count,
                    _pad: 0,
                }),
            );
        }
    }

    fn encode_radix(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipelines: &Pipelines,
        end_timestamp: Option<(&wgpu::QuerySet, Option<u32>, Option<u32>)>,
    ) {
        for (index, pass) in self.radix_passes.iter().take(self.pass_count).enumerate() {
            dispatch(
                encoder,
                &pipelines.histogram,
                &pass.histogram,
                self.radix_dispatch,
                "gsplat-resident-tiled-histogram-pass",
            );
            self.prefix_scan.encode(encoder, pipelines);
            dispatch(
                encoder,
                &pipelines.scatter_radix,
                &pass.scatter_keys,
                self.radix_dispatch,
                "gsplat-resident-tiled-scatter-keys-pass",
            );
            if index + 1 == self.pass_count {
                dispatch_timestamped(
                    encoder,
                    &pipelines.scatter_radix,
                    &pass.scatter_ids,
                    self.radix_dispatch,
                    "gsplat-resident-tiled-scatter-ids-pass",
                    end_timestamp,
                );
            } else {
                dispatch(
                    encoder,
                    &pipelines.scatter_radix,
                    &pass.scatter_ids,
                    self.radix_dispatch,
                    "gsplat-resident-tiled-scatter-ids-pass",
                );
            }
        }
    }

    fn sorted_ids(&self) -> &wgpu::Buffer {
        if self.pass_count.is_multiple_of(2) {
            &self.ids_a
        } else {
            &self.ids_b
        }
    }
}

/// Long-lived exact tiled renderer for a compact [`ResidentGpuResources`]
/// scene.
///
/// Counting and allocation are intentionally a separate submission from
/// scatter/raster. The count readback is the hard correctness boundary: the
/// second phase is never submitted until every required tile entry fits. A
/// retained high-water allocation avoids reallocating when the count later
/// shrinks, but it is never treated as permission to truncate a larger frame.
pub struct ResidentTiledGpu {
    pipelines: Pipelines,
    source_count: u32,
    width: u32,
    height: u32,
    tiles_x: u32,
    tiles_y: u32,
    tile_count: u32,
    dispatch_limit: u32,
    source_dispatch: Dispatch2d,
    tile_dispatch: Dispatch2d,
    projected_center: wgpu::Buffer,
    projected_conic: wgpu::Buffer,
    projected_bbox: wgpu::Buffer,
    work_counts: wgpu::Buffer,
    work_offsets: wgpu::Buffer,
    work_scan: Scan,
    params: wgpu::Buffer,
    status: wgpu::Buffer,
    status_readback: wgpu::Buffer,
    // Retains the texture behind `output_view`; browser transactional resize
    // intentionally rejects this diagnostic raster path.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    output_texture: wgpu::Texture,
    output_view: wgpu::TextureView,
    blit_bind_group: wgpu::BindGroup,
    entries: Option<EntryResources>,
    active_entry_count: u32,
    pending_work_count: Option<u32>,
    phase_queries: Option<PhaseQueries>,
    encoder_timestamps: bool,
    #[cfg(target_arch = "wasm32")]
    web_readback: WebReadbackState,
}

#[cfg(target_arch = "wasm32")]
enum WebReadbackState {
    Idle,
    Pending(Arc<Mutex<Option<bool>>>),
}

impl ResidentTiledGpu {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        resident: &ResidentGpuResources,
        width: u32,
        height: u32,
    ) -> Result<Self, ResidentTiledError> {
        validate_device(device, resident.capacity, width, height)?;
        let source_count = u32::try_from(resident.capacity)
            .map_err(|_| ResidentTiledError::AddressSpaceExceeded)?;
        let dispatch_limit = device.limits().max_compute_workgroups_per_dimension;
        let source_dispatch = Dispatch2d::new(
            source_count.div_ceil(PROJECT_WORKGROUP_SIZE).max(1),
            dispatch_limit,
        )?;
        let (tiles_x, tiles_y, tile_count) = tile_dimensions(width, height)?;
        let tile_dispatch = Dispatch2d::new(
            tile_count.div_ceil(PROJECT_WORKGROUP_SIZE).max(1),
            dispatch_limit,
        )?;
        let pipelines = create_scoped(device, || Pipelines::create(device, surface_format))?;

        let plane_bytes = u64::from(source_count.max(1)) * 16;
        let word_bytes = u64::from(source_count.max(1)) * 4;
        validate_binding(device, "projected center", plane_bytes)?;
        validate_binding(device, "projected conic", plane_bytes)?;
        validate_binding(device, "projected bbox", plane_bytes)?;
        validate_binding(device, "per-source tile counts", word_bytes)?;

        let (
            projected_center,
            projected_conic,
            projected_bbox,
            work_counts,
            work_offsets,
            params,
            status,
            status_readback,
            output_texture,
            output_view,
            blit_bind_group,
        ) = create_scoped(device, || {
            let projected_center = buffer(
                device,
                "gsplat-resident-tiled-projected-center",
                plane_bytes,
                wgpu::BufferUsages::STORAGE,
            );
            let projected_conic = buffer(
                device,
                "gsplat-resident-tiled-projected-conic",
                plane_bytes,
                wgpu::BufferUsages::STORAGE,
            );
            let projected_bbox = buffer(
                device,
                "gsplat-resident-tiled-projected-bbox",
                plane_bytes,
                wgpu::BufferUsages::STORAGE,
            );
            let work_counts = buffer(
                device,
                "gsplat-resident-tiled-work-counts",
                word_bytes,
                wgpu::BufferUsages::STORAGE,
            );
            let work_offsets = buffer(
                device,
                "gsplat-resident-tiled-work-offsets",
                word_bytes,
                wgpu::BufferUsages::STORAGE,
            );
            let params = uniform_copy_dst(
                device,
                "gsplat-resident-tiled-params",
                &TiledParams::zeroed(),
            );
            let status = buffer(
                device,
                "gsplat-resident-tiled-status",
                size_of::<Status>() as u64,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            );
            let status_readback = buffer(
                device,
                "gsplat-resident-tiled-status-readback",
                size_of::<Status>() as u64,
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            );
            let (output_texture, output_view, blit_bind_group) =
                create_output(device, &pipelines, width, height);
            (
                projected_center,
                projected_conic,
                projected_bbox,
                work_counts,
                work_offsets,
                params,
                status,
                status_readback,
                output_texture,
                output_view,
                blit_bind_group,
            )
        })?;
        let work_scan = create_scoped(device, || {
            Scan::create(
                device,
                &pipelines,
                &work_offsets,
                source_count,
                dispatch_limit,
            )
        })??;
        let phase_queries = device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
            .then(|| PhaseQueries {
                set: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("gsplat-resident-tiled-phase-queries"),
                    ty: wgpu::QueryType::Timestamp,
                    count: PHASE_QUERY_COUNT,
                }),
                resolve: buffer(
                    device,
                    "gsplat-resident-tiled-phase-query-resolve",
                    PHASE_QUERY_BYTES,
                    wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                ),
                readback: buffer(
                    device,
                    "gsplat-resident-tiled-phase-query-readback",
                    PHASE_QUERY_BYTES,
                    wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                ),
            });
        let encoder_timestamps = device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);

        Ok(Self {
            pipelines,
            source_count,
            width,
            height,
            tiles_x,
            tiles_y,
            tile_count,
            dispatch_limit,
            source_dispatch,
            tile_dispatch,
            projected_center,
            projected_conic,
            projected_bbox,
            work_counts,
            work_offsets,
            work_scan,
            params,
            status,
            status_readback,
            output_texture,
            output_view,
            blit_bind_group,
            entries: None,
            active_entry_count: 0,
            pending_work_count: None,
            phase_queries,
            encoder_timestamps,
            #[cfg(target_arch = "wasm32")]
            web_readback: WebReadbackState::Idle,
        })
    }

    #[cfg(test)]
    pub const fn execution_plan(&self) -> SurfaceRasterExecutionPlan {
        SurfaceRasterExecutionPlan::TiledExact
    }

    pub const fn entry_capacity(&self) -> u32 {
        match &self.entries {
            Some(entries) => entries.capacity,
            None => 0,
        }
    }

    pub const fn active_entry_count(&self) -> u32 {
        self.active_entry_count
    }

    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_phase_timings_blocking(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<Option<TiledPhaseTimings>, ResidentTiledError> {
        let Some(queries) = &self.phase_queries else {
            return Ok(None);
        };
        let bytes = read_buffer_blocking(device, &queries.readback, PHASE_QUERY_BYTES)?;
        let timestamps = bytemuck::try_cast_slice::<u8, u64>(&bytes)
            .map_err(|_| ResidentTiledError::Readback)?;
        if timestamps.len() != PHASE_QUERY_COUNT as usize {
            return Err(ResidentTiledError::Readback);
        }
        let period = queue.get_timestamp_period();
        let mut raw_ticks = [0_u64; PHASE_QUERY_COUNT as usize];
        raw_ticks.copy_from_slice(timestamps);
        Ok(Some(decode_phase_timings(raw_ticks, period)))
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<(), ResidentTiledError> {
        validate_dimensions(device, width, height)?;
        let (tiles_x, tiles_y, tile_count) = tile_dimensions(width, height)?;
        let tile_dispatch = Dispatch2d::new(
            tile_count.div_ceil(PROJECT_WORKGROUP_SIZE).max(1),
            self.dispatch_limit,
        )?;
        let (output_texture, output_view, blit_bind_group) = create_scoped(device, || {
            create_output(device, &self.pipelines, width, height)
        })?;
        self.width = width;
        self.height = height;
        self.tiles_x = tiles_x;
        self.tiles_y = tiles_y;
        self.tile_count = tile_count;
        self.tile_dispatch = tile_dispatch;
        self.output_texture = output_texture;
        self.output_view = output_view;
        self.blit_bind_group = blit_bind_group;
        // Tile-offset storage is viewport-shaped. Drop it transactionally only
        // after the replacement output resources have been created.
        self.entries = None;
        self.active_entry_count = 0;
        self.pending_work_count = None;
        #[cfg(target_arch = "wasm32")]
        {
            self.web_readback = WebReadbackState::Idle;
        }
        Ok(())
    }

    /// Encode resident projection and exact tile-entry counting.
    ///
    /// `source_order_btf` must be the authoritative stable back-to-front
    /// source-ID order produced by the selected CPU or GPU sorter. This method
    /// reverses that rank only when generating per-tile front-to-back work.
    pub fn encode_count_prepass(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        resident: &ResidentGpuResources,
        source_order_btf: &wgpu::Buffer,
        work_count: u32,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), ResidentTiledError> {
        if resident.capacity != self.source_count as usize || work_count > self.source_count {
            return Err(ResidentTiledError::AddressSpaceExceeded);
        }
        if self.pending_work_count.is_some() {
            return Err(ResidentTiledError::Internal(
                "previous tiled count prepass has not been resolved".into(),
            ));
        }
        let params = self.frame_params(work_count, 0, 0);
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(&self.status, 0, bytemuck::bytes_of(&Status::zeroed()));

        let project = bind_group(
            device,
            &self.pipelines.project,
            &[
                entry(0, &resident.position_alpha_buffer),
                entry(1, &resident.covariance0_buffer),
                entry(2, &resident.covariance1_buffer),
                entry(3, &resident.draw_params_buffer),
                entry(4, &self.projected_center),
                entry(5, &self.projected_conic),
                entry(6, &self.projected_bbox),
                entry(7, &self.params),
            ],
            "gsplat-resident-tiled-project-bg",
        );
        self.write_encoder_timestamp(encoder, 0);
        dispatch_timestamped(
            encoder,
            &self.pipelines.project,
            &project,
            self.source_dispatch,
            "gsplat-resident-tiled-project-pass",
            self.pass_timestamp(Some(0), Some(1)),
        );
        self.write_encoder_timestamp(encoder, 1);

        self.write_encoder_timestamp(encoder, 2);
        if work_count > 0 {
            let count = bind_group(
                device,
                &self.pipelines.count,
                &[
                    entry(0, &self.projected_center),
                    entry(1, &self.projected_conic),
                    entry(2, &self.projected_bbox),
                    entry(3, source_order_btf),
                    entry(4, &self.params),
                    entry(5, &self.work_counts),
                    entry(8, &self.status),
                ],
                "gsplat-resident-tiled-count-bg",
            );
            let copy = bind_group(
                device,
                &self.pipelines.copy_work_counts,
                &[
                    entry(4, &self.params),
                    entry(5, &self.work_counts),
                    entry(6, &self.work_offsets),
                ],
                "gsplat-resident-tiled-copy-work-bg",
            );
            dispatch_timestamped(
                encoder,
                &self.pipelines.count,
                &count,
                dispatch_for_items(work_count, PROJECT_WORKGROUP_SIZE, self.dispatch_limit)?,
                "gsplat-resident-tiled-count-pass",
                self.pass_timestamp(Some(2), None),
            );
            dispatch(
                encoder,
                &self.pipelines.copy_work_counts,
                &copy,
                dispatch_for_items(work_count, PROJECT_WORKGROUP_SIZE, self.dispatch_limit)?,
                "gsplat-resident-tiled-copy-work-pass",
            );
            self.work_scan.encode(encoder, &self.pipelines);
        }
        let finalize = bind_group(
            device,
            &self.pipelines.finalize_count,
            &[
                entry(4, &self.params),
                entry(5, &self.work_counts),
                entry(6, &self.work_offsets),
                entry(8, &self.status),
            ],
            "gsplat-resident-tiled-finalize-count-bg",
        );
        dispatch_timestamped(
            encoder,
            &self.pipelines.finalize_count,
            &finalize,
            Dispatch2d { x: 1, y: 1 },
            "gsplat-resident-tiled-finalize-count-pass",
            self.pass_timestamp((work_count == 0).then_some(2), Some(3)),
        );
        self.write_encoder_timestamp(encoder, 3);
        encoder.copy_buffer_to_buffer(
            &self.status,
            0,
            &self.status_readback,
            0,
            size_of::<Status>() as u64,
        );
        self.pending_work_count = Some(work_count);
        Ok(())
    }

    /// Resolve the prepass count and prepare a non-truncating high-water
    /// allocation. The caller must submit the encoder passed to
    /// [`Self::encode_count_prepass`] before calling this method.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn resolve_count_and_prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<u32, ResidentTiledError> {
        let _work_count = self.pending_work_count.take().ok_or_else(|| {
            ResidentTiledError::Internal("no tiled count prepass is pending".into())
        })?;
        let status = read_status_blocking(device, &self.status_readback)?;
        self.prepare_entry_count(device, queue, status)
    }

    fn prepare_entry_count(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        status: Status,
    ) -> Result<u32, ResidentTiledError> {
        if status.overflow != 0 {
            return Err(ResidentTiledError::EntryCountOverflow);
        }
        let entry_count = status.total_entries;
        let needs_growth = self
            .entries
            .as_ref()
            .is_none_or(|entries| entries.capacity < entry_count.max(1));
        if needs_growth {
            let previous = self.entry_capacity();
            let capacity = high_water_capacity(previous, entry_count);
            let replacement = create_scoped(device, || {
                EntryResources::create(
                    device,
                    &self.pipelines,
                    capacity,
                    self.tile_count,
                    self.dispatch_limit,
                )
            })??;
            self.entries = Some(replacement);
        }
        let entries = self
            .entries
            .as_ref()
            .ok_or(ResidentTiledError::OutOfMemory)?;
        entries.set_active_count(queue, entry_count);
        self.active_entry_count = entry_count;
        Ok(entry_count)
    }

    /// Non-blocking WebGPU count resolution. `Ok(None)` means the browser has
    /// not completed the mapped status copy yet; callers must keep the frame
    /// fail-closed and retry this method from a later event-loop turn.
    #[cfg(target_arch = "wasm32")]
    pub fn try_resolve_count_and_prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<Option<u32>, ResidentTiledError> {
        if self.pending_work_count.is_none() {
            return Err(ResidentTiledError::Internal(
                "no tiled count prepass is pending".into(),
            ));
        }
        if matches!(&self.web_readback, WebReadbackState::Idle) {
            let ready = Arc::new(Mutex::new(None));
            let callback_ready = Arc::clone(&ready);
            self.status_readback
                .slice(..size_of::<Status>() as u64)
                .map_async(wgpu::MapMode::Read, move |result| {
                    if let Ok(mut slot) = callback_ready.lock() {
                        *slot = Some(result.is_ok());
                    }
                });
            self.web_readback = WebReadbackState::Pending(ready);
            let _ = device.poll(wgpu::PollType::Poll);
            return Ok(None);
        }
        let WebReadbackState::Pending(ready) = &self.web_readback else {
            unreachable!();
        };
        let result = ready
            .lock()
            .map_err(|_| ResidentTiledError::Readback)?
            .take();
        let Some(success) = result else {
            let _ = device.poll(wgpu::PollType::Poll);
            return Ok(None);
        };
        self.web_readback = WebReadbackState::Idle;
        if !success {
            self.pending_work_count = None;
            return Err(ResidentTiledError::Readback);
        }
        let status = mapped_status(&self.status_readback)?;
        self.status_readback.unmap();
        let _ = self.pending_work_count.take();
        self.prepare_entry_count(device, queue, status).map(Some)
    }

    /// Encode deterministic scatter, stable tile grouping, exact FTB tile
    /// compositing into RGBA16F, and a final surface-format blit.
    pub fn encode_finish(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        resident: &ResidentGpuResources,
        frame: ResidentTiledFinish<'_>,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), ResidentTiledError> {
        let ResidentTiledFinish {
            source_order_btf,
            work_count,
            target,
        } = frame;
        if self.pending_work_count.is_some() {
            return Err(ResidentTiledError::Internal(
                "tiled count must be resolved before scatter".into(),
            ));
        }
        if work_count > self.source_count {
            return Err(ResidentTiledError::AddressSpaceExceeded);
        }
        let entry_count = self.active_entry_count;
        let entries = self
            .entries
            .as_ref()
            .ok_or(ResidentTiledError::OutOfMemory)?;
        if entry_count > entries.capacity {
            return Err(ResidentTiledError::IncompleteScatter);
        }
        queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&self.frame_params(work_count, entries.capacity, entry_count)),
        );
        queue.write_buffer(&self.status, 0, bytemuck::bytes_of(&Status::zeroed()));
        encoder.clear_buffer(&entries.tile_counts, 0, None);

        self.write_encoder_timestamp(encoder, 4);
        if work_count > 0 && entry_count > 0 {
            let scatter = bind_group(
                device,
                &self.pipelines.scatter,
                &[
                    entry(0, &self.projected_center),
                    entry(1, &self.projected_conic),
                    entry(2, &self.projected_bbox),
                    entry(3, source_order_btf),
                    entry(4, &self.params),
                    entry(6, &self.work_offsets),
                    entry(7, &entries.entries),
                    entry(8, &self.status),
                    entry(9, &entries.tile_counts),
                ],
                "gsplat-resident-tiled-scatter-bg",
            );
            dispatch_timestamped(
                encoder,
                &self.pipelines.scatter,
                &scatter,
                dispatch_for_items(work_count, PROJECT_WORKGROUP_SIZE, self.dispatch_limit)?,
                "gsplat-resident-tiled-scatter-pass",
                self.pass_timestamp(Some(4), None),
            );
        }

        let copy_tiles = bind_group(
            device,
            &self.pipelines.copy_tile_counts,
            &[
                entry(4, &self.params),
                entry(6, &entries.tile_offsets),
                entry(9, &entries.tile_counts),
            ],
            "gsplat-resident-tiled-copy-tiles-bg",
        );
        dispatch_timestamped(
            encoder,
            &self.pipelines.copy_tile_counts,
            &copy_tiles,
            self.tile_dispatch,
            "gsplat-resident-tiled-copy-tiles-pass",
            (!(work_count > 0 && entry_count > 0))
                .then(|| self.pass_timestamp(Some(4), None))
                .flatten(),
        );
        entries.tile_scan.encode(encoder, &self.pipelines);
        let finalize_tiles = bind_group(
            device,
            &self.pipelines.finalize_tiles,
            &[
                entry(4, &self.params),
                entry(6, &entries.tile_offsets),
                entry(8, &self.status),
            ],
            "gsplat-resident-tiled-finalize-tiles-bg",
        );
        dispatch_timestamped(
            encoder,
            &self.pipelines.finalize_tiles,
            &finalize_tiles,
            Dispatch2d { x: 1, y: 1 },
            "gsplat-resident-tiled-finalize-tiles-pass",
            self.pass_timestamp(None, Some(5)),
        );
        self.write_encoder_timestamp(encoder, 5);

        self.write_encoder_timestamp(encoder, 6);
        if entry_count > 0 {
            let init = bind_group(
                device,
                &self.pipelines.init_tile_keys,
                &[
                    entry(0, &entries.entries),
                    entry(1, &self.params),
                    entry(2, &entries.keys_a),
                    entry(3, &entries.ids_a),
                ],
                "gsplat-resident-tiled-init-keys-bg",
            );
            dispatch_timestamped(
                encoder,
                &self.pipelines.init_tile_keys,
                &init,
                entries.radix_dispatch,
                "gsplat-resident-tiled-init-keys-pass",
                self.pass_timestamp(Some(6), None),
            );
            entries.encode_radix(encoder, &self.pipelines, self.pass_timestamp(None, Some(7)));
        }
        self.write_encoder_timestamp(encoder, 7);

        let raster = device_bind_group_for_raster(
            device,
            &self.pipelines.raster,
            &self.projected_center,
            &self.projected_conic,
            &self.projected_bbox,
            &resident.resolved_color_buffer,
            &entries.entries,
            entries.sorted_ids(),
            &entries.tile_offsets,
            &self.params,
            &self.output_view,
        );
        self.write_encoder_timestamp(encoder, 8);
        dispatch_xy_timestamped(
            encoder,
            &self.pipelines.raster,
            &raster,
            self.tiles_x,
            self.tiles_y,
            "gsplat-resident-tiled-raster-pass",
            self.pass_timestamp(Some(8), Some(9)),
        );
        self.write_encoder_timestamp(encoder, 9);
        self.write_encoder_timestamp(encoder, 10);
        encode_blit(
            encoder,
            &self.pipelines.blit,
            &self.blit_bind_group,
            target,
            self.pass_timestamp(Some(10), Some(11)),
        );
        self.write_encoder_timestamp(encoder, 11);
        if let Some(queries) = &self.phase_queries {
            encoder.resolve_query_set(&queries.set, 0..PHASE_QUERY_COUNT, &queries.resolve, 0);
            encoder.copy_buffer_to_buffer(
                &queries.resolve,
                0,
                &queries.readback,
                0,
                PHASE_QUERY_BYTES,
            );
        }
        Ok(())
    }

    fn frame_params(&self, work_count: u32, entry_capacity: u32, entry_count: u32) -> TiledParams {
        TiledParams {
            width: self.width,
            height: self.height,
            tiles_x: self.tiles_x,
            tiles_y: self.tiles_y,
            source_count: self.source_count,
            work_count,
            tile_count: self.tile_count,
            entry_capacity,
            entry_count,
            _pad: [0; 3],
        }
    }

    fn write_encoder_timestamp(&self, encoder: &mut wgpu::CommandEncoder, index: u32) {
        if self.uses_encoder_timestamp(index)
            && let Some(queries) = &self.phase_queries
        {
            encoder.write_timestamp(&queries.set, index);
        }
    }

    const fn uses_encoder_timestamp(&self, index: u32) -> bool {
        // Apple tile GPUs cannot publish a render-pass end timestamp through
        // pass descriptors. Restrict arbitrary encoder writes to the final
        // raster/blit boundary; the earlier compute chain retains the stable
        // pass-descriptor path and avoids a Metal count-submit hang.
        self.encoder_timestamps && index >= 8
    }

    fn pass_timestamp(
        &self,
        begin: Option<u32>,
        end: Option<u32>,
    ) -> Option<(&wgpu::QuerySet, Option<u32>, Option<u32>)> {
        let encoder_begin = begin.is_none_or(|index| self.uses_encoder_timestamp(index));
        let encoder_end = end.is_none_or(|index| self.uses_encoder_timestamp(index));
        if (begin.is_some() || end.is_some()) && encoder_begin && encoder_end {
            None
        } else {
            self.phase_queries
                .as_ref()
                .map(|queries| (&queries.set, begin, end))
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_phase_timings(raw_ticks: [u64; 12], period_ns: f32) -> TiledPhaseTimings {
    let mut previous_end = 0_u64;
    let mut causal_chain_valid = period_ns.is_finite() && period_ns > 0.0;
    let mut elapsed = |begin: usize, end: usize| {
        let begin_tick = raw_ticks[begin];
        let end_tick = raw_ticks[end];
        if !causal_chain_valid
            || begin_tick == 0
            || end_tick < begin_tick
            || begin_tick < previous_end
        {
            // Query slots retain their previous value on some Metal paths
            // when a pass does not publish a timestamp. Once causality is
            // broken, that phase and every later phase are unavailable.
            causal_chain_valid = false;
            return None;
        }
        previous_end = end_tick;
        Some(((end_tick - begin_tick) as f64 * f64::from(period_ns) / 1.0e6) as f32)
    };
    TiledPhaseTimings {
        project_ms: elapsed(0, 1),
        count_scan_ms: elapsed(2, 3),
        scatter_tile_scan_ms: elapsed(4, 5),
        tile_radix_ms: elapsed(6, 7),
        raster_ms: elapsed(8, 9),
        blit_ms: elapsed(10, 11),
        raw_ticks,
    }
}

fn validate_dimensions(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> Result<(), ResidentTiledError> {
    if width == 0 || height == 0 {
        return Err(ResidentTiledError::InvalidDimensions);
    }
    let limit = device.limits().max_texture_dimension_2d;
    if width > limit || height > limit {
        return Err(ResidentTiledError::Unsupported(format!(
            "{width}x{height} output exceeds max texture dimension {limit}"
        )));
    }
    Ok(())
}

fn validate_device(
    device: &wgpu::Device,
    source_count: usize,
    width: u32,
    height: u32,
) -> Result<(), ResidentTiledError> {
    validate_dimensions(device, width, height)?;
    let _ = u32::try_from(source_count).map_err(|_| ResidentTiledError::AddressSpaceExceeded)?;
    let limits = device.limits();
    if limits.max_storage_buffers_per_shader_stage < 8 {
        return Err(ResidentTiledError::Unsupported(format!(
            "exact resident work generation requires 8 storage buffers; device exposes {}",
            limits.max_storage_buffers_per_shader_stage
        )));
    }
    if limits.max_compute_invocations_per_workgroup < SCAN_WORKGROUP_SIZE
        || limits.max_compute_workgroup_size_x < SCAN_WORKGROUP_SIZE
        || limits.max_compute_workgroup_size_y < 16
        || limits.max_compute_workgroup_storage_size < SCAN_STORAGE_BYTES
    {
        return Err(ResidentTiledError::Unsupported(format!(
            "requires 256 compute invocations, 256x16 dimensions, and {SCAN_STORAGE_BYTES} bytes of workgroup storage"
        )));
    }
    Ok(())
}

fn tile_dimensions(width: u32, height: u32) -> Result<(u32, u32, u32), ResidentTiledError> {
    let tiles_x = width.div_ceil(16);
    let tiles_y = height.div_ceil(16);
    let tile_count = tiles_x
        .checked_mul(tiles_y)
        .ok_or(ResidentTiledError::AddressSpaceExceeded)?;
    Ok((tiles_x, tiles_y, tile_count))
}

fn radix_pass_count(tile_count: u32) -> usize {
    let significant_bits = 32 - tile_count.saturating_sub(1).leading_zeros();
    significant_bits.max(1).div_ceil(4) as usize
}

fn dispatch_for_items(
    count: u32,
    items_per_group: u32,
    limit: u32,
) -> Result<Dispatch2d, ResidentTiledError> {
    Dispatch2d::new(count.div_ceil(items_per_group).max(1), limit)
}

fn validate_binding(
    device: &wgpu::Device,
    resource: &'static str,
    required_bytes: u64,
) -> Result<(), ResidentTiledError> {
    let limits = device.limits();
    let limit_bytes = limits
        .max_buffer_size
        .min(u64::from(limits.max_storage_buffer_binding_size));
    if required_bytes > limit_bytes {
        return Err(ResidentTiledError::BindingLimitExceeded {
            resource,
            required_bytes,
            limit_bytes,
        });
    }
    Ok(())
}

fn high_water_capacity(previous: u32, required: u32) -> u32 {
    // Retain the largest exact allocation observed. Growing to the exact new
    // requirement avoids turning alignment slack into an artificial device-
    // limit failure near max_storage_buffer_binding_size.
    previous.max(required.max(1))
}

fn shader(device: &wgpu::Device, label: &'static str, source: &'static str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

fn compute(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: Option<&wgpu::PipelineLayout>,
    entry_point: &'static str,
) -> wgpu::ComputePipeline {
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry_point),
        layout,
        module: shader,
        entry_point: Some(entry_point),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
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

fn uniform_layout(binding: u32) -> wgpu::BindGroupLayoutEntry {
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

fn buffer(
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

fn uniform<T: Pod>(device: &wgpu::Device, label: &'static str, value: &T) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::bytes_of(value),
        usage: wgpu::BufferUsages::UNIFORM,
    })
}

fn uniform_copy_dst<T: Pod>(device: &wgpu::Device, label: &'static str, value: &T) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::bytes_of(value),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn bind_group(
    device: &wgpu::Device,
    pipeline: &wgpu::ComputePipeline,
    entries: &[wgpu::BindGroupEntry<'_>],
    label: &'static str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout: &pipeline.get_bind_group_layout(0),
        entries,
    })
}

fn radix_bg(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    keys: &wgpu::Buffer,
    payload: &wgpu::Buffer,
    output: &wgpu::Buffer,
    prefix: &wgpu::Buffer,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gsplat-resident-tiled-radix-scatter-bg"),
        layout,
        entries: &[
            entry(4, keys),
            entry(5, payload),
            entry(6, output),
            entry(7, prefix),
            entry(8, params),
        ],
    })
}

fn create_output(
    device: &wgpu::Device,
    pipelines: &Pipelines,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gsplat-resident-tiled-rgba16f-output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gsplat-resident-tiled-blit-bg"),
        layout: &pipelines.blit_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
    });
    (texture, view, bind_group)
}

#[allow(clippy::too_many_arguments)]
fn device_bind_group_for_raster(
    device: &wgpu::Device,
    pipeline: &wgpu::ComputePipeline,
    projected_center: &wgpu::Buffer,
    projected_conic: &wgpu::Buffer,
    projected_bbox: &wgpu::Buffer,
    resolved_color: &wgpu::Buffer,
    entries: &wgpu::Buffer,
    sorted_ids: &wgpu::Buffer,
    tile_offsets: &wgpu::Buffer,
    params: &wgpu::Buffer,
    output: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gsplat-resident-tiled-raster-bg"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            entry(0, projected_center),
            entry(1, projected_conic),
            entry(2, projected_bbox),
            entry(3, resolved_color),
            entry(4, entries),
            entry(5, sorted_ids),
            entry(6, tile_offsets),
            entry(7, params),
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(output),
            },
        ],
    })
}

fn dispatch(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    groups: Dispatch2d,
    label: &'static str,
) {
    dispatch_timestamped(encoder, pipeline, bind_group, groups, label, None);
}

fn dispatch_timestamped(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    groups: Dispatch2d,
    label: &'static str,
    timestamps: Option<(&wgpu::QuerySet, Option<u32>, Option<u32>)>,
) {
    let timestamp_writes = timestamps.map(
        |(query_set, beginning_of_pass_write_index, end_of_pass_write_index)| {
            wgpu::ComputePassTimestampWrites {
                query_set,
                beginning_of_pass_write_index,
                end_of_pass_write_index,
            }
        },
    );
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some(label),
        timestamp_writes,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.dispatch_workgroups(groups.x, groups.y, 1);
}

#[allow(clippy::too_many_arguments)]
fn dispatch_xy_timestamped(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    x: u32,
    y: u32,
    label: &'static str,
    timestamps: Option<(&wgpu::QuerySet, Option<u32>, Option<u32>)>,
) {
    let timestamp_writes = timestamps.map(
        |(query_set, beginning_of_pass_write_index, end_of_pass_write_index)| {
            wgpu::ComputePassTimestampWrites {
                query_set,
                beginning_of_pass_write_index,
                end_of_pass_write_index,
            }
        },
    );
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some(label),
        timestamp_writes,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.dispatch_workgroups(x, y, 1);
}

fn encode_blit(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    target: &wgpu::TextureView,
    timestamps: Option<(&wgpu::QuerySet, Option<u32>, Option<u32>)>,
) {
    let timestamp_writes = timestamps.map(
        |(query_set, beginning_of_pass_write_index, end_of_pass_write_index)| {
            wgpu::RenderPassTimestampWrites {
                query_set,
                beginning_of_pass_write_index,
                end_of_pass_write_index,
            }
        },
    );
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("gsplat-resident-tiled-blit-pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        timestamp_writes,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

#[cfg(not(target_arch = "wasm32"))]
fn create_scoped<T>(
    device: &wgpu::Device,
    create: impl FnOnce() -> T,
) -> Result<T, ResidentTiledError> {
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let result = create();
    let internal_error = pollster::block_on(internal.pop());
    let oom_error = pollster::block_on(oom.pop());
    let validation_error = pollster::block_on(validation.pop());
    if oom_error.is_some() {
        return Err(ResidentTiledError::OutOfMemory);
    }
    if let Some(error) = validation_error {
        return Err(ResidentTiledError::Validation(error.to_string()));
    }
    if let Some(error) = internal_error {
        return Err(ResidentTiledError::Internal(error.to_string()));
    }
    Ok(result)
}

#[cfg(target_arch = "wasm32")]
fn create_scoped<T>(
    _device: &wgpu::Device,
    create: impl FnOnce() -> T,
) -> Result<T, ResidentTiledError> {
    // Surface construction already runs in the browser's async adapter/device
    // flow. Never attempt a blocking `pop_error_scope` on the Web event loop.
    // Execution remains fail-closed through the asynchronous count readback;
    // browser validation errors are reported by wgpu's configured handler.
    Ok(create())
}

#[cfg(not(target_arch = "wasm32"))]
fn read_status_blocking(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
) -> Result<Status, ResidentTiledError> {
    let bytes = read_buffer_blocking(device, buffer, size_of::<Status>() as u64)?;
    bytemuck::try_from_bytes::<Status>(&bytes)
        .copied()
        .map_err(|_| ResidentTiledError::Readback)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_buffer_blocking(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
    size: u64,
) -> Result<Vec<u8>, ResidentTiledError> {
    let slice = buffer.slice(..size);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|_| ResidentTiledError::Readback)?;
    receiver
        .recv()
        .map_err(|_| ResidentTiledError::Readback)?
        .map_err(|_| ResidentTiledError::Readback)?;
    let bytes = slice.get_mapped_range().to_vec();
    buffer.unmap();
    Ok(bytes)
}

#[cfg(target_arch = "wasm32")]
fn mapped_status(buffer: &wgpu::Buffer) -> Result<Status, ResidentTiledError> {
    let bytes = buffer
        .slice(..size_of::<Status>() as u64)
        .get_mapped_range();
    bytemuck::try_from_bytes::<Status>(&bytes)
        .copied()
        .map_err(|_| ResidentTiledError::Readback)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use gsplat_core::{Camera, CameraIntrinsics, CameraPose, SceneBuffers, Vec3f};

    use super::*;
    use crate::{
        ResidentSceneCpu,
        resident_gpu::{
            create_resident_color_bind_group_layout, create_resident_draw_bind_group_layout,
        },
    };

    #[test]
    fn phase_decoder_rejects_stale_nonzero_tail_queries() {
        let timings = decode_phase_timings(
            [100, 110, 120, 130, 140, 150, 160, 170, 90, 100, 110, 120],
            1.0,
        );
        assert!(timings.project_ms.is_some());
        assert!(timings.count_scan_ms.is_some());
        assert!(timings.scatter_tile_scan_ms.is_some());
        assert!(timings.tile_radix_ms.is_some());
        assert_eq!(timings.raster_ms, None);
        assert_eq!(timings.blit_ms, None);
    }

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
            limits.max_storage_buffers_per_shader_stage = 8;
            if !limits.check_limits(&adapter.limits()) {
                return None;
            }
            let adapter_features = adapter.features();
            let mut required_features = wgpu::Features::empty();
            if adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY) {
                required_features |= wgpu::Features::TIMESTAMP_QUERY;
                if !cfg!(any(target_os = "macos", target_os = "ios")) {
                    required_features |=
                        adapter_features & wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
                }
            }
            adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("resident-tiled-test-device"),
                    required_features,
                    required_limits: limits,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::Performance,
                    trace: wgpu::Trace::Off,
                })
                .await
                .ok()
        })
    }

    fn resident_scene() -> ResidentSceneCpu {
        ResidentSceneCpu::encode(&SceneBuffers {
            positions: vec![
                Vec3f::new(-0.08, 0.0, 2.0),
                Vec3f::new(0.08, 0.0, 2.5),
                Vec3f::new(0.0, 0.05, 3.0),
            ],
            opacity: vec![4.0, 3.0, 2.0],
            scale_xyz: vec![[-2.2; 3], [-2.0; 3], [-1.8; 3]],
            rotation_xyzw: vec![[0.0, 0.0, 0.0, 1.0]; 3],
            color_dc: vec![[1.0, -0.5, -0.5], [-0.5, 1.0, -0.5], [-0.5, -0.5, 1.0]],
            sh_degree: 0,
            sh_rest: None,
        })
        .expect("resident scene")
    }

    fn camera() -> Camera {
        Camera {
            pose: CameraPose {
                position: Vec3f::new(0.0, 0.0, 0.0),
                rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            },
            intrinsics: CameraIntrinsics {
                vertical_fov_radians: 60.0_f32.to_radians(),
                near_plane: 0.1,
                far_plane: 100.0,
            },
        }
    }

    #[test]
    fn tile_radix_uses_only_significant_base16_digits() {
        assert_eq!(radix_pass_count(0), 1);
        assert_eq!(radix_pass_count(1), 1);
        assert_eq!(radix_pass_count(16), 1);
        assert_eq!(radix_pass_count(17), 2);
        assert_eq!(radix_pass_count(256), 2);
        assert_eq!(radix_pass_count(257), 3);
        assert_eq!(radix_pass_count(920), 3);
        assert_eq!(radix_pass_count(4096), 3);
        assert_eq!(radix_pass_count(4097), 4);
        assert_eq!(radix_pass_count(u32::MAX), 8);
    }

    #[test]
    fn production_resident_pipelines_create_on_real_gpu() {
        let Some((device, _queue)) = test_device() else {
            return;
        };
        let scene = resident_scene();
        let draw_layout = create_resident_draw_bind_group_layout(&device);
        let color_layout = create_resident_color_bind_group_layout(&device);
        let resident = ResidentGpuResources::new(&device, &draw_layout, &color_layout, &scene)
            .expect("resident resources");
        let tiled =
            ResidentTiledGpu::new(&device, wgpu::TextureFormat::Rgba8Unorm, &resident, 96, 64)
                .expect("resident tiled pipelines");
        assert_eq!(
            tiled.execution_plan(),
            SurfaceRasterExecutionPlan::TiledExact
        );
        assert_eq!(tiled.entry_capacity(), 0);
    }

    #[test]
    fn resident_count_scatter_sort_and_raster_execute_without_truncation() {
        let Some((device, queue)) = test_device() else {
            return;
        };
        let scene = resident_scene();
        let draw_layout = create_resident_draw_bind_group_layout(&device);
        let color_layout = create_resident_color_bind_group_layout(&device);
        let resident = ResidentGpuResources::new(&device, &draw_layout, &color_layout, &scene)
            .expect("resident resources");
        let camera = camera();
        // A one-element visible order intentionally references source 2. The
        // projection pass must cover resident source capacity rather than
        // RenderParams.len (which is only the visible draw count).
        let order = [2_u32];
        resident
            .prepare_cpu_order(&queue, &order, &camera, 96, 64, true)
            .expect("CPU order upload");
        // Constant positive packed colors are sufficient for exercising the
        // exact raster path independently of SH resolve ownership.
        let packed_gray = [0x0002_0000_u32, (126_u32 << 22) | 0x0020_0008];
        for source in 0..3_u64 {
            queue.write_buffer(
                &resident.resolved_color_buffer,
                source * 8,
                bytemuck::cast_slice(&packed_gray),
            );
        }
        let mut tiled =
            ResidentTiledGpu::new(&device, wgpu::TextureFormat::Rgba8Unorm, &resident, 96, 64)
                .expect("resident tiled owner");
        let mut count_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-tiled-test-count"),
        });
        tiled
            .encode_count_prepass(
                &device,
                &queue,
                &resident,
                &resident.order_buffer,
                1,
                &mut count_encoder,
            )
            .expect("count prepass");
        queue.submit(Some(count_encoder.finish()));
        let entry_count = tiled
            .resolve_count_and_prepare(&device, &queue)
            .expect("exact entry allocation");
        assert!(entry_count > 0);
        assert!(tiled.entry_capacity() >= entry_count);

        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("resident-tiled-test-target"),
            size: wgpu::Extent3d {
                width: 96,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let padded_bytes_per_row = (96_u32 * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let target_readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-tiled-test-target-readback"),
            size: u64::from(padded_bytes_per_row) * 64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident-tiled-test-finish"),
        });
        tiled
            .encode_finish(
                &device,
                &queue,
                &resident,
                ResidentTiledFinish {
                    source_order_btf: &resident.order_buffer,
                    work_count: 1,
                    target: &target_view,
                },
                &mut encoder,
            )
            .expect("finish encode");
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &target_readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(64),
                },
            },
            wgpu::Extent3d {
                width: 96,
                height: 64,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU wait");
        assert!(pollster::block_on(validation.pop()).is_none());
        assert_eq!(tiled.active_entry_count(), entry_count);
        let target_bytes = read_buffer_blocking(
            &device,
            &target_readback,
            u64::from(padded_bytes_per_row) * 64,
        )
        .expect("target image readback");
        let row_bytes = usize::try_from(padded_bytes_per_row).expect("row byte count");
        assert!(target_bytes.chunks_exact(row_bytes).any(|row| {
            row[..96 * 4]
                .chunks_exact(4)
                .any(|pixel| pixel[..3].iter().any(|channel| *channel != 0))
        }));
        if device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            let timings = tiled
                .read_phase_timings_blocking(&device, &queue)
                .expect("phase timestamp readback")
                .expect("timestamp query resources");
            eprintln!("resident tiled test phase timings: {timings:?}");
            for phase_ms in [
                timings.project_ms,
                timings.count_scan_ms,
                timings.scatter_tile_scan_ms,
                timings.tile_radix_ms,
                timings.raster_ms,
                timings.blit_ms,
            ]
            .into_iter()
            .flatten()
            {
                assert!(phase_ms.is_finite() && phase_ms >= 0.0);
            }
        }
    }
}
